//! Execution result types for the suspendable interpreter
//!
//! This module defines the result types returned by the interpreter when
//! executing a task. The interpreter can complete, suspend, or fail.

use crate::vm::scheduler::SuspendReason;
use crate::vm::value::Value;
use crate::vm::VmError;

/// Result of executing a task
///
/// The interpreter returns this to indicate what happened during execution.
/// The worker loop uses this to decide what to do next:
/// - `Completed`: Task is done, wake up waiters
/// - `Suspended`: Task is waiting, register with appropriate wait mechanism
/// - `Failed`: Task errored, propagate exception or fail
#[derive(Debug)]
pub enum ExecutionResult {
    /// Task completed successfully with a return value
    Completed(Value),

    /// Task is suspended waiting for something
    /// The task's state has been saved and can be resumed later
    Suspended(SuspendReason),

    /// Task failed with an error
    Failed(VmError),
}

impl ExecutionResult {
    /// Create a completed result with null value
    pub fn completed_null() -> Self {
        ExecutionResult::Completed(Value::null())
    }

    /// Create a completed result with a value
    pub fn completed(value: Value) -> Self {
        ExecutionResult::Completed(value)
    }

    /// Create a suspended result
    pub fn suspended(reason: SuspendReason) -> Self {
        ExecutionResult::Suspended(reason)
    }

    /// Create a failed result
    pub fn failed(error: VmError) -> Self {
        ExecutionResult::Failed(error)
    }

    /// Check if the result is completed
    pub fn is_completed(&self) -> bool {
        matches!(self, ExecutionResult::Completed(_))
    }

    /// Check if the result is suspended
    pub fn is_suspended(&self) -> bool {
        matches!(self, ExecutionResult::Suspended(_))
    }

    /// Check if the result is failed
    pub fn is_failed(&self) -> bool {
        matches!(self, ExecutionResult::Failed(_))
    }
}

/// Result of executing a single opcode
///
/// Used internally by the interpreter to determine control flow.
#[derive(Debug)]
pub enum OpcodeResult {
    /// Continue to next instruction
    Continue,

    /// Return from current function with a value
    Return(Value),

    /// Suspend the task with the given reason
    Suspend(SuspendReason),

    /// An error occurred
    Error(VmError),

    /// Push a new call frame (frame-based interpreter)
    ///
    /// The main loop handles this by saving the current state into a
    /// `ExecutionFrame`, then setting up execution of the new function.
    PushFrame {
        func_id: usize,
        arg_count: usize,
        is_closure: bool,
        closure_val: Option<Value>,
        return_action: ReturnAction,
    },
}

/// What to do with the return value when popping a call frame
#[derive(Debug, Clone, Copy)]
pub enum ReturnAction {
    /// Push the callee's return value onto the caller's stack (normal calls)
    PushReturnValue,
    /// Push a specific object value (constructor calls - push the constructed object)
    PushObject(Value),
    /// Discard the return value (super() calls)
    Discard,
}

/// Saved state of a call frame for the frame-based interpreter
///
/// When a function is called, the current state is saved into an
/// `ExecutionFrame` and pushed onto the frame stack. When the function
/// returns, the frame is popped and execution resumes in the caller.
#[derive(Debug, Clone)]
pub struct ExecutionFrame {
    /// Function index of the caller (to restore code reference)
    pub func_id: usize,
    /// Saved instruction pointer (points past the Call instruction)
    pub ip: usize,
    /// Caller's locals base offset in the stack
    pub locals_base: usize,
    /// Whether the callee pushed a closure onto the closure stack
    pub is_closure: bool,
    /// What to push on the caller's stack when the callee returns
    pub return_action: ReturnAction,
    /// Argument count for the current call (for rest parameters)
    pub arg_count: usize,
}

impl OpcodeResult {
    /// Create a continue result
    pub fn cont() -> Self {
        OpcodeResult::Continue
    }

    /// Create a return result
    pub fn ret(value: Value) -> Self {
        OpcodeResult::Return(value)
    }

    /// Create a suspend result
    pub fn suspend(reason: SuspendReason) -> Self {
        OpcodeResult::Suspend(reason)
    }

    /// Create an error result
    pub fn error(e: VmError) -> Self {
        OpcodeResult::Error(e)
    }
}

// ============================================================================
// ControlFlow - New unified opcode execution result (Phase 1)
// ============================================================================

/// Control flow directive from opcode execution
///
/// This enum represents the result of executing a single opcode in the
/// unified dispatcher. It replaces `OpcodeResult` and is used by both
/// async and sync execution contexts.
///
/// # Differences from OpcodeResult
///
/// - Adds `Jump` variant for control flow opcodes
/// - Adds `Exception` variant for exception handling
/// - Uses `Result<ControlFlow, VmError>` instead of embedding errors
///
/// # Usage
///
/// ```rust,ignore
/// fn handle_iadd<C: ExecutionContext>(ctx: &mut C) -> Result<ControlFlow, VmError> {
///     let stack = ctx.stack_mut();
///     let b = stack.pop()?.as_i32()?;
///     let a = stack.pop()?.as_i32()?;
///     stack.push(Value::i32(a.wrapping_add(b)))?;
///     Ok(ControlFlow::Continue)
/// }
/// ```
#[derive(Debug)]
pub enum ControlFlow {
    /// Continue to next instruction
    Continue,

    /// Suspend execution with given reason
    ///
    /// Only valid in async contexts. Sync contexts will return an error
    /// if they try to suspend.
    Suspend(SuspendReason),

    /// Return from current function with a value
    Return(Value),

    /// Jump to a specific instruction offset
    ///
    /// Used by Jump, JumpIf, JumpIfNot opcodes.
    Jump(usize),

    /// Exception was thrown
    ///
    /// The value is the exception object. The interpreter will search for
    /// an exception handler or propagate to the caller.
    Exception(Value),
}

impl ControlFlow {
    /// Create a continue control flow
    pub fn cont() -> Self {
        ControlFlow::Continue
    }

    /// Create a suspend control flow
    pub fn suspend(reason: SuspendReason) -> Self {
        ControlFlow::Suspend(reason)
    }

    /// Create a return control flow
    pub fn ret(value: Value) -> Self {
        ControlFlow::Return(value)
    }

    /// Create a jump control flow
    pub fn jump(offset: usize) -> Self {
        ControlFlow::Jump(offset)
    }

    /// Create an exception control flow
    pub fn exception(value: Value) -> Self {
        ControlFlow::Exception(value)
    }
}
