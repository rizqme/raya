# Milestone 1.10: Task Scheduler (Work-Stealing Concurrency)

**Phase:** 1 - VM Core
**Crate:** `raya-core`
**Status:** ✅ Complete
**Prerequisites:**
- Milestone 1.9 (Safepoint Infrastructure) ✅
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

Implement the **Task Scheduler** to enable goroutine-style concurrency in Raya. This is the core concurrency primitive that allows `async` functions to create lightweight green threads (Tasks) that execute across a pool of OS worker threads using work-stealing.

**Key Architectural Decisions:**

- **Goroutine-style semantics** - `async` functions create Tasks immediately (not lazy)
- **Work-stealing scheduler** - Based on crossbeam deques for load balancing
- **M:N threading** - Many Tasks mapped to N OS threads (default = CPU core count)
- **Cooperative scheduling** - Tasks yield at `await` points and safepoints
- **Fair scheduling** - No Task can monopolize a worker indefinitely

**Key Deliverable:** A production-ready work-stealing task scheduler that enables concurrent execution of Raya async functions with minimal overhead.

---

## Goals

### Primary Goals

- [ ] Implement `Task` structure representing a green thread
- [ ] Implement `TaskHandle<T>` for awaiting Task results
- [ ] Create work-stealing scheduler with worker threads
- [ ] Implement task spawning via `SPAWN` opcode
- [ ] Implement task suspension/resumption via `AWAIT` opcode
- [ ] Integrate with safepoint infrastructure
- [ ] Support configurable worker thread count
- [ ] Add task statistics and monitoring
- [ ] Test coverage >85%

### Secondary Goals

- Task priority levels (high/normal/low)
- Task cancellation support
- Task-local storage
- Deadlock detection
- Performance profiling tools

### Non-Goals (Deferred)

- Preemptive scheduling (cooperative only)
- NUMA-aware scheduling
- Priority inheritance
- Real-time scheduling guarantees

---

## Design Philosophy

### Goroutine-Style vs Promise-Style

**Raya Tasks (Goroutines):**
```typescript
async function work(): Task<number> {
  return 42;
}

const task = work();  // Task starts NOW
const result = await task;  // Suspend current Task
```

**NOT like JavaScript Promises:**
```javascript
// This is NOT how Raya works
async function work() {
  return 42;
}

const promise = work();  // Lazy - doesn't start yet
const result = await promise;  // Start now
```

### Work-Stealing Algorithm

**Worker Structure:**
```
Worker 0: [T1, T2, T3] ← local deque (LIFO for own tasks)
Worker 1: [T4, T5]
Worker 2: []          ← idle, will steal
```

**Stealing Process:**
1. Worker 2 finds it has no work
2. Randomly selects Worker 0 as victim
3. Steals T1 from the **bottom** (FIFO for stealing)
4. Worker 0 continues with T3 (top)

**Why This Works:**
- **Cache locality** - Workers prefer their own tasks (LIFO)
- **Load balancing** - Idle workers steal oldest tasks (FIFO)
- **Low contention** - Only bottom of deque needs synchronization

### Task States

```
┌─────────────┐
│   Created   │ (just spawned)
└──────┬──────┘
       │
       v
┌─────────────┐
│   Running   │ (executing on worker)
└──┬────┬────┘
   │    │
   │    v
   │ ┌─────────────┐
   │ │  Suspended  │ (awaiting another Task)
   │ └──────┬──────┘
   │        │
   │        v
   │ ┌─────────────┐
   │ │   Resumed   │ (dependency completed)
   │ └──────┬──────┘
   │        │
   v        v
┌─────────────┐
│  Completed  │ (result available)
└─────────────┘
```

### Task Execution Model

**Single Task Execution:**
```rust
loop {
    // Execute bytecode
    match opcode {
        SPAWN => {
            // Create new Task
            // Push to local deque
        }
        AWAIT => {
            // Suspend current Task
            // Add to dependency list
            // Pick next Task from deque
        }
        RETURN => {
            // Complete Task
            // Resume waiting Tasks
            break;
        }
    }

    // Safepoint poll every N instructions
    safepoint.poll();
}
```

---

## Tasks

### Task 1: Task Structure

**File:** `crates/raya-core/src/scheduler/task.rs`

**Checklist:**

