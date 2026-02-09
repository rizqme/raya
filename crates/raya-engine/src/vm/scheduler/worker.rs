//! Worker thread that executes Tasks
//!
//! Workers pick up tasks from the global injector or steal from other workers,
//! then execute them using the TaskInterpreter for proper cooperative scheduling.

use crate::vm::scheduler::{SuspendReason, Task, TaskState};
use crate::vm::vm::{ExecutionResult, SharedVmState, TaskInterpreter};
use crossbeam_deque::{Injector, Stealer, Worker as CWorker};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Worker thread that executes Tasks
pub struct Worker {
    /// Worker ID
    id: usize,

    /// Stealers from other workers
    stealers: Vec<Stealer<Arc<Task>>>,

    /// Shared VM state
    state: Arc<SharedVmState>,

    /// Worker thread handle
    handle: Option<thread::JoinHandle<()>>,

    /// Shutdown signal
    shutdown: Arc<AtomicBool>,
}

impl Worker {
    /// Create a new Worker
    pub fn new(id: usize, stealers: Vec<Stealer<Arc<Task>>>, state: Arc<SharedVmState>) -> Self {
        Self {
            id,
            stealers,
            state,
            handle: None,
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the worker thread
    pub fn start(&mut self) {
        let id = self.id;
        let stealers = self.stealers.clone();
        let state = self.state.clone();
        let shutdown = self.shutdown.clone();

        let handle = thread::Builder::new()
            .name(format!("raya-worker-{}", id))
            .spawn(move || {
                // Create the worker deque on this thread (not Send, so must be created here)
                let worker = CWorker::new_lifo();
                Worker::run_loop(id, worker, stealers, state, shutdown);
            })
            .expect("Failed to spawn worker thread");

        self.handle = Some(handle);
    }

    /// Stop the worker thread
    pub fn stop(&mut self) {
        self.shutdown.store(true, Ordering::Release);

        if let Some(handle) = self.handle.take() {
            handle.join().expect("Failed to join worker thread");
        }
    }

    /// Worker thread main loop
    fn run_loop(
        id: usize,
        worker: CWorker<Arc<Task>>,
        stealers: Vec<Stealer<Arc<Task>>>,
        state: Arc<SharedVmState>,
        shutdown: Arc<AtomicBool>,
    ) {
        loop {
            // Check for shutdown signal
            if shutdown.load(Ordering::Acquire) {
                break;
            }

            // Find work (local pop, steal, or inject)
            let task = match Self::find_work(&worker, &stealers, &state.injector) {
                Some(task) => task,
                None => {
                    // No work available, sleep briefly to avoid busy-waiting
                    // Note: sleeping tasks are handled by the timer thread
                    thread::sleep(Duration::from_micros(100));

                    // Poll safepoint even when idle
                    state.safepoint.poll();
                    continue;
                }
            };

            // Execute task
            task.set_state(TaskState::Running);

            // Record start time for preemption monitoring (like Go)
            task.set_start_time(Instant::now());

            // Create TaskInterpreter with shared state
            let mut interpreter = TaskInterpreter::new(
                &state.gc,
                &state.classes,
                &state.mutex_registry,
                &state.safepoint,
                &state.globals_by_index,
                &state.tasks,
                &state.injector,
                &state.metadata,
                &state.class_metadata,
                &state.native_handler,
            );

            // Execute task using the suspendable interpreter
            let result = interpreter.run(&task);

            // Clear execution time tracking
            task.clear_start_time();

            // Handle execution result
            match result {
                ExecutionResult::Completed(value) => {
                    task.complete(value);

                    // Resume waiting tasks by re-queueing them
                    Self::wake_waiters(&state, &task);
                }
                ExecutionResult::Suspended(reason) => {
                    // Store the suspension reason
                    task.suspend(reason.clone());

                    // Handle specific suspension reasons
                    match reason {
                        SuspendReason::AwaitTask(awaited_id) => {
                            // The awaited task will wake this task when it completes
                            // This is already set up in the TaskInterpreter (add_waiter)
                            #[cfg(debug_assertions)]
                            eprintln!(
                                "Worker {}: Task {} suspended awaiting task {:?}",
                                id,
                                task.id().as_u64(),
                                awaited_id
                            );
                        }
                        SuspendReason::Sleep { wake_at } => {
                            // Register with timer thread for efficient wake-up
                            state.timer.register(task.clone(), wake_at);
                            #[cfg(debug_assertions)]
                            eprintln!(
                                "Worker {}: Task {} sleeping until {:?}",
                                id,
                                task.id().as_u64(),
                                wake_at
                            );
                        }
                        SuspendReason::MutexLock { mutex_id } => {
                            // Task will be woken when mutex is released
                            // The mutex unlock will call wake_waiters
                            #[cfg(debug_assertions)]
                            eprintln!(
                                "Worker {}: Task {} waiting for mutex {:?}",
                                id,
                                task.id().as_u64(),
                                mutex_id
                            );
                        }
                        SuspendReason::ChannelSend { channel_id, .. } => {
                            // Task will be woken when channel has space
                            #[cfg(debug_assertions)]
                            eprintln!(
                                "Worker {}: Task {} waiting to send on channel {}",
                                id,
                                task.id().as_u64(),
                                channel_id
                            );
                        }
                        SuspendReason::ChannelReceive { channel_id } => {
                            // Task will be woken when channel has data
                            #[cfg(debug_assertions)]
                            eprintln!(
                                "Worker {}: Task {} waiting to receive from channel {}",
                                id,
                                task.id().as_u64(),
                                channel_id
                            );
                        }
                    }
                }
                ExecutionResult::Failed(e) => {
                    eprintln!("Worker {}: Task {} failed: {:?}", id, task.id().as_u64(), e);
                    task.fail();

                    // Resume waiting tasks even on failure (they need to see the failure)
                    Self::wake_waiters(&state, &task);
                }
            }
        }

        #[cfg(debug_assertions)]
        eprintln!("Worker {} shutting down", id);
    }

    /// Wake tasks waiting for the completed task
    fn wake_waiters(state: &SharedVmState, task: &Arc<Task>) {
        let waiters = task.take_waiters();
        if !waiters.is_empty() {
            let tasks_map = state.tasks.read();
            let task_failed = task.state() == TaskState::Failed;
            let exception = if task_failed {
                task.current_exception()
            } else {
                None
            };

            for waiter_id in waiters {
                if let Some(waiter_task) = tasks_map.get(&waiter_id) {
                    if task_failed {
                        // Propagate the exception to the waiter task
                        if let Some(exc) = exception {
                            waiter_task.set_exception(exc);
                        } else {
                            // Create a generic exception if none was set
                            waiter_task.set_exception(crate::vm::value::Value::null());
                        }
                    } else {
                        // Set the result as resume value for the waiter
                        if let Some(result) = task.result() {
                            waiter_task.set_resume_value(result);
                        }
                    }
                    // Set waiter to Resumed state so it can be scheduled
                    waiter_task.set_state(TaskState::Resumed);
                    waiter_task.clear_suspend_reason();
                    state.injector.push(waiter_task.clone());
                }
            }
        }
    }

    /// Find work: local pop, then steal, then inject
    fn find_work(
        worker: &CWorker<Arc<Task>>,
        stealers: &[Stealer<Arc<Task>>],
        injector: &Arc<Injector<Arc<Task>>>,
    ) -> Option<Arc<Task>> {
        // 1. Try local deque (LIFO - cache locality)
        if let Some(task) = worker.pop() {
            return Some(task);
        }

        // 2. Try stealing from other workers (FIFO - load balancing)
        loop {
            if let Some(task) = Self::steal_from_others(stealers) {
                return Some(task);
            }

            // 3. Try global injector
            match injector.steal() {
                crossbeam_deque::Steal::Success(task) => return Some(task),
                crossbeam_deque::Steal::Empty => break,
                crossbeam_deque::Steal::Retry => continue,
            }
        }

        None
    }

    /// Steal from other workers
    fn steal_from_others(stealers: &[Stealer<Arc<Task>>]) -> Option<Arc<Task>> {
        use rand::Rng;

        if stealers.is_empty() {
            return None;
        }

        // Randomly select a victim
        let mut rng = rand::thread_rng();
        let start = rng.gen_range(0..stealers.len());

        // Try each stealer starting from random position
        for i in 0..stealers.len() {
            let index = (start + i) % stealers.len();
            let stealer = &stealers[index];

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

    /// Get the worker ID
    pub fn id(&self) -> usize {
        self.id
    }

    /// Check if the worker is running
    pub fn is_running(&self) -> bool {
        self.handle.is_some() && !self.shutdown.load(Ordering::Acquire)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::scheduler::{Task, TaskId};
    use crate::vm::value::Value;
    use crate::vm::vm::SafepointCoordinator;
    use crossbeam_deque::Injector;
    use parking_lot::RwLock;
    use crate::compiler::{Function, Module, Opcode};
    use rustc_hash::FxHashMap;

    fn create_shared_state() -> Arc<SharedVmState> {
        let safepoint = Arc::new(SafepointCoordinator::new(1));
        let tasks = Arc::new(RwLock::new(FxHashMap::default()));
        let injector = Arc::new(Injector::new());
        Arc::new(SharedVmState::new(safepoint, tasks, injector))
    }

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
    fn test_worker_creation() {
        let state = create_shared_state();
        let worker = Worker::new(0, vec![], state);

        assert_eq!(worker.id(), 0);
        assert!(!worker.is_running());
    }

    #[test]
    fn test_worker_start_stop() {
        let state = create_shared_state();
        let mut worker = Worker::new(0, vec![], state);

        worker.start();
        assert!(worker.is_running());

        // Give the worker thread time to start
        thread::sleep(Duration::from_millis(10));

        worker.stop();
        assert!(!worker.is_running());
    }

    #[test]
    fn test_worker_executes_task() {
        let state = create_shared_state();

        // Create task
        let task = create_test_task("test");

        // Push to injector
        state.injector.push(task.clone());

        let mut worker = Worker::new(0, vec![], state);

        worker.start();

        // Wait for task to complete
        thread::sleep(Duration::from_millis(100));

        // Check task completed
        assert_eq!(task.state(), TaskState::Completed);
        assert_eq!(task.result(), Some(Value::i32(42)));

        worker.stop();
    }

    #[test]
    fn test_worker_multiple_tasks() {
        let state = create_shared_state();

        // Create multiple tasks
        let task1 = create_test_task("task1");
        let task2 = create_test_task("task2");
        let task3 = create_test_task("task3");

        // Push to injector
        state.injector.push(task1.clone());
        state.injector.push(task2.clone());
        state.injector.push(task3.clone());

        let mut worker = Worker::new(0, vec![], state);

        worker.start();

        // Wait for tasks to complete
        thread::sleep(Duration::from_millis(200));

        // Check all tasks completed
        assert_eq!(task1.state(), TaskState::Completed);
        assert_eq!(task2.state(), TaskState::Completed);
        assert_eq!(task3.state(), TaskState::Completed);

        worker.stop();
    }

    #[test]
    fn test_worker_shutdown_signal() {
        let state = create_shared_state();
        let mut worker = Worker::new(0, vec![], state);

        worker.start();
        assert!(worker.is_running());

        // Shutdown should stop the worker
        worker.shutdown.store(true, Ordering::Release);
        thread::sleep(Duration::from_millis(50));

        worker.stop();
        assert!(!worker.is_running());
    }
}
