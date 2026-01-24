//! Worker thread that executes Tasks

use crate::scheduler::{Task, TaskId, TaskState};
use crate::value::Value;
use crate::vm::SafepointCoordinator;
use crate::{VmError, VmResult};
use crossbeam_deque::{Injector, Stealer, Worker as CWorker};
use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Worker thread that executes Tasks
pub struct Worker {
    /// Worker ID
    id: usize,

    /// Stealers from other workers
    stealers: Vec<Stealer<Arc<Task>>>,

    /// Global injector
    injector: Arc<Injector<Arc<Task>>>,

    /// Task registry (shared with scheduler)
    tasks: Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,

    /// Safepoint coordinator
    safepoint: Arc<SafepointCoordinator>,

    /// Worker thread handle
    handle: Option<thread::JoinHandle<()>>,

    /// Shutdown signal
    shutdown: Arc<AtomicBool>,
}

impl Worker {
    /// Create a new Worker
    pub fn new(
        id: usize,
        stealers: Vec<Stealer<Arc<Task>>>,
        injector: Arc<Injector<Arc<Task>>>,
        tasks: Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,
        safepoint: Arc<SafepointCoordinator>,
    ) -> Self {
        Self {
            id,
            stealers,
            injector,
            tasks,
            safepoint,
            handle: None,
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the worker thread
    pub fn start(&mut self) {
        let id = self.id;
        let stealers = self.stealers.clone();
        let injector = self.injector.clone();
        let tasks = self.tasks.clone();
        let safepoint = self.safepoint.clone();
        let shutdown = self.shutdown.clone();

        let handle = thread::Builder::new()
            .name(format!("raya-worker-{}", id))
            .spawn(move || {
                // Create the worker deque on this thread (not Send, so must be created here)
                let worker = CWorker::new_lifo();
                Worker::run_loop(id, worker, stealers, injector, tasks, safepoint, shutdown);
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
        injector: Arc<Injector<Arc<Task>>>,
        tasks: Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,
        safepoint: Arc<SafepointCoordinator>,
        shutdown: Arc<AtomicBool>,
    ) {
        loop {
            // Check for shutdown signal
            if shutdown.load(Ordering::Acquire) {
                break;
            }

            // Find work (local pop, steal, or inject)
            let task = match Self::find_work(&worker, &stealers, &injector) {
                Some(task) => task,
                None => {
                    // No work available, sleep briefly to avoid busy-waiting
                    thread::sleep(Duration::from_micros(100));

                    // Poll safepoint even when idle
                    safepoint.poll();
                    continue;
                }
            };

            // Execute task
            task.set_state(TaskState::Running);

            // Record start time for preemption monitoring (like Go)
            task.set_start_time(std::time::Instant::now());

            match Self::execute_task(&task, &injector, &tasks, &safepoint) {
                Ok(result) => {
                    // Clear execution time tracking
                    task.clear_start_time();

                    task.complete(result);

                    // Resume waiting tasks
                    let waiters = task.take_waiters();
                    if !waiters.is_empty() {
                        // TODO: Resume waiter tasks by re-queueing them
                        // This will be implemented when we integrate with the scheduler
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "Worker {}: Task {} completed with {} waiters",
                            id,
                            task.id().as_u64(),
                            waiters.len()
                        );
                    }
                }
                Err(VmError::TaskPreempted) => {
                    // Clear execution time tracking
                    task.clear_start_time();

                    // Re-queue the task for execution
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "Worker {}: Task {} preempted, re-queueing",
                        id,
                        task.id().as_u64()
                    );

                    // Put it back in the Created state so it can be rescheduled
                    task.set_state(TaskState::Created);
                    injector.push(task.clone());
                }
                Err(e) => {
                    // Clear execution time tracking
                    task.clear_start_time();

                    eprintln!("Worker {}: Task {} failed: {:?}", id, task.id().as_u64(), e);
                    task.fail();
                }
            }

            // Check if preemption was requested
            if task.is_preempt_requested() {
                task.clear_preempt();
                #[cfg(debug_assertions)]
                eprintln!(
                    "Worker {}: Task {} yielded after preemption",
                    id,
                    task.id().as_u64()
                );
                // Task will be rescheduled by being pushed back to deque
                // (This happens naturally when we get the next task)
            }
        }

        #[cfg(debug_assertions)]
        eprintln!("Worker {} shutting down", id);
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

    /// Execute a task's bytecode
    fn execute_task(
        task: &Task,
        injector: &Arc<Injector<Arc<Task>>>,
        tasks: &Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,
        safepoint: &Arc<SafepointCoordinator>,
    ) -> VmResult<Value> {
        use crate::gc::GarbageCollector;
        use crate::vm::ClassRegistry;
        use crate::VmError;
        use raya_bytecode::Opcode;

        // Get the function to execute
        let module = task.module();
        let func_index = task.function_id();

        if func_index >= module.functions.len() {
            return Err(VmError::RuntimeError(format!(
                "Invalid function index: {}",
                func_index
            )));
        }

        let function = &module.functions[func_index];
        let code = &function.code;

        // Create temporary execution context
        // Note: This is a simplified version - full implementation will use shared GC/Classes
        let _gc = GarbageCollector::default();
        let _classes = ClassRegistry::new();

        // Use task's stack
        let stack = task.stack();
        let mut stack_guard = stack.lock().unwrap();

        // Allocate space for local variables (push null values)
        for _ in 0..function.local_count {
            stack_guard.push(Value::null())?;
        }
        let locals_base = stack_guard.depth() - function.local_count;

        // Get/set instruction pointer
        let mut ip = task.ip();

        // Main execution loop
        loop {
            // Poll safepoint regularly
            safepoint.poll();

            // Check for asynchronous preemption (like Go)
            if task.is_preempt_requested() {
                // Clear preemption flag
                task.clear_preempt();

                #[cfg(debug_assertions)]
                eprintln!("Task {} preempted at safepoint", task.id().as_u64());

                // Yield task - save state and return
                task.set_ip(ip);
                drop(stack_guard);
                return Err(VmError::TaskPreempted);
            }

            if ip >= code.len() {
                break;
            }

            let opcode_byte = code[ip];
            ip += 1;

            let opcode = Opcode::from_u8(opcode_byte).ok_or(VmError::InvalidOpcode(opcode_byte))?;

            match opcode {
                Opcode::Return => {
                    // Return the top value
                    let result = if stack_guard.is_empty() {
                        Value::null()
                    } else {
                        stack_guard.pop()?
                    };

                    task.set_ip(ip);
                    return Ok(result);
                }

                Opcode::ConstI32 => {
                    let value =
                        i32::from_le_bytes([code[ip], code[ip + 1], code[ip + 2], code[ip + 3]]);
                    ip += 4;
                    stack_guard.push(Value::i32(value))?;
                }

                Opcode::Iadd => {
                    let b = stack_guard
                        .pop()?
                        .as_i32()
                        .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
                    let a = stack_guard
                        .pop()?
                        .as_i32()
                        .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
                    stack_guard.push(Value::i32(a + b))?;
                }

                Opcode::Imul => {
                    let b = stack_guard
                        .pop()?
                        .as_i32()
                        .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
                    let a = stack_guard
                        .pop()?
                        .as_i32()
                        .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
                    stack_guard.push(Value::i32(a * b))?;
                }

                Opcode::LoadLocal => {
                    let index = u16::from_le_bytes([code[ip], code[ip + 1]]) as usize;
                    ip += 2;

                    // Get local variable (directly from stack at locals_base + index)
                    let value = stack_guard.peek_at(locals_base + index)?;
                    stack_guard.push(value)?;
                }

                Opcode::StoreLocal => {
                    let index = u16::from_le_bytes([code[ip], code[ip + 1]]) as usize;
                    ip += 2;

                    let value = stack_guard.pop()?;
                    stack_guard.set_at(locals_base + index, value)?;
                }

                Opcode::Ilt => {
                    let b = stack_guard
                        .pop()?
                        .as_i32()
                        .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
                    let a = stack_guard
                        .pop()?
                        .as_i32()
                        .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
                    stack_guard.push(Value::bool(a < b))?;
                }

                Opcode::Jmp => {
                    let offset = i16::from_le_bytes([code[ip], code[ip + 1]]) as isize;
                    ip += 2;

                    // Backward jump - poll safepoint
                    if offset < 0 {
                        safepoint.poll();

                        // Check preemption at backward jumps (loop headers)
                        if task.is_preempt_requested() {
                            task.clear_preempt();
                            task.set_ip(ip);
                            drop(stack_guard);
                            return Err(VmError::RuntimeError(
                                "Task preempted at loop header".to_string(),
                            ));
                        }
                    }

                    ip = (ip as isize + offset) as usize;
                }

                Opcode::JmpIfFalse => {
                    let offset = i16::from_le_bytes([code[ip], code[ip + 1]]) as isize;
                    ip += 2;

                    let condition = stack_guard.pop()?;
                    if !condition.as_bool().unwrap_or(false) {
                        ip = (ip as isize + offset) as usize;
                    }
                }

                Opcode::Nop => {
                    // No operation - just continue
                }

                // SPAWN - Create and start a new task
                Opcode::Spawn => {
                    // Read function index (u16)
                    let func_index = u16::from_le_bytes([code[ip], code[ip + 1]]) as usize;
                    ip += 2;

                    // Create new task
                    let new_task = Arc::new(Task::new(
                        func_index,
                        module.clone(),
                        Some(task.id()), // This task is the parent
                    ));

                    let task_id = new_task.id();

                    // Register task in registry
                    tasks.write().insert(task_id, new_task.clone());

                    // Push to global injector for scheduling
                    injector.push(new_task);

                    // Push TaskId as u64 onto stack
                    stack_guard.push(Value::u64(task_id.as_u64()))?;
                }

                // AWAIT - Wait for a task to complete
                Opcode::Await => {
                    // Pop TaskId from stack
                    let task_id_val = stack_guard.pop()?;
                    let task_id_u64 = task_id_val.as_u64().ok_or_else(|| {
                        VmError::TypeError("Expected TaskId (u64) for AWAIT".to_string())
                    })?;

                    let awaited_task_id = TaskId::from_u64(task_id_u64);

                    // Poll for task completion
                    loop {
                        // Get the awaited task
                        let awaited_task =
                            tasks.read().get(&awaited_task_id).cloned().ok_or_else(|| {
                                VmError::RuntimeError(format!(
                                    "Task {:?} not found",
                                    awaited_task_id
                                ))
                            })?;

                        let state = awaited_task.state();

                        match state {
                            TaskState::Completed => {
                                // Get result and push onto stack
                                let result = awaited_task.result().unwrap_or(Value::null());
                                stack_guard.push(result)?;
                                break; // Continue execution
                            }
                            TaskState::Failed => {
                                return Err(VmError::RuntimeError(format!(
                                    "Awaited task {:?} failed",
                                    awaited_task_id
                                )));
                            }
                            _ => {
                                // Task still running - poll safepoint and yield briefly
                                drop(stack_guard); // Release lock while waiting
                                safepoint.poll();
                                thread::sleep(Duration::from_micros(100));
                                stack_guard = stack.lock().unwrap(); // Reacquire
                            }
                        }
                    }
                }

                _ => {
                    return Err(VmError::RuntimeError(format!(
                        "Opcode {:?} not implemented in task executor",
                        opcode
                    )));
                }
            }
        }

        // If we exit the loop without returning, return null
        task.set_ip(ip);
        Ok(Value::null())
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
    use crate::scheduler::Task;
    use crossbeam_deque::Injector;
    use raya_bytecode::{Function, Module, Opcode};

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
        let injector = Arc::new(Injector::new());
        let tasks = Arc::new(RwLock::new(FxHashMap::default()));
        let safepoint = Arc::new(SafepointCoordinator::new(1));

        let worker = Worker::new(0, vec![], injector, tasks, safepoint);

        assert_eq!(worker.id(), 0);
        assert!(!worker.is_running());
    }

    #[test]
    fn test_worker_start_stop() {
        let injector = Arc::new(Injector::new());
        let tasks = Arc::new(RwLock::new(FxHashMap::default()));
        let safepoint = Arc::new(SafepointCoordinator::new(1));

        let mut worker = Worker::new(0, vec![], injector, tasks, safepoint);

        worker.start();
        assert!(worker.is_running());

        // Give the worker thread time to start
        thread::sleep(Duration::from_millis(10));

        worker.stop();
        assert!(!worker.is_running());
    }

    #[test]
    fn test_worker_executes_task() {
        let injector = Arc::new(Injector::new());
        let tasks = Arc::new(RwLock::new(FxHashMap::default()));
        let safepoint = Arc::new(SafepointCoordinator::new(1));

        // Create task
        let task = create_test_task("test");

        // Push to injector
        injector.push(task.clone());

        let mut worker = Worker::new(0, vec![], injector, tasks, safepoint);

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
        let injector = Arc::new(Injector::new());
        let tasks = Arc::new(RwLock::new(FxHashMap::default()));
        let safepoint = Arc::new(SafepointCoordinator::new(1));

        // Create multiple tasks
        let task1 = create_test_task("task1");
        let task2 = create_test_task("task2");
        let task3 = create_test_task("task3");

        // Push to injector
        injector.push(task1.clone());
        injector.push(task2.clone());
        injector.push(task3.clone());

        let mut worker = Worker::new(0, vec![], injector, tasks, safepoint);

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
        let injector = Arc::new(Injector::new());
        let tasks = Arc::new(RwLock::new(FxHashMap::default()));
        let safepoint = Arc::new(SafepointCoordinator::new(1));

        let mut worker = Worker::new(0, vec![], injector, tasks, safepoint);

        worker.start();
        assert!(worker.is_running());

        // Shutdown should stop the worker
        worker.shutdown.store(true, Ordering::Release);
        thread::sleep(Duration::from_millis(50));

        worker.stop();
        assert!(!worker.is_running());
    }
}
