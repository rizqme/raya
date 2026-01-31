//! Task-based interpreter that can suspend and resume
//!
//! This interpreter executes a single task until it completes, suspends, or fails.
//! Unlike the synchronous `Vm`, this interpreter returns control to the scheduler
//! when the task needs to wait for something.

use super::execution::{ExecutionResult, OpcodeResult};
use super::{ClassRegistry, SafepointCoordinator};
use crate::compiler::{Module, Opcode};
use crate::vm::gc::GarbageCollector;
use crate::compiler::native_id::{CHANNEL_SEND, CHANNEL_RECEIVE, CHANNEL_TRY_SEND, CHANNEL_TRY_RECEIVE, CHANNEL_CLOSE, CHANNEL_IS_CLOSED, CHANNEL_LENGTH, CHANNEL_CAPACITY};
use crate::vm::builtin::{set, regexp, buffer};
use crate::vm::object::{Array, Buffer, ChannelObject, Closure, DateObject, MapObject, Object, RayaString, RegExpObject, SetObject};
use crate::vm::scheduler::{ExceptionHandler, SuspendReason, Task, TaskId, TaskState};
use crate::vm::stack::Stack;
use crate::vm::sync::{MutexId, MutexRegistry};
use crate::vm::value::Value;
use crate::vm::VmError;
use crossbeam_deque::Injector;
use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use std::time::Instant;

/// Helper to convert Value to f64, handling both f64 and i32 values
#[inline]
fn value_to_f64(v: Value) -> Result<f64, VmError> {
    if let Some(f) = v.as_f64() {
        Ok(f)
    } else if let Some(i) = v.as_i32() {
        Ok(i as f64)
    } else {
        Err(VmError::TypeError("Expected number".to_string()))
    }
}

/// Task interpreter that can suspend and resume
///
/// This struct holds references to shared state and executes a task.
/// The task's execution state (stack, IP, exception handlers, etc.) lives in the Task itself.
pub struct TaskInterpreter<'a> {
    /// Reference to the garbage collector
    gc: &'a parking_lot::Mutex<GarbageCollector>,

    /// Reference to the class registry
    classes: &'a RwLock<ClassRegistry>,

    /// Reference to the mutex registry
    mutex_registry: &'a MutexRegistry,

    /// Safepoint coordinator for GC
    safepoint: &'a SafepointCoordinator,

    /// Global variables by index
    globals_by_index: &'a RwLock<Vec<Value>>,

    /// Task registry (for spawn/await)
    tasks: &'a Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,

    /// Global task injector for scheduling spawned tasks
    injector: &'a Arc<Injector<Arc<Task>>>,
}

impl<'a> TaskInterpreter<'a> {
    /// Create a new task interpreter
    pub fn new(
        gc: &'a parking_lot::Mutex<GarbageCollector>,
        classes: &'a RwLock<ClassRegistry>,
        mutex_registry: &'a MutexRegistry,
        safepoint: &'a SafepointCoordinator,
        globals_by_index: &'a RwLock<Vec<Value>>,
        tasks: &'a Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,
        injector: &'a Arc<Injector<Arc<Task>>>,
    ) -> Self {
        Self {
            gc,
            classes,
            mutex_registry,
            safepoint,
            globals_by_index,
            tasks,
            injector,
        }
    }

    /// Execute a task until completion, suspension, or failure
    ///
    /// This is the main entry point for running a task. The task's state
    /// (stack, IP, exception handlers, etc.) is stored in the Task itself.
    pub fn run(&mut self, task: &Arc<Task>) -> ExecutionResult {
        // Get the module and function
        let module = task.module();
        let function_id = task.function_id();

        let function = match module.functions.get(function_id) {
            Some(f) => f,
            None => {
                return ExecutionResult::Failed(VmError::RuntimeError(format!(
                    "Function {} not found",
                    function_id
                )));
            }
        };

        // Get task's execution state
        let mut stack_guard = task.stack().lock().unwrap();
        let mut ip = task.ip();
        let code = &function.code;

        // Check if we're resuming from suspension
        if let Some(resume_value) = task.take_resume_value() {
            // Push the resume value onto the stack
            if let Err(e) = stack_guard.push(resume_value) {
                return ExecutionResult::Failed(e);
            }
        }

        // Check if there's a pending exception (e.g., from awaited task failure)
        if task.has_exception() {
            // Handle the propagated exception
            match self.handle_exception(task, &mut stack_guard, &mut ip) {
                Ok(()) => {
                    // Exception was handled, continue execution from the handler
                }
                Err(()) => {
                    // No handler found, propagate error
                    let exc = task.current_exception().unwrap_or_else(|| Value::null());
                    task.set_ip(ip);
                    drop(stack_guard);
                    return ExecutionResult::Failed(VmError::RuntimeError(format!(
                        "Unhandled exception from awaited task: {:?}",
                        exc
                    )));
                }
            }
        }

        // Initialize the task if this is a fresh start
        if ip == 0 && stack_guard.depth() == 0 {
            // Push initial call frame for stack traces
            task.push_call_frame(function_id);

            // Allocate space for local variables
            for _ in 0..function.local_count {
                if let Err(e) = stack_guard.push(Value::null()) {
                    return ExecutionResult::Failed(e);
                }
            }

            // Set initial arguments as the first N locals
            let initial_args = task.take_initial_args();
            for (i, arg) in initial_args.into_iter().enumerate() {
                if i < function.local_count as usize {
                    if let Err(e) = stack_guard.set_at(i, arg) {
                        return ExecutionResult::Failed(e);
                    }
                }
            }
        }

        let locals_base = 0;

        // Main execution loop
        loop {
            // Safepoint poll for GC
            self.safepoint.poll();

            // Check for preemption
            if task.is_preempt_requested() {
                task.clear_preempt();
                // Save state and yield
                task.set_ip(ip);
                drop(stack_guard);
                return ExecutionResult::Suspended(SuspendReason::Sleep {
                    wake_at: Instant::now(), // Immediate reschedule
                });
            }

            // Check for cancellation
            if task.is_cancelled() {
                task.set_ip(ip);
                drop(stack_guard);
                return ExecutionResult::Failed(VmError::RuntimeError(
                    "Task cancelled".to_string(),
                ));
            }

            // Bounds check
            if ip >= code.len() {
                // Implicit return at end of function
                let return_value = if stack_guard.depth() > function.local_count as usize {
                    stack_guard.pop().unwrap_or(Value::null())
                } else {
                    Value::null()
                };
                return ExecutionResult::Completed(return_value);
            }

            // Fetch and decode opcode
            let opcode_byte = code[ip];
            let opcode = match Opcode::from_u8(opcode_byte) {
                Some(op) => op,
                None => {
                    return ExecutionResult::Failed(VmError::InvalidOpcode(opcode_byte));
                }
            };

            ip += 1;

            // Execute the opcode
            match self.execute_opcode(
                task,
                &mut stack_guard,
                &mut ip,
                code,
                module,
                opcode,
                locals_base,
            ) {
                OpcodeResult::Continue => {
                    // Continue to next instruction
                }
                OpcodeResult::Return(value) => {
                    return ExecutionResult::Completed(value);
                }
                OpcodeResult::Suspend(reason) => {
                    // Save state and return
                    task.set_ip(ip);
                    drop(stack_guard);
                    return ExecutionResult::Suspended(reason);
                }
                OpcodeResult::Error(e) => {
                    // Set exception on task if not already set
                    if !task.has_exception() {
                        // Convert VmError to exception value
                        let error_msg = e.to_string();
                        let raya_string = RayaString::new(error_msg);
                        let gc_ptr = self.gc.lock().allocate(raya_string);
                        let exc_val =
                            unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        task.set_exception(exc_val);
                    }

                    // Try to handle exception
                    match self.handle_exception(task, &mut stack_guard, &mut ip) {
                        Ok(()) => {
                            // Exception was handled, continue execution
                        }
                        Err(()) => {
                            // No handler found, propagate error
                            task.set_ip(ip);
                            drop(stack_guard);
                            return ExecutionResult::Failed(e);
                        }
                    }
                }
            }
        }
    }

    /// Handle an exception by unwinding to the nearest handler
    ///
    /// Returns Ok(()) if exception was handled, Err(()) if no handler found.
    fn handle_exception(
        &mut self,
        task: &Arc<Task>,
        stack: &mut std::sync::MutexGuard<Stack>,
        ip: &mut usize,
    ) -> Result<(), ()> {
        // Get exception value - if not set, create a placeholder
        let exception = task.current_exception().unwrap_or_else(|| {
            let raya_string = RayaString::new("Unknown error".to_string());
            let gc_ptr = self.gc.lock().allocate(raya_string);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
        });

        // Try to find a handler
        loop {
            if let Some(handler) = task.peek_exception_handler() {
                // Unwind stack to handler's saved state
                while stack.depth() > handler.stack_size {
                    let _ = stack.pop();
                }

                // Jump to catch block if present
                if handler.catch_offset != -1 {
                    task.pop_exception_handler();
                    task.set_caught_exception(exception);
                    task.clear_exception();
                    let _ = stack.push(exception);
                    *ip = handler.catch_offset as usize;
                    return Ok(());
                }

                // No catch block, execute finally block if present
                if handler.finally_offset != -1 {
                    task.pop_exception_handler();
                    *ip = handler.finally_offset as usize;
                    return Ok(());
                }

                // No catch or finally, remove handler and continue unwinding
                task.pop_exception_handler();
            } else {
                // No handler found - store exception
                task.set_exception(exception);
                return Err(());
            }
        }
    }

