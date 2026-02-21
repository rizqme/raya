//! Task structure and execution state
//!
//! Uses grouped `parking_lot::Mutex` locks to reduce per-task overhead.
//! Field grouping is based on access-pattern analysis:
//!
//! | Group           | Fields                                                          | Accessed by          |
//! |-----------------|-----------------------------------------------------------------|----------------------|
//! | LifecycleState  | state, suspend_reason, resume_value, start_time, result,        | Reactor + VM workers |
//! |                 | waiters, awaiting_task                                          |                      |
//! | ExceptionState  | current_exception, caught_exception, exception_handlers         | VM workers only      |
//! | CallState       | closure_stack, call_stack, execution_frames                     | VM workers only      |
//! | InitState       | initial_args, held_mutexes                                     | VM workers only      |

use crate::vm::interpreter::execution::ExecutionFrame;
use crate::vm::snapshot::{BlockedReason, SerializedFrame, SerializedTask};
use crate::vm::stack::Stack;
use crate::vm::sync::MutexId;
use crate::vm::value::Value;
use parking_lot::Condvar as ParkingCondvar;
use parking_lot::Mutex as ParkingMutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;

/// Reason why a task is suspended
///
/// When a task cannot proceed (e.g., waiting for another task, sleeping,
/// waiting for a mutex), it suspends with a reason that tells the scheduler
/// what condition needs to be satisfied before resuming.
#[derive(Debug, Clone)]
pub enum SuspendReason {
    /// Waiting for another task to complete
    AwaitTask(TaskId),

    /// Sleeping until a specific time
    Sleep {
        /// When to wake up
        wake_at: Instant,
    },

    /// Waiting to acquire a mutex
    MutexLock {
        /// The mutex we're waiting for
        mutex_id: MutexId,
    },

    /// Waiting to send on a full channel
    ChannelSend {
        /// Channel handle
        channel_id: u64,
        /// Value to send (stored here while waiting)
        value: Value,
    },

    /// Waiting to receive from an empty channel
    ChannelReceive {
        /// Channel handle
        channel_id: u64,
    },

    /// Waiting for IO completion from the event loop (NativeCallResult::Suspend)
    IoWait,
}

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

// ============================================================================
// Grouped state structs
// ============================================================================

/// Lifecycle and scheduling state (accessed by both reactor and VM workers)
struct LifecycleState {
    state: TaskState,
    suspend_reason: Option<SuspendReason>,
    resume_value: Option<Value>,
    start_time: Option<Instant>,
    result: Option<Value>,
    waiters: Vec<TaskId>,
    awaiting_task: Option<TaskId>,
}

/// Exception handling state (VM worker only)
struct ExceptionState {
    current_exception: Option<Value>,
    caught_exception: Option<Value>,
    exception_handlers: Vec<ExceptionHandler>,
}

/// Call stack state (VM worker only)
struct CallState {
    closure_stack: Vec<Value>,
    call_stack: Vec<usize>,
    execution_frames: Vec<ExecutionFrame>,
}

/// Initialization and mutex state (VM worker only, rare access)
struct InitState {
    initial_args: Vec<Value>,
    held_mutexes: Vec<MutexId>,
}

// ============================================================================
// Task
// ============================================================================

/// A lightweight green thread
pub struct Task {
    // -- Immutable (set at creation, never changes) --

    /// Unique identifier
    id: TaskId,

    /// Function to execute
    function_id: usize,

    /// Module containing the function
    module: Arc<crate::compiler::Module>,

    /// Parent task (if spawned from another Task)
    parent: Option<TaskId>,

    // -- Atomics (lock-free) --

    /// Instruction pointer
    ip: AtomicUsize,

    /// Asynchronous preemption flag (like Go's preemption)
    preempt_requested: AtomicBool,

    /// Consecutive preemption count (for infinite loop detection)
    preempt_count: AtomicU32,

    /// Whether this task has been cancelled
    cancelled: AtomicBool,

    /// Current function being executed (may differ from function_id during nested calls)
    current_func_id: AtomicUsize,

    /// Current locals base offset in the stack
    current_locals_base: AtomicUsize,

