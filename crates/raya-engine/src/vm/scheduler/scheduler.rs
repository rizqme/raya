//! Main task scheduler — wraps Reactor with the same public API

use crate::vm::scheduler::{Reactor, Task, TaskId, TaskState};
use crate::vm::interpreter::SharedVmState;
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
#[derive(Debug, Clone)]
pub struct SchedulerLimits {
    /// Maximum worker threads (None = share parent's workers)
    pub max_workers: Option<usize>,
    /// Maximum concurrent running tasks (None = unlimited)
    pub max_concurrent_tasks: Option<usize>,
    /// Maximum stack size per task in bytes (None = unlimited)
    pub max_stack_size: Option<usize>,
    /// Maximum heap size in bytes (None = unlimited)
    pub max_heap_size: Option<usize>,
    /// Maximum consecutive preemptions before killing a task. Default: 1000.
    pub max_preemptions: u32,
    /// Preemption time slice in milliseconds. Default: 10ms.
    pub preempt_threshold_ms: u64,
}

impl Default for SchedulerLimits {
    fn default() -> Self {
        Self {
            max_workers: None,
            max_concurrent_tasks: None,
            max_stack_size: None,
            max_heap_size: None,
            max_preemptions: 1000,
            preempt_threshold_ms: 10,
        }
    }
}

impl SchedulerLimits {
    /// Create limits for a restricted inner VM
    pub fn restricted() -> Self {
        Self {
            max_workers: Some(1),
            max_concurrent_tasks: Some(10),
            max_stack_size: Some(1024 * 1024),
            max_heap_size: Some(10 * 1024 * 1024),
            ..Default::default()
        }
    }
}

/// Main task scheduler
pub struct Scheduler {
    /// Internal reactor
    reactor: Reactor,

    /// Shared VM state
    shared_state: Arc<SharedVmState>,

    /// Number of VM worker threads
    worker_count: usize,

    /// Whether the scheduler has been started
    started: bool,

    /// Resource limits
    limits: SchedulerLimits,
}

impl Scheduler {
    /// Create a new scheduler with the specified number of VM workers.
    /// If worker_count is 0, defaults to the number of CPU cores.
    pub fn new(worker_count: usize) -> Self {
        let count = if worker_count == 0 {
            num_cpus::get()
        } else {
            worker_count
        };
        Self::with_limits(count, SchedulerLimits::default())
    }

    /// Create a new scheduler with a custom native handler
    pub fn with_native_handler(
        worker_count: usize,
        native_handler: Arc<dyn crate::vm::NativeHandler>,
    ) -> Self {
        Self::with_limits_and_handler(worker_count, SchedulerLimits::default(), native_handler)
    }

    /// Create a new scheduler with resource limits
    pub fn with_limits(worker_count: usize, limits: SchedulerLimits) -> Self {
        Self::with_limits_and_handler(
            worker_count,
            limits,
            Arc::new(crate::vm::NoopNativeHandler),
        )
    }

