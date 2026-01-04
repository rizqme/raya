# Raya VM High-Level Architecture

A multi-threaded, task-based virtual machine for the Raya language (a strict subset of TypeScript).

---

## 1. Goals & Constraints

* **Execute Raya bytecode** (Raya is a strict, safe subset of TypeScript).
* **Support goroutine-style Tasks** (green threads) scheduled over multiple OS threads.
* **Provide async/await semantics** where:
  * `async` functions always run in their own Task.
  * Calling an async function starts a Task immediately and returns a `Task<T>` handle.
  * `await` blocks the current Task until the awaited Task completes.
* **Guarantee atomic single-variable reads/writes** (no torn values).
* **Provide Mutex** for multi-operation atomicity, with a simple, Go-like memory model.
* **Use static type information** from Raya to optimize execution (typed opcodes, unboxed locals, specialized layouts).

---

## 2. Top-Level Architecture

The Raya VM has three major layers:

### 2.1 Frontend / Compiler

* Parses Raya (TypeScript-subset syntax)
* Type checks according to Raya rules
* Produces a typed IR/AST
* Emits typed bytecode + metadata (types, classes, methods, modules)

### 2.2 Runtime / VM Core

* Executes bytecode
* Manages Tasks, Workers, Scheduler
* Manages Heap, GC, Mutexes, and built-in types

### 2.3 Host Integration / FFI Layer

* Provides system I/O, timers, networking, etc.
* Adapts host async operations into `Task<T>` handles

This document focuses on the **VM Core**.

---

## 3. Core Execution Model

### 3.1 Task (Green Thread)

A **Task** is the fundamental unit of execution in Raya, analogous to a goroutine.

Each Task has:

* A unique `TaskId`
* A call stack of frames
* A program counter (IP) into a function's bytecode
* A status: `NEW`, `READY`, `RUNNING`, `BLOCKED`, `COMPLETED`, `FAILED`
* Storage for its result or error
* Lists of waiting Tasks and then-callbacks (for Promise-like behavior)

Conceptual structure:

```rust
Task {
  id: TaskId
  status: TaskStatus

  stack: CallFrame[]
  sp: int           // stack pointer
  ip: IP            // instruction pointer

  result: Value
  error: Value | null

  waitingTasks: List<TaskId>          // tasks blocked on await this Task
  thenCallbacks: List<Continuation>   // registered via .then

  ownerWorker: WorkerId | null        // for locality (optional)
}
```

### 3.2 Call Frame

Each call frame represents one function activation:

```rust
CallFrame {
  func: FunctionHandle
  ip: IP             // return IP

  locals: Slot[]     // local variables (typed slots)
  args: Slot[]       // arguments, may share storage with locals
}
```

Slot representation can be optimized using static types (unboxed where possible).

---

## 4. Multi-Threaded Scheduler

### 4.0 Scheduler Overview

Like Go's runtime, the Raya VM is designed to **maximize CPU core utilization** by default:

* The VM automatically spawns **N worker threads**, where N = number of available CPU cores (via `std::thread::hardware_concurrency()` or equivalent)
* Tasks are distributed across all workers using work-stealing for load balancing
* This ensures that compute-intensive workloads automatically scale to use all available parallelism
* The number of workers can be configured via environment variable or API (e.g., `RAYA_NUM_THREADS`)

This design philosophy means:
* **No manual thread pool configuration required** — the VM handles parallelism automatically
* **Tasks run concurrently by default** — calling an async function immediately starts a Task that may run on any core
* **Efficient CPU utilization** — idle workers steal work from busy workers, minimizing thread idle time

### 4.1 Workers (OS Threads)

The Raya VM runs on N OS threads (typically N = CPU cores), each running a Worker loop.

```rust
Worker {
  id: WorkerId
  localQueue: Deque<TaskId>   // work-stealing deque
}
```

### 4.2 Global VM State

```rust
VM {
  tasks: ConcurrentMap<TaskId, Task>

  globalQueue: Deque<TaskId>  // shared fallback queue
  workers: Worker[]

  nextTaskId: AtomicCounter
  shutdownFlag: AtomicBool
}
```

### 4.3 Worker Loop

Each Worker thread executes a loop:

```rust
workerLoop(worker):
  while not vm.shutdownFlag:
    taskId = popFromLocalOrStealOrGlobal(worker)

    if taskId == NONE:
      parkThreadUntilWork()
      continue

    runTask(taskId)
```

* `popFromLocalOrStealOrGlobal` tries:
  * local queue first
  * then steals from other workers
  * then falls back to global queue

### 4.4 Task Execution

```rust
runTask(taskId):
  task = vm.tasks[taskId]
  task.status = RUNNING

  while true:
    instr = fetchInstruction(task)

    switch instr.opcode:
      case ... normal ops ...
      case AWAIT:
        handleAwait(task)
        return       // Task is now BLOCKED or completed
      case MUTEX_LOCK:
        if handleMutexLock(task, instr.mutexRef) == BLOCKED:
          return
      case YIELD:
        rescheduleTask(task)
        return
      case RETURN:
        completeTask(task, returnValue)
        return
```

The interpreter runs until the Task:

* blocks (on `AWAIT`, `MUTEX_LOCK`, I/O)
* yields
* returns or fails