- [ ] Define `Task` struct with execution state
- [ ] Define `TaskId` with unique ID generation
- [ ] Define `TaskState` enum
- [ ] Implement `TaskHandle<T>` for result retrieval
- [ ] Add task context (locals, stack, IP)
- [ ] Implement task creation and initialization

**Implementation:**

```rust
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use crate::value::Value;
use crate::stack::Stack;

/// Unique identifier for a Task
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct TaskId(u64);

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

impl TaskId {
    pub fn new() -> Self {
        TaskId(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed))
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// State of a Task
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TaskState {
    /// Just created, not yet scheduled
    Created,
    /// Currently executing on a worker
    Running,
    /// Suspended waiting for another Task
    Suspended,
    /// Ready to run (was suspended, now resumed)
    Resumed,
    /// Completed with a result
    Completed,
    /// Failed with an error
    Failed,
}

/// A lightweight green thread
pub struct Task {
    /// Unique identifier
    id: TaskId,

    /// Current state
    state: Mutex<TaskState>,

    /// Function to execute
    function_id: usize,

    /// Module containing the function
    module: Arc<raya_bytecode::Module>,

    /// Execution stack
    stack: Mutex<Stack>,

    /// Instruction pointer
    ip: AtomicUsize,

    /// Result value (if completed)
    result: Mutex<Option<Value>>,

    /// Tasks waiting for this Task to complete
    waiters: Mutex<Vec<TaskId>>,

    /// Parent task (if spawned from another Task)
    parent: Option<TaskId>,
}

impl Task {
    pub fn new(
        function_id: usize,
        module: Arc<raya_bytecode::Module>,
        parent: Option<TaskId>,
    ) -> Self {
        Self {
            id: TaskId::new(),
            state: Mutex::new(TaskState::Created),
            function_id,
            module,
            stack: Mutex::new(Stack::new()),
            ip: AtomicUsize::new(0),
            result: Mutex::new(None),
            waiters: Mutex::new(Vec::new()),
            parent,
        }
    }

    pub fn id(&self) -> TaskId {
        self.id
    }

    pub fn state(&self) -> TaskState {
        *self.state.lock().unwrap()
    }

    pub fn set_state(&self, state: TaskState) {
        *self.state.lock().unwrap() = state;
    }

    pub fn complete(&self, result: Value) {
        *self.result.lock().unwrap() = Some(result);
        self.set_state(TaskState::Completed);
    }

    pub fn add_waiter(&self, waiter_id: TaskId) {
        self.waiters.lock().unwrap().push(waiter_id);
    }

    pub fn take_waiters(&self) -> Vec<TaskId> {
        std::mem::take(&mut *self.waiters.lock().unwrap())
    }
}

/// Handle for awaiting a Task's result
pub struct TaskHandle<T> {
    task_id: TaskId,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> TaskHandle<T> {
    pub fn new(task_id: TaskId) -> Self {
        Self {
            task_id,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn task_id(&self) -> TaskId {
        self.task_id
    }
}
```

**Tests:**
- Create Task with function
- TaskId uniqueness
- State transitions
- Result storage
- Waiter list management

---

### Task 2: Work-Stealing Deque

**File:** `crates/raya-core/src/scheduler/deque.rs`

**Checklist:**

- [ ] Wrap crossbeam Worker/Stealer
- [ ] Implement push (LIFO for local worker)
- [ ] Implement pop (LIFO for local worker)
- [ ] Implement steal (FIFO from other workers)
- [ ] Add metrics (tasks pushed, stolen, etc.)

**Implementation:**

