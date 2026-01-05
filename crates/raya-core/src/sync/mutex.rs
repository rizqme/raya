//! Task-aware Mutex implementation

use crate::scheduler::TaskId;
use crate::sync::MutexId;
use crossbeam::atomic::AtomicCell;
use parking_lot::Mutex as ParkingLotMutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Errors that can occur when using a Mutex
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MutexError {
    /// Mutex not found in registry
    #[error("Mutex {0:?} not found")]
    NotFound(MutexId),

    /// Unlock called by non-owner Task
    #[error("Unlock called by non-owner Task {0:?}")]
    NotOwner(TaskId),

    /// Mutex already locked by the same Task (reentrant lock attempt)
    #[error("Mutex already locked by Task {0:?}")]
    AlreadyLocked(TaskId),
}

/// Reason why a Task is blocked
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockReason {
    /// Blocked waiting for a mutex
    AwaitingMutex(MutexId),
}

/// Task-aware Mutex with goroutine-style blocking semantics
///
/// Unlike OS-level mutexes that block threads, RayaMutex blocks Tasks
/// while allowing the worker thread to continue executing other Tasks.
pub struct Mutex {
    /// Unique mutex ID
    id: MutexId,

    /// Current owner Task (None if unlocked)
    owner: AtomicCell<Option<TaskId>>,

    /// FIFO wait queue of blocked Tasks
    wait_queue: ParkingLotMutex<VecDeque<TaskId>>,

    /// Lock count (for detecting reentrant locks - always 0 or 1)
    lock_count: AtomicUsize,
}

impl Mutex {
    /// Create a new mutex with unique ID
    pub fn new(id: MutexId) -> Self {
        Self {
            id,
            owner: AtomicCell::new(None),
            wait_queue: ParkingLotMutex::new(VecDeque::new()),
            lock_count: AtomicUsize::new(0),
        }
    }

    /// Get the mutex ID
    pub fn id(&self) -> MutexId {
        self.id
    }

    /// Attempt to lock the mutex (called from LOCK opcode)
    ///
    /// Returns:
    /// - Ok(()) if acquired immediately
    /// - Err(BlockReason::AwaitingMutex) if must block
    pub fn try_lock(&self, task_id: TaskId) -> Result<(), BlockReason> {
        // Check for reentrant lock attempt
        if let Some(current_owner) = self.owner.load() {
            if current_owner == task_id {
                // Same task trying to lock again - this is an error
                // We don't support reentrant locks
                return Err(BlockReason::AwaitingMutex(self.id));
            }
        }

        // Try to acquire the lock using compare-and-swap
        match self.owner.compare_exchange(None, Some(task_id)) {
            Ok(_) => {
                // Successfully acquired the lock
                self.lock_count.store(1, Ordering::Release);
                Ok(())
            }
            Err(_) => {
                // Lock is held by another task, must block
                // Add this task to wait queue
                self.wait_queue.lock().push_back(task_id);
                Err(BlockReason::AwaitingMutex(self.id))
            }
        }
    }

    /// Unlock the mutex (called from UNLOCK opcode)
    ///
    /// Returns:
    /// - Ok(Some(task_id)) if there's a waiting task to resume
    /// - Ok(None) if no waiting tasks
    /// - Err(MutexError) if unlock failed
    pub fn unlock(&self, task_id: TaskId) -> Result<Option<TaskId>, MutexError> {
        // Verify the caller owns the mutex
        match self.owner.load() {
            Some(owner) if owner == task_id => {
                // Clear owner and lock count
                self.lock_count.store(0, Ordering::Release);
                self.owner.store(None);

                // Check if there are waiting tasks
                let mut queue = self.wait_queue.lock();
                if let Some(next_task) = queue.pop_front() {
                    // Transfer ownership to the next waiting task
                    self.owner.store(Some(next_task));
                    self.lock_count.store(1, Ordering::Release);
                    Ok(Some(next_task))
                } else {
                    // No waiting tasks
                    Ok(None)
                }
            }
            Some(_other_owner) => Err(MutexError::NotOwner(task_id)),
            None => Err(MutexError::NotOwner(task_id)),
        }
    }

    /// Check current owner
    pub fn owner(&self) -> Option<TaskId> {
        self.owner.load()
    }

    /// Get number of waiting Tasks
    pub fn waiting_count(&self) -> usize {
        self.wait_queue.lock().len()
    }

