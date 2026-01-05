# Milestone 1.9: Safepoint Infrastructure

**Phase:** 1 - VM Core
**Crate:** `raya-core`
**Status:** ✅ Complete
**Prerequisites:**
- Milestone 1.7 (Garbage Collection) ✅
- Milestone 1.5 (Basic Bytecode Interpreter) ✅

---

## Table of Contents

1. [Overview](#overview)
2. [Goals](#goals)
3. [Design Philosophy](#design-philosophy)
4. [Tasks](#tasks)
5. [Implementation Details](#implementation-details)
6. [Testing Requirements](#testing-requirements)
7. [Success Criteria](#success-criteria)
8. [References](#references)

---

## Overview

Implement the **Safepoint Infrastructure** to enable coordinated stop-the-world (STW) pauses across all executing tasks. This infrastructure is critical for:

- **Garbage Collection:** Safe heap scanning when all tasks are paused
- **VM Snapshotting:** Consistent state capture for pause/resume functionality
- **Debugging:** Breakpoint support and stack inspection

**Key Architectural Decisions:**

- **Cooperative safepoints** - Tasks poll at specific locations (not preemptive)
- **Minimal overhead** - Inlined poll checks with atomic flag reads
- **Barrier synchronization** - All tasks must reach safepoint before STW operation proceeds
- **Reason tracking** - Different pause types (GC, snapshot, debug) for monitoring

**Key Deliverable:** A robust safepoint coordination system that enables safe STW pauses with minimal runtime overhead.

---

## Goals

### Primary Goals

- [ ] Implement `SafepointCoordinator` for managing STW pauses
- [ ] Add lightweight safepoint polling mechanism
- [ ] Create STW pause protocol (request, wait, resume)
- [ ] Insert safepoints at strategic bytecode locations
- [ ] Integrate with interpreter execution loop
- [ ] Support multiple pause reasons (GC, snapshot, debug)
- [ ] Add safepoint statistics and monitoring
- [ ] Test coverage >85%

### Secondary Goals

- Add configurable safepoint timeout handling
- Implement safepoint bias detection (prevent starvation)
- Add performance profiling for safepoint overhead
- Create debugging utilities for safepoint analysis

### Non-Goals (Deferred)

- Preemptive safepoints (cooperative only for now)
- Per-thread safepoint customization
- Safepoint elision optimization

---

## Design Philosophy

### Why Cooperative Safepoints?

**Cooperative over Preemptive:**
- Predictable pause points - tasks reach safepoint at known safe states
- Simpler implementation - no signal handlers or OS thread suspension
- Lower overhead - explicit polling at limited locations
- Better portability - works consistently across platforms

**Strategic Placement:**
- **Function calls** - Natural pause points with clean stack frames
- **Loop back-edges** - Prevents long-running tight loops from blocking pauses
- **Allocations** - Already on slow path, natural place for GC coordination
- **Await points** - Task suspension is already a safe state

**Performance Considerations:**
- Inlined poll checks compile to ~2-3 instructions
- Atomic flag reads are cheap on modern CPUs (cached coherently)
- Only enter safepoint handler when pause is actually pending
- No overhead when no STW operation is active

### Barrier Synchronization

All tasks must reach the safepoint before STW operation proceeds:

```
Request STW → Atomic flag set → Tasks poll → Barrier wait → STW operation → Resume
```

This ensures:
- Consistent heap state for GC scanning
- Complete VM state for snapshotting
- No tasks mutating memory during critical operations

---

## Tasks

### Task 1: SafepointCoordinator Structure

**File:** `crates/raya-core/src/vm/safepoint.rs`

**Checklist:**

- [ ] Define `SafepointCoordinator` struct
- [ ] Add atomic flags for pending pause reasons
- [ ] Implement worker count tracking
- [ ] Create barrier for synchronization
- [ ] Add pause reason enum
- [ ] Implement statistics tracking

**Implementation:**

```rust
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};

/// Reasons for requesting a safepoint pause
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// Garbage collection
    GarbageCollection,
    /// VM state snapshotting
    Snapshot,
    /// Debugger breakpoint
    Debug,
}

/// Coordinates stop-the-world pauses across all worker threads
pub struct SafepointCoordinator {
    /// Number of active worker threads
    worker_count: AtomicUsize,

    /// Workers currently at safepoint
    workers_at_safepoint: AtomicUsize,

    /// GC pause is pending
    gc_pending: AtomicBool,

    /// Snapshot pause is pending
    snapshot_pending: AtomicBool,

    /// Debug pause is pending
    debug_pending: AtomicBool,

    /// Current pause reason
    current_reason: Mutex<Option<StopReason>>,

    /// Barrier for synchronizing workers
    barrier: Arc<Barrier>,

    /// Statistics
    stats: SafepointStats,
}

#[derive(Debug, Default)]
struct SafepointStats {
    /// Total number of safepoints executed
    total_safepoints: AtomicUsize,

    /// Total time spent at safepoints (microseconds)
    total_pause_time_us: AtomicUsize,

    /// Maximum pause time (microseconds)
    max_pause_time_us: AtomicUsize,
}
```

**Tests:**
- Create SafepointCoordinator with various worker counts
- Verify atomic flag initialization
- Test statistics reset

---

### Task 2: Safepoint Poll Mechanism

**File:** `crates/raya-core/src/vm/safepoint.rs`

**Checklist:**

- [ ] Implement fast-path poll check
- [ ] Create slow-path safepoint handler
- [ ] Add barrier waiting logic
- [ ] Implement resume notification
- [ ] Add timeout handling
- [ ] Track poll frequency

**Implementation:**

```rust
impl SafepointCoordinator {
    /// Fast inline check - called frequently from interpreter
    #[inline(always)]
    pub fn poll(&self) {
        // Fast path: check if any pause is pending (single atomic load)
        if self.is_pause_pending_fast() {
            // Slow path: enter safepoint handler
            self.enter_safepoint();
        }
    }

    /// Fast check for pending pauses (inlines to ~2 instructions)
    #[inline(always)]
    fn is_pause_pending_fast(&self) -> bool {
        self.gc_pending.load(Ordering::Acquire) ||
        self.snapshot_pending.load(Ordering::Acquire) ||
        self.debug_pending.load(Ordering::Acquire)
    }

    /// Slow path: handle safepoint entry
    #[cold]
    #[inline(never)]
    fn enter_safepoint(&self) {
        let start = std::time::Instant::now();

        // Increment workers at safepoint
        let count = self.workers_at_safepoint.fetch_add(1, Ordering::AcqRel);

        // Last worker to arrive triggers the STW operation
        if count + 1 == self.worker_count.load(Ordering::Acquire) {
            // All workers are paused - safe to proceed
            self.execute_stw_operation();
        }

        // Wait at barrier for all workers
        self.barrier.wait();

        // Decrement workers at safepoint
        self.workers_at_safepoint.fetch_sub(1, Ordering::AcqRel);

        // Track statistics
        let elapsed = start.elapsed().as_micros() as usize;
        self.stats.total_pause_time_us.fetch_add(elapsed, Ordering::Relaxed);
        self.stats.total_safepoints.fetch_add(1, Ordering::Relaxed);

        // Update max pause time
        let mut max = self.stats.max_pause_time_us.load(Ordering::Relaxed);
        while elapsed > max {
            match self.stats.max_pause_time_us.compare_exchange_weak(
                max,
                elapsed,
                Ordering::Relaxed,
                Ordering::Relaxed
            ) {
                Ok(_) => break,
                Err(current) => max = current,
            }
        }
    }

    /// Execute the STW operation (called by last worker at safepoint)
    fn execute_stw_operation(&self) {
        let reason = self.current_reason.lock().unwrap();

        match *reason {
            Some(StopReason::GarbageCollection) => {
                // GC will be triggered externally
            }
            Some(StopReason::Snapshot) => {
                // Snapshot will be captured externally
            }
            Some(StopReason::Debug) => {
                // Debugger will inspect state externally
            }
            None => {
                // Should not happen
                log::warn!("Safepoint reached with no reason set");
            }
        }
    }
}
```

**Tests:**
- Poll with no pending pauses (fast path)
- Poll with pending GC pause
- Multiple workers reaching safepoint
- Barrier synchronization
- Statistics tracking

---

### Task 3: STW Pause Request & Resume

**File:** `crates/raya-core/src/vm/safepoint.rs`

**Checklist:**

- [ ] Implement pause request for each reason
- [ ] Add blocking wait for all workers
- [ ] Create resume protocol
- [ ] Handle concurrent pause requests
- [ ] Add timeout handling
- [ ] Implement pause priority

**Implementation:**

```rust
impl SafepointCoordinator {
    /// Request a stop-the-world pause
    pub fn request_stw_pause(&self, reason: StopReason) {
        // Set the current reason
        {
            let mut current = self.current_reason.lock().unwrap();
            if current.is_some() {
                panic!("Cannot request STW pause while another is active");
            }
            *current = Some(reason);
        }

        // Set the appropriate atomic flag
        match reason {
            StopReason::GarbageCollection => {
                self.gc_pending.store(true, Ordering::Release);
            }
            StopReason::Snapshot => {
                self.snapshot_pending.store(true, Ordering::Release);
            }
            StopReason::Debug => {
                self.debug_pending.store(true, Ordering::Release);
            }
        }

        // Wait for all workers to reach safepoint
        self.wait_for_all_workers();
    }

    /// Wait for all workers to reach safepoint
    fn wait_for_all_workers(&self) {
        let expected = self.worker_count.load(Ordering::Acquire);

        // Spin-wait with exponential backoff
        let mut backoff = 1;
        loop {
            let at_safepoint = self.workers_at_safepoint.load(Ordering::Acquire);

            if at_safepoint == expected {
                break;
            }

            // Exponential backoff
            for _ in 0..backoff {
                std::hint::spin_loop();
            }

            backoff = (backoff * 2).min(1000);
        }
    }

    /// Resume from STW pause
    pub fn resume_from_pause(&self) {
        // Clear the atomic flags
        self.gc_pending.store(false, Ordering::Release);
        self.snapshot_pending.store(false, Ordering::Release);
        self.debug_pending.store(false, Ordering::Release);

        // Clear the current reason
        {
            let mut current = self.current_reason.lock().unwrap();
            *current = None;
        }

        // Workers will resume after barrier
    }
}
```

**Tests:**
- Request GC pause and verify all workers stop
- Request snapshot pause
- Resume from pause and verify workers continue
- Handle timeout scenarios
- Test pause reason tracking

---

### Task 4: Interpreter Integration

**File:** `crates/raya-core/src/vm/interpreter.rs`

**Checklist:**

- [ ] Add safepoint coordinator to VM
- [ ] Insert polls at function calls
- [ ] Insert polls at loop back-edges
- [ ] Insert polls at allocation sites
- [ ] Insert polls at await points
- [ ] Add safepoint statistics reporting

**Implementation:**

```rust
// In interpreter.rs

impl Vm {
    pub fn execute(&mut self, module: &Module) -> VmResult<Value> {
        let code = &module.functions[0].code;
        let mut ip = 0;

        // Push initial frame
        self.stack.push_frame(0, 0, module.functions[0].local_count, 0)?;

        loop {
            // Safepoint poll at loop back-edge
            self.safepoint.poll();

            if ip >= code.len() {
                return Err(VmError::RuntimeError("Instruction pointer out of bounds".to_string()));
            }

            let opcode = Opcode::from_u8(code[ip])
                .ok_or_else(|| VmError::RuntimeError(format!("Invalid opcode: {}", code[ip])))?;
            ip += 1;

            match opcode {
                // Function call - insert safepoint
                Opcode::Call => {
                    self.safepoint.poll();
                    self.op_call(&mut ip, code)?;
                }

                // Allocation - insert safepoint
                Opcode::New => {
                    self.safepoint.poll();
                    self.op_new(&mut ip, code)?;
                }

                Opcode::NewArray => {
                    self.safepoint.poll();
                    self.op_new_array(&mut ip, code)?;
                }

                // Await point - insert safepoint
                Opcode::Await => {
                    self.safepoint.poll();
                    self.op_await(&mut ip, code)?;
                }

                // Jump back (loop back-edge) - insert safepoint
                Opcode::Jmp => {
                    let offset = self.read_i16(code, &mut ip)?;
                    if offset < 0 {
                        // Backward jump - loop back-edge
                        self.safepoint.poll();
                    }
                    ip = (ip as isize + offset as isize) as usize;
                }

                // ... other opcodes
            }
        }
    }
}
```

**Tests:**
- Safepoint polls at function calls
- Safepoint polls at loop back-edges
- Safepoint polls at allocations
- Verify correct pause behavior during execution

---

### Task 5: Worker Thread Registration

**File:** `crates/raya-core/src/vm/safepoint.rs`

**Checklist:**

- [ ] Implement worker registration
- [ ] Implement worker deregistration
- [ ] Update worker count atomically
- [ ] Rebuild barrier on count change
- [ ] Handle dynamic worker addition/removal

**Implementation:**

```rust
impl SafepointCoordinator {
    /// Register a new worker thread
    pub fn register_worker(&self) {
        let count = self.worker_count.fetch_add(1, Ordering::AcqRel) + 1;

        // Rebuild barrier with new count
        // Note: This is safe only when no STW operation is active
        // In practice, workers are registered during initialization
    }

    /// Deregister a worker thread
    pub fn deregister_worker(&self) {
        let count = self.worker_count.fetch_sub(1, Ordering::AcqRel) - 1;

        if count == 0 {
            // No more workers - cleanup
        }
    }

    /// Get current worker count
    pub fn worker_count(&self) -> usize {
        self.worker_count.load(Ordering::Acquire)
    }
}
```

**Tests:**
- Register multiple workers
- Deregister workers
- Verify count tracking
- Handle edge cases (0 workers)

---

## Implementation Details

### Safepoint Overhead Analysis

**Fast Path:**
- 1 atomic load (gc_pending)
- 1 atomic load (snapshot_pending)
- 2-3 comparison instructions
- 1 conditional branch (almost always not taken)

Total: ~5-6 CPU cycles on modern hardware

**Frequency:**
- Function calls: ~1-10 per millisecond (depending on workload)
- Loop back-edges: Variable (tight loops more frequent)
- Allocations: ~10-1000 per millisecond
- Await points: ~1-10 per millisecond

**Total Overhead:**
- Typical workload: <0.1% CPU time
- Allocation-heavy: <0.5% CPU time
- Tight loop-heavy: <1% CPU time

### Memory Ordering

**Acquire/Release Semantics:**
- `request_stw_pause` uses Release on flag store
- `poll` uses Acquire on flag load
- Ensures happens-before relationship between pause request and worker observation

**Relaxed Operations:**
- Statistics counters use Relaxed ordering
- Worker count reads can be Relaxed in most cases

### Platform Considerations

**Linux/macOS/Windows:**
- Atomic operations map to native CPU instructions
- `std::hint::spin_loop()` maps to `pause` (x86) or `yield` (ARM)

**Portability:**
- No OS-specific features required
- Standard library atomics work everywhere
- No signal handlers or thread suspension needed

---

## Testing Requirements

### Unit Tests (Minimum 20 tests)

**SafepointCoordinator Tests:**
1. Create coordinator with 1 worker
2. Create coordinator with multiple workers
3. Poll with no pending pause
4. Request GC pause
5. Request snapshot pause
6. Request debug pause
7. Multiple workers reach safepoint
8. Barrier synchronization
9. Resume from pause
10. Statistics tracking (total pauses)
11. Statistics tracking (pause time)
12. Statistics tracking (max pause time)
13. Worker registration
14. Worker deregistration
15. Concurrent pause requests (error handling)
16. Timeout handling
17. Zero workers edge case
18. Worker count tracking
19. Pause reason tracking
20. Fast path optimization (no branches when no pause)

### Integration Tests (10 tests)

**File:** `crates/raya-core/tests/safepoint_integration.rs`

1. **Interpreter with safepoint polls**
   - Execute bytecode with frequent safepoints
   - Request GC pause during execution
   - Verify execution pauses and resumes

2. **Multi-threaded safepoint coordination**
   - Spawn multiple worker threads
   - Each executes interpreter loop
   - Request STW pause
   - Verify all workers stop

3. **Loop back-edge safepoints**
   - Execute tight loop bytecode
   - Request pause during loop
   - Verify loop can be interrupted

4. **Allocation safepoints**
   - Allocate many objects
   - Request pause during allocation
   - Verify GC can run

5. **Nested pause requests**
   - Verify only one pause can be active
   - Test error handling for concurrent requests

6. **Statistics verification**
   - Execute with multiple pauses
   - Verify pause count
   - Verify pause time tracking

7. **Worker registration during execution**
   - Start with N workers
   - Add worker dynamically
   - Verify count updates

8. **Stress test: many pauses**
   - Execute long-running workload
   - Request pauses frequently
   - Verify correctness and performance

9. **Pause during function call**
   - Execute recursive function calls
   - Request pause during call
   - Verify stack integrity

10. **Resume correctness**
    - Pause during execution
    - Resume and continue
    - Verify state preserved

### Performance Tests

**Metrics to track:**
- Safepoint poll overhead (<0.5% CPU)
- STW pause latency (<1ms for 100 workers)
- Resume latency (<100μs)
- Memory overhead (constant, ~256 bytes)

---

## Success Criteria

### Must Have

- [ ] SafepointCoordinator fully implemented
- [ ] Safepoint polls at all required locations
- [ ] STW pause protocol works correctly
- [ ] Barrier synchronization verified
- [ ] Statistics tracking functional
- [ ] All unit tests pass (20+ tests)
- [ ] All integration tests pass (10+ tests)
- [ ] Test coverage >85%
- [ ] Documentation complete
- [ ] No race conditions in pause/resume

### Nice to Have

- Safepoint overhead <0.1% in typical workloads
- STW pause latency <500μs for up to 100 workers
- Configurable timeout handling
- Advanced debugging utilities
- Safepoint bias detection

### Performance Targets

- **Fast path overhead:** <5 CPU cycles per poll
- **Pause latency:** <1ms for 100 workers
- **Resume latency:** <100μs
- **Memory overhead:** <1KB per coordinator

---

## References

### Design Documents

- [ARCHITECTURE.md](../design/ARCHITECTURE.md) - Section 5.6: Safepoint Protocol
- [SNAPSHOTTING.md](../design/SNAPSHOTTING.md) - Section 2: Safepoint-Based Suspension
- [INNER_VM.md](../design/INNER_VM.md) - Section 5: Fair Scheduling (safepoints)

### Related Milestones

- Milestone 1.5: Bytecode Interpreter (insertion points)
- Milestone 1.7: Garbage Collection (STW pauses for GC)
- Milestone 1.10: Task Scheduler (multi-threaded coordination)
- Milestone 1.11: VM Snapshotting (STW pauses for snapshot)

### External References

- JVM Safepoint Implementation
- Go Runtime Preemption
- Rust Crossbeam Barriers
- Linux Futex-based Barriers

---

## Dependencies

**Crate Dependencies:**
```toml
[dependencies]
crossbeam = "0.8"      # For barrier primitives (optional)
parking_lot = "0.12"   # For Mutex if needed
```

**Internal Dependencies:**
- `raya-core::vm::Vm` - Interpreter loop integration
- `raya-core::gc::GarbageCollector` - GC pause coordination
- `raya-core::scheduler::Scheduler` - Task coordination (future)

---

## Implementation Notes

### Phase 1: Foundation (This Milestone)
- Basic SafepointCoordinator
- Single-threaded interpreter integration
- Simple STW pause protocol

### Phase 2: Multi-Threading (Milestone 1.10)
- Worker thread pool integration
- Per-worker safepoint tracking
- Work-stealing coordination

### Phase 3: Optimization (Future)
- Safepoint elision in hot loops
- Adaptive polling frequency
- Safepoint bias detection

---

## Open Questions

1. **Q:** Should we support nested STW pauses (e.g., GC during snapshot)?
   **A:** No - reject concurrent pause requests for simplicity.

2. **Q:** What timeout should we use for waiting for workers?
   **A:** Start with no timeout (infinite wait), add configurable timeout later.

3. **Q:** Should we track per-worker safepoint statistics?
   **A:** Not initially - global statistics sufficient.

4. **Q:** How do we handle workers blocked on I/O?
   **A:** Deferred - requires async I/O integration (future milestone).

---

**Status Legend:**
- 🔄 In Progress
- ✅ Complete
- ⏸️ Blocked
- 📝 Planned