```rust
use crossbeam_deque::{Injector, Stealer, Worker};
use std::sync::Arc;
use crate::scheduler::Task;

/// Work-stealing deque for a single worker
pub struct WorkerDeque {
    /// Local worker deque (LIFO for own tasks)
    worker: Worker<Arc<Task>>,

    /// Stealer handles for other workers
    stealers: Vec<Stealer<Arc<Task>>>,

    /// Global injector for tasks without affinity
    injector: Arc<Injector<Arc<Task>>>,
}

impl WorkerDeque {
    pub fn new(
        worker: Worker<Arc<Task>>,
        stealers: Vec<Stealer<Arc<Task>>>,
        injector: Arc<Injector<Arc<Task>>>,
    ) -> Self {
        Self {
            worker,
            stealers,
            injector,
        }
    }

    /// Push a task (LIFO)
    pub fn push(&self, task: Arc<Task>) {
        self.worker.push(task);
    }

    /// Pop a task (LIFO) - most recent task
    pub fn pop(&self) -> Option<Arc<Task>> {
        self.worker.pop()
    }

    /// Try to get work: local pop, then steal, then inject
    pub fn find_work(&self) -> Option<Arc<Task>> {
        // 1. Try local deque (LIFO)
        if let Some(task) = self.worker.pop() {
            return Some(task);
        }

        // 2. Try stealing from other workers (FIFO)
        loop {
            if let Some(task) = self.steal_from_others() {
                return Some(task);
            }

            // 3. Try global injector
            match self.injector.steal() {
                crossbeam_deque::Steal::Success(task) => return Some(task),
                crossbeam_deque::Steal::Empty => break,
                crossbeam_deque::Steal::Retry => continue,
            }
        }

        None
    }

    fn steal_from_others(&self) -> Option<Arc<Task>> {
        use rand::Rng;

        if self.stealers.is_empty() {
            return None;
        }

        // Randomly select a victim
        let mut rng = rand::thread_rng();
        let start = rng.gen_range(0..self.stealers.len());

        // Try each stealer starting from random position
        for i in 0..self.stealers.len() {
            let index = (start + i) % self.stealers.len();
            let stealer = &self.stealers[index];

            loop {
                match stealer.steal() {
                    crossbeam_deque::Steal::Success(task) => return Some(task),
                    crossbeam_deque::Steal::Empty => break,
                    crossbeam_deque::Steal::Retry => continue,
                }
            }
        }

        None
    }
}
```

**Tests:**
- Push and pop from local deque
- Stealing from other workers
- Global injector fallback
- Empty deque handling

---

### Task 3: Worker Thread

**File:** `crates/raya-core/src/scheduler/worker.rs`

**Checklist:**

- [ ] Define `Worker` struct
- [ ] Implement worker thread loop
- [ ] Integrate with work-stealing deque
- [ ] Add task execution logic
- [ ] Integrate with safepoints
- [ ] Add worker statistics

**Implementation:**

```rust
use std::sync::Arc;
use std::thread;
use crate::scheduler::{Task, TaskId, TaskState, WorkerDeque};
use crate::vm::SafepointCoordinator;
use crate::value::Value;
use crate::VmResult;

/// Worker thread that executes Tasks
pub struct Worker {
    /// Worker ID
    id: usize,

    /// Work-stealing deque
    deque: WorkerDeque,

    /// Safepoint coordinator
    safepoint: Arc<SafepointCoordinator>,

    /// Worker thread handle
    handle: Option<thread::JoinHandle<()>>,
}

impl Worker {
    pub fn new(
        id: usize,
        deque: WorkerDeque,
        safepoint: Arc<SafepointCoordinator>,
    ) -> Self {
        Self {
            id,
            deque,
            safepoint,
            handle: None,
        }
    }

    /// Start the worker thread
    pub fn start(&mut self) {
        let id = self.id;
        let deque = self.deque.clone();
        let safepoint = self.safepoint.clone();

        let handle = thread::spawn(move || {
            Worker::run_loop(id, deque, safepoint);
        });

        self.handle = Some(handle);
    }

    fn run_loop(
        id: usize,
        deque: WorkerDeque,
        safepoint: Arc<SafepointCoordinator>,
    ) {
        loop {
            // Find work (local pop, steal, or inject)
            let task = match deque.find_work() {
                Some(task) => task,
                None => {
                    // No work available, sleep briefly
                    thread::sleep(std::time::Duration::from_micros(100));
                    continue;
                }
            };

            // Execute task
            task.set_state(TaskState::Running);

            match Self::execute_task(&task, &safepoint) {
                Ok(result) => {
                    task.complete(result);
                    // Resume waiting tasks
                    let waiters = task.take_waiters();
                    for waiter_id in waiters {
                        // TODO: Resume waiter task
                    }
                }
                Err(e) => {
                    eprintln!("Task {} failed: {:?}", task.id().as_u64(), e);
                    task.set_state(TaskState::Failed);
                }
            }
        }
    }

    fn execute_task(
        task: &Task,
        safepoint: &Arc<SafepointCoordinator>,
    ) -> VmResult<Value> {
        // TODO: Execute task bytecode
        // This will be similar to Vm::execute but:
        // 1. Use task's stack
        // 2. Handle SPAWN opcode
        // 3. Handle AWAIT opcode
        // 4. Poll safepoints regularly

        Ok(Value::null())
    }
}
```

