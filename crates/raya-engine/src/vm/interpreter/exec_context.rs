//! Execution context abstraction for opcode handlers
//!
//! This module provides the `ExecutionContext` trait that abstracts over
//! async (task-based) and sync (nested call) execution modes. This allows
//! opcode handlers to be written once and work in both contexts.
//!
//! # Design
//!
//! The key insight is that most opcodes (90%) have identical behavior in both
//! async and sync contexts - they just manipulate the stack and local variables.
//! Only a few opcodes (suspension points, blocking I/O) need different behavior.
//!
//! The `ExecutionContext` trait provides:
//! - Stack access (same for both contexts)
//! - Suspension capability check (async: yes, sync: no)
//! - Call/return handling (async: may suspend, sync: must complete)
//!
//! # Example
//!
//! ```rust,ignore
//! fn handle_iadd<C: ExecutionContext>(ctx: &mut C) -> Result<ControlFlow, VmError> {
//!     let stack = ctx.stack_mut();
//!     let b = stack.pop()?.as_i32()?;
//!     let a = stack.pop()?.as_i32()?;
//!     stack.push(Value::i32(a.wrapping_add(b)))?;
//!     Ok(ControlFlow::Continue)
//! }
//! ```

use super::execution::ControlFlow;
use crate::compiler::Module;
use crate::vm::scheduler::{SuspendReason, Task};
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

/// Execution context for opcode handlers
///
/// This trait abstracts over async (task-based) and sync (nested call) execution.
/// It provides a unified interface for opcode handlers while allowing context-specific
/// behavior for suspension points and call handling.
pub trait ExecutionContext {
    /// Get mutable reference to the execution stack
    fn stack_mut(&mut self) -> &mut Stack;

    /// Get immutable reference to the execution stack
    fn stack(&self) -> &Stack;

    /// Can this context suspend execution?
    ///
    /// Returns:
    /// - `true` for async contexts (can await, sleep, block on I/O)
    /// - `false` for sync contexts (must complete synchronously)
    fn can_suspend(&self) -> bool;

    /// Request suspension of execution
    ///
    /// This is called by opcodes that need to suspend (Await, Sleep, MutexLock, etc.).
    ///
    /// # Behavior
    ///
    /// - **AsyncContext**: Returns `Ok(ControlFlow::Suspend(reason))`
    /// - **SyncContext**: Returns `Err(VmError::RuntimeError)` - cannot suspend
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Await opcode implementation
    /// fn handle_await<C: ExecutionContext>(ctx: &mut C) -> Result<ControlFlow, VmError> {
    ///     let task_id = ctx.stack_mut().pop()?.as_task_id()?;
    ///     ctx.request_suspend(SuspendReason::AwaitTask(task_id))
    /// }
    /// ```
    fn request_suspend(&mut self, reason: SuspendReason) -> Result<ControlFlow, VmError>;

    /// Handle a function call
    ///
    /// This is called by the Call opcode and needs context-specific behavior:
    ///
    /// - **AsyncContext**: Push call frame, continue execution (may suspend later)
    /// - **SyncContext**: Execute function synchronously and return result
    fn handle_call(
        &mut self,
        interpreter: &mut super::core::Interpreter,
        task: &Arc<Task>,
        func_index: usize,
        args: Vec<Value>,
        module: &Module,
    ) -> Result<ControlFlow, VmError>;

    /// Handle return from function
    ///
    /// This is called by Return/ReturnVoid opcodes.
    ///
    /// - **AsyncContext**: Return control to scheduler
    /// - **SyncContext**: Return value to caller
    fn handle_return(&mut self, value: Value) -> Result<ControlFlow, VmError>;
}

// ============================================================================
// AsyncContext - For normal task execution (can suspend)
// ============================================================================

/// Async execution context (task-based execution)
///
/// This context represents normal task execution where suspension is allowed.
/// It uses the task's own stack and can yield control to the scheduler when
/// needed (await, sleep, blocking I/O).
///
/// # Stack Access
///
/// Note: Currently we need a mutable reference to the stack passed in from
/// the interpreter. In the future, we might refactor Task to provide direct
/// stack access via interior mutability.
pub struct AsyncContext<'a> {
    /// Reference to the task's execution stack
    ///
    /// TODO: Consider moving this into Task with RefCell/Mutex for direct access
    stack: &'a mut Stack,
}

impl<'a> AsyncContext<'a> {
    /// Create a new async execution context
    ///
    /// # Arguments
    ///
    /// * `stack` - Mutable reference to the task's execution stack
    pub fn new(stack: &'a mut Stack) -> Self {
        AsyncContext { stack }
    }
}

impl<'a> ExecutionContext for AsyncContext<'a> {
    fn stack_mut(&mut self) -> &mut Stack {
        self.stack
    }

    fn stack(&self) -> &Stack {
        self.stack
    }

    fn can_suspend(&self) -> bool {
        true
    }

    fn request_suspend(&mut self, reason: SuspendReason) -> Result<ControlFlow, VmError> {
        // Async context can suspend - return suspend control flow
        Ok(ControlFlow::Suspend(reason))
    }

