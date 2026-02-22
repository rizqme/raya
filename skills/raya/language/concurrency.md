# Concurrency Model

Raya uses **goroutine-style concurrency** with lightweight green threads called **Tasks**.

## Core Concepts

### Tasks

Tasks are lightweight green threads managed by a work-stealing scheduler:

- Start **immediately** when created
- Run on a **pool of OS threads** (CPU cores)
- **Non-blocking** - suspended tasks don't block OS threads
- **Cheap to create** - lazy stack allocation, stack pooling

### Async Functions

```typescript
async function fetchData(): Task<string> {
  // Task starts immediately when function is called
  return "data";
}

// Call creates and starts a Task
const task = fetchData();  // Task is already running

// Await suspends current Task until result is ready
const result = await task;  // result: string
```

**Key Points:**
- `async` functions return `Task<T>`, not `Promise<T>`
- Tasks start immediately (not lazy like JavaScript Promises)
- `await` suspends the current Task (doesn't block OS thread)

### Await Operator

```typescript
async function main(): Task<void> {
  const task1 = async computeA();  // Start task1
  const task2 = async computeB();  // Start task2 (runs concurrently)
  
  const a = await task1;  // Suspend until task1 completes
  const b = await task2;  // Suspend until task2 completes
  
  logger.info("Results:", a, b);
}
```

## Work-Stealing Scheduler

### Architecture

```
┌─────────────────────────────────┐
│   Unified Reactor + Scheduler   │
├─────────────────────────────────┤
│  VM Worker Pool  │  IO Pool     │
│  (CPU cores)     │  (blocking)  │
└─────────────────────────────────┘
```

### VM Worker Pool
- One thread per CPU core (default)
- Work-stealing deques for load balancing
- Each worker runs ready Tasks
- No thread-local storage dependencies

### IO Pool
- Handles blocking operations (file I/O, sleep, etc.)
- Suspends Task, runs on IO thread, resumes on VM worker
- Prevents blocking VM workers

## Task Lifecycle

```typescript
async function example(): Task<int> {
  // 1. Task created and started immediately
  
  // 2. Execute synchronously until suspension point
  const x = 10;
  const y = 20;
  
  // 3. Suspension point (await, I/O, sleep)
  await someOtherTask();  // Current Task suspended
  
  // 4. Resumed on any available VM worker
  return x + y;  // 5. Task completes
}
```

**States:**
- **Running** - Executing on a VM worker
- **Suspended** - Waiting for I/O, await, or sleep
- **Ready** - Waiting for available VM worker
- **Complete** - Finished with result

## Concurrent Patterns

### Parallel Execution

```typescript
import time from "std:time";

async function compute(id: int): Task<int> {
  time.sleep(100);  // Sleep 100ms
  return id * 2;
}

function main(): void {
  const tasks: Task<int>[] = [];
  
  // Start 10 tasks concurrently
  for (let i = 0; i < 10; i = i + 1) {
    tasks.push(compute(i));
  }
  
  // Collect results
  for (let i = 0; i < tasks.length; i = i + 1) {
    const result = await tasks[i];
    logger.info("Task", i, ":", result);
  }
}
```

### I/O Concurrency

All I/O operations are synchronous. Achieve concurrency with goroutines:

```typescript
import fs from "std:fs";

async function readFile(path: string): Task<string> {
  return fs.readTextFile(path);  // Blocks this Task, not OS thread
}

function main(): void {
  // Read multiple files concurrently
  const t1 = readFile("a.txt");
  const t2 = readFile("b.txt");
  const t3 = readFile("c.txt");
  
  // All reads happen in parallel on IO pool
  const a = await t1;
  const b = await t2;
  const c = await t3;
  
  logger.info("Total length:", a.length + b.length + c.length);
}
```

### Fan-Out/Fan-In

```typescript
type Result<T> = { ok: true; value: T } | { ok: false; error: string };

async function worker(id: int): Task<Result<int>> {
  try {
    const result = await doWork(id);
    return { ok: true, value: result };
  } catch (e) {
    return { ok: false, error: e.message };
  }
}

function main(): void {
  const workers: Task<Result<int>>[] = [];
  
  // Fan-out: Start many workers
  for (let i = 0; i < 100; i = i + 1) {
    workers.push(worker(i));
  }
  
  // Fan-in: Collect results
  let succeeded = 0;
  let failed = 0;
  
  for (let i = 0; i < workers.length; i = i + 1) {
    const result = await workers[i];
    if (result.ok) {
      succeeded = succeeded + 1;
    } else {
      failed = failed + 1;
    }
  }
  
  logger.info("Succeeded:", succeeded, "Failed:", failed);
}
```

## Task Optimization

### Lazy Stack Allocation

Tasks start with minimal stack space. Stacks grow on demand:
- Initial: 4KB per Task
- Growth: Double when needed (4KB → 8KB → 16KB...)
- Max: 1MB per Task

### Stack Pooling

Completed Task stacks are reused:
- Reduces allocation overhead
- Improves cache locality
- Minimizes GC pressure

### Per-Task Nursery Allocator

Each Task has a 64KB bump allocator:
- Fast allocation (just increment pointer)
- Reduced GC contention
- Short-lived objects stay in nursery

## Synchronization

### Mutexes

```typescript
import { Mutex } from "std:sync";

const counter = new Mutex<int>(0);

async function increment(): Task<void> {
  const lock = counter.lock();
  const value = lock.get();
  lock.set(value + 1);
  lock.unlock();
}
```

### Channels (Future)

Planned for inter-Task communication:

```typescript
// Future API (not yet implemented)
import { Channel } from "std:sync";

const ch = new Channel<int>();

async function sender(): Task<void> {
  for (let i = 0; i < 10; i = i + 1) {
    ch.send(i);
  }
  ch.close();
}

async function receiver(): Task<void> {
  while (true) {
    const msg = ch.receive();
    if (msg == null) break;
    logger.info("Received:", msg);
  }
}
```

## Preemption

Tasks are **cooperatively scheduled** but with periodic preemption:

- Check preemption at loop back-edges
- Long-running Tasks yield to scheduler
- Prevents starvation
- Configurable preemption interval

## Error Handling

Uncaught errors in Tasks propagate to `await` point:

```typescript
async function mayFail(): Task<int> {
  throw new Error("Something went wrong");
}

function main(): void {
  const task = mayFail();
  
  try {
    const result = await task;  // Error propagates here
  } catch (e) {
    logger.error("Task failed:", e.message);
  }
}
```

## Best Practices

1. **Start Tasks early** - They begin executing immediately
2. **Batch awaits** - Start many Tasks, await later
3. **Use I/O pool** - Let blocking ops run on separate threads
4. **Avoid shared mutable state** - Use message passing or Mutex
5. **Keep Tasks small** - Better load balancing

## Performance Characteristics

| Operation | Cost |
|-----------|------|
| Task creation | ~100ns (lazy stack) |
| Task switch | ~20-50ns (no syscall) |
| Await | ~50-100ns |
| Mutex lock (uncontended) | ~10-20ns |
| I/O operation | Runs on IO pool (non-blocking) |

## Comparison to Other Models

| Feature | Raya | JavaScript | Go | Rust (tokio) |
|---------|------|------------|-----|--------------|
| Concurrency | Tasks (green threads) | Promises (event loop) | Goroutines | Futures + executor |
| Start | Immediate | Lazy | Immediate | Lazy |
| Scheduler | Work-stealing | Single-threaded | M:N | Configurable |
| Syntax | `async`/`await` | `async`/`await` | `go` keyword | `async`/`await` |

## Related

- [Type System](type-system.md) - Task<T> type
- [Standard Library](../stdlib/overview.md) - Concurrency primitives
- [VM Architecture](../architecture/vm.md) - Scheduler internals