    /// Execute a single opcode
    #[allow(clippy::too_many_arguments)]
    fn execute_opcode(
        &mut self,
        task: &Arc<Task>,
        stack: &mut std::sync::MutexGuard<Stack>,
        ip: &mut usize,
        code: &[u8],
        module: &Module,
        opcode: Opcode,
        locals_base: usize,
    ) -> OpcodeResult {
        match opcode {
            // =========================================================
            // Stack Manipulation
            // =========================================================
            Opcode::Nop => OpcodeResult::Continue,

            Opcode::Pop => {
                if let Err(e) = stack.pop() {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Dup => {
                match stack.peek() {
                    Ok(value) => {
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    Err(e) => return OpcodeResult::Error(e),
                }
                OpcodeResult::Continue
            }

            Opcode::Swap => {
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(a) {
                    return OpcodeResult::Error(e);
                }
                if let Err(e) = stack.push(b) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Constants
            // =========================================================
            Opcode::ConstNull => {
                if let Err(e) = stack.push(Value::null()) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ConstTrue => {
                if let Err(e) = stack.push(Value::bool(true)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ConstFalse => {
                if let Err(e) = stack.push(Value::bool(false)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ConstI32 => {
                let value = match Self::read_i32(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(value)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ConstF64 => {
                let value = match Self::read_f64(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(value)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ConstStr => {
                let index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let s = match module.constants.strings.get(index) {
                    Some(s) => s,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid string constant index: {}",
                            index
                        )));
                    }
                };
                let raya_string = RayaString::new(s.clone());
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Local Variables
            // =========================================================
            Opcode::LoadLocal => {
                let index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.peek_at(locals_base + index) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreLocal => {
                let index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.set_at(locals_base + index, value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadLocal0 => {
                let value = match stack.peek_at(locals_base) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadLocal1 => {
                let value = match stack.peek_at(locals_base + 1) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreLocal0 => {
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.set_at(locals_base, value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreLocal1 => {
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.set_at(locals_base + 1, value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Global Variables
            // =========================================================
            Opcode::LoadGlobal => {
                let index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let globals = self.globals_by_index.read();
                let value = globals.get(index).copied().unwrap_or(Value::null());
                drop(globals);
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreGlobal => {
                let index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let mut globals = self.globals_by_index.write();
                if index >= globals.len() {
                    globals.resize(index + 1, Value::null());
                }
                globals[index] = value;
                OpcodeResult::Continue
            }

            // =========================================================
            // Integer Arithmetic
            // =========================================================
            Opcode::Iadd => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a.wrapping_add(b))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Isub => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a.wrapping_sub(b))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Imul => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a.wrapping_mul(b))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Idiv => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if b == 0 {
                    return OpcodeResult::Error(VmError::RuntimeError(
                        "Division by zero".to_string(),
                    ));
                }
                if let Err(e) = stack.push(Value::i32(a / b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Imod => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if b == 0 {
                    return OpcodeResult::Error(VmError::RuntimeError(
                        "Division by zero".to_string(),
                    ));
                }
                if let Err(e) = stack.push(Value::i32(a % b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ineg => {
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(-a)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ipow => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a.pow(b as u32))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Integer Bitwise
            // =========================================================
            Opcode::Ishl => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a << (b & 31))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ishr => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a >> (b & 31))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Iushr => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(((a as u32) >> (b & 31)) as i32)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Iand => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a & b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ior => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a | b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ixor => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a ^ b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Inot => {
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(!a)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Integer Comparisons
            // =========================================================
            Opcode::Ieq => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a == b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ine => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a != b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ilt => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a < b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ile => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a <= b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Igt => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a > b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ige => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a >= b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Float Arithmetic
            // =========================================================
            Opcode::Fadd => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(a + b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fsub => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(a - b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fmul => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(a * b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fdiv => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(a / b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fneg => {
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(-a)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fpow => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(a.powf(b))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fmod => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(a % b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Float Comparisons
            // =========================================================
            Opcode::Feq => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a == b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fne => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a != b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Flt => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a < b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fle => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a <= b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fgt => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a > b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fge => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a >= b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Generic Number Operations (N = handles both i32 and f64)
            // =========================================================
            Opcode::Nadd | Opcode::Nsub | Opcode::Nmul | Opcode::Ndiv | Opcode::Nmod | Opcode::Nneg | Opcode::Npow => {
                // Helper to convert value to f64
                fn value_to_number(v: Value) -> f64 {
                    if let Some(f) = v.as_f64() {
                        f
                    } else if let Some(i) = v.as_i32() {
                        i as f64
                    } else if let Some(i) = v.as_i64() {
                        i as f64
                    } else {
                        0.0
                    }
                }

                // Helper to check if value is f64
                fn is_float(v: &Value) -> bool {
                    v.is_f64()
                }

                match opcode {
                    Opcode::Nadd => {
                        let b_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let a_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        // If either is float, use float arithmetic
                        let result = if is_float(&a_val) || is_float(&b_val) {
                            Value::f64(value_to_number(a_val) + value_to_number(b_val))
                        } else {
                            let a = a_val.as_i32().unwrap_or(0);
                            let b = b_val.as_i32().unwrap_or(0);
                            Value::i32(a.wrapping_add(b))
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    Opcode::Nsub => {
                        let b_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let a_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let result = if is_float(&a_val) || is_float(&b_val) {
                            Value::f64(value_to_number(a_val) - value_to_number(b_val))
                        } else {
                            let a = a_val.as_i32().unwrap_or(0);
                            let b = b_val.as_i32().unwrap_or(0);
                            Value::i32(a.wrapping_sub(b))
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    Opcode::Nmul => {
                        let b_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let a_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let result = if is_float(&a_val) || is_float(&b_val) {
                            Value::f64(value_to_number(a_val) * value_to_number(b_val))
                        } else {
                            let a = a_val.as_i32().unwrap_or(0);
                            let b = b_val.as_i32().unwrap_or(0);
                            Value::i32(a.wrapping_mul(b))
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    Opcode::Ndiv => {
                        // Division always returns f64
                        let b_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let a_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let a = value_to_number(a_val);
                        let b = value_to_number(b_val);
                        let result = if b != 0.0 { a / b } else { f64::NAN };
                        if let Err(e) = stack.push(Value::f64(result)) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    Opcode::Nmod => {
                        let b_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let a_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let result = if is_float(&a_val) || is_float(&b_val) {
                            let a = value_to_number(a_val);
                            let b = value_to_number(b_val);
                            Value::f64(if b != 0.0 { a % b } else { f64::NAN })
                        } else {
                            let a = a_val.as_i32().unwrap_or(0);
                            let b = b_val.as_i32().unwrap_or(1);
                            Value::i32(if b != 0 { a % b } else { 0 })
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    Opcode::Nneg => {
                        let a_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let result = if is_float(&a_val) {
                            Value::f64(-value_to_number(a_val))
                        } else {
                            Value::i32(-a_val.as_i32().unwrap_or(0))
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    Opcode::Npow => {
                        let b_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let a_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let result = if is_float(&a_val) || is_float(&b_val) {
                            Value::f64(value_to_number(a_val).powf(value_to_number(b_val)))
                        } else {
                            let a = a_val.as_i32().unwrap_or(0);
                            let b = b_val.as_i32().unwrap_or(0);
                            Value::i32(a.pow(b as u32))
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    _ => unreachable!(),
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Boolean Operations
            // =========================================================
            Opcode::Not => {
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(!a.is_truthy())) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::And => {
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a.is_truthy() && b.is_truthy())) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Or => {
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a.is_truthy() || b.is_truthy())) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Generic Equality
            // =========================================================
            Opcode::Eq | Opcode::StrictEq => {
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a == b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ne | Opcode::StrictNe => {
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a != b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Control Flow
            // =========================================================
            Opcode::Jmp => {
                let offset = match Self::read_i16(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if offset < 0 {
                    self.safepoint.poll();
                }
                *ip = (*ip as isize + offset as isize) as usize;
                OpcodeResult::Continue
            }

            Opcode::JmpIfTrue => {
                let offset = match Self::read_i16(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let cond = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if cond.is_truthy() {
                    *ip = (*ip as isize + offset as isize) as usize;
                }
                OpcodeResult::Continue
            }

            Opcode::JmpIfFalse => {
                let offset = match Self::read_i16(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let cond = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if !cond.is_truthy() {
                    *ip = (*ip as isize + offset as isize) as usize;
                }
                OpcodeResult::Continue
            }

            Opcode::JmpIfNull => {
                let offset = match Self::read_i16(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if value.is_null() {
                    *ip = (*ip as isize + offset as isize) as usize;
                }
                OpcodeResult::Continue
            }

            Opcode::JmpIfNotNull => {
                let offset = match Self::read_i16(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if !value.is_null() {
                    *ip = (*ip as isize + offset as isize) as usize;
                }
                OpcodeResult::Continue
            }

            Opcode::Return => {
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(_) => Value::null(),
                };
                OpcodeResult::Return(value)
            }

            Opcode::ReturnVoid => OpcodeResult::Return(Value::null()),

            // =========================================================
            // Exception Handling
            // =========================================================
            Opcode::Try => {
                let catch_rel = match Self::read_i32(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let catch_abs = if catch_rel >= 0 {
                    (*ip as i32 + catch_rel) as i32
                } else {
                    -1
                };

                let finally_rel = match Self::read_i32(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let finally_abs = if finally_rel > 0 {
                    (*ip as i32 + finally_rel) as i32
                } else {
                    -1
                };

                let handler = ExceptionHandler {
                    catch_offset: catch_abs,
                    finally_offset: finally_abs,
                    stack_size: stack.depth(),
                    frame_count: 0,
                    mutex_count: task.held_mutex_count(),
                };
                task.push_exception_handler(handler);
                OpcodeResult::Continue
            }

            Opcode::EndTry => {
                task.pop_exception_handler();
                OpcodeResult::Continue
            }

            Opcode::Throw => {
                eprintln!("[DEBUG] Throw opcode in execute_opcode (main)");
                let exception = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // If exception is an Error object, set its stack property
                if exception.is_ptr() {
                    if let Some(obj_ptr) = unsafe { exception.as_ptr::<Object>() } {
                        let obj = unsafe { &mut *obj_ptr.as_ptr() };
                        let classes = self.classes.read();

                        // Check if this is an Error or subclass (Error class has "name" and "stack" fields)
                        // Error fields: 0=message, 1=name, 2=stack
                        if let Some(class) = classes.get_class(obj.class_id) {
                            // Check if class is Error or inherits from Error
                            let is_error = class.name == "Error"
                                || class.name == "TypeError"
                                || class.name == "RangeError"
                                || class.name == "ReferenceError"
                                || class.name == "SyntaxError"
                                || class.name == "ChannelClosedError"
                                || class.name == "AssertionError"
                                || class.parent_id.is_some(); // Subclasses have parent

                            if is_error && obj.fields.len() >= 3 {
                                // Get error name and message
                                let error_name = if let Some(name_ptr) =
                                    unsafe { obj.fields[1].as_ptr::<RayaString>() }
                                {
                                    unsafe { &*name_ptr.as_ptr() }.data.clone()
                                } else {
                                    "Error".to_string()
                                };

                                let error_message = if let Some(msg_ptr) =
                                    unsafe { obj.fields[0].as_ptr::<RayaString>() }
                                {
                                    unsafe { &*msg_ptr.as_ptr() }.data.clone()
                                } else {
                                    String::new()
                                };

                                drop(classes);

                                // Build stack trace
                                let stack_trace =
                                    task.build_stack_trace(&error_name, &error_message);

                                // Allocate stack trace string
                                let raya_string = RayaString::new(stack_trace);
                                let gc_ptr = self.gc.lock().allocate(raya_string);
                                let stack_value = unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                    )
                                };

                                // Set stack field (index 2)
                                obj.fields[2] = stack_value;
                            }
                        }
                    }
                }

                task.set_exception(exception);
                OpcodeResult::Error(VmError::RuntimeError("throw".to_string()))
            }

            Opcode::Rethrow => {
                if let Some(exception) = task.caught_exception() {
                    task.set_exception(exception);
                    OpcodeResult::Error(VmError::RuntimeError("rethrow".to_string()))
                } else {
                    OpcodeResult::Error(VmError::RuntimeError(
                        "RETHROW with no active exception".to_string(),
                    ))
                }
            }

            // =========================================================
            // Function Calls
            // =========================================================
            Opcode::Call => {
                self.safepoint.poll();
                let func_index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };


                if func_index == 0xFFFFFFFF {
                    // Closure call
                    match self.call_closure(task, stack, arg_count, module, locals_base) {
                        Ok(result) => {
                            if let Err(e) = stack.push(result) {
                                return OpcodeResult::Error(e);
                            }
                        }
                        Err(e) => {
                            eprintln!("[DEBUG] Closure call returned error: {:?}", e);
                            return OpcodeResult::Error(e);
                        }
                    }
                } else {
                    // Regular function call
                    match self.call_function(task, stack, func_index, arg_count, module, locals_base)
                    {
                        Ok(result) => {
                            if let Err(e) = stack.push(result) {
                                return OpcodeResult::Error(e);
                            }
                        }
                        Err(e) => {
                            eprintln!("[DEBUG] Function call returned error: {:?}", e);
                            return OpcodeResult::Error(e);
                        }
                    }
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Object Operations
            // =========================================================
            Opcode::New => {
                self.safepoint.poll();
                let class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let classes = self.classes.read();
                let class = match classes.get_class(class_index) {
                    Some(c) => c,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid class index: {}",
                            class_index
                        )));
                    }
                };
                let field_count = class.field_count;
                drop(classes);

                let obj = Object::new(class_index, field_count);
                let gc_ptr = self.gc.lock().allocate(obj);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadField => {
                let field_offset = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !obj_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected object for field access".to_string(),
                    ));
                }

                let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                let value = match obj.get_field(field_offset) {
                    Some(v) => v,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Field offset {} out of bounds",
                            field_offset
                        )));
                    }
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreField => {
                let field_offset = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !obj_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected object for field access".to_string(),
                    ));
                }

                let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
                let obj = unsafe { &mut *obj_ptr.unwrap().as_ptr() };
                if let Err(e) = obj.set_field(field_offset, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                OpcodeResult::Continue
            }

            Opcode::OptionalField => {
                let field_offset = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // If null, return null (optional chaining semantics)
                if obj_val.is_null() {
                    if let Err(e) = stack.push(Value::null()) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }

                if !obj_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected object or null for optional field access".to_string(),
                    ));
                }

                let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                let value = match obj.get_field(field_offset) {
                    Some(v) => v,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Field offset {} out of bounds",
                            field_offset
                        )));
                    }
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadFieldFast => {
                let field_offset = match Self::read_u8(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !obj_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected object for field access".to_string(),
                    ));
                }

                let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                let value = match obj.get_field(field_offset) {
                    Some(v) => v,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Field offset {} out of bounds",
                            field_offset
                        )));
                    }
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreFieldFast => {
                let field_offset = match Self::read_u8(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !obj_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected object for field access".to_string(),
                    ));
                }

                let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
                let obj = unsafe { &mut *obj_ptr.unwrap().as_ptr() };
                if let Err(e) = obj.set_field(field_offset, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                OpcodeResult::Continue
            }

            Opcode::ObjectLiteral => {
                self.safepoint.poll();
                let class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let field_count = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let obj = Object::new(class_index, field_count);
                let gc_ptr = self.gc.lock().allocate(obj);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::InitObject => {
                let field_offset = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.peek() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !obj_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected object for field initialization".to_string(),
                    ));
                }

                let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
                let obj = unsafe { &mut *obj_ptr.unwrap().as_ptr() };
                if let Err(e) = obj.set_field(field_offset, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Array Operations
            // =========================================================
            Opcode::NewArray => {
                self.safepoint.poll();
                let type_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let len = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0) as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let arr = Array::new(type_index, len);
                let gc_ptr = self.gc.lock().allocate(arr);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadElem => {
                let index = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0) as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arr_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };


                if !arr_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected array".to_string()));
                }

                let arr_ptr = unsafe { arr_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                let value = match arr.get(index) {
                    Some(v) => {
                        v
                    }
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Array index {} out of bounds",
                            index
                        )));
                    }
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreElem => {
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let index = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0) as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arr_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !arr_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected array".to_string()));
                }

                let arr_ptr = unsafe { arr_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                if let Err(e) = arr.set(index, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                OpcodeResult::Continue
            }

            Opcode::ArrayLen => {
                let arr_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !arr_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected array".to_string()));
                }

                let arr_ptr = unsafe { arr_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                if let Err(e) = stack.push(Value::i32(arr.len() as i32)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ArrayLiteral => {
                self.safepoint.poll();
                let type_index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let length = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Pop elements from stack in reverse order (last pushed = last element)
                let mut elements = Vec::with_capacity(length);
                for _ in 0..length {
                    match stack.pop() {
                        Ok(v) => elements.push(v),
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }
                // Reverse to get correct order (first pushed = first element)
                elements.reverse();

                // Create array with the elements
                let mut arr = Array::new(type_index, length);
                for (i, elem) in elements.into_iter().enumerate() {
                    if let Err(e) = arr.set(i, elem) {
                        return OpcodeResult::Error(VmError::RuntimeError(e));
                    }
                }

                let gc_ptr = self.gc.lock().allocate(arr);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::InitArray => {
                let index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arr_val = match stack.peek() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !arr_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected array".to_string()));
                }

                let arr_ptr = unsafe { arr_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                if let Err(e) = arr.set(index, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Closure Operations
            // =========================================================
            Opcode::MakeClosure => {
                self.safepoint.poll();
                let func_index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let capture_count = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let mut captures = Vec::with_capacity(capture_count);
                for _ in 0..capture_count {
                    match stack.pop() {
                        Ok(v) => captures.push(v),
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }
                captures.reverse();

                let closure = Closure::new(func_index, captures);
                let gc_ptr = self.gc.lock().allocate(closure);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadCaptured => {
                let capture_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let closure_val = match task.current_closure() {
                    Some(v) => v,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(
                            "LoadCaptured without active closure".to_string(),
                        ));
                    }
                };

                let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
                let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                let value = match closure.get_captured(capture_index) {
                    Some(v) => v,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Capture index {} out of bounds",
                            capture_index
                        )));
                    }
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreCaptured => {
                let capture_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let closure_val = match task.current_closure() {
                    Some(v) => v,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(
                            "StoreCaptured without active closure".to_string(),
                        ));
                    }
                };

                let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
                let closure = unsafe { &mut *closure_ptr.unwrap().as_ptr() };
                if let Err(e) = closure.set_captured(capture_index, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                OpcodeResult::Continue
            }

            Opcode::SetClosureCapture => {
                let capture_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let closure_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !closure_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected closure".to_string()));
                }

                let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
                let closure = unsafe { &mut *closure_ptr.unwrap().as_ptr() };
                if let Err(e) = closure.set_captured(capture_index, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                if let Err(e) = stack.push(closure_val) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // RefCell Operations
            // =========================================================
            Opcode::NewRefCell => {
                use crate::vm::object::RefCell;
                let initial_value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let refcell = RefCell::new(initial_value);
                let gc_ptr = self.gc.lock().allocate(refcell);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadRefCell => {
                use crate::vm::object::RefCell;
                let refcell_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !refcell_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected RefCell".to_string()));
                }

                let refcell_ptr = unsafe { refcell_val.as_ptr::<RefCell>() };
                let refcell = unsafe { &*refcell_ptr.unwrap().as_ptr() };
                if let Err(e) = stack.push(refcell.get()) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreRefCell => {
                use crate::vm::object::RefCell;
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let refcell_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !refcell_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected RefCell".to_string()));
                }

                let refcell_ptr = unsafe { refcell_val.as_ptr::<RefCell>() };
                let refcell = unsafe { &mut *refcell_ptr.unwrap().as_ptr() };
                refcell.set(value);
                OpcodeResult::Continue
            }

            // =========================================================
            // String Operations
            // =========================================================
            Opcode::Sconcat => {
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let a_str = if a_val.is_ptr() {
                    let ptr = unsafe { a_val.as_ptr::<RayaString>() };
                    unsafe { &*ptr.unwrap().as_ptr() }.data.clone()
                } else {
                    format!("{:?}", a_val)
                };

                let b_str = if b_val.is_ptr() {
                    let ptr = unsafe { b_val.as_ptr::<RayaString>() };
                    unsafe { &*ptr.unwrap().as_ptr() }.data.clone()
                } else {
                    format!("{:?}", b_val)
                };

                let result = RayaString::new(format!("{}{}", a_str, b_str));
                let gc_ptr = self.gc.lock().allocate(result);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Slen => {
                let s_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !s_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected string".to_string()));
                }

                let str_ptr = unsafe { s_val.as_ptr::<RayaString>() };
                let s = unsafe { &*str_ptr.unwrap().as_ptr() };
                if let Err(e) = stack.push(Value::i32(s.len() as i32)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Seq => {
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let result = if a_val.is_ptr() && b_val.is_ptr() {
                    let a_ptr = unsafe { a_val.as_ptr::<RayaString>() };
                    let b_ptr = unsafe { b_val.as_ptr::<RayaString>() };
                    let a = unsafe { &*a_ptr.unwrap().as_ptr() };
                    let b = unsafe { &*b_ptr.unwrap().as_ptr() };
                    a.data == b.data
                } else {
                    false
                };
                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Sne => {
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let result = if a_val.is_ptr() && b_val.is_ptr() {
                    let a_ptr = unsafe { a_val.as_ptr::<RayaString>() };
                    let b_ptr = unsafe { b_val.as_ptr::<RayaString>() };
                    let a = unsafe { &*a_ptr.unwrap().as_ptr() };
                    let b = unsafe { &*b_ptr.unwrap().as_ptr() };
                    a.data != b.data
                } else {
                    true
                };
                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Slt | Opcode::Sle | Opcode::Sgt | Opcode::Sge => {
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let result = if a_val.is_ptr() && b_val.is_ptr() {
                    let a_ptr = unsafe { a_val.as_ptr::<RayaString>() };
                    let b_ptr = unsafe { b_val.as_ptr::<RayaString>() };
                    let a = unsafe { &*a_ptr.unwrap().as_ptr() };
                    let b = unsafe { &*b_ptr.unwrap().as_ptr() };
                    match opcode {
                        Opcode::Slt => a.data < b.data,
                        Opcode::Sle => a.data <= b.data,
                        Opcode::Sgt => a.data > b.data,
                        Opcode::Sge => a.data >= b.data,
                        _ => unreachable!(),
                    }
                } else {
                    false
                };
                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ToString => {
                let val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                // Convert value to string properly
                let s = if val.is_null() {
                    "null".to_string()
                } else if let Some(b) = val.as_bool() {
                    if b { "true".to_string() } else { "false".to_string() }
                } else if let Some(i) = val.as_i32() {
                    i.to_string()
                } else if let Some(f) = val.as_f64() {
                    // Format float like JavaScript: no trailing zeros, no scientific notation for small numbers
                    if f.fract() == 0.0 && f.abs() < 1e15 {
                        (f as i64).to_string()
                    } else {
                        f.to_string()
                    }
                } else if val.is_ptr() {
                    // Check if it's already a string
                    if let Some(ptr) = unsafe { val.as_ptr::<RayaString>() } {
                        unsafe { &*ptr.as_ptr() }.data.clone()
                    } else {
                        "[object]".to_string()
                    }
                } else {
                    "undefined".to_string()
                };
                let result = RayaString::new(s);
                let gc_ptr = self.gc.lock().allocate(result);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Concurrency Operations - These SUSPEND instead of blocking
            // =========================================================
            Opcode::Spawn => {
                let func_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    match stack.pop() {
                        Ok(v) => args.push(v),
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }

                let new_task = Arc::new(Task::with_args(
                    func_index,
                    task.module().clone(),
                    Some(task.id()),
                    args,
                ));

                let task_id = new_task.id();
                self.tasks.write().insert(task_id, new_task.clone());
                self.injector.push(new_task);

                if let Err(e) = stack.push(Value::u64(task_id.as_u64())) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::SpawnClosure => {
                let arg_count = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let closure_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if !closure_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected closure".to_string()));
                }

                let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
                let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };

                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    match stack.pop() {
                        Ok(v) => args.push(v),
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }

                // Prepend captures to args
                let mut task_args = closure.captures.clone();
                task_args.extend(args);

                let new_task = Arc::new(Task::with_args(
                    closure.func_id,
                    task.module().clone(),
                    Some(task.id()),
                    task_args,
                ));

                let task_id = new_task.id();
                self.tasks.write().insert(task_id, new_task.clone());
                self.injector.push(new_task);

                if let Err(e) = stack.push(Value::u64(task_id.as_u64())) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Await => {
                let task_id_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let task_id_u64 = match task_id_val.as_u64() {
                    Some(id) => id,
                    None => {
                        return OpcodeResult::Error(VmError::TypeError(
                            "Expected TaskId".to_string(),
                        ));
                    }
                };

                let awaited_id = TaskId::from_u64(task_id_u64);

                // Check if the awaited task is already complete
                let tasks_guard = self.tasks.read();
                if let Some(awaited_task) = tasks_guard.get(&awaited_id).cloned() {
                    drop(tasks_guard);
                    match awaited_task.state() {
                        TaskState::Completed => {
                            // Already done, push result
                            let result = awaited_task.result().unwrap_or(Value::null());
                            if let Err(e) = stack.push(result) {
                                return OpcodeResult::Error(e);
                            }
                            return OpcodeResult::Continue;
                        }
                        TaskState::Failed => {
                            // Propagate exception
                            if let Some(exc) = awaited_task.current_exception() {
                                task.set_exception(exc);
                            }
                            return OpcodeResult::Error(VmError::RuntimeError(format!(
                                "Awaited task {:?} failed",
                                awaited_id
                            )));
                        }
                        _ => {
                            // Not done yet - register as waiter and suspend
                            awaited_task.add_waiter(task.id());
                            return OpcodeResult::Suspend(SuspendReason::AwaitTask(awaited_id));
                        }
                    }
                } else {
                    drop(tasks_guard);
                    return OpcodeResult::Error(VmError::RuntimeError(format!(
                        "Task {:?} not found",
                        awaited_id
                    )));
                }
            }

            Opcode::WaitAll => {
                // WaitAll: await [task1, task2, ...] - wait for all tasks and return results array
                // Note: When resumed after awaiting, run() pushes a resume value.
                // We need to handle this by checking if we got an array or a resume value.
                let top_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Check if this is the array we need, or a resume value from a previous await
                let arr_val = if top_val.is_ptr() {
                    // Could be array or something else
                    if unsafe { top_val.as_ptr::<Array>() }.is_some() {
                        // Looks like an array - verify it contains task IDs
                        top_val
                    } else {
                        // Not an array - this is a resume value, pop the real array
                        match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        }
                    }
                } else {
                    // This is a resume value (probably a number), pop the real array
                    match stack.pop() {
                        Ok(v) => v,
                        Err(e) => return OpcodeResult::Error(e),
                    }
                };

                if !arr_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError(
                        "WaitAll expects an array of tasks".to_string(),
                    ));
                }

                let arr_ptr = match unsafe { arr_val.as_ptr::<Array>() } {
                    Some(p) => p,
                    None => {
                        return OpcodeResult::Error(VmError::TypeError(
                            "Expected array for WaitAll".to_string(),
                        ))
                    }
                };
                let arr = unsafe { &*arr_ptr.as_ptr() };
                let task_count = arr.len();

                // Collect task IDs and check their states
                let mut task_ids = Vec::with_capacity(task_count);
                let mut results = Vec::with_capacity(task_count);
                let mut all_completed = true;
                let mut first_incomplete: Option<TaskId> = None;
                let mut failed_task_info: Option<(TaskId, Option<Value>)> = None;
                let mut missing_task: Option<TaskId> = None;

                {
                    let tasks_guard = self.tasks.read();
                    for i in 0..task_count {
                        let elem = arr.get(i).unwrap_or(Value::null());
                        let task_id_u64 = match elem.as_u64() {
                            Some(id) => id,
                            None => {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "Expected TaskId in array".to_string(),
                                ));
                            }
                        };
                        let awaited_id = TaskId::from_u64(task_id_u64);
                        task_ids.push(awaited_id);

                        if let Some(awaited_task) = tasks_guard.get(&awaited_id) {
                            match awaited_task.state() {
                                TaskState::Completed => {
                                    let result = awaited_task.result().unwrap_or(Value::null());
                                    results.push(result);
                                }
                                TaskState::Failed => {
                                    // Record failure info to handle after releasing lock
                                    let exc = awaited_task.current_exception();
                                    failed_task_info = Some((awaited_id, exc));
                                    break;
                                }
                                _ => {
                                    all_completed = false;
                                    if first_incomplete.is_none() {
                                        first_incomplete = Some(awaited_id);
                                    }
                                    results.push(Value::null()); // placeholder
                                }
                            }
                        } else {
                            missing_task = Some(awaited_id);
                            break;
                        }
                    }
                } // tasks_guard dropped here

                // Handle error cases after releasing the lock
                if let Some(task_id) = missing_task {
                    return OpcodeResult::Error(VmError::RuntimeError(format!(
                        "Task {:?} not found in WaitAll",
                        task_id
                    )));
                }
                if let Some((awaited_id, exc)) = failed_task_info {
                    if let Some(exc_val) = exc {
                        task.set_exception(exc_val);
                    }
                    return OpcodeResult::Error(VmError::RuntimeError(format!(
                        "Awaited task {:?} failed in WaitAll",
                        awaited_id
                    )));
                }

                if all_completed {
                    // All tasks done - create result array
                    let mut result_arr = Array::new(task_count, task_count);
                    for (i, result) in results.into_iter().enumerate() {
                        result_arr.set(i, result);
                    }
                    let gc_ptr = self.gc.lock().allocate(result_arr);
                    let result_val =
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    if let Err(e) = stack.push(result_val) {
                        return OpcodeResult::Error(e);
                    }
                    OpcodeResult::Continue
                } else {
                    // Not all complete - push array back and suspend
                    // When we resume, we'll re-execute WaitAll with the same array
                    if let Err(e) = stack.push(arr_val) {
                        return OpcodeResult::Error(e);
                    }
                    // Decrement ip to re-execute WaitAll when resumed
                    // We modify the local ip so that when run() calls task.set_ip(ip),
                    // it will point back to the WaitAll opcode
                    *ip -= 1;

                    // Register as waiter on first incomplete task
                    if let Some(awaited_id) = first_incomplete {
                        let tasks_guard = self.tasks.read();
                        if let Some(awaited_task) = tasks_guard.get(&awaited_id) {
                            awaited_task.add_waiter(task.id());
                        }
                        drop(tasks_guard);
                        OpcodeResult::Suspend(SuspendReason::AwaitTask(awaited_id))
                    } else {
                        // Shouldn't happen
                        OpcodeResult::Continue
                    }
                }
            }

            Opcode::Sleep => {
                let duration_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let ms = duration_val.as_i64().unwrap_or(0) as u64;
                let wake_at = Instant::now() + std::time::Duration::from_millis(ms);

                // Suspend until wake time - scheduler will wake us up
                OpcodeResult::Suspend(SuspendReason::Sleep { wake_at })
            }

            Opcode::MutexLock => {
                let mutex_id_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let mutex_id = MutexId::from_u64(mutex_id_val.as_i64().unwrap_or(0) as u64);

                // Try to acquire the lock
                if let Some(mutex) = self.mutex_registry.get(mutex_id) {
                    match mutex.try_lock(task.id()) {
                        Ok(()) => {
                            // Acquired immediately
                            task.add_held_mutex(mutex_id);
                            OpcodeResult::Continue
                        }
                        Err(_) => {
                            // Need to wait - suspend
                            OpcodeResult::Suspend(SuspendReason::MutexLock { mutex_id })
                        }
                    }
                } else {
                    OpcodeResult::Error(VmError::RuntimeError(format!(
                        "Mutex {:?} not found",
                        mutex_id
                    )))
                }
            }

            Opcode::MutexUnlock => {
                let mutex_id_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let mutex_id = MutexId::from_u64(mutex_id_val.as_i64().unwrap_or(0) as u64);

                if let Some(mutex) = self.mutex_registry.get(mutex_id) {
                    match mutex.unlock(task.id()) {
                        Ok(next_waiter) => {
                            task.remove_held_mutex(mutex_id);

                            // If there's a waiting task, wake it up
                            if let Some(waiter_id) = next_waiter {
                                let tasks = self.tasks.read();
                                if let Some(waiter_task) = tasks.get(&waiter_id) {
                                    // The mutex is now owned by the waiter (set by mutex.unlock)
                                    waiter_task.add_held_mutex(mutex_id);
                                    waiter_task.set_state(TaskState::Resumed);
                                    waiter_task.clear_suspend_reason();
                                    self.injector.push(waiter_task.clone());
                                }
                            }
                            OpcodeResult::Continue
                        }
                        Err(e) => OpcodeResult::Error(VmError::RuntimeError(format!("{}", e))),
                    }
                } else {
                    OpcodeResult::Error(VmError::RuntimeError(format!(
                        "Mutex {:?} not found",
                        mutex_id
                    )))
                }
            }

            Opcode::Yield => {
                // Voluntary yield - suspend with immediate wake
                OpcodeResult::Suspend(SuspendReason::Sleep {
                    wake_at: Instant::now(),
                })
            }

            Opcode::TaskCancel => {
                // Pop task ID from stack
                let task_id_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let task_id_u64 = match task_id_val.as_u64() {
                    Some(id) => id,
                    None => {
                        return OpcodeResult::Error(VmError::TypeError(
                            "TaskCancel: expected task handle (u64)".to_string(),
                        ));
                    }
                };

                let target_id = TaskId::from_u64(task_id_u64);

                // Look up the task and cancel it
                if let Some(target_task) = self.tasks.read().get(&target_id).cloned() {
                    target_task.cancel();
                }
                // Silently ignore if task not found (may have already completed)

                OpcodeResult::Continue
            }

            // =========================================================
            // Native Calls and Builtins
            // =========================================================
            Opcode::NativeCall => {
                use crate::vm::builtin::{buffer, map, set, date, regexp};

                let native_id = match Self::read_u16(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u8(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };


                // Pop arguments
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    match stack.pop() {
                        Ok(v) => args.push(v),
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }
                args.reverse();

                // Execute native call - handle channel operations specially for suspension
                match native_id {
                    CHANNEL_SEND => {
                        // args: [channel, value]
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_SEND requires 2 arguments".to_string()
                            ));
                        }
                        let channel_val = args[0];
                        let value = args[1];

                        if !channel_val.is_ptr() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }

                        let channel_ptr = unsafe { channel_val.as_ptr::<ChannelObject>() };
                        if channel_ptr.is_none() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }
                        let channel = unsafe { &*channel_ptr.unwrap().as_ptr() };
                        let task_id_u64 = task.id().as_u64();

                        match channel.send_or_suspend(value, task_id_u64) {
                            Ok(None) => {
                                // Send succeeded, push null result
                                if let Err(e) = stack.push(Value::null()) {
                                    return OpcodeResult::Error(e);
                                }
                                OpcodeResult::Continue
                            }
                            Ok(Some(wake_id)) if wake_id != task_id_u64 => {
                                // Send succeeded and we need to wake a receiver
                                // Push null result first
                                if let Err(e) = stack.push(Value::null()) {
                                    return OpcodeResult::Error(e);
                                }
                                // TODO: Wake the receiver task
                                OpcodeResult::Continue
                            }
                            Ok(Some(_)) => {
                                // Channel is full, need to suspend
                                let channel_id = channel_ptr.unwrap().as_ptr() as u64;
                                return OpcodeResult::Suspend(SuspendReason::ChannelSend {
                                    channel_id,
                                    value,
                                });
                            }
                            Err(_) => {
                                return OpcodeResult::Error(VmError::RuntimeError(
                                    "Channel closed".to_string()
                                ));
                            }
                        }
                    }

                    CHANNEL_RECEIVE => {
                        // args: [channel]
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_RECEIVE requires 1 argument".to_string()
                            ));
                        }
                        let channel_val = args[0];

                        if !channel_val.is_ptr() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }

                        let channel_ptr = unsafe { channel_val.as_ptr::<ChannelObject>() };
                        if channel_ptr.is_none() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }
                        let channel = unsafe { &*channel_ptr.unwrap().as_ptr() };
                        let task_id_u64 = task.id().as_u64();

                        match channel.receive_or_suspend(task_id_u64) {
                            Ok(Some(value)) => {
                                // Receive succeeded
                                if let Err(e) = stack.push(value) {
                                    return OpcodeResult::Error(e);
                                }
                                OpcodeResult::Continue
                            }
                            Ok(None) => {
                                // Channel is empty, need to suspend
                                let channel_id = channel_ptr.unwrap().as_ptr() as u64;
                                return OpcodeResult::Suspend(SuspendReason::ChannelReceive {
                                    channel_id,
                                });
                            }
                            Err(_) => {
                                return OpcodeResult::Error(VmError::RuntimeError(
                                    "Channel closed".to_string()
                                ));
                            }
                        }
                    }

                    CHANNEL_TRY_SEND => {
                        // Non-blocking send - returns boolean
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_TRY_SEND requires 2 arguments".to_string()
                            ));
                        }
                        let channel_val = args[0];
                        let value = args[1];

                        if !channel_val.is_ptr() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }

                        let channel_ptr = unsafe { channel_val.as_ptr::<ChannelObject>() };
                        if channel_ptr.is_none() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }
                        let channel = unsafe { &*channel_ptr.unwrap().as_ptr() };
                        let result = channel.try_send(value);
                        if let Err(e) = stack.push(Value::bool(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_TRY_RECEIVE => {
                        // Non-blocking receive - returns value or null
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_TRY_RECEIVE requires 1 argument".to_string()
                            ));
                        }
                        let channel_val = args[0];

                        if !channel_val.is_ptr() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }

                        let channel_ptr = unsafe { channel_val.as_ptr::<ChannelObject>() };
                        if channel_ptr.is_none() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }
                        let channel = unsafe { &*channel_ptr.unwrap().as_ptr() };
                        let result = channel.try_receive().unwrap_or(Value::null());
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_CLOSE => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_CLOSE requires 1 argument".to_string()
                            ));
                        }
                        let channel_val = args[0];

                        if !channel_val.is_ptr() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }

                        let channel_ptr = unsafe { channel_val.as_ptr::<ChannelObject>() };
                        if channel_ptr.is_none() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }
                        let channel = unsafe { &*channel_ptr.unwrap().as_ptr() };
                        channel.close();
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_IS_CLOSED => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_IS_CLOSED requires 1 argument".to_string()
                            ));
                        }
                        let channel_val = args[0];

                        if !channel_val.is_ptr() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }

                        let channel_ptr = unsafe { channel_val.as_ptr::<ChannelObject>() };
                        if channel_ptr.is_none() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }
                        let channel = unsafe { &*channel_ptr.unwrap().as_ptr() };
                        let result = channel.is_closed();
                        if let Err(e) = stack.push(Value::bool(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_LENGTH => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_LENGTH requires 1 argument".to_string()
                            ));
                        }
                        let channel_val = args[0];

                        if !channel_val.is_ptr() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }

                        let channel_ptr = unsafe { channel_val.as_ptr::<ChannelObject>() };
                        if channel_ptr.is_none() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }
                        let channel = unsafe { &*channel_ptr.unwrap().as_ptr() };
                        let result = channel.length() as i32;
                        if let Err(e) = stack.push(Value::i32(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_CAPACITY => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_CAPACITY requires 1 argument".to_string()
                            ));
                        }
                        let channel_val = args[0];

                        if !channel_val.is_ptr() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }

                        let channel_ptr = unsafe { channel_val.as_ptr::<ChannelObject>() };
                        if channel_ptr.is_none() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }
                        let channel = unsafe { &*channel_ptr.unwrap().as_ptr() };
                        let result = channel.capacity() as i32;
                        if let Err(e) = stack.push(Value::i32(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    // Buffer native calls
                    id if id == buffer::NEW as u16 => {
                        let size = args[0].as_i32().unwrap_or(0) as usize;
                        let buf = Buffer::new(size);
                        let gc_ptr = self.gc.lock().allocate(buf);
                        let handle = gc_ptr.as_ptr() as u64;
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::LENGTH as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid buffer handle".to_string()));
                        }
                        let buf = unsafe { &*buf_ptr };
                        if let Err(e) = stack.push(Value::i32(buf.length() as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::GET_BYTE as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let index = args[1].as_i32().unwrap_or(0) as usize;
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid buffer handle".to_string()));
                        }
                        let buf = unsafe { &*buf_ptr };
                        let value = buf.get_byte(index).unwrap_or(0);
                        if let Err(e) = stack.push(Value::i32(value as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::SET_BYTE as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let index = args[1].as_i32().unwrap_or(0) as usize;
                        let value = args[2].as_i32().unwrap_or(0) as u8;
                        let buf_ptr = handle as *mut Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid buffer handle".to_string()));
                        }
                        let buf = unsafe { &mut *buf_ptr };
                        if let Err(msg) = buf.set_byte(index, value) {
                            return OpcodeResult::Error(VmError::RuntimeError(msg));
                        }
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Map native calls
                    id if id == map::NEW as u16 => {
                        let map = MapObject::new();
                        let gc_ptr = self.gc.lock().allocate(map);
                        let handle = gc_ptr.as_ptr() as u64;
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::SIZE as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid map handle".to_string()));
                        }
                        let map = unsafe { &*map_ptr };
                        if let Err(e) = stack.push(Value::i32(map.size() as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::GET as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let key = args[1];
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid map handle".to_string()));
                        }
                        let map = unsafe { &*map_ptr };
                        let value = map.get(key).unwrap_or(Value::null());
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::SET as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let key = args[1];
                        let value = args[2];
                        let map_ptr = handle as *mut MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid map handle".to_string()));
                        }
                        let map = unsafe { &mut *map_ptr };
                        map.set(key, value);
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::HAS as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let key = args[1];
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid map handle".to_string()));
                        }
                        let map = unsafe { &*map_ptr };
                        if let Err(e) = stack.push(Value::bool(map.has(key))) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::DELETE as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let key = args[1];
                        let map_ptr = handle as *mut MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid map handle".to_string()));
                        }
                        let map = unsafe { &mut *map_ptr };
                        let result = map.delete(key);
                        if let Err(e) = stack.push(Value::bool(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::CLEAR as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let map_ptr = handle as *mut MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid map handle".to_string()));
                        }
                        let map = unsafe { &mut *map_ptr };
                        map.clear();
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Set native calls
                    id if id == set::NEW as u16 => {
                        let set_obj = SetObject::new();
                        let gc_ptr = self.gc.lock().allocate(set_obj);
                        let handle = gc_ptr.as_ptr() as u64;
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::SIZE as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let set_ptr = handle as *const SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid set handle".to_string()));
                        }
                        let set_obj = unsafe { &*set_ptr };
                        if let Err(e) = stack.push(Value::i32(set_obj.size() as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::ADD as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let value = args[1];
                        let set_ptr = handle as *mut SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid set handle".to_string()));
                        }
                        let set_obj = unsafe { &mut *set_ptr };
                        set_obj.add(value);
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::HAS as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let value = args[1];
                        let set_ptr = handle as *const SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid set handle".to_string()));
                        }
                        let set_obj = unsafe { &*set_ptr };
                        if let Err(e) = stack.push(Value::bool(set_obj.has(value))) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::DELETE as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let value = args[1];
                        let set_ptr = handle as *mut SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid set handle".to_string()));
                        }
                        let set_obj = unsafe { &mut *set_ptr };
                        let result = set_obj.delete(value);
                        if let Err(e) = stack.push(Value::bool(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::CLEAR as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let set_ptr = handle as *mut SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid set handle".to_string()));
                        }
                        let set_obj = unsafe { &mut *set_ptr };
                        set_obj.clear();
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Date native calls
                    id if id == date::NOW as u16 => {
                        use std::time::{SystemTime, UNIX_EPOCH};
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_millis() as f64)
                            .unwrap_or(0.0);
                        if let Err(e) = stack.push(Value::f64(now)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_FULL_YEAR as u16 => {
                        // args[0] is the timestamp in milliseconds (as f64 number)
                        let timestamp = args[0].as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_full_year())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_MONTH as u16 => {
                        let timestamp = args[0].as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_month())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_DATE as u16 => {
                        let timestamp = args[0].as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_date())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_DAY as u16 => {
                        let timestamp = args[0].as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_day())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // RegExp native calls
                    id if id == regexp::NEW as u16 => {
                        let pattern = if args[0].is_ptr() {
                            if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let flags = if args.len() > 1 && args[1].is_ptr() {
                            if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        match RegExpObject::new(&pattern, &flags) {
                            Ok(re) => {
                                let gc_ptr = self.gc.lock().allocate(re);
                                let handle = gc_ptr.as_ptr() as u64;
                                if let Err(e) = stack.push(Value::u64(handle)) {
                                    return OpcodeResult::Error(e);
                                }
                                OpcodeResult::Continue
                            }
                            Err(e) => {
                                return OpcodeResult::Error(VmError::RuntimeError(format!("Invalid regex: {}", e)));
                            }
                        }
                    }
                    id if id == regexp::TEST as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let input = if args[1].is_ptr() {
                            if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid regexp handle".to_string()));
                        }
                        let re = unsafe { &*re_ptr };
                        if let Err(e) = stack.push(Value::bool(re.test(&input))) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::EXEC as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let input = if args[1].is_ptr() {
                            if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid regexp handle".to_string()));
                        }
                        let re = unsafe { &*re_ptr };
                        match re.exec(&input) {
                            Some((matched, index, groups)) => {
                                let mut arr = Array::new(0, 0);
                                let matched_str = RayaString::new(matched);
                                let gc_ptr = self.gc.lock().allocate(matched_str);
                                let matched_val = unsafe {
                                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                                };
                                arr.push(matched_val);
                                arr.push(Value::i32(index as i32));
                                for group in groups {
                                    let group_str = RayaString::new(group);
                                    let gc_ptr = self.gc.lock().allocate(group_str);
                                    let group_val = unsafe {
                                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                                    };
                                    arr.push(group_val);
                                }
                                let arr_gc = self.gc.lock().allocate(arr);
                                let arr_val = unsafe {
                                    Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                                };
                                if let Err(e) = stack.push(arr_val) {
                                    return OpcodeResult::Error(e);
                                }
                            }
                            None => {
                                if let Err(e) = stack.push(Value::null()) {
                                    return OpcodeResult::Error(e);
                                }
                            }
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::EXEC_ALL as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let input = if args[1].is_ptr() {
                            if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid regexp handle".to_string()));
                        }
                        let re = unsafe { &*re_ptr };
                        let matches = re.exec_all(&input);
                        let mut result_arr = Array::new(0, 0);
                        for (matched, index, groups) in matches {
                            let mut match_arr = Array::new(0, 0);
                            let matched_str = RayaString::new(matched);
                            let gc_ptr = self.gc.lock().allocate(matched_str);
                            let matched_val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            };
                            match_arr.push(matched_val);
                            match_arr.push(Value::i32(index as i32));
                            for group in groups {
                                let group_str = RayaString::new(group);
                                let gc_ptr = self.gc.lock().allocate(group_str);
                                let group_val = unsafe {
                                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                                };
                                match_arr.push(group_val);
                            }
                            let match_arr_gc = self.gc.lock().allocate(match_arr);
                            let match_arr_val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(match_arr_gc.as_ptr()).unwrap())
                            };
                            result_arr.push(match_arr_val);
                        }
                        let arr_gc = self.gc.lock().allocate(result_arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::REPLACE as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let input = if args[1].is_ptr() {
                            if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let replacement = if args[2].is_ptr() {
                            if let Some(s) = unsafe { args[2].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid regexp handle".to_string()));
                        }
                        let re = unsafe { &*re_ptr };
                        let result = re.replace(&input, &replacement);
                        let result_str = RayaString::new(result);
                        let gc_ptr = self.gc.lock().allocate(result_str);
                        let result_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(result_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::SPLIT as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let input = if args[1].is_ptr() {
                            if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let limit = if args.len() > 2 {
                            args[2].as_i32().map(|v| v as usize)
                        } else {
                            None
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid regexp handle".to_string()));
                        }
                        let re = unsafe { &*re_ptr };
                        let parts = re.split(&input, limit);
                        let mut arr = Array::new(0, 0);
                        for part in parts {
                            let s = RayaString::new(part);
                            let gc_ptr = self.gc.lock().allocate(s);
                            let val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            };
                            arr.push(val);
                        }
                        let arr_gc = self.gc.lock().allocate(arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // JSON.stringify
                    0x0C00 => {
                        use crate::vm::json;

                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "JSON.stringify requires 1 argument".to_string()
                            ));
                        }
                        let value = args[0];

                        // Convert Value to JsonValue
                        let json_value = json::value_to_json(value, &mut self.gc.lock());

                        // Stringify the JsonValue
                        match json::stringify::stringify(&json_value) {
                            Ok(json_str) => {
                                let result_str = RayaString::new(json_str);
                                let gc_ptr = self.gc.lock().allocate(result_str);
                                let result_val = unsafe {
                                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                                };
                                if let Err(e) = stack.push(result_val) {
                                    return OpcodeResult::Error(e);
                                }
                            }
                            Err(e) => {
                                return OpcodeResult::Error(e);
                            }
                        }
                        OpcodeResult::Continue
                    }

                    // JSON.parse
                    0x0C01 => {
                        use crate::vm::json;

                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "JSON.parse requires 1 argument".to_string()
                            ));
                        }
                        let json_str = if args[0].is_ptr() {
                            if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "JSON.parse requires a string argument".to_string()
                                ));
                            }
                        } else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "JSON.parse requires a string argument".to_string()
                            ));
                        };

                        // Parse the JSON string (lock scope ends before json_to_value)
                        let json_value = {
                            let mut gc = self.gc.lock();
                            match json::parser::parse(&json_str, &mut gc) {
                                Ok(v) => v,
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        };

                        // Convert JsonValue to Value (separate lock scope)
                        let result = json::json_to_value(&json_value, &mut self.gc.lock());
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    // JSON.decode<T> - typed decode with field metadata
                    // Args: [json_string, field_count, ...field_keys]
                    0x0C02 => {
                        use crate::vm::json;
                        use crate::vm::object::Object;

                        if args.len() < 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "JSON.decode requires at least 2 arguments".to_string()
                            ));
                        }

                        // Get JSON string
                        let json_str = if args[0].is_ptr() {
                            if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "JSON.decode requires a string argument".to_string()
                                ));
                            }
                        } else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "JSON.decode requires a string argument".to_string()
                            ));
                        };

                        // Get field count
                        let field_count = if let Some(n) = args[1].as_i32() {
                            n as usize
                        } else if let Some(n) = args[1].as_f64() {
                            n as usize
                        } else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "JSON.decode field count must be a number".to_string()
                            ));
                        };

                        // Collect field keys
                        let mut field_keys: Vec<String> = Vec::with_capacity(field_count);
                        for i in 0..field_count {
                            if args.len() <= 2 + i {
                                break;
                            }
                            if args[2 + i].is_ptr() {
                                if let Some(s) = unsafe { args[2 + i].as_ptr::<RayaString>() } {
                                    field_keys.push(unsafe { &*s.as_ptr() }.data.clone());
                                }
                            }
                        }

                        // Parse the JSON string
                        let json_value = {
                            let mut gc = self.gc.lock();
                            match json::parser::parse(&json_str, &mut gc) {
                                Ok(v) => v,
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        };

                        // Create a new object with the specified fields
                        let mut gc = self.gc.lock();
                        let mut obj = Object::new(0, field_keys.len()); // class_id 0 for anonymous

                        // Extract each field from the JSON and store in object
                        for (index, key) in field_keys.iter().enumerate() {
                            let field_value = json_value.get_property(key);
                            let vm_value = json::json_to_value(&field_value, &mut gc);
                            obj.set_field(index, vm_value);
                        }

                        // Allocate and return the object
                        let obj_ptr = gc.allocate(obj);
                        let result = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap())
                        };
                        drop(gc); // Release lock before push

                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    _ => {
                        // Other native calls not yet implemented
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "NativeCall {:#06x} not yet implemented in TaskInterpreter (args={})",
                            native_id, args.len()
                        )));
                    }
                }
            }

            Opcode::InstanceOf => {
                let class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let result = if obj_val.is_ptr() {
                    if let Some(obj_ptr) = unsafe { obj_val.as_ptr::<Object>() } {
                        let obj = unsafe { &*obj_ptr.as_ptr() };
                        let classes = self.classes.read();
                        let mut current_class_id = Some(obj.class_id);
                        let mut matches = false;
                        while let Some(cid) = current_class_id {
                            if cid == class_index {
                                matches = true;
                                break;
                            }
                            if let Some(class) = classes.get_class(cid) {
                                current_class_id = class.parent_id;
                            } else {
                                break;
                            }
                        }
                        matches
                    } else {
                        false
                    }
                } else {
                    false
                };

                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Cast => {
                let class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Null check - null can be cast to any type (it represents absence of value)
                if obj_val.is_null() {
                    if let Err(e) = stack.push(obj_val) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }

                // Check if object is an instance of the target class
                let valid_cast = if obj_val.is_ptr() {
                    if let Some(obj_ptr) = unsafe { obj_val.as_ptr::<Object>() } {
                        let obj = unsafe { &*obj_ptr.as_ptr() };
                        let classes = self.classes.read();
                        let mut current_class_id = Some(obj.class_id);
                        let mut matches = false;
                        while let Some(cid) = current_class_id {
                            if cid == class_index {
                                matches = true;
                                break;
                            }
                            if let Some(class) = classes.get_class(cid) {
                                current_class_id = class.parent_id;
                            } else {
                                break;
                            }
                        }
                        matches
                    } else {
                        false
                    }
                } else {
                    false
                };

                if valid_cast {
                    // Cast is valid, push object back
                    if let Err(e) = stack.push(obj_val) {
                        return OpcodeResult::Error(e);
                    }
                    OpcodeResult::Continue
                } else {
                    // Cast failed - throw TypeError
                    OpcodeResult::Error(VmError::TypeError(format!(
                        "Cannot cast object to class index {}",
                        class_index
                    )))
                }
            }

            // =========================================================
            // JSON Operations (Duck Typing)
            // =========================================================
            Opcode::JsonGet => {
                use crate::vm::json::{self, JsonValue};

                // Read property name index from constant pool
                let prop_index = match Self::read_u32(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Get property name from constant pool
                let prop_name = match module.constants.get_string(prop_index) {
                    Some(s) => s.to_string(),
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid constant index {} for JSON property",
                            prop_index
                        )));
                    }
                };

                // Pop the JSON object from stack
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Handle different value types
                let result = if obj_val.is_null() {
                    // Accessing property on null returns null
                    Value::null()
                } else if obj_val.is_ptr() {
                    // Try to access as JsonValue (stored on heap by json_to_value)
                    let ptr = unsafe { obj_val.as_ptr::<JsonValue>() };
                    if let Some(json_ptr) = ptr {
                        let json_val = unsafe { &*json_ptr.as_ptr() };
                        // Use JsonValue's get_property method for duck typing
                        let prop_val = json_val.get_property(&prop_name);
                        // Convert the result to a Value
                        json::json_to_value(&prop_val, &mut self.gc.lock())
                    } else {
                        return OpcodeResult::Error(VmError::TypeError(
                            "Expected JSON object for property access".to_string(),
                        ));
                    }
                } else {
                    // Primitive types don't support property access
                    Value::null()
                };

                if let Err(e) = stack.push(result) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::JsonSet => {
                use crate::vm::json::{self, JsonValue};

                // Read property name index from constant pool
                let prop_index = match Self::read_u32(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Get property name from constant pool
                let prop_name = match module.constants.get_string(prop_index) {
                    Some(s) => s.to_string(),
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid constant index {} for JSON property",
                            prop_index
                        )));
                    }
                };

                // Pop value and object from stack
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !obj_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected JSON object for property assignment".to_string(),
                    ));
                }

                // Try to access as JsonValue and set property
                let ptr = unsafe { obj_val.as_ptr::<JsonValue>() };
                if let Some(json_ptr) = ptr {
                    let json_val = unsafe { &*json_ptr.as_ptr() };
                    // Get the inner HashMap from the JsonValue::Object
                    if let Some(obj_ptr) = json_val.as_object() {
                        let map = unsafe { &mut *obj_ptr.as_ptr() };
                        // Convert Value to JsonValue
                        let new_json_val = json::value_to_json(value, &mut self.gc.lock());
                        map.insert(prop_name, new_json_val);
                    } else {
                        return OpcodeResult::Error(VmError::TypeError(
                            "Expected JSON object for property assignment".to_string(),
                        ));
                    }
                } else {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected JSON object for property assignment".to_string(),
                    ));
                }

                OpcodeResult::Continue
            }

            // =========================================================
            // Method Calls
            // =========================================================
            Opcode::CallMethod => {
                self.safepoint.poll();
                let method_index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let method_id = method_index as u16;

                // Check for built-in array methods
                if crate::vm::builtin::is_array_method(method_id) {
                    match self.call_array_method(task, stack, method_id, arg_count, module) {
                        Ok(()) => return OpcodeResult::Continue,
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }

                // Check for built-in string methods
                if crate::vm::builtin::is_string_method(method_id) {
                    match self.call_string_method(task, stack, method_id, arg_count, module) {
                        Ok(()) => return OpcodeResult::Continue,
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }

                // Check for built-in regexp methods
                if crate::vm::builtin::is_regexp_method(method_id) {
                    match self.call_regexp_method(task, stack, method_id, arg_count) {
                        Ok(()) => return OpcodeResult::Continue,
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }

                // Fall through to vtable dispatch for user-defined methods
                let receiver_pos = match stack.depth().checked_sub(arg_count + 1) {
                    Some(pos) => pos,
                    None => return OpcodeResult::Error(VmError::StackUnderflow),
                };

                let receiver_val = match stack.peek_at(receiver_pos) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !receiver_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected object for method call".to_string(),
                    ));
                }

                let obj_ptr = unsafe { receiver_val.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };

                let classes = self.classes.read();
                let class = match classes.get_class(obj.class_id) {
                    Some(c) => c,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid class ID: {}",
                            obj.class_id
                        )));
                    }
                };

                let function_id = match class.vtable.get_method(method_index) {
                    Some(id) => id,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Method index {} not found in vtable",
                            method_index
                        )));
                    }
                };
                drop(classes);

                // Execute the method
                match self.call_function(task, stack, function_id, arg_count + 1, module, locals_base) {
                    Ok(result) => {
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    Err(e) => return OpcodeResult::Error(e),
                }
                OpcodeResult::Continue
            }

            Opcode::CallConstructor => {
                self.safepoint.poll();
                let class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u8(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Pop arguments (they're pushed in reverse order)
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    match stack.pop() {
                        Ok(v) => args.push(v),
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }
                args.reverse();

                // Look up class and create object
                let classes = self.classes.read();
                let class = match classes.get_class(class_index) {
                    Some(c) => c,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid class index: {}",
                            class_index
                        )));
                    }
                };
                let field_count = class.field_count;
                let constructor_id = class.get_constructor();
                drop(classes);

                // Create the object
                let obj = Object::new(class_index, field_count);
                let gc_ptr = self.gc.lock().allocate(obj);
                let obj_val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };

                // If no constructor, just return the object
                let constructor_id = match constructor_id {
                    Some(id) => id,
                    None => {
                        if let Err(e) = stack.push(obj_val) {
                            return OpcodeResult::Error(e);
                        }
                        return OpcodeResult::Continue;
                    }
                };

                // Push object (as receiver) and args for constructor call
                if let Err(e) = stack.push(obj_val) {
                    return OpcodeResult::Error(e);
                }
                for arg in args {
                    if let Err(e) = stack.push(arg) {
                        return OpcodeResult::Error(e);
                    }
                }

                // Execute constructor
                match self.call_function(task, stack, constructor_id, arg_count + 1, module, locals_base) {
                    Ok(_) => {
                        // Constructor doesn't return a value, push the object
                        if let Err(e) = stack.push(obj_val) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    Err(e) => return OpcodeResult::Error(e),
                }
                OpcodeResult::Continue
            }

            Opcode::CallSuper => {
                let class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u8(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let classes = self.classes.read();
                let class = match classes.get_class(class_index) {
                    Some(c) => c,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid class index: {}",
                            class_index
                        )));
                    }
                };

                let parent_id = match class.parent_id {
                    Some(id) => id,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(
                            "Class has no parent".to_string(),
                        ));
                    }
                };

                let parent_class = match classes.get_class(parent_id) {
                    Some(c) => c,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid parent class ID: {}",
                            parent_id
                        )));
                    }
                };

                let constructor_id = match parent_class.get_constructor() {
                    Some(id) => id,
                    None => {
                        drop(classes);
                        return OpcodeResult::Continue;
                    }
                };
                drop(classes);

                match self.call_function(task, stack, constructor_id, arg_count + 1, module, locals_base) {
                    Ok(_) => {}
                    Err(e) => return OpcodeResult::Error(e),
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Mutex Creation
            // =========================================================
            Opcode::NewMutex => {
                let (mutex_id, _) = self.mutex_registry.create_mutex();
                if let Err(e) = stack.push(Value::i64(mutex_id.as_u64() as i64)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Channel Creation
            // =========================================================
            Opcode::NewChannel => {
                self.safepoint.poll();
                let capacity_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let capacity = capacity_val.as_i32().unwrap_or(0) as usize;
                let channel = ChannelObject::new(capacity);
                let gc_ptr = self.gc.lock().allocate(channel);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Static Fields
            // =========================================================
            Opcode::LoadStatic => {
                let class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let field_offset = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Get static field from the class registry
                let classes = self.classes.read();
                let class = match classes.get_class(class_index) {
                    Some(c) => c,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid class index: {}",
                            class_index
                        )));
                    }
                };
                let value = class.get_static_field(field_offset).unwrap_or(Value::null());
                drop(classes);

                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreStatic => {
                let class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let field_offset = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Set static field in the class registry
                let mut classes = self.classes.write();
                let class = match classes.get_class_mut(class_index) {
                    Some(c) => c,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid class index: {}",
                            class_index
                        )));
                    }
                };
                if let Err(e) = class.set_static_field(field_offset, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Type Operators
            // =========================================================
            Opcode::Typeof => {
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let type_str = if value.is_null() {
                    "null"
                } else if value.is_bool() {
                    "boolean"
                } else if value.is_i32() || value.is_i64() || value.is_f64() {
                    "number"
                } else if value.is_ptr() {
                    // Check if it's a string
                    if let Some(ptr) = unsafe { value.as_ptr::<RayaString>() } {
                        let _ = ptr; // Validate it's a string
                        "string"
                    } else {
                        "object"
                    }
                } else {
                    "undefined"
                };

                let raya_string = RayaString::new(type_str.to_string());
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let str_value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(str_value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Catch-all for unimplemented opcodes
            // =========================================================
            _ => OpcodeResult::Error(VmError::RuntimeError(format!(
                "Opcode {:?} not yet implemented in TaskInterpreter",
                opcode
            ))),
        }
    }

    /// Call a closure
    fn call_closure(
        &mut self,
        task: &Arc<Task>,
        stack: &mut std::sync::MutexGuard<Stack>,
        arg_count: usize,
        module: &Module,
        locals_base: usize,
    ) -> Result<Value, VmError> {
        // Pop arguments
        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(stack.pop()?);
        }
        args.reverse();

        // Pop closure
        let closure_val = stack.pop()?;
        if !closure_val.is_ptr() {
            return Err(VmError::TypeError("Expected closure".to_string()));
        }

        let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
        let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
        let func_index = closure.func_id();

        // Push closure onto closure stack
        task.push_closure(closure_val);

        // Execute the closure's function
        let result = self.execute_nested_function(task, func_index, args, module)?;

        // Pop closure from closure stack
        task.pop_closure();

        Ok(result)
    }

    /// Call a regular function
    fn call_function(
        &mut self,
        task: &Arc<Task>,
        stack: &mut std::sync::MutexGuard<Stack>,
        func_index: usize,
        arg_count: usize,
        module: &Module,
        _locals_base: usize,
    ) -> Result<Value, VmError> {
        // Pop arguments
        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(stack.pop()?);
        }
        args.reverse();

        // Execute the function
        self.execute_nested_function(task, func_index, args, module)
    }

    /// Execute a nested function call
    fn execute_nested_function(
        &mut self,
        task: &Arc<Task>,
        func_index: usize,
        args: Vec<Value>,
        module: &Module,
    ) -> Result<Value, VmError> {
        if func_index >= module.functions.len() {
            return Err(VmError::RuntimeError(format!(
                "Invalid function index: {}",
                func_index
            )));
        }

        // Push call frame for stack traces
        task.push_call_frame(func_index);

        let function = &module.functions[func_index];
        let code = &function.code;

        // Create a new stack for this call
        let mut call_stack = Stack::new();

        // Allocate locals
        for _ in 0..function.local_count {
            call_stack.push(Value::null())?;
        }

        // Set arguments
        for (i, arg) in args.into_iter().enumerate() {
            if i < function.local_count as usize {
                call_stack.set_at(i, arg)?;
            }
        }

        let locals_base = 0;
        let mut ip = 0;

        loop {
            self.safepoint.poll();

            if ip >= code.len() {
                return Ok(Value::null());
            }

            let opcode_byte = code[ip];
            ip += 1;

            let opcode = Opcode::from_u8(opcode_byte).ok_or(VmError::InvalidOpcode(opcode_byte))?;

            // Handle opcodes for nested calls (simplified version)
            match opcode {
                Opcode::Return => {
                    task.pop_call_frame();
                    return Ok(if call_stack.depth() > 0 {
                        call_stack.pop()?
                    } else {
                        Value::null()
                    });
                }
                Opcode::ReturnVoid => {
                    task.pop_call_frame();
                    return Ok(Value::null());
                }
                Opcode::ConstNull => call_stack.push(Value::null())?,
                Opcode::ConstTrue => call_stack.push(Value::bool(true))?,
                Opcode::ConstFalse => call_stack.push(Value::bool(false))?,
                Opcode::ConstI32 => {
                    let value = Self::read_i32(code, &mut ip)?;
                    call_stack.push(Value::i32(value))?;
                }
                Opcode::ConstF64 => {
                    let value = Self::read_f64(code, &mut ip)?;
                    call_stack.push(Value::f64(value))?;
                }
                Opcode::ConstStr => {
                    let index = Self::read_u16(code, &mut ip)? as usize;
                    let s = module.constants.strings.get(index).ok_or_else(|| {
                        VmError::RuntimeError(format!("Invalid string constant index: {}", index))
                    })?;
                    let raya_string = RayaString::new(s.clone());
                    let gc_ptr = self.gc.lock().allocate(raya_string);
                    let value = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                    };
                    call_stack.push(value)?;
                }
                Opcode::LoadLocal => {
                    let index = Self::read_u16(code, &mut ip)? as usize;
                    let value = call_stack.peek_at(locals_base + index)?;
                    call_stack.push(value)?;
                }
                Opcode::StoreLocal => {
                    let index = Self::read_u16(code, &mut ip)? as usize;
                    let value = call_stack.pop()?;
                    call_stack.set_at(locals_base + index, value)?;
                }
                Opcode::LoadLocal0 => {
                    let value = call_stack.peek_at(locals_base)?;
                    call_stack.push(value)?;
                }
                Opcode::LoadLocal1 => {
                    let value = call_stack.peek_at(locals_base + 1)?;
                    call_stack.push(value)?;
                }
                Opcode::StoreLocal0 => {
                    let value = call_stack.pop()?;
                    call_stack.set_at(locals_base, value)?;
                }
                Opcode::StoreLocal1 => {
                    let value = call_stack.pop()?;
                    call_stack.set_at(locals_base + 1, value)?;
                }
                Opcode::Iadd => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0);
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::i32(a.wrapping_add(b)))?;
                }
                Opcode::Isub => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0);
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::i32(a.wrapping_sub(b)))?;
                }
                Opcode::Imul => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0);
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::i32(a.wrapping_mul(b)))?;
                }
                Opcode::Idiv => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(1);
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    if b == 0 {
                        return Err(VmError::RuntimeError("Division by zero".to_string()));
                    }
                    call_stack.push(Value::i32(a.wrapping_div(b)))?;
                }
                Opcode::Imod => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(1);
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    if b == 0 {
                        return Err(VmError::RuntimeError("Modulo by zero".to_string()));
                    }
                    call_stack.push(Value::i32(a.wrapping_rem(b)))?;
                }
                Opcode::Ineg => {
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::i32(a.wrapping_neg()))?;
                }
                Opcode::Iand => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0);
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::i32(a & b))?;
                }
                Opcode::Ior => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0);
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::i32(a | b))?;
                }
                Opcode::Ixor => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0);
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::i32(a ^ b))?;
                }
                Opcode::Inot => {
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::i32(!a))?;
                }
                Opcode::Ishl => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0) as u32;
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::i32(a.wrapping_shl(b)))?;
                }
                Opcode::Ishr => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0) as u32;
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::i32(a.wrapping_shr(b)))?;
                }
                Opcode::Iushr => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0) as u32;
                    let a = call_stack.pop()?.as_i32().unwrap_or(0) as u32;
                    call_stack.push(Value::i32((a >> b) as i32))?;
                }
                Opcode::Ile => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0);
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::bool(a <= b))?;
                }
                Opcode::Ilt => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0);
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::bool(a < b))?;
                }
                Opcode::Igt => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0);
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::bool(a > b))?;
                }
                Opcode::Ige => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0);
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::bool(a >= b))?;
                }
                Opcode::Ieq => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0);
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::bool(a == b))?;
                }
                Opcode::Ine => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0);
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::bool(a != b))?;
                }
                Opcode::Fadd => {
                    let b_val = call_stack.pop()?;
                    let a_val = call_stack.pop()?;
                    let b = b_val.as_f64().or_else(|| b_val.as_i32().map(|i| i as f64)).unwrap_or(0.0);
                    let a = a_val.as_f64().or_else(|| a_val.as_i32().map(|i| i as f64)).unwrap_or(0.0);
                    call_stack.push(Value::f64(a + b))?;
                }
                Opcode::Fsub => {
                    let b_val = call_stack.pop()?;
                    let a_val = call_stack.pop()?;
                    let b = b_val.as_f64().or_else(|| b_val.as_i32().map(|i| i as f64)).unwrap_or(0.0);
                    let a = a_val.as_f64().or_else(|| a_val.as_i32().map(|i| i as f64)).unwrap_or(0.0);
                    call_stack.push(Value::f64(a - b))?;
                }
                Opcode::Fmul => {
                    let b_val = call_stack.pop()?;
                    let a_val = call_stack.pop()?;
                    let b = b_val.as_f64().or_else(|| b_val.as_i32().map(|i| i as f64)).unwrap_or(0.0);
                    let a = a_val.as_f64().or_else(|| a_val.as_i32().map(|i| i as f64)).unwrap_or(0.0);
                    call_stack.push(Value::f64(a * b))?;
                }
                Opcode::Fdiv => {
                    let b_val = call_stack.pop()?;
                    let a_val = call_stack.pop()?;
                    let b = b_val.as_f64().or_else(|| b_val.as_i32().map(|i| i as f64)).unwrap_or(1.0);
                    let a = a_val.as_f64().or_else(|| a_val.as_i32().map(|i| i as f64)).unwrap_or(0.0);
                    call_stack.push(Value::f64(a / b))?;
                }
                Opcode::Fle => {
                    let b_val = call_stack.pop()?;
                    let a_val = call_stack.pop()?;
                    let b = b_val.as_f64().or_else(|| b_val.as_i32().map(|i| i as f64)).unwrap_or(0.0);
                    let a = a_val.as_f64().or_else(|| a_val.as_i32().map(|i| i as f64)).unwrap_or(0.0);
                    call_stack.push(Value::bool(a <= b))?;
                }
                Opcode::Flt => {
                    let b_val = call_stack.pop()?;
                    let a_val = call_stack.pop()?;
                    let b = b_val.as_f64().or_else(|| b_val.as_i32().map(|i| i as f64)).unwrap_or(0.0);
                    let a = a_val.as_f64().or_else(|| a_val.as_i32().map(|i| i as f64)).unwrap_or(0.0);
                    call_stack.push(Value::bool(a < b))?;
                }
                Opcode::Fgt => {
                    let b_val = call_stack.pop()?;
                    let a_val = call_stack.pop()?;
                    let b = b_val.as_f64().or_else(|| b_val.as_i32().map(|i| i as f64)).unwrap_or(0.0);
                    let a = a_val.as_f64().or_else(|| a_val.as_i32().map(|i| i as f64)).unwrap_or(0.0);
                    call_stack.push(Value::bool(a > b))?;
                }
                Opcode::Fge => {
                    let b_val = call_stack.pop()?;
                    let a_val = call_stack.pop()?;
                    let b = b_val.as_f64().or_else(|| b_val.as_i32().map(|i| i as f64)).unwrap_or(0.0);
                    let a = a_val.as_f64().or_else(|| a_val.as_i32().map(|i| i as f64)).unwrap_or(0.0);
                    call_stack.push(Value::bool(a >= b))?;
                }
                Opcode::Feq => {
                    let b_val = call_stack.pop()?;
                    let a_val = call_stack.pop()?;
                    let b = b_val.as_f64().or_else(|| b_val.as_i32().map(|i| i as f64)).unwrap_or(0.0);
                    let a = a_val.as_f64().or_else(|| a_val.as_i32().map(|i| i as f64)).unwrap_or(0.0);
                    call_stack.push(Value::bool(a == b))?;
                }
                Opcode::Fne => {
                    let b_val = call_stack.pop()?;
                    let a_val = call_stack.pop()?;
                    let b = b_val.as_f64().or_else(|| b_val.as_i32().map(|i| i as f64)).unwrap_or(0.0);
                    let a = a_val.as_f64().or_else(|| a_val.as_i32().map(|i| i as f64)).unwrap_or(0.0);
                    call_stack.push(Value::bool(a != b))?;
                }
                Opcode::LoadField => {
                    let field_offset = Self::read_u16(code, &mut ip)? as usize;
                    let obj_val = call_stack.pop()?;
                    if !obj_val.is_ptr() {
                        return Err(VmError::TypeError("Expected object".to_string()));
                    }
                    let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
                    let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                    let field_val = obj.get_field(field_offset).unwrap_or(Value::null());
                    call_stack.push(field_val)?;
                }
                Opcode::StoreField => {
                    let field_offset = Self::read_u16(code, &mut ip)? as usize;
                    let value = call_stack.pop()?;
                    let obj_val = call_stack.pop()?;
                    if !obj_val.is_ptr() {
                        return Err(VmError::TypeError("Expected object".to_string()));
                    }
                    let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
                    let obj = unsafe { &mut *obj_ptr.unwrap().as_ptr() };
                    obj.set_field(field_offset, value);
                }
                Opcode::LoadElem => {
                    let index = call_stack.pop()?.as_i32().unwrap_or(0) as usize;
                    let arr_val = call_stack.pop()?;
                    if !arr_val.is_ptr() {
                        return Err(VmError::TypeError("Expected array".to_string()));
                    }
                    let arr_ptr = unsafe { arr_val.as_ptr::<Array>() };
                    let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                    let value = arr.get(index).unwrap_or(Value::null());
                    call_stack.push(value)?;
                }
                Opcode::StoreElem => {
                    let value = call_stack.pop()?;
                    let index = call_stack.pop()?.as_i32().unwrap_or(0) as usize;
                    let arr_val = call_stack.pop()?;
                    if !arr_val.is_ptr() {
                        return Err(VmError::TypeError("Expected array".to_string()));
                    }
                    let arr_ptr = unsafe { arr_val.as_ptr::<Array>() };
                    let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                    let _ = arr.set(index, value);
                }
                Opcode::ArrayLen => {
                    let arr_val = call_stack.pop()?;
                    if !arr_val.is_ptr() {
                        return Err(VmError::TypeError("Expected array".to_string()));
                    }
                    let arr_ptr = unsafe { arr_val.as_ptr::<Array>() };
                    let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                    call_stack.push(Value::i32(arr.len() as i32))?;
                }
                Opcode::Jmp => {
                    let offset = Self::read_i16(code, &mut ip)?;
                    ip = (ip as isize + offset as isize) as usize;
                }
                Opcode::JmpIfFalse => {
                    let offset = Self::read_i16(code, &mut ip)?;
                    let cond = call_stack.pop()?;
                    if !cond.is_truthy() {
                        ip = (ip as isize + offset as isize) as usize;
                    }
                }
                Opcode::JmpIfTrue => {
                    let offset = Self::read_i16(code, &mut ip)?;
                    let cond = call_stack.pop()?;
                    if cond.is_truthy() {
                        ip = (ip as isize + offset as isize) as usize;
                    }
                }
                Opcode::Pop => {
                    call_stack.pop()?;
                }
                Opcode::Dup => {
                    let value = call_stack.peek()?;
                    call_stack.push(value)?;
                }
                Opcode::LoadCaptured => {
                    let capture_index = Self::read_u16(code, &mut ip)? as usize;

                    let closure_val = task.current_closure().ok_or_else(|| {
                        VmError::RuntimeError("LoadCaptured without active closure".to_string())
                    })?;

                    let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
                    let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                    let value = closure.get_captured(capture_index).ok_or_else(|| {
                        VmError::RuntimeError(format!(
                            "Capture index {} out of bounds",
                            capture_index
                        ))
                    })?;
                    call_stack.push(value)?;
                }
                Opcode::StoreCaptured => {
                    let capture_index = Self::read_u16(code, &mut ip)? as usize;
                    let value = call_stack.pop()?;

                    let closure_val = task.current_closure().ok_or_else(|| {
                        VmError::RuntimeError("StoreCaptured without active closure".to_string())
                    })?;

                    let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
                    let closure = unsafe { &mut *closure_ptr.unwrap().as_ptr() };
                    closure
                        .set_captured(capture_index, value)
                        .map_err(|e| VmError::RuntimeError(e))?;
                }
                Opcode::MakeClosure => {
                    let func_index = Self::read_u32(code, &mut ip)? as usize;
                    let capture_count = Self::read_u16(code, &mut ip)? as usize;

                    let mut captures = Vec::with_capacity(capture_count);
                    for _ in 0..capture_count {
                        captures.push(call_stack.pop()?);
                    }
                    captures.reverse();

                    let closure = Closure::new(func_index, captures);
                    let gc_ptr = self.gc.lock().allocate(closure);
                    let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    call_stack.push(value)?;
                }
                Opcode::LoadRefCell => {
                    let cell_val = call_stack.pop()?;
                    if !cell_val.is_ptr() {
                        return Err(VmError::TypeError("Expected RefCell".to_string()));
                    }
                    let cell_ptr = unsafe { cell_val.as_ptr::<crate::vm::object::RefCell>() };
                    let cell = unsafe { &*cell_ptr.unwrap().as_ptr() };
                    call_stack.push(cell.get())?;
                }
                Opcode::StoreRefCell => {
                    let value = call_stack.pop()?;
                    let cell_val = call_stack.pop()?;
                    if !cell_val.is_ptr() {
                        return Err(VmError::TypeError("Expected RefCell".to_string()));
                    }
                    let cell_ptr = unsafe { cell_val.as_ptr::<crate::vm::object::RefCell>() };
                    let cell = unsafe { &mut *cell_ptr.unwrap().as_ptr() };
                    cell.set(value);
                    call_stack.push(cell_val)?;
                }
                Opcode::NewRefCell => {
                    let value = call_stack.pop()?;
                    let refcell = crate::vm::object::RefCell::new(value);
                    let gc_ptr = self.gc.lock().allocate(refcell);
                    let cell_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    call_stack.push(cell_val)?;
                }
                Opcode::Call => {
                    let nested_func_index = Self::read_u32(code, &mut ip)? as usize;
                    let nested_arg_count = Self::read_u16(code, &mut ip)? as usize;

                    if nested_func_index == 0xFFFFFFFF {
                        // Closure call in nested context
                        let mut nested_args = Vec::with_capacity(nested_arg_count);
                        for _ in 0..nested_arg_count {
                            nested_args.push(call_stack.pop()?);
                        }
                        nested_args.reverse();

                        let closure_val = call_stack.pop()?;
                        if !closure_val.is_ptr() {
                            return Err(VmError::TypeError("Expected closure".to_string()));
                        }

                        let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
                        let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                        let closure_func_index = closure.func_id();

                        task.push_closure(closure_val);
                        let result =
                            self.execute_nested_function(task, closure_func_index, nested_args, module)?;
                        task.pop_closure();

                        call_stack.push(result)?;
                    } else {
                        // Regular function call in nested context
                        let mut nested_args = Vec::with_capacity(nested_arg_count);
                        for _ in 0..nested_arg_count {
                            nested_args.push(call_stack.pop()?);
                        }
                        nested_args.reverse();

                        let result =
                            self.execute_nested_function(task, nested_func_index, nested_args, module)?;
                        call_stack.push(result)?;
                    }
                }
                Opcode::Seq => {
                    let b_val = call_stack.pop()?;
                    let a_val = call_stack.pop()?;
                    let result = if a_val.is_ptr() && b_val.is_ptr() {
                        let a_ptr = unsafe { a_val.as_ptr::<RayaString>() };
                        let b_ptr = unsafe { b_val.as_ptr::<RayaString>() };
                        if let (Some(a_p), Some(b_p)) = (a_ptr, b_ptr) {
                            let a = unsafe { &*a_p.as_ptr() };
                            let b = unsafe { &*b_p.as_ptr() };
                            a.data == b.data
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    call_stack.push(Value::bool(result))?;
                }
                Opcode::Sne => {
                    let b_val = call_stack.pop()?;
                    let a_val = call_stack.pop()?;
                    let result = if a_val.is_ptr() && b_val.is_ptr() {
                        let a_ptr = unsafe { a_val.as_ptr::<RayaString>() };
                        let b_ptr = unsafe { b_val.as_ptr::<RayaString>() };
                        if let (Some(a_p), Some(b_p)) = (a_ptr, b_ptr) {
                            let a = unsafe { &*a_p.as_ptr() };
                            let b = unsafe { &*b_p.as_ptr() };
                            a.data != b.data
                        } else {
                            true
                        }
                    } else {
                        true
                    };
                    call_stack.push(Value::bool(result))?;
                }
                Opcode::Sconcat => {
                    let b_val = call_stack.pop()?;
                    let a_val = call_stack.pop()?;

                    let a_str = if a_val.is_ptr() {
                        let a_ptr = unsafe { a_val.as_ptr::<RayaString>() };
                        if let Some(p) = a_ptr {
                            let s = unsafe { &*p.as_ptr() };
                            s.data.clone()
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    };

                    let b_str = if b_val.is_ptr() {
                        let b_ptr = unsafe { b_val.as_ptr::<RayaString>() };
                        if let Some(p) = b_ptr {
                            let s = unsafe { &*p.as_ptr() };
                            s.data.clone()
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    };

                    let result = format!("{}{}", a_str, b_str);
                    let raya_string = RayaString::new(result);
                    let gc_ptr = self.gc.lock().allocate(raya_string);
                    let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    call_stack.push(value)?;
                }
                Opcode::Not => {
                    let val = call_stack.pop()?;
                    call_stack.push(Value::bool(!val.is_truthy()))?;
                }
                Opcode::ToString => {
                    let val = call_stack.pop()?;
                    let s = if val.is_null() {
                        "null".to_string()
                    } else if let Some(i) = val.as_i32() {
                        i.to_string()
                    } else if let Some(f) = val.as_f64() {
                        if f.fract() == 0.0 && f.abs() < 1e15 {
                            (f as i64).to_string()
                        } else {
                            f.to_string()
                        }
                    } else if let Some(b) = val.as_bool() {
                        if b { "true".to_string() } else { "false".to_string() }
                    } else if val.is_ptr() {
                        if let Some(str_ptr) = unsafe { val.as_ptr::<RayaString>() } {
                            let rs = unsafe { &*str_ptr.as_ptr() };
                            rs.data.clone()
                        } else {
                            "[object]".to_string()
                        }
                    } else {
                        "undefined".to_string()
                    };
                    let raya_string = RayaString::new(s);
                    let gc_ptr = self.gc.lock().allocate(raya_string);
                    let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    call_stack.push(value)?;
                }
                Opcode::MutexLock => {
                    let mutex_id_val = call_stack.pop()?;
                    let mutex_id = MutexId::from_u64(mutex_id_val.as_i64().unwrap_or(0) as u64);

                    // In nested context, we can only handle immediate lock acquisition
                    if let Some(mutex) = self.mutex_registry.get(mutex_id) {
                        match mutex.try_lock(task.id()) {
                            Ok(()) => {
                                task.add_held_mutex(mutex_id);
                                // Continue execution
                            }
                            Err(_) => {
                                return Err(VmError::RuntimeError(
                                    "Cannot wait for mutex in nested call context".to_string()
                                ));
                            }
                        }
                    } else {
                        return Err(VmError::RuntimeError(format!(
                            "Mutex {:?} not found", mutex_id
                        )));
                    }
                }
                Opcode::MutexUnlock => {
                    let mutex_id_val = call_stack.pop()?;
                    let mutex_id = MutexId::from_u64(mutex_id_val.as_i64().unwrap_or(0) as u64);

                    if let Some(mutex) = self.mutex_registry.get(mutex_id) {
                        match mutex.unlock(task.id()) {
                            Ok(next_waiter) => {
                                task.remove_held_mutex(mutex_id);

                                // If there's a waiting task, wake it up
                                if let Some(waiter_id) = next_waiter {
                                    let tasks = self.tasks.read();
                                    if let Some(waiter_task) = tasks.get(&waiter_id) {
                                        waiter_task.add_held_mutex(mutex_id);
                                        waiter_task.set_state(TaskState::Resumed);
                                        waiter_task.clear_suspend_reason();
                                        self.injector.push(waiter_task.clone());
                                    }
                                }
                            }
                            Err(e) => {
                                return Err(VmError::RuntimeError(format!(
                                    "Failed to unlock mutex: {}", e
                                )));
                            }
                        }
                    } else {
                        return Err(VmError::RuntimeError(format!(
                            "Mutex {:?} not found", mutex_id
                        )));
                    }
                }
                Opcode::NativeCall => {
                    use crate::vm::builtin::{mutex, buffer, map, set, channel, date, regexp};

                    let native_id = Self::read_u16(code, &mut ip)? as u32;
                    let arg_count = Self::read_u8(code, &mut ip)? as usize;

                    // Pop arguments
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        args.push(call_stack.pop()?);
                    }
                    args.reverse();

                    match native_id {
                        id if id == mutex::IS_LOCKED as u32 => {
                            let mutex_id = MutexId::from_u64(args[0].as_i64().unwrap_or(0) as u64);
                            if let Some(mutex) = self.mutex_registry.get(mutex_id) {
                                let is_locked = mutex.is_locked();
                                call_stack.push(Value::bool(is_locked))?;
                            } else {
                                return Err(VmError::RuntimeError(format!(
                                    "Mutex {:?} not found", mutex_id
                                )));
                            }
                        }
                        id if id == mutex::TRY_LOCK as u32 => {
                            let mutex_id = MutexId::from_u64(args[0].as_i64().unwrap_or(0) as u64);
                            if let Some(mutex) = self.mutex_registry.get(mutex_id) {
                                match mutex.try_lock(task.id()) {
                                    Ok(()) => {
                                        task.add_held_mutex(mutex_id);
                                        call_stack.push(Value::bool(true))?;
                                    }
                                    Err(_) => {
                                        call_stack.push(Value::bool(false))?;
                                    }
                                }
                            } else {
                                return Err(VmError::RuntimeError(format!(
                                    "Mutex {:?} not found", mutex_id
                                )));
                            }
                        }
                        // Buffer native calls
                        id if id == buffer::NEW as u32 => {
                            let size = args[0].as_i32().unwrap_or(0) as usize;
                            let buf = Buffer::new(size);
                            let gc_ptr = self.gc.lock().allocate(buf);
                            let handle = gc_ptr.as_ptr() as u64;
                            call_stack.push(Value::u64(handle))?;
                        }
                        id if id == buffer::LENGTH as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let buf_ptr = handle as *const Buffer;
                            if buf_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid buffer handle".to_string()));
                            }
                            let buf = unsafe { &*buf_ptr };
                            call_stack.push(Value::i32(buf.length() as i32))?;
                        }
                        id if id == buffer::GET_BYTE as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let index = args[1].as_i32().unwrap_or(0) as usize;
                            let buf_ptr = handle as *const Buffer;
                            if buf_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid buffer handle".to_string()));
                            }
                            let buf = unsafe { &*buf_ptr };
                            let value = buf.get_byte(index).unwrap_or(0);
                            call_stack.push(Value::i32(value as i32))?;
                        }
                        id if id == buffer::SET_BYTE as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let index = args[1].as_i32().unwrap_or(0) as usize;
                            let value = args[2].as_i32().unwrap_or(0) as u8;
                            let buf_ptr = handle as *mut Buffer;
                            if buf_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid buffer handle".to_string()));
                            }
                            let buf = unsafe { &mut *buf_ptr };
                            buf.set_byte(index, value).map_err(VmError::RuntimeError)?;
                            call_stack.push(Value::null())?;
                        }
                        id if id == buffer::GET_INT32 as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let index = args[1].as_i32().unwrap_or(0) as usize;
                            let buf_ptr = handle as *const Buffer;
                            if buf_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid buffer handle".to_string()));
                            }
                            let buf = unsafe { &*buf_ptr };
                            let value = buf.get_int32(index).unwrap_or(0);
                            call_stack.push(Value::i32(value))?;
                        }
                        id if id == buffer::SET_INT32 as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let index = args[1].as_i32().unwrap_or(0) as usize;
                            let value = args[2].as_i32().unwrap_or(0);
                            let buf_ptr = handle as *mut Buffer;
                            if buf_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid buffer handle".to_string()));
                            }
                            let buf = unsafe { &mut *buf_ptr };
                            buf.set_int32(index, value).map_err(VmError::RuntimeError)?;
                            call_stack.push(Value::null())?;
                        }
                        id if id == buffer::GET_FLOAT64 as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let index = args[1].as_i32().unwrap_or(0) as usize;
                            let buf_ptr = handle as *const Buffer;
                            if buf_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid buffer handle".to_string()));
                            }
                            let buf = unsafe { &*buf_ptr };
                            let value = buf.get_float64(index).unwrap_or(0.0);
                            call_stack.push(Value::f64(value))?;
                        }
                        id if id == buffer::SET_FLOAT64 as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let index = args[1].as_i32().unwrap_or(0) as usize;
                            let value = args[2].as_f64().unwrap_or(0.0);
                            let buf_ptr = handle as *mut Buffer;
                            if buf_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid buffer handle".to_string()));
                            }
                            let buf = unsafe { &mut *buf_ptr };
                            buf.set_float64(index, value).map_err(VmError::RuntimeError)?;
                            call_stack.push(Value::null())?;
                        }
                        // Map native calls
                        id if id == map::NEW as u32 => {
                            let map = MapObject::new();
                            let gc_ptr = self.gc.lock().allocate(map);
                            let handle = gc_ptr.as_ptr() as u64;
                            call_stack.push(Value::u64(handle))?;
                        }
                        id if id == map::SIZE as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let map_ptr = handle as *const MapObject;
                            if map_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid map handle".to_string()));
                            }
                            let map = unsafe { &*map_ptr };
                            call_stack.push(Value::i32(map.size() as i32))?;
                        }
                        id if id == map::GET as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let key = args[1];
                            let map_ptr = handle as *const MapObject;
                            if map_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid map handle".to_string()));
                            }
                            let map = unsafe { &*map_ptr };
                            let value = map.get(key).unwrap_or(Value::null());
                            call_stack.push(value)?;
                        }
                        id if id == map::SET as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let key = args[1];
                            let value = args[2];
                            let map_ptr = handle as *mut MapObject;
                            if map_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid map handle".to_string()));
                            }
                            let map = unsafe { &mut *map_ptr };
                            map.set(key, value);
                            call_stack.push(Value::null())?;
                        }
                        id if id == map::HAS as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let key = args[1];
                            let map_ptr = handle as *const MapObject;
                            if map_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid map handle".to_string()));
                            }
                            let map = unsafe { &*map_ptr };
                            call_stack.push(Value::bool(map.has(key)))?;
                        }
                        id if id == map::DELETE as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let key = args[1];
                            let map_ptr = handle as *mut MapObject;
                            if map_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid map handle".to_string()));
                            }
                            let map = unsafe { &mut *map_ptr };
                            let result = map.delete(key);
                            call_stack.push(Value::bool(result))?;
                        }
                        id if id == map::CLEAR as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let map_ptr = handle as *mut MapObject;
                            if map_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid map handle".to_string()));
                            }
                            let map = unsafe { &mut *map_ptr };
                            map.clear();
                            call_stack.push(Value::null())?;
                        }
                        // Set native calls
                        id if id == set::NEW as u32 => {
                            let set = SetObject::new();
                            let gc_ptr = self.gc.lock().allocate(set);
                            let handle = gc_ptr.as_ptr() as u64;
                            call_stack.push(Value::u64(handle))?;
                        }
                        id if id == set::SIZE as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let set_ptr = handle as *const SetObject;
                            if set_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid set handle".to_string()));
                            }
                            let set = unsafe { &*set_ptr };
                            call_stack.push(Value::i32(set.size() as i32))?;
                        }
                        id if id == set::ADD as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let value = args[1];
                            let set_ptr = handle as *mut SetObject;
                            if set_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid set handle".to_string()));
                            }
                            let set = unsafe { &mut *set_ptr };
                            set.add(value);
                            call_stack.push(Value::null())?;
                        }
                        id if id == set::HAS as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let value = args[1];
                            let set_ptr = handle as *const SetObject;
                            if set_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid set handle".to_string()));
                            }
                            let set = unsafe { &*set_ptr };
                            call_stack.push(Value::bool(set.has(value)))?;
                        }
                        id if id == set::DELETE as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let value = args[1];
                            let set_ptr = handle as *mut SetObject;
                            if set_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid set handle".to_string()));
                            }
                            let set = unsafe { &mut *set_ptr };
                            let result = set.delete(value);
                            call_stack.push(Value::bool(result))?;
                        }
                        id if id == set::CLEAR as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let set_ptr = handle as *mut SetObject;
                            if set_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid set handle".to_string()));
                            }
                            let set = unsafe { &mut *set_ptr };
                            set.clear();
                            call_stack.push(Value::null())?;
                        }
                        // Channel native calls
                        id if id == channel::NEW as u32 => {
                            let capacity = args[0].as_i32().unwrap_or(0) as usize;
                            let ch = ChannelObject::new(capacity);
                            let gc_ptr = self.gc.lock().allocate(ch);
                            let handle = gc_ptr.as_ptr() as u64;
                            call_stack.push(Value::u64(handle))?;
                        }
                        id if id == channel::SEND as u32 => {
                            // Blocking send - for nested calls, try to send and error if full
                            let handle = args[0].as_u64().unwrap_or(0);
                            let value = args[1];
                            let ch_ptr = handle as *const ChannelObject;
                            if ch_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid channel handle".to_string()));
                            }
                            let ch = unsafe { &*ch_ptr };
                            if !ch.try_send(value) {
                                return Err(VmError::RuntimeError("Channel buffer full".to_string()));
                            }
                            call_stack.push(Value::null())?;
                        }
                        id if id == channel::RECEIVE as u32 => {
                            // Blocking receive - for nested calls, try to receive and error if empty
                            let handle = args[0].as_u64().unwrap_or(0);
                            let ch_ptr = handle as *const ChannelObject;
                            if ch_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid channel handle".to_string()));
                            }
                            let ch = unsafe { &*ch_ptr };
                            if let Some(value) = ch.try_receive() {
                                call_stack.push(value)?;
                            } else {
                                return Err(VmError::RuntimeError("Channel buffer empty".to_string()));
                            }
                        }
                        id if id == channel::TRY_SEND as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let value = args[1];
                            let ch_ptr = handle as *const ChannelObject;
                            if ch_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid channel handle".to_string()));
                            }
                            let ch = unsafe { &*ch_ptr };
                            let result = ch.try_send(value);
                            call_stack.push(Value::bool(result))?;
                        }
                        id if id == channel::TRY_RECEIVE as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let ch_ptr = handle as *const ChannelObject;
                            if ch_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid channel handle".to_string()));
                            }
                            let ch = unsafe { &*ch_ptr };
                            let result = ch.try_receive().unwrap_or(Value::null());
                            call_stack.push(result)?;
                        }
                        id if id == channel::CLOSE as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let ch_ptr = handle as *const ChannelObject;
                            if ch_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid channel handle".to_string()));
                            }
                            let ch = unsafe { &*ch_ptr };
                            ch.close();
                            call_stack.push(Value::null())?;
                        }
                        id if id == channel::IS_CLOSED as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let ch_ptr = handle as *const ChannelObject;
                            if ch_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid channel handle".to_string()));
                            }
                            let ch = unsafe { &*ch_ptr };
                            call_stack.push(Value::bool(ch.is_closed()))?;
                        }
                        id if id == channel::LENGTH as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let ch_ptr = handle as *const ChannelObject;
                            if ch_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid channel handle".to_string()));
                            }
                            let ch = unsafe { &*ch_ptr };
                            call_stack.push(Value::i32(ch.length() as i32))?;
                        }
                        id if id == channel::CAPACITY as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let ch_ptr = handle as *const ChannelObject;
                            if ch_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid channel handle".to_string()));
                            }
                            let ch = unsafe { &*ch_ptr };
                            call_stack.push(Value::i32(ch.capacity() as i32))?;
                        }
                        // Date native calls
                        id if id == date::NOW as u32 => {
                            use std::time::{SystemTime, UNIX_EPOCH};
                            let now = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .map(|d| d.as_millis() as f64)
                                .unwrap_or(0.0);
                            call_stack.push(Value::f64(now))?;
                        }
                        id if id == date::GET_FULL_YEAR as u32 => {
                            // args[0] is the timestamp in milliseconds (as f64 number)
                            let timestamp = args[0].as_f64()
                                .or_else(|| args[0].as_i64().map(|v| v as f64))
                                .or_else(|| args[0].as_i32().map(|v| v as f64))
                                .unwrap_or(0.0) as i64;
                            let date = DateObject::from_timestamp(timestamp);
                            call_stack.push(Value::i32(date.get_full_year()))?;
                        }
                        id if id == date::GET_MONTH as u32 => {
                            let timestamp = args[0].as_f64()
                                .or_else(|| args[0].as_i64().map(|v| v as f64))
                                .or_else(|| args[0].as_i32().map(|v| v as f64))
                                .unwrap_or(0.0) as i64;
                            let date = DateObject::from_timestamp(timestamp);
                            call_stack.push(Value::i32(date.get_month()))?;
                        }
                        id if id == date::GET_DATE as u32 => {
                            let timestamp = args[0].as_f64()
                                .or_else(|| args[0].as_i64().map(|v| v as f64))
                                .or_else(|| args[0].as_i32().map(|v| v as f64))
                                .unwrap_or(0.0) as i64;
                            let date = DateObject::from_timestamp(timestamp);
                            call_stack.push(Value::i32(date.get_date()))?;
                        }
                        id if id == date::GET_DAY as u32 => {
                            let timestamp = args[0].as_f64()
                                .or_else(|| args[0].as_i64().map(|v| v as f64))
                                .or_else(|| args[0].as_i32().map(|v| v as f64))
                                .unwrap_or(0.0) as i64;
                            let date = DateObject::from_timestamp(timestamp);
                            call_stack.push(Value::i32(date.get_day()))?;
                        }
                        // RegExp native calls
                        id if id == regexp::NEW as u32 => {
                            // args: pattern (string), flags (string)
                            let pattern = if args[0].is_ptr() {
                                if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                                    unsafe { &*s.as_ptr() }.data.clone()
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            };
                            let flags = if args.len() > 1 && args[1].is_ptr() {
                                if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                    unsafe { &*s.as_ptr() }.data.clone()
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            };
                            match RegExpObject::new(&pattern, &flags) {
                                Ok(re) => {
                                    let gc_ptr = self.gc.lock().allocate(re);
                                    let handle = gc_ptr.as_ptr() as i64;
                                    call_stack.push(Value::i64(handle))?;
                                }
                                Err(e) => {
                                    return Err(VmError::RuntimeError(format!("Invalid regex: {}", e)));
                                }
                            }
                        }
                        id if id == regexp::TEST as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let input = if args[1].is_ptr() {
                                if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                    unsafe { &*s.as_ptr() }.data.clone()
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            };
                            let re_ptr = handle as *const RegExpObject;
                            if re_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
                            }
                            let re = unsafe { &*re_ptr };
                            call_stack.push(Value::bool(re.test(&input)))?;
                        }
                        id if id == regexp::EXEC as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let input = if args[1].is_ptr() {
                                if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                    unsafe { &*s.as_ptr() }.data.clone()
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            };
                            let re_ptr = handle as *const RegExpObject;
                            if re_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
                            }
                            let re = unsafe { &*re_ptr };
                            match re.exec(&input) {
                                Some((matched, index, groups)) => {
                                    // Return array: [matched_text, index, ...groups]
                                    let mut arr = Array::new(0, 0);
                                    // [0] = matched text
                                    let matched_str = RayaString::new(matched);
                                    let gc_ptr = self.gc.lock().allocate(matched_str);
                                    let matched_val = unsafe {
                                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                                    };
                                    arr.push(matched_val);
                                    // [1] = index
                                    arr.push(Value::i32(index as i32));
                                    // [2..] = groups
                                    for group in groups {
                                        let group_str = RayaString::new(group);
                                        let gc_ptr = self.gc.lock().allocate(group_str);
                                        let group_val = unsafe {
                                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                                        };
                                        arr.push(group_val);
                                    }
                                    let arr_gc = self.gc.lock().allocate(arr);
                                    let arr_val = unsafe {
                                        Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                                    };
                                    call_stack.push(arr_val)?;
                                }
                                None => {
                                    call_stack.push(Value::null())?;
                                }
                            }
                        }
                        id if id == regexp::EXEC_ALL as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let input = if args[1].is_ptr() {
                                if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                    unsafe { &*s.as_ptr() }.data.clone()
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            };
                            let re_ptr = handle as *const RegExpObject;
                            if re_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
                            }
                            let re = unsafe { &*re_ptr };
                            let matches = re.exec_all(&input);
                            // Create array of match arrays (each match is [matched_text, index, ...groups])
                            let mut result_arr = Array::new(0, 0);
                            for (matched, index, groups) in matches {
                                let mut match_arr = Array::new(0, 0);
                                // [0] = matched text
                                let matched_str = RayaString::new(matched);
                                let gc_ptr = self.gc.lock().allocate(matched_str);
                                let matched_val = unsafe {
                                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                                };
                                match_arr.push(matched_val);
                                // [1] = index
                                match_arr.push(Value::i32(index as i32));
                                // [2..] = groups
                                for group in groups {
                                    let group_str = RayaString::new(group);
                                    let gc_ptr = self.gc.lock().allocate(group_str);
                                    let group_val = unsafe {
                                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                                    };
                                    match_arr.push(group_val);
                                }
                                let match_arr_gc = self.gc.lock().allocate(match_arr);
                                let match_arr_val = unsafe {
                                    Value::from_ptr(std::ptr::NonNull::new(match_arr_gc.as_ptr()).unwrap())
                                };
                                result_arr.push(match_arr_val);
                            }
                            let arr_gc = self.gc.lock().allocate(result_arr);
                            let arr_val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                            };
                            call_stack.push(arr_val)?;
                        }
                        id if id == regexp::REPLACE as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let input = if args[1].is_ptr() {
                                if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                    unsafe { &*s.as_ptr() }.data.clone()
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            };
                            let replacement = if args[2].is_ptr() {
                                if let Some(s) = unsafe { args[2].as_ptr::<RayaString>() } {
                                    unsafe { &*s.as_ptr() }.data.clone()
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            };
                            let re_ptr = handle as *const RegExpObject;
                            if re_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
                            }
                            let re = unsafe { &*re_ptr };
                            let result = re.replace(&input, &replacement);
                            let result_str = RayaString::new(result);
                            let gc_ptr = self.gc.lock().allocate(result_str);
                            let result_val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            };
                            call_stack.push(result_val)?;
                        }
                        id if id == regexp::SPLIT as u32 => {
                            let handle = args[0].as_u64().unwrap_or(0);
                            let input = if args[1].is_ptr() {
                                if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                    unsafe { &*s.as_ptr() }.data.clone()
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            };
                            let limit = if args.len() > 2 {
                                args[2].as_i32().map(|v| v as usize)
                            } else {
                                None
                            };
                            let re_ptr = handle as *const RegExpObject;
                            if re_ptr.is_null() {
                                return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
                            }
                            let re = unsafe { &*re_ptr };
                            let parts = re.split(&input, limit);
                            let mut arr = Array::new(0, 0);
                            for part in parts {
                                let s = RayaString::new(part);
                                let gc_ptr = self.gc.lock().allocate(s);
                                let val = unsafe {
                                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                                };
                                arr.push(val);
                            }
                            let arr_gc = self.gc.lock().allocate(arr);
                            let arr_val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                            };
                            call_stack.push(arr_val)?;
                        }
                        _ => {
                            return Err(VmError::RuntimeError(format!(
                                "NativeCall {:#06x} not implemented in nested call (args={})",
                                native_id, args.len()
                            )));
                        }
                    }
                }
                Opcode::New => {
                    let class_index = Self::read_u16(code, &mut ip)? as usize;
                    let classes = self.classes.read();
                    let class = classes.get_class(class_index).ok_or_else(|| {
                        VmError::RuntimeError(format!("Invalid class index: {}", class_index))
                    })?;
                    let field_count = class.field_count;
                    drop(classes);

                    let obj = Object::new(class_index, field_count);
                    let gc_ptr = self.gc.lock().allocate(obj);
                    let value = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                    };
                    call_stack.push(value)?;
                }
                Opcode::CallConstructor => {
                    let class_index = Self::read_u16(code, &mut ip)? as usize;
                    let arg_count = Self::read_u8(code, &mut ip)? as usize;

                    // Pop arguments
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        args.push(call_stack.pop()?);
                    }
                    args.reverse();

                    // Look up class and create object
                    let classes = self.classes.read();
                    let class = classes.get_class(class_index).ok_or_else(|| {
                        VmError::RuntimeError(format!("Invalid class index: {}", class_index))
                    })?;
                    let field_count = class.field_count;
                    let constructor_id = class.constructor_id;
                    drop(classes);

                    // Create the object
                    let obj = Object::new(class_index, field_count);
                    let gc_ptr = self.gc.lock().allocate(obj);
                    let obj_val = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                    };

                    if let Some(ctor_id) = constructor_id {
                        // Add self as first argument
                        let mut ctor_args = vec![obj_val];
                        ctor_args.extend(args);

                        // Call constructor
                        let _ = self.execute_nested_function(task, ctor_id, ctor_args, module)?;
                    }

                    // Push object back
                    call_stack.push(obj_val)?;
                }
                Opcode::CallSuper => {
                    let class_index = Self::read_u16(code, &mut ip)? as usize;
                    let arg_count = Self::read_u8(code, &mut ip)? as usize;

                    // Pop arguments (plus 'this' receiver)
                    let mut args = Vec::with_capacity(arg_count + 1);
                    for _ in 0..(arg_count + 1) {
                        args.push(call_stack.pop()?);
                    }
                    args.reverse();

                    // Get the parent class's constructor
                    let classes = self.classes.read();
                    let class = classes.get_class(class_index).ok_or_else(|| {
                        VmError::RuntimeError(format!("Invalid class index: {}", class_index))
                    })?;
                    let parent_id = class.parent_id.ok_or_else(|| {
                        VmError::RuntimeError("Class has no parent".to_string())
                    })?;
                    let parent_class = classes.get_class(parent_id).ok_or_else(|| {
                        VmError::RuntimeError(format!("Invalid parent class ID: {}", parent_id))
                    })?;
                    let constructor_id = parent_class.get_constructor();
                    drop(classes);

                    // Call parent constructor if it exists
                    if let Some(ctor_id) = constructor_id {
                        let _ = self.execute_nested_function(task, ctor_id, args, module)?;
                    }
                }
                Opcode::Throw => {
                    eprintln!("[DEBUG] Throw opcode in execute_nested_function");
                    // Pop exception value from stack
                    let exception = call_stack.pop()?;

                    // If exception is an Error object, set its stack property
                    if exception.is_ptr() {
                        if let Some(obj_ptr) = unsafe { exception.as_ptr::<Object>() } {
                            let obj = unsafe { &mut *obj_ptr.as_ptr() };
                            let classes = self.classes.read();

                            // Check if this is an Error or subclass
                            if let Some(class) = classes.get_class(obj.class_id) {
                                let is_error = class.name == "Error"
                                    || class.name == "TypeError"
                                    || class.name == "RangeError"
                                    || class.name == "ReferenceError"
                                    || class.name == "SyntaxError"
                                    || class.name == "ChannelClosedError"
                                    || class.name == "AssertionError";

                                eprintln!("[DEBUG] Throw in nested: class={}, is_error={}, fields_len={}",
                                    class.name, is_error, obj.fields.len());

                                if is_error && obj.fields.len() >= 3 {
                                    // Get error name and message
                                    let error_name = if let Some(name_ptr) =
                                        unsafe { obj.fields[1].as_ptr::<RayaString>() }
                                    {
                                        unsafe { &*name_ptr.as_ptr() }.data.clone()
                                    } else {
                                        "Error".to_string()
                                    };

                                    let error_message = if let Some(msg_ptr) =
                                        unsafe { obj.fields[0].as_ptr::<RayaString>() }
                                    {
                                        unsafe { &*msg_ptr.as_ptr() }.data.clone()
                                    } else {
                                        String::new()
                                    };

                                    drop(classes);

                                    // Build stack trace
                                    let stack_trace =
                                        task.build_stack_trace(&error_name, &error_message);

                                    eprintln!("[DEBUG] Built stack trace: {}", stack_trace);

                                    // Allocate stack trace string
                                    let raya_string = RayaString::new(stack_trace);
                                    let gc_ptr = self.gc.lock().allocate(raya_string);
                                    let stack_value = unsafe {
                                        Value::from_ptr(
                                            std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                        )
                                    };

                                    eprintln!("[DEBUG] Setting field[2], is_ptr before: {}", obj.fields[2].is_ptr());

                                    // Set stack field (index 2)
                                    obj.fields[2] = stack_value;

                                    eprintln!("[DEBUG] Set field[2], is_ptr after: {}", obj.fields[2].is_ptr());
                                }
                            }
                        }
                    }

                    // Set exception and return error to propagate
                    task.set_exception(exception);
                    return Err(VmError::RuntimeError("throw".to_string()));
                }
                _ => {
                    return Err(VmError::RuntimeError(format!(
                        "Opcode {:?} not implemented in nested call",
                        opcode
                    )));
                }
            }
        }
    }

    // ===== Helper Methods =====

    #[inline]
    fn read_u8(code: &[u8], ip: &mut usize) -> Result<u8, VmError> {
        if *ip >= code.len() {
            return Err(VmError::RuntimeError(
                "Unexpected end of bytecode".to_string(),
            ));
        }
        let value = code[*ip];
        *ip += 1;
        Ok(value)
    }

    #[inline]
    fn read_u16(code: &[u8], ip: &mut usize) -> Result<u16, VmError> {
        if *ip + 1 >= code.len() {
            return Err(VmError::RuntimeError(
                "Unexpected end of bytecode".to_string(),
            ));
        }
        let value = u16::from_le_bytes([code[*ip], code[*ip + 1]]);
        *ip += 2;
        Ok(value)
    }

    #[inline]
    fn read_i16(code: &[u8], ip: &mut usize) -> Result<i16, VmError> {
        if *ip + 1 >= code.len() {
            return Err(VmError::RuntimeError(
                "Unexpected end of bytecode".to_string(),
            ));
        }
        let value = i16::from_le_bytes([code[*ip], code[*ip + 1]]);
        *ip += 2;
        Ok(value)
    }

    #[inline]
    fn read_u32(code: &[u8], ip: &mut usize) -> Result<u32, VmError> {
        if *ip + 3 >= code.len() {
            return Err(VmError::RuntimeError(
                "Unexpected end of bytecode".to_string(),
            ));
        }
        let value = u32::from_le_bytes([code[*ip], code[*ip + 1], code[*ip + 2], code[*ip + 3]]);
        *ip += 4;
        Ok(value)
    }

    #[inline]
    fn read_i32(code: &[u8], ip: &mut usize) -> Result<i32, VmError> {
        if *ip + 3 >= code.len() {
            return Err(VmError::RuntimeError(
                "Unexpected end of bytecode".to_string(),
            ));
        }
        let value = i32::from_le_bytes([code[*ip], code[*ip + 1], code[*ip + 2], code[*ip + 3]]);
        *ip += 4;
        Ok(value)
    }

    #[inline]
    fn read_f64(code: &[u8], ip: &mut usize) -> Result<f64, VmError> {
        if *ip + 7 >= code.len() {
            return Err(VmError::RuntimeError(
                "Unexpected end of bytecode".to_string(),
            ));
        }
        let bytes = [
            code[*ip],
            code[*ip + 1],
            code[*ip + 2],
            code[*ip + 3],
            code[*ip + 4],
            code[*ip + 5],
            code[*ip + 6],
            code[*ip + 7],
        ];
        let value = f64::from_le_bytes(bytes);
        *ip += 8;
        Ok(value)
    }

    // =========================================================
    // Built-in Method Handlers
    // =========================================================

    /// Handle built-in array methods
    fn call_array_method(
        &mut self,
        task: &Arc<Task>,
        stack: &mut std::sync::MutexGuard<'_, Stack>,
        method_id: u16,
        arg_count: usize,
        module: &Module,
    ) -> Result<(), VmError> {
        use crate::vm::builtin::array;

        match method_id {
            array::PUSH => {
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.push expects 1 argument, got {}", arg_count
                    )));
                }
                let value = stack.pop()?;
                let array_val = stack.pop()?;
                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                let new_len = arr.push(value);
                stack.push(Value::i32(new_len as i32))?;
                Ok(())
            }
            array::POP => {
                if arg_count != 0 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.pop expects 0 arguments, got {}", arg_count
                    )));
                }
                let array_val = stack.pop()?;
                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                let result = arr.pop().unwrap_or(Value::null());
                stack.push(result)?;
                Ok(())
            }
            array::SHIFT => {
                if arg_count != 0 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.shift expects 0 arguments, got {}", arg_count
                    )));
                }
                let array_val = stack.pop()?;
                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                let result = arr.shift().unwrap_or(Value::null());
                stack.push(result)?;
                Ok(())
            }
            array::UNSHIFT => {
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.unshift expects 1 argument, got {}", arg_count
                    )));
                }
                let value = stack.pop()?;
                let array_val = stack.pop()?;
                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                let new_len = arr.unshift(value);
                stack.push(Value::i32(new_len as i32))?;
                Ok(())
            }
            array::INDEX_OF => {
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.indexOf expects 1 argument, got {}", arg_count
                    )));
                }
                let value = stack.pop()?;
                let array_val = stack.pop()?;
                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                let result = arr.index_of(value);
                stack.push(Value::i32(result))?;
                Ok(())
            }
            array::INCLUDES => {
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.includes expects 1 argument, got {}", arg_count
                    )));
                }
                let value = stack.pop()?;
                let array_val = stack.pop()?;
                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                let result = arr.includes(value);
                stack.push(Value::bool(result))?;
                Ok(())
            }
            array::SLICE => {
                // slice(start, end?) - arg_count is 1 or 2
                let end_val = if arg_count >= 2 { Some(stack.pop()?) } else { None };
                let start_val = if arg_count >= 1 { stack.pop()? } else { Value::i32(0) };
                let array_val = stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };

                let len = arr.len();
                let start = start_val.as_i32().unwrap_or(0) as usize;
                let end = end_val.and_then(|v| v.as_i32()).map(|e| e as usize).unwrap_or(len);
                let start = start.min(len);
                let end = end.min(len);

                let mut new_arr = Array::new(arr.type_id, 0);
                if start < end {
                    for i in start..end {
                        if let Some(v) = arr.get(i) {
                            new_arr.push(v);
                        }
                    }
                }
                let gc_ptr = self.gc.lock().allocate(new_arr);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            array::REVERSE => {
                if arg_count != 0 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.reverse expects 0 arguments, got {}", arg_count
                    )));
                }
                let array_val = stack.pop()?;
                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                arr.elements.reverse();
                stack.push(array_val)?;
                Ok(())
            }
            array::CONCAT => {
                // concat(other): merge two arrays
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.concat expects 1 argument, got {}", arg_count
                    )));
                }
                let other_val = stack.pop()?;
                let array_val = stack.pop()?;

                if !array_val.is_ptr() || !other_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                let other_ptr = unsafe { other_val.as_ptr::<Array>() };
                let other = unsafe { &*other_ptr.unwrap().as_ptr() };

                let mut new_arr = Array::new(0, 0);
                for elem in arr.elements.iter() {
                    new_arr.push(*elem);
                }
                for elem in other.elements.iter() {
                    new_arr.push(*elem);
                }

                let gc_ptr = self.gc.lock().allocate(new_arr);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            array::LAST_INDEX_OF => {
                // lastIndexOf(value): find last occurrence
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.lastIndexOf expects 1 argument, got {}", arg_count
                    )));
                }
                let search_val = stack.pop()?;
                let array_val = stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };

                let mut found_index: i32 = -1;
                for (i, elem) in arr.elements.iter().enumerate().rev() {
                    // Compare values
                    let matches = if let (Some(a), Some(b)) = (elem.as_i32(), search_val.as_i32()) {
                        a == b
                    } else if let (Some(a), Some(b)) = (elem.as_f64(), search_val.as_f64()) {
                        a == b
                    } else if let (Some(a), Some(b)) = (elem.as_bool(), search_val.as_bool()) {
                        a == b
                    } else if elem.is_null() && search_val.is_null() {
                        true
                    } else {
                        false
                    };
                    if matches {
                        found_index = i as i32;
                        break;
                    }
                }

                stack.push(Value::i32(found_index))?;
                Ok(())
            }
            array::FILL => {
                // fill(value, start?, end?): fill with value
                if arg_count < 1 || arg_count > 3 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.fill expects 1-3 arguments, got {}", arg_count
                    )));
                }

                // Pop arguments in reverse order
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    args.push(stack.pop()?);
                }
                args.reverse();

                let array_val = stack.pop()?;
                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };

                let fill_value = args[0];
                let start = if arg_count >= 2 { args[1].as_i32().unwrap_or(0).max(0) as usize } else { 0 };
                let end = if arg_count >= 3 { args[2].as_i32().unwrap_or(arr.len() as i32).max(0) as usize } else { arr.len() };

                for i in start..end.min(arr.len()) {
                    arr.elements[i] = fill_value;
                }

                stack.push(array_val)?;
                Ok(())
            }
            array::FLAT => {
                // flat(depth?): flatten nested arrays
                let depth = if arg_count >= 1 {
                    let d = stack.pop()?.as_i32().unwrap_or(1);
                    d.max(0) as usize
                } else {
                    1
                };
                let array_val = stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };

                fn flatten(gc: &parking_lot::Mutex<crate::vm::gc::Gc>, arr: &Array, depth: usize) -> Array {
                    let mut result = Array::new(0, 0);
                    for elem in arr.elements.iter() {
                        if depth > 0 && elem.is_ptr() {
                            if let Some(ptr) = unsafe { elem.as_ptr::<Array>() } {
                                let inner = unsafe { &*ptr.as_ptr() };
                                let flattened = flatten(gc, inner, depth - 1);
                                for inner_elem in flattened.elements {
                                    result.push(inner_elem);
                                }
                                continue;
                            }
                        }
                        result.push(*elem);
                    }
                    result
                }

                let result = flatten(&self.gc, arr, depth);
                let gc_ptr = self.gc.lock().allocate(result);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            array::SORT => {
                // sort(compareFn?): sort array
                let callback_val = if arg_count >= 1 { Some(stack.pop()?) } else { None };
                let array_val = stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };

                if let Some(cb) = callback_val {
                    // Sort with custom comparator
                    if !cb.is_ptr() {
                        return Err(VmError::TypeError("Expected callback function".to_string()));
                    }
                    let closure_ptr = unsafe { cb.as_ptr::<Closure>() };
                    let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                    let func_index = closure.func_id();

                    // We need to implement a sort that calls the callback
                    // For now, just use a simple bubble sort
                    task.push_closure(cb);
                    let n = arr.len();
                    for i in 0..n {
                        for j in 0..n - i - 1 {
                            let a = arr.elements[j];
                            let b = arr.elements[j + 1];
                            let args = vec![a, b];
                            let result = self.execute_nested_function(task, func_index, args, module)?;
                            let cmp = result.as_i32().unwrap_or(0);
                            if cmp > 0 {
                                arr.elements.swap(j, j + 1);
                            }
                        }
                    }
                    task.pop_closure();
                } else {
                    // Default sort (numeric/string comparison)
                    arr.elements.sort_by(|a, b| {
                        if let (Some(ai), Some(bi)) = (a.as_i32(), b.as_i32()) {
                            ai.cmp(&bi)
                        } else if let (Some(af), Some(bf)) = (a.as_f64(), b.as_f64()) {
                            af.partial_cmp(&bf).unwrap_or(std::cmp::Ordering::Equal)
                        } else {
                            std::cmp::Ordering::Equal
                        }
                    });
                }

                stack.push(array_val)?;
                Ok(())
            }
            array::REDUCE => {
                // reduce(callback, initialValue?): reduce to single value
                if arg_count < 1 || arg_count > 2 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.reduce expects 1-2 arguments, got {}", arg_count
                    )));
                }

                let initial_value = if arg_count >= 2 { Some(stack.pop()?) } else { None };
                let callback_val = stack.pop()?;
                let array_val = stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                if !callback_val.is_ptr() {
                    return Err(VmError::TypeError("Expected callback function".to_string()));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                let closure_ptr = unsafe { callback_val.as_ptr::<Closure>() };
                let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                let func_index = closure.func_id();

                let (start_idx, mut accumulator) = if let Some(init) = initial_value {
                    (0, init)
                } else if !arr.elements.is_empty() {
                    (1, arr.elements[0])
                } else {
                    return Err(VmError::RuntimeError("Reduce of empty array with no initial value".to_string()));
                };

                task.push_closure(callback_val);
                for i in start_idx..arr.len() {
                    let elem = arr.elements[i];
                    let args = vec![accumulator, elem];
                    accumulator = self.execute_nested_function(task, func_index, args, module)?;
                }
                task.pop_closure();

                stack.push(accumulator)?;
                Ok(())
            }
            array::JOIN => {
                // join(separator?) - arg_count is 0 or 1
                let sep = if arg_count >= 1 {
                    let sep_val = stack.pop()?;
                    if let Some(ptr) = unsafe { sep_val.as_ptr::<RayaString>() } {
                        let s = unsafe { &*ptr.as_ptr() };
                        s.data.clone()
                    } else {
                        ",".to_string()
                    }
                } else {
                    ",".to_string()
                };
                let array_val = stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };

                // Convert elements to strings and join
                let parts: Vec<String> = arr.elements.iter().map(|v| {
                    if let Some(ptr) = unsafe { v.as_ptr::<RayaString>() } {
                        unsafe { &*ptr.as_ptr() }.data.clone()
                    } else if let Some(i) = v.as_i32() {
                        i.to_string()
                    } else if let Some(f) = v.as_f64() {
                        f.to_string()
                    } else if v.is_null() {
                        String::new()
                    } else if let Some(b) = v.as_bool() {
                        b.to_string()
                    } else {
                        String::new()
                    }
                }).collect();
                let result = parts.join(&sep);
                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            array::FILTER => {
                // filter(callback): array method with callback
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.filter expects 1 argument, got {}", arg_count
                    )));
                }
                let callback_val = stack.pop()?;
                let array_val = stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                if !callback_val.is_ptr() {
                    return Err(VmError::TypeError("Expected callback function".to_string()));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                let closure_ptr = unsafe { callback_val.as_ptr::<Closure>() };
                let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                let func_index = closure.func_id();

                // Filter elements
                let mut result_arr = Array::new(0, 0);
                task.push_closure(callback_val);
                for elem in arr.elements.iter() {
                    let args = vec![*elem];
                    let result = self.execute_nested_function(task, func_index, args, module)?;
                    if result.is_truthy() {
                        result_arr.push(*elem);
                    }
                }
                task.pop_closure();

                let gc_ptr = self.gc.lock().allocate(result_arr);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            array::MAP => {
                // map(callback): transform each element
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.map expects 1 argument, got {}", arg_count
                    )));
                }
                let callback_val = stack.pop()?;
                let array_val = stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                if !callback_val.is_ptr() {
                    return Err(VmError::TypeError("Expected callback function".to_string()));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                let closure_ptr = unsafe { callback_val.as_ptr::<Closure>() };
                let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                let func_index = closure.func_id();

                let mut result_arr = Array::new(0, 0);
                task.push_closure(callback_val);
                for elem in arr.elements.iter() {
                    let args = vec![*elem];
                    let result = self.execute_nested_function(task, func_index, args, module)?;
                    result_arr.push(result);
                }
                task.pop_closure();

                let gc_ptr = self.gc.lock().allocate(result_arr);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            array::FIND => {
                // find(callback): find first element matching predicate
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.find expects 1 argument, got {}", arg_count
                    )));
                }
                let callback_val = stack.pop()?;
                let array_val = stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                if !callback_val.is_ptr() {
                    return Err(VmError::TypeError("Expected callback function".to_string()));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                let closure_ptr = unsafe { callback_val.as_ptr::<Closure>() };
                let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                let func_index = closure.func_id();

                task.push_closure(callback_val);
                let mut found = Value::null();
                for elem in arr.elements.iter() {
                    let args = vec![*elem];
                    let result = self.execute_nested_function(task, func_index, args, module)?;
                    if result.is_truthy() {
                        found = *elem;
                        break;
                    }
                }
                task.pop_closure();
                stack.push(found)?;
                Ok(())
            }
            array::FIND_INDEX => {
                // findIndex(callback): find index of first element matching predicate
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.findIndex expects 1 argument, got {}", arg_count
                    )));
                }
                let callback_val = stack.pop()?;
                let array_val = stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                if !callback_val.is_ptr() {
                    return Err(VmError::TypeError("Expected callback function".to_string()));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                let closure_ptr = unsafe { callback_val.as_ptr::<Closure>() };
                let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                let func_index = closure.func_id();

                task.push_closure(callback_val);
                let mut found_index: i32 = -1;
                for (i, elem) in arr.elements.iter().enumerate() {
                    let args = vec![*elem];
                    let result = self.execute_nested_function(task, func_index, args, module)?;
                    if result.is_truthy() {
                        found_index = i as i32;
                        break;
                    }
                }
                task.pop_closure();
                stack.push(Value::i32(found_index))?;
                Ok(())
            }
            array::FOR_EACH => {
                // forEach(callback): execute callback for each element
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.forEach expects 1 argument, got {}", arg_count
                    )));
                }
                let callback_val = stack.pop()?;
                let array_val = stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                if !callback_val.is_ptr() {
                    return Err(VmError::TypeError("Expected callback function".to_string()));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                let closure_ptr = unsafe { callback_val.as_ptr::<Closure>() };
                let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                let func_index = closure.func_id();

                task.push_closure(callback_val);
                for elem in arr.elements.iter() {
                    let args = vec![*elem];
                    let _ = self.execute_nested_function(task, func_index, args, module)?;
                }
                task.pop_closure();
                stack.push(Value::null())?;
                Ok(())
            }
            array::EVERY => {
                // every(callback): check if all elements match predicate
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.every expects 1 argument, got {}", arg_count
                    )));
                }
                let callback_val = stack.pop()?;
                let array_val = stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                if !callback_val.is_ptr() {
                    return Err(VmError::TypeError("Expected callback function".to_string()));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                let closure_ptr = unsafe { callback_val.as_ptr::<Closure>() };
                let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                let func_index = closure.func_id();

                task.push_closure(callback_val);
                let mut all_match = true;
                for elem in arr.elements.iter() {
                    let args = vec![*elem];
                    let result = self.execute_nested_function(task, func_index, args, module)?;
                    if !result.is_truthy() {
                        all_match = false;
                        break;
                    }
                }
                task.pop_closure();
                stack.push(Value::bool(all_match))?;
                Ok(())
            }
            array::SOME => {
                // some(callback): check if any element matches predicate
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.some expects 1 argument, got {}", arg_count
                    )));
                }
                let callback_val = stack.pop()?;
                let array_val = stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                if !callback_val.is_ptr() {
                    return Err(VmError::TypeError("Expected callback function".to_string()));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                let closure_ptr = unsafe { callback_val.as_ptr::<Closure>() };
                let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                let func_index = closure.func_id();

                task.push_closure(callback_val);
                let mut any_match = false;
                for elem in arr.elements.iter() {
                    let args = vec![*elem];
                    let result = self.execute_nested_function(task, func_index, args, module)?;
                    if result.is_truthy() {
                        any_match = true;
                        break;
                    }
                }
                task.pop_closure();
                stack.push(Value::bool(any_match))?;
                Ok(())
            }
            _ => Err(VmError::RuntimeError(format!(
                "Array method {:#06x} not yet implemented in TaskInterpreter",
                method_id
            ))),
        }
    }

    /// Handle built-in string methods
    fn call_string_method(
        &mut self,
        task: &Arc<Task>,
        stack: &mut std::sync::MutexGuard<'_, Stack>,
        method_id: u16,
        arg_count: usize,
        module: &Module,
    ) -> Result<(), VmError> {
        use crate::vm::builtin::string;

        // Pop arguments first (they're on top of the stack)
        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(stack.pop()?);
        }
        args.reverse(); // Now args[0] is the first argument

        // Pop the string (receiver)
        let string_val = stack.pop()?;
        if !string_val.is_ptr() {
            return Err(VmError::TypeError("Expected string".to_string()));
        }
        let str_ptr = unsafe { string_val.as_ptr::<RayaString>() };
        let raya_str = unsafe { &*str_ptr.unwrap().as_ptr() };
        let s = &raya_str.data;

        match method_id {
            string::CHAR_AT => {
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "String.charAt expects 1 argument, got {}", arg_count
                    )));
                }
                let index = args[0].as_i32().unwrap_or(0) as usize;
                let result = s.chars().nth(index).map(|c| c.to_string()).unwrap_or_default();
                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::TO_UPPER_CASE => {
                let result = s.to_uppercase();
                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::TO_LOWER_CASE => {
                let result = s.to_lowercase();
                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::TRIM => {
                let result = s.trim().to_string();
                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::INDEX_OF => {
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "String.indexOf expects 1 argument, got {}", arg_count
                    )));
                }
                let search_val = args[0];
                let search_str = if let Some(ptr) = unsafe { search_val.as_ptr::<RayaString>() } {
                    unsafe { &*ptr.as_ptr() }.data.clone()
                } else {
                    String::new()
                };
                let result = s.find(&search_str).map(|i| i as i32).unwrap_or(-1);
                stack.push(Value::i32(result))?;
                Ok(())
            }
            string::INCLUDES => {
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "String.includes expects 1 argument, got {}", arg_count
                    )));
                }
                let search_val = args[0];
                let search_str = if let Some(ptr) = unsafe { search_val.as_ptr::<RayaString>() } {
                    unsafe { &*ptr.as_ptr() }.data.clone()
                } else {
                    String::new()
                };
                let result = s.contains(&search_str);
                stack.push(Value::bool(result))?;
                Ok(())
            }
            string::STARTS_WITH => {
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "String.startsWith expects 1 argument, got {}", arg_count
                    )));
                }
                let prefix_val = args[0];
                let prefix_str = if let Some(ptr) = unsafe { prefix_val.as_ptr::<RayaString>() } {
                    unsafe { &*ptr.as_ptr() }.data.clone()
                } else {
                    String::new()
                };
                let result = s.starts_with(&prefix_str);
                stack.push(Value::bool(result))?;
                Ok(())
            }
            string::ENDS_WITH => {
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "String.endsWith expects 1 argument, got {}", arg_count
                    )));
                }
                let suffix_val = args[0];
                let suffix_str = if let Some(ptr) = unsafe { suffix_val.as_ptr::<RayaString>() } {
                    unsafe { &*ptr.as_ptr() }.data.clone()
                } else {
                    String::new()
                };
                let result = s.ends_with(&suffix_str);
                stack.push(Value::bool(result))?;
                Ok(())
            }
            string::SUBSTRING => {
                // substring(start, end?)
                let start_val = if arg_count >= 1 { args[0] } else { Value::i32(0) };
                let end_val = if arg_count >= 2 { Some(args[1]) } else { None };

                let start = start_val.as_i32().unwrap_or(0).max(0) as usize;
                let end = end_val.and_then(|v| v.as_i32()).map(|e| e.max(0) as usize).unwrap_or(s.len());

                let result: String = s.chars().skip(start).take(end.saturating_sub(start)).collect();
                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::SPLIT => {
                if arg_count < 1 || arg_count > 2 {
                    return Err(VmError::RuntimeError(format!(
                        "String.split expects 1-2 arguments, got {}", arg_count
                    )));
                }
                let sep_val = args[0];
                let sep_str = if let Some(ptr) = unsafe { sep_val.as_ptr::<RayaString>() } {
                    unsafe { &*ptr.as_ptr() }.data.clone()
                } else {
                    String::new()
                };

                // Get optional limit argument (try both i32 and i64)
                // In Raya, limit 0 means "no limit"
                let limit = if arg_count == 2 {
                    let raw_limit = args[1].as_i32()
                        .or_else(|| args[1].as_i64().map(|v| v as i32))
                        .unwrap_or(0);
                    if raw_limit > 0 { Some(raw_limit as usize) } else { None }
                } else {
                    None
                };

                // Split and optionally limit the parts
                let parts: Vec<_> = if sep_str.is_empty() {
                    let chars: Vec<_> = s.chars().map(|c| c.to_string()).collect();
                    if let Some(limit) = limit {
                        chars.into_iter().take(limit).collect()
                    } else {
                        chars
                    }
                } else {
                    let all_parts: Vec<_> = s.split(&sep_str).map(|p| p.to_string()).collect();
                    if let Some(limit) = limit {
                        all_parts.into_iter().take(limit).collect()
                    } else {
                        all_parts
                    }
                };

                let mut arr = Array::new(0, 0);
                for part in parts {
                    let raya_string = RayaString::new(part);
                    let gc_ptr = self.gc.lock().allocate(raya_string);
                    let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    arr.push(value);
                }

                let gc_ptr = self.gc.lock().allocate(arr);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::CHAR_CODE_AT => {
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "String.charCodeAt expects 1 argument, got {}", arg_count
                    )));
                }
                let index = args[0].as_i32().unwrap_or(0) as usize;
                let result = s.chars().nth(index).map(|c| c as i32).unwrap_or(-1);
                stack.push(Value::i32(result))?;
                Ok(())
            }
            string::LAST_INDEX_OF => {
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "String.lastIndexOf expects 1 argument, got {}", arg_count
                    )));
                }
                let search_val = args[0];
                let search_str = if let Some(ptr) = unsafe { search_val.as_ptr::<RayaString>() } {
                    unsafe { &*ptr.as_ptr() }.data.clone()
                } else {
                    String::new()
                };
                let result = s.rfind(&search_str).map(|i| i as i32).unwrap_or(-1);
                stack.push(Value::i32(result))?;
                Ok(())
            }
            string::PAD_START => {
                // padStart(targetLength, padString?)
                if arg_count < 1 {
                    return Err(VmError::RuntimeError(format!(
                        "String.padStart expects at least 1 argument, got {}", arg_count
                    )));
                }
                let target_length = args[0].as_i32().unwrap_or(0) as usize;
                let pad_str = if arg_count >= 2 {
                    if let Some(ptr) = unsafe { args[1].as_ptr::<RayaString>() } {
                        unsafe { &*ptr.as_ptr() }.data.clone()
                    } else {
                        " ".to_string()
                    }
                } else {
                    " ".to_string()
                };

                let result = if s.len() >= target_length {
                    s.clone()
                } else {
                    let pad_len = target_length - s.len();
                    let pad_repeated = pad_str.repeat((pad_len / pad_str.len().max(1)) + 1);
                    format!("{}{}", &pad_repeated[..pad_len], s)
                };

                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::PAD_END => {
                // padEnd(targetLength, padString?)
                if arg_count < 1 {
                    return Err(VmError::RuntimeError(format!(
                        "String.padEnd expects at least 1 argument, got {}", arg_count
                    )));
                }
                let target_length = args[0].as_i32().unwrap_or(0) as usize;
                let pad_str = if arg_count >= 2 {
                    if let Some(ptr) = unsafe { args[1].as_ptr::<RayaString>() } {
                        unsafe { &*ptr.as_ptr() }.data.clone()
                    } else {
                        " ".to_string()
                    }
                } else {
                    " ".to_string()
                };

                let result = if s.len() >= target_length {
                    s.clone()
                } else {
                    let pad_len = target_length - s.len();
                    let pad_repeated = pad_str.repeat((pad_len / pad_str.len().max(1)) + 1);
                    format!("{}{}", s, &pad_repeated[..pad_len])
                };

                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::TRIM_START => {
                let result = s.trim_start().to_string();
                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::TRIM_END => {
                let result = s.trim_end().to_string();
                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::MATCH => {
                // match(regexp): returns array of matches or null
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "String.match expects 1 argument, got {}", arg_count
                    )));
                }
                let regexp_val = args[0];
                let handle = regexp_val.as_u64().ok_or_else(|| {
                    VmError::TypeError("Expected RegExp argument".to_string())
                })?;
                let re_ptr = handle as *const RegExpObject;
                if re_ptr.is_null() {
                    return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
                }
                let re = unsafe { &*re_ptr };

                // Check if global flag is set
                let is_global = re.flags.contains('g');

                if is_global {
                    // Return all matches
                    let matches: Vec<_> = re.compiled.find_iter(s).map(|m| m.as_str().to_string()).collect();
                    if matches.is_empty() {
                        stack.push(Value::null())?;
                    } else {
                        let mut arr = Array::new(0, 0);
                        for m in matches {
                            let raya_string = RayaString::new(m);
                            let gc_ptr = self.gc.lock().allocate(raya_string);
                            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                            arr.push(value);
                        }
                        let gc_ptr = self.gc.lock().allocate(arr);
                        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        stack.push(value)?;
                    }
                } else {
                    // Return first match only
                    if let Some(m) = re.compiled.find(s) {
                        let mut arr = Array::new(0, 0);
                        let raya_string = RayaString::new(m.as_str().to_string());
                        let gc_ptr = self.gc.lock().allocate(raya_string);
                        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        arr.push(value);
                        let gc_ptr = self.gc.lock().allocate(arr);
                        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        stack.push(value)?;
                    } else {
                        stack.push(Value::null())?;
                    }
                }
                Ok(())
            }
            string::MATCH_ALL => {
                // matchAll(regexp): returns array of [match, index] arrays
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "String.matchAll expects 1 argument, got {}", arg_count
                    )));
                }
                let regexp_val = args[0];
                let handle = regexp_val.as_u64().ok_or_else(|| {
                    VmError::TypeError("Expected RegExp argument".to_string())
                })?;
                let re_ptr = handle as *const RegExpObject;
                if re_ptr.is_null() {
                    return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
                }
                let re = unsafe { &*re_ptr };

                // Return all matches as array of [match, index] arrays
                let mut result_arr = Array::new(0, 0);
                for m in re.compiled.find_iter(s) {
                    // Create inner array [match_string, index]
                    let mut match_arr = Array::new(0, 0);

                    // Add match string
                    let raya_string = RayaString::new(m.as_str().to_string());
                    let gc_ptr = self.gc.lock().allocate(raya_string);
                    let match_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    match_arr.push(match_val);

                    // Add index
                    match_arr.push(Value::i32(m.start() as i32));

                    let inner_gc_ptr = self.gc.lock().allocate(match_arr);
                    let inner_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(inner_gc_ptr.as_ptr()).unwrap()) };
                    result_arr.push(inner_val);
                }
                let gc_ptr = self.gc.lock().allocate(result_arr);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::SEARCH => {
                // search(regexp): returns index of first match or -1
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "String.search expects 1 argument, got {}", arg_count
                    )));
                }
                let regexp_val = args[0];
                let handle = regexp_val.as_u64().ok_or_else(|| {
                    VmError::TypeError("Expected RegExp argument".to_string())
                })?;
                let re_ptr = handle as *const RegExpObject;
                if re_ptr.is_null() {
                    return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
                }
                let re = unsafe { &*re_ptr };

                let result = re.compiled.find(s).map(|m| m.start() as i32).unwrap_or(-1);
                stack.push(Value::i32(result))?;
                Ok(())
            }
            string::REPLACE_REGEXP => {
                // replace(regexp, replacement): replace matches with string
                if arg_count != 2 {
                    return Err(VmError::RuntimeError(format!(
                        "String.replace expects 2 arguments, got {}", arg_count
                    )));
                }
                let regexp_val = args[0];
                let handle = regexp_val.as_u64().ok_or_else(|| {
                    VmError::TypeError("Expected RegExp argument".to_string())
                })?;
                let re_ptr = handle as *const RegExpObject;
                if re_ptr.is_null() {
                    return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
                }
                let re = unsafe { &*re_ptr };

                let replacement = if let Some(ptr) = unsafe { args[1].as_ptr::<RayaString>() } {
                    unsafe { &*ptr.as_ptr() }.data.clone()
                } else {
                    String::new()
                };

                let is_global = re.flags.contains('g');
                let result = if is_global {
                    re.compiled.replace_all(s, replacement.as_str()).to_string()
                } else {
                    re.compiled.replace(s, replacement.as_str()).to_string()
                };

                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::SPLIT_REGEXP => {
                // split(regexp, limit?): split string by regexp
                if arg_count < 1 || arg_count > 2 {
                    return Err(VmError::RuntimeError(format!(
                        "String.split expects 1-2 arguments, got {}", arg_count
                    )));
                }
                let regexp_val = args[0];
                let handle = regexp_val.as_u64().ok_or_else(|| {
                    VmError::TypeError("Expected RegExp argument".to_string())
                })?;
                let re_ptr = handle as *const RegExpObject;
                if re_ptr.is_null() {
                    return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
                }
                let re = unsafe { &*re_ptr };

                // Get optional limit argument (try both i32 and i64)
                // In Raya, limit 0 means "no limit"
                let limit = if arg_count == 2 {
                    let raw_limit = args[1].as_i32()
                        .or_else(|| args[1].as_i64().map(|v| v as i32))
                        .unwrap_or(0);
                    if raw_limit > 0 { Some(raw_limit as usize) } else { None }
                } else {
                    None
                };

                // Split and optionally limit the parts
                let all_parts: Vec<_> = re.compiled.split(s).map(|p| p.to_string()).collect();
                let parts: Vec<_> = if let Some(limit) = limit {
                    all_parts.into_iter().take(limit).collect()
                } else {
                    all_parts
                };

                let mut arr = Array::new(0, 0);
                for part in parts {
                    let raya_string = RayaString::new(part);
                    let gc_ptr = self.gc.lock().allocate(raya_string);
                    let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    arr.push(value);
                }

                let gc_ptr = self.gc.lock().allocate(arr);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::REPLACE_WITH_REGEXP => {
                // replaceWith(regexp, callback): replace using callback function
                if arg_count != 2 {
                    return Err(VmError::RuntimeError(format!(
                        "String.replaceWith expects 2 arguments, got {}", arg_count
                    )));
                }
                let regexp_val = args[0];
                let handle = regexp_val.as_u64().ok_or_else(|| {
                    VmError::TypeError("Expected RegExp argument".to_string())
                })?;
                let re_ptr = handle as *const RegExpObject;
                if re_ptr.is_null() {
                    return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
                }
                let re = unsafe { &*re_ptr };

                // Get callback function
                let callback_val = args[1];
                if !callback_val.is_ptr() {
                    return Err(VmError::TypeError("Expected callback function".to_string()));
                }
                let closure_ptr = unsafe { callback_val.as_ptr::<Closure>() };
                let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                let func_index = closure.func_id();

                let is_global = re.flags.contains('g');

                // Build result string by replacing matches
                let mut result = String::new();
                let mut last_end = 0;

                task.push_closure(callback_val);

                if is_global {
                    // Replace all matches
                    for m in re.compiled.find_iter(s) {
                        // Add text before this match
                        result.push_str(&s[last_end..m.start()]);

                        // Create match array argument
                        let mut match_arr = Array::new(0, 0);
                        let match_str = RayaString::new(m.as_str().to_string());
                        let gc_ptr = self.gc.lock().allocate(match_str);
                        let match_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        match_arr.push(match_val);
                        let arr_gc_ptr = self.gc.lock().allocate(match_arr);
                        let arr_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc_ptr.as_ptr()).unwrap()) };

                        // Call callback with match array
                        let callback_result = self.execute_nested_function(task, func_index, vec![arr_val], module)?;

                        // Get replacement string from callback result
                        let replacement = if let Some(ptr) = unsafe { callback_result.as_ptr::<RayaString>() } {
                            unsafe { &*ptr.as_ptr() }.data.clone()
                        } else {
                            String::new()
                        };
                        result.push_str(&replacement);
                        last_end = m.end();
                    }
                } else {
                    // Replace first match only
                    if let Some(m) = re.compiled.find(s) {
                        result.push_str(&s[..m.start()]);

                        // Create match array argument
                        let mut match_arr = Array::new(0, 0);
                        let match_str = RayaString::new(m.as_str().to_string());
                        let gc_ptr = self.gc.lock().allocate(match_str);
                        let match_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        match_arr.push(match_val);
                        let arr_gc_ptr = self.gc.lock().allocate(match_arr);
                        let arr_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc_ptr.as_ptr()).unwrap()) };

                        // Call callback
                        let callback_result = self.execute_nested_function(task, func_index, vec![arr_val], module)?;

                        let replacement = if let Some(ptr) = unsafe { callback_result.as_ptr::<RayaString>() } {
                            unsafe { &*ptr.as_ptr() }.data.clone()
                        } else {
                            String::new()
                        };
                        result.push_str(&replacement);
                        last_end = m.end();
                    }
                }

                task.pop_closure();

                // Add remaining text after last match
                result.push_str(&s[last_end..]);

                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            _ => Err(VmError::RuntimeError(format!(
                "String method {:#06x} not yet implemented in TaskInterpreter",
                method_id
            ))),
        }
    }

    /// Handle built-in regexp methods
    fn call_regexp_method(
        &mut self,
        _task: &Arc<Task>,
        stack: &mut std::sync::MutexGuard<'_, Stack>,
        method_id: u16,
        arg_count: usize,
    ) -> Result<(), VmError> {
        use crate::vm::builtin::regexp;

        // Pop arguments (excluding receiver)
        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(stack.pop()?);
        }
        args.reverse();

        // Pop receiver (the RegExp handle)
        let receiver = stack.pop()?;
        let handle = receiver.as_u64().ok_or_else(|| {
            VmError::TypeError("Expected RegExp handle".to_string())
        })?;
        let re_ptr = handle as *const RegExpObject;
        if re_ptr.is_null() {
            return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
        }
        let re = unsafe { &*re_ptr };

        match method_id {
            id if id == regexp::TEST => {
                let input = if !args.is_empty() && args[0].is_ptr() {
                    if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                        unsafe { &*s.as_ptr() }.data.clone()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                stack.push(Value::bool(re.test(&input)))?;
            }
            id if id == regexp::EXEC => {
                let input = if !args.is_empty() && args[0].is_ptr() {
                    if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                        unsafe { &*s.as_ptr() }.data.clone()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                match re.exec(&input) {
                    Some((matched, index, groups)) => {
                        let mut arr = Array::new(0, 0);
                        let matched_str = RayaString::new(matched);
                        let gc_ptr = self.gc.lock().allocate(matched_str);
                        let matched_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        arr.push(matched_val);
                        arr.push(Value::i32(index as i32));
                        for group in groups {
                            let group_str = RayaString::new(group);
                            let gc_ptr = self.gc.lock().allocate(group_str);
                            let group_val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            };
                            arr.push(group_val);
                        }
                        let arr_gc = self.gc.lock().allocate(arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        stack.push(arr_val)?;
                    }
                    None => {
                        stack.push(Value::null())?;
                    }
                }
            }
            id if id == regexp::EXEC_ALL => {
                let input = if !args.is_empty() && args[0].is_ptr() {
                    if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                        unsafe { &*s.as_ptr() }.data.clone()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                let matches = re.exec_all(&input);
                let mut result_arr = Array::new(0, 0);
                for (matched, index, groups) in matches {
                    let mut match_arr = Array::new(0, 0);
                    let matched_str = RayaString::new(matched);
                    let gc_ptr = self.gc.lock().allocate(matched_str);
                    let matched_val = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                    };
                    match_arr.push(matched_val);
                    match_arr.push(Value::i32(index as i32));
                    for group in groups {
                        let group_str = RayaString::new(group);
                        let gc_ptr = self.gc.lock().allocate(group_str);
                        let group_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        match_arr.push(group_val);
                    }
                    let match_arr_gc = self.gc.lock().allocate(match_arr);
                    let match_arr_val = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(match_arr_gc.as_ptr()).unwrap())
                    };
                    result_arr.push(match_arr_val);
                }
                let arr_gc = self.gc.lock().allocate(result_arr);
                let arr_val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                };
                stack.push(arr_val)?;
            }
            id if id == regexp::REPLACE => {
                let input = if !args.is_empty() && args[0].is_ptr() {
                    if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                        unsafe { &*s.as_ptr() }.data.clone()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                let replacement = if args.len() > 1 && args[1].is_ptr() {
                    if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                        unsafe { &*s.as_ptr() }.data.clone()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                let result = re.replace(&input, &replacement);
                let result_str = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(result_str);
                let result_val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };
                stack.push(result_val)?;
            }
            id if id == regexp::SPLIT => {
                let input = if !args.is_empty() && args[0].is_ptr() {
                    if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                        unsafe { &*s.as_ptr() }.data.clone()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                // In Raya, limit 0 means "no limit"
                let limit = if args.len() > 1 {
                    let raw_limit = args[1].as_i32().unwrap_or(0);
                    if raw_limit > 0 { Some(raw_limit as usize) } else { None }
                } else {
                    None
                };
                let parts = re.split(&input, limit);
                let mut arr = Array::new(0, 0);
                for part in parts {
                    let s = RayaString::new(part);
                    let gc_ptr = self.gc.lock().allocate(s);
                    let val = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                    };
                    arr.push(val);
                }
                let arr_gc = self.gc.lock().allocate(arr);
                let arr_val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                };
                stack.push(arr_val)?;
            }
            _ => {
                return Err(VmError::RuntimeError(format!(
                    "RegExp method {:#06x} not yet implemented in TaskInterpreter",
                    method_id
                )));
            }
        }
        Ok(())
    }
}
