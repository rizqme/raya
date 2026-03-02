//! Task-based interpreter that can suspend and resume
//!
//! This interpreter executes a single task until it completes, suspends, or fails.
//! Unlike the synchronous `Vm`, this interpreter returns control to the scheduler
//! when the task needs to wait for something.

use super::execution::{ExecutionFrame, ExecutionResult, OpcodeResult, ReturnAction};
use super::{ClassRegistry, SafepointCoordinator};
use crate::compiler::{Module, Opcode};
use crate::vm::builtins::handlers::{
    call_runtime_method as runtime_handler, RuntimeHandlerContext,
};
use crate::vm::gc::GarbageCollector;
use crate::vm::native_handler::NativeHandler;
use crate::vm::object::{Object, RayaString};
use crate::vm::scheduler::{SuspendReason, Task, TaskId, TaskState};
use crate::vm::stack::Stack;
use crate::vm::sync::MutexRegistry;
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

#[cfg(all(test, feature = "jit"))]
mod tests {
    use super::Interpreter;
    use crate::compiler::{Function, Opcode};
    use crate::jit::runtime::trampoline::JitExitInfo;
    use crate::vm::interpreter::JitTelemetry;
    use crate::vm::value::Value;
    use std::sync::Arc;

    fn make_function(code: Vec<u8>) -> Function {
        Function {
            name: "f".to_string(),
            param_count: 0,
            local_count: 0,
            code,
        }
    }

    #[test]
    fn resume_guard_allows_entry_nativecall_zero_args() {
        let mut code = Vec::new();
        code.push(Opcode::NativeCall as u8);
        code.extend_from_slice(&0u16.to_le_bytes());
        code.push(0u8);
        code.push(Opcode::Return as u8);
        let func = make_function(code);
        assert_eq!(
            Interpreter::native_resume_boundary_arg_count(&func, 0),
            Some(0)
        );
    }

    #[test]
    fn resume_guard_rejects_non_entry_offset() {
        let mut code = Vec::new();
        code.push(Opcode::ConstNull as u8);
        code.push(Opcode::Pop as u8);
        code.push(Opcode::NativeCall as u8);
        code.extend_from_slice(&0u16.to_le_bytes());
        code.push(0u8);
        code.push(Opcode::Return as u8);
        let func = make_function(code);
        assert_eq!(
            Interpreter::native_resume_boundary_arg_count(&func, 2),
            Some(0)
        );
    }

    #[test]
    fn resume_guard_allows_nativecall_with_args_when_stack_empty() {
        let mut code = Vec::new();
        code.push(Opcode::NativeCall as u8);
        code.extend_from_slice(&0u16.to_le_bytes());
        code.push(1u8);
        let func = make_function(code);
        assert_eq!(
            Interpreter::native_resume_boundary_arg_count(&func, 0),
            Some(1)
        );
    }

    #[test]
    fn resume_guard_rejects_non_entry_with_non_empty_stack() {
        let mut code = Vec::new();
        code.push(Opcode::ConstNull as u8); // leaves stack depth = 1
        code.push(Opcode::NativeCall as u8);
        code.extend_from_slice(&0u16.to_le_bytes());
        code.push(0u8);
        let func = make_function(code);
        assert_eq!(
            Interpreter::native_resume_boundary_arg_count(&func, 1),
            None
        );
    }

    #[test]
    fn resume_guard_allows_non_entry_nativecall_with_args_when_stack_empty() {
        let mut code = Vec::new();
        code.push(Opcode::ConstNull as u8);
        code.push(Opcode::Pop as u8); // stack depth back to 0
        code.push(Opcode::NativeCall as u8);
        code.extend_from_slice(&0u16.to_le_bytes());
        code.push(2u8);
        let func = make_function(code);
        assert_eq!(
            Interpreter::native_resume_boundary_arg_count(&func, 2),
            Some(2)
        );
    }

    #[test]
    fn resume_guard_allows_preemption_on_jmp_with_empty_stack() {
        let mut code = Vec::new();
        code.push(Opcode::Jmp as u8);
        code.extend_from_slice(&0i16.to_le_bytes());
        let func = make_function(code);
        assert!(Interpreter::can_resume_at_preemption_boundary(&func, 0));
    }

    #[test]
    fn resume_guard_rejects_preemption_on_conditional_jump_stack_dep() {
        let mut code = Vec::new();
        code.push(Opcode::ConstTrue as u8);
        code.push(Opcode::JmpIfFalse as u8);
        code.extend_from_slice(&0i16.to_le_bytes());
        let func = make_function(code);
        assert!(!Interpreter::can_resume_at_preemption_boundary(&func, 1));
    }

    #[test]
    fn resume_telemetry_counters_increment() {
        let t = Arc::new(JitTelemetry::default());
        let opt = Some(t.clone());

        Interpreter::record_native_resume_decision(&opt, true);
        Interpreter::record_native_resume_decision(&opt, false);
        Interpreter::record_preemption_resume_decision(&opt, true);
        Interpreter::record_preemption_resume_decision(&opt, false);

        let snap = t.snapshot();
        assert_eq!(snap.resume_native_ok, 1);
        assert_eq!(snap.resume_native_reject, 1);
        assert_eq!(snap.resume_preemption_ok, 1);
        assert_eq!(snap.resume_preemption_reject, 1);
    }

    #[test]
    fn native_resume_materialization_accepts_matching_count() {
        let mut code = Vec::new();
        code.push(Opcode::NativeCall as u8);
        code.extend_from_slice(&0u16.to_le_bytes());
        code.push(2u8);
        let func = make_function(code);

        let mut exit = JitExitInfo {
            bytecode_offset: 0,
            native_arg_count: 2,
            ..Default::default()
        };
        exit.native_args[0] = Value::i32(7).raw();
        exit.native_args[1] = Value::i32(11).raw();

        let vals = Interpreter::materialize_native_resume_operands(&func, &exit)
            .expect("expected materialized operand values");
        assert_eq!(vals.len(), 2);
        assert_eq!(vals[0].as_i32(), Some(7));
        assert_eq!(vals[1].as_i32(), Some(11));
    }