**Tests:**
- Worker thread startup
- Task execution
- Work stealing behavior
- Safepoint integration

---

### Task 4: Scheduler

**File:** `crates/raya-core/src/scheduler/scheduler.rs`

**Checklist:**

- [ ] Define `Scheduler` struct
- [ ] Initialize worker pool
- [ ] Implement task spawning
- [ ] Implement task registry
- [ ] Add shutdown protocol
- [ ] Expose statistics API

**Implementation:**

```rust
use std::sync::Arc;
use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use crossbeam_deque::{Injector, Worker as WorkerDeque, Stealer};
use crate::scheduler::{Task, TaskId, Worker, WorkerDeque as WDeque};
use crate::vm::SafepointCoordinator;

/// Main task scheduler
pub struct Scheduler {
    /// Worker threads
    workers: Vec<Worker>,

    /// All active tasks (by ID)
    tasks: Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,

    /// Global task injector
    injector: Arc<Injector<Arc<Task>>>,

    /// Safepoint coordinator
    safepoint: Arc<SafepointCoordinator>,

    /// Number of worker threads
    worker_count: usize,
}

impl Scheduler {
    /// Create a new scheduler with the specified number of workers
    pub fn new(worker_count: usize) -> Self {
        let safepoint = Arc::new(SafepointCoordinator::new(worker_count));
        let injector = Arc::new(Injector::new());
        let tasks = Arc::new(RwLock::new(FxHashMap::default()));

        // Create worker deques
        let mut worker_deques = Vec::new();
        let mut stealers = Vec::new();

        for _ in 0..worker_count {
            let worker = WorkerDeque::new_lifo();
            stealers.push(worker.stealer());
            worker_deques.push(worker);
        }

        // Create workers
        let mut workers = Vec::new();
        for (id, worker_deque) in worker_deques.into_iter().enumerate() {
            let other_stealers: Vec<_> = stealers
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != id)
                .map(|(_, s)| s.clone())
                .collect();

            let deque = WDeque::new(
                worker_deque,
                other_stealers,
                injector.clone(),
            );

            let worker = Worker::new(id, deque, safepoint.clone());
            workers.push(worker);
        }

        Self {
            workers,
            tasks,
            injector,
            safepoint,
            worker_count,
        }
    }

    /// Start all worker threads
    pub fn start(&mut self) {
        for worker in &mut self.workers {
            worker.start();
        }
    }

    /// Spawn a new task
    pub fn spawn(&self, task: Arc<Task>) -> TaskId {
        let task_id = task.id();

        // Register task
        self.tasks.write().insert(task_id, task.clone());

        // Push to global injector
        self.injector.push(task);

        task_id
    }

    /// Get a task by ID
    pub fn get_task(&self, task_id: TaskId) -> Option<Arc<Task>> {
        self.tasks.read().get(&task_id).cloned()
    }

    /// Number of active tasks
    pub fn task_count(&self) -> usize {
        self.tasks.read().len()
    }

    /// Shutdown the scheduler
    pub fn shutdown(&mut self) {
        // TODO: Signal workers to stop
        // Wait for all workers to finish
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        let worker_count = num_cpus::get();
        Self::new(worker_count)
    }
}
```

**Tests:**
- Scheduler creation with N workers
- Task spawning and registration
- Task retrieval
- Worker count configuration

---

### Task 5: Interpreter Integration (SPAWN/AWAIT Opcodes)

**File:** `crates/raya-core/src/vm/interpreter.rs`

**Checklist:**

- [ ] Add `SPAWN` opcode handling
- [ ] Add `AWAIT` opcode handling
- [ ] Integrate scheduler with VM
- [ ] Handle task suspension
- [ ] Handle task resumption

**Implementation:**