    // -- Grouped Mutexes (parking_lot::Mutex — ~8 bytes each) --

    /// Lifecycle and scheduling state
    lifecycle: ParkingMutex<LifecycleState>,

    /// Exception handling state
    exceptions: ParkingMutex<ExceptionState>,

    /// Call stack and frame state
    calls: ParkingMutex<CallState>,

    /// Initialization args and held mutexes
    init: ParkingMutex<InitState>,

    // -- Separate locks (special reasons) --

    /// Execution stack (held for full interpreter run duration — std::sync::Mutex)
    stack: StdMutex<Stack>,

    /// Completion tracking for blocking wait
    completion_lock: ParkingMutex<bool>,

    /// Condvar for blocking until task completes
    completion_condvar: ParkingCondvar,
}

impl Task {
    /// Create a new Task
    pub fn new(
        function_id: usize,
        module: Arc<crate::compiler::Module>,
        parent: Option<TaskId>,
    ) -> Self {
        Self::with_args(function_id, module, parent, Vec::new())
    }

    /// Create a new Task with initial arguments
    pub fn with_args(
        function_id: usize,
        module: Arc<crate::compiler::Module>,
        parent: Option<TaskId>,
        args: Vec<Value>,
    ) -> Self {
        Self {
            id: TaskId::new(),
            function_id,
            module,
            parent,

            ip: AtomicUsize::new(0),
            preempt_requested: AtomicBool::new(false),
            preempt_count: AtomicU32::new(0),
            cancelled: AtomicBool::new(false),
            current_func_id: AtomicUsize::new(function_id),
            current_locals_base: AtomicUsize::new(0),

            lifecycle: ParkingMutex::new(LifecycleState {
                state: TaskState::Created,
                suspend_reason: None,
                resume_value: None,
                start_time: None,
                result: None,
                waiters: Vec::new(),
                awaiting_task: None,
            }),

            exceptions: ParkingMutex::new(ExceptionState {
                current_exception: None,
                caught_exception: None,
                exception_handlers: Vec::new(),
            }),

            calls: ParkingMutex::new(CallState {
                closure_stack: Vec::new(),
                call_stack: Vec::new(),
                execution_frames: Vec::new(),
            }),

            init: ParkingMutex::new(InitState {
                initial_args: args,
                held_mutexes: Vec::new(),
            }),

            stack: StdMutex::new(Stack::new()),
            completion_lock: ParkingMutex::new(false),
            completion_condvar: ParkingCondvar::new(),
        }
    }

    // =========================================================================
    // Immutable accessors
    // =========================================================================

    /// Get the Task's unique ID
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Get the function ID this task is executing
    pub fn function_id(&self) -> usize {
        self.function_id
    }

    /// Get the module
    pub fn module(&self) -> &Arc<crate::compiler::Module> {
        &self.module
    }

    /// Get the parent task ID (if any)
    pub fn parent(&self) -> Option<TaskId> {
        self.parent
    }

    // =========================================================================
    // Atomic accessors
    // =========================================================================

    /// Get the instruction pointer
    pub fn ip(&self) -> usize {
        self.ip.load(Ordering::Relaxed)
    }

