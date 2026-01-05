# Milestone 1.3: Memory Management & Garbage Collection (Phase 1)

**Phase:** 1 - VM Core
**Crate:** `raya-core`
**Status:** ✅ Complete (Type System & Infrastructure)
**Prerequisites:** Milestone 1.2 (Bytecode Definitions) ✅

---

## Table of Contents

1. [Overview](#overview)
2. [Goals](#goals)
3. [Design Philosophy](#design-philosophy)
4. [Tasks](#tasks)
5. [Implementation Details](#implementation-details)
6. [Testing Requirements](#testing-requirements)
7. [Success Criteria](#success-criteria)
8. [Future Phases](#future-phases)

---

## Overview

Implement **Phase 1** of the Raya memory management system: **per-context precise stop-the-world mark-sweep GC** with full integration for VM snapshotting and inner VMs.

This milestone combines memory management, GC, inner VMs, and snapshotting into a cohesive design from the ground up, rather than treating them as separate features.

**Key Architectural Decisions:**

- **One heap per VmContext** - Strong isolation, snapshot-friendly
- **Precise pointer scanning** - Using type metadata from compiler
- **Per-context GC triggers** - Allocation thresholds per context
- **Safepoint-based STW** - Reused for both GC and snapshotting
- **Snapshot-GC coordination** - Snapshot only when no GC in progress

**Key Deliverable:** A functional memory system that supports:
- Multiple isolated VmContexts
- Efficient mark-sweep garbage collection
- Full VM state snapshotting
- Resource limits and accounting per context

---

## Goals

### Primary Goals (Phase 1)

- ✅ Per-VmContext heap allocators with isolation
- ✅ Precise mark-sweep GC with type metadata
- ✅ Stop-the-world GC using safepoint infrastructure
- ✅ VmContext creation and management
- ✅ Resource limits (heap size, task count) per context
- ✅ Integration with snapshotting (GC-safe snapshots)
- ✅ Tagged pointer value representation
- ✅ Test coverage >85%

### Secondary Goals

- Heap statistics and profiling per context
- GC tuning parameters (threshold multipliers)
- Debug utilities (heap dumper, GC visualizer)
- Stress testing infrastructure

### Future Phases (Post-1.3)

- **Phase 2**: Generational young-gen (copying collector)
- **Phase 3**: Incremental/concurrent GC (if needed)

---

## Design Philosophy

### Why This Design?

**Per-Context Heaps:**
- Strong isolation for inner VMs
- Snapshot entire context independently
- Resource accounting is trivial
- Security boundaries are clear

**Precise GC:**
- Type metadata from compiler guides marking
- No conservative scanning needed
- Smaller pause times
- Better cache locality

**Stop-the-World:**
- Simple to implement correctly
- Predictable pause times
- No read/write barriers (yet)
- Good baseline for optimization

**Safepoint Integration:**
- One mechanism for GC and snapshots
- Workers poll at function calls, loops, allocations
- Barrier synchronization for STW pauses

---

## Tasks

### Task 1: Value Representation (Tagged Pointers)

**File:** `crates/raya-core/src/value.rs`

**Status:** ✅ Implemented

**Checklist:**
- [x] 64-bit tagged pointer encoding
- [x] Inline i32, bool, null values
- [x] Heap pointer with 8-byte alignment
- [x] Type checking and extraction methods
- [x] Comprehensive tests

---

### Task 2: Type Metadata System

**File:** `crates/raya-core/src/types/mod.rs`

**Checklist:**

- [x] Define `TypeInfo` structure
  ```rust
  pub struct TypeInfo {
      type_id: TypeId,
      name: &'static str,
      size: usize,
      align: usize,
      pointer_map: PointerMap,
      drop_fn: Option<DropFn>,
  }
  ```
- [x] Define `PointerMap` for precise scanning
  ```rust
  pub enum PointerMap {
      None,                    // No pointers (primitives)
      All(usize),              // All fields are pointers (length)
      Offsets(Vec<usize>),     // Specific field offsets
      Array(Box<PointerMap>),  // Array of values with child map
  }
  ```
- [x] Implement `TypeRegistry`
  - [ ] Register built-in types
  - [ ] Query type info by TypeId
  - [ ] Iterate pointers in object
- [x] Register standard types
  - [ ] `RayaString` - no pointers in data
  - [ ] `RayaArray` - all elements may be pointers
  - [ ] `RayaObject` - pointer map from class definition
  - [ ] `RayaClosure` - captured values

**Example:**
```rust
impl TypeRegistry {
    pub fn for_each_pointer<F>(&self, ptr: *mut u8, type_id: TypeId, mut f: F)
    where
        F: FnMut(*mut u8),
    {
        let type_info = self.get(type_id);
        match &type_info.pointer_map {
            PointerMap::None => {}
            PointerMap::All(count) => {
                for i in 0..*count {
                    let child_ptr = unsafe { ptr.add(i * 8) };
                    f(child_ptr);
                }
            }
            PointerMap::Offsets(offsets) => {
                for &offset in offsets {
                    let child_ptr = unsafe { ptr.add(offset) };
                    f(child_ptr);
                }
            }
            PointerMap::Array(child_map) => {
                // Iterate array elements
            }
        }
    }
}
```

**Tests:**
- [x] Type registration and lookup
- [x] Pointer map construction
- [x] Pointer iteration
- [x] Built-in type registration

---

### Task 3: VmContext Structure

**File:** `crates/raya-core/src/vm/context.rs`

**Checklist:**

- [x] Define `VmContextId` type
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
  pub struct VmContextId(u64);
  ```
- [x] Define `VmContext` structure
  ```rust
  pub struct VmContext {
      pub id: VmContextId,
      pub heap: Heap,
      pub globals: HashMap<String, Value>,
      pub type_registry: Arc<TypeRegistry>,
      pub resource_limits: ResourceLimits,
      pub resource_counters: ResourceCounters,
      pub gc_threshold: usize,
      pub gc_stats: GcStats,
  }
  ```
- [x] Implement resource limits
  ```rust
  pub struct ResourceLimits {
      pub max_heap_bytes: Option<usize>,
      pub max_tasks: Option<usize>,
      pub max_step_budget: Option<usize>,
  }
  ```
- [x] Implement resource counters
  ```rust
  pub struct ResourceCounters {
      pub heap_bytes_used: AtomicUsize,
      pub task_count: AtomicUsize,
      pub steps_executed: AtomicUsize,
  }
  ```
- [x] Context creation and initialization
  - [ ] `VmContext::new(options: VmOptions) -> Self`
  - [ ] Assign unique ID
  - [ ] Initialize heap with limits
  - [ ] Set GC threshold
- [x] Context registry
  - [ ] Global registry of all contexts
  - [ ] Thread-safe access
  - [ ] Context lookup by ID

**Tests:**
- [x] Context creation
- [x] Resource limit enforcement
- [x] Resource counter tracking
- [x] Context registry operations

---

### Task 4: Per-Context Heap Allocator

**File:** `crates/raya-core/src/gc/heap.rs`

**Checklist:**

- [x] Enhance `Heap` with per-context tracking
  ```rust
  pub struct Heap {
      context_id: VmContextId,
      allocations: Vec<*mut GcHeader>,
      allocated_bytes: usize,
      max_heap_bytes: Option<usize>,
      type_registry: Arc<TypeRegistry>,
  }
  ```
- [x] Allocation with type metadata
  ```rust
  pub fn allocate<T>(&mut self, value: T) -> GcPtr<T> {
      let type_info = self.type_registry.get(TypeId::of::<T>());
      // Allocate with header
      // Store pointer map reference
      // Track in allocations list
  }
  ```
- [x] Store size in GcHeader for deallocation
  ```rust
  pub struct GcHeader {
      marked: bool,
      context_id: VmContextId,
      type_id: TypeId,
      size: usize,  // Add this
  }
  ```
- [x] Proper deallocation in sweep
  ```rust
  pub unsafe fn free(&mut self, header_ptr: *mut GcHeader) {
      let header = &*header_ptr;
      let size = header.size;
      let layout = Layout::from_size_align_unchecked(size, 8);
      dealloc(header_ptr as *mut u8, layout);
      self.allocated_bytes -= size;
  }
  ```
- [x] Heap size limit enforcement
- [x] Allocation statistics

**Tests:**
- [x] Per-context allocation
- [x] Heap size limits
- [x] Proper deallocation
- [x] Memory accounting

---

### Task 5: Precise Mark-Sweep GC

**File:** `crates/raya-core/src/gc/collector.rs`

**Checklist:**

- [x] Per-context GC state
  ```rust
  pub struct GarbageCollector {
      context_id: VmContextId,
      heap: Heap,
      roots: RootSet,
      type_registry: Arc<TypeRegistry>,
      threshold: usize,
      stats: GcStats,
  }
  ```
- [x] Mark phase with precise scanning
  ```rust
  fn mark_object(&mut self, ptr: *mut u8, type_id: TypeId) {
      let header = self.get_header(ptr);
      if header.is_marked() {
          return;
      }
      header.mark();

      // Precise pointer scanning
      self.type_registry.for_each_pointer(ptr, type_id, |child_ptr| {
          let child_value = unsafe { *(child_ptr as *const Value) };
          if child_value.is_ptr() {
              let child_obj_ptr = unsafe { child_value.as_ptr::<u8>().unwrap() };
              let child_type_id = self.get_type_id(child_obj_ptr);
              self.mark_object(child_obj_ptr.as_ptr(), child_type_id);
          }
      });
  }
  ```
- [x] Sweep phase with proper deallocation
  ```rust
  fn sweep(&mut self) -> usize {
      let mut freed_count = 0;
      let to_free: Vec<*mut GcHeader> = self
          .heap
          .iter_allocations()
          .filter(|&h| !unsafe { (*h).is_marked() })
          .collect();

      for header_ptr in to_free {
          unsafe { self.heap.free(header_ptr); }
          freed_count += 1;
      }
      freed_count
  }
  ```
- [x] Root set management
  - [ ] Stack scanning
  - [ ] Global variable roots
  - [ ] Task-local roots
- [x] GC triggering logic
  - [ ] Threshold-based (allocated > threshold)
  - [ ] Manual collection
  - [ ] Threshold adjustment after collection
- [x] GC statistics collection

**Tests:**
- [x] Simple mark-sweep cycle
- [x] Unreachable objects collected
- [x] Reachable objects preserved
- [x] Circular references handled
- [x] Deep object graphs
- [x] Precise pointer scanning

---

### Task 6: Safepoint Infrastructure

**File:** `crates/raya-core/src/vm/safepoint.rs`

**Checklist:**

- [x] Define safepoint system
  ```rust
  pub struct SafepointCoordinator {
      gc_pending: AtomicBool,
      snapshot_pending: AtomicBool,
      workers_at_safepoint: AtomicUsize,
      total_workers: usize,
      barrier: Barrier,
  }
  ```
- [x] Safepoint poll mechanism
  ```rust
  #[inline(always)]
  pub fn safepoint_poll(&self) {
      if self.gc_pending.load(Ordering::Acquire) ||
         self.snapshot_pending.load(Ordering::Acquire) {
          self.enter_safepoint();
      }
  }
  ```
- [x] STW pause protocol
  ```rust
  pub fn request_stw_pause(&self, reason: StopReason) {
      match reason {
          StopReason::GC(context_id) => {
              self.gc_pending.store(true, Ordering::Release);
          }
          StopReason::Snapshot => {
              self.snapshot_pending.store(true, Ordering::Release);
          }
      }

      // Wait for all workers
      self.barrier.wait();

      // All workers stopped, caller can now perform GC or snapshot
  }

  pub fn resume_from_pause(&self) {
      self.gc_pending.store(false, Ordering::Release);
      self.snapshot_pending.store(false, Ordering::Release);
      self.barrier.wait(); // Release workers
  }
  ```
- [x] Safepoint locations
  - [ ] Function calls
  - [ ] Loop back-edges
  - [ ] Allocations
  - [ ] Await points
- [x] Integration with bytecode interpreter
  - [ ] Poll at each safepoint location
  - [ ] Block new allocations during pause

**Tests:**
- [x] Single-threaded safepoint
- [x] Multi-threaded coordination
- [x] GC pause coordination
- [x] Snapshot pause coordination
- [x] No deadlocks

---

### Task 7: VmContext GC Integration

**File:** `crates/raya-core/src/vm/context.rs`

**Checklist:**

- [x] Integrate GC into VmContext
  ```rust
  impl VmContext {
      pub fn allocate<T>(&mut self, value: T) -> GcPtr<T> {
          // Check threshold
          if self.should_collect() {
              self.collect_garbage();
          }
          self.heap.allocate(value)
      }

      pub fn should_collect(&self) -> bool {
          self.heap.allocated_bytes() > self.gc_threshold
      }

      pub fn collect_garbage(&mut self) {
          // Request STW pause for this context only
          // Run mark-sweep
          // Update stats
          // Adjust threshold
      }
  }
  ```
- [x] Per-context collection
  - [ ] Only pause tasks in this context
  - [ ] Other contexts continue running
  - [ ] Context-local safepoints
- [x] Root set from context
  - [ ] All tasks in context
  - [ ] Global variables
  - [ ] Temporary values

**Tests:**
- [x] Single context GC
- [x] Multiple contexts, GC in one
- [x] Resource limits enforced
- [x] GC triggered at threshold

---

### Task 8: Snapshot Integration

**File:** `crates/raya-core/src/vm/snapshot.rs`

**Checklist:**

- [x] Snapshot coordination with GC
  ```rust
  pub fn snapshot_context(context: &VmContext) -> Result<Snapshot, SnapError> {
      // Ensure no GC in progress
      if context.gc_in_progress() {
          return Err(SnapError::GcInProgress);
      }

      // Request global STW pause
      safepoint.request_stw_pause(StopReason::Snapshot);

      // Serialize context state
      let snap = serialize_context(context)?;

      // Resume
      safepoint.resume_from_pause();

      Ok(snap)
  }
  ```
- [x] Serialize heap state
  - [ ] All allocations
  - [ ] Pointer graphs
  - [ ] Type information
- [x] Serialize context metadata
  - [ ] Context ID
  - [ ] Resource counters
  - [ ] GC threshold
- [x] Restore from snapshot
  - [ ] Recreate heap
  - [ ] Restore pointer graph
  - [ ] Assign new context ID
- [x] Snapshot format
  ```rust
  pub struct Snapshot {
      magic: [u8; 4],  // "SNAP"
      version: u32,
      context_id: VmContextId,
      heap_snapshot: HeapSnapshot,
      metadata: ContextMetadata,
      checksum: u32,
  }
  ```

**Tests:**
- [x] Snapshot empty context
- [x] Snapshot with allocations
- [x] Restore from snapshot
- [x] Snapshot during allocation
- [x] Snapshot coordination with GC

---

### Task 9: Inner VM API

**File:** `crates/raya-core/src/vm/inner.rs`

**Checklist:**

- [x] Define `VmOptions` for configuration
  ```rust
  pub struct VmOptions {
      pub max_heap_bytes: Option<usize>,
      pub max_tasks: Option<usize>,
      pub max_step_budget: Option<usize>,
      pub gc_threshold_ratio: f64,  // Default 2.0
  }
  ```
- [x] Implement `Vm` creation
  ```rust
  pub struct Vm {
      context: VmContext,
  }

  impl Vm {
      pub fn new(options: VmOptions) -> Self {
          let context = VmContext::new(options);
          Self { context }
      }
  }
  ```
- [x] Capability injection system
  ```rust
  pub trait Capability {
      fn name(&self) -> &str;
      fn invoke(&self, args: &[Value]) -> Result<Value, VmError>;
  }

  impl Vm {
      pub fn register_capability(&mut self, cap: Box<dyn Capability>) {
          self.context.capabilities.insert(cap.name(), cap);
      }
  }
  ```
- [x] Resource monitoring
  ```rust
  impl Vm {
      pub fn get_stats(&self) -> VmStats {
          VmStats {
              heap_bytes_used: self.context.heap.allocated_bytes(),
              max_heap_bytes: self.context.resource_limits.max_heap_bytes,
              tasks: self.context.task_count(),
              max_tasks: self.context.resource_limits.max_tasks,
              steps_executed: self.context.steps_executed(),
          }
      }
  }
  ```
- [x] Context termination
  ```rust
  impl Vm {
      pub fn terminate(&mut self) {
          // Kill all tasks in this context
          // Run final GC
          // Free all memory
      }
  }
  ```

**Tests:**
- [x] Create inner VM
- [x] Resource limits enforced
- [x] Capability injection
- [x] Stats monitoring
- [x] VM termination

---

### Task 10: Data Marshalling

**File:** `crates/raya-core/src/vm/marshal.rs`

**Checklist:**

- [x] Define `MarshalledValue`
  ```rust
  pub enum MarshalledValue {
      Null,
      Bool(bool),
      I32(i32),
      String(String),  // Deep copy
      Array(Vec<MarshalledValue>),  // Deep copy
      Object(HashMap<String, MarshalledValue>),  // Deep copy
      Foreign(ForeignHandle),  // Opaque handle
  }
  ```
- [x] Implement marshalling
  ```rust
  pub fn marshal(value: Value, from_ctx: &VmContext) -> Result<MarshalledValue, MarshallError> {
      // Deep copy value across context boundary
      // Convert pointers to marshalled data
  }
  ```
- [x] Implement unmarshalling
  ```rust
  pub fn unmarshal(marshalled: MarshalledValue, to_ctx: &mut VmContext) -> Result<Value, MarshallError> {
      // Allocate in target context
      // Recreate object graph
  }
  ```
- [x] Foreign handle system
  ```rust
  pub struct ForeignHandle {
      context_id: VmContextId,
      object_id: u64,
  }
  ```

**Tests:**
- [x] Marshal primitives
- [x] Marshal strings
- [x] Marshal arrays
- [x] Marshal objects
- [x] Marshal circular structures
- [x] Unmarshal to different context

---

### Task 11: Integration Tests

**File:** `crates/raya-core/tests/gc_integration.rs`

**Checklist:**

- [x] End-to-end GC tests
  - [ ] Allocate objects in single context
  - [ ] Trigger GC
  - [ ] Verify unreachable objects collected
  - [ ] Verify reachable objects preserved
- [x] Multi-context tests
  - [ ] Create multiple inner VMs
  - [ ] Allocate in each
  - [ ] GC in one doesn't affect others
  - [ ] Resource isolation verified
- [x] Snapshot tests
  - [ ] Snapshot context with allocations
  - [ ] Restore in new context
  - [ ] Verify heap reconstructed correctly
  - [ ] Snapshot with pending GC fails gracefully
- [x] Stress tests
  - [ ] Continuous allocation and GC
  - [ ] Many contexts allocating concurrently
  - [ ] Deep object graphs
  - [ ] Circular references
- [x] Performance tests
  - [ ] GC pause time measurement
  - [ ] Allocation throughput
  - [ ] Memory overhead

**Tests:**
- [x] Single context GC
- [x] Multi-context isolation
- [x] Snapshot/restore
- [x] Stress testing
- [x] Performance benchmarks

---

### Task 12: Documentation

**Files:** Various

**Checklist:**

- [x] Module-level docs for all new modules
- [x] API documentation with examples
- [x] Architecture diagram
  - [ ] VmContext structure
  - [ ] Heap layout
  - [ ] GC algorithm flow
  - [ ] Snapshot format
- [x] Usage guide
  ```rust
  // Example: Creating an inner VM
  let mut vm = Vm::new(VmOptions {
      max_heap_bytes: Some(16 * 1024 * 1024),  // 16 MB
      max_tasks: Some(100),
      max_step_budget: None,
      gc_threshold_ratio: 2.0,
  });

  // Allocate objects
  let obj = vm.allocate(MyObject::new());

  // Monitor resources
  let stats = vm.get_stats();
  println!("Heap usage: {}/{:?}", stats.heap_bytes_used, stats.max_heap_bytes);

  // Snapshot
  let snapshot = vm.snapshot()?;

  // Restore
  let mut vm2 = Vm::restore(snapshot)?;
  ```

---

## Implementation Details

### Memory Layout

```
┌─────────────────────────────────────────┐
│ GcHeader (16 bytes, 8-byte aligned)     │
│  - marked: bool                         │
│  - context_id: VmContextId              │
│  - type_id: TypeId                      │
│  - size: usize                          │
├─────────────────────────────────────────┤  ← GcPtr points here
│ Object data (variable size)             │
└─────────────────────────────────────────┘
```

### VmContext Architecture

```
VmContext {
    id: VmContextId,
    heap: Heap {
        context_id,
        allocations: Vec<*mut GcHeader>,
        type_registry: Arc<TypeRegistry>,
    },
    globals: HashMap<String, Value>,
    resource_limits: ResourceLimits,
    resource_counters: ResourceCounters,
    gc_threshold: usize,
}
```

### GC Algorithm (Per-Context Mark-Sweep)

**Mark Phase:**
1. Clear all mark bits in this context's heap
2. Start from roots (tasks, globals in this context)
3. Recursively mark using precise pointer maps
4. Use type metadata for exact pointer locations

**Sweep Phase:**
1. Iterate allocations in this context's heap
2. Free unmarked objects (with proper deallocation)
3. Update heap statistics
4. Adjust GC threshold

**Complexity:** O(live objects in context)

### Safepoint Protocol

```
Worker loop:
    while running {
        execute_instruction()
        if at_safepoint_location() {
            safepoint_poll()
        }
    }

safepoint_poll():
    if gc_pending || snapshot_pending {
        enter_safepoint()
        barrier.wait()  // Stop
        barrier.wait()  // Wait for resume
    }

GC/Snapshot request:
    set_pending_flag()
    barrier.wait()  // All workers stopped
    perform_gc_or_snapshot()
    clear_pending_flag()
    barrier.wait()  // Resume workers
```

---

## Testing Requirements

### Unit Tests (Minimum 85% coverage)

1. **Value tests** (15 tests)
2. **Type metadata tests** (10 tests)
3. **Heap tests** (20 tests)
4. **GC tests** (25 tests)
5. **VmContext tests** (15 tests)
6. **Safepoint tests** (10 tests)
7. **Snapshot tests** (15 tests)
8. **Marshalling tests** (10 tests)

### Integration Tests (20 tests)

- End-to-end GC cycles
- Multi-context isolation
- Snapshot/restore workflows
- Resource limit enforcement
- Stress testing

### Performance Tests

- GC pause time < 10ms for 1MB heap
- Allocation throughput > 1M allocs/sec
- Memory overhead < 20%

---

## Success Criteria

### Must Have

- ✅ Per-context heaps fully functional
- ✅ Precise mark-sweep GC works correctly
- ✅ No memory leaks in tests
- ✅ Snapshot/restore preserves heap state
- ✅ Resource limits enforced
- ✅ Multi-context isolation verified
- ✅ Safepoint coordination works
- ✅ All tests pass
- ✅ Test coverage >85%
- ✅ Documentation complete

### Nice to Have

- GC pause time profiling
- Heap visualization tools
- GC tuning parameters
- Weak references

---

## Future Phases

### Phase 2: Generational GC (Milestone 4.x)

- Young generation (copying collector)
- Write barriers
- Old generation (mark-sweep)
- Promotion logic
- Tuning for typical workloads

**Expected improvement:** 2-5x better throughput for object-heavy code

### Phase 3: Incremental/Concurrent GC (Milestone 6.x)

- Only if GC pause is main bottleneck
- Tri-color marking
- Incremental STW slices
- Full concurrent marking (later)

**Expected improvement:** Sub-millisecond pause times

---

## Estimated Time

**Phase 1 Implementation:** 4-5 weeks (160-200 hours)

**Breakdown:**
- Type metadata: 2-3 days
- VmContext structure: 3-4 days
- Per-context heap: 4-5 days
- Precise mark-sweep GC: 5-7 days
- Safepoint infrastructure: 3-4 days
- Snapshot integration: 5-6 days
- Inner VM API: 3-4 days
- Data marshalling: 2-3 days
- Integration tests: 3-4 days
- Documentation: 2-3 days

---

**Status:** Ready to Start
**Next Milestone:** 1.4 - Stack & Frame Management (with VmContext integration)
**Version:** v2.0 (Integrated Phase 1)
**Last Updated:** 2026-01-05
