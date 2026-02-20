//! Task structure and execution state

use crate::vm::interpreter::execution::ExecutionFrame;
use crate::vm::snapshot::{BlockedReason, SerializedFrame, SerializedTask};
use crate::vm::stack::Stack;
use crate::vm::sync::MutexId;
use crate::vm::value::Value;
use parking_lot::Condvar as ParkingCondvar;
use parking_lot::Mutex as ParkingMutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
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

/// A lightweight green thread
pub struct Task {
    /// Unique identifier
    id: TaskId,

    /// Current state
    state: Mutex<TaskState>,

    /// Function to execute
    function_id: usize,

    /// Module containing the function
    module: Arc<crate::compiler::Module>,

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

    /// Consecutive preemption count (for infinite loop detection)
    preempt_count: AtomicU32,

    /// When this task started executing (for preemption monitoring)
    start_time: Mutex<Option<Instant>>,

    /// Exception handler stack (for try-catch-finally)
    exception_handlers: Mutex<Vec<ExceptionHandler>>,

    /// Currently thrown exception (if any)
    current_exception: Mutex<Option<Value>>,

    /// Caught exception (for Rethrow - preserved even after catch entry clears current_exception)
    caught_exception: Mutex<Option<Value>>,

    /// Mutexes currently held by this Task (for auto-unlock on exception)
    held_mutexes: Mutex<Vec<MutexId>>,

    /// Stack of currently executing closures (for LoadCaptured access)
    closure_stack: Mutex<Vec<Value>>,

    /// Call stack for stack traces (function IDs)
    call_stack: Mutex<Vec<usize>>,

    /// Reason for suspension (when state is Suspended)
    suspend_reason: Mutex<Option<SuspendReason>>,

    /// Value to push on resume (e.g., received channel value)
    resume_value: Mutex<Option<Value>>,

    /// Initial arguments passed to this task when spawned
    initial_args: Mutex<Vec<Value>>,

    /// Task ID this task is waiting for (when suspended on await)
    awaiting_task: Mutex<Option<TaskId>>,

    /// Whether this task has been cancelled
    cancelled: AtomicBool,

    /// Completion tracking for blocking wait (using parking_lot for efficiency)
    /// The bool indicates whether the task has finished (completed or failed)
    completion_lock: ParkingMutex<bool>,

    /// Condvar for blocking until task completes
    completion_condvar: ParkingCondvar,

    /// Current function being executed (may differ from function_id during nested calls)
    current_func_id: AtomicUsize,

    /// Current locals base offset in the stack
    current_locals_base: AtomicUsize,

