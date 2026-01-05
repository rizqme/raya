//! Task structure and execution state

use crate::stack::Stack;
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
}
