//! Synchronization primitives

use parking_lot::Mutex as ParkingLotMutex;
use crate::scheduler::TaskId;

/// Raya Mutex for task synchronization
pub struct RayaMutex {
    /// Internal mutex
    inner: ParkingLotMutex<()>,
    /// Current owner task ID
    owner: ParkingLotMutex<Option<TaskId>>,
}

impl RayaMutex {
    /// Create a new mutex
    pub fn new() -> Self {
        Self {
            inner: ParkingLotMutex::new(()),
            owner: ParkingLotMutex::new(None),
        }
    }

    /// Lock the mutex for a task
    pub fn lock(&self, task_id: TaskId) -> bool {
        let _guard = self.inner.lock();
        *self.owner.lock() = Some(task_id);
        true
    }

    /// Unlock the mutex
    pub fn unlock(&self, task_id: TaskId) -> bool {
        let mut owner = self.owner.lock();
        if *owner == Some(task_id) {
            *owner = None;
            drop(owner);
            // Unlock happens automatically when guard is dropped
            true
        } else {
            false
        }
    }

    /// Check if locked by a specific task
    pub fn is_locked_by(&self, task_id: TaskId) -> bool {
        *self.owner.lock() == Some(task_id)
    }
}

impl Default for RayaMutex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mutex_lock_unlock() {
        let mutex = RayaMutex::new();
        let task_id = 1;

        assert!(mutex.lock(task_id));
        assert!(mutex.is_locked_by(task_id));
        assert!(mutex.unlock(task_id));
    }
}