```rust
// In interpreter.rs

match opcode {
    Opcode::Spawn => {
        // Read function index
        let func_index = self.read_u16(code, &mut ip)? as usize;

        // Create new Task
        let task = Arc::new(Task::new(
            func_index,
            self.module.clone(),
            self.current_task_id,
        ));

        // Spawn on scheduler
        let task_id = self.scheduler.spawn(task);

        // Push TaskHandle<T> onto stack
        let handle = TaskHandle::new(task_id);
        self.stack.push(Value::task_handle(handle))?;
    }

    Opcode::Await => {
        // Pop TaskHandle<T> from stack
        let handle = self.stack.pop()?.as_task_handle()
            .ok_or_else(|| VmError::TypeError("Expected TaskHandle".to_string()))?;

        // Get the task
        let task = self.scheduler.get_task(handle.task_id())
            .ok_or_else(|| VmError::RuntimeError("Task not found".to_string()))?;

        // Check if task is complete
        match task.state() {
            TaskState::Completed => {
                // Task is done, get result
                let result = task.result.lock().unwrap()
                    .ok_or_else(|| VmError::RuntimeError("Task has no result".to_string()))?;
                self.stack.push(result)?;
            }
            _ => {
                // Task not complete yet - suspend current task
                task.add_waiter(self.current_task_id);
                self.suspend_current_task();
                // This function won't return - control passes to scheduler
                unreachable!();
            }
        }
    }
}
```

**Tests:**
- SPAWN opcode creates task
- AWAIT on completed task returns result
- AWAIT on pending task suspends
- Task suspension and resumption

---

## Implementation Details

### Thread Count Configuration

**Default:** `num_cpus::get()` (number of CPU cores)

**Override via environment variable:**
```bash
RAYA_NUM_THREADS=8 raya run program.raya
```

**Override programmatically:**
```rust
let scheduler = Scheduler::new(8);
```

### Work-Stealing Algorithm Details

**Local Queue (LIFO):**
- Worker pushes: `push(task)` → top of deque
- Worker pops: `pop()` → top of deque (most recent)
- **Why:** Cache locality - recently pushed tasks likely share data

**Stealing (FIFO):**
- Thief steals: `steal()` → bottom of deque (oldest)
- **Why:** Load balancing - oldest tasks likely to have more work

**Performance:**
- Local push/pop: O(1), no synchronization
- Stealing: O(1), lock-free CAS
- Contention only at deque bottom (rare)

### Task Lifecycle Example

```typescript
async function child(): Task<number> {
  return 42;
}

async function parent(): Task<number> {
  const t1 = child();  // SPAWN opcode
  const t2 = child();  // SPAWN opcode

  const r1 = await t1;  // AWAIT opcode
  const r2 = await t2;  // AWAIT opcode

  return r1 + r2;
}
```

**Execution:**
1. `parent()` called → Task P created, pushed to worker 0
2. Worker 0 executes P
3. `child()` called → Task C1 created, pushed to worker 0
4. `child()` called → Task C2 created, pushed to worker 0
5. Worker 1 idle → steals C1 from worker 0
6. `await t1` → P suspends, added to C1's waiter list
7. Worker 0 picks C2, executes, completes
8. C1 completes on worker 1 → resumes P
9. P resumes on worker 0, gets result
10. `await t2` → P gets C2 result (already complete)
11. P completes

---

## Testing Requirements

### Unit Tests (Minimum 25 tests)

**Task Tests:**
1. Task creation with function
2. TaskId uniqueness
3. State transitions (Created → Running → Completed)
4. Result storage
5. Waiter list management
6. Parent task tracking

**WorkerDeque Tests:**
7. Push and pop (LIFO)
8. Empty deque handling
9. Stealing from other workers (FIFO)
10. Global injector fallback
11. Multiple stealers
12. Concurrent steal attempts

**Worker Tests:**
13. Worker thread startup
14. Task execution
15. Work stealing behavior
16. Idle worker handling
17. Safepoint integration

**Scheduler Tests:**
18. Scheduler creation
19. Worker pool initialization
20. Task spawning
21. Task registration
22. Task retrieval by ID
23. Task count tracking
24. Worker count configuration
25. Default worker count (CPU cores)

### Integration Tests (15 tests)

**File:** `crates/raya-core/tests/scheduler_integration.rs`

1. **Simple async function**
   - Spawn single task
   - Await result
   - Verify correctness

2. **Multiple concurrent tasks**
   - Spawn 10 tasks
   - Await all results
   - Verify all complete

3. **Nested task spawning**
   - Parent spawns children
   - Children spawn grandchildren
   - Verify hierarchy

4. **Work stealing verification**
   - Spawn many tasks on one worker
   - Verify other workers steal

5. **Task suspension and resumption**
   - Task awaits incomplete task
   - Verify suspension
   - Verify resumption after completion

6. **Fibonacci concurrent**
   - Recursive Fibonacci using async
   - Verify correctness

