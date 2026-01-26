//! Work-stealing deque for task scheduling

use crate::scheduler::Task;
use crossbeam_deque::{Injector, Stealer, Worker};
use std::sync::Arc;

/// Work-stealing deque for a single worker
pub struct WorkerDeque {
    /// Local worker deque (LIFO for own tasks)
    worker: Worker<Arc<Task>>,

    /// Stealer handles for other workers
    stealers: Vec<Stealer<Arc<Task>>>,

    /// Global injector for tasks without affinity
    injector: Arc<Injector<Arc<Task>>>,
}

impl WorkerDeque {
    /// Create a new WorkerDeque
    pub fn new(
        worker: Worker<Arc<Task>>,
        stealers: Vec<Stealer<Arc<Task>>>,
        injector: Arc<Injector<Arc<Task>>>,
    ) -> Self {
        Self {
            worker,
            stealers,
            injector,
        }
    }

    /// Push a task to the local deque (LIFO)
    pub fn push(&self, task: Arc<Task>) {
        self.worker.push(task);
    }

    /// Pop a task from the local deque (LIFO) - most recent task
    pub fn pop(&self) -> Option<Arc<Task>> {
        self.worker.pop()
    }

    /// Try to get work: local pop, then steal, then inject
    pub fn find_work(&self) -> Option<Arc<Task>> {
        // 1. Try local deque (LIFO - cache locality)
        if let Some(task) = self.worker.pop() {
            return Some(task);
        }

        // 2. Try stealing from other workers (FIFO - load balancing)
        loop {
            if let Some(task) = self.steal_from_others() {
                return Some(task);
            }

            // 3. Try global injector
            match self.injector.steal() {
                crossbeam_deque::Steal::Success(task) => return Some(task),
                crossbeam_deque::Steal::Empty => break,
                crossbeam_deque::Steal::Retry => continue,
            }
        }

        None
    }

    /// Steal from other workers (FIFO from their deque bottom)
    fn steal_from_others(&self) -> Option<Arc<Task>> {
        use rand::Rng;

        if self.stealers.is_empty() {
            return None;
        }

        // Randomly select a victim to reduce contention
        let mut rng = rand::thread_rng();
        let start = rng.gen_range(0..self.stealers.len());

        // Try each stealer starting from random position
        for i in 0..self.stealers.len() {
            let index = (start + i) % self.stealers.len();
            let stealer = &self.stealers[index];

            // Retry loop for stealing (handle concurrent modifications)
            loop {
                match stealer.steal() {
                    crossbeam_deque::Steal::Success(task) => return Some(task),
                    crossbeam_deque::Steal::Empty => break,
                    crossbeam_deque::Steal::Retry => continue,
                }
            }
        }

        None
    }

    /// Get the number of tasks in the local deque (approximate)
    pub fn len(&self) -> usize {
        // Note: This is an approximation due to concurrent access
        let mut count = 0;
        let mut temp = Vec::new();

        // Pop all items to count
        while let Some(task) = self.worker.pop() {
            count += 1;
            temp.push(task);
        }

        // Push them back
        for task in temp.into_iter().rev() {
            self.worker.push(task);
        }

        count
    }

    /// Check if the local deque is empty (approximate)
    pub fn is_empty(&self) -> bool {
        self.worker.is_empty()
    }
}

// Implement Clone to allow sharing between worker threads
impl Clone for WorkerDeque {
    fn clone(&self) -> Self {
        // Note: We can't actually clone the Worker, so we create a new one
        // This is only used for sharing the stealers and injector
        // The actual worker deque is thread-local
        panic!("WorkerDeque cannot be cloned - each worker has its own deque");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::Task;
    use raya_compiler::{Function, Module, Opcode};

    fn create_test_task(name: &str) -> Arc<Task> {
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: name.to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![Opcode::Return as u8],
        });