    #[test]
    fn native_resume_materialization_rejects_mismatched_count() {
        let mut code = Vec::new();
        code.push(Opcode::NativeCall as u8);
        code.extend_from_slice(&0u16.to_le_bytes());
        code.push(1u8);
        let func = make_function(code);

        let exit = JitExitInfo {
            bytecode_offset: 0,
            native_arg_count: 2,
            ..Default::default()
        };
        assert!(Interpreter::materialize_native_resume_operands(&func, &exit).is_none());
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
    pub(in crate::vm::interpreter) metadata:
        &'a parking_lot::Mutex<crate::vm::reflect::MetadataStore>,

    /// Class metadata registry for reflection (field/method names)
    pub(in crate::vm::interpreter) class_metadata:
        &'a RwLock<crate::vm::reflect::ClassMetadataRegistry>,

    /// External native call handler (stdlib implementation)
    #[allow(dead_code)]
    pub(in crate::vm::interpreter) native_handler: &'a Arc<dyn NativeHandler>,

    /// Resolved native functions for ModuleNativeCall dispatch
    pub(in crate::vm::interpreter) resolved_natives:
        &'a RwLock<crate::vm::native_registry::ResolvedNatives>,

    /// IO submission sender for NativeCallResult::Suspend (None in tests without reactor)
    pub(in crate::vm::interpreter) io_submit_tx:
        Option<&'a crossbeam::channel::Sender<crate::vm::scheduler::IoSubmission>>,

    /// Maximum consecutive preemptions before killing a task
    pub(in crate::vm::interpreter) max_preemptions: u32,

    /// Stack pool for reusing Stack allocations across spawned tasks
    pub(in crate::vm::interpreter) stack_pool: &'a crate::vm::scheduler::StackPool,

    /// JIT code cache for native dispatch (None when JIT is disabled)
    #[cfg(feature = "jit")]
    pub(in crate::vm::interpreter) code_cache:
        Option<Arc<crate::jit::runtime::code_cache::CodeCache>>,

    /// Per-module profiling counters for on-the-fly JIT compilation
    #[cfg(feature = "jit")]
    pub(in crate::vm::interpreter) module_profile:
        Option<Arc<crate::jit::profiling::counters::ModuleProfile>>,

    /// Handle to submit compilation requests to the background JIT thread
    #[cfg(feature = "jit")]
    pub(in crate::vm::interpreter) background_compiler:
        Option<Arc<crate::jit::profiling::BackgroundCompiler>>,

    /// Shared counters for JIT telemetry.
    #[cfg(feature = "jit")]
    pub(in crate::vm::interpreter) jit_telemetry: Option<Arc<crate::vm::interpreter::JitTelemetry>>,

    /// Compilation policy for deciding when a function is hot enough
    #[cfg(feature = "jit")]
    pub(in crate::vm::interpreter) compilation_policy:
        crate::jit::profiling::policy::CompilationPolicy,

    /// Current function ID being executed (tracked for loop profiling)
    #[cfg(feature = "jit")]
    pub(in crate::vm::interpreter) current_func_id_for_profiling: usize,

    /// Current module Arc used by loop-based JIT profiling requests.
    #[cfg(feature = "jit")]
    pub(in crate::vm::interpreter) current_module_for_profiling: Option<Arc<Module>>,

    /// Current module id in code cache used by loop-based JIT profiling requests.
    #[cfg(feature = "jit")]
    pub(in crate::vm::interpreter) current_module_id_for_profiling: Option<u64>,

    /// Debug state for debugger coordination (None = no debugger attached)
    pub(in crate::vm::interpreter) debug_state: Option<Arc<super::debug_state::DebugState>>,

    /// Sampling profiler (None when profiling is disabled).
    pub(in crate::vm::interpreter) profiler: Option<Arc<crate::profiler::Profiler>>,

    /// Current function ID for profiler stack capture.
    pub(in crate::vm::interpreter) profiler_func_id: usize,
}

impl<'a> Interpreter<'a> {
    #[inline]
    fn format_exception_value(exception: Value) -> String {
        if exception.is_null() {
            return "null".to_string();
        }
        if !exception.is_ptr() {
            return format!("{:?}", exception);
        }
        if let Some(s) = unsafe { exception.as_ptr::<RayaString>() } {
            return unsafe { &*s.as_ptr() }.data.clone();
        }
        if let Some(obj) = unsafe { exception.as_ptr::<Object>() } {
            if let Some(msg_val) = unsafe { &*obj.as_ptr() }.get_field(0) {
                if msg_val.is_ptr() {
                    if let Some(s) = unsafe { msg_val.as_ptr::<RayaString>() } {
                        return unsafe { &*s.as_ptr() }.data.clone();
                    }
                }
            }
        }
        format!("{:?}", exception)
    }

