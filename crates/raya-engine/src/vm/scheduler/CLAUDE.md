# scheduler module

Work-stealing task scheduler for Raya's goroutine-style concurrency.

## Overview

The scheduler manages lightweight green threads (Tasks) across multiple OS threads using a work-stealing algorithm. This provides Go-style concurrency with automatic parallelism.

## Module Structure

```
scheduler/
├── mod.rs        # Scheduler struct, public API
├── task.rs       # Task struct, state machine
├── worker.rs     # Worker thread implementation
└── queue.rs      # Work-stealing deques
```

## Key Types

### Scheduler
```rust
pub struct Scheduler {
    workers: Vec<Worker>,
    global_queue: ConcurrentQueue<Task>,
    task_count: AtomicUsize,
}

scheduler.spawn(func_id, args) -> TaskId
scheduler.run() -> Vec<TaskResult>
scheduler.shutdown()
```

### Task
```rust
pub struct Task {
    pub id: TaskId,
    pub state: TaskState,
    pub context: VmContext,
    pub result: Option<Value>,
    pub blocked_on: Option<BlockReason>,
}

pub enum TaskState {
    Ready,
    Running,
    Blocked,
    Complete,
}

pub enum BlockReason {
    AwaitingTask(TaskId),
    Mutex(MutexId),
    Channel(ChannelId),
}
```

### Worker
```rust
pub struct Worker {
    id: usize,
    local_queue: Deque<Task>,
    scheduler: Arc<Scheduler>,
    vm: Vm,
}
```

## Work-Stealing Algorithm

```
┌─────────────────────────────────────────────────────────────┐
│                      Global Queue                            │
└─────────────────────────────────────────────────────────────┘
         │                    │                    │
         ▼                    ▼                    ▼
┌───────────────┐    ┌───────────────┐    ┌───────────────┐
│   Worker 0    │    │   Worker 1    │    │   Worker 2    │
│ ┌───────────┐ │    │ ┌───────────┐ │    │ ┌───────────┐ │
│ │Local Queue│ │◄──►│ │Local Queue│ │◄──►│ │Local Queue│ │
│ └───────────┘ │    │ └───────────┘ │    │ └───────────┘ │
│               │    │               │    │               │
│  ┌─────────┐  │    │  ┌─────────┐  │    │  ┌─────────┐  │
│  │   VM    │  │    │  │   VM    │  │    │  │   VM    │  │
│  └─────────┘  │    │  └─────────┘  │    │  └─────────┘  │
└───────────────┘    └───────────────┘    └───────────────┘
        │                    │                    │
        └────── Steal ───────┴────── Steal ───────┘
```

**Scheduling Logic:**
1. Try local queue first (LIFO for cache locality)
2. If empty, try global queue
3. If empty, steal from random worker (FIFO from victim)

## Preemption

Tasks are preempted cooperatively after N operations:

```rust
impl Worker {
    fn run_task(&mut self, task: &mut Task) {
        loop {
            match self.vm.step(task) {
                Ok(()) => {
                    task.op_count += 1;
                    if task.op_count >= PREEMPTION_THRESHOLD {
                        // Yield to other tasks
                        self.local_queue.push_back(task);
                        return;
                    }
                }
                Err(VmError::Suspended) => {
                    // Task awaiting something
                    self.scheduler.park_task(task);
                    return;
                }
                // ...
            }
        }
    }
}
```

Default preemption threshold: 10ms worth of operations (~10,000 ops)

## Task Lifecycle

```
         spawn()
            │
            ▼
        ┌───────┐
        │ Ready │
        └───┬───┘
            │ scheduled
            ▼
        ┌─────────┐
   ┌───►│ Running │◄───┐
   │    └────┬────┘    │
   │         │         │
   │   await/mutex     │ resumed
   │         │         │
   │         ▼         │
   │    ┌─────────┐    │
   │    │ Blocked │────┘
   │    └─────────┘
   │
   │ preempted
   │
   └─── back to Ready

        completed
            │
            ▼
        ┌──────────┐
        │ Complete │
        └──────────┘
```

## Configuration

```rust
// Environment variable
RAYA_NUM_THREADS=4  // Number of worker threads

// Or via VmOptions
VmOptions {
    num_workers: 4,
    preemption_threshold: 10_000,
}
```

Default: `num_cpus::get()` workers

## For AI Assistants

- Uses `crossbeam-deque` for work-stealing queues
- Tasks are NOT 1:1 with OS threads
- Preemption is cooperative, not timer-based
- Blocked tasks don't consume worker threads
- Mutex blocking is task-level, not thread-level
- `SPAWN` opcode creates new task, returns TaskRef
- `AWAIT` suspends current task until target completes
