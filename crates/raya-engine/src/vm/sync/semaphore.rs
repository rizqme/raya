//! Task-aware Semaphore implementation

use crate::vm::scheduler::TaskId;
use parking_lot::Mutex as ParkingLotMutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Unique identifier for a Semaphore
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct SemaphoreId(u64);

impl SemaphoreId {
    /// Create a new unique semaphore ID
    pub fn new() -> Self {
        use std::sync::atomic::AtomicU64;
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for SemaphoreId {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur when using a Semaphore
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SemaphoreError {
    /// Semaphore not found in registry
    #[error("Semaphore {0:?} not found")]
    NotFound(SemaphoreId),

    /// Invalid permit count
    #[error("Invalid permit count: {0}")]
    InvalidCount(usize),
}

/// Reason why a Task is blocked on a semaphore
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemaphoreBlockReason {
    /// Blocked waiting for semaphore permits
    AwaitingSemaphore(SemaphoreId),
}

/// Task-aware Semaphore with goroutine-style blocking semantics
///
/// Unlike OS-level semaphores that block threads, RayaSemaphore blocks Tasks
/// while allowing the worker thread to continue executing other Tasks.
///
/// A semaphore maintains a count of available permits. Tasks can acquire
/// permits (decrementing the count) and release permits (incrementing the count).
/// When no permits are available, tasks block until a permit is released.
pub struct Semaphore {
    /// Unique semaphore ID
    id: SemaphoreId,

    /// Current number of available permits
    permits: AtomicUsize,

    /// Maximum number of permits (capacity)
    max_permits: usize,

    /// FIFO wait queue of blocked Tasks with their requested permit counts
    wait_queue: ParkingLotMutex<VecDeque<(TaskId, usize)>>,
}

impl Semaphore {
    /// Create a new semaphore with the given number of permits
    pub fn new(id: SemaphoreId, permits: usize) -> Self {
        Self {
            id,
            permits: AtomicUsize::new(permits),
            max_permits: permits,
            wait_queue: ParkingLotMutex::new(VecDeque::new()),
        }
    }

    /// Get the semaphore ID
    pub fn id(&self) -> SemaphoreId {
        self.id
    }

    /// Get the current number of available permits
    pub fn available_permits(&self) -> usize {
        self.permits.load(Ordering::Acquire)
    }

    /// Get the maximum number of permits
    pub fn max_permits(&self) -> usize {
        self.max_permits
    }

    /// Attempt to acquire one or more permits (called from SEM_ACQUIRE opcode)
    ///
    /// Returns:
    /// - Ok(()) if acquired immediately
    /// - Err(SemaphoreBlockReason) if must block
    pub fn try_acquire(&self, task_id: TaskId, count: usize) -> Result<(), SemaphoreBlockReason> {
        if count == 0 {
            return Ok(());
        }

        // Try to acquire permits atomically
        loop {
            let current = self.permits.load(Ordering::Acquire);

            if current >= count {
                // Try to decrement permits
                if self.permits.compare_exchange(
                    current,
                    current - count,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ).is_ok() {
                    return Ok(());
                }
                // CAS failed, retry
            } else {
                // Not enough permits, add to wait queue
                self.wait_queue.lock().push_back((task_id, count));
                return Err(SemaphoreBlockReason::AwaitingSemaphore(self.id));
            }
        }
    }

    /// Release one or more permits (called from SEM_RELEASE opcode)
    ///
    /// Returns a list of tasks that can now be resumed
    pub fn release(&self, count: usize) -> Result<Vec<TaskId>, SemaphoreError> {
        if count == 0 {
            return Ok(Vec::new());
        }

        // Increment permit count
        let new_count = self.permits.fetch_add(count, Ordering::AcqRel) + count;

        // Don't exceed max permits
        if new_count > self.max_permits {
            self.permits.store(self.max_permits, Ordering::Release);
        }

        // Try to wake up waiting tasks
        let mut resumed_tasks = Vec::new();
        let mut queue = self.wait_queue.lock();

        while let Some((waiting_task, needed)) = queue.front().copied() {
            let current = self.permits.load(Ordering::Acquire);

            if current >= needed {
                // Try to acquire for the waiting task
                if self.permits.compare_exchange(
                    current,
                    current - needed,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ).is_ok() {
                    queue.pop_front();
                    resumed_tasks.push(waiting_task);
                }
            } else {
                // Not enough permits for this task, stop checking
                break;
            }
        }

        Ok(resumed_tasks)
    }

    /// Get number of waiting tasks
    pub fn waiting_count(&self) -> usize {
        self.wait_queue.lock().len()
    }

    /// Get the wait queue (for serialization/debugging)
    #[allow(dead_code)]
    pub(crate) fn get_wait_queue(&self) -> Vec<(TaskId, usize)> {
        self.wait_queue.lock().iter().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semaphore_creation() {
        let id = SemaphoreId::new();
        let sem = Semaphore::new(id, 3);

        assert_eq!(sem.id(), id);
        assert_eq!(sem.available_permits(), 3);
        assert_eq!(sem.max_permits(), 3);
        assert_eq!(sem.waiting_count(), 0);
    }

    #[test]
    fn test_semaphore_acquire_release() {
        let sem = Semaphore::new(SemaphoreId::new(), 5);
        let task1 = TaskId::new();
        let task2 = TaskId::new();

        // Acquire 2 permits
        assert!(sem.try_acquire(task1, 2).is_ok());
        assert_eq!(sem.available_permits(), 3);

        // Acquire 2 more permits
        assert!(sem.try_acquire(task2, 2).is_ok());
        assert_eq!(sem.available_permits(), 1);

        // Release 3 permits
        let resumed = sem.release(3).unwrap();
        assert_eq!(sem.available_permits(), 4);
        assert_eq!(resumed.len(), 0); // No waiting tasks

        // Release 1 more (should cap at max_permits)
        sem.release(1).unwrap();
        assert_eq!(sem.available_permits(), 5);
    }

    #[test]
    fn test_semaphore_blocking() {
        let sem = Semaphore::new(SemaphoreId::new(), 2);
        let task1 = TaskId::new();
        let task2 = TaskId::new();
        let task3 = TaskId::new();

        // Acquire all permits
        assert!(sem.try_acquire(task1, 2).is_ok());
        assert_eq!(sem.available_permits(), 0);

        // Next acquire should block
        assert!(sem.try_acquire(task2, 1).is_err());
        assert_eq!(sem.waiting_count(), 1);

        // Another acquire should also block
        assert!(sem.try_acquire(task3, 1).is_err());
        assert_eq!(sem.waiting_count(), 2);

        // Release 1 permit - should wake task2
        let resumed = sem.release(1).unwrap();
        assert_eq!(resumed.len(), 1);
        assert_eq!(resumed[0], task2);
        assert_eq!(sem.waiting_count(), 1);
        assert_eq!(sem.available_permits(), 0);

        // Release 1 more permit - should wake task3
        let resumed = sem.release(1).unwrap();
        assert_eq!(resumed.len(), 1);
        assert_eq!(resumed[0], task3);
        assert_eq!(sem.waiting_count(), 0);
        assert_eq!(sem.available_permits(), 0);
    }

    #[test]
    fn test_semaphore_acquire_multiple() {
        let sem = Semaphore::new(SemaphoreId::new(), 10);
        let task1 = TaskId::new();
        let task2 = TaskId::new();

        // Acquire 5 permits
        assert!(sem.try_acquire(task1, 5).is_ok());
        assert_eq!(sem.available_permits(), 5);

        // Try to acquire 7 permits - should block (only 5 available)
        assert!(sem.try_acquire(task2, 7).is_err());
        assert_eq!(sem.waiting_count(), 1);

        // Release 1 permit - still not enough for task2 (need 7, have 6)
        let resumed = sem.release(1).unwrap();
        assert_eq!(resumed.len(), 0);
        assert_eq!(sem.available_permits(), 6);
        assert_eq!(sem.waiting_count(), 1);

        // Release 1 more permit - now task2 can proceed (have 7)
        let resumed = sem.release(1).unwrap();
        assert_eq!(resumed.len(), 1);
        assert_eq!(resumed[0], task2);
        assert_eq!(sem.available_permits(), 0); // 7 permits acquired by task2
        assert_eq!(sem.waiting_count(), 0);
    }

    #[test]
    fn test_semaphore_zero_count() {
        let sem = Semaphore::new(SemaphoreId::new(), 5);
        let task = TaskId::new();

        // Acquire 0 permits - should succeed immediately
        assert!(sem.try_acquire(task, 0).is_ok());
        assert_eq!(sem.available_permits(), 5);

        // Release 0 permits - should be no-op
        let resumed = sem.release(0).unwrap();
        assert_eq!(resumed.len(), 0);
        assert_eq!(sem.available_permits(), 5);
    }

    #[test]
    fn test_semaphore_fifo_order() {
        let sem = Semaphore::new(SemaphoreId::new(), 1);
        let task1 = TaskId::new();
        let task2 = TaskId::new();
        let task3 = TaskId::new();
        let task4 = TaskId::new();

        // task1 acquires the permit
        assert!(sem.try_acquire(task1, 1).is_ok());

        // task2, task3, task4 all block
        assert!(sem.try_acquire(task2, 1).is_err());
        assert!(sem.try_acquire(task3, 1).is_err());
        assert!(sem.try_acquire(task4, 1).is_err());

        // Release 1 - should wake task2 (FIFO)
        let resumed = sem.release(1).unwrap();
        assert_eq!(resumed.len(), 1);
        assert_eq!(resumed[0], task2);

        // Release 1 - should wake task3
        let resumed = sem.release(1).unwrap();
        assert_eq!(resumed.len(), 1);
        assert_eq!(resumed[0], task3);

        // Release 1 - should wake task4
        let resumed = sem.release(1).unwrap();
        assert_eq!(resumed.len(), 1);
        assert_eq!(resumed[0], task4);
    }
}
