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
use crate::vm::builtins::handlers::{
    ArrayHandlerContext, RegExpHandlerContext, ReflectHandlerContext, StringHandlerContext,
    RuntimeHandlerContext,
    call_array_method as array_handler, call_regexp_method as regexp_handler,
    call_reflect_method as reflect_handler, call_string_method as string_handler,
    call_runtime_method as runtime_handler,
};
use crate::vm::object::{Array, Buffer, ChannelObject, Closure, DateObject, MapObject, Object, RayaString, RegExpObject, SetObject};
use crate::vm::reflect::{ObjectDiff, ObjectSnapshot, SnapshotContext, SnapshotValue};
use crate::vm::scheduler::{ExceptionHandler, SuspendReason, Task, TaskId, TaskState};
use crate::vm::stack::Stack;
use crate::vm::sync::{MutexId, MutexRegistry};
use crate::vm::native_handler::NativeHandler;
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
pub struct Interpreter<'a> {
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

    /// Metadata store for Reflect API
    metadata: &'a parking_lot::Mutex<crate::vm::reflect::MetadataStore>,

    /// Class metadata registry for reflection (field/method names)
    class_metadata: &'a RwLock<crate::vm::reflect::ClassMetadataRegistry>,

    /// External native call handler (stdlib implementation)
    native_handler: &'a Arc<dyn NativeHandler>,
}

