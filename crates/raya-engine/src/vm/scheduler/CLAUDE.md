# VM Scheduler

This folder schedules Raya tasks across VM workers and IO workers. It is responsible for making concurrency features actually run without blocking the entire runtime.

## What This Folder Owns

- Task lifecycle and task ids/states.
- Reactor event loop.
- Submission of blocking work to IO workers.
- Worker-count and resource-limit policy.
- Preemption thresholds and scheduling fairness hooks.

## File Guide

- `scheduler.rs`: public scheduler API and limits.
- `reactor.rs`: central event loop and IO submission handling.
- `pool.rs`: worker/stack pool support.
- `task.rs`: task structures, states, suspend reasons, and handlers.

## Start Here When

- Tasks deadlock, starve, or fail to resume.
- Blocking work runs on the wrong pool.
- Concurrency limits or preemption behavior need adjustment.
- Task bookkeeping is inconsistent.

## Read Next

- VM execution side: [`../interpreter/CLAUDE.md`](../interpreter/CLAUDE.md)
- Sync primitives that suspend tasks: [`../sync/CLAUDE.md`](../sync/CLAUDE.md)
- GC root registration interactions: [`../gc/CLAUDE.md`](../gc/CLAUDE.md)