    /// Create a new scheduler with resource limits and a custom native handler
    pub fn with_limits_and_handler(
        worker_count: usize,
        limits: SchedulerLimits,
        native_handler: Arc<dyn crate::vm::NativeHandler>,
    ) -> Self {
        let actual_worker_count = limits
            .max_workers
            .map(|max| worker_count.min(max))
            .unwrap_or(worker_count);

        let safepoint = Arc::new(crate::vm::interpreter::SafepointCoordinator::new(
            actual_worker_count,
        ));
        let injector = Arc::new(crossbeam_deque::Injector::new());
        let tasks = Arc::new(parking_lot::RwLock::new(rustc_hash::FxHashMap::default()));

        let mut state = SharedVmState::with_native_handler(
            safepoint,
            tasks,
            injector,
            native_handler,
        );
        state.max_preemptions = limits.max_preemptions;
        state.preempt_threshold_ms = limits.preempt_threshold_ms;
        let shared_state = Arc::new(state);

        let io_worker_count = std::env::var("RAYA_IO_THREADS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(4);

        let reactor = Reactor::new(actual_worker_count, io_worker_count, shared_state.clone());

        Self {
            reactor,
            shared_state,
            worker_count: actual_worker_count,
            started: false,
            limits,
        }
    }

    /// Create a sub-scheduler for an inner VM with specified limits
    pub fn new_sub_scheduler(limits: SchedulerLimits) -> Self {
        let worker_count = limits.max_workers.unwrap_or(1);
        Self::with_limits(worker_count, limits)
    }

    /// Start the reactor and all worker pools
    pub fn start(&mut self) {
        if self.started {
            return;
        }
        self.reactor.start();
        self.started = true;
    }

    /// Spawn a new task. Returns None if the concurrent task limit is reached.
    pub fn spawn(&self, task: Arc<Task>) -> Option<TaskId> {
        if let Some(max_concurrent) = self.limits.max_concurrent_tasks {
            let tasks = self.shared_state.tasks.read();
            let running_count = tasks
                .values()
                .filter(|t| {
                    let state = t.state();
                    state == TaskState::Running || state == TaskState::Created
                })
                .count();
            if running_count >= max_concurrent {
                return None;
            }
        }

        let task_id = task.id();

        // Register task
        self.shared_state
            .tasks
            .write()
            .insert(task_id, task.clone());

        // Push to global injector — reactor will drain it
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

    /// Number of VM workers
    pub fn worker_count(&self) -> usize {
        self.worker_count
    }

    /// Get the safepoint coordinator
    pub fn safepoint(&self) -> &Arc<crate::vm::interpreter::SafepointCoordinator> {
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
        self.reactor.shutdown();
        self.started = false;
    }

    /// Wait for all tasks to complete (with timeout)
    pub fn wait_all(&self, timeout: std::time::Duration) -> bool {
        let start = std::time::Instant::now();
        loop {
            let all_done = {
                let tasks = self.shared_state.tasks.read();
                tasks.values().all(|task| {
                    let state = task.state();
                    state == TaskState::Completed || state == TaskState::Failed
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
    pub fn block_on_mutex(&self, task_id: TaskId, _mutex_id: crate::vm::sync::MutexId) {
        if let Some(task) = self.get_task(task_id) {
            task.set_state(TaskState::Suspended);
        }
    }

    /// Resume a Task that was blocked on a mutex
    pub fn resume_from_mutex(&self, task_id: TaskId) {
        if let Some(task) = self.get_task(task_id) {
            task.set_state(TaskState::Resumed);
            self.shared_state.injector.push(task);
        }
    }

    /// Get scheduler statistics
    pub fn stats(&self) -> SchedulerStats {
        let tasks = self.shared_state.tasks.read();
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
            tasks_spawned: 0,
            tasks_completed: 0,
            active_tasks,
        }
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new(num_cpus::get())
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
            register_count: 0,
            reg_code: Vec::new(),
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
        scheduler.start(); // idempotent
        assert!(scheduler.is_started());
        scheduler.shutdown();
    }

    #[test]
    fn test_scheduler_spawn_task() {
        let scheduler = Scheduler::new(2);
        let task = create_test_task("test");
        let task_id = scheduler.spawn(task.clone()).expect("Failed to spawn");
        assert_eq!(task_id, task.id());
        assert_eq!(scheduler.task_count(), 1);
        let retrieved = scheduler.get_task(task_id);
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_scheduler_task_execution() {
        let mut scheduler = Scheduler::new(2);
        scheduler.start();
        let task = create_test_task("test");
        scheduler.spawn(task.clone()).expect("Failed to spawn");
        thread::sleep(Duration::from_millis(200));
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
            let task_id = scheduler.spawn(task).expect("Failed to spawn");
            task_ids.push(task_id);
        }
        thread::sleep(Duration::from_millis(500));
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
        let task_id = scheduler.spawn(task.clone()).expect("Failed to spawn");
        assert_eq!(scheduler.task_count(), 1);
        let removed = scheduler.remove_task(task_id);
        assert!(removed.is_some());
        assert_eq!(scheduler.task_count(), 0);
    }

    #[test]
    fn test_scheduler_shutdown() {
        let mut scheduler = Scheduler::new(2);
        scheduler.start();
        let task = create_test_task("test");
        scheduler.spawn(task);
        assert!(scheduler.is_started());
        scheduler.shutdown();
        assert!(!scheduler.is_started());
        assert_eq!(scheduler.task_count(), 0);
    }

    #[test]
    fn test_scheduler_wait_all() {
        let mut scheduler = Scheduler::new(2);
        scheduler.start();
        for i in 0..5 {
            let task = create_test_task(&format!("task{}", i));
            scheduler.spawn(task);
        }
        let completed = scheduler.wait_all(Duration::from_secs(2));
        assert!(completed);
        scheduler.shutdown();
    }

    #[test]
    fn test_scheduler_wait_all_timeout() {
        let scheduler = Scheduler::new(2);
        // Don't start — tasks won't complete
        let task = create_test_task("test");
        scheduler.spawn(task);
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