impl<'a> Interpreter<'a> {
    /// Create a new task interpreter
    pub fn new(
        gc: &'a parking_lot::Mutex<GarbageCollector>,
        classes: &'a RwLock<ClassRegistry>,
        mutex_registry: &'a MutexRegistry,
        safepoint: &'a SafepointCoordinator,
        globals_by_index: &'a RwLock<Vec<Value>>,
        tasks: &'a Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,
        injector: &'a Arc<Injector<Arc<Task>>>,
        metadata: &'a parking_lot::Mutex<crate::vm::reflect::MetadataStore>,
        class_metadata: &'a RwLock<crate::vm::reflect::ClassMetadataRegistry>,
        native_handler: &'a Arc<dyn NativeHandler>,
    ) -> Self {
        Self {
            gc,
            classes,
            mutex_registry,
            safepoint,
            globals_by_index,
            tasks,
            injector,
            metadata,
            class_metadata,
            native_handler,
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

                // Check if the object is a proxy - if so, unwrap to target
                // TODO: Full trap support would call handler.get(target, fieldName)
                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
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

                // Check if the object is a proxy - if so, unwrap to target
                // TODO: Full trap support would call handler.set(target, fieldName, value)
                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
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

                // Check if the object is a proxy - if so, unwrap to target
                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
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

                // Check if the object is a proxy - if so, unwrap to target
                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
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

                // Check if the object is a proxy - if so, unwrap to target
                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
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

            Opcode::ArrayPush => {
                // Stack: [value, array] -> [] (mutates array in-place)
                let element = match stack.pop() {
                    Ok(v) => v,
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
                arr.push(element);
                OpcodeResult::Continue
            }

            Opcode::ArrayPop => {
                // Stack: [array] -> [popped_element]
                let arr_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !arr_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected array".to_string()));
                }

                let arr_ptr = unsafe { arr_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                let value = arr.pop().unwrap_or(Value::null());
                if let Err(e) = stack.push(value) {
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
                    // Number native calls
                    id if id == 0x0F00u16 => {
                        // NUMBER_TO_FIXED: format number with fixed decimal places
                        // args[0] = number value, args[1] = digits
                        let value = args[0].as_f64()
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0);
                        let digits = args.get(1).and_then(|v| v.as_i32()).unwrap_or(0) as usize;
                        let formatted = format!("{:.prec$}", value, prec = digits);
                        let s = RayaString::new(formatted);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        if let Err(e) = stack.push(val) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == 0x0F01u16 => {
                        // NUMBER_TO_PRECISION: format with N significant digits
                        let value = args[0].as_f64()
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0);
                        let prec = args.get(1).and_then(|v| v.as_i32()).unwrap_or(1).max(1) as usize;
                        let formatted = if value == 0.0 {
                            format!("{:.prec$}", 0.0, prec = prec - 1)
                        } else {
                            let magnitude = value.abs().log10().floor() as i32;
                            let decimal_places = if prec as i32 > magnitude + 1 {
                                (prec as i32 - magnitude - 1) as usize
                            } else {
                                0
                            };
                            format!("{:.prec$}", value, prec = decimal_places)
                        };
                        let s = RayaString::new(formatted);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        if let Err(e) = stack.push(val) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == 0x0F02u16 => {
                        // NUMBER_TO_STRING_RADIX: convert to string with radix
                        let value = args[0].as_f64()
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0);
                        let radix = args.get(1).and_then(|v| v.as_i32()).unwrap_or(10);
                        let formatted = if radix == 10 || radix < 2 || radix > 36 {
                            if value.fract() == 0.0 && value.abs() < i64::MAX as f64 {
                                format!("{}", value as i64)
                            } else {
                                format!("{}", value)
                            }
                        } else {
                            // Integer radix conversion
                            let int_val = value as i64;
                            match radix {
                                2 => format!("{:b}", int_val),
                                8 => format!("{:o}", int_val),
                                16 => format!("{:x}", int_val),
                                _ => {
                                    // General radix conversion
                                    if int_val == 0 { "0".to_string() }
                                    else {
                                        let negative = int_val < 0;
                                        let mut n = int_val.unsigned_abs();
                                        let mut digits = Vec::new();
                                        let radix = radix as u64;
                                        while n > 0 {
                                            let d = (n % radix) as u8;
                                            digits.push(if d < 10 { b'0' + d } else { b'a' + d - 10 });
                                            n /= radix;
                                        }
                                        digits.reverse();
                                        let s = String::from_utf8(digits).unwrap_or_default();
                                        if negative { format!("-{}", s) } else { s }
                                    }
                                }
                            }
                        };
                        let s = RayaString::new(formatted);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        if let Err(e) = stack.push(val) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    // Object native calls
                    id if id == 0x0001u16 => {
                        // OBJECT_TO_STRING: return "[object Object]"
                        let s = RayaString::new("[object Object]".to_string());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        if let Err(e) = stack.push(value) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == 0x0002u16 => {
                        // OBJECT_HASH_CODE: return identity hash from object pointer
                        let hash = if !args.is_empty() {
                            // Use the raw bits of the value as a hash
                            let bits = args[0].as_u64().unwrap_or(0);
                            (bits ^ (bits >> 16)) as i32
                        } else {
                            0
                        };
                        if let Err(e) = stack.push(Value::i32(hash)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == 0x0003u16 => {
                        // OBJECT_EQUAL: reference equality
                        let equal = if args.len() >= 2 {
                            args[0].as_u64() == args[1].as_u64()
                        } else {
                            false
                        };
                        if let Err(e) = stack.push(Value::bool(equal)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    // Task native calls
                    id if id == 0x0500u16 => {
                        // TASK_IS_DONE: check if task completed
                        let task_id = TaskId::from_u64(args[0].as_u64().unwrap_or(0));
                        let tasks = self.tasks.read();
                        let is_done = tasks.get(&task_id)
                            .map(|t| matches!(t.state(), TaskState::Completed | TaskState::Failed))
                            .unwrap_or(true);
                        if let Err(e) = stack.push(Value::bool(is_done)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == 0x0501u16 => {
                        // TASK_IS_CANCELLED: check if task cancelled
                        let task_id = TaskId::from_u64(args[0].as_u64().unwrap_or(0));
                        let tasks = self.tasks.read();
                        let is_cancelled = tasks.get(&task_id)
                            .map(|t| t.is_cancelled())
                            .unwrap_or(false);
                        if let Err(e) = stack.push(Value::bool(is_cancelled)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    // Error native calls
                    id if id == 0x0600u16 => {
                        // ERROR_STACK: return stack trace string
                        // The stack trace is captured at throw time and stored in the error object
                        // For now, return an empty string as placeholder
                        let s = RayaString::new(String::new());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        if let Err(e) = stack.push(value) { return OpcodeResult::Error(e); }
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
                    id if id == date::GET_HOURS as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_hours())) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_MINUTES as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_minutes())) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_SECONDS as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_seconds())) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_MILLISECONDS as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_milliseconds())) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    // Date setters: args[0]=timestamp, args[1]=new value, returns new timestamp as f64
                    id if id == date::SET_FULL_YEAR as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(0);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_full_year(val) as f64)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_MONTH as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(0);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_month(val) as f64)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_DATE as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(1);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_date(val) as f64)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_HOURS as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(0);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_hours(val) as f64)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_MINUTES as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(0);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_minutes(val) as f64)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_SECONDS as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(0);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_seconds(val) as f64)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_MILLISECONDS as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(0);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_milliseconds(val) as f64)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    // Date string formatting: args[0]=timestamp, returns string
                    id if id == date::TO_STRING as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        let s = RayaString::new(date.to_string_repr());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        if let Err(e) = stack.push(value) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::TO_ISO_STRING as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        let s = RayaString::new(date.to_iso_string());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        if let Err(e) = stack.push(value) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::TO_DATE_STRING as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        let s = RayaString::new(date.to_date_string());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        if let Err(e) = stack.push(value) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::TO_TIME_STRING as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        let s = RayaString::new(date.to_time_string());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        if let Err(e) = stack.push(value) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    // Date.parse: args[0]=string, returns timestamp f64 (NaN on failure)
                    id if id == date::PARSE as u16 => {
                        let input = if !args.is_empty() && args[0].is_ptr() {
                            if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else { String::new() }
                        } else { String::new() };
                        let result = match DateObject::parse(&input) {
                            Some(ts) => Value::f64(ts as f64),
                            None => Value::f64(f64::NAN),
                        };
                        if let Err(e) = stack.push(result) { return OpcodeResult::Error(e); }
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

                    // Logger/Math/Crypto/Path/Codec native calls — dispatches to native handler
                    id if crate::vm::builtin::is_logger_method(id)
                        || crate::vm::builtin::is_math_method(id)
                        || crate::vm::builtin::is_crypto_method(id)
                        || crate::vm::builtin::is_path_method(id)
                        || crate::vm::builtin::is_codec_method(id) => {
                        use crate::vm::{NativeCallResult, NativeContext, NativeValue, Scheduler};
                        use std::sync::Arc;

                        // Create placeholder scheduler for NativeContext (task operations are stubbed)
                        let placeholder_scheduler = Arc::new(Scheduler::new(1));
                        let ctx = NativeContext::new(
                            self.gc,
                            self.classes,
                            &placeholder_scheduler,
                            task.id(),
                        );

                        // Convert arguments to NativeValue
                        let native_args: Vec<NativeValue> = args.iter()
                            .map(|v| NativeValue::from_value(*v))
                            .collect();

                        match self.native_handler.call(&ctx, id, &native_args) {
                            NativeCallResult::Value(val) => {
                                if let Err(e) = stack.push(val.into_value()) {
                                    return OpcodeResult::Error(e);
                                }
                                OpcodeResult::Continue
                            }
                            NativeCallResult::Unhandled => {
                                return OpcodeResult::Error(VmError::RuntimeError(format!(
                                    "Native call {:#06x} not handled by native handler",
                                    id
                                )));
                            }
                            NativeCallResult::Error(msg) => {
                                return OpcodeResult::Error(VmError::RuntimeError(msg));
                            }
                        }
                    }

                    _ => {
                        // Check if this is a reflect method - pass args directly (don't push/pop)
                        if crate::vm::builtin::is_reflect_method(native_id) {
                            match self.call_reflect_method(task, &mut **stack, native_id, args, module) {
                                Ok(()) => return OpcodeResult::Continue,
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        }

                        // Check if this is a runtime method (std:runtime)
                        if crate::vm::builtin::is_runtime_method(native_id) {
                            match self.call_runtime_method(task, &mut **stack, native_id, args, module) {
                                Ok(()) => return OpcodeResult::Continue,
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        }

                        // Check if this is a time method (std:time)
                        if crate::vm::builtin::is_time_method(native_id) {
                            match self.call_time_method(&mut **stack, native_id, args) {
                                Ok(()) => return OpcodeResult::Continue,
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        }

                        // Other native calls not yet implemented
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "NativeCall {:#06x} not yet implemented in Interpreter (args={})",
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
                    match self.call_regexp_method(task, stack, method_id, arg_count, module) {
                        Ok(()) => return OpcodeResult::Continue,
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }

                // Check for built-in number methods
                if crate::vm::builtin::is_number_method(method_id) {
                    // Pop arguments
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        match stack.pop() {
                            Ok(v) => args.push(v),
                            Err(e) => return OpcodeResult::Error(e),
                        }
                    }
                    args.reverse();
                    // Pop receiver (number value)
                    let receiver = match stack.pop() {
                        Ok(v) => v,
                        Err(e) => return OpcodeResult::Error(e),
                    };
                    // Prepend receiver as args[0] for NativeCall pattern
                    let mut native_args = Vec::with_capacity(args.len() + 1);
                    native_args.push(receiver);
                    native_args.extend(args);

                    // Dispatch based on method ID
                    let value = native_args[0].as_f64()
                        .or_else(|| native_args[0].as_i32().map(|v| v as f64))
                        .unwrap_or(0.0);

                    let result_str = match method_id {
                        0x0F00 => {
                            // toFixed(digits)
                            let digits = native_args.get(1).and_then(|v| v.as_i32()).unwrap_or(0) as usize;
                            format!("{:.prec$}", value, prec = digits)
                        }
                        0x0F01 => {
                            // toPrecision(prec)
                            let prec = native_args.get(1).and_then(|v| v.as_i32()).unwrap_or(1).max(1) as usize;
                            if value == 0.0 {
                                format!("{:.prec$}", 0.0, prec = prec - 1)
                            } else {
                                let magnitude = value.abs().log10().floor() as i32;
                                if prec as i32 <= magnitude + 1 {
                                    let shift = 10f64.powi(magnitude + 1 - prec as i32);
                                    let rounded = (value / shift).round() * shift;
                                    format!("{}", rounded as i64)
                                } else {
                                    let decimal_places = (prec as i32 - magnitude - 1) as usize;
                                    format!("{:.prec$}", value, prec = decimal_places)
                                }
                            }
                        }
                        0x0F02 => {
                            // toString(radix?)
                            let radix = native_args.get(1).and_then(|v| v.as_i32()).unwrap_or(10);
                            if radix == 10 || radix < 2 || radix > 36 {
                                if value.fract() == 0.0 && value.abs() < i64::MAX as f64 {
                                    format!("{}", value as i64)
                                } else {
                                    format!("{}", value)
                                }
                            } else {
                                let int_val = value as i64;
                                match radix {
                                    2 => format!("{:b}", int_val),
                                    8 => format!("{:o}", int_val),
                                    16 => format!("{:x}", int_val),
                                    _ => {
                                        if int_val == 0 { "0".to_string() }
                                        else {
                                            let negative = int_val < 0;
                                            let mut n = int_val.unsigned_abs();
                                            let mut digits = Vec::new();
                                            let r = radix as u64;
                                            while n > 0 {
                                                let d = (n % r) as u8;
                                                digits.push(if d < 10 { b'0' + d } else { b'a' + d - 10 });
                                                n /= r;
                                            }
                                            digits.reverse();
                                            let s = String::from_utf8(digits).unwrap_or_default();
                                            if negative { format!("-{}", s) } else { s }
                                        }
                                    }
                                }
                            }
                        }
                        _ => String::new(),
                    };

                    let s = RayaString::new(result_str);
                    let gc_ptr = self.gc.lock().allocate(s);
                    let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    if let Err(e) = stack.push(val) { return OpcodeResult::Error(e); }
                    return OpcodeResult::Continue;
                }

                // Check for built-in reflect methods
                if crate::vm::builtin::is_reflect_method(method_id) {
                    // Pop args from stack into a Vec for call_reflect_method
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        match stack.pop() {
                            Ok(v) => args.push(v),
                            Err(e) => return OpcodeResult::Error(e),
                        }
                    }
                    args.reverse();
                    match self.call_reflect_method(task, &mut **stack, method_id, args, module) {
                        Ok(()) => return OpcodeResult::Continue,
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }

                // Check for built-in runtime methods (std:runtime)
                if crate::vm::builtin::is_runtime_method(method_id) {
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        match stack.pop() {
                            Ok(v) => args.push(v),
                            Err(e) => return OpcodeResult::Error(e),
                        }
                    }
                    args.reverse();
                    match self.call_runtime_method(task, &mut **stack, method_id, args, module) {
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
                "Opcode {:?} not yet implemented in Interpreter",
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
    ///
    /// This method is accessible within the interpreter module for use by
    /// ExecutionContext implementations.
    pub(super) fn execute_nested_function(
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

                    // Check if the object is a proxy - if so, unwrap to target
                    // TODO: Full trap support would call handler.get(target, fieldName)
                    let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                    if !actual_obj.is_ptr() {
                        return Err(VmError::TypeError("Expected object".to_string()));
                    }
                    let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
                    let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                    let field_val = obj.get_field(field_offset).unwrap_or(Value::null());
                    call_stack.push(field_val)?;
                }
                Opcode::StoreField => {
                    let field_offset = Self::read_u16(code, &mut ip)? as usize;
                    let value = call_stack.pop()?;
                    let obj_val = call_stack.pop()?;

                    // Check if the object is a proxy - if so, unwrap to target
                    // TODO: Full trap support would call handler.set(target, fieldName, value)
                    let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                    if !actual_obj.is_ptr() {
                        return Err(VmError::TypeError("Expected object".to_string()));
                    }
                    let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
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
                        // Number native calls
                        id if id == 0x0F00u32 => {
                            // NUMBER_TO_FIXED: format number with fixed decimal places
                            let value = args[0].as_f64()
                                .or_else(|| args[0].as_i32().map(|v| v as f64))
                                .unwrap_or(0.0);
                            let digits = args.get(1).and_then(|v| v.as_i32()).unwrap_or(0) as usize;
                            let formatted = format!("{:.prec$}", value, prec = digits);
                            let s = RayaString::new(formatted);
                            let gc_ptr = self.gc.lock().allocate(s);
                            let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                            call_stack.push(val)?;
                        }
                        id if id == 0x0F01u32 => {
                            // NUMBER_TO_PRECISION: format with N significant digits
                            let value = args[0].as_f64()
                                .or_else(|| args[0].as_i32().map(|v| v as f64))
                                .unwrap_or(0.0);
                            let prec = args.get(1).and_then(|v| v.as_i32()).unwrap_or(1).max(1) as usize;
                            let formatted = if value == 0.0 {
                                format!("{:.prec$}", 0.0, prec = prec - 1)
                            } else {
                                let magnitude = value.abs().log10().floor() as i32;
                                if prec as i32 <= magnitude + 1 {
                                    let shift = 10f64.powi(magnitude + 1 - prec as i32);
                                    let rounded = (value / shift).round() * shift;
                                    format!("{}", rounded as i64)
                                } else {
                                    let decimal_places = (prec as i32 - magnitude - 1) as usize;
                                    format!("{:.prec$}", value, prec = decimal_places)
                                }
                            };
                            let s = RayaString::new(formatted);
                            let gc_ptr = self.gc.lock().allocate(s);
                            let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                            call_stack.push(val)?;
                        }
                        id if id == 0x0F02u32 => {
                            // NUMBER_TO_STRING_RADIX: convert to string with radix
                            let value = args[0].as_f64()
                                .or_else(|| args[0].as_i32().map(|v| v as f64))
                                .unwrap_or(0.0);
                            let radix = args.get(1).and_then(|v| v.as_i32()).unwrap_or(10);
                            let formatted = if radix == 10 || radix < 2 || radix > 36 {
                                if value.fract() == 0.0 && value.abs() < i64::MAX as f64 {
                                    format!("{}", value as i64)
                                } else {
                                    format!("{}", value)
                                }
                            } else {
                                let int_val = value as i64;
                                match radix {
                                    2 => format!("{:b}", int_val),
                                    8 => format!("{:o}", int_val),
                                    16 => format!("{:x}", int_val),
                                    _ => {
                                        if int_val == 0 { "0".to_string() }
                                        else {
                                            let negative = int_val < 0;
                                            let mut n = int_val.unsigned_abs();
                                            let mut digits = Vec::new();
                                            let radix = radix as u64;
                                            while n > 0 {
                                                let d = (n % radix) as u8;
                                                digits.push(if d < 10 { b'0' + d } else { b'a' + d - 10 });
                                                n /= radix;
                                            }
                                            digits.reverse();
                                            let s = String::from_utf8(digits).unwrap_or_default();
                                            if negative { format!("-{}", s) } else { s }
                                        }
                                    }
                                }
                            };
                            let s = RayaString::new(formatted);
                            let gc_ptr = self.gc.lock().allocate(s);
                            let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                            call_stack.push(val)?;
                        }
                        // Object native calls
                        id if id == 0x0001u32 => {
                            // OBJECT_TO_STRING: return "[object Object]"
                            let s = RayaString::new("[object Object]".to_string());
                            let gc_ptr = self.gc.lock().allocate(s);
                            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                            call_stack.push(value)?;
                        }
                        id if id == 0x0002u32 => {
                            // OBJECT_HASH_CODE: return identity hash from object pointer
                            let hash = if !args.is_empty() {
                                let bits = args[0].as_u64().unwrap_or(0);
                                (bits ^ (bits >> 16)) as i32
                            } else {
                                0
                            };
                            call_stack.push(Value::i32(hash))?;
                        }
                        id if id == 0x0003u32 => {
                            // OBJECT_EQUAL: reference equality
                            let equal = if args.len() >= 2 {
                                args[0].as_u64() == args[1].as_u64()
                            } else {
                                false
                            };
                            call_stack.push(Value::bool(equal))?;
                        }
                        // Task native calls
                        id if id == 0x0500u32 => {
                            // TASK_IS_DONE: check if task completed
                            let task_id = TaskId::from_u64(args[0].as_u64().unwrap_or(0));
                            let tasks = self.tasks.read();
                            let is_done = tasks.get(&task_id)
                                .map(|t| matches!(t.state(), TaskState::Completed | TaskState::Failed))
                                .unwrap_or(true);
                            call_stack.push(Value::bool(is_done))?;
                        }
                        id if id == 0x0501u32 => {
                            // TASK_IS_CANCELLED: check if task cancelled
                            let task_id = TaskId::from_u64(args[0].as_u64().unwrap_or(0));
                            let tasks = self.tasks.read();
                            let is_cancelled = tasks.get(&task_id)
                                .map(|t| t.is_cancelled())
                                .unwrap_or(false);
                            call_stack.push(Value::bool(is_cancelled))?;
                        }
                        // Error native calls
                        id if id == 0x0600u32 => {
                            // ERROR_STACK: return stack trace string
                            let s = RayaString::new(String::new());
                            let gc_ptr = self.gc.lock().allocate(s);
                            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                            call_stack.push(value)?;
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
                        id if id == date::GET_HOURS as u32 => {
                            let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                            call_stack.push(Value::i32(DateObject::from_timestamp(timestamp).get_hours()))?;
                        }
                        id if id == date::GET_MINUTES as u32 => {
                            let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                            call_stack.push(Value::i32(DateObject::from_timestamp(timestamp).get_minutes()))?;
                        }
                        id if id == date::GET_SECONDS as u32 => {
                            let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                            call_stack.push(Value::i32(DateObject::from_timestamp(timestamp).get_seconds()))?;
                        }
                        id if id == date::GET_MILLISECONDS as u32 => {
                            let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                            call_stack.push(Value::i32(DateObject::from_timestamp(timestamp).get_milliseconds()))?;
                        }
                        // Date setters
                        id if id == date::SET_FULL_YEAR as u32 => {
                            let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                            let val = args[1].as_i32().unwrap_or(0);
                            call_stack.push(Value::f64(DateObject::from_timestamp(timestamp).set_full_year(val) as f64))?;
                        }
                        id if id == date::SET_MONTH as u32 => {
                            let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                            let val = args[1].as_i32().unwrap_or(0);
                            call_stack.push(Value::f64(DateObject::from_timestamp(timestamp).set_month(val) as f64))?;
                        }
                        id if id == date::SET_DATE as u32 => {
                            let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                            let val = args[1].as_i32().unwrap_or(1);
                            call_stack.push(Value::f64(DateObject::from_timestamp(timestamp).set_date(val) as f64))?;
                        }
                        id if id == date::SET_HOURS as u32 => {
                            let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                            let val = args[1].as_i32().unwrap_or(0);
                            call_stack.push(Value::f64(DateObject::from_timestamp(timestamp).set_hours(val) as f64))?;
                        }
                        id if id == date::SET_MINUTES as u32 => {
                            let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                            let val = args[1].as_i32().unwrap_or(0);
                            call_stack.push(Value::f64(DateObject::from_timestamp(timestamp).set_minutes(val) as f64))?;
                        }
                        id if id == date::SET_SECONDS as u32 => {
                            let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                            let val = args[1].as_i32().unwrap_or(0);
                            call_stack.push(Value::f64(DateObject::from_timestamp(timestamp).set_seconds(val) as f64))?;
                        }
                        id if id == date::SET_MILLISECONDS as u32 => {
                            let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                            let val = args[1].as_i32().unwrap_or(0);
                            call_stack.push(Value::f64(DateObject::from_timestamp(timestamp).set_milliseconds(val) as f64))?;
                        }
                        // Date string formatting
                        id if id == date::TO_STRING as u32 => {
                            let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                            let s = RayaString::new(DateObject::from_timestamp(timestamp).to_string_repr());
                            let gc_ptr = self.gc.lock().allocate(s);
                            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                            call_stack.push(value)?;
                        }
                        id if id == date::TO_ISO_STRING as u32 => {
                            let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                            let s = RayaString::new(DateObject::from_timestamp(timestamp).to_iso_string());
                            let gc_ptr = self.gc.lock().allocate(s);
                            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                            call_stack.push(value)?;
                        }
                        id if id == date::TO_DATE_STRING as u32 => {
                            let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                            let s = RayaString::new(DateObject::from_timestamp(timestamp).to_date_string());
                            let gc_ptr = self.gc.lock().allocate(s);
                            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                            call_stack.push(value)?;
                        }
                        id if id == date::TO_TIME_STRING as u32 => {
                            let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                            let s = RayaString::new(DateObject::from_timestamp(timestamp).to_time_string());
                            let gc_ptr = self.gc.lock().allocate(s);
                            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                            call_stack.push(value)?;
                        }
                        // Date.parse
                        id if id == date::PARSE as u32 => {
                            let input = if !args.is_empty() && args[0].is_ptr() {
                                if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                                    unsafe { &*s.as_ptr() }.data.clone()
                                } else { String::new() }
                            } else { String::new() };
                            let result = match DateObject::parse(&input) {
                                Some(ts) => Value::f64(ts as f64),
                                None => Value::f64(f64::NAN),
                            };
                            call_stack.push(result)?;
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
                        // Logger/Math/Crypto/Path/Codec native calls — dispatches to native handler
                        id if crate::vm::builtin::is_logger_method(id as u16)
                            || crate::vm::builtin::is_math_method(id as u16)
                            || crate::vm::builtin::is_crypto_method(id as u16)
                            || crate::vm::builtin::is_path_method(id as u16)
                            || crate::vm::builtin::is_codec_method(id as u16) => {
                            use crate::vm::{NativeCallResult, NativeContext, NativeValue, Scheduler};
                            use std::sync::Arc;

                            // Create placeholder scheduler for NativeContext (task operations are stubbed)
                            let placeholder_scheduler = Arc::new(Scheduler::new(1));
                            let ctx = NativeContext::new(
                                self.gc,
                                self.classes,
                                &placeholder_scheduler,
                                task.id(),
                            );

                            // Convert arguments to NativeValue
                            let native_args: Vec<NativeValue> = args.iter()
                                .map(|v| NativeValue::from_value(*v))
                                .collect();

                            match self.native_handler.call(&ctx, id as u16, &native_args) {
                                NativeCallResult::Value(val) => {
                                    call_stack.push(val.into_value())?;
                                }
                                NativeCallResult::Unhandled => {
                                    return Err(VmError::RuntimeError(format!(
                                        "Native call {:#06x} not handled by native handler",
                                        id
                                    )));
                                }
                                NativeCallResult::Error(msg) => {
                                    return Err(VmError::RuntimeError(msg));
                                }
                            }
                        }
                        // Reflect native calls (std:reflect) — dispatches directly via call_reflect_method
                        id if crate::vm::builtin::is_reflect_method(id as u16) => {
                            self.call_reflect_method(task, &mut call_stack, id as u16, args, module)?;
                        }
                        // Runtime native calls (std:runtime) — dispatches directly via call_runtime_method
                        id if crate::vm::builtin::is_runtime_method(id as u16) => {
                            self.call_runtime_method(task, &mut call_stack, id as u16, args, module)?;
                        }
                        // Time native calls (std:time) — dispatches directly via call_time_method
                        id if crate::vm::builtin::is_time_method(id as u16) => {
                            self.call_time_method(&mut call_stack, id as u16, args)?;
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
            // NOTE: SORT, REDUCE, FILTER, MAP, FIND, FIND_INDEX, FOR_EACH, EVERY, SOME
            // are now compiled as inline loops by the compiler (see lower_array_intrinsic in expr.rs)
            // and never reach this handler at runtime.
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
            // NOTE: FILTER, MAP, FIND, FIND_INDEX, FOR_EACH, EVERY, SOME, SORT, REDUCE
            // are now compiled as inline loops by the compiler (see lower_array_intrinsic in expr.rs)
            // and never reach this handler at runtime.
            _ => Err(VmError::RuntimeError(format!(
                "Array method {:#06x} not yet implemented in Interpreter",
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
                "String method {:#06x} not yet implemented in Interpreter",
                method_id
            ))),
        }
    }

    /// Handle built-in regexp methods
    fn call_regexp_method(
        &mut self,
        task: &Arc<Task>,
        stack: &mut std::sync::MutexGuard<'_, Stack>,
        method_id: u16,
        arg_count: usize,
        module: &Module,
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
            id if id == regexp::REPLACE_WITH => {
                // replaceWith(str, callback): replace matches using callback
                let input = if !args.is_empty() && args[0].is_ptr() {
                    if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                        unsafe { &*s.as_ptr() }.data.clone()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };

                let callback_val = if args.len() > 1 { args[1] } else {
                    return Err(VmError::TypeError("Expected callback function".to_string()));
                };
                if !callback_val.is_ptr() {
                    return Err(VmError::TypeError("Expected callback function".to_string()));
                }
                let closure_ptr = unsafe { callback_val.as_ptr::<Closure>() };
                let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                let func_index = closure.func_id();

                let is_global = re.flags.contains('g');
                let mut result = String::new();
                let mut last_end = 0;

                task.push_closure(callback_val);

                if is_global {
                    for m in re.compiled.find_iter(&input) {
                        result.push_str(&input[last_end..m.start()]);
                        let mut match_arr = Array::new(0, 0);
                        let match_str = RayaString::new(m.as_str().to_string());
                        let gc_ptr = self.gc.lock().allocate(match_str);
                        let match_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        match_arr.push(match_val);
                        let arr_gc_ptr = self.gc.lock().allocate(match_arr);
                        let arr_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc_ptr.as_ptr()).unwrap()) };
                        let callback_result = self.execute_nested_function(task, func_index, vec![arr_val], module)?;
                        let replacement = if let Some(ptr) = unsafe { callback_result.as_ptr::<RayaString>() } {
                            unsafe { &*ptr.as_ptr() }.data.clone()
                        } else {
                            String::new()
                        };
                        result.push_str(&replacement);
                        last_end = m.end();
                    }
                } else {
                    if let Some(m) = re.compiled.find(&input) {
                        result.push_str(&input[..m.start()]);
                        let mut match_arr = Array::new(0, 0);
                        let match_str = RayaString::new(m.as_str().to_string());
                        let gc_ptr = self.gc.lock().allocate(match_str);
                        let match_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        match_arr.push(match_val);
                        let arr_gc_ptr = self.gc.lock().allocate(match_arr);
                        let arr_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc_ptr.as_ptr()).unwrap()) };
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

                result.push_str(&input[last_end..]);
                task.pop_closure();

                let result_str = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(result_str);
                let result_val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };
                stack.push(result_val)?;
            }
            _ => {
                return Err(VmError::RuntimeError(format!(
                    "RegExp method {:#06x} not yet implemented in Interpreter",
                    method_id
                )));
            }
        }
        Ok(())
    }