7. **Producer-consumer pattern**
   - Producer tasks create data
   - Consumer tasks process data
   - Verify ordering

8. **Stress test: 1000 tasks**
   - Spawn 1000 simple tasks
   - Verify all complete
   - Check performance

9. **Safepoint coordination**
   - Execute tasks while requesting STW pause
   - Verify all tasks reach safepoint

10. **Task failure handling**
    - Task throws error
    - Verify error propagation
    - Verify other tasks unaffected

11. **Worker thread count**
    - Test with 1, 2, 4, 8 workers
    - Verify correctness at each count

12. **Global injector**
    - Push tasks to global injector
    - Verify workers pick them up

13. **Task dependencies**
    - Task A awaits Task B awaits Task C
    - Verify correct execution order

14. **Interleaved execution**
    - Two tasks with alternating work
    - Verify both make progress

15. **Scheduler shutdown**
    - Spawn tasks
    - Shutdown scheduler
    - Verify cleanup

---

## Success Criteria

### Must Have

- [ ] Task structure fully implemented
- [ ] Work-stealing scheduler functional
- [ ] SPAWN and AWAIT opcodes working
- [ ] All unit tests pass (25+ tests)
- [ ] All integration tests pass (15+ tests)
- [ ] Test coverage >85%
- [ ] Documentation complete
- [ ] No race conditions in scheduler
- [ ] Proper cleanup on shutdown

### Nice to Have

- Task spawning overhead <10μs
- Work stealing latency <1μs
- Support for 10,000+ concurrent tasks
- Task-local storage
- Advanced debugging tools

### Performance Targets

- **Task spawn time:** <10μs per task
- **Context switch time:** <500ns
- **Work steal latency:** <1μs
- **Throughput:** >1M tasks/second on 8 cores

---

## References

### Design Documents

- [ARCHITECTURE.md](../design/ARCHITECTURE.md) - Section 4: Task Scheduler
- [LANG.md](../design/LANG.md) - Section 14: Concurrency Model
- [SNAPSHOTTING.md](../design/SNAPSHOTTING.md) - Section 4: Task Suspension

### Related Milestones

- Milestone 1.5: Bytecode Interpreter (execution engine)
- Milestone 1.9: Safepoint Infrastructure (coordination)
- Milestone 1.11: VM Snapshotting (task serialization)

### External References

- Go Runtime Scheduler
- Tokio Runtime (Rust)
- Crossbeam Work-Stealing Deques
- Rayon Thread Pool

---

## Dependencies

**Crate Dependencies:**
```toml
[dependencies]
crossbeam-deque = "0.8"     # Work-stealing deques
num_cpus = "1.16"            # CPU core detection
parking_lot = "0.12"         # Efficient RwLock
rustc-hash = "1.1"           # Fast hashing
rand = "0.8"                 # Random victim selection
```

**Internal Dependencies:**
- `raya-core::vm::SafepointCoordinator` - STW coordination
- `raya-core::stack::Stack` - Execution stack
- `raya-core::value::Value` - Task results
- `raya-bytecode::Opcode` - SPAWN, AWAIT opcodes

---

## Implementation Notes

### Phase 1: Foundation (This Milestone)
- Basic Task structure
- Single-threaded scheduler (no work-stealing yet)
- Simple SPAWN and AWAIT

### Phase 2: Work-Stealing
- Multi-threaded worker pool
- Work-stealing deques
- Load balancing

### Phase 3: Optimization (Future)
- Task pooling (reuse Task objects)
- Inline task execution (skip queue for small tasks)
- NUMA-aware scheduling

---

## Open Questions

1. **Q:** Should Tasks be pooled and reused?
   **A:** Not initially - allocate fresh. Add pooling in optimization phase.

2. **Q:** How to handle panics in Tasks?
   **A:** Catch panics, mark Task as Failed, don't crash worker.

3. **Q:** Should we support task priorities?
   **A:** Deferred - all tasks equal priority initially.

4. **Q:** How to prevent Task starvation?
   **A:** Work-stealing ensures fairness. Monitor metrics for issues.

5. **Q:** Should SPAWN be blocking or non-blocking?
   **A:** Non-blocking - Task is created and queued immediately.

---

**Status Legend:**
- 🔄 In Progress
- ✅ Complete
- ⏸️ Blocked
- 📝 Planned