    /// Set the instruction pointer
    pub fn set_ip(&self, ip: usize) {
        self.ip.store(ip, Ordering::Relaxed);
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

    /// Increment consecutive preemption counter, returns new count
    pub fn increment_preempt_count(&self) -> u32 {
        self.preempt_count.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Reset consecutive preemption counter (called on voluntary suspend/completion)
    pub fn reset_preempt_count(&self) {
        self.preempt_count.store(0, Ordering::Relaxed);
    }

    /// Cancel this task
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Check if this task has been cancelled
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Get the current function ID being executed
    pub fn current_func_id(&self) -> usize {
        self.current_func_id.load(Ordering::Relaxed)
    }

    /// Set the current function ID being executed
    pub fn set_current_func_id(&self, func_id: usize) {
        self.current_func_id.store(func_id, Ordering::Relaxed);
    }

    /// Get the current locals base offset
    pub fn current_locals_base(&self) -> usize {
        self.current_locals_base.load(Ordering::Relaxed)
    }

    /// Set the current locals base offset
    pub fn set_current_locals_base(&self, base: usize) {
        self.current_locals_base.store(base, Ordering::Relaxed);
    }

    // =========================================================================
    // Stack (std::sync::Mutex — held for full interpreter run)
    // =========================================================================

    /// Get the execution stack (for execution)
    pub fn stack(&self) -> &StdMutex<Stack> {
        &self.stack
    }

    /// Take the initial arguments (consumes them)
    pub fn take_initial_args(&self) -> Vec<Value> {
        std::mem::take(&mut self.init.lock().initial_args)
    }

    /// Replace the task's stack with a pre-allocated one (for pool reuse).
    /// Must be called before the task starts executing.
    pub fn replace_stack(&self, stack: Stack) {
        *self.stack.lock().unwrap() = stack;
    }

    /// Take the stack out of this task, replacing it with an empty one.
    /// Used to return the stack to a pool after task completion.
    pub fn take_stack(&self) -> Stack {
        std::mem::take(&mut *self.stack.lock().unwrap())
    }

    // =========================================================================
    // Lifecycle state (scheduling, suspend/resume, completion)
    // =========================================================================

    /// Get the current state
    pub fn state(&self) -> TaskState {
        self.lifecycle.lock().state
    }

    /// Set the current state
    pub fn set_state(&self, state: TaskState) {
        self.lifecycle.lock().state = state;
    }

    /// Complete the task with a result
    pub fn complete(&self, result: Value) {
        {
            let mut lc = self.lifecycle.lock();
            lc.result = Some(result);
            lc.state = TaskState::Completed;
        }
        self.signal_completion();
    }

    /// Mark the task as failed
    pub fn fail(&self) {
        self.lifecycle.lock().state = TaskState::Failed;
        self.signal_completion();
    }

    /// Mark this task as failed with an error message stored as exception
    pub fn fail_with_error(&self, error: &crate::vm::VmError) {
        let msg = error.to_string();
        let raya_str = crate::vm::RayaString::new(msg);
        let boxed = Box::new(raya_str);
        let ptr = Box::into_raw(boxed);
        let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(ptr).unwrap()) };
        self.set_exception(val);
        self.fail();
    }

    /// Signal that the task has completed (either success or failure)
    fn signal_completion(&self) {
        let mut done = self.completion_lock.lock();
        *done = true;
        self.completion_condvar.notify_all();
    }

    /// Block until this task completes (either successfully or with failure)
    /// Returns the task state after completion
    pub fn wait_completion(&self) -> TaskState {
        let mut done = self.completion_lock.lock();
        while !*done {
            self.completion_condvar.wait(&mut done);
        }
        self.lifecycle.lock().state
    }

    /// Block until this task completes, with a timeout
    /// Returns the task state (may still be Running if timeout occurred)
    pub fn wait_completion_timeout(&self, timeout: std::time::Duration) -> TaskState {
        let mut done = self.completion_lock.lock();
        if !*done {
            self.completion_condvar.wait_for(&mut done, timeout);
        }
        self.lifecycle.lock().state
    }

    /// Get the result (if completed)
    pub fn result(&self) -> Option<Value> {
        self.lifecycle.lock().result
    }

    /// Add a task that is waiting for this task to complete
    pub fn add_waiter(&self, waiter_id: TaskId) {
        self.lifecycle.lock().waiters.push(waiter_id);
    }

    /// Take all waiting tasks (used when task completes)
    pub fn take_waiters(&self) -> Vec<TaskId> {
        std::mem::take(&mut self.lifecycle.lock().waiters)
    }

    /// Set the task this task is waiting for (await)
    pub fn set_awaiting(&self, task_id: TaskId) {
        self.lifecycle.lock().awaiting_task = Some(task_id);
    }

    /// Get and clear the task this task was waiting for
    pub fn take_awaiting(&self) -> Option<TaskId> {
        self.lifecycle.lock().awaiting_task.take()
    }