    /// Check if the mutex is locked
    pub fn is_locked(&self) -> bool {
        self.owner.load().is_some()
    }

    /// Check if locked by a specific task
    pub fn is_locked_by(&self, task_id: TaskId) -> bool {
        self.owner.load() == Some(task_id)
    }

    /// Get the wait queue (for serialization)
    pub(crate) fn get_wait_queue(&self) -> Vec<TaskId> {
        self.wait_queue.lock().iter().copied().collect()
    }

    /// Serialize the mutex state
    pub fn serialize(&self) -> crate::sync::SerializedMutex {
        crate::sync::SerializedMutex {
            mutex_id: self.id(),
            owner: self.owner(),
            wait_queue: self.get_wait_queue(),
        }
    }

    /// Deserialize and restore mutex state
    pub fn deserialize(data: crate::sync::SerializedMutex) -> Self {
        use crossbeam::atomic::AtomicCell;
        use parking_lot::Mutex as ParkingLotMutex;
        use std::collections::VecDeque;
        use std::sync::atomic::AtomicUsize;

        Self {
            id: data.mutex_id,
            owner: AtomicCell::new(data.owner),
            wait_queue: ParkingLotMutex::new(data.wait_queue.into_iter().collect::<VecDeque<_>>()),
            lock_count: AtomicUsize::new(if data.owner.is_some() { 1 } else { 0 }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mutex_creation() {
        let id = MutexId::new();
        let mutex = Mutex::new(id);

        assert_eq!(mutex.id(), id);
        assert!(!mutex.is_locked());
        assert_eq!(mutex.owner(), None);
        assert_eq!(mutex.waiting_count(), 0);
    }

    #[test]
    fn test_mutex_uncontended_lock_unlock() {
        let mutex = Mutex::new(MutexId::new());
        let task_id = TaskId::new();

        // Lock should succeed
        assert!(mutex.try_lock(task_id).is_ok());
        assert!(mutex.is_locked());
        assert!(mutex.is_locked_by(task_id));
        assert_eq!(mutex.owner(), Some(task_id));

        // Unlock should succeed
        let next = mutex.unlock(task_id).unwrap();
        assert_eq!(next, None);
        assert!(!mutex.is_locked());
        assert_eq!(mutex.owner(), None);
    }

    #[test]
    fn test_mutex_reentrant_error() {
        let mutex = Mutex::new(MutexId::new());
        let task_id = TaskId::new();

        // First lock succeeds
        assert!(mutex.try_lock(task_id).is_ok());

        // Second lock by same task should fail (we don't support reentrant locks)
        assert!(mutex.try_lock(task_id).is_err());
    }

    #[test]
    fn test_mutex_unlock_non_owner() {
        let mutex = Mutex::new(MutexId::new());
        let task1 = TaskId::new();
        let task2 = TaskId::new();

        // task1 locks
        mutex.try_lock(task1).unwrap();

        // task2 tries to unlock - should fail
        let result = mutex.unlock(task2);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), MutexError::NotOwner(task2));
    }

    #[test]
    fn test_mutex_wait_queue_fifo() {
        let mutex = Mutex::new(MutexId::new());
        let task1 = TaskId::new();
        let task2 = TaskId::new();
        let task3 = TaskId::new();

        // task1 locks
        assert!(mutex.try_lock(task1).is_ok());

        // task2 and task3 try to lock - should block and be queued
        assert!(mutex.try_lock(task2).is_err());
        assert!(mutex.try_lock(task3).is_err());

        assert_eq!(mutex.waiting_count(), 2);

        // task1 unlocks - task2 should get the lock
        let next = mutex.unlock(task1).unwrap();
        assert_eq!(next, Some(task2));
        assert!(mutex.is_locked_by(task2));
        assert_eq!(mutex.waiting_count(), 1);

        // task2 unlocks - task3 should get the lock
        let next = mutex.unlock(task2).unwrap();
        assert_eq!(next, Some(task3));
        assert!(mutex.is_locked_by(task3));
        assert_eq!(mutex.waiting_count(), 0);

        // task3 unlocks - no more waiters
        let next = mutex.unlock(task3).unwrap();
        assert_eq!(next, None);
        assert!(!mutex.is_locked());
    }

    #[test]
    fn test_mutex_unlock_without_lock() {
        let mutex = Mutex::new(MutexId::new());
        let task_id = TaskId::new();

        // Unlock without lock should fail
        let result = mutex.unlock(task_id);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), MutexError::NotOwner(task_id));
    }
}