---

## 5. Heap & Object Model

### 5.1 Value Model

Conceptually:

```rust
Value =
  Int | Float | Bool |
  String | Array | Map |
  Object | Null
```

Implementation optimizations:

* Unboxed primitives in locals/stack where types are known
* Boxed values in generic containers and erased contexts

### 5.2 Object Layout

```rust
Object {
  typeId: TypeId
  vtablePtr: *VTable
  fields: [FieldStorage]
}
```

`TypeId` maps to metadata describing:

* Field count and types
* Pointer map for GC
* Vtable layout for methods

### 5.3 Arrays & Maps

* `number[]` → contiguous numeric storage
* `User[]` → contiguous references
* `Map<K,V>` → hash table structure

Array/data structures use type metadata to avoid boxing where possible.

---

## 6. Async/Task Semantics

### 6.1 Task Lifecycle

#### Creation

* Via `SPAWN` instruction
* New Task created with initial frame for entry function
* Task status = `READY` and enqueued on a Worker queue

#### Running

* Worker picks Task, sets status = `RUNNING`

#### Blocking

* On `AWAIT`, contended `MUTEX_LOCK`, or I/O wait
* Task status = `BLOCKED`
* Worker picks another Task

#### Completion

* On `RETURN` or unhandled error
* Task status = `COMPLETED` or `FAILED`
* Result/error stored
* All `waitingTasks` are moved to `READY` and enqueued
* `.then` continuations scheduled as Tasks if present

### 6.2 Await Behavior

`AWAIT` implementation:

* Pop `TaskHandle` → target Task
* If `target.status` is `COMPLETED` or `FAILED`:
  * Push `target.result` or throw `target.error`
  * Continue execution
* Else:
  * Append current `TaskId` to `target.waitingTasks`
  * Set current Task status = `BLOCKED`
  * Return to Worker loop

When target completes:

* For each waiter in `waitingTasks`:
  * Set `waiter.status = READY`
  * Enqueue waiter on some Worker queue

---

## 7. Memory Model & Atomicity

### 7.1 Single Access Atomicity

The Raya VM guarantees:

* All single reads/writes of word-sized values are atomic:
  * No torn reads or writes

Implementation:

* Use aligned memory and appropriate atomic operations for variable storage

### 7.2 Synchronization via Tasks and Mutexes

* **Task completion and await** act as synchronization points:
  * All writes before a Task's completion are visible after an `await` on that Task
* **Mutex operations**:
  * `lock()` uses an acquire fence when taking the lock
  * `unlock()` uses a release fence when releasing

This defines a clear happens-before relation across Tasks.

---

## 8. Mutex Design

### 8.1 Mutex Structure

```rust
Mutex {
  state: UNLOCKED | LOCKED
  owner: TaskId | null
  waitQueue: Queue<TaskId>
}
```

### 8.2 lock()

* If `state == UNLOCKED`, atomically set to `LOCKED` and set `owner = currentTask`
* If `state == LOCKED`, append current Task to `waitQueue`, set Task status `BLOCKED`, yield

### 8.3 unlock()

* Only the owning Task may unlock
* If `waitQueue` is empty:
  * Set `state = UNLOCKED`, `owner = null`
* Else:
  * Pop next `TaskId`
  * Set `owner = nextTask`
  * Keep `state = LOCKED`
  * Set next Task status = `READY`, enqueue it

### 8.4 Await in Critical Sections

* Raya compiler and runtime forbid `await` while holding a `Mutex`
* Prevents deadlocks where a Task suspends with the lock held

---

## 9. Type-Aware Optimizations

### 9.1 Typed Opcodes

* Use static type information from Raya to emit:
  * `IADD` instead of generic `ADD`
  * `SCONCAT` instead of dynamic `+` on unions
  * Reduces runtime type checks and tagging overhead

### 9.2 Unboxed Locals & Stack

* For monomorphic functions, represent locals and stack slots using unboxed primitives
* Box only when storing into generic containers or crossing type-erased boundaries

### 9.3 Object & Array Layout

* Use type metadata to:
  * Store numeric arrays as raw numeric buffers
  * Store object arrays as raw pointer buffers

### 9.4 GC Pointer Maps

* Use class and type metadata to know which fields/slots are pointers
* Avoid scanning non-pointer data
* Speeds up GC traversal

---

## 10. Error Handling

* Runtime errors (e.g., invalid unlock, out-of-bounds, null access) terminate the current Task
* Awaiters receive the error when awaiting that Task
* The VM may provide hooks to log or propagate errors to the host

---

## 11. FFI Integration (High-Level)

* Host functions can be exposed as builtins
* Async host operations return `Task<T>` handles
* The VM treats external Tasks similarly to internal ones, with adapted completion callbacks

---

## 12. Future Extensions

Potential evolution paths:

* **JIT compilation** for hot functions
* **Channels** (Go-style) built on top of Tasks + Mutexes
* **Preemption** based on instruction or time quotas
* **More advanced type-based optimizations** (e.g., escape analysis)
* **Distributed Task scheduling** across processes or nodes

---

Raya VM provides a clear foundation for running Raya programs with goroutine-like concurrency, type-driven performance, and predictable semantics on modern multi-core systems.