    /// Record when task started executing
    pub fn set_start_time(&self, time: Instant) {
        self.lifecycle.lock().start_time = Some(time);
    }

    /// Get task execution start time
    pub fn start_time(&self) -> Option<Instant> {
        self.lifecycle.lock().start_time
    }

    /// Clear start time (when task yields or completes)
    pub fn clear_start_time(&self) {
        self.lifecycle.lock().start_time = None;
    }

    // =========================================================================
    // Suspension
    // =========================================================================

    /// Set the suspension reason
    pub fn suspend(&self, reason: SuspendReason) {
        let mut lc = self.lifecycle.lock();
        lc.state = TaskState::Suspended;
        lc.suspend_reason = Some(reason);
    }

    /// Conditionally suspend only if the task is still Running.
    ///
    /// Returns true if suspended, false if the state was already changed
    /// (e.g., by a MutexUnlock on another VM worker that already woke this task).
    /// This prevents the reactor from overwriting a Resumed state back to Suspended.
    pub fn try_suspend(&self, reason: SuspendReason) -> bool {
        let mut lc = self.lifecycle.lock();
        if lc.state == TaskState::Running {
            lc.state = TaskState::Suspended;
            lc.suspend_reason = Some(reason);
            true
        } else {
            false
        }
    }

    /// Get the suspension reason
    pub fn suspend_reason(&self) -> Option<SuspendReason> {
        self.lifecycle.lock().suspend_reason.clone()
    }

    /// Clear the suspension reason (when resuming)
    pub fn clear_suspend_reason(&self) {
        self.lifecycle.lock().suspend_reason = None;
    }

    /// Set the value to push when resuming (e.g., channel receive result)
    pub fn set_resume_value(&self, value: Value) {
        self.lifecycle.lock().resume_value = Some(value);
    }

    /// Take the resume value (consumes it)
    pub fn take_resume_value(&self) -> Option<Value> {
        self.lifecycle.lock().resume_value.take()
    }

    // =========================================================================
    // Exception state
    // =========================================================================

    /// Push an exception handler onto the stack
    pub fn push_exception_handler(&self, handler: ExceptionHandler) {
        self.exceptions.lock().exception_handlers.push(handler);
    }

    /// Pop an exception handler from the stack
    pub fn pop_exception_handler(&self) -> Option<ExceptionHandler> {
        self.exceptions.lock().exception_handlers.pop()
    }

    /// Get the topmost exception handler without removing it
    pub fn peek_exception_handler(&self) -> Option<ExceptionHandler> {
        self.exceptions.lock().exception_handlers.last().cloned()
    }

    /// Get the current exception (if any)
    pub fn current_exception(&self) -> Option<Value> {
        self.exceptions.lock().current_exception
    }

    /// Set the current exception
    pub fn set_exception(&self, exception: Value) {
        self.exceptions.lock().current_exception = Some(exception);
    }

    /// Clear the current exception
    pub fn clear_exception(&self) {
        self.exceptions.lock().current_exception = None;
    }

    /// Check if there is an active exception
    pub fn has_exception(&self) -> bool {
        self.exceptions.lock().current_exception.is_some()
    }

    /// Get the caught exception (for Rethrow)
    pub fn caught_exception(&self) -> Option<Value> {
        self.exceptions.lock().caught_exception
    }

    /// Set the caught exception (when entering catch block)
    pub fn set_caught_exception(&self, exception: Value) {
        self.exceptions.lock().caught_exception = Some(exception);
    }

    /// Clear the caught exception
    pub fn clear_caught_exception(&self) {
        self.exceptions.lock().caught_exception = None;
    }

    /// Get the exception handler count (for debugging)
    pub fn exception_handler_count(&self) -> usize {
        self.exceptions.lock().exception_handlers.len()
    }

    // =========================================================================
    // Call state (closures, call stack, execution frames)
    // =========================================================================

    /// Push a closure onto the closure stack
    pub fn push_closure(&self, closure: Value) {
        self.calls.lock().closure_stack.push(closure);
    }

    /// Pop a closure from the closure stack
    pub fn pop_closure(&self) -> Option<Value> {
        self.calls.lock().closure_stack.pop()
    }