    #[cfg(feature = "jit")]
    #[inline]
    fn record_native_resume_decision(
        telemetry: &Option<Arc<crate::vm::interpreter::JitTelemetry>>,
        resumed: bool,
    ) {
        if let Some(t) = telemetry {
            if resumed {
                t.resume_native_ok
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            } else {
                t.resume_native_reject
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    #[cfg(feature = "jit")]
    #[inline]
    fn record_preemption_resume_decision(
        telemetry: &Option<Arc<crate::vm::interpreter::JitTelemetry>>,
        resumed: bool,
    ) {
        if let Some(t) = telemetry {
            if resumed {
                t.resume_preemption_ok
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            } else {
                t.resume_preemption_reject
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    #[cfg(feature = "jit")]
    #[inline]
    fn materialize_native_resume_operands(
        func: &crate::compiler::Function,
        exit_info: &crate::jit::runtime::trampoline::JitExitInfo,
    ) -> Option<Vec<Value>> {
        let expected_arg_count =
            Self::native_resume_boundary_arg_count(func, exit_info.bytecode_offset)?;
        let mat_count = exit_info.native_arg_count as usize;
        let max_native_args = crate::jit::runtime::trampoline::JIT_EXIT_MAX_NATIVE_ARGS;
        if mat_count != expected_arg_count as usize || mat_count > max_native_args {
            return None;
        }

        let mut vals = Vec::with_capacity(mat_count);
        for i in 0..mat_count {
            vals.push(unsafe { Value::from_raw(exit_info.native_args[i]) });
        }
        Some(vals)
    }

    /// Create a new task interpreter
    #[allow(clippy::too_many_arguments)] // Interpreter borrows many VM subsystems; a config struct would just move the problem.
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
        stack_pool: &'a crate::vm::scheduler::StackPool,
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
            stack_pool,
            debug_state: None,
            #[cfg(feature = "jit")]
            code_cache: None,
            #[cfg(feature = "jit")]
            module_profile: None,
            #[cfg(feature = "jit")]
            background_compiler: None,
            #[cfg(feature = "jit")]
            jit_telemetry: None,
            #[cfg(feature = "jit")]
            compilation_policy: crate::jit::profiling::policy::CompilationPolicy::new(),
            #[cfg(feature = "jit")]
            current_func_id_for_profiling: 0,
            #[cfg(feature = "jit")]
            current_module_for_profiling: None,
            #[cfg(feature = "jit")]
            current_module_id_for_profiling: None,
            profiler: None,
            profiler_func_id: 0,
        }
    }

    /// Set the debug state for debugger coordination.
    pub fn set_debug_state(&mut self, debug_state: Option<Arc<super::debug_state::DebugState>>) {
        self.debug_state = debug_state;
    }

    /// Set the profiler for sampling.
    pub fn set_profiler(&mut self, profiler: Option<Arc<crate::profiler::Profiler>>) {
        self.profiler = profiler;
    }

    /// Set the JIT code cache for native dispatch.
    ///
    /// Called by the reactor worker after constructing the interpreter.
    #[cfg(feature = "jit")]
    pub fn set_code_cache(
        &mut self,
        cache: Option<Arc<crate::jit::runtime::code_cache::CodeCache>>,
    ) {
        self.code_cache = cache;
    }

    /// Set the module profile for on-the-fly JIT profiling.
    #[cfg(feature = "jit")]
    pub fn set_module_profile(
        &mut self,
        profile: Option<Arc<crate::jit::profiling::counters::ModuleProfile>>,
    ) {
        self.module_profile = profile;
    }

    /// Set the background compiler handle for submitting compilation requests.
    #[cfg(feature = "jit")]
    pub fn set_background_compiler(
        &mut self,
        compiler: Option<Arc<crate::jit::profiling::BackgroundCompiler>>,
    ) {
        self.background_compiler = compiler;
    }

    /// Set shared JIT telemetry counters.
    #[cfg(feature = "jit")]
    pub fn set_jit_telemetry(
        &mut self,
        telemetry: Option<Arc<crate::vm::interpreter::JitTelemetry>>,
    ) {
        self.jit_telemetry = telemetry;
    }

    /// Set the compilation policy thresholds.
    #[cfg(feature = "jit")]
    pub fn set_compilation_policy(
        &mut self,
        policy: crate::jit::profiling::policy::CompilationPolicy,
    ) {
        self.compilation_policy = policy;
    }

    /// Wake a suspended task by setting its resume value and pushing it to the scheduler.
    #[allow(dead_code)]
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

        // JIT: look up the module_id for this module's checksum (cached for the run)
        #[cfg(feature = "jit")]
        let jit_module_id: Option<u64> = self
            .code_cache
            .as_ref()
            .and_then(|cache| cache.module_id(&module.checksum));

        // Restore execution state (supports suspend/resume)
        let mut current_func_id = task.current_func_id();
        let mut frames: Vec<ExecutionFrame> = task.take_execution_frames();

        // Track current function for loop profiling
        #[cfg(feature = "jit")]
        {
            self.current_func_id_for_profiling = current_func_id;
            self.current_module_for_profiling = Some(module.clone());
            self.current_module_id_for_profiling = jit_module_id;
        }
        self.profiler_func_id = current_func_id;

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
        let mut current_arg_count = 0usize; // Track current function's arg count (for rest parameters)

        // Check if we're resuming from suspension.
        //
        // Most await sites expect the resumed task's value to be pushed back
        // onto the operand stack. `WaitAll` is different: it re-executes with
        // its original array operand already on the stack and does not consume
        // the resumed value from a single completed child task.
        if task.has_exception() {
            // Exception resumption path must not also materialize a prior resume value.
            // Mixing both can corrupt operand expectations at catch/unwind boundaries.
            let _ = task.take_resume_value();
        } else if let Some(resume_value) = task.take_resume_value() {
            let next_opcode = code.get(ip).and_then(|b| Opcode::from_u8(*b));
            if !matches!(next_opcode, Some(Opcode::WaitAll)) {
                if let Err(e) = stack_guard.push(resume_value) {
                    return ExecutionResult::Failed(e);
                }
            }
        }

        // Check if there's a pending exception (e.g., from awaited task failure).
        // Use the same frame-aware unwind logic as the main OpcodeResult::Error path.
        if task.has_exception() {
            let exception = task.current_exception().unwrap_or_else(Value::null);
            let mut handled = false;
            'resume_exception_search: loop {
                while let Some(handler) = task.peek_exception_handler() {
                    if handler.frame_count != frames.len() {
                        break;
                    }

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
                        break 'resume_exception_search;
                    }

                    if handler.finally_offset != -1 {
                        task.pop_exception_handler();
                        ip = handler.finally_offset as usize;
                        handled = true;
                        break 'resume_exception_search;
                    }

                    task.pop_exception_handler();
                }

                if let Some(frame) = frames.pop() {
                    task.pop_call_frame();
                    if frame.is_closure {
                        task.pop_closure();
                    }
                    current_func_id = frame.func_id;
                    #[cfg(feature = "jit")]
                    {
                        self.current_func_id_for_profiling = current_func_id;
                    }
                    code = &module.functions[frame.func_id].code;
                    ip = frame.ip;
                    locals_base = frame.locals_base;
                    current_arg_count = frame.arg_count;
                } else {
                    break;
                }
            }

            if !handled {
                task.set_ip(ip);
                drop(stack_guard);
                return ExecutionResult::Failed(VmError::RuntimeError(format!(
                    "Unhandled exception from awaited task: {}",
                    Self::format_exception_value(exception)
                )));
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
                if i < function.local_count {
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
                    #[cfg(feature = "jit")]
                    {
                        self.current_func_id_for_profiling = current_func_id;
                    }
                    self.profiler_func_id = current_func_id;
                    code = &module.functions[frame.func_id].code;
                    ip = frame.ip;
                    locals_base = frame.locals_base;
                    current_arg_count = frame.arg_count; // Restore caller's arg count

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

        // Debug: break at entry point if requested
        if let Some(ref ds) = self.debug_state {
            if ds
                .break_at_entry
                .swap(false, std::sync::atomic::Ordering::AcqRel)
            {
                let bytecode_offset = ip as u32;
                let current_line = self.lookup_line(module, current_func_id, bytecode_offset);
                let info = self.build_pause_info(
                    module,
                    current_func_id,
                    bytecode_offset,
                    current_line,
                    super::debug_state::PauseReason::Entry,
                );
                ds.signal_pause(info);
            }
        }

        // Main execution loop
        loop {
            // Safepoint poll for GC
            self.safepoint.poll();

            // Profiler: sample at preemption points (zero-cost when profiler is None)
            if let Some(ref profiler) = self.profiler {
                profiler.maybe_sample(task, self.profiler_func_id, ip);
            }

            // Check for preemption
            if task.is_preempt_requested() {
                task.clear_preempt();
                let count = task.increment_preempt_count();
                // Infinite loop detection: kill task after max_preemptions consecutive
                // preemptions without voluntary suspension
                if count >= self.max_preemptions {
                    save_frame_state!();
                    drop(stack_guard);
                    return ExecutionResult::Failed(VmError::RuntimeError(format!(
                        "Maximum execution time exceeded (task preempted {} times)",
                        count
                    )));
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
                // Cancellation is observable to awaiters as a rejected task.
                // Unhandled rejection reporting already suppresses cancelled tasks.
                return ExecutionResult::Failed(VmError::RuntimeError(
                    "Task cancelled".to_string(),
                ));
            }

            // Bounds check - implicit return at end of function
            if ip >= code.len() {
                let local_count = module.functions[current_func_id].local_count;
                let return_value = if stack_guard.depth() > locals_base + local_count {
                    stack_guard.pop().unwrap_or_default()
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

            // Debug check: test breakpoints, step modes, and debugger statements
            // when a debugger is attached. The fast path (no debugger) is a single
            // atomic relaxed load.
            if let Some(ref ds) = self.debug_state {
                if ds.active.load(std::sync::atomic::Ordering::Relaxed) {
                    let bytecode_offset = (ip - 1) as u32;
                    let current_line = self.lookup_line(module, current_func_id, bytecode_offset);

                    // Check for `debugger;` statement first
                    let pause_reason = if opcode == Opcode::Debugger {
                        Some(super::debug_state::PauseReason::DebuggerStatement)
                    } else {
                        ds.should_break(
                            current_func_id,
                            bytecode_offset,
                            frames.len() + 1,
                            current_line,
                        )
                    };

                    if let Some(reason) = pause_reason {
                        if let super::debug_state::PauseReason::Breakpoint(bp_id) = &reason {
                            ds.increment_hit_count(*bp_id);
                        }
                        let info = self.build_pause_info(
                            module,
                            current_func_id,
                            bytecode_offset,
                            current_line,
                            reason,
                        );
                        ds.signal_pause(info);
                    }
                }
            }

            // Execute the opcode
            match self.execute_opcode(
                task,
                &mut stack_guard,
                &mut ip,
                code,
                module,
                opcode,
                locals_base,
                frames.len(),
                current_arg_count,
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
                    #[cfg(feature = "jit")]
                    let mut forced_callee_ip: Option<usize> = None;
                    #[cfg(feature = "jit")]
                    let mut forced_callee_extra_locals: Option<Vec<u64>> = None;
                    #[cfg(feature = "jit")]
                    let mut forced_callee_operand_values: Option<Vec<Value>> = None;

                    // JIT profiling: record call and check if function should be compiled
                    #[cfg(feature = "jit")]
                    if !is_closure {
                        if let Some(ref profile) = self.module_profile {
                            let count = profile.record_call(func_id);
                            if let Some(ref telemetry) = self.jit_telemetry {
                                telemetry
                                    .call_samples
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                            // Check compilation policy periodically to amortize overhead
                            if count & crate::vm::defaults::JIT_POLICY_CHECK_MASK == 0 {
                                if let Some(mid) = jit_module_id {
                                    self.maybe_request_compilation(func_id, task.module(), mid);
                                }
                            }
                        }
                    }

                    // JIT fast path: dispatch to native code if available
                    // Only for non-closure, non-constructor calls (pure function calls)
                    #[cfg(feature = "jit")]
                    if !is_closure {
                        if let (Some(cache), Some(mid)) = (&self.code_cache, jit_module_id) {
                            if let Some(jit_fn) = cache.get(mid, func_id as u32) {
                                if let Some(ref telemetry) = self.jit_telemetry {
                                    telemetry
                                        .cache_hits
                                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                }
                                // Collect args from stack as NaN-boxed u64s
                                let args: Vec<u64> = (0..arg_count)
                                    .map(|i| {
                                        stack_guard
                                            .peek_at(stack_guard.depth() - arg_count + i)
                                            .unwrap_or_default()
                                            .raw()
                                    })
                                    .collect();

                                let func = &module.functions[func_id];
                                let local_count = func.local_count;
                                let extra_locals = local_count.saturating_sub(arg_count);
                                let mut locals_buf = vec![0u64; extra_locals];
                                let mut exit_info =
                                    crate::jit::runtime::trampoline::JitExitInfo::default();
                                let bridge_ctx =
                                    crate::jit::runtime::helpers::build_runtime_bridge_context(
                                        self.safepoint,
                                        task,
                                        self.gc,
                                        self.classes,
                                        self.class_metadata,
                                        self.resolved_natives,
                                        self.io_submit_tx,
                                    );
                                let mut runtime_ctx =
                                    crate::jit::runtime::helpers::build_runtime_context(
                                        &bridge_ctx,
                                        module.as_ref(),
                                    );

                                // Call JIT-compiled function with runtime context so safepoint/preemption
                                // branches inside machine code can hand off to interpreter thread loop.
                                let result = unsafe {
                                    jit_fn(
                                        args.as_ptr(),
                                        arg_count as u32,
                                        locals_buf.as_mut_ptr(),
                                        extra_locals as u32,
                                        (&mut runtime_ctx as *mut _),
                                        (&mut exit_info as *mut _),
                                    )
                                };

                                // Pop args from stack
                                for _ in 0..arg_count {
                                    let _ = stack_guard.pop();
                                }

                                // Future-proof exit handling for suspension/deopt bridges.
                                if exit_info.kind
                                    != crate::jit::runtime::trampoline::JitExitKind::Completed
                                        as u32
                                {
                                    // Fallback safely to interpreter execution path.
                                    // Leave no JIT-only side effects behind.
                                    if let Some(ref telemetry) = self.jit_telemetry {
                                        telemetry
                                            .cache_misses
                                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                    }
                                    // Re-push args so normal interpreter call-frame setup can proceed.
                                    for raw in &args {
                                        if let Err(e) =
                                            stack_guard.push(unsafe { Value::from_raw(*raw) })
                                        {
                                            return ExecutionResult::Failed(e);
                                        }
                                    }

                                    // Conservative continuation: if JIT suspended exactly at a
                                    // zero-arg native boundary, resume interpreter from that
                                    // bytecode offset instead of restarting the whole callee.
                                    if exit_info.kind
                                        == crate::jit::runtime::trampoline::JitExitKind::Suspended
                                            as u32
                                    {
                                        match exit_info.suspend_reason {
                                            x if x
                                                == crate::jit::runtime::trampoline::JitSuspendReason::NativeCallBoundary
                                                    as u32 =>
                                            {
                                                let resumed = if let Some(vals) =
                                                    Self::materialize_native_resume_operands(
                                                        func, &exit_info,
                                                    )
                                                {
                                                    forced_callee_ip =
                                                        Some(exit_info.bytecode_offset as usize);
                                                    forced_callee_extra_locals =
                                                        Some(locals_buf.clone());
                                                    forced_callee_operand_values = Some(vals);
                                                    true
                                                } else {
                                                    false
                                                };
                                                Self::record_native_resume_decision(
                                                    &self.jit_telemetry,
                                                    resumed,
                                                );
                                            }
                                            x if x
                                                == crate::jit::runtime::trampoline::JitSuspendReason::Preemption
                                                    as u32 =>
                                            {
                                                let resumed =
                                                    if Self::can_resume_at_preemption_boundary(
                                                        func,
                                                        exit_info.bytecode_offset,
                                                    ) {
                                                        forced_callee_ip =
                                                            Some(exit_info.bytecode_offset as usize);
                                                        forced_callee_extra_locals =
                                                            Some(locals_buf.clone());
                                                        true
                                                    } else {
                                                        false
                                                    };
                                                Self::record_preemption_resume_decision(
                                                    &self.jit_telemetry,
                                                    resumed,
                                                );
                                            }
                                            _ => {}
                                        }
                                    }
                                    // continue below into bytecode frame setup (no `continue`)
                                } else {
                                    // Push return value (or handle based on return_action)
                                    // Safety: result is a NaN-boxed Value returned by JIT-compiled code
                                    let return_val = unsafe { Value::from_raw(result) };
                                    match return_action {
                                        ReturnAction::PushReturnValue => {
                                            if let Err(e) = stack_guard.push(return_val) {
                                                return ExecutionResult::Failed(e);
                                            }
                                        }
                                        ReturnAction::PushObject(obj) => {
                                            if let Err(e) = stack_guard.push(obj) {
                                                return ExecutionResult::Failed(e);
                                            }
                                        }
                                        ReturnAction::Discard => {}
                                    }
                                    continue; // skip bytecode frame setup
                                }
                            } else if let Some(ref telemetry) = self.jit_telemetry {
                                telemetry
                                    .cache_misses
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                    }

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
                    let new_local_count = new_func.local_count;

                    // Save caller's frame
                    frames.push(ExecutionFrame {
                        func_id: current_func_id,
                        ip,
                        locals_base,
                        is_closure,
                        return_action,
                        arg_count: current_arg_count, // Save caller's arg count
                    });

                    // Push call frame for stack traces
                    task.push_call_frame(func_id);

                    // Push closure onto closure stack if needed
                    if let Some(cv) = closure_val {
                        task.push_closure(cv);
                    }

                    // Set up callee's frame on the same stack
                    // Args are already on the stack from the caller
                    locals_base = stack_guard.depth() - arg_count;

                    // Allocate remaining locals (initialized to null)
                    // Note: If arg_count > new_local_count, we don't discard extras.
                    // This allows rest parameters to access all arguments via LoadArgLocal.
                    for _ in 0..(new_local_count.saturating_sub(arg_count)) {
                        if let Err(e) = stack_guard.push(Value::null()) {
                            return ExecutionResult::Failed(e);
                        }
                    }

                    // First stack materialization piece:
                    // if we resume interpreter from a JIT native-boundary suspension,
                    // restore JIT-mutated non-arg locals into interpreter local slots.
                    #[cfg(feature = "jit")]
                    if forced_callee_ip.is_some() {
                        if let Some(extra_locals) = forced_callee_extra_locals.as_ref() {
                            for (i, raw) in extra_locals.iter().enumerate() {
                                let slot = locals_base + arg_count + i;
                                if slot >= stack_guard.depth() {
                                    break;
                                }
                                if let Err(e) =
                                    stack_guard.set_at(slot, unsafe { Value::from_raw(*raw) })
                                {
                                    return ExecutionResult::Failed(e);
                                }
                            }
                            if let Some(operand_vals) = forced_callee_operand_values.as_ref() {
                                for v in operand_vals {
                                    if let Err(e) = stack_guard.push(*v) {
                                        return ExecutionResult::Failed(e);
                                    }
                                }
                            }
                        }
                    }

                    // Switch to callee's code
                    current_func_id = func_id;
                    #[cfg(feature = "jit")]
                    {
                        self.current_func_id_for_profiling = current_func_id;
                    }
                    self.profiler_func_id = current_func_id;
                    code = &module.functions[func_id].code;
                    current_arg_count = arg_count; // Set current arg count to callee's arg count
                    #[cfg(feature = "jit")]
                    {
                        ip = forced_callee_ip.unwrap_or(0);
                    }
                    #[cfg(not(feature = "jit"))]
                    {
                        ip = 0;
                    }
                }
                OpcodeResult::Error(e) => {
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

                    let exception = task.current_exception().unwrap_or_else(Value::null);

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
                            #[cfg(feature = "jit")]
                            {
                                self.current_func_id_for_profiling = current_func_id;
                            }
                            code = &module.functions[frame.func_id].code;
                            ip = frame.ip;
                            locals_base = frame.locals_base;
                            current_arg_count = frame.arg_count; // Restore caller's arg count
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
        arg_count: usize, // Current function's arg count (for rest parameters)
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
            Opcode::ConstNull
            | Opcode::ConstTrue
            | Opcode::ConstFalse
            | Opcode::ConstI32
            | Opcode::ConstF64
            | Opcode::ConstStr => self.exec_constant_ops(stack, ip, code, module, opcode),

            // =========================================================
            // Variables
            // =========================================================
            Opcode::LoadLocal
            | Opcode::StoreLocal
            | Opcode::LoadLocal0
            | Opcode::LoadLocal1
            | Opcode::StoreLocal0
            | Opcode::StoreLocal1
            | Opcode::GetArgCount
            | Opcode::LoadArgLocal
            | Opcode::LoadGlobal
            | Opcode::StoreGlobal => {
                self.exec_variable_ops(stack, ip, code, locals_base, opcode, arg_count)
            }

            // =========================================================
            // Integer and Float Arithmetic
            // =========================================================
            Opcode::Iadd
            | Opcode::Isub
            | Opcode::Imul
            | Opcode::Idiv
            | Opcode::Imod
            | Opcode::Ineg
            | Opcode::Ipow
            | Opcode::Ishl
            | Opcode::Ishr
            | Opcode::Iushr
            | Opcode::Iand
            | Opcode::Ior
            | Opcode::Ixor
            | Opcode::Inot
            | Opcode::Fadd
            | Opcode::Fsub
            | Opcode::Fmul
            | Opcode::Fdiv
            | Opcode::Fneg
            | Opcode::Fpow
            | Opcode::Fmod => self.exec_arithmetic_ops(stack, opcode),

            // =========================================================
            // Comparisons and Logical Operators
            // =========================================================
            Opcode::Ieq
            | Opcode::Ine
            | Opcode::Ilt
            | Opcode::Ile
            | Opcode::Igt
            | Opcode::Ige
            | Opcode::Feq
            | Opcode::Fne
            | Opcode::Flt
            | Opcode::Fle
            | Opcode::Fgt
            | Opcode::Fge
            | Opcode::Not
            | Opcode::And
            | Opcode::Or
            | Opcode::Eq
            | Opcode::Ne
            | Opcode::StrictEq
            | Opcode::StrictNe => self.exec_comparison_ops(stack, opcode),

            // =========================================================
            // Control Flow
            // =========================================================
            Opcode::Jmp
            | Opcode::JmpIfTrue
            | Opcode::JmpIfFalse
            | Opcode::JmpIfNull
            | Opcode::JmpIfNotNull
            | Opcode::Return
            | Opcode::ReturnVoid => self.exec_control_flow_ops(stack, ip, code, opcode),

            // =========================================================
            // Exception Handling
            // =========================================================
            Opcode::Try | Opcode::EndTry | Opcode::Throw | Opcode::Rethrow => {
                self.exec_exception_ops(stack, ip, code, task, frame_depth, opcode)
            }

            // =========================================================
            // Object Operations
            // =========================================================
            Opcode::New
            | Opcode::LoadField
            | Opcode::StoreField
            | Opcode::OptionalField
            | Opcode::LoadFieldFast
            | Opcode::StoreFieldFast
            | Opcode::ObjectLiteral
            | Opcode::InitObject
            | Opcode::BindMethod => self.exec_object_ops(stack, ip, code, opcode),

            // =========================================================
            // Array Operations
            // =========================================================
            Opcode::NewArray
            | Opcode::LoadElem
            | Opcode::StoreElem
            | Opcode::ArrayLen
            | Opcode::ArrayPush
            | Opcode::ArrayPop
            | Opcode::ArrayLiteral
            | Opcode::InitArray => self.exec_array_ops(stack, ip, code, opcode),

            // =========================================================
            // Closure Operations
            // =========================================================
            Opcode::MakeClosure
            | Opcode::LoadCaptured
            | Opcode::StoreCaptured
            | Opcode::SetClosureCapture
            | Opcode::NewRefCell
            | Opcode::LoadRefCell
            | Opcode::StoreRefCell => self.exec_closure_ops(stack, ip, code, task, opcode),

            // =========================================================
            // String Operations
            // =========================================================
            Opcode::Sconcat
            | Opcode::Slen
            | Opcode::Seq
            | Opcode::Sne
            | Opcode::Slt
            | Opcode::Sle
            | Opcode::Sgt
            | Opcode::Sge
            | Opcode::ToString => self.exec_string_ops(stack, opcode),

            // =========================================================
            // Concurrency (needs MutexGuard for Await/WaitAll suspension)
            // =========================================================
            Opcode::Spawn
            | Opcode::SpawnClosure
            | Opcode::Await
            | Opcode::WaitAll
            | Opcode::Sleep
            | Opcode::MutexLock
            | Opcode::MutexUnlock
            | Opcode::Yield
            | Opcode::TaskCancel => {
                self.exec_concurrency_ops(stack, ip, code, module, task, opcode)
            }

            // =========================================================
            // Function Calls (needs MutexGuard for frame operations)
            // =========================================================
            Opcode::Call
            | Opcode::CallMethod
            | Opcode::OptionalCallMethod
            | Opcode::CallConstructor
            | Opcode::CallSuper => self.exec_call_ops(stack, ip, code, module, task, opcode),

            // =========================================================
            // Native Calls (needs MutexGuard for suspend/resume)
            // =========================================================
            Opcode::NativeCall | Opcode::ModuleNativeCall => {
                self.exec_native_ops(stack, ip, code, module, task, opcode)
            }

            // =========================================================
            // Type Operations, JSON, Static Fields, Channels, Mutexes
            // =========================================================
            Opcode::InstanceOf
            | Opcode::Cast
            | Opcode::DynGet
            | Opcode::DynSet
            | Opcode::NewMutex
            | Opcode::NewChannel
            | Opcode::LoadStatic
            | Opcode::StoreStatic
            | Opcode::Typeof => self.exec_type_ops(stack, ip, code, module, task, opcode),

            // =========================================================
            // Debugger Statement
            // =========================================================
            Opcode::Debugger => {
                // The actual pause is handled in the main loop via the
                // `debugger_pause` flag. This handler is a no-op.
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
    pub(in crate::vm::interpreter) fn read_u16(
        code: &[u8],
        ip: &mut usize,
    ) -> Result<u16, VmError> {
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
    pub(in crate::vm::interpreter) fn read_i16(
        code: &[u8],
        ip: &mut usize,
    ) -> Result<i16, VmError> {
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
    pub(in crate::vm::interpreter) fn read_u32(
        code: &[u8],
        ip: &mut usize,
    ) -> Result<u32, VmError> {
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
    pub(in crate::vm::interpreter) fn read_i32(
        code: &[u8],
        ip: &mut usize,
    ) -> Result<i32, VmError> {
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
    pub(in crate::vm::interpreter) fn read_f64(
        code: &[u8],
        ip: &mut usize,
    ) -> Result<f64, VmError> {
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
        let ctx = RuntimeHandlerContext { gc: self.gc };

        // Push args back onto stack so the handler can pop them
        let arg_count = args.len();
        for arg in args {
            stack.push(arg)?;
        }

        runtime_handler(&ctx, stack, method_id, arg_count)
    }

    /// Check if a function should be compiled on-the-fly and submit a request.
    ///
    /// Called after profiling counters are incremented. Uses the compilation policy
    /// to decide, then CAS-claims the function and sends a request to the background thread.
    #[cfg(feature = "jit")]
    pub(in crate::vm::interpreter) fn maybe_request_compilation(
        &self,
        func_id: usize,
        module: &Arc<Module>,
        module_id: u64,
    ) {
        let Some(ref profile) = self.module_profile else {
            return;
        };
        let Some(func_profile) = profile.get(func_id) else {
            return;
        };

        // Already compiled or in progress
        if func_profile.is_jit_available() {
            return;
        }

        if let Some(func) = module.functions.get(func_id) {
            if !crate::jit::analysis::heuristics::function_supported_for_jit(func) {
                func_profile.finish_compile_failed();
                return;
            }
        } else {
            return;
        }

        let code_size = module
            .functions
            .get(func_id)
            .map(|f| f.code.len())
            .unwrap_or(0);
        if !self
            .compilation_policy
            .should_compile(func_profile, code_size)
        {
            return;
        }

        // CAS to claim this function for compilation (prevents duplicate requests)
        if !func_profile.try_start_compile() {
            return;
        }

        // Submit to background compiler
        if let Some(ref compiler) = self.background_compiler {
            let request = crate::jit::profiling::CompilationRequest {
                module: module.clone(),
                func_index: func_id,
                module_id,
                module_profile: profile.clone(),
            };
            let accepted = compiler.try_submit(request);
            if let Some(ref telemetry) = self.jit_telemetry {
                if accepted {
                    telemetry
                        .compile_requests_submitted
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                } else {
                    telemetry
                        .compile_requests_dropped
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
    }

    /// Conservative check for direct resume into a native-call suspension point.
    ///
    /// Safe only for zero-arg NativeCall/ModuleNativeCall and when the
    /// bytecode prefix guarantees an empty operand stack at resume point.
    #[cfg(feature = "jit")]
    fn native_resume_boundary_arg_count(
        func: &crate::compiler::Function,
        bytecode_offset: u32,
    ) -> Option<u8> {
        let offset = bytecode_offset as usize;
        let code = &func.code;
        if offset >= code.len() {
            return None;
        }
        let op = code[offset];
        if op != Opcode::NativeCall as u8 && op != Opcode::ModuleNativeCall as u8 {
            return None;
        }
        // Encoding: opcode (1) + native_id (2) + arg_count (1)
        if offset + 3 >= code.len() {
            return None;
        }
        let arg_count = code[offset + 3];

        // First materialization phase safety rule:
        // resume only if operand stack depth at target is statically zero.
        if Self::conservative_stack_depth_until(code, offset) != Some(0) {
            return None;
        }
        Some(arg_count)
    }

    /// Conservative check for resume at a preemption boundary.
    ///
    /// Currently limited to unconditional `Jmp` sites with statically-empty
    /// operand stack at the resume offset.
    #[cfg(feature = "jit")]
    fn can_resume_at_preemption_boundary(
        func: &crate::compiler::Function,
        bytecode_offset: u32,
    ) -> bool {
        let offset = bytecode_offset as usize;
        let code = &func.code;
        if offset >= code.len() {
            return false;
        }
        if code[offset] != Opcode::Jmp as u8 {
            return false;
        }
        if Self::conservative_stack_depth_until(code, offset) != Some(0) {
            return false;
        }
        true
    }

    /// Conservative linear stack-depth evaluator for resume safety.
    ///
    /// Returns `None` for unsupported/control-flow opcodes or malformed bytecode.
    #[cfg(feature = "jit")]
    fn conservative_stack_depth_until(code: &[u8], target_offset: usize) -> Option<i32> {
        let mut ip = 0usize;
        let mut depth = 0i32;

        while ip < target_offset {
            let op = Opcode::from_u8(*code.get(ip)?)?;
            ip += 1;

            let (pop, push, imm): (i32, i32, usize) = match op {
                // constants
                Opcode::ConstNull
                | Opcode::ConstTrue
                | Opcode::ConstFalse
                | Opcode::LoadLocal0
                | Opcode::LoadLocal1
                | Opcode::LoadGlobal
                | Opcode::LoadConst => (
                    0,
                    1,
                    match op {
                        Opcode::LoadGlobal | Opcode::LoadConst => 4,
                        _ => 0,
                    },
                ),
                Opcode::ConstI32 => (0, 1, 4),
                Opcode::ConstF64 => (0, 1, 8),
                Opcode::ConstStr => (0, 1, 2),
                Opcode::LoadLocal => (0, 1, 2),

                // stores/stack ops
                Opcode::StoreLocal0 | Opcode::StoreLocal1 | Opcode::Pop => (1, 0, 0),
                Opcode::StoreLocal => (1, 0, 2),
                Opcode::Dup => (1, 2, 0),
                Opcode::Swap => (2, 2, 0),

                // integer arithmetic/comparison
                Opcode::Iadd
                | Opcode::Isub
                | Opcode::Imul
                | Opcode::Idiv
                | Opcode::Imod
                | Opcode::Ieq
                | Opcode::Ine
                | Opcode::Ilt
                | Opcode::Ile
                | Opcode::Igt
                | Opcode::Ige => (2, 1, 0),
                Opcode::Ineg => (1, 1, 0),

                // float arithmetic/comparison
                Opcode::Fadd
                | Opcode::Fsub
                | Opcode::Fmul
                | Opcode::Fdiv
                | Opcode::Feq
                | Opcode::Fne
                | Opcode::Flt
                | Opcode::Fle
                | Opcode::Fgt
                | Opcode::Fge => (2, 1, 0),
                Opcode::Fneg => (1, 1, 0),

                // conservative stop: control flow/calls/exceptions/other complex ops
                _ => return None,
            };

            ip = ip.checked_add(imm)?;
            if ip > target_offset {
                return None;
            }

            depth -= pop;
            if depth < 0 {
                return None;
            }
            depth += push;
        }

        if ip == target_offset {
            Some(depth)
        } else {
            None
        }
    }

    /// Look up the source line for a bytecode offset in a function.
    /// Returns 0 if debug info is unavailable.
    #[inline]
    fn lookup_line(&self, module: &Module, func_id: usize, bytecode_offset: u32) -> u32 {
        module
            .debug_info
            .as_ref()
            .and_then(|di| di.functions.get(func_id))
            .and_then(|fd| fd.lookup_location(bytecode_offset))
            .map(|entry| entry.line)
            .unwrap_or(0)
    }

    /// Build a PauseInfo struct from the current execution state.
    fn build_pause_info(
        &self,
        module: &Module,
        func_id: usize,
        bytecode_offset: u32,
        current_line: u32,
        reason: super::debug_state::PauseReason,
    ) -> super::debug_state::PauseInfo {
        let (source_file, column) = module
            .debug_info
            .as_ref()
            .and_then(|di| {
                let fd = di.functions.get(func_id)?;
                let entry = fd.lookup_location(bytecode_offset)?;
                let file = di
                    .source_files
                    .get(fd.source_file_index as usize)
                    .cloned()
                    .unwrap_or_default();
                Some((file, entry.column))
            })
            .unwrap_or_else(|| (String::new(), 0));

        let function_name = module
            .functions
            .get(func_id)
            .map(|f| f.name.clone())
            .unwrap_or_else(|| format!("<func_{}>", func_id));

        super::debug_state::PauseInfo {
            func_id,
            bytecode_offset,
            source_file,
            line: current_line,
            column,
            reason,
            function_name,
        }
    }

    /// Signal debug completion or failure after a task finishes.
    ///
    /// Called by the reactor after `run()` returns a terminal result (Completed or Failed).
    /// Suspended tasks don't signal — they'll signal on final completion.
    pub fn signal_debug_result(&self, result: &ExecutionResult) {
        if let Some(ref ds) = self.debug_state {
            if ds.active.load(std::sync::atomic::Ordering::Relaxed) {
                match result {
                    ExecutionResult::Completed(value) => {
                        ds.signal_completed(value.raw() as i64);
                    }
                    ExecutionResult::Failed(err) => {
                        ds.signal_failed(err.to_string());
                    }
                    ExecutionResult::Suspended(_) => {
                        // Don't signal on suspend — task will resume later
                    }
                }
            }
        }
    }

    /// Record a backward jump (loop iteration) for profiling.
    #[cfg(feature = "jit")]
    #[inline]
    pub(in crate::vm::interpreter) fn record_loop_for_profiling(&self) {
        if let Some(ref profile) = self.module_profile {
            let count = profile.record_loop(self.current_func_id_for_profiling);
            if let Some(ref telemetry) = self.jit_telemetry {
                telemetry
                    .loop_samples
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            if count & crate::vm::defaults::JIT_POLICY_CHECK_MASK == 0 {
                if let (Some(module), Some(module_id)) = (
                    self.current_module_for_profiling.as_ref(),
                    self.current_module_id_for_profiling,
                ) {
                    self.maybe_request_compilation(
                        self.current_func_id_for_profiling,
                        module,
                        module_id,
                    );
                }
            }
        }
    }
}
