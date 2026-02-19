//! Register-based execution types for the suspendable interpreter
//!
//! Parallel to `execution.rs` which defines stack-based types. These types
//! are used by the register-based interpreter path (`run_register()`).

use crate::vm::interpreter::execution::ReturnAction;
use crate::vm::scheduler::SuspendReason;
use crate::vm::value::Value;
use crate::vm::VmError;

/// Saved state of a register-based call frame
///
/// When a function is called, the caller's state is saved into a
/// `RegExecutionFrame` and pushed onto the frame stack. When the callee
/// returns, the frame is popped and execution resumes in the caller.
#[derive(Debug, Clone)]
pub struct RegExecutionFrame {
    /// Function index of the caller (to restore code reference)
    pub func_id: usize,
    /// Saved instruction pointer (word index past the Call instruction)
    pub ip: usize,
    /// Caller's register base in the RegisterFile
    pub reg_base: usize,
    /// Caller's register count (for documentation/debugging)
    pub reg_count: u16,
    /// Which register in the caller receives the return value
    pub dest_reg: u8,
    /// Whether the callee pushed a closure onto the closure stack
    pub is_closure: bool,
    /// What to do with the return value when popping this frame
    pub return_action: ReturnAction,
}

/// Result of executing a single register-based opcode
///
/// Used internally by the register interpreter to determine control flow.
#[derive(Debug)]
pub enum RegOpcodeResult {
    /// Continue to next instruction
    Continue,

    /// Jump to an absolute instruction index
    Jump(usize),

    /// Return from current function with a value
    Return(Value),

    /// Suspend the task with the given reason
    Suspend(SuspendReason),

    /// An error occurred
    Error(VmError),

    /// Push a new call frame (register-based calling convention)
    ///
    /// The main loop handles frame allocation, argument copying, and state saving.
    PushFrame {
        /// Function index to call
        func_id: usize,
        /// First argument register offset in caller's frame (rB from Call instruction)
        arg_base: u8,
        /// Number of arguments to copy (C from Call instruction)
        arg_count: u8,
        /// Caller's destination register for the return value (rA from Call instruction)
        dest_reg: u8,
        /// Whether this is a closure call
        is_closure: bool,
        /// Closure value (if is_closure is true)
        closure_val: Option<Value>,
        /// What to do with the return value
        return_action: ReturnAction,
    },
}

impl RegOpcodeResult {
    /// Create a continue result
    pub fn cont() -> Self {
        RegOpcodeResult::Continue
    }

    /// Create a return result
    pub fn ret(value: Value) -> Self {
        RegOpcodeResult::Return(value)
    }

    /// Create a suspend result
    pub fn suspend(reason: SuspendReason) -> Self {
        RegOpcodeResult::Suspend(reason)
    }

    /// Create an error result
    pub fn error(e: VmError) -> Self {
        RegOpcodeResult::Error(e)
    }

    /// Create an error from a string message
    pub fn runtime_error(msg: impl Into<String>) -> Self {
        RegOpcodeResult::Error(VmError::RuntimeError(msg.into()))
    }
}