    /// Get the current closure (top of stack)
    pub fn current_closure(&self) -> Option<Value> {
        self.calls.lock().closure_stack.last().copied()
    }

    /// Push a function ID onto the call stack
    pub fn push_call_frame(&self, function_id: usize) {
        self.calls.lock().call_stack.push(function_id);
    }

    /// Pop a function ID from the call stack
    pub fn pop_call_frame(&self) -> Option<usize> {
        self.calls.lock().call_stack.pop()
    }

    /// Get the current call stack (for building stack traces)
    pub fn get_call_stack(&self) -> Vec<usize> {
        self.calls.lock().call_stack.clone()
    }

    /// Take the execution frames (drains them from the task)
    pub fn take_execution_frames(&self) -> Vec<ExecutionFrame> {
        std::mem::take(&mut self.calls.lock().execution_frames)
    }

    /// Get a snapshot of the execution frames (non-draining clone, for profiling).
    pub fn get_execution_frames(&self) -> Vec<ExecutionFrame> {
        self.calls.lock().execution_frames.clone()
    }

    /// Save execution frames (for suspend)
    pub fn save_execution_frames(&self, frames: Vec<ExecutionFrame>) {
        self.calls.lock().execution_frames = frames;
    }

    /// Build a stack trace string from the current call stack
    pub fn build_stack_trace(&self, error_name: &str, error_message: &str) -> String {
        let cs = self.calls.lock();
        let module = &self.module;
        let debug_info = module.debug_info.as_ref();

        let mut trace = if error_message.is_empty() {
            error_name.to_string()
        } else {
            format!("{}: {}", error_name, error_message)
        };

        let current_ip = self.ip.load(std::sync::atomic::Ordering::Relaxed);

        for (i, &func_id) in cs.call_stack.iter().rev().enumerate() {
            if let Some(func) = module.functions.get(func_id) {
                let ip = if i == 0 {
                    Some(current_ip)
                } else {
                    let frame_idx = cs.execution_frames.len().checked_sub(i);
                    frame_idx.and_then(|idx| cs.execution_frames.get(idx)).map(|f| f.ip)
                };

                let location = ip.and_then(|offset| {
                    debug_info.and_then(|di| {
                        di.functions.get(func_id).and_then(|fdi| {
                            fdi.lookup_location(offset as u32)
                        })
                    })
                });

                if let Some(loc) = location {
                    trace.push_str(&format!("\n    at {} (line {}:{})", func.name, loc.line, loc.column));
                } else {
                    trace.push_str(&format!("\n    at {}", func.name));
                }
            }
        }

        trace
    }

    // =========================================================================
    // Init state (held mutexes)
    // =========================================================================

    /// Record that this task has acquired a mutex
    pub fn add_held_mutex(&self, mutex_id: MutexId) {
        self.init.lock().held_mutexes.push(mutex_id);
    }

    /// Record that this task has released a mutex
    pub fn remove_held_mutex(&self, mutex_id: MutexId) {
        let mut is = self.init.lock();
        if let Some(pos) = is.held_mutexes.iter().position(|&id| id == mutex_id) {
            is.held_mutexes.remove(pos);
        }
    }

    /// Get the number of mutexes currently held
    pub fn held_mutex_count(&self) -> usize {
        self.init.lock().held_mutexes.len()
    }

    /// Take all mutexes held after a certain count (for exception unwinding)
    pub fn take_mutexes_since(&self, count: usize) -> Vec<MutexId> {
        let mut is = self.init.lock();
        if is.held_mutexes.len() > count {
            is.held_mutexes.drain(count..).collect()
        } else {
            Vec::new()
        }
    }

    /// Get all held mutexes (for debugging)
    pub fn get_held_mutexes(&self) -> Vec<MutexId> {
        self.init.lock().held_mutexes.clone()
    }

    // =========================================================================
    // Snapshot serialization bridge
    // =========================================================================