    fn handle_call(
        &mut self,
        _interpreter: &mut super::core::Interpreter,
        _task: &Arc<Task>,
        _func_index: usize,
        _args: Vec<Value>,
        _module: &Module,
    ) -> Result<ControlFlow, VmError> {
        // For async context, calls are handled by the main execution loop
        // (push frame, continue execution). We just return Continue here
        // and let the interpreter handle the actual call setup.
        //
        // TODO: Move call frame setup logic here once we refactor the main loop
        Ok(ControlFlow::Continue)
    }

    fn handle_return(&mut self, value: Value) -> Result<ControlFlow, VmError> {
        // Return the value - the main loop will handle returning to caller
        Ok(ControlFlow::Return(value))
    }
}

// ============================================================================
// SyncContext - For nested synchronous calls (cannot suspend)
// ============================================================================

/// Synchronous execution context (nested calls)
///
/// This context represents synchronous nested function execution where
/// suspension is NOT allowed. It uses a local stack and must complete
/// the entire call tree synchronously.
///
/// This is used for:
/// - Reflect API calls (need immediate results)
/// - Builtin method calls from native code
/// - Any operation that can't yield to the scheduler
///
/// # Restrictions
///
/// - Cannot await other tasks
/// - Cannot sleep
/// - Cannot block on mutex/channel (errors if not immediately available)
pub struct SyncContext {
    /// Local execution stack for this nested call
    ///
    /// Each nested call gets its own stack to maintain isolation from
    /// the parent task's stack.
    stack: Stack,
}

impl SyncContext {
    /// Create a new sync execution context
    ///
    /// # Arguments
    ///
    /// * `local_count` - Number of local variables to allocate
    /// * `args` - Arguments to initialize locals with
    pub fn new(local_count: usize, args: Vec<Value>) -> Self {
        let mut stack = Stack::new();

        // Initialize locals
        for _ in 0..local_count {
            // Safe to unwrap - new stack can't overflow
            stack.push(Value::null()).unwrap();
        }

        // Set arguments
        for (i, arg) in args.into_iter().enumerate() {
            if i < local_count {
                // Safe to unwrap - we just allocated these slots
                stack.set_at(i, arg).unwrap();
            }
        }

        SyncContext { stack }
    }
}

impl ExecutionContext for SyncContext {
    fn stack_mut(&mut self) -> &mut Stack {
        &mut self.stack
    }

    fn stack(&self) -> &Stack {
        &self.stack
    }

    fn can_suspend(&self) -> bool {
        false
    }

    fn request_suspend(&mut self, reason: SuspendReason) -> Result<ControlFlow, VmError> {
        // Sync context cannot suspend - this is a runtime error
        Err(VmError::RuntimeError(format!(
            "Cannot suspend in synchronous nested call: {:?}",
            reason
        )))
    }

    fn handle_call(
        &mut self,
        interpreter: &mut super::core::Interpreter,
        task: &Arc<Task>,
        func_index: usize,
        args: Vec<Value>,
        module: &Module,
    ) -> Result<ControlFlow, VmError> {
        // For sync context, we need to execute the call recursively and
        // complete it synchronously
        //
        // Call the nested function executor
        let result = interpreter.execute_nested_function(task, func_index, args, module)?;

        // Push the result onto our stack
        self.stack.push(result)?;

        // Continue execution
        Ok(ControlFlow::Continue)
    }

    fn handle_return(&mut self, value: Value) -> Result<ControlFlow, VmError> {
        // For sync context, return immediately exits the nested call
        Ok(ControlFlow::Return(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_async_context_can_suspend() {
        let mut stack = Stack::new();
        let ctx = AsyncContext::new(&mut stack);
        assert!(ctx.can_suspend());
    }

    #[test]
    fn test_sync_context_cannot_suspend() {
        let ctx = SyncContext::new(0, vec![]);
        assert!(!ctx.can_suspend());
    }

    #[test]
    fn test_sync_context_suspend_error() {
        use crate::vm::scheduler::TaskId;
        let mut ctx = SyncContext::new(0, vec![]);
        let result = ctx.request_suspend(SuspendReason::AwaitTask(TaskId::from_u64(1)));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Cannot suspend"));
    }

    #[test]
    fn test_sync_context_initializes_locals() {
        let args = vec![Value::i32(10), Value::i32(20), Value::i32(30)];
        let ctx = SyncContext::new(5, args);

        // First 3 locals should be args
        assert_eq!(ctx.stack().peek_at(0).unwrap(), Value::i32(10));
        assert_eq!(ctx.stack().peek_at(1).unwrap(), Value::i32(20));
        assert_eq!(ctx.stack().peek_at(2).unwrap(), Value::i32(30));

        // Remaining locals should be null
        assert_eq!(ctx.stack().peek_at(3).unwrap(), Value::null());
        assert_eq!(ctx.stack().peek_at(4).unwrap(), Value::null());
    }
}