        Arc::new(Task::new(0, Arc::new(module), None))
    }

    #[test]
    fn test_worker_deque_push_pop() {
        let worker = Worker::new_lifo();
        let injector = Arc::new(Injector::new());
        let deque = WorkerDeque::new(worker, vec![], injector);

        let task1 = create_test_task("task1");
        let task2 = create_test_task("task2");

        deque.push(task1.clone());
        deque.push(task2.clone());

        // LIFO order - last pushed comes out first
        let popped2 = deque.pop().unwrap();
        assert_eq!(popped2.id(), task2.id());

        let popped1 = deque.pop().unwrap();
        assert_eq!(popped1.id(), task1.id());

        assert!(deque.pop().is_none());
    }

    #[test]
    fn test_worker_deque_empty() {
        let worker = Worker::new_lifo();
        let injector = Arc::new(Injector::new());
        let deque = WorkerDeque::new(worker, vec![], injector);

        assert!(deque.is_empty());
        assert!(deque.pop().is_none());

        let task = create_test_task("task");
        deque.push(task);

        assert!(!deque.is_empty());
    }

    #[test]
    fn test_worker_deque_stealing() {
        // Create two workers
        let worker1 = Worker::new_lifo();
        let worker2 = Worker::new_lifo();

        let stealer1 = worker1.stealer();
        let stealer2 = worker2.stealer();

        let injector = Arc::new(Injector::new());

        // Worker 1 can steal from worker 2
        let deque1 = WorkerDeque::new(worker1, vec![stealer2.clone()], injector.clone());
        let deque2 = WorkerDeque::new(worker2, vec![stealer1], injector);

        // Push tasks to worker 2
        let task1 = create_test_task("task1");
        let task2 = create_test_task("task2");
        deque2.push(task1.clone());
        deque2.push(task2.clone());

        // Worker 1 should be able to steal
        let stolen = deque1.find_work();
        assert!(stolen.is_some());

        // Stolen task should be task1 (FIFO for stealing - from bottom)
        let stolen_task = stolen.unwrap();
        assert_eq!(stolen_task.id(), task1.id());
    }

    #[test]
    fn test_worker_deque_global_injector() {
        let worker = Worker::new_lifo();
        let injector = Arc::new(Injector::new());
        let deque = WorkerDeque::new(worker, vec![], injector.clone());

        // Push task to global injector
        let task = create_test_task("task");
        injector.push(task.clone());

        // Worker should find it via find_work
        let found = deque.find_work();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id(), task.id());
    }

    #[test]
    fn test_worker_deque_find_work_priority() {
        // Test that find_work prioritizes: local > steal > inject
        let worker1 = Worker::new_lifo();
        let worker2 = Worker::new_lifo();
        let stealer2 = worker2.stealer();
        let injector = Arc::new(Injector::new());

        let deque1 = WorkerDeque::new(worker1, vec![stealer2], injector.clone());

        // Add tasks to all three sources
        let local_task = create_test_task("local");
        let steal_task = create_test_task("steal");
        let inject_task = create_test_task("inject");

        deque1.push(local_task.clone());
        worker2.push(steal_task.clone());
        injector.push(inject_task.clone());

        // Should get local task first
        let found = deque1.find_work().unwrap();
        assert_eq!(found.id(), local_task.id());

        // Next should get stolen task
        let found = deque1.find_work().unwrap();
        assert_eq!(found.id(), steal_task.id());

        // Finally should get injected task
        let found = deque1.find_work().unwrap();
        assert_eq!(found.id(), inject_task.id());
    }

    #[test]
    fn test_worker_deque_len() {
        let worker = Worker::new_lifo();
        let injector = Arc::new(Injector::new());
        let deque = WorkerDeque::new(worker, vec![], injector);

        assert_eq!(deque.len(), 0);

        let task1 = create_test_task("task1");
        let task2 = create_test_task("task2");
        let task3 = create_test_task("task3");

        deque.push(task1);
        deque.push(task2);
        deque.push(task3);

        // Note: len() is approximate but should be 3
        assert_eq!(deque.len(), 3);

        deque.pop();
        assert_eq!(deque.len(), 2);
    }
}
