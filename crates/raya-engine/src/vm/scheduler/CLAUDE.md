# scheduler module

Unified reactor with VM + IO worker pools for Raya's goroutine-style concurrency.

## Overview

The scheduler uses a **single-threaded reactor** pattern: one reactor thread runs the control loop, dispatching tasks to a VM worker pool (bytecode execution) and an IO worker pool (blocking operations). This replaces the previous multi-threaded work-stealing scheduler.

## Module Structure

```
scheduler/
├── mod.rs        # Public API, re-exports
├── scheduler.rs  # Scheduler struct (spawns reactor)
├── reactor.rs    # Unified reactor loop (single control thread)
└── task.rs       # Task struct, state machine
```

## Architecture

```
                    ┌──────────────┐
          ┌────────►│   Reactor    │◄────────┐
          │         │  (1 thread)  │         │
          │         └──────┬───────┘         │
          │                │                 │
    VmResult          Dispatch          IoCompletion
          │                │                 │
          ▲                ▼                 ▲
  ┌───────┴───────┐  ready_queue   ┌────────┴────────┐
  │  VM Workers   │               │   IO Workers    │
  │  (N threads)  │               │   (M threads)   │
  │  run bytecode │               │  blocking work  │
  └───────────────┘               └─────────────────┘
```

## Reactor Loop (9 steps per iteration)

1. **Drain VM results** — process completed/suspended/failed tasks
2. **Drain IO submissions** — route BlockingWork to IO pool
3. **Drain IO completions** — resume tasks with IO results
4. **Check timers** — wake sleep-expired tasks from timer_heap
5. **Retry channel waiters** — 3-phase: try buffer, pair match, re-queue
6. **Drain injector** — pick up spawned/woken tasks (e.g., from MutexUnlock)
7. **Check preemption** — request preempt on long-running tasks
8. **Dispatch ready tasks** — send from ready_queue to VM workers
9. **Select/wait** — block briefly for next event

## Suspend Reasons & Wakeup Ownership

| Reason | Woken by | Notes |
|--------|----------|-------|
| AwaitTask | Reactor (wake_waiters) | When awaited task completes/fails |
| Sleep | Reactor (timer_heap, Step 4) | Timer-based wake |
| MutexLock | **VM Worker** (MutexUnlock opcode) | Uses `try_suspend` to avoid race |
| ChannelSend/Receive | Reactor (Step 5) | 3-phase channel waiter retry |
| IoWait | Reactor (Step 3) | IO pool completion |

**MutexLock race fix:** MutexUnlock on a VM worker may wake a task before the reactor processes its Suspended VmResult. `Task::try_suspend()` only transitions Running→Suspended, skipping if already Resumed.

## Task States

```
Ready → Running → Suspended → Resumed → Running → ... → Completed/Failed
```

- `Running`: set by VM worker on dispatch
- `Suspended`: set by reactor in handle_vm_result (or try_suspend for MutexLock)
- `Resumed`: set by wakeup source (reactor or VM worker for MutexUnlock)

## IO Integration (BlockingWork)

Native handlers return `NativeCallResult::Suspend(IoRequest::BlockingWork { work })` to offload blocking IO to the IO pool. The reactor routes the work, and when the IO pool returns `IoCompletion`, the reactor converts it to a `Value` and resumes the task via `complete_task`.

## For AI Assistants

- **Single reactor thread** — all scheduling decisions happen here (no races between scheduler operations)
- **MutexLock is special** — only suspend reason where wakeup happens on VM workers, not reactor. Uses `try_suspend()`.
- The `injector` (crossbeam deque) is used for spawned tasks and mutex-woken tasks
- `active_vm_tasks` tracks how many tasks are on VM workers (for backpressure)
- Timer heap uses `BinaryHeap<SleepEntry>` with `Reverse`-like ordering (earliest wake first)
- Channel waiters use 3-phase resolution: try buffer ops, pair senders with receivers, re-queue unresolved
