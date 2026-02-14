//! Main task scheduler coordinating worker threads

use crate::vm::scheduler::{
    PreemptMonitor, Task, TaskId, TaskState, Worker, DEFAULT_PREEMPT_THRESHOLD,
};
use crate::vm::interpreter::{SafepointCoordinator, SharedVmState};
use crossbeam_deque::{Injector, Worker as CWorker};
use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use std::sync::Arc;

/// Scheduler statistics
#[derive(Debug, Clone, Default)]
pub struct SchedulerStats {
    /// Total tasks spawned
    pub tasks_spawned: u64,

    /// Total tasks completed
    pub tasks_completed: u64,

    /// Currently active tasks
    pub active_tasks: usize,
}

/// Resource limits for sub-schedulers (inner VMs)
#[derive(Debug, Clone, Default)]
pub struct SchedulerLimits {
    /// Maximum worker threads (None = share parent's workers)
    pub max_workers: Option<usize>,

    /// Maximum concurrent running tasks (None = unlimited)
    pub max_concurrent_tasks: Option<usize>,

    /// Maximum stack size per task in bytes (None = unlimited)
    pub max_stack_size: Option<usize>,

    /// Maximum heap size in bytes (None = unlimited)
    pub max_heap_size: Option<usize>,
}

impl SchedulerLimits {
    /// Create limits for a restricted inner VM (1 worker, limited concurrency and resources)
    pub fn restricted() -> Self {
        Self {
            max_workers: Some(1),
            max_concurrent_tasks: Some(10), // Max 10 tasks running concurrently
            max_stack_size: Some(1024 * 1024), // 1MB stack per task
            max_heap_size: Some(10 * 1024 * 1024), // 10MB heap total
        }
    }
}

/// Main task scheduler
pub struct Scheduler {
    /// Worker threads
    workers: Vec<Worker>,

    /// Shared VM state (contains tasks, injector, safepoint, GC, classes, globals)
    shared_state: Arc<SharedVmState>,

    /// Preemption monitor (like Go's sysmon)
    preempt_monitor: PreemptMonitor,

    /// Number of worker threads
    worker_count: usize,

    /// Whether the scheduler has been started
    started: bool,

    /// Resource limits (for sub-schedulers)
    limits: SchedulerLimits,
}

impl Scheduler {
    /// Create a new scheduler with the specified number of workers
    /// If worker_count is 0, defaults to the number of CPU cores
    pub fn new(worker_count: usize) -> Self {
        let count = if worker_count == 0 {
            num_cpus::get()
        } else {
            worker_count
        };
        Self::with_limits(count, SchedulerLimits::default())
    }

    /// Create a new scheduler with a custom native handler
    pub fn with_native_handler(worker_count: usize, native_handler: Arc<dyn crate::vm::NativeHandler>) -> Self {
        Self::with_limits_and_handler(worker_count, SchedulerLimits::default(), native_handler)
    }

    /// Create a new scheduler with resource limits (for sub-schedulers)
    pub fn with_limits(worker_count: usize, limits: SchedulerLimits) -> Self {
        Self::with_limits_and_handler(worker_count, limits, Arc::new(crate::vm::NoopNativeHandler))
    }

    /// Create a new scheduler with resource limits and a custom native handler
    pub fn with_limits_and_handler(worker_count: usize, limits: SchedulerLimits, native_handler: Arc<dyn crate::vm::NativeHandler>) -> Self {
        // Apply worker limit if specified
        let actual_worker_count = limits
            .max_workers
            .map(|max| worker_count.min(max))
            .unwrap_or(worker_count);

        let safepoint = Arc::new(SafepointCoordinator::new(actual_worker_count));
        let injector = Arc::new(Injector::new());
        let tasks = Arc::new(RwLock::new(FxHashMap::default()));

        // Create shared VM state
        let shared_state = Arc::new(SharedVmState::with_native_handler(
            safepoint.clone(),
            tasks.clone(),
            injector.clone(),
            native_handler,
        ));

        // Create worker deques to get stealers
        let mut worker_deques = Vec::new();
        let mut stealers = Vec::new();

        for _ in 0..actual_worker_count {
            let worker = CWorker::new_lifo();
            stealers.push(worker.stealer());
            worker_deques.push(worker);
        }

        // Note: worker_deques are dropped here since they're not Send
        // Each worker will create its own deque on its own thread
        drop(worker_deques);

        // Create workers with shared state
        let mut workers = Vec::new();
        for id in 0..actual_worker_count {
            // Get stealers from other workers (exclude self)
            let other_stealers: Vec<_> = stealers
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != id)
                .map(|(_, s)| s.clone())
                .collect();

            let worker = Worker::new(id, other_stealers, shared_state.clone());
            workers.push(worker);
        }

        // Create preemption monitor (like Go's sysmon)
        let preempt_monitor = PreemptMonitor::new(tasks, DEFAULT_PREEMPT_THRESHOLD);