    /// Execution frame stack for frame-based interpreter (saved across suspend/resume)
    execution_frames: Mutex<Vec<ExecutionFrame>>,
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
            state: Mutex::new(TaskState::Created),
            function_id,
            module,
            stack: Mutex::new(Stack::new()),
            ip: AtomicUsize::new(0),
            result: Mutex::new(None),
            waiters: Mutex::new(Vec::new()),
            parent,
            preempt_requested: AtomicBool::new(false),
            preempt_count: AtomicU32::new(0),
            start_time: Mutex::new(None),
            exception_handlers: Mutex::new(Vec::new()),
            current_exception: Mutex::new(None),
            caught_exception: Mutex::new(None),
            held_mutexes: Mutex::new(Vec::new()),
            closure_stack: Mutex::new(Vec::new()),
            call_stack: Mutex::new(Vec::new()),
            suspend_reason: Mutex::new(None),
            resume_value: Mutex::new(None),
            initial_args: Mutex::new(args),
            awaiting_task: Mutex::new(None),
            cancelled: AtomicBool::new(false),
            completion_lock: ParkingMutex::new(false),
            completion_condvar: ParkingCondvar::new(),
            current_func_id: AtomicUsize::new(function_id),
            current_locals_base: AtomicUsize::new(0),
            execution_frames: Mutex::new(Vec::new()),
        }
    }

    /// Take the initial arguments (consumes them)
    pub fn take_initial_args(&self) -> Vec<Value> {
        std::mem::take(&mut *self.initial_args.lock().unwrap())
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
    pub fn module(&self) -> &Arc<crate::compiler::Module> {
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
        // Signal completion to any waiters
        self.signal_completion();
    }

    /// Mark the task as failed
    pub fn fail(&self) {
        self.set_state(TaskState::Failed);
        // Signal completion to any waiters (failure is also a completion)
        self.signal_completion();
    }

    /// Mark this task as failed with an error message stored as exception
    pub fn fail_with_error(&self, error: &crate::vm::VmError) {
        // Store the error message as a string in current_exception
        // so extract_exception_message can find it
        let msg = error.to_string();
        let raya_str = crate::vm::RayaString::new(msg);
        // Use GC-bypass: allocate on heap directly (this is a fatal error path)
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
        self.state()
    }

    /// Block until this task completes, with a timeout
    /// Returns the task state (may still be Running if timeout occurred)
    pub fn wait_completion_timeout(&self, timeout: std::time::Duration) -> TaskState {
        let mut done = self.completion_lock.lock();
        if !*done {
            self.completion_condvar.wait_for(&mut done, timeout);
        }
        self.state()
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

    /// Set the task this task is waiting for (await)
    pub fn set_awaiting(&self, task_id: TaskId) {
        *self.awaiting_task.lock().unwrap() = Some(task_id);
    }

    /// Get and clear the task this task was waiting for
    pub fn take_awaiting(&self) -> Option<TaskId> {
        self.awaiting_task.lock().unwrap().take()
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

    /// Get the caught exception (for Rethrow)
    pub fn caught_exception(&self) -> Option<Value> {
        *self.caught_exception.lock().unwrap()
    }

    /// Set the caught exception (when entering catch block)
    pub fn set_caught_exception(&self, exception: Value) {
        *self.caught_exception.lock().unwrap() = Some(exception);
    }

    /// Clear the caught exception
    pub fn clear_caught_exception(&self) {
        *self.caught_exception.lock().unwrap() = None;
    }

    /// Get the exception handler count (for debugging)
    pub fn exception_handler_count(&self) -> usize {
        self.exception_handlers.lock().unwrap().len()
    }

    // =========================================================================
    // Closure Stack
    // =========================================================================

    /// Push a closure onto the closure stack
    pub fn push_closure(&self, closure: Value) {
        self.closure_stack.lock().unwrap().push(closure);
    }

    /// Pop a closure from the closure stack
    pub fn pop_closure(&self) -> Option<Value> {
        self.closure_stack.lock().unwrap().pop()
    }

    /// Get the current closure (top of stack)
    pub fn current_closure(&self) -> Option<Value> {
        self.closure_stack.lock().unwrap().last().copied()
    }

    /// Get the closure stack (for execution)
    pub fn closure_stack(&self) -> &Mutex<Vec<Value>> {
        &self.closure_stack
    }

    // =========================================================================
    // Call Stack (for stack traces)
    // =========================================================================

    /// Push a function ID onto the call stack
    pub fn push_call_frame(&self, function_id: usize) {
        self.call_stack.lock().unwrap().push(function_id);
    }

    /// Pop a function ID from the call stack
    pub fn pop_call_frame(&self) -> Option<usize> {
        self.call_stack.lock().unwrap().pop()
    }

    /// Get the current call stack (for building stack traces)
    pub fn get_call_stack(&self) -> Vec<usize> {
        self.call_stack.lock().unwrap().clone()
    }

    /// Build a stack trace string from the current call stack
    pub fn build_stack_trace(&self, error_name: &str, error_message: &str) -> String {
        let call_stack = self.call_stack.lock().unwrap();
        let execution_frames = self.execution_frames.lock().unwrap();
        let module = &self.module;
        let debug_info = module.debug_info.as_ref();

        let mut trace = if error_message.is_empty() {
            error_name.to_string()
        } else {
            format!("{}: {}", error_name, error_message)
        };

        // Build a map of func_id → bytecode offset from execution frames
        // The current frame's IP comes from self.ip, caller frames come from execution_frames
        let current_ip = self.ip.load(std::sync::atomic::Ordering::Relaxed);

        // Add each frame to the stack trace (most recent first)
        for (i, &func_id) in call_stack.iter().rev().enumerate() {
            if let Some(func) = module.functions.get(func_id) {
                // Try to get bytecode offset: first frame uses current ip,
                // subsequent frames use execution_frames (which are in reverse order)
                let ip = if i == 0 {
                    Some(current_ip)
                } else {
                    // Execution frames are pushed in call order, so reverse iteration
                    // matches the call_stack reverse iteration
                    let frame_idx = execution_frames.len().checked_sub(i);
                    frame_idx.and_then(|idx| execution_frames.get(idx)).map(|f| f.ip)
                };

                // Try to look up source location from debug info
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
    // Execution Frames (frame-based interpreter)
    // =========================================================================

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

    /// Take the execution frames (drains them from the task)
    pub fn take_execution_frames(&self) -> Vec<ExecutionFrame> {
        std::mem::take(&mut *self.execution_frames.lock().unwrap())
    }

    /// Save execution frames (for suspend)
    pub fn save_execution_frames(&self, frames: Vec<ExecutionFrame>) {
        *self.execution_frames.lock().unwrap() = frames;
    }

    // =========================================================================
    // Suspension
    // =========================================================================

    /// Set the suspension reason
    pub fn suspend(&self, reason: SuspendReason) {
        self.set_state(TaskState::Suspended);
        *self.suspend_reason.lock().unwrap() = Some(reason);
    }

    /// Conditionally suspend only if the task is still Running.
    ///
    /// Returns true if suspended, false if the state was already changed
    /// (e.g., by a MutexUnlock on another VM worker that already woke this task).
    /// This prevents the reactor from overwriting a Resumed state back to Suspended.
    pub fn try_suspend(&self, reason: SuspendReason) -> bool {
        let mut state = self.state.lock().unwrap();
        if *state == TaskState::Running {
            *state = TaskState::Suspended;
            drop(state);
            *self.suspend_reason.lock().unwrap() = Some(reason);
            true
        } else {
            false
        }
    }

    /// Get the suspension reason
    pub fn suspend_reason(&self) -> Option<SuspendReason> {
        self.suspend_reason.lock().unwrap().clone()
    }

    /// Clear the suspension reason (when resuming)
    pub fn clear_suspend_reason(&self) {
        *self.suspend_reason.lock().unwrap() = None;
    }

    /// Set the value to push when resuming (e.g., channel receive result)
    pub fn set_resume_value(&self, value: Value) {
        *self.resume_value.lock().unwrap() = Some(value);
    }

    /// Take the resume value (consumes it)
    pub fn take_resume_value(&self) -> Option<Value> {
        self.resume_value.lock().unwrap().take()
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

    // =========================================================================
    // Snapshot serialization bridge
    // =========================================================================

    /// Serialize this task into a `SerializedTask` for snapshot persistence.
    ///
    /// Must only be called when the task is paused (not actively executing on a worker).
    /// Typically invoked during a stop-the-world safepoint or when the scheduler is idle.
    pub fn to_serialized(&self) -> SerializedTask {
        let state = *self.state.lock().unwrap();
        let ip = self.ip.load(Ordering::Relaxed);
        let stack_values = self.stack.lock().unwrap().as_slice().to_vec();
        let result = self.result.lock().unwrap().clone();
        let suspend_reason = self.suspend_reason.lock().unwrap().clone();
        let execution_frames = self.execution_frames.lock().unwrap().clone();

        // Map ExecutionFrame -> SerializedFrame
        let frames: Vec<SerializedFrame> = execution_frames
            .iter()
            .map(|ef| SerializedFrame {
                function_index: ef.func_id,
                return_ip: ef.ip,
                base_pointer: ef.locals_base,
                locals: Vec::new(), // locals live on the shared stack, not per-frame
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
                is_closure: false, // not persisted — closures are on the stack
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
            BlockedReason::Other(_) => SuspendReason::IoWait, // best-effort for non-resumable reasons
        });

        // Determine current_func_id from the topmost execution frame or the root function
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
            state: Mutex::new(serialized.state),
            function_id: serialized.function_index,
            module,
            stack: Mutex::new(stack),
            ip: AtomicUsize::new(serialized.ip),
            result: Mutex::new(serialized.result),
            waiters: Mutex::new(Vec::new()),
            parent: serialized.parent,
            preempt_requested: AtomicBool::new(false),
            preempt_count: AtomicU32::new(0),
            start_time: Mutex::new(None),
            exception_handlers: Mutex::new(Vec::new()),
            current_exception: Mutex::new(None),
            caught_exception: Mutex::new(None),
            held_mutexes: Mutex::new(Vec::new()),
            closure_stack: Mutex::new(Vec::new()),
            call_stack: Mutex::new(Vec::new()),
            suspend_reason: Mutex::new(suspend_reason),
            resume_value: Mutex::new(None),
            initial_args: Mutex::new(Vec::new()),
            awaiting_task: Mutex::new(None),
            cancelled: AtomicBool::new(false),
            completion_lock: ParkingMutex::new(false),
            completion_condvar: ParkingCondvar::new(),
            current_func_id: AtomicUsize::new(current_func_id),
            current_locals_base: AtomicUsize::new(current_locals_base),
            execution_frames: Mutex::new(execution_frames),
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
