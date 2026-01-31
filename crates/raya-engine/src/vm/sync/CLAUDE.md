# sync module

Task-aware synchronization primitives for Raya.

## Overview

Provides synchronization primitives that work with the task scheduler, blocking tasks (not OS threads) when contended.

## Module Structure

```
sync/
├── mod.rs     # Re-exports
└── mutex.rs   # Mutex implementation
```

## Mutex

### MutexId
```rust
#[derive(Copy, Clone, Eq, Hash)]
pub struct MutexId(u32);
```

### Mutex
```rust
pub struct Mutex {
    id: MutexId,
    locked: AtomicBool,
    owner: Option<TaskId>,
    wait_queue: VecDeque<TaskId>,
}

mutex.lock(task_id) -> Result<MutexGuard, MutexError>
mutex.try_lock(task_id) -> Option<MutexGuard>
mutex.unlock(task_id) -> Result<(), MutexError>
```

### MutexGuard
```rust
pub struct MutexGuard<'a> {
    mutex: &'a Mutex,
    task_id: TaskId,
}

impl Drop for MutexGuard {
    fn drop(&mut self) {
        self.mutex.unlock(self.task_id).unwrap();
    }
}
```

### MutexRegistry
```rust
pub struct MutexRegistry {
    mutexes: HashMap<MutexId, Mutex>,
    next_id: AtomicU32,
}

registry.create() -> MutexId
registry.get(id) -> Option<&Mutex>
registry.lock(id, task_id) -> Result<(), MutexError>
registry.unlock(id, task_id) -> Option<TaskId>  // Returns next waiter
```

## Task-Aware Blocking

When a task tries to lock a held mutex:

```
Task A                    Task B
   │                         │
   ▼                         │
lock(mutex) ────────────►   │
   │                         │
   ▼                         │
[Acquired]                   │
   │                         ▼
   │                    lock(mutex)
   │                         │
   │                         ▼
   │                    [Blocked]
   │                    (task parked)
   │                         │
unlock(mutex) ◄─────────    │
   │                         │
   │    (resume Task B)      │
   │                         ▼
   │                    [Acquired]
```

The scheduler handles this:
```rust
scheduler.block_on_mutex(task_id, mutex_id)
scheduler.resume_from_mutex(task_id)
```

## Error Handling

```rust
pub enum MutexError {
    NotOwner,           // Unlock by non-owner
    DoubleLock,         // Same task locking twice
    NotFound,           // Invalid mutex ID
    Poisoned,           // Owner task panicked
}
```

## Bytecode Integration

```
// lock(mutex)
LOAD_LOCAL 0          // Load mutex reference
NATIVE_CALL MUTEX_LOCK

// Critical section
...

// unlock(mutex)
LOAD_LOCAL 0
NATIVE_CALL MUTEX_UNLOCK
```

## Snapshot Support

Mutexes are serialized in snapshots:
```rust
pub struct MutexSnapshot {
    id: MutexId,
    locked: bool,
    owner: Option<TaskId>,
    waiters: Vec<TaskId>,
}
```

## For AI Assistants

- Mutexes block tasks, NOT OS threads
- FIFO wait queue for fairness
- MutexGuard provides RAII unlock
- Scheduler integration for blocking/resuming
- No reader-writer locks yet (Mutex only)
- Mutex IDs are stable across snapshots
- Deadlock detection is NOT implemented