    /// Serialize this task into a `SerializedTask` for snapshot persistence.
    ///
    /// Must only be called when the task is paused (not actively executing on a worker).
    pub fn to_serialized(&self) -> SerializedTask {
        let lc = self.lifecycle.lock();
        let state = lc.state;
        let result = lc.result;
        let suspend_reason = lc.suspend_reason.clone();
        drop(lc);

        let ip = self.ip.load(Ordering::Relaxed);
        let stack_values = self.stack.lock().unwrap().as_slice().to_vec();
        let execution_frames = self.calls.lock().execution_frames.clone();

        // Map ExecutionFrame -> SerializedFrame
        let frames: Vec<SerializedFrame> = execution_frames
            .iter()
            .map(|ef| SerializedFrame {
                function_index: ef.func_id,
                return_ip: ef.ip,
                base_pointer: ef.locals_base,
                locals: Vec::new(),
            })
            .collect();

        // Map SuspendReason -> BlockedReason
        let blocked_on = suspend_reason.map(|reason| match reason {
            SuspendReason::AwaitTask(task_id) => BlockedReason::AwaitingTask(task_id),
            SuspendReason::MutexLock { mutex_id } => {
                BlockedReason::AwaitingMutex(mutex_id.as_u64())
            }
            SuspendReason::Sleep { .. } => BlockedReason::Other("sleep".to_string()),
            SuspendReason::ChannelSend { channel_id, .. } => {
                BlockedReason::Other(format!("channel_send:{}", channel_id))
            }
            SuspendReason::ChannelReceive { channel_id } => {
                BlockedReason::Other(format!("channel_recv:{}", channel_id))
            }
            SuspendReason::IoWait => BlockedReason::Other("io_wait".to_string()),
        });

        SerializedTask {
            task_id: self.id,
            state,
            function_index: self.function_id,
            ip,
            frames,
            stack: stack_values,
            result,
            parent: self.parent,
            blocked_on,
        }
    }

