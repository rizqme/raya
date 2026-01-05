//! RAII guard for automatic mutex unlock

use crate::scheduler::TaskId;
use crate::sync::Mutex;
use std::sync::Arc;

/// RAII guard for Mutex (auto-unlocks on drop)
///
/// This guard ensures that a mutex is automatically unlocked when
/// the guard goes out of scope, even in the case of panics.
/// This prevents deadlocks caused by forgetting to unlock.
pub struct MutexGuard<'a> {
    /// Reference to the mutex
    mutex: &'a Mutex,
    /// Task ID that owns the lock
    task_id: TaskId,
    /// Whether the guard has been manually unlocked
    unlocked: bool,
}

impl<'a> MutexGuard<'a> {
    /// Create a new mutex guard
    ///
    /// # Safety
    /// The caller must ensure that the task actually owns the mutex lock.
    pub(crate) fn new(mutex: &'a Mutex, task_id: TaskId) -> Self {
        Self {
            mutex,
            task_id,
            unlocked: false,
        }
    }

    /// Manually unlock the mutex early (before drop)
    ///
    /// Returns the next Task ID to resume, if any.
    pub fn unlock(mut self) -> Result<Option<TaskId>, crate::sync::MutexError> {
        if self.unlocked {
            return Ok(None);
        }
        self.unlocked = true;
        self.mutex.unlock(self.task_id)
    }
}

impl Drop for MutexGuard<'_> {
    fn drop(&mut self) {
        if !self.unlocked {
            // Ignore any errors on drop - we're already cleaning up
            let _ = self.mutex.unlock(self.task_id);
        }
    }
}

/// RAII guard for Arc<Mutex> (owned version)
///
/// This version owns an Arc to the mutex, allowing it to outlive
/// the original mutex reference.
pub struct OwnedMutexGuard {
    /// Arc to the mutex
    mutex: Arc<Mutex>,
    /// Task ID that owns the lock
    task_id: TaskId,
    /// Whether the guard has been manually unlocked
    unlocked: bool,
}

impl OwnedMutexGuard {
    /// Create a new owned mutex guard
    ///
    /// # Safety
    /// The caller must ensure that the task actually owns the mutex lock.
    pub fn new(mutex: Arc<Mutex>, task_id: TaskId) -> Self {
        Self {
            mutex,
            task_id,
            unlocked: false,
        }
    }

    /// Manually unlock the mutex early (before drop)
    ///
    /// Returns the next Task ID to resume, if any.
    pub fn unlock(mut self) -> Result<Option<TaskId>, crate::sync::MutexError> {
        if self.unlocked {
            return Ok(None);
        }
        self.unlocked = true;
        self.mutex.unlock(self.task_id)
    }

    /// Get a reference to the mutex
    pub fn mutex(&self) -> &Arc<Mutex> {
        &self.mutex
    }
}

impl Drop for OwnedMutexGuard {
    fn drop(&mut self) {
        if !self.unlocked {
            // Ignore any errors on drop - we're already cleaning up
            let _ = self.mutex.unlock(self.task_id);
        }
    }
}

impl Mutex {
    /// Lock with RAII guard (borrowed version)
    ///
    /// This is a convenience method that combines try_lock with guard creation.
    /// Note: This will block if the mutex is already locked. For VM usage,
    /// prefer using the LOCK opcode which integrates with the scheduler.
    pub fn lock_guard(&self, task_id: TaskId) -> Result<MutexGuard<'_>, crate::sync::BlockReason> {
        self.try_lock(task_id)?;
        Ok(MutexGuard::new(self, task_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::MutexId;

    #[test]
    fn test_mutex_guard_auto_unlock() {
        let mutex = Mutex::new(MutexId::new());
        let task_id = TaskId::new();

        {
            let _guard = mutex.lock_guard(task_id).unwrap();
            assert!(mutex.is_locked());
            assert!(mutex.is_locked_by(task_id));
        } // Guard dropped here

        // Mutex should be automatically unlocked
        assert!(!mutex.is_locked());
    }

    #[test]
    fn test_mutex_guard_manual_unlock() {
        let mutex = Mutex::new(MutexId::new());
        let task_id = TaskId::new();

        let guard = mutex.lock_guard(task_id).unwrap();
        assert!(mutex.is_locked());

        // Manual unlock
        let next = guard.unlock().unwrap();
        assert_eq!(next, None);
        assert!(!mutex.is_locked());
    }

    #[test]
    fn test_mutex_guard_unlock_resumes_waiter() {
        let mutex = Mutex::new(MutexId::new());
        let task1 = TaskId::new();
        let task2 = TaskId::new();

        // task1 locks
        let guard = mutex.lock_guard(task1).unwrap();

        // task2 tries to lock (will be queued)
        let result = mutex.try_lock(task2);
        assert!(result.is_err());
        assert_eq!(mutex.waiting_count(), 1);

        // task1 unlocks via guard drop
        drop(guard);

        // task2 should now own the lock
        assert!(mutex.is_locked_by(task2));
        assert_eq!(mutex.waiting_count(), 0);
    }

    #[test]
    fn test_owned_mutex_guard() {
        let mutex = Arc::new(Mutex::new(MutexId::new()));
        let task_id = TaskId::new();

        {
            mutex.try_lock(task_id).unwrap();
            let _guard = OwnedMutexGuard::new(mutex.clone(), task_id);
            assert!(mutex.is_locked());
        } // Guard dropped here

        // Mutex should be automatically unlocked
        assert!(!mutex.is_locked());
    }

    #[test]
    fn test_owned_mutex_guard_manual_unlock() {
        let mutex = Arc::new(Mutex::new(MutexId::new()));
        let task_id = TaskId::new();

        mutex.try_lock(task_id).unwrap();
        let guard = OwnedMutexGuard::new(mutex.clone(), task_id);

        assert!(mutex.is_locked());

        let next = guard.unlock().unwrap();
        assert_eq!(next, None);
        assert!(!mutex.is_locked());
    }

    #[test]
    fn test_guard_prevents_double_unlock() {
        let mutex = Mutex::new(MutexId::new());
        let task_id = TaskId::new();

        let guard = mutex.lock_guard(task_id).unwrap();

        // Manual unlock
        guard.unlock().unwrap();

        // Guard drop should not try to unlock again
        // (no panic or error)
    }
}
