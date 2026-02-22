# Concurrency

Raya uses goroutine-style concurrency with lightweight Tasks and a work-stealing scheduler.

## Tasks

Tasks are green threads (like goroutines). They're cheap to create - you can spawn thousands.

### Creating Tasks

Use `async` prefix to create a Task:

```typescript
import io from "std:io";

async function fetchData(id: int): Task<string> {
  return `Data ${id}`;
}

function main(): void {
  // Task starts immediately
  const task = fetchData(42);
  
  // Await suspends current task
  const result = await task;
  io.writeln(result);
}
```

**Key points:**
- `async` creates a Task that starts immediately
- No explicit `spawn()` needed
- `await` suspends the current Task (doesn't block OS thread)

## Concurrent Execution

Run multiple tasks concurrently:

```typescript
import io from "std:io";
import time from "std:time";

async function download(url: string): Task<string> {
  time.sleep(1_000_000_000); // 1 second
  return `Downloaded: ${url}`;
}

function main(): void {
  const start = time.monotonic();
  
  // Start 3 concurrent tasks
  const task1 = download("https://example.com/1");
  const task2 = download("https://example.com/2");
  const task3 = download("https://example.com/3");
  
  // Wait for all to complete
  const result1 = await task1;
  const result2 = await task2;
  const result3 = await task3;
  
  io.writeln(result1);
  io.writeln(result2);
  io.writeln(result3);
  
  const elapsed = time.elapsed(start) / 1_000_000_000;
  io.writeln(`Took ${elapsed} seconds`);  // ~1 second, not 3
}
```

## Work-Stealing Scheduler

Tasks run on a work-stealing scheduler across all CPU cores:

- **Lightweight** - minimal stack overhead
- **Fair** - busy threads steal from idle ones
- **Efficient** - no OS thread per task

## Async I/O

All stdlib I/O is synchronous. Make it concurrent with `async`:

```typescript
import fs from "std:fs";

// Synchronous - blocks current task
const data = fs.readTextFile("file.txt");

// Concurrent - run in parallel
const task1 = async fs.readTextFile("a.txt");
const task2 = async fs.readTextFile("b.txt");
const a = await task1;
const b = await task2;
```

## Task Methods

```typescript
const task = async doWork();

// Check if done
if (task.isDone()) {
  const result = await task;
}

// Check if cancelled
if (task.isCancelled()) {
  io.writeln("Task was cancelled");
}
```

## Per-Task Nursery Allocator

Each Task has a 64KB bump allocator for short-lived allocations:
- Reduces GC pressure
- Fast allocation
- Auto-freed when task completes

This is handled automatically - no API exposed.

## Goroutine Comparison

| Feature | Go | Raya |
|---------|-----|------|
| Syntax | `go func()` | `async func()` |
| Start | Deferred | Immediate |
| Type Safety | Weak | Strong (static) |
| Scheduler | Work-stealing | Work-stealing |
| Overhead | ~2KB | ~2KB |

## Best Practices

1. **Start tasks early** - they begin immediately
2. **Await late** - let tasks run concurrently
3. **Use discriminated unions for errors** - not exceptions
4. **Don't share mutable state** - use message passing

```typescript
// ✅ Good - concurrent execution
const tasks = [fetchUser(1), fetchUser(2), fetchUser(3)];
const users = await tasks;  // Returns array of results

// ❌ Bad - sequential execution
const users = [
  await fetchUser(1),
  await fetchUser(2),
  await fetchUser(3),
];
```
