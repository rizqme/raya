# VM Sync

This folder implements synchronization primitives for Raya tasks. These are runtime-level coordination tools, not OS-thread locks for Rust internals.

## What This Folder Owns

- Task-aware mutexes and guards.
- Task-aware semaphores.
- Global registries used to look them up by id at runtime.
- Serialization support for snapshot/restore paths.

## File Guide

- `mutex.rs` and `guard.rs`: mutex behavior and guard types.
- `semaphore.rs`: semaphore behavior and blocking semantics.
- `registry.rs`: registries for mutexes and semaphores.
- `mutex_id.rs`: id types and generation.
- `serialize.rs`: snapshot-facing serialized sync state.

## Start Here When

- Locking or semaphore behavior is wrong.
- A task blocks or wakes incorrectly around synchronization primitives.
- Snapshot restoration needs to preserve sync state.

## Read Next

- Scheduler that suspends/resumes blocked tasks: [`../scheduler/CLAUDE.md`](../scheduler/CLAUDE.md)
- Interpreter integration: [`../interpreter/CLAUDE.md`](../interpreter/CLAUDE.md)