    /// Handle built-in Reflect methods
    fn call_reflect_method(
        &mut self,
        task: &Arc<Task>,
        stack: &mut Stack,
        method_id: u16,
        args: Vec<Value>,
        module: &Module,
    ) -> Result<(), VmError> {
        use crate::vm::builtin::reflect;

        // Helper to get string from Value
        let get_string = |v: Value| -> Result<String, VmError> {
            if !v.is_ptr() {
                return Err(VmError::TypeError("Expected string".to_string()));
            }
            let s_ptr = unsafe { v.as_ptr::<RayaString>() };
            let s = unsafe { &*s_ptr.unwrap().as_ptr() };
            Ok(s.data.clone())
        };

        let result = match method_id {
            reflect::DEFINE_METADATA => {
                // defineMetadata(key, value, target)
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "defineMetadata requires 3 arguments".to_string()
                    ));
                }
                let key = get_string(args[0].clone())?;
                let value = args[1];
                let target = args[2];

                let mut metadata = self.metadata.lock();
                metadata.define_metadata(key, value, target);
                Value::null()
            }

            reflect::DEFINE_METADATA_PROP => {
                // defineMetadata(key, value, target, propertyKey)
                if args.len() < 4 {
                    return Err(VmError::RuntimeError(
                        "defineMetadata with property requires 4 arguments".to_string()
                    ));
                }
                let key = get_string(args[0].clone())?;
                let value = args[1];
                let target = args[2];
                let property_key = get_string(args[3].clone())?;

                let mut metadata = self.metadata.lock();
                metadata.define_metadata_property(key, value, target, property_key);
                Value::null()
            }

            reflect::GET_METADATA => {
                // getMetadata(key, target)
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "getMetadata requires 2 arguments".to_string()
                    ));
                }
                let key = get_string(args[0].clone())?;
                let target = args[1];

                let metadata = self.metadata.lock();
                metadata.get_metadata(&key, target).unwrap_or(Value::null())
            }

            reflect::GET_METADATA_PROP => {
                // getMetadata(key, target, propertyKey)
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "getMetadata with property requires 3 arguments".to_string()
                    ));
                }
                let key = get_string(args[0].clone())?;
                let target = args[1];
                let property_key = get_string(args[2].clone())?;

                let metadata = self.metadata.lock();
                metadata.get_metadata_property(&key, target, &property_key).unwrap_or(Value::null())
            }

            reflect::HAS_METADATA => {
                // hasMetadata(key, target)
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "hasMetadata requires 2 arguments".to_string()
                    ));
                }
                let key = get_string(args[0].clone())?;
                let target = args[1];

                let metadata = self.metadata.lock();
                Value::bool(metadata.has_metadata(&key, target))
            }

            reflect::HAS_METADATA_PROP => {
                // hasMetadata(key, target, propertyKey)
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "hasMetadata with property requires 3 arguments".to_string()
                    ));
                }
                let key = get_string(args[0].clone())?;
                let target = args[1];
                let property_key = get_string(args[2].clone())?;

                let metadata = self.metadata.lock();
                Value::bool(metadata.has_metadata_property(&key, target, &property_key))
            }

            reflect::GET_METADATA_KEYS => {
                // getMetadataKeys(target)
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getMetadataKeys requires 1 argument".to_string()
                    ));
                }
                let target = args[0];

                let metadata = self.metadata.lock();
                let keys = metadata.get_metadata_keys(target);

                // Create an array of string keys
                let mut arr = Array::new(0, keys.len());
                for (i, key) in keys.into_iter().enumerate() {
                    let s = RayaString::new(key);
                    let gc_ptr = self.gc.lock().allocate(s);
                    let val = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                    };
                    arr.set(i, val).ok();
                }
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            reflect::GET_METADATA_KEYS_PROP => {
                // getMetadataKeys(target, propertyKey)
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "getMetadataKeys with property requires 2 arguments".to_string()
                    ));
                }
                let target = args[0];
                let property_key = get_string(args[1].clone())?;

                let metadata = self.metadata.lock();
                let keys = metadata.get_metadata_keys_property(target, &property_key);

                // Create an array of string keys
                let mut arr = Array::new(0, keys.len());
                for (i, key) in keys.into_iter().enumerate() {
                    let s = RayaString::new(key);
                    let gc_ptr = self.gc.lock().allocate(s);
                    let val = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                    };
                    arr.set(i, val).ok();
                }
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            reflect::DELETE_METADATA => {
                // deleteMetadata(key, target)
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "deleteMetadata requires 2 arguments".to_string()
                    ));
                }
                let key = get_string(args[0].clone())?;
                let target = args[1];

                let mut metadata = self.metadata.lock();
                Value::bool(metadata.delete_metadata(&key, target))
            }

            reflect::DELETE_METADATA_PROP => {
                // deleteMetadata(key, target, propertyKey)
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "deleteMetadata with property requires 3 arguments".to_string()
                    ));
                }
                let key = get_string(args[0].clone())?;
                let target = args[1];
                let property_key = get_string(args[2].clone())?;

                let mut metadata = self.metadata.lock();
                Value::bool(metadata.delete_metadata_property(&key, target, &property_key))
            }

            // ===== Phase 2: Class Introspection =====

            reflect::GET_CLASS => {
                // getClass(obj) -> returns class ID as i32, or null if not an object
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getClass requires 1 argument".to_string()
                    ));
                }
                let obj = args[0];
                if let Some(class_id) = crate::vm::reflect::get_class_id(obj) {
                    Value::i32(class_id as i32)
                } else {
                    Value::null()
                }
            }

            reflect::GET_CLASS_BY_NAME => {
                // getClassByName(name) -> returns class ID as i32, or null if not found
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getClassByName requires 1 argument".to_string()
                    ));
                }
                let name = get_string(args[0].clone())?;
                let classes = self.classes.read();
                if let Some(class) = classes.get_class_by_name(&name) {
                    Value::i32(class.id as i32)
                } else {
                    Value::null()
                }
            }

            reflect::GET_ALL_CLASSES => {
                // getAllClasses() -> returns array of class IDs
                let classes = self.classes.read();
                let class_ids: Vec<Value> = classes
                    .iter()
                    .map(|(id, _)| Value::i32(id as i32))
                    .collect();

                let mut arr = Array::new(0, class_ids.len());
                for (i, val) in class_ids.into_iter().enumerate() {
                    arr.set(i, val).ok();
                }
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            reflect::GET_CLASSES_WITH_DECORATOR => {
                // getClassesWithDecorator(decorator) -> returns array of class IDs
                // NOTE: This requires --emit-reflection to work fully
                // For now, returns empty array (decorator metadata not yet stored)
                let arr = Array::new(0, 0);
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            reflect::IS_SUBCLASS_OF => {
                // isSubclassOf(subClassId, superClassId) -> boolean
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "isSubclassOf requires 2 arguments".to_string()
                    ));
                }
                let sub_id = args[0].as_i32().unwrap_or(-1);
                let super_id = args[1].as_i32().unwrap_or(-1);

                if sub_id < 0 || super_id < 0 {
                    Value::bool(false)
                } else {
                    let classes = self.classes.read();
                    Value::bool(crate::vm::reflect::is_subclass_of(
                        &classes,
                        sub_id as usize,
                        super_id as usize,
                    ))
                }
            }

            reflect::IS_INSTANCE_OF => {
                // isInstanceOf(obj, classId) -> boolean
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "isInstanceOf requires 2 arguments".to_string()
                    ));
                }
                let obj = args[0];
                let class_id = args[1].as_i32().unwrap_or(-1);

                if class_id < 0 {
                    Value::bool(false)
                } else {
                    let classes = self.classes.read();
                    Value::bool(crate::vm::reflect::is_instance_of(
                        &classes,
                        obj,
                        class_id as usize,
                    ))
                }
            }

            reflect::GET_TYPE_INFO => {
                // getTypeInfo(target) -> returns type kind as string
                // NOTE: Full TypeInfo requires --emit-reflection
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getTypeInfo requires 1 argument".to_string()
                    ));
                }
                let target = args[0];
                let type_info = crate::vm::reflect::get_type_info_for_value(target);

                // Return the type name as a string for now
                let s = RayaString::new(type_info.name);
                let gc_ptr = self.gc.lock().allocate(s);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
            }

            reflect::GET_CLASS_HIERARCHY => {
                // getClassHierarchy(obj) -> returns array of class IDs from obj's class to root
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getClassHierarchy requires 1 argument".to_string()
                    ));
                }
                let obj = args[0];

                if let Some(class_id) = crate::vm::reflect::get_class_id(obj) {
                    let classes = self.classes.read();
                    let hierarchy = crate::vm::reflect::get_class_hierarchy(&classes, class_id);

                    let class_ids: Vec<Value> = hierarchy
                        .iter()
                        .map(|c| Value::i32(c.id as i32))
                        .collect();

                    drop(classes);

                    let mut arr = Array::new(0, class_ids.len());
                    for (i, val) in class_ids.into_iter().enumerate() {
                        arr.set(i, val).ok();
                    }
                    let arr_gc = self.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                } else {
                    // Not an object, return empty array
                    let arr = Array::new(0, 0);
                    let arr_gc = self.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                }
            }

            // ===== Phase 3: Field Access =====

            reflect::GET => {
                // get(target, propertyKey) -> get field value by name
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "get requires 2 arguments (target, propertyKey)".to_string()
                    ));
                }
                let target = args[0];
                let property_key = get_string(args[1].clone())?;

                if !target.is_ptr() {
                    return Err(VmError::TypeError("get: target must be an object".to_string()));
                }

                // Get class ID from object
                let class_id = crate::vm::reflect::get_class_id(target)
                    .ok_or_else(|| VmError::TypeError("get: target is not a class instance".to_string()))?;

                // Look up field index from class metadata
                let class_metadata = self.class_metadata.read();
                let field_index = class_metadata.get(class_id)
                    .and_then(|meta| meta.get_field_index(&property_key));
                drop(class_metadata);

                if let Some(index) = field_index {
                    let obj_ptr = unsafe { target.as_ptr::<Object>() };
                    let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                    obj.get_field(index).unwrap_or(Value::null())
                } else {
                    // Field not found in metadata - return null
                    Value::null()
                }
            }

            reflect::SET => {
                // set(target, propertyKey, value) -> set field value by name
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "set requires 3 arguments (target, propertyKey, value)".to_string()
                    ));
                }
                let target = args[0];
                let property_key = get_string(args[1].clone())?;
                let value = args[2];

                if !target.is_ptr() {
                    return Err(VmError::TypeError("set: target must be an object".to_string()));
                }

                // Get class ID from object
                let class_id = crate::vm::reflect::get_class_id(target)
                    .ok_or_else(|| VmError::TypeError("set: target is not a class instance".to_string()))?;

                // Look up field index from class metadata
                let class_metadata = self.class_metadata.read();
                let field_index = class_metadata.get(class_id)
                    .and_then(|meta| meta.get_field_index(&property_key));
                drop(class_metadata);

                if let Some(index) = field_index {
                    let obj_ptr = unsafe { target.as_ptr::<Object>() };
                    let obj = unsafe { &mut *obj_ptr.unwrap().as_ptr() };
                    match obj.set_field(index, value) {
                        Ok(()) => Value::bool(true),
                        Err(_) => Value::bool(false),
                    }
                } else {
                    // Field not found in metadata
                    Value::bool(false)
                }
            }

            reflect::HAS => {
                // has(target, propertyKey) -> check if field exists
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "has requires 2 arguments (target, propertyKey)".to_string()
                    ));
                }
                let target = args[0];
                let property_key = get_string(args[1].clone())?;

                if !target.is_ptr() {
                    Value::bool(false)
                } else if let Some(class_id) = crate::vm::reflect::get_class_id(target) {
                    let class_metadata = self.class_metadata.read();
                    let has_field = class_metadata.get(class_id)
                        .map(|meta| meta.has_field(&property_key))
                        .unwrap_or(false);
                    Value::bool(has_field)
                } else {
                    Value::bool(false)
                }
            }

            reflect::GET_FIELD_NAMES => {
                // getFieldNames(target) -> list all field names
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getFieldNames requires 1 argument".to_string()
                    ));
                }
                let target = args[0];

                let field_names = if let Some(class_id) = crate::vm::reflect::get_class_id(target) {
                    let class_metadata = self.class_metadata.read();
                    class_metadata.get(class_id)
                        .map(|meta| meta.field_names.clone())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };

                // Create array of strings
                let mut arr = Array::new(0, field_names.len());
                for (i, name) in field_names.into_iter().enumerate() {
                    if !name.is_empty() {
                        let s = RayaString::new(name);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        arr.set(i, val).ok();
                    }
                }
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            reflect::GET_FIELD_INFO => {
                // getFieldInfo(target, propertyKey) -> get field metadata as Map
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "getFieldInfo requires 2 arguments (target, propertyKey)".to_string()
                    ));
                }
                let target = args[0];
                let property_key = get_string(args[1].clone())?;

                if !target.is_ptr() {
                    Value::null()
                } else if let Some(class_id) = crate::vm::reflect::get_class_id(target) {
                    let class_metadata = self.class_metadata.read();
                    if let Some(meta) = class_metadata.get(class_id) {
                        if let Some(field_info) = meta.get_field_info(&property_key) {
                            // Create a MapObject with field info properties
                            let mut map = MapObject::new();

                            // Add field properties
                            let name_str = RayaString::new(field_info.name.clone());
                            let name_gc = self.gc.lock().allocate(name_str);
                            let name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(name_gc.as_ptr()).unwrap()) };

                            let type_str = RayaString::new(field_info.type_info.name.clone());
                            let type_gc = self.gc.lock().allocate(type_str);
                            let type_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(type_gc.as_ptr()).unwrap()) };

                            let key_name = RayaString::new("name".to_string());
                            let key_name_gc = self.gc.lock().allocate(key_name);
                            let key_name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_name_gc.as_ptr()).unwrap()) };
                            map.set(key_name_val, name_val);

                            let key_type = RayaString::new("type".to_string());
                            let key_type_gc = self.gc.lock().allocate(key_type);
                            let key_type_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_type_gc.as_ptr()).unwrap()) };
                            map.set(key_type_val, type_val);

                            let key_index = RayaString::new("index".to_string());
                            let key_index_gc = self.gc.lock().allocate(key_index);
                            let key_index_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_index_gc.as_ptr()).unwrap()) };
                            map.set(key_index_val, Value::i32(field_info.field_index as i32));

                            let key_static = RayaString::new("isStatic".to_string());
                            let key_static_gc = self.gc.lock().allocate(key_static);
                            let key_static_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_static_gc.as_ptr()).unwrap()) };
                            map.set(key_static_val, Value::bool(field_info.is_static));

                            let key_readonly = RayaString::new("isReadonly".to_string());
                            let key_readonly_gc = self.gc.lock().allocate(key_readonly);
                            let key_readonly_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_readonly_gc.as_ptr()).unwrap()) };
                            map.set(key_readonly_val, Value::bool(field_info.is_readonly));

                            let key_class = RayaString::new("declaringClass".to_string());
                            let key_class_gc = self.gc.lock().allocate(key_class);
                            let key_class_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_class_gc.as_ptr()).unwrap()) };
                            map.set(key_class_val, Value::i32(field_info.declaring_class_id as i32));

                            let map_gc = self.gc.lock().allocate(map);
                            unsafe { Value::from_ptr(std::ptr::NonNull::new(map_gc.as_ptr()).unwrap()) }
                        } else {
                            Value::null()
                        }
                    } else {
                        Value::null()
                    }
                } else {
                    Value::null()
                }
            }

            reflect::GET_FIELDS => {
                // getFields(target) -> get all field infos as array of Maps
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getFields requires 1 argument (target)".to_string()
                    ));
                }
                let target = args[0];

                if !target.is_ptr() {
                    let arr = Array::new(0, 0);
                    let arr_gc = self.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                } else if let Some(class_id) = crate::vm::reflect::get_class_id(target) {
                    let class_metadata = self.class_metadata.read();
                    if let Some(meta) = class_metadata.get(class_id) {
                        let fields = meta.get_all_field_infos();
                        let mut arr = Array::new(fields.len(), 0);

                        for (i, field_info) in fields.iter().enumerate() {
                            // Create a MapObject for each field
                            let mut map = MapObject::new();

                            let key_name = RayaString::new("name".to_string());
                            let key_name_gc = self.gc.lock().allocate(key_name);
                            let key_name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_name_gc.as_ptr()).unwrap()) };

                            let name_str = RayaString::new(field_info.name.clone());
                            let name_gc = self.gc.lock().allocate(name_str);
                            let name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(name_gc.as_ptr()).unwrap()) };
                            map.set(key_name_val, name_val);

                            let key_type = RayaString::new("type".to_string());
                            let key_type_gc = self.gc.lock().allocate(key_type);
                            let key_type_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_type_gc.as_ptr()).unwrap()) };

                            let type_str = RayaString::new(field_info.type_info.name.clone());
                            let type_gc = self.gc.lock().allocate(type_str);
                            let type_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(type_gc.as_ptr()).unwrap()) };
                            map.set(key_type_val, type_val);

                            let key_index = RayaString::new("index".to_string());
                            let key_index_gc = self.gc.lock().allocate(key_index);
                            let key_index_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_index_gc.as_ptr()).unwrap()) };
                            map.set(key_index_val, Value::i32(field_info.field_index as i32));

                            let key_static = RayaString::new("isStatic".to_string());
                            let key_static_gc = self.gc.lock().allocate(key_static);
                            let key_static_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_static_gc.as_ptr()).unwrap()) };
                            map.set(key_static_val, Value::bool(field_info.is_static));

                            let key_readonly = RayaString::new("isReadonly".to_string());
                            let key_readonly_gc = self.gc.lock().allocate(key_readonly);
                            let key_readonly_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_readonly_gc.as_ptr()).unwrap()) };
                            map.set(key_readonly_val, Value::bool(field_info.is_readonly));

                            let key_class = RayaString::new("declaringClass".to_string());
                            let key_class_gc = self.gc.lock().allocate(key_class);
                            let key_class_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_class_gc.as_ptr()).unwrap()) };
                            map.set(key_class_val, Value::i32(field_info.declaring_class_id as i32));

                            let map_gc = self.gc.lock().allocate(map);
                            let map_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(map_gc.as_ptr()).unwrap()) };
                            arr.set(i, map_val).ok();
                        }

                        let arr_gc = self.gc.lock().allocate(arr);
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                    } else {
                        let arr = Array::new(0, 0);
                        let arr_gc = self.gc.lock().allocate(arr);
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                    }
                } else {
                    let arr = Array::new(0, 0);
                    let arr_gc = self.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                }
            }

            reflect::GET_STATIC_FIELD_NAMES => {
                // getStaticFieldNames(classId) -> get static field names as array
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getStaticFieldNames requires 1 argument (classId)".to_string()
                    ));
                }
                let class_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("getStaticFieldNames: classId must be a number".to_string()))?
                    as usize;

                let class_metadata = self.class_metadata.read();
                if let Some(meta) = class_metadata.get(class_id) {
                    let names = &meta.static_field_names;
                    let mut arr = Array::new(names.len(), 0);
                    for (i, name) in names.iter().enumerate() {
                        if !name.is_empty() {
                            let s = RayaString::new(name.clone());
                            let gc_ptr = self.gc.lock().allocate(s);
                            let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                            arr.set(i, val).ok();
                        }
                    }
                    let arr_gc = self.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                } else {
                    let arr = Array::new(0, 0);
                    let arr_gc = self.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                }
            }

            reflect::GET_STATIC_FIELDS => {
                // getStaticFields(classId) -> get static field infos (stub for now)
                // Static field detailed info requires additional metadata
                let arr = Array::new(0, 0);
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            // ===== Phase 4: Method Invocation =====

            reflect::HAS_METHOD => {
                // hasMethod(target, methodName) -> check if method exists
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "hasMethod requires 2 arguments (target, methodName)".to_string()
                    ));
                }
                let target = args[0];
                let method_name = get_string(args[1].clone())?;

                if !target.is_ptr() {
                    Value::bool(false)
                } else if let Some(class_id) = crate::vm::reflect::get_class_id(target) {
                    let class_metadata = self.class_metadata.read();
                    let has_method = class_metadata.get(class_id)
                        .map(|meta| meta.has_method(&method_name))
                        .unwrap_or(false);
                    Value::bool(has_method)
                } else {
                    Value::bool(false)
                }
            }

            reflect::GET_METHODS | reflect::GET_METHOD | reflect::GET_METHOD_INFO |
            reflect::INVOKE | reflect::INVOKE_ASYNC | reflect::INVOKE_STATIC |
            reflect::GET_STATIC_METHODS => {
                // These require full --emit-reflection metadata and dynamic dispatch
                // Return null/empty for now
                match method_id {
                    reflect::INVOKE | reflect::INVOKE_ASYNC | reflect::INVOKE_STATIC => {
                        return Err(VmError::RuntimeError(
                            "Dynamic method invocation requires --emit-reflection".to_string()
                        ));
                    }
                    reflect::GET_METHOD | reflect::GET_METHOD_INFO => Value::null(),
                    _ => {
                        let arr = Array::new(0, 0);
                        let arr_gc = self.gc.lock().allocate(arr);
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                    }
                }
            }

            // ===== Phase 5: Object Creation =====

            reflect::CONSTRUCT => {
                // construct(classId, ...args) -> create instance
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "construct requires at least 1 argument (classId)".to_string()
                    ));
                }
                let class_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("construct: classId must be a number".to_string()))?
                    as usize;

                let classes = self.classes.read();
                let class = classes.get_class(class_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("Class {} not found", class_id)))?;
                let field_count = class.field_count;
                drop(classes);

                // Allocate new object
                let obj = Object::new(class_id, field_count);
                let gc_ptr = self.gc.lock().allocate(obj);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }

                // Note: Constructor call with args requires more work (call constructor function)
            }

            reflect::ALLOCATE => {
                // allocate(classId) -> allocate uninitialized instance
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "allocate requires 1 argument (classId)".to_string()
                    ));
                }
                let class_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("allocate: classId must be a number".to_string()))?
                    as usize;

                let classes = self.classes.read();
                let class = classes.get_class(class_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("Class {} not found", class_id)))?;
                let field_count = class.field_count;
                drop(classes);

                // Allocate new object (uninitialized - fields are null)
                let obj = Object::new(class_id, field_count);
                let gc_ptr = self.gc.lock().allocate(obj);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
            }

            reflect::CLONE => {
                // clone(obj) -> shallow clone
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "clone requires 1 argument".to_string()
                    ));
                }
                let target = args[0];

                if !target.is_ptr() {
                    // Primitives are copied by value
                    target
                } else if let Some(_class_id) = crate::vm::reflect::get_class_id(target) {
                    // Clone object
                    let obj_ptr = unsafe { target.as_ptr::<Object>() };
                    let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                    let cloned = obj.clone();
                    let gc_ptr = self.gc.lock().allocate(cloned);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
                } else {
                    // Unknown pointer type, return as-is
                    target
                }
            }

            reflect::CONSTRUCT_WITH | reflect::DEEP_CLONE | reflect::GET_CONSTRUCTOR_INFO => {
                // These require more complex implementation
                match method_id {
                    reflect::CONSTRUCT_WITH => {
                        return Err(VmError::RuntimeError(
                            "constructWith requires --emit-reflection".to_string()
                        ));
                    }
                    reflect::DEEP_CLONE => {
                        return Err(VmError::RuntimeError(
                            "deepClone not yet implemented".to_string()
                        ));
                    }
                    _ => Value::null()
                }
            }

            // ===== Phase 6: Type Utilities =====

            reflect::IS_STRING => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError("isString requires 1 argument".to_string()));
                }
                let value = args[0];
                let is_string = value.is_ptr() && unsafe { value.as_ptr::<RayaString>().is_some() };
                Value::bool(is_string)
            }

            reflect::IS_NUMBER => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError("isNumber requires 1 argument".to_string()));
                }
                let value = args[0];
                let is_number = value.as_f64().is_some() || value.as_i32().is_some();
                Value::bool(is_number)
            }

            reflect::IS_BOOLEAN => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError("isBoolean requires 1 argument".to_string()));
                }
                let value = args[0];
                let is_bool = value.as_bool().is_some();
                Value::bool(is_bool)
            }

            reflect::IS_NULL => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError("isNull requires 1 argument".to_string()));
                }
                let value = args[0];
                Value::bool(value.is_null())
            }

            reflect::IS_ARRAY => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError("isArray requires 1 argument".to_string()));
                }
                let value = args[0];
                let is_array = value.is_ptr() && unsafe { value.as_ptr::<Array>().is_some() };
                Value::bool(is_array)
            }

            reflect::IS_FUNCTION => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError("isFunction requires 1 argument".to_string()));
                }
                let value = args[0];
                let is_func = value.is_ptr() && unsafe { value.as_ptr::<Closure>().is_some() };
                Value::bool(is_func)
            }

            reflect::IS_OBJECT => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError("isObject requires 1 argument".to_string()));
                }
                let value = args[0];
                let is_obj = value.is_ptr() && unsafe { value.as_ptr::<Object>().is_some() };
                Value::bool(is_obj)
            }

            reflect::TYPE_OF => {
                // typeOf(typeName) - get TypeInfo from string
                if args.is_empty() {
                    return Err(VmError::RuntimeError("typeOf requires 1 argument".to_string()));
                }
                let type_name = get_string(args[0].clone())?;

                // Check primitive types
                let (kind, class_id) = match type_name.as_str() {
                    "string" | "number" | "boolean" | "null" | "void" | "any" =>
                        ("primitive".to_string(), None),
                    _ => {
                        // Check if it's a class name
                        let classes = self.classes.read();
                        if let Some(class) = classes.get_class_by_name(&type_name) {
                            ("class".to_string(), Some(class.id))
                        } else {
                            // Unknown type
                            return Ok(stack.push(Value::null())?);
                        }
                    }
                };

                // Return TypeInfo as a Map
                let mut map = MapObject::new();
                let kind_str = RayaString::new(kind);
                let kind_ptr = self.gc.lock().allocate(kind_str);
                let kind_key = RayaString::new("kind".to_string());
                let kind_key_ptr = self.gc.lock().allocate(kind_key);
                map.set(unsafe { Value::from_ptr(std::ptr::NonNull::new(kind_key_ptr.as_ptr()).unwrap()) },
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(kind_ptr.as_ptr()).unwrap()) });

                let name_str = RayaString::new(type_name);
                let name_ptr = self.gc.lock().allocate(name_str);
                let name_key = RayaString::new("name".to_string());
                let name_key_ptr = self.gc.lock().allocate(name_key);
                map.set(unsafe { Value::from_ptr(std::ptr::NonNull::new(name_key_ptr.as_ptr()).unwrap()) },
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(name_ptr.as_ptr()).unwrap()) });

                if let Some(id) = class_id {
                    let id_key = RayaString::new("classId".to_string());
                    let id_key_ptr = self.gc.lock().allocate(id_key);
                    map.set(unsafe { Value::from_ptr(std::ptr::NonNull::new(id_key_ptr.as_ptr()).unwrap()) },
                            Value::i32(id as i32));
                }

                let map_ptr = self.gc.lock().allocate(map);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(map_ptr.as_ptr()).unwrap()) }
            }

            reflect::IS_ASSIGNABLE_TO => {
                // isAssignableTo(sourceType, targetType) - check type compatibility
                if args.len() < 2 {
                    return Err(VmError::RuntimeError("isAssignableTo requires 2 arguments".to_string()));
                }
                let source = get_string(args[0].clone())?;
                let target = get_string(args[1].clone())?;

                // Same type is always assignable
                if source == target {
                    Value::bool(true)
                } else if target == "any" {
                    // Everything is assignable to any
                    Value::bool(true)
                } else {
                    // Check class hierarchy
                    let classes = self.classes.read();
                    let source_class = classes.get_class_by_name(&source);
                    let target_class = classes.get_class_by_name(&target);

                    if let (Some(src), Some(tgt)) = (source_class, target_class) {
                        let is_subclass = crate::vm::reflect::is_subclass_of(&classes, src.id, tgt.id);
                        Value::bool(is_subclass)
                    } else {
                        Value::bool(false)
                    }
                }
            }

            reflect::CAST => {
                // cast(value, classId) - safe cast, returns null if incompatible
                if args.len() < 2 {
                    return Err(VmError::RuntimeError("cast requires 2 arguments".to_string()));
                }
                let value = args[0];
                let class_id = value_to_f64(args[1])? as usize;

                let classes = self.classes.read();
                if crate::vm::reflect::is_instance_of(&classes, value, class_id) {
                    value
                } else {
                    Value::null()
                }
            }

            reflect::CAST_OR_THROW => {
                // castOrThrow(value, classId) - cast or throw error
                if args.len() < 2 {
                    return Err(VmError::RuntimeError("castOrThrow requires 2 arguments".to_string()));
                }
                let value = args[0];
                let class_id = value_to_f64(args[1])? as usize;

                let classes = self.classes.read();
                if crate::vm::reflect::is_instance_of(&classes, value, class_id) {
                    value
                } else {
                    return Err(VmError::TypeError(format!(
                        "Cannot cast value to class {}",
                        class_id
                    )));
                }
            }

            // ===== Phase 7: Interface and Hierarchy Query =====

            reflect::IMPLEMENTS => {
                // implements(classId, interfaceName) - check if class implements interface
                if args.len() < 2 {
                    return Err(VmError::RuntimeError("implements requires 2 arguments".to_string()));
                }
                let class_id = value_to_f64(args[0])? as usize;
                let interface_name = get_string(args[1].clone())?;

                let class_metadata = self.class_metadata.read();
                if let Some(meta) = class_metadata.get(class_id) {
                    Value::bool(meta.implements_interface(&interface_name))
                } else {
                    Value::bool(false)
                }
            }

            reflect::GET_INTERFACES => {
                // getInterfaces(classId) - get interfaces implemented by class
                if args.is_empty() {
                    return Err(VmError::RuntimeError("getInterfaces requires 1 argument".to_string()));
                }
                let class_id = value_to_f64(args[0])? as usize;

                let class_metadata = self.class_metadata.read();
                let interfaces: Vec<String> = if let Some(meta) = class_metadata.get(class_id) {
                    meta.get_interfaces().to_vec()
                } else {
                    Vec::new()
                };
                drop(class_metadata);

                // Build array of interface names
                let mut arr = Array::new(0, 0);
                for iface in interfaces {
                    let s = RayaString::new(iface);
                    let s_ptr = self.gc.lock().allocate(s);
                    arr.push(unsafe { Value::from_ptr(std::ptr::NonNull::new(s_ptr.as_ptr()).unwrap()) });
                }
                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }

            reflect::GET_SUPERCLASS => {
                // getSuperclass(classId) - get parent class
                if args.is_empty() {
                    return Err(VmError::RuntimeError("getSuperclass requires 1 argument".to_string()));
                }
                let class_id = value_to_f64(args[0])? as usize;

                let classes = self.classes.read();
                if let Some(class) = classes.get_class(class_id) {
                    if let Some(parent) = class.parent_id {
                        Value::i32(parent as i32)
                    } else {
                        Value::null()
                    }
                } else {
                    Value::null()
                }
            }

            reflect::GET_SUBCLASSES => {
                // getSubclasses(classId) - get direct subclasses
                if args.is_empty() {
                    return Err(VmError::RuntimeError("getSubclasses requires 1 argument".to_string()));
                }
                let class_id = value_to_f64(args[0])? as usize;

                let classes = self.classes.read();
                let mut subclasses = Vec::new();
                for (id, class) in classes.iter() {
                    if class.parent_id == Some(class_id) {
                        subclasses.push(id);
                    }
                }
                drop(classes);

                let mut arr = Array::new(0, 0);
                for id in subclasses {
                    arr.push(Value::i32(id as i32));
                }
                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }

            reflect::GET_IMPLEMENTORS => {
                // getImplementors(interfaceName) - get all classes implementing interface
                if args.is_empty() {
                    return Err(VmError::RuntimeError("getImplementors requires 1 argument".to_string()));
                }
                let interface_name = get_string(args[0].clone())?;

                let class_metadata = self.class_metadata.read();
                let implementors = class_metadata.get_implementors(&interface_name);
                drop(class_metadata);

                let mut arr = Array::new(0, 0);
                for id in implementors {
                    arr.push(Value::i32(id as i32));
                }
                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }

            reflect::IS_STRUCTURALLY_COMPATIBLE => {
                // isStructurallyCompatible(sourceClassId, targetClassId) - check structural compatibility
                if args.len() < 2 {
                    return Err(VmError::RuntimeError("isStructurallyCompatible requires 2 arguments".to_string()));
                }
                let source_id = value_to_f64(args[0])? as usize;
                let target_id = value_to_f64(args[1])? as usize;

                let class_metadata = self.class_metadata.read();
                let source_meta = class_metadata.get(source_id);
                let target_meta = class_metadata.get(target_id);

                if let (Some(source), Some(target)) = (source_meta, target_meta) {
                    // Check if source has all fields of target
                    let fields_ok = target.field_names.iter().all(|name| source.has_field(name));
                    // Check if source has all methods of target
                    let methods_ok = target.method_names.iter().all(|name|
                        name.is_empty() || source.has_method(name)
                    );
                    Value::bool(fields_ok && methods_ok)
                } else {
                    Value::bool(false)
                }
            }

            // ===== Phase 8: Object Inspection =====

            reflect::INSPECT => {
                // inspect(obj, depth?) - human-readable representation
                if args.is_empty() {
                    return Err(VmError::RuntimeError("inspect requires 1 argument".to_string()));
                }
                let target = args[0];
                let max_depth = if args.len() > 1 {
                    value_to_f64(args[1])? as usize
                } else {
                    2
                };

                let result = self.inspect_value(target, 0, max_depth)?;
                let s = RayaString::new(result);
                let s_ptr = self.gc.lock().allocate(s);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(s_ptr.as_ptr()).unwrap()) }
            }

            reflect::GET_OBJECT_ID => {
                // getObjectId(obj) - unique object identifier
                if args.is_empty() {
                    return Err(VmError::RuntimeError("getObjectId requires 1 argument".to_string()));
                }
                let value = args[0];

                if !value.is_ptr() || value.is_null() {
                    Value::i32(0)
                } else if let Some(ptr) = unsafe { value.as_ptr::<Object>() } {
                    Value::i32((ptr.as_ptr() as usize & 0x7FFFFFFF) as i32)
                } else if let Some(ptr) = unsafe { value.as_ptr::<Array>() } {
                    Value::i32((ptr.as_ptr() as usize & 0x7FFFFFFF) as i32)
                } else if let Some(ptr) = unsafe { value.as_ptr::<RayaString>() } {
                    Value::i32((ptr.as_ptr() as usize & 0x7FFFFFFF) as i32)
                } else {
                    Value::i32(0)
                }
            }

            reflect::DESCRIBE => {
                // describe(classId) - detailed class description
                if args.is_empty() {
                    return Err(VmError::RuntimeError("describe requires 1 argument".to_string()));
                }
                let class_id = value_to_f64(args[0])? as usize;

                let classes = self.classes.read();
                let class = classes.get_class(class_id);
                let class_metadata = self.class_metadata.read();
                let meta = class_metadata.get(class_id);

                let description = if let Some(class) = class {
                    let mut desc = format!("class {} {{\n", class.name);

                    if let Some(m) = meta {
                        // Fields
                        for name in &m.field_names {
                            desc.push_str(&format!("  {}: any;\n", name));
                        }
                        // Methods
                        for name in &m.method_names {
                            if !name.is_empty() {
                                desc.push_str(&format!("  {}(): any;\n", name));
                            }
                        }
                    } else {
                        desc.push_str(&format!("  // {} fields\n", class.field_count));
                    }

                    desc.push_str("}");
                    desc
                } else {
                    format!("Unknown class {}", class_id)
                };

                let s = RayaString::new(description);
                let s_ptr = self.gc.lock().allocate(s);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(s_ptr.as_ptr()).unwrap()) }
            }

            reflect::SNAPSHOT => {
                // snapshot(obj) - Capture object state as a snapshot
                if args.is_empty() {
                    return Err(VmError::RuntimeError("snapshot requires 1 argument".to_string()));
                }
                let target = args[0];

                // Create snapshot context with max depth of 10
                let mut ctx = SnapshotContext::new(10);

                // Get class name if it's an object
                let (class_name, field_names) = if let Some(ptr) = unsafe { target.as_ptr::<Object>() } {
                    let obj = unsafe { &*ptr.as_ptr() };
                    let class_registry = self.classes.read();
                    if let Some(class) = class_registry.get_class(obj.class_id) {
                        let names: Vec<String> = (0..class.field_count)
                            .map(|i| format!("field_{}", i))
                            .collect();
                        (class.name.clone(), names)
                    } else {
                        (format!("Class{}", obj.class_id), Vec::new())
                    }
                } else {
                    ("unknown".to_string(), Vec::new())
                };

                // Capture the snapshot
                let snapshot = ctx.capture_object_with_names(target, &field_names, &class_name);

                // Convert snapshot to a Raya Object
                self.snapshot_to_value(&snapshot)
            }

            reflect::DIFF => {
                // diff(a, b) - Compare two objects and return differences
                if args.len() < 2 {
                    return Err(VmError::RuntimeError("diff requires 2 arguments".to_string()));
                }
                let obj_a = args[0];
                let obj_b = args[1];

                // Capture both objects as snapshots
                let mut ctx = SnapshotContext::new(10);

                let (class_name_a, field_names_a) = if let Some(ptr) = unsafe { obj_a.as_ptr::<Object>() } {
                    let obj = unsafe { &*ptr.as_ptr() };
                    let class_registry = self.classes.read();
                    if let Some(class) = class_registry.get_class(obj.class_id) {
                        let names: Vec<String> = (0..class.field_count)
                            .map(|i| format!("field_{}", i))
                            .collect();
                        (class.name.clone(), names)
                    } else {
                        (format!("Class{}", obj.class_id), Vec::new())
                    }
                } else {
                    ("unknown".to_string(), Vec::new())
                };

                let (class_name_b, field_names_b) = if let Some(ptr) = unsafe { obj_b.as_ptr::<Object>() } {
                    let obj = unsafe { &*ptr.as_ptr() };
                    let class_registry = self.classes.read();
                    if let Some(class) = class_registry.get_class(obj.class_id) {
                        let names: Vec<String> = (0..class.field_count)
                            .map(|i| format!("field_{}", i))
                            .collect();
                        (class.name.clone(), names)
                    } else {
                        (format!("Class{}", obj.class_id), Vec::new())
                    }
                } else {
                    ("unknown".to_string(), Vec::new())
                };

                let snapshot_a = ctx.capture_object_with_names(obj_a, &field_names_a, &class_name_a);
                let snapshot_b = ctx.capture_object_with_names(obj_b, &field_names_b, &class_name_b);

                // Compute the diff
                let diff = ObjectDiff::compute(&snapshot_a, &snapshot_b);

                // Convert diff to a Raya Object
                self.diff_to_value(&diff)
            }

            // ===== Phase 8: Memory Analysis =====

            reflect::GET_OBJECT_SIZE => {
                // getObjectSize(obj) - shallow memory size
                if args.is_empty() {
                    return Err(VmError::RuntimeError("getObjectSize requires 1 argument".to_string()));
                }
                let value = args[0];

                let size = if !value.is_ptr() || value.is_null() {
                    8 // primitive size
                } else if let Some(ptr) = unsafe { value.as_ptr::<Object>() } {
                    let obj = unsafe { &*ptr.as_ptr() };
                    std::mem::size_of::<Object>() + obj.fields.len() * 8
                } else if let Some(ptr) = unsafe { value.as_ptr::<Array>() } {
                    let arr = unsafe { &*ptr.as_ptr() };
                    std::mem::size_of::<Array>() + arr.len() * 8
                } else if let Some(ptr) = unsafe { value.as_ptr::<RayaString>() } {
                    let s = unsafe { &*ptr.as_ptr() };
                    std::mem::size_of::<RayaString>() + s.data.len()
                } else {
                    8
                };

                Value::i32(size as i32)
            }

            reflect::GET_RETAINED_SIZE => {
                // getRetainedSize(obj) - size including referenced objects
                if args.is_empty() {
                    return Err(VmError::RuntimeError("getRetainedSize requires 1 argument".to_string()));
                }
                let target = args[0];

                let mut visited = std::collections::HashSet::new();
                let size = self.calculate_retained_size(target, &mut visited);
                Value::i32(size as i32)
            }

            reflect::GET_REFERENCES => {
                // getReferences(obj) - objects referenced by this object
                if args.is_empty() {
                    return Err(VmError::RuntimeError("getReferences requires 1 argument".to_string()));
                }
                let target = args[0];

                let mut refs = Vec::new();
                self.collect_references(target, &mut refs);

                let mut arr = Array::new(0, 0);
                for r in refs {
                    arr.push(r);
                }
                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }

            reflect::GET_REFERRERS => {
                // getReferrers(obj) - objects that reference this object
                if args.is_empty() {
                    return Err(VmError::RuntimeError("getReferrers requires 1 argument".to_string()));
                }
                let target = args[0];

                // Get target's identity
                let target_id = if let Some(ptr) = unsafe { target.as_ptr::<u8>() } {
                    ptr.as_ptr() as usize
                } else {
                    return Ok(stack.push(Value::null())?);
                };

                // Scan all allocations for references to target
                let gc = self.gc.lock();
                let mut referrers = Vec::new();

                for header_ptr in gc.heap().iter_allocations() {
                    let header = unsafe { &*header_ptr };
                    // Get the object pointer (after header)
                    let obj_ptr = unsafe { header_ptr.add(1) as *const u8 };

                    // Check if this object references the target
                    // This is a simplified check - just look at Object types
                    if header.type_id() == std::any::TypeId::of::<Object>() {
                        let obj = unsafe { &*(obj_ptr as *const Object) };
                        for field in &obj.fields {
                            if let Some(ptr) = unsafe { field.as_ptr::<u8>() } {
                                if ptr.as_ptr() as usize == target_id {
                                    let value = unsafe {
                                        Value::from_ptr(std::ptr::NonNull::new(obj_ptr as *mut Object).unwrap())
                                    };
                                    referrers.push(value);
                                    break;
                                }
                            }
                        }
                    }
                }
                drop(gc);

                let mut arr = Array::new(0, 0);
                for r in referrers {
                    arr.push(r);
                }
                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }

            reflect::GET_HEAP_STATS => {
                // getHeapStats() - heap statistics
                let gc = self.gc.lock();
                let stats = gc.heap_stats();
                drop(gc);

                let mut map = MapObject::new();

                // totalObjects
                let key = RayaString::new("totalObjects".to_string());
                let key_ptr = self.gc.lock().allocate(key);
                map.set(unsafe { Value::from_ptr(std::ptr::NonNull::new(key_ptr.as_ptr()).unwrap()) },
                        Value::i32(stats.allocation_count as i32));

                // totalBytes
                let key = RayaString::new("totalBytes".to_string());
                let key_ptr = self.gc.lock().allocate(key);
                map.set(unsafe { Value::from_ptr(std::ptr::NonNull::new(key_ptr.as_ptr()).unwrap()) },
                        Value::i32(stats.allocated_bytes as i32));

                let map_ptr = self.gc.lock().allocate(map);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(map_ptr.as_ptr()).unwrap()) }
            }

            reflect::FIND_INSTANCES => {
                // findInstances(classId) - find all live instances of a class
                if args.is_empty() {
                    return Err(VmError::RuntimeError("findInstances requires 1 argument".to_string()));
                }
                let class_id = value_to_f64(args[0])? as usize;

                let gc = self.gc.lock();
                let mut instances = Vec::new();

                for header_ptr in gc.heap().iter_allocations() {
                    let header = unsafe { &*header_ptr };
                    // Check if this is an Object with matching class_id
                    if header.type_id() == std::any::TypeId::of::<Object>() {
                        let obj_ptr = unsafe { header_ptr.add(1) as *const Object };
                        let obj = unsafe { &*obj_ptr };
                        if obj.class_id == class_id {
                            let value = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(obj_ptr as *mut Object).unwrap())
                            };
                            instances.push(value);
                        }
                    }
                }
                drop(gc);

                let mut arr = Array::new(0, 0);
                for inst in instances {
                    arr.push(inst);
                }
                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }

            // ===== Phase 8: Stack Introspection =====

            reflect::GET_CALL_STACK => {
                // getCallStack() - get current call frames
                let call_stack = task.get_call_stack();
                let stack_frames: Vec<_> = stack.frames().collect();

                let mut arr = Array::new(0, 0);

                for (i, &func_id) in call_stack.iter().enumerate() {
                    let mut frame_map = MapObject::new();

                    // Function name
                    let func_name = module.functions.get(func_id)
                        .map(|f| f.name.clone())
                        .unwrap_or_else(|| format!("<function_{}>", func_id));

                    let name_key = RayaString::new("functionName".to_string());
                    let name_key_ptr = self.gc.lock().allocate(name_key);
                    let name_val = RayaString::new(func_name);
                    let name_val_ptr = self.gc.lock().allocate(name_val);
                    frame_map.set(
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(name_key_ptr.as_ptr()).unwrap()) },
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(name_val_ptr.as_ptr()).unwrap()) }
                    );

                    // Frame index
                    let idx_key = RayaString::new("frameIndex".to_string());
                    let idx_key_ptr = self.gc.lock().allocate(idx_key);
                    frame_map.set(
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(idx_key_ptr.as_ptr()).unwrap()) },
                        Value::i32(i as i32)
                    );

                    // Add frame info if available
                    if let Some(frame) = stack_frames.get(i) {
                        let locals_key = RayaString::new("localCount".to_string());
                        let locals_key_ptr = self.gc.lock().allocate(locals_key);
                        frame_map.set(
                            unsafe { Value::from_ptr(std::ptr::NonNull::new(locals_key_ptr.as_ptr()).unwrap()) },
                            Value::i32(frame.local_count as i32)
                        );

                        let args_key = RayaString::new("argCount".to_string());
                        let args_key_ptr = self.gc.lock().allocate(args_key);
                        frame_map.set(
                            unsafe { Value::from_ptr(std::ptr::NonNull::new(args_key_ptr.as_ptr()).unwrap()) },
                            Value::i32(frame.arg_count as i32)
                        );
                    }

                    let frame_ptr = self.gc.lock().allocate(frame_map);
                    arr.push(unsafe { Value::from_ptr(std::ptr::NonNull::new(frame_ptr.as_ptr()).unwrap()) });
                }

                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }

            reflect::GET_LOCALS => {
                // getLocals(frameIndex?) - get local variables
                let frame_index = if !args.is_empty() {
                    value_to_f64(args[0])? as usize
                } else {
                    0
                };

                let frames: Vec<_> = stack.frames().collect();
                if let Some(frame) = frames.get(frame_index) {
                    let mut locals_arr = Array::new(0, 0);

                    for i in 0..frame.local_count {
                        if let Ok(local) = stack.load_local(i) {
                            locals_arr.push(local);
                        } else {
                            locals_arr.push(Value::null());
                        }
                    }

                    let arr_ptr = self.gc.lock().allocate(locals_arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
                } else {
                    Value::null()
                }
            }

            reflect::GET_SOURCE_LOCATION => {
                // getSourceLocation(classId, methodName) - source location
                // Args: classId (number), methodName (string)
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "getSourceLocation requires 2 arguments: classId, methodName".to_string()
                    ));
                }

                let class_id = args[0].as_i32().ok_or_else(|| {
                    VmError::RuntimeError("getSourceLocation: classId must be a number".to_string())
                })? as usize;

                let method_name = if let Some(ptr) = unsafe { args[1].as_ptr::<RayaString>() } {
                    let s = unsafe { &*ptr.as_ptr() };
                    s.data.clone()
                } else {
                    return Err(VmError::RuntimeError(
                        "getSourceLocation: methodName must be a string".to_string()
                    ));
                };

                // Check if module has debug info
                if !module.has_debug_info() {
                    // Return null if no debug info available
                    Value::null()
                } else if let Some(ref debug_info) = module.debug_info {
                    // Find the class and method
                    let class_def = module.classes.get(class_id);
                    if class_def.is_none() {
                        Value::null()
                    } else {
                        let class_def = class_def.unwrap();
                        // Find the method by name
                        let method = class_def.methods.iter()
                            .find(|m| m.name == method_name);

                        if let Some(method) = method {
                            let function_id = method.function_id;

                            // Get function debug info
                            if let Some(func_debug) = debug_info.functions.get(function_id) {
                                // Get source file path
                                let source_file = debug_info
                                    .get_source_file(func_debug.source_file_index)
                                    .unwrap_or("unknown");

                                // Create a SourceLocation object with: file, line, column
                                let mut result_obj = Object::new(0, 3);

                                // Set file
                                let file_str = RayaString::new(source_file.to_string());
                                let file_ptr = self.gc.lock().allocate(file_str);
                                result_obj.set_field(0, unsafe {
                                    Value::from_ptr(std::ptr::NonNull::new(file_ptr.as_ptr()).unwrap())
                                });

                                // Set line (1-indexed)
                                result_obj.set_field(1, Value::i32(func_debug.start_line as i32));

                                // Set column (1-indexed)
                                result_obj.set_field(2, Value::i32(func_debug.start_column as i32));

                                let result_ptr = self.gc.lock().allocate(result_obj);
                                unsafe { Value::from_ptr(std::ptr::NonNull::new(result_ptr.as_ptr()).unwrap()) }
                            } else {
                                Value::null()
                            }
                        } else {
                            // Method not found
                            Value::null()
                        }
                    }
                } else {
                    Value::null()
                }
            }

            // ===== Phase 8: Serialization Helpers =====

            reflect::TO_JSON => {
                // toJSON(obj) - JSON string representation
                if args.is_empty() {
                    return Err(VmError::RuntimeError("toJSON requires 1 argument".to_string()));
                }
                let target = args[0];
                let mut visited = Vec::new();
                let json = self.value_to_json(target, &mut visited)?;
                let s = RayaString::new(json);
                let s_ptr = self.gc.lock().allocate(s);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(s_ptr.as_ptr()).unwrap()) }
            }

            reflect::GET_ENUMERABLE_KEYS => {
                // getEnumerableKeys(obj) - get field names
                if args.is_empty() {
                    return Err(VmError::RuntimeError("getEnumerableKeys requires 1 argument".to_string()));
                }
                let target = args[0];

                let mut arr = Array::new(0, 0);

                if let Some(class_id) = crate::vm::reflect::get_class_id(target) {
                    let class_metadata = self.class_metadata.read();
                    if let Some(meta) = class_metadata.get(class_id) {
                        for name in &meta.field_names {
                            let s = RayaString::new(name.clone());
                            let s_ptr = self.gc.lock().allocate(s);
                            arr.push(unsafe { Value::from_ptr(std::ptr::NonNull::new(s_ptr.as_ptr()).unwrap()) });
                        }
                    }
                }

                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }

            reflect::IS_CIRCULAR => {
                // isCircular(obj) - check for circular references
                if args.is_empty() {
                    return Err(VmError::RuntimeError("isCircular requires 1 argument".to_string()));
                }
                let target = args[0];
                let mut visited = Vec::new();
                let is_circular = self.check_circular(target, &mut visited);
                Value::bool(is_circular)
            }

            // ===== Decorator Registration (Phase 3/4 codegen) =====

            reflect::REGISTER_CLASS_DECORATOR => {
                // registerClassDecorator(classId, decoratorName)
                // Metadata registration - currently a no-op, decorator function does the work
                // The DecoratorRegistry is populated by the codegen emitted registration calls
                // which use global state. For now, we just acknowledge the call.
                Value::null()
            }

            reflect::REGISTER_METHOD_DECORATOR => {
                // registerMethodDecorator(classId, methodName, decoratorName)
                // Metadata registration - currently a no-op
                Value::null()
            }

            reflect::REGISTER_FIELD_DECORATOR => {
                // registerFieldDecorator(classId, fieldName, decoratorName)
                // Metadata registration - currently a no-op
                Value::null()
            }

            reflect::REGISTER_PARAMETER_DECORATOR => {
                // registerParameterDecorator(classId, methodName, paramIndex, decoratorName)
                // Metadata registration - currently a no-op
                Value::null()
            }

            reflect::GET_CLASS_DECORATORS => {
                // getClassDecorators(classId) -> get decorators applied to class
                // Returns empty array for now - full implementation uses DecoratorRegistry
                let arr = Array::new(0, 0);
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            reflect::GET_METHOD_DECORATORS => {
                // getMethodDecorators(classId, methodName) -> get decorators on method
                let arr = Array::new(0, 0);
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            reflect::GET_FIELD_DECORATORS => {
                // getFieldDecorators(classId, fieldName) -> get decorators on field
                let arr = Array::new(0, 0);
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            // ── BytecodeBuilder (Phase 15, delegated from std:runtime Phase 6) ──

            reflect::NEW_BYTECODE_BUILDER => {
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "BytecodeBuilder requires 3 arguments (name, paramCount, returnType)".to_string()
                    ));
                }
                let name = get_string(args[0].clone())?;
                let param_count = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("paramCount must be a number".to_string()))?
                    as usize;
                let return_type = get_string(args[2].clone())?;
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder_id = registry.create_builder(name, param_count, return_type);
                Value::i32(builder_id as i32)
            }

            reflect::BUILDER_EMIT => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "emit requires at least 2 arguments (builderId, opcode)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let opcode = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("opcode must be a number".to_string()))?
                    as u8;
                let operands: Vec<u8> = args[2..].iter()
                    .filter_map(|v| v.as_i32().map(|n| n as u8))
                    .collect();
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                builder.emit(opcode, &operands)?;
                Value::null()
            }

            reflect::BUILDER_EMIT_PUSH => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "emitPush requires 2 arguments (builderId, value)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let value = args[1];
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                if value.is_null() {
                    builder.emit_push_null()?;
                } else if let Some(b) = value.as_bool() {
                    builder.emit_push_bool(b)?;
                } else if let Some(i) = value.as_i32() {
                    builder.emit_push_i32(i)?;
                } else if let Some(f) = value.as_f64() {
                    builder.emit_push_f64(f)?;
                } else {
                    builder.emit_push_i32(0)?;
                }
                Value::null()
            }

            reflect::BUILDER_DEFINE_LABEL => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "defineLabel requires 1 argument (builderId)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                let label = builder.define_label();
                Value::i32(label.id as i32)
            }

            reflect::BUILDER_MARK_LABEL => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "markLabel requires 2 arguments (builderId, labelId)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let label_id = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("labelId must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                builder.mark_label(crate::vm::reflect::Label { id: label_id })?;
                Value::null()
            }

            reflect::BUILDER_EMIT_JUMP => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "emitJump requires 2 arguments (builderId, labelId)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let label_id = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("labelId must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                builder.emit_jump(crate::vm::reflect::Label { id: label_id })?;
                Value::null()
            }

            reflect::BUILDER_EMIT_JUMP_IF => {
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "emitJumpIf requires 3 arguments (builderId, labelId, ifTrue)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let label_id = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("labelId must be a number".to_string()))?
                    as usize;
                let if_true = args[2].as_bool().unwrap_or(false);
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                if if_true {
                    builder.emit_jump_if_true(crate::vm::reflect::Label { id: label_id })?;
                } else {
                    builder.emit_jump_if_false(crate::vm::reflect::Label { id: label_id })?;
                }
                Value::null()
            }

            reflect::BUILDER_DECLARE_LOCAL => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "declareLocal requires 2 arguments (builderId, typeName)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let type_name = get_string(args[1].clone())?;
                let stack_type = match type_name.as_str() {
                    "number" | "i32" | "i64" | "int" => crate::vm::reflect::StackType::Integer,
                    "f64" | "float" => crate::vm::reflect::StackType::Float,
                    "boolean" | "bool" => crate::vm::reflect::StackType::Boolean,
                    "string" => crate::vm::reflect::StackType::String,
                    "null" => crate::vm::reflect::StackType::Null,
                    _ => crate::vm::reflect::StackType::Object,
                };
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                let index = builder.declare_local(None, stack_type)?;
                Value::i32(index as i32)
            }

            reflect::BUILDER_EMIT_LOAD_LOCAL => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "emitLoadLocal requires 2 arguments (builderId, index)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let index = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("index must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                builder.emit_load_local(index)?;
                Value::null()
            }

            reflect::BUILDER_EMIT_STORE_LOCAL => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "emitStoreLocal requires 2 arguments (builderId, index)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let index = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("index must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                builder.emit_store_local(index)?;
                Value::null()
            }

            reflect::BUILDER_EMIT_CALL => {
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "emitCall requires 3 arguments (builderId, functionId, argCount)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let function_id = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("functionId must be a number".to_string()))?
                    as u32;
                let arg_count = args[2].as_i32()
                    .ok_or_else(|| VmError::TypeError("argCount must be a number".to_string()))?
                    as u16;
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                builder.emit_call(function_id, arg_count)?;
                Value::null()
            }

            reflect::BUILDER_EMIT_RETURN => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "emitReturn requires at least 1 argument (builderId)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let has_value = args.get(1).and_then(|v| v.as_bool()).unwrap_or(true);
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                if has_value {
                    builder.emit_return()?;
                } else {
                    builder.emit_return_void()?;
                }
                Value::null()
            }

            reflect::BUILDER_VALIDATE => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "validate requires 1 argument (builderId)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                let result = builder.validate();
                Value::bool(result.is_valid)
            }

            reflect::BUILDER_BUILD_FUNCTION => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "build requires 1 argument (builderId)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                let func = builder.build()?;
                let func_id = func.function_id;
                registry.register_function(func);
                Value::i32(func_id as i32)
            }

            // ===== Phase 14: ClassBuilder (0x0DE0-0x0DE6) =====

            reflect::NEW_CLASS_BUILDER => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "newClassBuilder requires 1 argument (name)".to_string()
                    ));
                }
                let name = get_string(args[0].clone())?;
                let mut registry = crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                let builder_id = registry.create_builder(name);
                Value::i32(builder_id as i32)
            }

            reflect::BUILDER_ADD_FIELD => {
                if args.len() < 5 {
                    return Err(VmError::RuntimeError(
                        "addField requires 5 arguments (builderId, name, typeName, isStatic, isReadonly)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let name = get_string(args[1].clone())?;
                let type_name = get_string(args[2].clone())?;
                let is_static = args[3].as_bool().unwrap_or(false);
                let is_readonly = args[4].as_bool().unwrap_or(false);
                let mut registry = crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id)))?;
                builder.add_field(name, &type_name, is_static, is_readonly)?;
                Value::null()
            }

            reflect::BUILDER_ADD_METHOD => {
                if args.len() < 5 {
                    return Err(VmError::RuntimeError(
                        "addMethod requires 5 arguments (builderId, name, functionId, isStatic, isAsync)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let name = get_string(args[1].clone())?;
                let function_id = args[2].as_i32()
                    .ok_or_else(|| VmError::TypeError("functionId must be a number".to_string()))?
                    as usize;
                let is_static = args[3].as_bool().unwrap_or(false);
                let is_async = args[4].as_bool().unwrap_or(false);
                let mut registry = crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id)))?;
                builder.add_method(name, function_id, is_static, is_async)?;
                Value::null()
            }

            reflect::BUILDER_SET_CONSTRUCTOR => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "setConstructor requires 2 arguments (builderId, functionId)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let function_id = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("functionId must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id)))?;
                builder.set_constructor(function_id)?;
                Value::null()
            }

            reflect::BUILDER_SET_PARENT => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "setParent requires 2 arguments (builderId, parentClassId)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let parent_id = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("parentClassId must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id)))?;
                builder.set_parent(parent_id)?;
                Value::null()
            }

            reflect::BUILDER_ADD_INTERFACE => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "addInterface requires 2 arguments (builderId, interfaceName)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let interface_name = get_string(args[1].clone())?;
                let mut registry = crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id)))?;
                builder.add_interface(interface_name)?;
                Value::null()
            }

            reflect::BUILDER_BUILD => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "build requires 1 argument (builderId)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;

                let builder = {
                    let mut registry = crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                    registry.remove(builder_id)
                        .ok_or_else(|| VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id)))?
                };

                let def = builder.to_definition();
                let mut classes_write = self.classes.write();
                let next_id = classes_write.next_class_id();
                let mut dyn_builder = crate::vm::reflect::DynamicClassBuilder::new(next_id);

                let (new_class, new_metadata) = if let Some(parent_id) = builder.parent_id {
                    let parent = classes_write.get_class(parent_id)
                        .ok_or_else(|| VmError::RuntimeError(format!("Parent class {} not found", parent_id)))?
                        .clone();
                    drop(classes_write);

                    let class_metadata_guard = self.class_metadata.read();
                    let parent_metadata = class_metadata_guard.get(parent_id).cloned();
                    drop(class_metadata_guard);

                    let result = dyn_builder.create_subclass(
                        builder.name,
                        &parent,
                        parent_metadata.as_ref(),
                        &def,
                    );
                    classes_write = self.classes.write();
                    result
                } else {
                    dyn_builder.create_root_class(builder.name, &def)
                };

                let new_class_id = new_class.id;
                classes_write.register_class(new_class);
                drop(classes_write);

                let mut class_metadata_write = self.class_metadata.write();
                class_metadata_write.register(new_class_id, new_metadata);
                drop(class_metadata_write);

                Value::i32(new_class_id as i32)
            }

            // ===== Phase 17: DynamicModule (0x0E10-0x0E15) =====

            reflect::CREATE_MODULE => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "createModule requires 1 argument (name)".to_string()
                    ));
                }
                let name = get_string(args[0].clone())?;
                let mut registry = crate::vm::builtins::handlers::reflect::DYNAMIC_MODULE_REGISTRY.lock();
                let module_id = registry.create_module(name)?;
                Value::i32(module_id as i32)
            }

            reflect::MODULE_ADD_FUNCTION => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "addFunction requires 2 arguments (moduleId, functionId)".to_string()
                    ));
                }
                let module_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("moduleId must be a number".to_string()))?
                    as usize;
                // Cast i32 → u32 → usize to preserve bit pattern (function IDs start at 0x8000_0000)
                let function_id = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("functionId must be a number".to_string()))?
                    as u32 as usize;

                let bytecode_registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let func = bytecode_registry.get_function(function_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("Function {} not found", function_id)))?
                    .clone();
                drop(bytecode_registry);

                let mut registry = crate::vm::builtins::handlers::reflect::DYNAMIC_MODULE_REGISTRY.lock();
                let module = registry.get_mut(module_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("Module {} not found", module_id)))?;
                module.add_function(func)?;
                Value::null()
            }

            reflect::MODULE_ADD_CLASS => {
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "addClass requires 3 arguments (moduleId, classId, name)".to_string()
                    ));
                }
                let module_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("moduleId must be a number".to_string()))?
                    as usize;
                let class_id = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("classId must be a number".to_string()))?
                    as usize;
                let name = get_string(args[2].clone())?;
                let mut registry = crate::vm::builtins::handlers::reflect::DYNAMIC_MODULE_REGISTRY.lock();
                let module = registry.get_mut(module_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("Module {} not found", module_id)))?;
                module.add_class(class_id, class_id, name)?;
                Value::null()
            }

            reflect::MODULE_ADD_GLOBAL => {
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "addGlobal requires 3 arguments (moduleId, name, value)".to_string()
                    ));
                }
                let module_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("moduleId must be a number".to_string()))?
                    as usize;
                let name = get_string(args[1].clone())?;
                let value = args[2];
                let mut registry = crate::vm::builtins::handlers::reflect::DYNAMIC_MODULE_REGISTRY.lock();
                let module = registry.get_mut(module_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("Module {} not found", module_id)))?;
                module.add_global(name, value)?;
                Value::null()
            }

            reflect::MODULE_SEAL => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "seal requires 1 argument (moduleId)".to_string()
                    ));
                }
                let module_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("moduleId must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::DYNAMIC_MODULE_REGISTRY.lock();
                let module = registry.get_mut(module_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("Module {} not found", module_id)))?;
                module.seal()?;
                Value::null()
            }

            reflect::MODULE_LINK => {
                // Stub: full import resolution not yet implemented
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "link requires 1 argument (moduleId)".to_string()
                    ));
                }
                Value::null()
            }

            _ => {
                return Err(VmError::RuntimeError(format!(
                    "Reflect method {:#06x} not yet implemented",
                    method_id
                )));
            }
        };

        stack.push(result)?;
        Ok(())
    }

    /// Helper: Inspect a value recursively with depth limit
    fn inspect_value(&self, value: Value, depth: usize, max_depth: usize) -> Result<String, VmError> {
        if depth > max_depth {
            return Ok("...".to_string());
        }

        if value.is_null() {
            return Ok("null".to_string());
        }

        if let Some(b) = value.as_bool() {
            return Ok(if b { "true" } else { "false" }.to_string());
        }

        if let Some(i) = value.as_i32() {
            return Ok(i.to_string());
        }

        if let Some(f) = value.as_f64() {
            return Ok(f.to_string());
        }

        if !value.is_ptr() {
            return Ok("<unknown>".to_string());
        }

        // String
        if let Some(ptr) = unsafe { value.as_ptr::<RayaString>() } {
            let s = unsafe { &*ptr.as_ptr() };
            return Ok(format!("\"{}\"", s.data.replace('\\', "\\\\").replace('"', "\\\"")));
        }

        // Array
        if let Some(ptr) = unsafe { value.as_ptr::<Array>() } {
            let arr = unsafe { &*ptr.as_ptr() };
            if depth >= max_depth {
                return Ok(format!("[Array({})]", arr.len()));
            }
            let mut items = Vec::new();
            for i in 0..arr.len().min(10) {
                items.push(self.inspect_value(arr.get(i).unwrap_or(Value::null()), depth + 1, max_depth)?);
            }
            if arr.len() > 10 {
                items.push(format!("... {} more", arr.len() - 10));
            }
            return Ok(format!("[{}]", items.join(", ")));
        }

        // Object
        if let Some(class_id) = crate::vm::reflect::get_class_id(value) {
            let classes = self.classes.read();
            let class_name = classes.get_class(class_id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| format!("Class{}", class_id));
            drop(classes);

            if depth >= max_depth {
                return Ok(format!("{} {{}}", class_name));
            }

            let class_metadata = self.class_metadata.read();
            if let Some(meta) = class_metadata.get(class_id) {
                let obj_ptr = unsafe { value.as_ptr::<Object>() };
                if let Some(ptr) = obj_ptr {
                    let obj = unsafe { &*ptr.as_ptr() };
                    let mut fields = Vec::new();
                    for (i, name) in meta.field_names.iter().enumerate() {
                        if let Some(&field_val) = obj.fields.get(i) {
                            let val_str = self.inspect_value(field_val, depth + 1, max_depth)?;
                            fields.push(format!("{}: {}", name, val_str));
                        }
                    }
                    return Ok(format!("{} {{ {} }}", class_name, fields.join(", ")));
                }
            }
            return Ok(format!("{} {{ ... }}", class_name));
        }

        Ok("<ptr>".to_string())
    }

    /// Helper: Calculate retained size by traversing references
    fn calculate_retained_size(&self, value: Value, visited: &mut std::collections::HashSet<usize>) -> usize {
        if !value.is_ptr() || value.is_null() {
            return 8; // primitive size
        }

        // Get object ID for cycle detection
        let obj_id = if let Some(ptr) = unsafe { value.as_ptr::<u8>() } {
            ptr.as_ptr() as usize
        } else {
            return 8;
        };

        // Already visited - don't count again
        if visited.contains(&obj_id) {
            return 0;
        }
        visited.insert(obj_id);

        // Calculate size based on type
        if let Some(ptr) = unsafe { value.as_ptr::<Object>() } {
            let obj = unsafe { &*ptr.as_ptr() };
            let mut size = std::mem::size_of::<Object>() + obj.fields.len() * 8;
            // Add retained size of referenced objects
            for &field in &obj.fields {
                size += self.calculate_retained_size(field, visited);
            }
            return size;
        }

        if let Some(ptr) = unsafe { value.as_ptr::<Array>() } {
            let arr = unsafe { &*ptr.as_ptr() };
            let mut size = std::mem::size_of::<Array>() + arr.len() * 8;
            // Add retained size of elements
            for i in 0..arr.len() {
                if let Some(elem) = arr.get(i) {
                    size += self.calculate_retained_size(elem, visited);
                }
            }
            return size;
        }

        if let Some(ptr) = unsafe { value.as_ptr::<RayaString>() } {
            let s = unsafe { &*ptr.as_ptr() };
            return std::mem::size_of::<RayaString>() + s.data.len();
        }

        8 // default
    }

    /// Helper: Collect direct references from an object
    fn collect_references(&self, value: Value, refs: &mut Vec<Value>) {
        if !value.is_ptr() || value.is_null() {
            return;
        }

        if let Some(ptr) = unsafe { value.as_ptr::<Object>() } {
            let obj = unsafe { &*ptr.as_ptr() };
            for &field in &obj.fields {
                if field.is_ptr() && !field.is_null() {
                    refs.push(field);
                }
            }
        } else if let Some(ptr) = unsafe { value.as_ptr::<Array>() } {
            let arr = unsafe { &*ptr.as_ptr() };
            for i in 0..arr.len() {
                if let Some(elem) = arr.get(i) {
                    if elem.is_ptr() && !elem.is_null() {
                        refs.push(elem);
                    }
                }
            }
        }
    }

    /// Helper: Convert value to JSON string
    fn value_to_json(&self, value: Value, visited: &mut Vec<usize>) -> Result<String, VmError> {
        if value.is_null() {
            return Ok("null".to_string());
        }

        if let Some(b) = value.as_bool() {
            return Ok(if b { "true" } else { "false" }.to_string());
        }

        if let Some(i) = value.as_i32() {
            return Ok(i.to_string());
        }

        if let Some(f) = value.as_f64() {
            if f.is_nan() || f.is_infinite() {
                return Ok("null".to_string());
            }
            return Ok(f.to_string());
        }

        if !value.is_ptr() {
            return Ok("null".to_string());
        }

        // Check for circular reference
        let obj_id = if let Some(ptr) = unsafe { value.as_ptr::<u8>() } {
            ptr.as_ptr() as usize
        } else {
            0
        };

        if obj_id != 0 && visited.contains(&obj_id) {
            return Ok("\"[Circular]\"".to_string());
        }
        visited.push(obj_id);

        // String
        if let Some(ptr) = unsafe { value.as_ptr::<RayaString>() } {
            let s = unsafe { &*ptr.as_ptr() };
            visited.pop();
            return Ok(format!("\"{}\"",
                s.data.replace('\\', "\\\\")
                      .replace('"', "\\\"")
                      .replace('\n', "\\n")
                      .replace('\r', "\\r")
                      .replace('\t', "\\t")));
        }

        // Array
        if let Some(ptr) = unsafe { value.as_ptr::<Array>() } {
            let arr = unsafe { &*ptr.as_ptr() };
            let mut items = Vec::new();
            for i in 0..arr.len() {
                if let Some(elem) = arr.get(i) {
                    items.push(self.value_to_json(elem, visited)?);
                }
            }
            visited.pop();
            return Ok(format!("[{}]", items.join(",")));
        }

        // Object
        if let Some(class_id) = crate::vm::reflect::get_class_id(value) {
            let class_metadata = self.class_metadata.read();
            if let Some(meta) = class_metadata.get(class_id) {
                let obj_ptr = unsafe { value.as_ptr::<Object>() };
                if let Some(ptr) = obj_ptr {
                    let obj = unsafe { &*ptr.as_ptr() };
                    let mut fields = Vec::new();
                    for (i, name) in meta.field_names.iter().enumerate() {
                        if let Some(&field_val) = obj.fields.get(i) {
                            let val_json = self.value_to_json(field_val, visited)?;
                            fields.push(format!("\"{}\":{}", name, val_json));
                        }
                    }
                    visited.pop();
                    return Ok(format!("{{{}}}", fields.join(",")));
                }
            }
            visited.pop();
            return Ok("{}".to_string());
        }

        visited.pop();
        Ok("null".to_string())
    }

    /// Helper: Check for circular references
    fn check_circular(&self, value: Value, visited: &mut Vec<usize>) -> bool {
        if !value.is_ptr() || value.is_null() {
            return false;
        }

        let obj_id = if let Some(ptr) = unsafe { value.as_ptr::<u8>() } {
            ptr.as_ptr() as usize
        } else {
            return false;
        };

        // Found a cycle
        if visited.contains(&obj_id) {
            return true;
        }
        visited.push(obj_id);

        // Check Object fields
        if let Some(ptr) = unsafe { value.as_ptr::<Object>() } {
            let obj = unsafe { &*ptr.as_ptr() };
            for &field in &obj.fields {
                if self.check_circular(field, visited) {
                    return true;
                }
            }
        }

        // Check Array elements
        if let Some(ptr) = unsafe { value.as_ptr::<Array>() } {
            let arr = unsafe { &*ptr.as_ptr() };
            for i in 0..arr.len() {
                if let Some(elem) = arr.get(i) {
                    if self.check_circular(elem, visited) {
                        return true;
                    }
                }
            }
        }

        visited.pop();
        false
    }

    /// Helper: Convert ObjectSnapshot to a Raya Value (Object)
    fn snapshot_to_value(&self, snapshot: &ObjectSnapshot) -> Value {
        // Create an object with snapshot fields:
        // - class_name: string
        // - identity: number
        // - timestamp: number
        // - fields: object mapping field names to values
        let mut obj = Object::new(0, 4); // class_id 0 for dynamic object, 4 fields

        // Store class_name
        let class_name_str = RayaString::new(snapshot.class_name.clone());
        let class_name_ptr = self.gc.lock().allocate(class_name_str);
        let class_name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(class_name_ptr.as_ptr()).unwrap()) };
        obj.set_field(0, class_name_val);

        // Store identity
        obj.set_field(1, Value::i32(snapshot.identity as i32));

        // Store timestamp
        obj.set_field(2, Value::i32(snapshot.timestamp as i32));

        // Create fields object
        let fields_obj = self.snapshot_fields_to_value(&snapshot.fields);
        obj.set_field(3, fields_obj);

        let obj_ptr = self.gc.lock().allocate(obj);
        unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) }
    }

    /// Helper: Convert snapshot fields HashMap to a Raya Value (Object)
    fn snapshot_fields_to_value(&self, fields: &std::collections::HashMap<String, crate::vm::reflect::FieldSnapshot>) -> Value {
        // Create an object with field count matching the number of fields
        let field_count = fields.len();
        let mut obj = Object::new(0, field_count);

        // Sort fields by name for consistent ordering
        let mut field_names: Vec<_> = fields.keys().collect();
        field_names.sort();

        for (i, name) in field_names.iter().enumerate() {
            if let Some(field) = fields.get(*name) {
                // Create a field info object with: name, value, type_name
                let mut field_obj = Object::new(0, 3);

                // Field name
                let name_str = RayaString::new(field.name.clone());
                let name_ptr = self.gc.lock().allocate(name_str);
                let name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(name_ptr.as_ptr()).unwrap()) };
                field_obj.set_field(0, name_val);

                // Field value (converted from SnapshotValue)
                let val = self.snapshot_value_to_value(&field.value);
                field_obj.set_field(1, val);

                // Type name
                let type_str = RayaString::new(field.type_name.clone());
                let type_ptr = self.gc.lock().allocate(type_str);
                let type_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(type_ptr.as_ptr()).unwrap()) };
                field_obj.set_field(2, type_val);

                let field_ptr = self.gc.lock().allocate(field_obj);
                obj.set_field(i, unsafe { Value::from_ptr(std::ptr::NonNull::new(field_ptr.as_ptr()).unwrap()) });
            }
        }

        let obj_ptr = self.gc.lock().allocate(obj);
        unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) }
    }

    /// Helper: Convert SnapshotValue to a Raya Value
    fn snapshot_value_to_value(&self, snapshot_val: &SnapshotValue) -> Value {
        match snapshot_val {
            SnapshotValue::Null => Value::null(),
            SnapshotValue::Boolean(b) => Value::bool(*b),
            SnapshotValue::Integer(i) => Value::i32(*i),
            SnapshotValue::Float(f) => Value::f64(*f),
            SnapshotValue::String(s) => {
                let raya_str = RayaString::new(s.clone());
                let str_ptr = self.gc.lock().allocate(raya_str);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(str_ptr.as_ptr()).unwrap()) }
            }
            SnapshotValue::ObjectRef(id) => {
                // Return the object ID as an integer for reference tracking
                Value::i32(*id as i32)
            }
            SnapshotValue::Array(elements) => {
                let mut arr = Array::new(0, elements.len());
                for elem in elements {
                    arr.push(self.snapshot_value_to_value(elem));
                }
                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }
            SnapshotValue::Object(nested_snapshot) => {
                // Recursively convert nested snapshot
                self.snapshot_to_value(nested_snapshot)
            }
        }
    }

    /// Helper: Convert ObjectDiff to a Raya Value (Object)
    fn diff_to_value(&self, diff: &ObjectDiff) -> Value {
        // Create an object with diff fields:
        // - added: string[] (field names added)
        // - removed: string[] (field names removed)
        // - changed: object mapping field name to { old, new }
        let mut obj = Object::new(0, 3);

        // Create added array
        let mut added_arr = Array::new(0, diff.added.len());
        for name in &diff.added {
            let name_str = RayaString::new(name.clone());
            let name_ptr = self.gc.lock().allocate(name_str);
            added_arr.push(unsafe { Value::from_ptr(std::ptr::NonNull::new(name_ptr.as_ptr()).unwrap()) });
        }
        let added_ptr = self.gc.lock().allocate(added_arr);
        obj.set_field(0, unsafe { Value::from_ptr(std::ptr::NonNull::new(added_ptr.as_ptr()).unwrap()) });

        // Create removed array
        let mut removed_arr = Array::new(0, diff.removed.len());
        for name in &diff.removed {
            let name_str = RayaString::new(name.clone());
            let name_ptr = self.gc.lock().allocate(name_str);
            removed_arr.push(unsafe { Value::from_ptr(std::ptr::NonNull::new(name_ptr.as_ptr()).unwrap()) });
        }
        let removed_ptr = self.gc.lock().allocate(removed_arr);
        obj.set_field(1, unsafe { Value::from_ptr(std::ptr::NonNull::new(removed_ptr.as_ptr()).unwrap()) });

        // Create changed object
        let changed_obj = self.diff_changes_to_value(&diff.changed);
        obj.set_field(2, changed_obj);

        let obj_ptr = self.gc.lock().allocate(obj);
        unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) }
    }

    /// Helper: Convert diff changes HashMap to a Raya Value (Object)
    fn diff_changes_to_value(&self, changes: &std::collections::HashMap<String, crate::vm::reflect::ValueChange>) -> Value {
        let change_count = changes.len();
        let mut obj = Object::new(0, change_count);

        // Sort changes by name for consistent ordering
        let mut change_names: Vec<_> = changes.keys().collect();
        change_names.sort();

        for (i, name) in change_names.iter().enumerate() {
            if let Some(change) = changes.get(*name) {
                // Create a change object with: fieldName, old, new
                let mut change_obj = Object::new(0, 3);

                // Field name
                let name_str = RayaString::new((*name).clone());
                let name_ptr = self.gc.lock().allocate(name_str);
                change_obj.set_field(0, unsafe { Value::from_ptr(std::ptr::NonNull::new(name_ptr.as_ptr()).unwrap()) });

                // Old value
                let old_val = self.snapshot_value_to_value(&change.old);
                change_obj.set_field(1, old_val);

                // New value
                let new_val = self.snapshot_value_to_value(&change.new);
                change_obj.set_field(2, new_val);

                let change_ptr = self.gc.lock().allocate(change_obj);
                obj.set_field(i, unsafe { Value::from_ptr(std::ptr::NonNull::new(change_ptr.as_ptr()).unwrap()) });
            }
        }

        let obj_ptr = self.gc.lock().allocate(obj);
        unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) }
    }

    /// Handle built-in runtime methods (std:runtime)
    ///
    /// Bridge between Interpreter's call convention (pre-popped args Vec)
    /// and the runtime handler's stack-based convention.
    fn call_runtime_method(
        &mut self,
        _task: &Arc<Task>,
        stack: &mut Stack,
        method_id: u16,
        args: Vec<Value>,
        _module: &Module,
    ) -> Result<(), VmError> {
        let ctx = RuntimeHandlerContext {
            gc: &self.gc,
        };

        // Push args back onto stack so the handler can pop them
        let arg_count = args.len();
        for arg in args {
            stack.push(arg)?;
        }

        runtime_handler(&ctx, stack, method_id, arg_count)
    }

    /// Handle built-in crypto methods (std:crypto)
    ///
    /// Bridge between Interpreter's call convention (pre-popped args Vec)
    /// and the crypto handler's stack-based convention.
    fn call_time_method(
        &mut self,
        stack: &mut Stack,
        method_id: u16,
        args: Vec<Value>,
    ) -> Result<(), VmError> {
        use crate::vm::builtins::handlers::time::call_time_method;

        // Push args back onto stack so the handler can pop them
        let arg_count = args.len();
        for arg in args {
            stack.push(arg)?;
        }

        call_time_method(stack, method_id, arg_count)
    }

}
