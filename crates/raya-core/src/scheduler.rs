//! Task scheduler for goroutine-style concurrency

use std::sync::Arc;
use parking_lot::Mutex;

/// Task identifier
pub type TaskId = usize;

/// Task status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Task is ready to run
    Ready,
    /// Task is currently running
    Running,
    /// Task is blocked (awaiting)
    Blocked,
    /// Task has completed successfully
    Completed,
    /// Task has failed with an error
    Failed,
}

/// A Task (green thread)
pub struct Task {
    /// Unique task ID
    pub id: TaskId,
    /// Current status
    pub status: TaskStatus,
}

/// Multi-threaded task scheduler
pub struct Scheduler {
    /// All tasks
    tasks: Arc<Mutex<rustc_hash::FxHashMap<TaskId, Task>>>,
    /// Next task ID
    next_task_id: Arc<Mutex<TaskId>>,
    /// Number of worker threads
    num_workers: usize,
}

impl Scheduler {
    /// Create a new scheduler
    pub fn new() -> Self {
        let num_workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        Self {
            tasks: Arc::new(Mutex::new(rustc_hash::FxHashMap::default())),
            next_task_id: Arc::new(Mutex::new(0)),
            num_workers,
        }
    }

    /// Spawn a new task
    pub fn spawn(&self) -> TaskId {
        let mut next_id = self.next_task_id.lock();
        let task_id = *next_id;
        *next_id += 1;

        let task = Task {
            id: task_id,
            status: TaskStatus::Ready,
        };

        self.tasks.lock().insert(task_id, task);
        task_id
    }

    /// Get number of workers
    pub fn num_workers(&self) -> usize {
        self.num_workers
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_creation() {
        let scheduler = Scheduler::new();
        assert!(scheduler.num_workers() > 0);
    }

    #[test]
    fn test_task_spawning() {
        let scheduler = Scheduler::new();
        let task1 = scheduler.spawn();
        let task2 = scheduler.spawn();
        assert_ne!(task1, task2);
    }
}
