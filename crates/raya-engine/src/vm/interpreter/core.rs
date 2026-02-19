//! Task-based interpreter that can suspend and resume
//!
//! This interpreter executes a single task until it completes, suspends, or fails.
//! Unlike the synchronous `Vm`, this interpreter returns control to the scheduler
//! when the task needs to wait for something.

use super::execution::{ExecutionFrame, ExecutionResult, OpcodeResult, ReturnAction};
use super::reg_execution::{RegExecutionFrame, RegOpcodeResult};
use super::{ClassRegistry, SafepointCoordinator};
use crate::compiler::bytecode::reg_opcode::{RegInstr, RegOpcode};
use crate::compiler::{Module, Opcode};
use crate::vm::gc::GarbageCollector;
use crate::vm::builtins::handlers::{
    RuntimeHandlerContext,
    call_runtime_method as runtime_handler,
};
use crate::vm::object::RayaString;
use crate::vm::register_file::RegisterFile;
use crate::vm::scheduler::{SuspendReason, Task, TaskId, TaskState};
use crate::vm::stack::Stack;
use crate::vm::sync::MutexRegistry;
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
pub(in crate::vm::interpreter) fn value_to_f64(v: Value) -> Result<f64, VmError> {
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
    pub(in crate::vm::interpreter) gc: &'a parking_lot::Mutex<GarbageCollector>,

    /// Reference to the class registry
    pub(in crate::vm::interpreter) classes: &'a RwLock<ClassRegistry>,

    /// Reference to the mutex registry
    pub(in crate::vm::interpreter) mutex_registry: &'a MutexRegistry,

    /// Safepoint coordinator for GC
    pub(in crate::vm::interpreter) safepoint: &'a SafepointCoordinator,

    /// Global variables by index
    pub(in crate::vm::interpreter) globals_by_index: &'a RwLock<Vec<Value>>,

    /// Task registry (for spawn/await)
    pub(in crate::vm::interpreter) tasks: &'a Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,

    /// Global task injector for scheduling spawned tasks
    pub(in crate::vm::interpreter) injector: &'a Arc<Injector<Arc<Task>>>,

    /// Metadata store for Reflect API
    pub(in crate::vm::interpreter) metadata: &'a parking_lot::Mutex<crate::vm::reflect::MetadataStore>,

    /// Class metadata registry for reflection (field/method names)
    pub(in crate::vm::interpreter) class_metadata: &'a RwLock<crate::vm::reflect::ClassMetadataRegistry>,

    /// External native call handler (stdlib implementation)
    #[allow(dead_code)]
    pub(in crate::vm::interpreter) native_handler: &'a Arc<dyn NativeHandler>,

    /// Resolved native functions for ModuleNativeCall dispatch
    pub(in crate::vm::interpreter) resolved_natives: &'a RwLock<crate::vm::native_registry::ResolvedNatives>,

    /// IO submission sender for NativeCallResult::Suspend (None in tests without reactor)
    pub(in crate::vm::interpreter) io_submit_tx: Option<&'a crossbeam::channel::Sender<crate::vm::scheduler::IoSubmission>>,

    /// Maximum consecutive preemptions before killing a task
    pub(in crate::vm::interpreter) max_preemptions: u32,
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
        resolved_natives: &'a RwLock<crate::vm::native_registry::ResolvedNatives>,
        io_submit_tx: Option<&'a crossbeam::channel::Sender<crate::vm::scheduler::IoSubmission>>,
        max_preemptions: u32,
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
            resolved_natives,
            io_submit_tx,
            max_preemptions,
        }
    }

    /// Wake a suspended task by setting its resume value and pushing it to the scheduler.
    pub(in crate::vm::interpreter) fn wake_task(&self, task_id: u64, resume_value: Value) {
        let tasks = self.tasks.read();
        let target_id = TaskId::from_u64(task_id);
        if let Some(target_task) = tasks.get(&target_id) {
            target_task.set_resume_value(resume_value);
            target_task.set_state(TaskState::Resumed);
            target_task.clear_suspend_reason();
            self.injector.push(target_task.clone());
        }
    }

    /// Execute a task until completion, suspension, or failure
    ///
    /// This is the main entry point for running a task. Uses frame-based execution:
    /// function calls push a CallFrame and continue in the same loop. This allows
    /// suspension (channel operations, await, sleep) to work at any call depth.
    pub fn run(&mut self, task: &Arc<Task>) -> ExecutionResult {
        let module = task.module();

        // Auto-detect register-based execution: if the entry function has reg_code, use it
        let current_func_id_peek = task.current_func_id();
        if let Some(func) = module.functions.get(current_func_id_peek) {
            if !func.reg_code.is_empty() {
                return self.run_register(task);
            }
        }

        // Restore execution state (supports suspend/resume)
        let mut current_func_id = task.current_func_id();
        let mut frames: Vec<ExecutionFrame> = task.take_execution_frames();

        let function = match module.functions.get(current_func_id) {
            Some(f) => f,
            None => {
                return ExecutionResult::Failed(VmError::RuntimeError(format!(
                    "Function {} not found",
                    current_func_id
                )));
            }
        };

        let mut stack_guard = task.stack().lock().unwrap();
        let mut ip = task.ip();
        let mut code: &[u8] = &function.code;
        let mut locals_base = task.current_locals_base();

        // Check if we're resuming from suspension
        if let Some(resume_value) = task.take_resume_value() {
            if let Err(e) = stack_guard.push(resume_value) {
                return ExecutionResult::Failed(e);
            }
        }

        // Check if there's a pending exception (e.g., from awaited task failure)
        if task.has_exception() {
            match self.handle_exception(task, &mut stack_guard, &mut ip) {
                Ok(()) => {}
                Err(()) => {
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
        if ip == 0 && stack_guard.depth() == 0 && frames.is_empty() {
            task.push_call_frame(current_func_id);

            for _ in 0..function.local_count {
                if let Err(e) = stack_guard.push(Value::null()) {
                    return ExecutionResult::Failed(e);
                }
            }

            let initial_args = task.take_initial_args();
            for (i, arg) in initial_args.into_iter().enumerate() {
                if i < function.local_count as usize {
                    if let Err(e) = stack_guard.set_at(i, arg) {
                        return ExecutionResult::Failed(e);
                    }
                }
            }
        }

        // Macro to save all frame state before leaving run()
        macro_rules! save_frame_state {
            () => {
                task.set_ip(ip);
                task.set_current_func_id(current_func_id);
                task.set_current_locals_base(locals_base);
                task.save_execution_frames(frames);
            };
        }

        // Helper: handle return from current function (frame pop)
        // Returns None if frame popped successfully (continue execution),
        // or Some(ExecutionResult) if this was the top-level return.
        macro_rules! handle_frame_return {
            ($return_value:expr) => {{
                let return_value = $return_value;
                // Clean up current frame's locals and operand stack
                while stack_guard.depth() > locals_base {
                    let _ = stack_guard.pop();
                }

                if let Some(frame) = frames.pop() {
                    task.pop_call_frame();
                    if frame.is_closure {
                        task.pop_closure();
                    }

                    // Restore caller's state
                    current_func_id = frame.func_id;
                    code = &module.functions[frame.func_id].code;
                    ip = frame.ip;
                    locals_base = frame.locals_base;

                    // Push appropriate value onto caller's stack
                    if !matches!(frame.return_action, ReturnAction::Discard) {
                        let push_val = match frame.return_action {
                            ReturnAction::PushReturnValue => return_value,
                            ReturnAction::PushObject(obj) => obj,
                            ReturnAction::Discard => unreachable!(),
                        };
                        match stack_guard.push(push_val) {
                            Ok(()) => None,
                            Err(e) => Some(ExecutionResult::Failed(e)),
                        }
                    } else {
                        None // Discard return value (super() call)
                    }
                } else {
                    // Top-level return - task is complete
                    Some(ExecutionResult::Completed(return_value))
                }
            }};
        }

        // Main execution loop
        loop {
            // Safepoint poll for GC
            self.safepoint.poll();

            // Check for preemption
            if task.is_preempt_requested() {
                task.clear_preempt();
                let count = task.increment_preempt_count();
                // Infinite loop detection: kill task after max_preemptions consecutive
                // preemptions without voluntary suspension
                if count >= self.max_preemptions {
                    save_frame_state!();
                    drop(stack_guard);
                    return ExecutionResult::Failed(VmError::RuntimeError(
                        format!("Maximum execution time exceeded (task preempted {} times)", count),
                    ));
                }
                save_frame_state!();
                drop(stack_guard);
                return ExecutionResult::Suspended(SuspendReason::Sleep {
                    wake_at: Instant::now(),
                });
            }

            // Check for cancellation
            if task.is_cancelled() {
                save_frame_state!();
                drop(stack_guard);
                return ExecutionResult::Failed(VmError::RuntimeError(
                    "Task cancelled".to_string(),
                ));
            }

            // Bounds check - implicit return at end of function
            if ip >= code.len() {
                let local_count = module.functions[current_func_id].local_count as usize;
                let return_value = if stack_guard.depth() > locals_base + local_count {
                    stack_guard.pop().unwrap_or(Value::null())
                } else {
                    Value::null()
                };

                if let Some(result) = handle_frame_return!(return_value) {
                    return result;
                }
                continue;
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
                &mut *stack_guard,
                &mut ip,
                code,
                module,
                opcode,
                locals_base,
                frames.len(),
            ) {
                OpcodeResult::Continue => {
                    // Continue to next instruction
                }
                OpcodeResult::Return(value) => {
                    if let Some(result) = handle_frame_return!(value) {
                        return result;
                    }
                }
                OpcodeResult::Suspend(reason) => {
                    task.reset_preempt_count();
                    save_frame_state!();
                    drop(stack_guard);
                    return ExecutionResult::Suspended(reason);
                }
                OpcodeResult::PushFrame {
                    func_id,
                    arg_count,
                    is_closure,
                    closure_val,
                    return_action,
                } => {
                    // Validate function index
                    let new_func = match module.functions.get(func_id) {
                        Some(f) => f,
                        None => {
                            return ExecutionResult::Failed(VmError::RuntimeError(format!(
                                "Invalid function index: {}",
                                func_id
                            )));
                        }
                    };
                    let new_local_count = new_func.local_count as usize;

                    // Save caller's frame
                    frames.push(ExecutionFrame {
                        func_id: current_func_id,
                        ip,
                        locals_base,
                        is_closure,
                        return_action,
                    });

                    // Push call frame for stack traces
                    task.push_call_frame(func_id);

                    // Push closure onto closure stack if needed
                    if let Some(cv) = closure_val {
                        task.push_closure(cv);
                    }

                    // Set up callee's frame on the same stack
                    // Args are already on the stack from the caller
                    if arg_count > new_local_count {
                        // More args than locals - discard extras
                        for _ in 0..(arg_count - new_local_count) {
                            let _ = stack_guard.pop();
                        }
                        locals_base = stack_guard.depth() - new_local_count;
                    } else {
                        locals_base = stack_guard.depth() - arg_count;
                        // Allocate remaining locals (initialized to null)
                        for _ in 0..(new_local_count - arg_count) {
                            if let Err(e) = stack_guard.push(Value::null()) {
                                return ExecutionResult::Failed(e);
                            }
                        }
                    }

                    // Switch to callee's code
                    current_func_id = func_id;
                    code = &module.functions[func_id].code;
                    ip = 0;
                }
                OpcodeResult::Error(e) => {
                    // Set exception on task if not already set
                    if !task.has_exception() {
                        let error_msg = e.to_string();
                        let raya_string = RayaString::new(error_msg);
                        let gc_ptr = self.gc.lock().allocate(raya_string);
                        let exc_val =
                            unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        task.set_exception(exc_val);
                    }

                    let exception = task.current_exception().unwrap_or_else(|| Value::null());

                    // Frame-aware exception handling: search for handlers,
                    // unwinding frames as needed to find a catch/finally block.
                    let mut handled = false;
                    'exception_search: loop {
                        // Process handlers that belong to the current frame depth
                        while let Some(handler) = task.peek_exception_handler() {
                            if handler.frame_count != frames.len() {
                                // This handler belongs to a different frame, stop
                                break;
                            }

                            // Unwind stack to handler's saved state
                            while stack_guard.depth() > handler.stack_size {
                                let _ = stack_guard.pop();
                            }

                            if handler.catch_offset != -1 {
                                task.pop_exception_handler();
                                task.set_caught_exception(exception);
                                task.clear_exception();
                                let _ = stack_guard.push(exception);
                                ip = handler.catch_offset as usize;
                                handled = true;
                                break 'exception_search;
                            }

                            if handler.finally_offset != -1 {
                                task.pop_exception_handler();
                                ip = handler.finally_offset as usize;
                                handled = true;
                                break 'exception_search;
                            }

                            // No catch or finally, pop and continue
                            task.pop_exception_handler();
                        }

                        // No handler in current frame — pop frame and try parent
                        if let Some(frame) = frames.pop() {
                            task.pop_call_frame();
                            if frame.is_closure {
                                task.pop_closure();
                            }
                            // Restore caller's context — don't clean stack here,
                            // the exception handler's stack_size will handle unwinding
                            current_func_id = frame.func_id;
                            code = &module.functions[frame.func_id].code;
                            ip = frame.ip;
                            locals_base = frame.locals_base;
                            // Continue searching in parent frame
                        } else {
                            // No more frames — unhandled exception
                            break;
                        }
                    }

                    if !handled {
                        task.set_ip(ip);
                        drop(stack_guard);
                        return ExecutionResult::Failed(e);
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
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        module: &Module,
        opcode: Opcode,
        locals_base: usize,
        frame_depth: usize,
    ) -> OpcodeResult {
        match opcode {
            // =========================================================
            // Stack Manipulation
            // =========================================================
            Opcode::Nop | Opcode::Pop | Opcode::Dup | Opcode::Swap => {
                self.exec_stack_ops(stack, opcode)
            }

            // =========================================================
            // Constants
            // =========================================================
            Opcode::ConstNull | Opcode::ConstTrue | Opcode::ConstFalse |
            Opcode::ConstI32 | Opcode::ConstF64 | Opcode::ConstStr => {
                self.exec_constant_ops(stack, ip, code, module, opcode)
            }

            // =========================================================
            // Variables
            // =========================================================
            Opcode::LoadLocal | Opcode::StoreLocal |
            Opcode::LoadLocal0 | Opcode::LoadLocal1 |
            Opcode::StoreLocal0 | Opcode::StoreLocal1 |
            Opcode::LoadGlobal | Opcode::StoreGlobal => {
                self.exec_variable_ops(stack, ip, code, locals_base, opcode)
            }

            // =========================================================
            // Integer and Float Arithmetic
            // =========================================================
            Opcode::Iadd | Opcode::Isub | Opcode::Imul | Opcode::Idiv |
            Opcode::Imod | Opcode::Ineg | Opcode::Ipow |
            Opcode::Ishl | Opcode::Ishr | Opcode::Iushr |
            Opcode::Iand | Opcode::Ior | Opcode::Ixor | Opcode::Inot |
            Opcode::Fadd | Opcode::Fsub | Opcode::Fmul | Opcode::Fdiv |
            Opcode::Fneg | Opcode::Fpow | Opcode::Fmod => {
                self.exec_arithmetic_ops(stack, opcode)
            }

            // =========================================================
            // Comparisons and Logical Operators
            // =========================================================
            Opcode::Ieq | Opcode::Ine | Opcode::Ilt | Opcode::Ile |
            Opcode::Igt | Opcode::Ige |
            Opcode::Feq | Opcode::Fne | Opcode::Flt | Opcode::Fle |
            Opcode::Fgt | Opcode::Fge |
            Opcode::Not | Opcode::And | Opcode::Or |
            Opcode::Eq | Opcode::Ne | Opcode::StrictEq | Opcode::StrictNe => {
                self.exec_comparison_ops(stack, opcode)
            }

            // =========================================================
            // Control Flow
            // =========================================================
            Opcode::Jmp | Opcode::JmpIfTrue | Opcode::JmpIfFalse |
            Opcode::JmpIfNull | Opcode::JmpIfNotNull |
            Opcode::Return | Opcode::ReturnVoid => {
                self.exec_control_flow_ops(stack, ip, code, opcode)
            }

            // =========================================================
            // Exception Handling
            // =========================================================
            Opcode::Try | Opcode::EndTry | Opcode::Throw | Opcode::Rethrow => {
                self.exec_exception_ops(stack, ip, code, task, frame_depth, opcode)
            }

            // =========================================================
            // Object Operations
            // =========================================================
            Opcode::New | Opcode::LoadField | Opcode::StoreField |
            Opcode::OptionalField | Opcode::LoadFieldFast | Opcode::StoreFieldFast |
            Opcode::ObjectLiteral | Opcode::InitObject => {
                self.exec_object_ops(stack, ip, code, opcode)
            }

            // =========================================================
            // Array Operations
            // =========================================================
            Opcode::NewArray | Opcode::LoadElem | Opcode::StoreElem |
            Opcode::ArrayLen | Opcode::ArrayPush | Opcode::ArrayPop |
            Opcode::ArrayLiteral | Opcode::InitArray => {
                self.exec_array_ops(stack, ip, code, opcode)
            }

            // =========================================================
            // Closure Operations
            // =========================================================
            Opcode::MakeClosure | Opcode::LoadCaptured | Opcode::StoreCaptured |
            Opcode::SetClosureCapture |
            Opcode::NewRefCell | Opcode::LoadRefCell | Opcode::StoreRefCell => {
                self.exec_closure_ops(stack, ip, code, task, opcode)
            }

            // =========================================================
            // String Operations
            // =========================================================
            Opcode::Sconcat | Opcode::Slen | Opcode::Seq | Opcode::Sne |
            Opcode::Slt | Opcode::Sle | Opcode::Sgt | Opcode::Sge |
            Opcode::ToString => {
                self.exec_string_ops(stack, opcode)
            }

            // =========================================================
            // Concurrency (needs MutexGuard for Await/WaitAll suspension)
            // =========================================================
            Opcode::Spawn | Opcode::SpawnClosure | Opcode::Await |
            Opcode::WaitAll | Opcode::Sleep |
            Opcode::MutexLock | Opcode::MutexUnlock |
            Opcode::Yield | Opcode::TaskCancel => {
                self.exec_concurrency_ops(stack, ip, code, module, task, opcode)
            }

            // =========================================================
            // Function Calls (needs MutexGuard for frame operations)
            // =========================================================
            Opcode::Call | Opcode::CallMethod |
            Opcode::CallConstructor | Opcode::CallSuper => {
                self.exec_call_ops(stack, ip, code, module, task, opcode)
            }

            // =========================================================
            // Native Calls (needs MutexGuard for suspend/resume)
            // =========================================================
            Opcode::NativeCall | Opcode::ModuleNativeCall => {
                self.exec_native_ops(stack, ip, code, module, task, opcode)
            }

            // =========================================================
            // Type Operations, JSON, Static Fields, Channels, Mutexes
            // =========================================================
            Opcode::InstanceOf | Opcode::Cast |
            Opcode::JsonGet | Opcode::JsonSet |
            Opcode::NewMutex | Opcode::NewChannel |
            Opcode::LoadStatic | Opcode::StoreStatic |
            Opcode::Typeof => {
                self.exec_type_ops(stack, ip, code, module, task, opcode)
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



    // ===== Helper Methods =====

    #[inline]
    pub(in crate::vm::interpreter) fn read_u8(code: &[u8], ip: &mut usize) -> Result<u8, VmError> {
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
    pub(in crate::vm::interpreter) fn read_u16(code: &[u8], ip: &mut usize) -> Result<u16, VmError> {
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
    pub(in crate::vm::interpreter) fn read_i16(code: &[u8], ip: &mut usize) -> Result<i16, VmError> {
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
    pub(in crate::vm::interpreter) fn read_u32(code: &[u8], ip: &mut usize) -> Result<u32, VmError> {
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
    pub(in crate::vm::interpreter) fn read_i32(code: &[u8], ip: &mut usize) -> Result<i32, VmError> {
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
    pub(in crate::vm::interpreter) fn read_f64(code: &[u8], ip: &mut usize) -> Result<f64, VmError> {
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


    /// Handle built-in runtime methods (std:runtime)
    ///
    /// Bridge between Interpreter's call convention (pre-popped args Vec)
    /// and the runtime handler's stack-based convention.
    pub(in crate::vm::interpreter) fn call_runtime_method(
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

    // =========================================================================
    // Register-based interpreter
    // =========================================================================

    /// Execute a task using register-based bytecode until completion, suspension, or failure.
    ///
    /// This is the register-based counterpart to `run()`. It uses a `RegisterFile` instead
    /// of a `Stack`, and decodes fixed-width 32-bit instructions instead of variable-length
    /// byte sequences.
    pub fn run_register(&mut self, task: &Arc<Task>) -> ExecutionResult {
        let module = task.module();

        // Restore execution state (supports suspend/resume)
        let mut current_func_id = task.current_func_id();
        let mut frames: Vec<RegExecutionFrame> = task.take_reg_execution_frames();

        let function = match module.functions.get(current_func_id) {
            Some(f) => f,
            None => {
                return ExecutionResult::Failed(VmError::RuntimeError(format!(
                    "Function {} not found",
                    current_func_id
                )));
            }
        };

        let mut regs = task.take_register_file();
        let mut ip = task.ip();
        let mut code: &[u32] = &function.reg_code;
        let mut reg_base = task.reg_base();

        // Check if we're resuming from suspension
        if let Some(resume_value) = task.take_resume_value() {
            let dest = task.resume_reg_dest();
            if let Err(e) = regs.set_reg(reg_base, dest, resume_value) {
                return ExecutionResult::Failed(e);
            }
        }

        // Check for pending exception (e.g., from awaited task failure)
        if task.has_exception() {
            let exception = task.current_exception().unwrap_or_else(|| Value::null());
            let mut handled = false;
            'resume_exc: loop {
                while let Some(handler) = task.peek_exception_handler() {
                    if handler.frame_count != frames.len() {
                        break;
                    }
                    if handler.catch_offset != -1 {
                        let catch_reg = handler.catch_reg;
                        task.pop_exception_handler();
                        task.set_caught_exception(exception);
                        task.clear_exception();
                        let _ = regs.set_reg(reg_base, catch_reg, exception);
                        ip = handler.catch_offset as usize;
                        handled = true;
                        break 'resume_exc;
                    }
                    if handler.finally_offset != -1 {
                        task.pop_exception_handler();
                        ip = handler.finally_offset as usize;
                        handled = true;
                        break 'resume_exc;
                    }
                    task.pop_exception_handler();
                }
                if let Some(frame) = frames.pop() {
                    task.pop_call_frame();
                    if frame.is_closure {
                        task.pop_closure();
                    }
                    regs.free_frame(reg_base);
                    current_func_id = frame.func_id;
                    code = &module.functions[frame.func_id].reg_code;
                    ip = frame.ip;
                    reg_base = frame.reg_base;
                } else {
                    break;
                }
            }
            if !handled {
                let msg = exception.raw().to_string();
                task.set_ip(ip);
                task.save_register_file(regs);
                return ExecutionResult::Failed(VmError::RuntimeError(
                    format!("Unhandled exception from awaited task: {}", msg),
                ));
            }
        }

        // Initialize the task if this is a fresh start
        if ip == 0 && regs.is_empty() && frames.is_empty() {
            task.push_call_frame(current_func_id);

            // Allocate the initial frame
            let reg_count = function.register_count as usize;
            match regs.alloc_frame(reg_count) {
                Ok(base) => reg_base = base,
                Err(e) => return ExecutionResult::Failed(e),
            }

            // Copy initial args into registers r0..rN
            let initial_args = task.take_initial_args();
            for (i, arg) in initial_args.into_iter().enumerate() {
                if i < reg_count {
                    if let Err(e) = regs.set_reg(reg_base, i as u8, arg) {
                        return ExecutionResult::Failed(e);
                    }
                }
            }
        }

        // Macro to save all frame state before leaving run_register()
        macro_rules! save_reg_state {
            () => {
                task.set_ip(ip);
                task.set_current_func_id(current_func_id);
                task.set_reg_base(reg_base);
                task.save_reg_execution_frames(frames);
                task.save_register_file(regs);
            };
        }

        // Helper: handle return from current function (frame pop)
        macro_rules! handle_reg_frame_return {
            ($return_value:expr) => {{
                let return_value = $return_value;

                if let Some(frame) = frames.pop() {
                    task.pop_call_frame();
                    if frame.is_closure {
                        task.pop_closure();
                    }

                    // Free callee's frame
                    regs.free_frame(reg_base);

                    // Restore caller's state
                    current_func_id = frame.func_id;
                    code = &module.functions[frame.func_id].reg_code;
                    ip = frame.ip;
                    reg_base = frame.reg_base;

                    // Write return value to caller's destination register
                    match frame.return_action {
                        ReturnAction::PushReturnValue => {
                            if let Err(e) = regs.set_reg(reg_base, frame.dest_reg, return_value) {
                                Some(ExecutionResult::Failed(e))
                            } else {
                                None
                            }
                        }
                        ReturnAction::PushObject(obj) => {
                            if let Err(e) = regs.set_reg(reg_base, frame.dest_reg, obj) {
                                Some(ExecutionResult::Failed(e))
                            } else {
                                None
                            }
                        }
                        ReturnAction::Discard => None,
                    }
                } else {
                    // Top-level return - task is complete
                    Some(ExecutionResult::Completed(return_value))
                }
            }};
        }

        // Main execution loop
        loop {
            // Safepoint poll for GC
            self.safepoint.poll();

            // Check for preemption
            if task.is_preempt_requested() {
                task.clear_preempt();
                let count = task.increment_preempt_count();
                if count >= self.max_preemptions {
                    save_reg_state!();
                    return ExecutionResult::Failed(VmError::RuntimeError(format!(
                        "Maximum execution time exceeded (task preempted {} times)",
                        count
                    )));
                }
                save_reg_state!();
                return ExecutionResult::Suspended(SuspendReason::Sleep {
                    wake_at: Instant::now(),
                });
            }

            // Check for cancellation
            if task.is_cancelled() {
                save_reg_state!();
                return ExecutionResult::Failed(VmError::RuntimeError(
                    "Task cancelled".to_string(),
                ));
            }

            // Bounds check - implicit return at end of function
            if ip >= code.len() {
                if let Some(result) = handle_reg_frame_return!(Value::null()) {
                    return result;
                }
                continue;
            }

            // Fetch instruction
            let instr = RegInstr::from_raw(code[ip]);
            ip += 1;

            // Read extra word for extended instructions
            let extra = match instr.opcode() {
                Some(op) if op.is_extended() => {
                    if ip >= code.len() {
                        return ExecutionResult::Failed(VmError::RuntimeError(
                            "Extended instruction missing extra word".to_string(),
                        ));
                    }
                    let e = code[ip];
                    ip += 1;
                    e
                }
                _ => 0,
            };

            // Handle exception opcodes inline (they need frames.len())
            let opcode_for_dispatch = instr.opcode();
            if matches!(
                opcode_for_dispatch,
                Some(RegOpcode::Try)
                | Some(RegOpcode::EndTry)
                | Some(RegOpcode::Throw)
                | Some(RegOpcode::Rethrow)
            ) {
                let result = self.exec_reg_exception_ops(
                    task,
                    &mut regs,
                    reg_base,
                    instr,
                    extra,
                    frames.len(),
                );
                // AwaitAll re-execution handled below; exception results fall through
                // to the same match below
                match result {
                    RegOpcodeResult::Continue => continue,
                    RegOpcodeResult::Jump(target) => {
                        ip = target;
                        continue;
                    }
                    _ => {
                        // Error/Suspend/Return — fall through to main match
                    }
                }
                // If we get here, result is Error/Suspend/Return
                // Re-match for the error handler
                match result {
                    RegOpcodeResult::Return(value) => {
                        if let Some(r) = handle_reg_frame_return!(value) {
                            return r;
                        }
                        continue;
                    }
                    RegOpcodeResult::Suspend(reason) => {
                        task.reset_preempt_count();
                        save_reg_state!();
                        return ExecutionResult::Suspended(reason);
                    }
                    RegOpcodeResult::Error(e) => {
                        // Fall through to the error handler below
                        // We need to replicate the error handling inline
                        if !task.has_exception() {
                            let error_msg = e.to_string();
                            let raya_string = RayaString::new(error_msg);
                            let gc_ptr = self.gc.lock().allocate(raya_string);
                            let exc_val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            };
                            task.set_exception(exc_val);
                        }

                        let exception = task.current_exception().unwrap_or_else(|| Value::null());
                        let mut handled = false;
                        'exc_search: loop {
                            while let Some(handler) = task.peek_exception_handler() {
                                if handler.frame_count != frames.len() {
                                    break;
                                }
                                if handler.catch_offset != -1 {
                                    let catch_reg = handler.catch_reg;
                                    task.pop_exception_handler();
                                    task.set_caught_exception(exception);
                                    task.clear_exception();
                                    let _ = regs.set_reg(reg_base, catch_reg, exception);
                                    ip = handler.catch_offset as usize;
                                    handled = true;
                                    break 'exc_search;
                                }
                                if handler.finally_offset != -1 {
                                    task.pop_exception_handler();
                                    ip = handler.finally_offset as usize;
                                    handled = true;
                                    break 'exc_search;
                                }
                                task.pop_exception_handler();
                            }
                            if let Some(frame) = frames.pop() {
                                task.pop_call_frame();
                                if frame.is_closure {
                                    task.pop_closure();
                                }
                                regs.free_frame(reg_base);
                                current_func_id = frame.func_id;
                                code = &module.functions[frame.func_id].reg_code;
                                ip = frame.ip;
                                reg_base = frame.reg_base;
                            } else {
                                break;
                            }
                        }
                        if !handled {
                            task.set_ip(ip);
                            task.save_register_file(regs);
                            return ExecutionResult::Failed(e);
                        }
                        continue;
                    }
                    _ => continue, // Continue/Jump already handled
                }
            }

            // Dispatch to handler
            match self.execute_reg_opcode(
                task,
                &mut regs,
                reg_base,
                instr,
                extra,
                module,
                ip,
            ) {
                RegOpcodeResult::Continue => {
                    // Continue to next instruction
                }
                RegOpcodeResult::Jump(target) => {
                    ip = target;
                }
                RegOpcodeResult::Return(value) => {
                    if let Some(result) = handle_reg_frame_return!(value) {
                        return result;
                    }
                }
                RegOpcodeResult::Suspend(reason) => {
                    task.reset_preempt_count();
                    save_reg_state!();
                    return ExecutionResult::Suspended(reason);
                }
                RegOpcodeResult::PushFrame {
                    func_id,
                    arg_base,
                    arg_count,
                    dest_reg,
                    is_closure,
                    closure_val,
                    return_action,
                } => {
                    // Validate function index
                    let new_func = match module.functions.get(func_id) {
                        Some(f) => f,
                        None => {
                            return ExecutionResult::Failed(VmError::RuntimeError(format!(
                                "Invalid function index: {}",
                                func_id
                            )));
                        }
                    };
                    let new_reg_count = new_func.register_count as usize;

                    // Save caller's frame
                    frames.push(RegExecutionFrame {
                        func_id: current_func_id,
                        ip,
                        reg_base,
                        reg_count: module.functions[current_func_id].register_count,
                        dest_reg,
                        is_closure,
                        return_action,
                    });

                    // Push call frame for stack traces
                    task.push_call_frame(func_id);

                    // Push closure onto closure stack if needed
                    if let Some(cv) = closure_val {
                        task.push_closure(cv);
                    }

                    // Allocate callee's register frame
                    let new_base = match regs.alloc_frame(new_reg_count) {
                        Ok(b) => b,
                        Err(e) => return ExecutionResult::Failed(e),
                    };

                    // Copy arguments from caller's registers to callee's registers
                    // For constructors (PushObject), callee.r0 = object, then user args follow
                    let callee_offset = match &return_action {
                        ReturnAction::PushObject(obj_val) => {
                            if let Err(e) = regs.set_reg(new_base, 0, *obj_val) {
                                return ExecutionResult::Failed(e);
                            }
                            1u8 // user args start at callee r1
                        }
                        _ => 0u8, // normal: args start at callee r0
                    };

                    let count = arg_count as usize;
                    for i in 0..count {
                        let val = match regs.get_reg(reg_base, arg_base.wrapping_add(i as u8)) {
                            Ok(v) => v,
                            Err(e) => return ExecutionResult::Failed(e),
                        };
                        if let Err(e) = regs.set_reg(new_base, callee_offset.wrapping_add(i as u8), val) {
                            return ExecutionResult::Failed(e);
                        }
                    }

                    // Switch to callee's code
                    current_func_id = func_id;
                    code = &module.functions[func_id].reg_code;
                    reg_base = new_base;
                    ip = 0;
                }
                RegOpcodeResult::Error(e) => {
                    // Set exception on task if not already set
                    if !task.has_exception() {
                        let error_msg = e.to_string();
                        let raya_string = RayaString::new(error_msg);
                        let gc_ptr = self.gc.lock().allocate(raya_string);
                        let exc_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        task.set_exception(exc_val);
                    }

                    let exception = task.current_exception().unwrap_or_else(|| Value::null());

                    // Frame-aware exception handling: search for handlers,
                    // unwinding register frames as needed.
                    let mut handled = false;
                    'exception_search: loop {
                        while let Some(handler) = task.peek_exception_handler() {
                            if handler.frame_count != frames.len() {
                                break;
                            }

                            if handler.catch_offset != -1 {
                                let catch_reg = handler.catch_reg;
                                task.pop_exception_handler();
                                task.set_caught_exception(exception);
                                task.clear_exception();
                                // Write exception to catch dest register
                                let _ = regs.set_reg(reg_base, catch_reg, exception);
                                ip = handler.catch_offset as usize;
                                handled = true;
                                break 'exception_search;
                            }

                            if handler.finally_offset != -1 {
                                task.pop_exception_handler();
                                ip = handler.finally_offset as usize;
                                handled = true;
                                break 'exception_search;
                            }

                            task.pop_exception_handler();
                        }

                        // No handler in current frame — pop frame and try parent
                        if let Some(frame) = frames.pop() {
                            task.pop_call_frame();
                            if frame.is_closure {
                                task.pop_closure();
                            }
                            regs.free_frame(reg_base);
                            current_func_id = frame.func_id;
                            code = &module.functions[frame.func_id].reg_code;
                            ip = frame.ip;
                            reg_base = frame.reg_base;
                        } else {
                            break;
                        }
                    }

                    if !handled {
                        task.set_ip(ip);
                        task.save_register_file(regs);
                        return ExecutionResult::Failed(e);
                    }
                }
            }
        }
    }

    /// Dispatch a single register-based opcode to the appropriate handler
    #[allow(clippy::too_many_arguments)]
    fn execute_reg_opcode(
        &mut self,
        task: &Arc<Task>,
        regs: &mut RegisterFile,
        reg_base: usize,
        instr: RegInstr,
        extra: u32,
        module: &Module,
        ip: usize,
    ) -> RegOpcodeResult {
        use super::reg_opcodes::control_flow::RegControlFlow;

        let opcode = match instr.opcode() {
            Some(op) => op,
            None => {
                return RegOpcodeResult::error(VmError::InvalidOpcode(instr.opcode_byte()));
            }
        };

        match opcode {
            // =========================================================
            // Constants & Moves
            // =========================================================
            RegOpcode::Nop
            | RegOpcode::Move
            | RegOpcode::LoadNil
            | RegOpcode::LoadTrue
            | RegOpcode::LoadFalse
            | RegOpcode::LoadInt
            | RegOpcode::LoadConst
            | RegOpcode::LoadGlobal
            | RegOpcode::StoreGlobal => {
                self.exec_reg_constant_ops(regs, reg_base, instr, module, self.globals_by_index)
            }

            // =========================================================
            // Integer and Float Arithmetic
            // =========================================================
            RegOpcode::Iadd
            | RegOpcode::Isub
            | RegOpcode::Imul
            | RegOpcode::Idiv
            | RegOpcode::Imod
            | RegOpcode::Ineg
            | RegOpcode::Ipow
            | RegOpcode::Ishl
            | RegOpcode::Ishr
            | RegOpcode::Iushr
            | RegOpcode::Iand
            | RegOpcode::Ior
            | RegOpcode::Ixor
            | RegOpcode::Inot
            | RegOpcode::Fadd
            | RegOpcode::Fsub
            | RegOpcode::Fmul
            | RegOpcode::Fdiv
            | RegOpcode::Fneg
            | RegOpcode::Fpow
            | RegOpcode::Fmod => self.exec_reg_arithmetic_ops(regs, reg_base, instr),

            // =========================================================
            // Comparisons and Logical
            // =========================================================
            RegOpcode::Ieq
            | RegOpcode::Ine
            | RegOpcode::Ilt
            | RegOpcode::Ile
            | RegOpcode::Igt
            | RegOpcode::Ige
            | RegOpcode::Feq
            | RegOpcode::Fne
            | RegOpcode::Flt
            | RegOpcode::Fle
            | RegOpcode::Fgt
            | RegOpcode::Fge
            | RegOpcode::Eq
            | RegOpcode::Ne
            | RegOpcode::StrictEq
            | RegOpcode::StrictNe
            | RegOpcode::Not
            | RegOpcode::And
            | RegOpcode::Or
            | RegOpcode::Typeof => self.exec_reg_comparison_ops(regs, reg_base, instr),

            // =========================================================
            // Control Flow
            // =========================================================
            RegOpcode::Jmp
            | RegOpcode::JmpIf
            | RegOpcode::JmpIfNot
            | RegOpcode::JmpIfNull
            | RegOpcode::JmpIfNotNull => {
                match self.exec_reg_control_flow_ops(regs, reg_base, instr, ip) {
                    Ok(RegControlFlow::Continue) => RegOpcodeResult::Continue,
                    Ok(RegControlFlow::Jump(target)) => RegOpcodeResult::Jump(target),
                    Ok(RegControlFlow::Return(val)) => RegOpcodeResult::Return(val),
                    Err(e) => RegOpcodeResult::Error(e),
                }
            }

            RegOpcode::Return | RegOpcode::ReturnVoid => {
                match self.exec_reg_control_flow_ops(regs, reg_base, instr, ip) {
                    Ok(RegControlFlow::Return(val)) => RegOpcodeResult::Return(val),
                    Ok(_) => RegOpcodeResult::runtime_error("Return opcode didn't return"),
                    Err(e) => RegOpcodeResult::Error(e),
                }
            }

            // =========================================================
            // String Operations
            // =========================================================
            RegOpcode::Sconcat
            | RegOpcode::Slen
            | RegOpcode::Seq
            | RegOpcode::Sne
            | RegOpcode::Slt
            | RegOpcode::Sle
            | RegOpcode::Sgt
            | RegOpcode::Sge
            | RegOpcode::ToString => self.exec_reg_string_ops(regs, reg_base, instr),

            // =========================================================
            // Function Calls
            // =========================================================
            RegOpcode::Call
            | RegOpcode::CallMethod
            | RegOpcode::CallConstructor
            | RegOpcode::CallSuper
            | RegOpcode::CallClosure
            | RegOpcode::CallStatic => {
                self.exec_reg_call_ops(task, regs, reg_base, instr, extra)
            }

            // =========================================================
            // Closures & Captures
            // =========================================================
            RegOpcode::MakeClosure
            | RegOpcode::LoadCaptured
            | RegOpcode::StoreCaptured
            | RegOpcode::SetClosureCapture
            | RegOpcode::NewRefCell
            | RegOpcode::LoadRefCell
            | RegOpcode::StoreRefCell => {
                self.exec_reg_closure_ops(task, regs, reg_base, instr, extra)
            }

            // =========================================================
            // Object Operations
            // =========================================================
            RegOpcode::New
            | RegOpcode::LoadField
            | RegOpcode::StoreField
            | RegOpcode::ObjectLiteral
            | RegOpcode::LoadStatic
            | RegOpcode::StoreStatic
            | RegOpcode::InstanceOf
            | RegOpcode::Cast
            | RegOpcode::OptionalField => {
                self.exec_reg_object_ops(regs, reg_base, instr, extra)
            }

            // =========================================================
            // Array & Tuple Operations
            // =========================================================
            RegOpcode::NewArray
            | RegOpcode::LoadElem
            | RegOpcode::StoreElem
            | RegOpcode::ArrayLen
            | RegOpcode::ArrayLiteral
            | RegOpcode::ArrayPush
            | RegOpcode::ArrayPop
            | RegOpcode::TupleLiteral
            | RegOpcode::TupleGet => {
                self.exec_reg_array_ops(regs, reg_base, instr, extra)
            }

            // =========================================================
            // Exception Handling
            // =========================================================
            RegOpcode::Try
            | RegOpcode::EndTry
            | RegOpcode::Throw
            | RegOpcode::Rethrow => {
                // frame_count is not available here; caller must pass it.
                // We use a sentinel that the dispatch loop will replace.
                // Actually, we handle exceptions specially in the dispatch loop.
                // Return a marker that the loop processes.
                RegOpcodeResult::runtime_error("Exception opcodes handled in dispatch loop")
            }

            // =========================================================
            // Concurrency
            // =========================================================
            RegOpcode::Spawn
            | RegOpcode::SpawnClosure
            | RegOpcode::Await
            | RegOpcode::AwaitAll
            | RegOpcode::Sleep
            | RegOpcode::Yield
            | RegOpcode::NewMutex
            | RegOpcode::MutexLock
            | RegOpcode::MutexUnlock
            | RegOpcode::NewChannel
            | RegOpcode::TaskCancel
            | RegOpcode::TaskThen => {
                self.exec_reg_concurrency_ops(task, regs, reg_base, instr, extra)
            }

            // =========================================================
            // Native Calls
            // =========================================================
            RegOpcode::NativeCall
            | RegOpcode::ModuleNativeCall
            | RegOpcode::Trap => {
                self.exec_reg_native_ops(task, regs, reg_base, instr, extra, module)
            }

            // =========================================================
            // JSON Operations
            // =========================================================
            RegOpcode::JsonGet
            | RegOpcode::JsonSet
            | RegOpcode::JsonDelete
            | RegOpcode::JsonIndex
            | RegOpcode::JsonIndexSet
            | RegOpcode::JsonPush
            | RegOpcode::JsonPop
            | RegOpcode::JsonNewObject
            | RegOpcode::JsonNewArray
            | RegOpcode::JsonKeys
            | RegOpcode::JsonLength => {
                self.exec_reg_json_ops(regs, reg_base, instr, extra, module)
            }

            // =========================================================
            // Not yet implemented
            // =========================================================
            _ => RegOpcodeResult::runtime_error(format!(
                "Register opcode {:?} not yet implemented",
                opcode
            )),
        }
    }

}
