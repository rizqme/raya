//! Task structure and execution state

use crate::stack::Stack;
use crate::sync::MutexId;
use crate::value::Value;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Unique identifier for a Task
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct TaskId(u64);

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

impl TaskId {
    /// Generate a new unique TaskId
    pub fn new() -> Self {
        TaskId(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Get the numeric ID value
    pub fn as_u64(self) -> u64 {
        self.0
    }

    /// Create a TaskId from a u64 value
    pub fn from_u64(id: u64) -> Self {
        TaskId(id)
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

/// State of a Task
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TaskState {
    /// Just created, not yet scheduled
    Created,
    /// Currently executing on a worker
    Running,
    /// Suspended waiting for another Task
    Suspended,
    /// Ready to run (was suspended, now resumed)
    Resumed,
    /// Completed with a result
    Completed,
    /// Failed with an error
    Failed,
}

/// Exception handler entry for try-catch-finally blocks
#[derive(Debug, Clone)]
pub struct ExceptionHandler {
    /// Bytecode offset to catch block (-1 if no catch)
    pub catch_offset: i32,

    /// Bytecode offset to finally block (-1 if no finally)
    pub finally_offset: i32,

    /// Stack size when handler was installed (for unwinding)
    pub stack_size: usize,

    /// Call frame count when handler was installed (for unwinding)
    pub frame_count: usize,

    /// Number of mutexes held when handler was installed (for auto-unlock on unwind)
    pub mutex_count: usize,
}

/// A lightweight green thread
pub struct Task {
    /// Unique identifier
    id: TaskId,

    /// Current state
    state: Mutex<TaskState>,

    /// Function to execute
    function_id: usize,

    /// Module containing the function
    module: Arc<raya_bytecode::Module>,

    /// Execution stack
    stack: Mutex<Stack>,

    /// Instruction pointer
    ip: AtomicUsize,

    /// Result value (if completed)
    result: Mutex<Option<Value>>,

    /// Tasks waiting for this Task to complete
    waiters: Mutex<Vec<TaskId>>,

    /// Parent task (if spawned from another Task)
    parent: Option<TaskId>,

    /// Asynchronous preemption flag (like Go's preemption)
    preempt_requested: AtomicBool,

    /// When this task started executing (for preemption monitoring)
    start_time: Mutex<Option<Instant>>,

    /// Exception handler stack (for try-catch-finally)
    exception_handlers: Mutex<Vec<ExceptionHandler>>,

    /// Currently thrown exception (if any)
    current_exception: Mutex<Option<Value>>,

    /// Mutexes currently held by this Task (for auto-unlock on exception)
    held_mutexes: Mutex<Vec<MutexId>>,
}

impl Task {
    /// Create a new Task
    pub fn new(
        function_id: usize,
        module: Arc<raya_bytecode::Module>,
        parent: Option<TaskId>,
    ) -> Self {
        Self {
            id: TaskId::new(),
            state: Mutex::new(TaskState::Created),
            function_id,
            module,
            stack: Mutex::new(Stack::new()),
            ip: AtomicUsize::new(0),
            result: Mutex::new(None),
            waiters: Mutex::new(Vec::new()),
            parent,
            preempt_requested: AtomicBool::new(false),
            start_time: Mutex::new(None),
            exception_handlers: Mutex::new(Vec::new()),
            current_exception: Mutex::new(None),
            held_mutexes: Mutex::new(Vec::new()),
        }
    }

    /// Get the Task's unique ID
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Get the current state
    pub fn state(&self) -> TaskState {
        *self.state.lock().unwrap()
    }

    /// Set the current state
    pub fn set_state(&self, state: TaskState) {
        *self.state.lock().unwrap() = state;
    }

    /// Get the function ID this task is executing
    pub fn function_id(&self) -> usize {
        self.function_id
    }

    /// Get the module
    pub fn module(&self) -> &Arc<raya_bytecode::Module> {
        &self.module
    }

    /// Get the instruction pointer
    pub fn ip(&self) -> usize {
        self.ip.load(Ordering::Relaxed)
    }

    /// Set the instruction pointer
    pub fn set_ip(&self, ip: usize) {
        self.ip.store(ip, Ordering::Relaxed);
    }

    /// Get the parent task ID (if any)
    pub fn parent(&self) -> Option<TaskId> {
        self.parent
    }

    /// Complete the task with a result
    pub fn complete(&self, result: Value) {
        *self.result.lock().unwrap() = Some(result);
        self.set_state(TaskState::Completed);
    }

    /// Mark the task as failed
    pub fn fail(&self) {
        self.set_state(TaskState::Failed);
    }

    /// Get the result (if completed)
    pub fn result(&self) -> Option<Value> {
        *self.result.lock().unwrap()
    }

    /// Add a task that is waiting for this task to complete
    pub fn add_waiter(&self, waiter_id: TaskId) {
        self.waiters.lock().unwrap().push(waiter_id);
    }

    /// Take all waiting tasks (used when task completes)
    pub fn take_waiters(&self) -> Vec<TaskId> {
        std::mem::take(&mut *self.waiters.lock().unwrap())
    }

    /// Get the execution stack (for execution)
    pub fn stack(&self) -> &Mutex<Stack> {
        &self.stack
    }

    /// Request asynchronous preemption (like Go's preemption)
    pub fn request_preempt(&self) {
        self.preempt_requested.store(true, Ordering::Release);
    }

    /// Check if preemption is requested
    pub fn is_preempt_requested(&self) -> bool {
        self.preempt_requested.load(Ordering::Acquire)
    }

    /// Clear preemption flag
    pub fn clear_preempt(&self) {
        self.preempt_requested.store(false, Ordering::Release);
    }

    /// Record when task started executing
    pub fn set_start_time(&self, time: Instant) {
        *self.start_time.lock().unwrap() = Some(time);
    }

    /// Get task execution start time
    pub fn start_time(&self) -> Option<Instant> {
        *self.start_time.lock().unwrap()
    }

    /// Clear start time (when task yields or completes)
    pub fn clear_start_time(&self) {
        *self.start_time.lock().unwrap() = None;
    }

    /// Push an exception handler onto the stack
    pub fn push_exception_handler(&self, handler: ExceptionHandler) {
        self.exception_handlers.lock().unwrap().push(handler);
    }

    /// Pop an exception handler from the stack
    pub fn pop_exception_handler(&self) -> Option<ExceptionHandler> {
        self.exception_handlers.lock().unwrap().pop()
    }

    /// Get the topmost exception handler without removing it
    pub fn peek_exception_handler(&self) -> Option<ExceptionHandler> {
        self.exception_handlers.lock().unwrap().last().cloned()
    }

    /// Get the current exception (if any)
    pub fn current_exception(&self) -> Option<Value> {
        *self.current_exception.lock().unwrap()
    }

    /// Set the current exception
    pub fn set_exception(&self, exception: Value) {
        *self.current_exception.lock().unwrap() = Some(exception);
    }

    /// Clear the current exception
    pub fn clear_exception(&self) {
        *self.current_exception.lock().unwrap() = None;
    }

    /// Check if there is an active exception
    pub fn has_exception(&self) -> bool {
        self.current_exception.lock().unwrap().is_some()
    }

    /// Get the exception handler count (for debugging)
    pub fn exception_handler_count(&self) -> usize {
        self.exception_handlers.lock().unwrap().len()
    }

    /// Record that this task has acquired a mutex
    pub fn add_held_mutex(&self, mutex_id: MutexId) {
        self.held_mutexes.lock().unwrap().push(mutex_id);
    }

    /// Record that this task has released a mutex
    pub fn remove_held_mutex(&self, mutex_id: MutexId) {
        let mut mutexes = self.held_mutexes.lock().unwrap();
        if let Some(pos) = mutexes.iter().position(|&id| id == mutex_id) {
            mutexes.remove(pos);
        }
    }

    /// Get the number of mutexes currently held
    pub fn held_mutex_count(&self) -> usize {
        self.held_mutexes.lock().unwrap().len()
    }

    /// Take all mutexes held after a certain count (for exception unwinding)
    pub fn take_mutexes_since(&self, count: usize) -> Vec<MutexId> {
        let mut mutexes = self.held_mutexes.lock().unwrap();
        if mutexes.len() > count {
            mutexes.drain(count..).collect()
        } else {
            Vec::new()
        }
    }

    /// Get all held mutexes (for debugging)
    pub fn get_held_mutexes(&self) -> Vec<MutexId> {
        self.held_mutexes.lock().unwrap().clone()
    }
}

/// Handle for awaiting a Task's result
pub struct TaskHandle<T> {
    task_id: TaskId,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> TaskHandle<T> {
    /// Create a new TaskHandle
    pub fn new(task_id: TaskId) -> Self {
        Self {
            task_id,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get the task ID
    pub fn task_id(&self) -> TaskId {
        self.task_id
    }
}

impl<T> Clone for TaskHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for TaskHandle<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use raya_bytecode::{Function, Module, Opcode};

    fn create_test_module() -> Arc<Module> {
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "test_fn".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![Opcode::Return as u8],
        });
        Arc::new(module)
    }

    #[test]
    fn test_task_id_uniqueness() {
        let id1 = TaskId::new();
        let id2 = TaskId::new();
        assert_ne!(id1, id2);
        assert!(id2.as_u64() > id1.as_u64());
    }

    #[test]
    fn test_task_id_default() {
        let id = TaskId::default();
        assert!(id.as_u64() > 0);
    }

    #[test]
    fn test_task_creation() {
        let module = create_test_module();
        let task = Task::new(0, module.clone(), None);

        assert_eq!(task.function_id(), 0);
        assert_eq!(task.state(), TaskState::Created);
        assert_eq!(task.parent(), None);
        assert_eq!(task.ip(), 0);
        assert!(task.result().is_none());
    }

    #[test]
    fn test_task_with_parent() {
        let module = create_test_module();
        let parent_id = TaskId::new();
        let task = Task::new(0, module.clone(), Some(parent_id));

        assert_eq!(task.parent(), Some(parent_id));
    }

    #[test]
    fn test_task_state_transitions() {
        let module = create_test_module();
        let task = Task::new(0, module.clone(), None);

        assert_eq!(task.state(), TaskState::Created);

        task.set_state(TaskState::Running);
        assert_eq!(task.state(), TaskState::Running);

        task.set_state(TaskState::Suspended);
        assert_eq!(task.state(), TaskState::Suspended);

        task.set_state(TaskState::Resumed);
        assert_eq!(task.state(), TaskState::Resumed);

        task.set_state(TaskState::Completed);
        assert_eq!(task.state(), TaskState::Completed);
    }

    #[test]
    fn test_task_completion() {
        let module = create_test_module();
        let task = Task::new(0, module.clone(), None);

        assert!(task.result().is_none());

        task.complete(Value::i32(42));

        assert_eq!(task.state(), TaskState::Completed);
        assert_eq!(task.result(), Some(Value::i32(42)));
    }

    #[test]
    fn test_task_failure() {
        let module = create_test_module();
        let task = Task::new(0, module.clone(), None);

        task.fail();

        assert_eq!(task.state(), TaskState::Failed);
    }

    #[test]
    fn test_task_waiters() {
        let module = create_test_module();
        let task = Task::new(0, module.clone(), None);

        let waiter1 = TaskId::new();
        let waiter2 = TaskId::new();

        task.add_waiter(waiter1);
        task.add_waiter(waiter2);

        let waiters = task.take_waiters();
        assert_eq!(waiters.len(), 2);
        assert_eq!(waiters[0], waiter1);
        assert_eq!(waiters[1], waiter2);

        // After taking, should be empty
        let empty_waiters = task.take_waiters();
        assert_eq!(empty_waiters.len(), 0);
    }

    #[test]
    fn test_task_ip() {
        let module = create_test_module();
        let task = Task::new(0, module.clone(), None);

        assert_eq!(task.ip(), 0);

        task.set_ip(42);
        assert_eq!(task.ip(), 42);
    }

    #[test]
    fn test_task_handle_creation() {
        let task_id = TaskId::new();
        let handle: TaskHandle<i32> = TaskHandle::new(task_id);

        assert_eq!(handle.task_id(), task_id);
    }

    #[test]
    fn test_task_handle_clone() {
        let task_id = TaskId::new();
        let handle: TaskHandle<i32> = TaskHandle::new(task_id);
        let handle2 = handle;

        assert_eq!(handle.task_id(), handle2.task_id());
    }

    #[test]
    fn test_exception_handler_push_pop() {
        let module = create_test_module();
        let task = Task::new(0, module.clone(), None);

        assert_eq!(task.exception_handler_count(), 0);

        let handler = ExceptionHandler {
            catch_offset: 100,
            finally_offset: 200,
            stack_size: 5,
            frame_count: 2,
            mutex_count: 0,
        };

        task.push_exception_handler(handler.clone());
        assert_eq!(task.exception_handler_count(), 1);

        let popped = task.pop_exception_handler().unwrap();
        assert_eq!(popped.catch_offset, 100);
        assert_eq!(popped.finally_offset, 200);
        assert_eq!(popped.stack_size, 5);
        assert_eq!(popped.frame_count, 2);

        assert_eq!(task.exception_handler_count(), 0);
    }

    #[test]
    fn test_exception_handler_peek() {
        let module = create_test_module();
        let task = Task::new(0, module.clone(), None);

        let handler = ExceptionHandler {
            catch_offset: 100,
            finally_offset: -1,
            stack_size: 5,
            frame_count: 2,
            mutex_count: 0,
        };

        task.push_exception_handler(handler.clone());

        let peeked = task.peek_exception_handler().unwrap();
        assert_eq!(peeked.catch_offset, 100);
        assert_eq!(peeked.finally_offset, -1);

        // Peek should not remove the handler
        assert_eq!(task.exception_handler_count(), 1);
    }

    #[test]
    fn test_exception_handler_stack() {
        let module = create_test_module();
        let task = Task::new(0, module.clone(), None);

        let handler1 = ExceptionHandler {
            catch_offset: 100,
            finally_offset: -1,
            stack_size: 5,
            frame_count: 2,
            mutex_count: 0,
        };

        let handler2 = ExceptionHandler {
            catch_offset: 200,
            finally_offset: 250,
            stack_size: 10,
            frame_count: 3,
            mutex_count: 0,
        };

        task.push_exception_handler(handler1);
        task.push_exception_handler(handler2);

        assert_eq!(task.exception_handler_count(), 2);

        // Pop should return handler2 first (LIFO)
        let popped2 = task.pop_exception_handler().unwrap();
        assert_eq!(popped2.catch_offset, 200);

        let popped1 = task.pop_exception_handler().unwrap();
        assert_eq!(popped1.catch_offset, 100);

        assert_eq!(task.exception_handler_count(), 0);
        assert!(task.pop_exception_handler().is_none());
    }

    #[test]
    fn test_current_exception() {
        let module = create_test_module();
        let task = Task::new(0, module.clone(), None);

        assert!(!task.has_exception());
        assert!(task.current_exception().is_none());

        let error = Value::i32(42);
        task.set_exception(error);

        assert!(task.has_exception());
        assert_eq!(task.current_exception(), Some(Value::i32(42)));

        task.clear_exception();

        assert!(!task.has_exception());
        assert!(task.current_exception().is_none());
    }
}