    /// Restore a task from a `SerializedTask` and a module reference.
    ///
    /// The module must be the same module that was active when the snapshot was taken.
    /// Runtime-transient state (preemption counters, start_time, waiters, etc.) is
    /// reset to defaults — only persistent execution state is restored.
    pub fn from_serialized(
        serialized: SerializedTask,
        module: Arc<crate::compiler::Module>,
    ) -> Self {
        // Map SerializedFrame -> ExecutionFrame
        let execution_frames: Vec<ExecutionFrame> = serialized
            .frames
            .iter()
            .map(|sf| ExecutionFrame {
                func_id: sf.function_index,
                ip: sf.return_ip,
                locals_base: sf.base_pointer,
                is_closure: false,
                return_action: super::super::interpreter::execution::ReturnAction::PushReturnValue,
            })
            .collect();

        // Rebuild stack from serialized values
        let mut stack = Stack::new();
        for value in &serialized.stack {
            let _ = stack.push(*value);
        }

        // Map BlockedReason -> SuspendReason
        let suspend_reason = serialized.blocked_on.map(|reason| match reason {
            BlockedReason::AwaitingTask(task_id) => SuspendReason::AwaitTask(task_id),
            BlockedReason::AwaitingMutex(id) => SuspendReason::MutexLock {
                mutex_id: MutexId::from_u64(id),
            },
            BlockedReason::Other(_) => SuspendReason::IoWait,
        });

        let current_func_id = execution_frames
            .last()
            .map(|f| f.func_id)
            .unwrap_or(serialized.function_index);

        let current_locals_base = execution_frames
            .last()
            .map(|f| f.locals_base)
            .unwrap_or(0);

        Self {
            id: serialized.task_id,
            function_id: serialized.function_index,
            module,
            parent: serialized.parent,

            ip: AtomicUsize::new(serialized.ip),
            preempt_requested: AtomicBool::new(false),
            preempt_count: AtomicU32::new(0),
            cancelled: AtomicBool::new(false),
            current_func_id: AtomicUsize::new(current_func_id),
            current_locals_base: AtomicUsize::new(current_locals_base),

            lifecycle: ParkingMutex::new(LifecycleState {
                state: serialized.state,
                suspend_reason,
                resume_value: None,
                start_time: None,
                result: serialized.result,
                waiters: Vec::new(),
                awaiting_task: None,
            }),

            exceptions: ParkingMutex::new(ExceptionState {
                current_exception: None,
                caught_exception: None,
                exception_handlers: Vec::new(),
            }),

            calls: ParkingMutex::new(CallState {
                closure_stack: Vec::new(),
                call_stack: Vec::new(),
                execution_frames,
            }),

            init: ParkingMutex::new(InitState {
                initial_args: Vec::new(),
                held_mutexes: Vec::new(),
            }),

            stack: StdMutex::new(stack),
            completion_lock: ParkingMutex::new(false),
            completion_condvar: ParkingCondvar::new(),
        }
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
    use crate::compiler::{Function, Module, Opcode};

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

    // =========================================================================
    // Snapshot bridge tests
    // =========================================================================

    #[test]
    fn test_task_to_serialized_basic() {
        let module = create_test_module();
        let parent_id = TaskId::from_u64(99);
        let task = Task::new(0, module.clone(), Some(parent_id));

        task.set_state(TaskState::Running);
        task.set_ip(42);
        task.stack().lock().unwrap().push(Value::i32(10)).unwrap();
        task.stack().lock().unwrap().push(Value::i32(20)).unwrap();

        let serialized = task.to_serialized();

        assert_eq!(serialized.task_id, task.id());
        assert_eq!(serialized.state, TaskState::Running);
        assert_eq!(serialized.function_index, 0);
        assert_eq!(serialized.ip, 42);
        assert_eq!(serialized.stack.len(), 2);
        assert_eq!(serialized.stack[0], Value::i32(10));
        assert_eq!(serialized.stack[1], Value::i32(20));
        assert_eq!(serialized.parent, Some(parent_id));
        assert!(serialized.result.is_none());
        assert!(serialized.blocked_on.is_none());
    }

    #[test]
    fn test_task_to_serialized_completed() {
        let module = create_test_module();
        let task = Task::new(0, module.clone(), None);
        task.complete(Value::i32(42));

        let serialized = task.to_serialized();

        assert_eq!(serialized.state, TaskState::Completed);
        assert_eq!(serialized.result, Some(Value::i32(42)));
    }

    #[test]
    fn test_task_to_serialized_suspended_await() {
        let module = create_test_module();
        let task = Task::new(0, module.clone(), None);
        let awaited = TaskId::from_u64(77);
        task.suspend(SuspendReason::AwaitTask(awaited));

        let serialized = task.to_serialized();

        assert_eq!(serialized.state, TaskState::Suspended);
        match &serialized.blocked_on {
            Some(BlockedReason::AwaitingTask(id)) => assert_eq!(id.as_u64(), 77),
            other => panic!("Expected AwaitingTask, got {:?}", other),
        }
    }

    #[test]
    fn test_task_to_serialized_suspended_mutex() {
        use crate::vm::sync::MutexId;
        let module = create_test_module();
        let task = Task::new(0, module.clone(), None);
        let mutex_id = MutexId::from_u64(55);
        task.suspend(SuspendReason::MutexLock { mutex_id });

        let serialized = task.to_serialized();

        match &serialized.blocked_on {
            Some(BlockedReason::AwaitingMutex(id)) => assert_eq!(*id, 55),
            other => panic!("Expected AwaitingMutex, got {:?}", other),
        }
    }

    #[test]
    fn test_task_to_serialized_with_frames() {
        use crate::vm::interpreter::execution::{ExecutionFrame, ReturnAction};
        let module = create_test_module();
        let task = Task::new(0, module.clone(), None);

        let frames = vec![
            ExecutionFrame {
                func_id: 0,
                ip: 10,
                locals_base: 0,
                is_closure: false,
                return_action: ReturnAction::PushReturnValue,
            },
            ExecutionFrame {
                func_id: 1,
                ip: 25,
                locals_base: 5,
                is_closure: true,
                return_action: ReturnAction::Discard,
            },
        ];
        task.save_execution_frames(frames);

        let serialized = task.to_serialized();

        assert_eq!(serialized.frames.len(), 2);
        assert_eq!(serialized.frames[0].function_index, 0);
        assert_eq!(serialized.frames[0].return_ip, 10);
        assert_eq!(serialized.frames[0].base_pointer, 0);
        assert_eq!(serialized.frames[1].function_index, 1);
        assert_eq!(serialized.frames[1].return_ip, 25);
        assert_eq!(serialized.frames[1].base_pointer, 5);
    }

    #[test]
    fn test_task_from_serialized_basic() {
        let module = create_test_module();
        let task_id = TaskId::from_u64(42);
        let parent_id = TaskId::from_u64(99);

        let mut serialized = SerializedTask::new(task_id, 0);
        serialized.state = TaskState::Suspended;
        serialized.ip = 100;
        serialized.stack = vec![Value::i32(1), Value::i32(2), Value::i32(3)];
        serialized.parent = Some(parent_id);

        let task = Task::from_serialized(serialized, module);

        assert_eq!(task.id().as_u64(), 42);
        assert_eq!(task.state(), TaskState::Suspended);
        assert_eq!(task.function_id(), 0);
        assert_eq!(task.ip(), 100);
        assert_eq!(task.stack().lock().unwrap().as_slice().len(), 3);
        assert_eq!(task.parent(), Some(parent_id));
    }

    #[test]
    fn test_task_round_trip() {
        use crate::vm::interpreter::execution::{ExecutionFrame, ReturnAction};
        let module = create_test_module();
        let parent_id = TaskId::from_u64(88);
        let task = Task::with_args(0, module.clone(), Some(parent_id), vec![]);

        task.set_state(TaskState::Running);
        task.set_ip(50);
        task.stack().lock().unwrap().push(Value::i32(100)).unwrap();
        task.stack().lock().unwrap().push(Value::null()).unwrap();
        task.save_execution_frames(vec![ExecutionFrame {
            func_id: 0,
            ip: 20,
            locals_base: 0,
            is_closure: false,
            return_action: ReturnAction::PushReturnValue,
        }]);

        // Serialize
        let serialized = task.to_serialized();

        // Deserialize
        let restored = Task::from_serialized(serialized, module);

        assert_eq!(restored.id(), task.id());
        assert_eq!(restored.state(), TaskState::Running);
        assert_eq!(restored.function_id(), 0);
        assert_eq!(restored.ip(), 50);
        assert_eq!(restored.parent(), Some(parent_id));

        let stack = restored.stack().lock().unwrap();
        assert_eq!(stack.as_slice().len(), 2);
        assert_eq!(stack.as_slice()[0], Value::i32(100));
        assert_eq!(stack.as_slice()[1], Value::null());
        drop(stack);

        let frames = restored.take_execution_frames();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].func_id, 0);
        assert_eq!(frames[0].ip, 20);
        assert_eq!(frames[0].locals_base, 0);
    }

    #[test]
    fn test_task_round_trip_binary() {
        // Full round trip: Task -> SerializedTask -> bytes -> SerializedTask -> Task
        let module = create_test_module();
        let task = Task::new(0, module.clone(), None);
        task.set_state(TaskState::Suspended);
        task.set_ip(75);
        task.stack().lock().unwrap().push(Value::i32(42)).unwrap();
        task.suspend(SuspendReason::AwaitTask(TaskId::from_u64(99)));

        let serialized = task.to_serialized();

        // Encode to bytes
        let mut bytes = Vec::new();
        serialized.encode(&mut bytes).unwrap();

        // Decode from bytes
        let decoded = SerializedTask::decode(&mut &bytes[..], false).unwrap();

        // Restore task
        let restored = Task::from_serialized(decoded, module);

        assert_eq!(restored.id(), task.id());
        assert_eq!(restored.state(), TaskState::Suspended);
        assert_eq!(restored.ip(), 75);
        assert_eq!(
            restored.stack().lock().unwrap().as_slice(),
            &[Value::i32(42)]
        );

        match restored.suspend_reason() {
            Some(SuspendReason::AwaitTask(id)) => assert_eq!(id.as_u64(), 99),
            other => panic!("Expected AwaitTask, got {:?}", other),
        }
    }
}