        Self {
            workers,
            shared_state,
            preempt_monitor,
            worker_count: actual_worker_count,
            started: false,
            limits,
        }
    }

    /// Create a sub-scheduler for an inner VM with specified limits
    pub fn new_sub_scheduler(limits: SchedulerLimits) -> Self {
        // Sub-schedulers get 1 worker by default, unless overridden
        let worker_count = limits.max_workers.unwrap_or(1);
        Self::with_limits(worker_count, limits)
    }

    /// Start all worker threads and preemption monitor
    pub fn start(&mut self) {
        if self.started {
            return;
        }

        // Start workers
        for worker in &mut self.workers {
            worker.start();
        }

        // Start preemption monitor (like Go's sysmon)
        self.preempt_monitor.start();

        self.started = true;
    }

    /// Spawn a new task
    ///
    /// Returns None if the concurrent task limit is reached
    pub fn spawn(&self, task: Arc<Task>) -> Option<TaskId> {
        // Check concurrent task limit
        if let Some(max_concurrent) = self.limits.max_concurrent_tasks {
            let tasks = self.shared_state.tasks.read();
            let running_count = tasks
                .values()
                .filter(|t| {
                    let state = t.state();
                    state == crate::vm::scheduler::TaskState::Running
                        || state == crate::vm::scheduler::TaskState::Created
                })
                .count();

            if running_count >= max_concurrent {
                return None; // Limit reached
            }
        }

        let task_id = task.id();

        // Register task
        self.shared_state.tasks.write().insert(task_id, task.clone());

        // Push to global injector
        self.shared_state.injector.push(task);

        Some(task_id)
    }

    /// Get a task by ID
    pub fn get_task(&self, task_id: TaskId) -> Option<Arc<Task>> {
        self.shared_state.tasks.read().get(&task_id).cloned()
    }

    /// Remove a completed task from the registry
    pub fn remove_task(&self, task_id: TaskId) -> Option<Arc<Task>> {
        self.shared_state.tasks.write().remove(&task_id)
    }

    /// Number of active tasks
    pub fn task_count(&self) -> usize {
        self.shared_state.tasks.read().len()
    }

    /// Number of workers
    pub fn worker_count(&self) -> usize {
        self.worker_count
    }

    /// Get the safepoint coordinator
    pub fn safepoint(&self) -> &Arc<SafepointCoordinator> {
        &self.shared_state.safepoint
    }

    /// Get the shared VM state
    pub fn shared_state(&self) -> &Arc<SharedVmState> {
        &self.shared_state
    }

    /// Check if the scheduler has been started
    pub fn is_started(&self) -> bool {
        self.started
    }

    /// Shutdown the scheduler
    pub fn shutdown(&mut self) {
        if !self.started {
            return;
        }

        // Stop preemption monitor first
        self.preempt_monitor.stop();

        // Stop all workers
        for worker in &mut self.workers {
            worker.stop();
        }

        self.started = false;

        // Clear task registry
        self.shared_state.tasks.write().clear();
    }

    /// Wait for all tasks to complete (with timeout)
    pub fn wait_all(&self, timeout: std::time::Duration) -> bool {
        let start = std::time::Instant::now();

        loop {
            // Check if all tasks are completed or failed
            let all_done = {
                let tasks = self.shared_state.tasks.read();
                tasks.values().all(|task| {
                    let state = task.state();
                    state == crate::vm::scheduler::TaskState::Completed
                        || state == crate::vm::scheduler::TaskState::Failed
                })
            };

            if all_done {
                return true;
            }

            if start.elapsed() > timeout {
                return false;
            }

            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// Block a Task on a mutex
    ///
    /// This is called when a Task tries to acquire a mutex that is already locked.
    /// The Task's state is set to Suspended and it will not be scheduled until resumed.
    pub fn block_on_mutex(&self, task_id: TaskId, _mutex_id: crate::vm::sync::MutexId) {
        if let Some(task) = self.get_task(task_id) {
            task.set_state(TaskState::Suspended);
        }
    }

    /// Resume a Task that was blocked on a mutex
    ///
    /// This is called when a mutex is unlocked and the next waiting Task should be resumed.
    /// The Task's state is set to Resumed and it will be scheduled for execution.
    pub fn resume_from_mutex(&self, task_id: TaskId) {
        if let Some(task) = self.get_task(task_id) {
            task.set_state(TaskState::Resumed);
            // Push task back to global injector so it can be picked up by a worker
            self.shared_state.injector.push(task);
        }
    }

    /// Get scheduler statistics
    pub fn stats(&self) -> SchedulerStats {
        let tasks = self.shared_state.tasks.read();

        // Count active tasks by state
        let active_tasks = tasks
            .values()
            .filter(|task| {
                matches!(
                    task.state(),
                    TaskState::Created | TaskState::Running | TaskState::Resumed
                )
            })
            .count();

        SchedulerStats {
            tasks_spawned: 0,   // TODO: Track this with atomic counter
            tasks_completed: 0, // TODO: Track this with atomic counter
            active_tasks,
        }
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        let worker_count = num_cpus::get();
        Self::new(worker_count)
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::scheduler::TaskState;
    use crate::compiler::{Function, Module, Opcode};
    use std::thread;
    use std::time::Duration;

    fn create_test_task(name: &str) -> Arc<Task> {
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: name.to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![Opcode::ConstI32 as u8, 42, 0, 0, 0, Opcode::Return as u8],
        });

        Arc::new(Task::new(0, Arc::new(module), None))
    }

    #[test]
    fn test_scheduler_creation() {
        let scheduler = Scheduler::new(4);
        assert_eq!(scheduler.worker_count(), 4);
        assert_eq!(scheduler.task_count(), 0);
        assert!(!scheduler.is_started());
    }

    #[test]
    fn test_scheduler_default() {
        let scheduler = Scheduler::default();
        assert_eq!(scheduler.worker_count(), num_cpus::get());
    }

    #[test]
    fn test_scheduler_start() {
        let mut scheduler = Scheduler::new(2);
        assert!(!scheduler.is_started());

        scheduler.start();
        assert!(scheduler.is_started());

        // Starting again should be idempotent
        scheduler.start();
        assert!(scheduler.is_started());

        scheduler.shutdown();
    }

    #[test]
    fn test_scheduler_spawn_task() {
        let scheduler = Scheduler::new(2);

        let task = create_test_task("test");
        let task_id = scheduler.spawn(task.clone()).expect("Failed to spawn task");

        assert_eq!(task_id, task.id());
        assert_eq!(scheduler.task_count(), 1);

        let retrieved = scheduler.get_task(task_id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id(), task_id);
    }

    #[test]
    fn test_scheduler_task_execution() {
        let mut scheduler = Scheduler::new(2);
        scheduler.start();

        let task = create_test_task("test");
        scheduler.spawn(task.clone()).expect("Failed to spawn task");

        // Wait for task to complete
        thread::sleep(Duration::from_millis(100));

        assert_eq!(task.state(), TaskState::Completed);

        scheduler.shutdown();
    }

    #[test]
    fn test_scheduler_multiple_tasks() {
        let mut scheduler = Scheduler::new(2);
        scheduler.start();

        let mut task_ids = Vec::new();
        for i in 0..10 {
            let task = create_test_task(&format!("task{}", i));
            let task_id = scheduler.spawn(task).expect("Failed to spawn task");
            task_ids.push(task_id);
        }

        // Wait for all tasks to complete
        thread::sleep(Duration::from_millis(200));

        // Check all tasks completed
        for task_id in task_ids {
            if let Some(task) = scheduler.get_task(task_id) {
                assert_eq!(task.state(), TaskState::Completed);
            }
        }

        scheduler.shutdown();
    }

    #[test]
    fn test_scheduler_remove_task() {
        let scheduler = Scheduler::new(2);

        let task = create_test_task("test");
        let task_id = scheduler.spawn(task.clone()).expect("Failed to spawn task");

        assert_eq!(scheduler.task_count(), 1);

        let removed = scheduler.remove_task(task_id);
        assert!(removed.is_some());
        assert_eq!(scheduler.task_count(), 0);

        let not_found = scheduler.get_task(task_id);
        assert!(not_found.is_none());
    }

    #[test]
    fn test_scheduler_shutdown() {
        let mut scheduler = Scheduler::new(2);
        scheduler.start();

        let task = create_test_task("test");
        scheduler.spawn(task);

        assert!(scheduler.is_started());
        assert!(scheduler.task_count() > 0);

        scheduler.shutdown();

        assert!(!scheduler.is_started());
        assert_eq!(scheduler.task_count(), 0);
    }

    #[test]
    fn test_scheduler_wait_all() {
        let mut scheduler = Scheduler::new(2);
        scheduler.start();

        // Spawn tasks
        for i in 0..5 {
            let task = create_test_task(&format!("task{}", i));
            scheduler.spawn(task);
        }

        // Wait for all tasks to complete (they should complete quickly)
        let completed = scheduler.wait_all(Duration::from_secs(1));
        assert!(completed);

        scheduler.shutdown();
    }

    #[test]
    fn test_scheduler_wait_all_timeout() {
        let scheduler = Scheduler::new(2);
        // Don't start the scheduler, so tasks won't complete

        let task = create_test_task("test");
        scheduler.spawn(task);

        // Should timeout since scheduler isn't running
        let completed = scheduler.wait_all(Duration::from_millis(100));
        assert!(!completed);
    }

    #[test]
    fn test_scheduler_safepoint_access() {
        let scheduler = Scheduler::new(2);
        let safepoint = scheduler.safepoint();

        assert_eq!(safepoint.worker_count(), 2);
    }
}
