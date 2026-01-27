//! Virtual machine interpreter

use super::{ClassRegistry, SafepointCoordinator};
use crate::{
    builtin,
    gc::GarbageCollector,
    object::{Array, Closure, Object, RayaString},
    scheduler::{ExceptionHandler, Scheduler, Task, TaskState},
    stack::Stack,
    value::Value,
    VmError, VmResult,
};
use raya_compiler::{Module, Opcode};
use std::sync::Arc;

/// Raya virtual machine
pub struct Vm {
    /// Garbage collector
    gc: GarbageCollector,
    /// Operand stack
    stack: Stack,
    /// Global variables (string-keyed)
    globals: rustc_hash::FxHashMap<String, Value>,
    /// Global variables (index-based, for static fields)
    globals_by_index: Vec<Value>,
    /// Class registry
    pub classes: ClassRegistry,
    /// Task scheduler
    scheduler: Scheduler,
    /// Stack of currently executing closures (for LoadCaptured access)
    closure_stack: Vec<Value>,
    /// Exception handler stack (shared across all function calls)
    exception_handlers: Vec<ExceptionHandler>,
    /// Current exception being processed
    current_exception: Option<Value>,
    /// Held mutexes for exception unwinding
    held_mutexes: Vec<crate::sync::MutexId>,
}

impl Vm {
    /// Create a new VM with default worker count
    pub fn new() -> Self {
        let worker_count = num_cpus::get();
        Self::with_worker_count(worker_count)
    }

    /// Create a new VM with specified worker count
    pub fn with_worker_count(worker_count: usize) -> Self {
        let mut scheduler = Scheduler::new(worker_count);
        scheduler.start();

        Self {
            gc: GarbageCollector::default(),
            stack: Stack::new(),
            globals: rustc_hash::FxHashMap::default(),
            globals_by_index: Vec::new(),
            classes: ClassRegistry::new(),
            scheduler,
            closure_stack: Vec::new(),
            exception_handlers: Vec::new(),
            current_exception: None,
            held_mutexes: Vec::new(),
        }
    }

    /// Get the scheduler
    pub fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    /// Get mutable scheduler
    pub fn scheduler_mut(&mut self) -> &mut Scheduler {
        &mut self.scheduler
    }

    /// Get the safepoint coordinator
    pub fn safepoint(&self) -> &Arc<SafepointCoordinator> {
        self.scheduler.safepoint()
    }

    /// Collect GC roots from the stack
    fn collect_roots(&mut self) {
        self.gc.clear_stack_roots();

        // Add all values from the operand stack
        for i in 0..self.stack.depth() {
            if let Ok(value) = self.stack.peek_at(i) {
                if value.is_heap_allocated() {
                    self.gc.add_root(value);
                }
            }
        }

        // Add values from all call frames' local variables
        for frame in self.stack.frames() {
            let locals_start = frame.locals_start();
            let locals_count = frame.locals_count();

            for i in 0..locals_count {
                if let Ok(value) = self.stack.peek_at(locals_start + i) {
                    if value.is_heap_allocated() {
                        self.gc.add_root(value);
                    }
                }
            }
        }

        // Add global variables as roots
        for value in self.globals.values() {
            if value.is_heap_allocated() {
                self.gc.add_root(*value);
            }
        }
    }

    /// Trigger garbage collection
    pub fn collect_garbage(&mut self) {
        self.collect_roots();
        self.gc.collect();
    }

    /// Execute a module
    pub fn execute(&mut self, module: &Module) -> VmResult<Value> {
        // Validate module
        module.validate().map_err(|e| VmError::RuntimeError(e))?;

        // Register classes from the module
        for (i, class_def) in module.classes.iter().enumerate() {
            let class = crate::object::Class::new(
                i,
                class_def.name.clone(),
                class_def.field_count,
            );
            self.classes.register_class(class);
        }

        // Find main function
        let main_fn = module
            .functions
            .iter()
            .find(|f| f.name == "main")
            .ok_or_else(|| VmError::RuntimeError("No main function".to_string()))?;

        // Execute main function
        self.execute_function(main_fn, module)
    }

    /// Execute a single function
    fn execute_function(
        &mut self,
        function: &raya_compiler::Function,
        module: &Module,
    ) -> VmResult<Value> {
        // Push initial frame
        // Arguments are already on stack, push_frame will set base pointer correctly
        self.stack.push_frame(
            0, // function_id (will be used later for call stack)
            0, // return IP (none for entry point)
            function.local_count,
            function.param_count,
        )?;

        let mut ip = 0;
        let code = &function.code;

        // Use VM-level exception state (shared across function calls)
        // These are self.exception_handlers, self.current_exception, self.held_mutexes

        loop {
            // Safepoint poll at loop back-edge
            self.safepoint().poll();

            // Bounds check
            if ip >= code.len() {
                return Err(VmError::RuntimeError(
                    "Instruction pointer out of bounds".to_string(),
                ));
            }

            // Fetch opcode
            let opcode_byte = code[ip];
            let opcode = Opcode::from_u8(opcode_byte).ok_or(VmError::InvalidOpcode(opcode_byte))?;

            ip += 1;

            // Dispatch and execute
            match opcode {
                // Stack manipulation
                Opcode::Nop => {}
                Opcode::Pop => self.op_pop()?,
                Opcode::Dup => self.op_dup()?,
                Opcode::Swap => self.op_swap()?,

                // Constants
                Opcode::ConstNull => self.op_const_null()?,
                Opcode::ConstTrue => self.op_const_true()?,
                Opcode::ConstFalse => self.op_const_false()?,
                Opcode::ConstI32 => {
                    let value = self.read_i32(code, &mut ip)?;
                    self.op_const_i32(value)?;
                }
                Opcode::ConstF64 => {
                    let value = self.read_f64(code, &mut ip)?;
                    self.op_const_f64(value)?;
                }
                Opcode::ConstStr => {
                    let index = self.read_u16(code, &mut ip)? as usize;
                    let s = module.constants.strings.get(index).ok_or_else(|| {
                        VmError::RuntimeError(format!("Invalid string constant index: {}", index))
                    })?;
                    self.op_const_str(s)?;
                }

                // Local variables
                Opcode::LoadLocal => {
                    let index = self.read_u16(code, &mut ip)?;
                    self.op_load_local(index as usize)?;
                }
                Opcode::StoreLocal => {
                    let index = self.read_u16(code, &mut ip)?;
                    self.op_store_local(index as usize)?;
                }
                Opcode::LoadLocal0 => self.op_load_local(0)?,
                Opcode::LoadLocal1 => self.op_load_local(1)?,
                Opcode::StoreLocal0 => self.op_store_local(0)?,
                Opcode::StoreLocal1 => self.op_store_local(1)?,

                // Global variables (for static fields)
                Opcode::LoadGlobal => {
                    let index = self.read_u32(code, &mut ip)?;
                    self.op_load_global_index(index as usize)?;
                }
                Opcode::StoreGlobal => {
                    let index = self.read_u32(code, &mut ip)?;
                    self.op_store_global_index(index as usize)?;
                }

                // Arithmetic - Integer
                Opcode::Iadd => self.op_iadd()?,
                Opcode::Isub => self.op_isub()?,
                Opcode::Imul => self.op_imul()?,
                Opcode::Idiv => self.op_idiv()?,
                Opcode::Imod => self.op_imod()?,
                Opcode::Ineg => self.op_ineg()?,
                Opcode::Ipow => self.op_ipow()?,
                Opcode::Ishl => self.op_ishl()?,
                Opcode::Ishr => self.op_ishr()?,
                Opcode::Iushr => self.op_iushr()?,
                Opcode::Iand => self.op_iand()?,
                Opcode::Ior => self.op_ior()?,
                Opcode::Ixor => self.op_ixor()?,
                Opcode::Inot => self.op_inot()?,

                // Arithmetic - Float
                Opcode::Fadd => self.op_fadd()?,
                Opcode::Fsub => self.op_fsub()?,
                Opcode::Fmul => self.op_fmul()?,
                Opcode::Fdiv => self.op_fdiv()?,
                Opcode::Fneg => self.op_fneg()?,
                Opcode::Fpow => self.op_fpow()?,

                // Arithmetic - Number (generic)
                Opcode::Nadd => self.op_nadd()?,
                Opcode::Nsub => self.op_nsub()?,
                Opcode::Nmul => self.op_nmul()?,
                Opcode::Ndiv => self.op_ndiv()?,
                Opcode::Nmod => self.op_nmod()?,
                Opcode::Nneg => self.op_nneg()?,
                Opcode::Npow => self.op_npow()?,

                // Comparisons - Integer
                Opcode::Ieq => self.op_ieq()?,
                Opcode::Ine => self.op_ine()?,
                Opcode::Ilt => self.op_ilt()?,
                Opcode::Ile => self.op_ile()?,
                Opcode::Igt => self.op_igt()?,
                Opcode::Ige => self.op_ige()?,

                // Comparisons - Float
                Opcode::Feq => self.op_feq()?,
                Opcode::Fne => self.op_fne()?,
                Opcode::Flt => self.op_flt()?,
                Opcode::Fle => self.op_fle()?,
                Opcode::Fgt => self.op_fgt()?,
                Opcode::Fge => self.op_fge()?,

                // Comparisons - Generic
                Opcode::Eq => self.op_eq()?,
                Opcode::Ne => self.op_ne()?,
                Opcode::StrictEq => self.op_strict_eq()?,
                Opcode::StrictNe => self.op_strict_ne()?,

                // Boolean operations
                Opcode::Not => self.op_not()?,
                Opcode::And => self.op_and()?,
                Opcode::Or => self.op_or()?,

                // Control flow
                Opcode::Jmp => {
                    let offset = self.read_i16(code, &mut ip)?;
                    // Poll on backward jumps (loop back-edges)
                    if offset < 0 {
                        self.safepoint().poll();
                    }
                    ip = (ip as isize + offset as isize) as usize;
                }
                Opcode::JmpIfTrue => {
                    let offset = self.read_i16(code, &mut ip)?;
                    if self.stack.pop()?.is_truthy() {
                        ip = (ip as isize + offset as isize) as usize;
                    }
                }
                Opcode::JmpIfFalse => {
                    let offset = self.read_i16(code, &mut ip)?;
                    if !self.stack.pop()?.is_truthy() {
                        ip = (ip as isize + offset as isize) as usize;
                    }
                }
                Opcode::JmpIfNull => {
                    let offset = self.read_i16(code, &mut ip)?;
                    if self.stack.pop()?.is_null() {
                        ip = (ip as isize + offset as isize) as usize;
                    }
                }
                Opcode::JmpIfNotNull => {
                    let offset = self.read_i16(code, &mut ip)?;
                    if !self.stack.pop()?.is_null() {
                        ip = (ip as isize + offset as isize) as usize;
                    }
                }

                // Function calls
                Opcode::Call => {
                    // Safepoint poll before function call
                    self.safepoint().poll();

                    let func_index = self.read_u32(code, &mut ip)?;
                    let arg_count = self.read_u16(code, &mut ip)? as usize;

                    let result = if func_index == 0xFFFFFFFF {
                        // Closure call: closure is on stack below arguments
                        self.op_call_closure(arg_count, module)?
                    } else {
                        // Regular function call
                        if func_index as usize >= module.functions.len() {
                            return Err(VmError::RuntimeError(format!(
                                "Invalid function index: {}",
                                func_index
                            )));
                        }
                        let callee = &module.functions[func_index as usize];

                        // Execute callee (recursive call)
                        self.execute_function(callee, module)?
                    };

                    // Check for pending exception from callee
                    if self.current_exception.is_some() {
                        // Exception is propagating - find handler in this frame
                        let current_frame = self.stack.frame_count();
                        loop {
                            if let Some(handler) = self.exception_handlers.last().cloned() {
                                if handler.frame_count < current_frame {
                                    // Handler is in a caller - continue propagating
                                    self.stack.pop_frame()?;
                                    return Ok(Value::null());
                                }

                                // Unwind stack to handler's saved state
                                while self.stack.depth() > handler.stack_size {
                                    self.stack.pop()?;
                                }

                                // Jump to catch block if present
                                if handler.catch_offset != -1 {
                                    self.exception_handlers.pop();
                                    let exc = self.current_exception.take().unwrap();
                                    self.stack.push(exc)?;
                                    ip = handler.catch_offset as usize;
                                    break;
                                }

                                // No catch, try finally
                                if handler.finally_offset != -1 {
                                    self.exception_handlers.pop();
                                    ip = handler.finally_offset as usize;
                                    break;
                                }

                                self.exception_handlers.pop();
                            } else {
                                return Err(VmError::RuntimeError(format!(
                                    "Uncaught exception: {:?}",
                                    self.current_exception
                                )));
                            }
                        }
                    } else {
                        // Normal return - push result
                        self.stack.push(result)?;
                    }
                }
                Opcode::Return => {
                    // Pop return value (or null if none)
                    let return_value = if self.stack.depth() > 0 {
                        self.stack.pop()?
                    } else {
                        Value::null()
                    };

                    // Pop frame
                    self.stack.pop_frame()?;

                    return Ok(return_value);
                }
                Opcode::ReturnVoid => {
                    // Pop frame and return null
                    self.stack.pop_frame()?;
                    return Ok(Value::null());
                }

                // Object operations
                Opcode::New => {
                    // Safepoint poll before allocation
                    self.safepoint().poll();

                    let class_index = self.read_u16(code, &mut ip)? as usize;
                    self.op_new(class_index)?;
                }
                Opcode::LoadField => {
                    let field_offset = self.read_u16(code, &mut ip)? as usize;
                    self.op_load_field(field_offset)?;
                }
                Opcode::StoreField => {
                    let field_offset = self.read_u16(code, &mut ip)? as usize;
                    self.op_store_field(field_offset)?;
                }
                Opcode::LoadFieldFast => {
                    let offset = self.read_u8(code, &mut ip)?;
                    self.op_load_field_fast(offset)?;
                }
                Opcode::StoreFieldFast => {
                    let offset = self.read_u8(code, &mut ip)?;
                    self.op_store_field_fast(offset)?;
                }
                Opcode::ObjectLiteral => {
                    // Safepoint poll before allocation
                    self.safepoint().poll();

                    let class_index = self.read_u16(code, &mut ip)? as usize;
                    let field_count = self.read_u16(code, &mut ip)?;
                    self.op_object_literal(class_index, field_count)?;
                }
                Opcode::InitObject => {
                    let field_offset = self.read_u16(code, &mut ip)?;
                    self.op_init_object(field_offset)?;
                }
                Opcode::LoadStatic => {
                    let class_index = self.read_u16(code, &mut ip)? as usize;
                    let field_offset = self.read_u16(code, &mut ip)?;
                    self.op_load_static(class_index, field_offset)?;
                }
                Opcode::StoreStatic => {
                    let class_index = self.read_u16(code, &mut ip)? as usize;
                    let field_offset = self.read_u16(code, &mut ip)?;
                    self.op_store_static(class_index, field_offset)?;
                }
                Opcode::OptionalField => {
                    let field_offset = self.read_u16(code, &mut ip)?;
                    self.op_optional_field(field_offset)?;
                }

                // Array operations
                Opcode::NewArray => {
                    // Safepoint poll before allocation
                    self.safepoint().poll();

                    let type_index = self.read_u16(code, &mut ip)? as usize;
                    self.op_new_array(type_index)?;
                }
                Opcode::LoadElem => self.op_load_elem()?,
                Opcode::StoreElem => self.op_store_elem()?,
                Opcode::ArrayLen => self.op_array_len()?,
                Opcode::ArrayLiteral => {
                    // Safepoint poll before allocation
                    self.safepoint().poll();

                    let type_index = self.read_u32(code, &mut ip)? as usize;
                    let length = self.read_u32(code, &mut ip)?;
                    self.op_array_literal(type_index, length)?;
                }
                Opcode::InitArray => {
                    let index = self.read_u32(code, &mut ip)?;
                    self.op_init_array(index)?;
                }

                // String operations
                Opcode::Sconcat => self.op_sconcat()?,
                Opcode::Slen => self.op_slen()?,
                Opcode::Seq => self.op_seq()?,
                Opcode::Sne => self.op_sne()?,
                Opcode::Slt => self.op_slt()?,
                Opcode::Sle => self.op_sle()?,
                Opcode::Sgt => self.op_sgt()?,
                Opcode::Sge => self.op_sge()?,
                Opcode::ToString => self.op_to_string()?,

                // Method dispatch
                Opcode::CallMethod => {
                    let method_index = self.read_u32(code, &mut ip)? as usize;
                    let arg_count = self.read_u16(code, &mut ip)? as usize;
                    self.op_call_method(method_index, arg_count, module)?;
                }
                Opcode::CallConstructor => {
                    // Safepoint poll before allocation
                    self.safepoint().poll();

                    let class_index = self.read_u16(code, &mut ip)? as usize;
                    let arg_count = self.read_u8(code, &mut ip)? as usize;
                    self.op_call_constructor(class_index, arg_count, module)?;
                }
                Opcode::CallSuper => {
                    let class_index = self.read_u16(code, &mut ip)? as usize;
                    let arg_count = self.read_u8(code, &mut ip)? as usize;
                    self.op_call_super(class_index, arg_count, module)?;
                }

                // JSON operations
                Opcode::JsonGet => {
                    let property_index = self.read_u32(code, &mut ip)? as usize;
                    self.op_json_get(property_index, module)?;
                }
                Opcode::JsonIndex => {
                    self.op_json_index()?;
                }
                // Note: JsonCast removed - use native method value.as<T>() instead

                // Concurrency operations
                Opcode::Spawn => {
                    let func_index = self.read_u16(code, &mut ip)? as usize;
                    let arg_count = self.read_u16(code, &mut ip)? as usize;
                    self.op_spawn(func_index, arg_count, module)?;
                }
                Opcode::SpawnClosure => {
                    let arg_count = self.read_u16(code, &mut ip)? as usize;
                    self.op_spawn_closure(arg_count, module)?;
                }
                Opcode::Await => {
                    use crate::scheduler::TaskId;

                    self.safepoint().poll();

                    let task_id_val = self.stack.pop()?;
                    let task_id_u64 = task_id_val
                        .as_u64()
                        .ok_or_else(|| VmError::TypeError("Expected TaskId".to_string()))?;
                    let task_id = TaskId::from_u64(task_id_u64);

                    'await_loop: loop {
                        let task = self.scheduler.get_task(task_id).ok_or_else(|| {
                            VmError::RuntimeError(format!("Task {:?} not found", task_id))
                        })?;

                        match task.state() {
                            TaskState::Completed => {
                                let result = task.result().unwrap_or(Value::null());
                                self.stack.push(result)?;
                                break 'await_loop;
                            }
                            TaskState::Failed => {
                                if let Some(exc) = task.current_exception() {
                                    // Re-throw the exception
                                    self.current_exception = Some(exc.clone());

                                    let current_frame = self.stack.frame_count();
                                    loop {
                                        if let Some(handler) =
                                            self.exception_handlers.last().cloned()
                                        {
                                            if handler.frame_count < current_frame {
                                                self.stack.pop_frame()?;
                                                return Ok(Value::null());
                                            }

                                            while self.stack.depth() > handler.stack_size {
                                                self.stack.pop()?;
                                            }

                                            if handler.catch_offset != -1 {
                                                self.exception_handlers.pop();
                                                self.stack.push(exc)?;
                                                ip = handler.catch_offset as usize;
                                                break 'await_loop;
                                            }

                                            if handler.finally_offset != -1 {
                                                self.exception_handlers.pop();
                                                ip = handler.finally_offset as usize;
                                                break 'await_loop;
                                            }

                                            self.exception_handlers.pop();
                                        } else {
                                            return Err(VmError::RuntimeError(format!(
                                                "Uncaught exception from task {:?}",
                                                task_id
                                            )));
                                        }
                                    }
                                } else {
                                    return Err(VmError::RuntimeError(format!(
                                        "Awaited task {:?} failed",
                                        task_id
                                    )));
                                }
                            }
                            _ => {
                                self.safepoint().poll();
                                std::thread::sleep(std::time::Duration::from_micros(100));
                            }
                        }
                    }
                }
                Opcode::WaitAll => {
                    use crate::scheduler::TaskId;

                    self.safepoint().poll();

                    // Pop array pointer from stack (array is allocated on GC heap)
                    let array_val = self.stack.pop()?;
                    let array_ptr = unsafe {
                        array_val
                            .as_ptr::<Vec<Value>>()
                            .ok_or_else(|| VmError::TypeError("Expected array pointer for WAIT_ALL".to_string()))?
                    };
                    let task_array = unsafe { &*array_ptr.as_ptr() };

                    // Extract TaskIds from array
                    let mut task_ids = Vec::with_capacity(task_array.len());
                    for val in task_array.iter() {
                        let task_id_u64 = val
                            .as_u64()
                            .ok_or_else(|| VmError::TypeError("Expected TaskId (u64) in array".to_string()))?;
                        task_ids.push(TaskId::from_u64(task_id_u64));
                    }

                    // Wait for all tasks to complete
                    let mut results = Vec::with_capacity(task_ids.len());
                    'wait_all_outer: for task_id in task_ids {
                        'wait_task: loop {
                            let task = self
                                .scheduler
                                .get_task(task_id)
                                .ok_or_else(|| VmError::RuntimeError(format!("Task {:?} not found", task_id)))?;

                            match task.state() {
                                TaskState::Completed => {
                                    let result = task.result().unwrap_or(Value::null());
                                    results.push(result);
                                    break 'wait_task;
                                }
                                TaskState::Failed => {
                                    // Handle failed task - re-throw exception
                                    if let Some(exc) = task.current_exception() {
                                        self.current_exception = Some(exc.clone());

                                        let current_frame = self.stack.frame_count();
                                        loop {
                                            if let Some(handler) = self.exception_handlers.last().cloned() {
                                                if handler.frame_count < current_frame {
                                                    self.stack.pop_frame()?;
                                                    return Ok(Value::null());
                                                }

                                                while self.stack.depth() > handler.stack_size {
                                                    self.stack.pop()?;
                                                }

                                                if handler.catch_offset != -1 {
                                                    self.exception_handlers.pop();
                                                    self.stack.push(exc)?;
                                                    ip = handler.catch_offset as usize;
                                                    break 'wait_all_outer;
                                                }

                                                if handler.finally_offset != -1 {
                                                    self.exception_handlers.pop();
                                                    ip = handler.finally_offset as usize;
                                                    break 'wait_all_outer;
                                                }

                                                self.exception_handlers.pop();
                                            } else {
                                                return Err(VmError::RuntimeError(format!(
                                                    "Uncaught exception from task {:?} in WAIT_ALL",
                                                    task_id
                                                )));
                                            }
                                        }
                                    } else {
                                        return Err(VmError::RuntimeError(format!(
                                            "Task {:?} in WAIT_ALL failed",
                                            task_id
                                        )));
                                    }
                                }
                                _ => {
                                    self.safepoint().poll();
                                    std::thread::sleep(std::time::Duration::from_micros(100));
                                }
                            }
                        }
                    }

                    // All tasks completed successfully - create result array
                    let result_array_gc = self.gc.allocate(results);
                    let result_ptr = unsafe { std::ptr::NonNull::new(result_array_gc.as_ptr()).unwrap() };
                    self.stack.push(unsafe { Value::from_ptr(result_ptr) })?;
                }

                // Closure operations
                Opcode::MakeClosure => {
                    // Safepoint poll before allocation
                    self.safepoint().poll();

                    let func_index = self.read_u32(code, &mut ip)? as usize;
                    let capture_count = self.read_u16(code, &mut ip)? as usize;
                    self.op_make_closure(func_index, capture_count)?;
                }
                Opcode::LoadCaptured => {
                    let capture_index = self.read_u16(code, &mut ip)? as usize;
                    self.op_load_captured(capture_index)?;
                }
                Opcode::StoreCaptured => {
                    let capture_index = self.read_u16(code, &mut ip)? as usize;
                    self.op_store_captured(capture_index)?;
                }
                Opcode::SetClosureCapture => {
                    let capture_index = self.read_u16(code, &mut ip)? as usize;
                    self.op_set_closure_capture(capture_index)?;
                }

                // RefCell operations (for capture-by-reference)
                Opcode::NewRefCell => {
                    self.op_new_refcell()?;
                }
                Opcode::LoadRefCell => {
                    self.op_load_refcell()?;
                }
                Opcode::StoreRefCell => {
                    self.op_store_refcell()?;
                }

                // Exception handling
                Opcode::Try => {
                    // Read relative offsets and convert to absolute positions
                    // Offsets are relative to the position after reading them
                    // -1 means "no catch/finally block"
                    let catch_offset = self.read_i32(code, &mut ip)?;
                    let catch_abs = if catch_offset >= 0 {
                        (ip as i32 + catch_offset) as i32
                    } else {
                        -1 // No catch block
                    };

                    let finally_offset = self.read_i32(code, &mut ip)?;
                    let finally_abs = if finally_offset > 0 {
                        (ip as i32 + finally_offset) as i32
                    } else {
                        -1 // No finally block (0 or negative)
                    };

                    // Install exception handler with absolute positions
                    let handler = ExceptionHandler {
                        catch_offset: catch_abs,
                        finally_offset: finally_abs,
                        stack_size: self.stack.depth(),
                        frame_count: self.stack.frame_count(),
                        mutex_count: self.held_mutexes.len(),
                    };
                    self.exception_handlers.push(handler);
                }
                Opcode::EndTry => {
                    // Remove exception handler from stack
                    // Note: If there's a finally block, the compiler should place
                    // the finally code inline after END_TRY so it executes naturally
                    self.exception_handlers.pop();
                }
                Opcode::Throw => {
                    // Pop exception value from stack
                    let exception = self.stack.pop()?;
                    self.current_exception = Some(exception);

                    // Begin exception unwinding
                    let current_frame = self.stack.frame_count();
                    loop {
                        if let Some(handler) = self.exception_handlers.last().cloned() {
                            // Check if handler is in a caller frame
                            if handler.frame_count < current_frame {
                                // Handler is in a caller - return from this function
                                // The caller's Call opcode will handle the exception
                                self.stack.pop_frame()?;
                                return Ok(Value::null()); // Return value is ignored when exception is pending
                            }

                            // Unwind stack to handler's saved state
                            while self.stack.depth() > handler.stack_size {
                                self.stack.pop()?;
                            }

                            // Auto-unlock mutexes acquired after this handler was installed
                            if self.held_mutexes.len() > handler.mutex_count {
                                while self.held_mutexes.len() > handler.mutex_count {
                                    self.held_mutexes.pop();
                                }
                            }

                            // Jump to catch block if present
                            if handler.catch_offset != -1 {
                                self.exception_handlers.pop();
                                // Push exception value for catch block
                                let exc = self.current_exception.as_ref().unwrap().clone();
                                self.stack.push(exc)?;
                                ip = handler.catch_offset as usize;
                                break;
                            }

                            // No catch block, execute finally block if present
                            if handler.finally_offset != -1 {
                                self.exception_handlers.pop();
                                ip = handler.finally_offset as usize;
                                break;
                            }

                            // No catch or finally, remove handler and continue unwinding
                            self.exception_handlers.pop();
                        } else {
                            // No handler found, propagate error
                            return Err(VmError::RuntimeError(format!(
                                "Uncaught exception: {:?}",
                                self.current_exception
                            )));
                        }
                    }
                }
                Opcode::Rethrow => {
                    // Re-raise the current exception
                    if let Some(_exception) = self.current_exception.as_ref() {
                        // Begin exception unwinding (same logic as THROW)
                        loop {
                            if let Some(handler) = self.exception_handlers.last().cloned() {
                                // Unwind stack to handler's saved state
                                while self.stack.depth() > handler.stack_size {
                                    self.stack.pop()?;
                                }

                                // Auto-unlock mutexes acquired after this handler was installed
                                if self.held_mutexes.len() > handler.mutex_count {
                                    while self.held_mutexes.len() > handler.mutex_count {
                                        if let Some(_mutex_id) = self.held_mutexes.pop() {
                                            // Mutex unlock tracking
                                        }
                                    }
                                }

                                // Execute finally block if present
                                if handler.finally_offset != -1 {
                                    self.exception_handlers.pop();
                                    ip = handler.finally_offset as usize;
                                    break;
                                }

                                // Jump to catch block if present
                                if handler.catch_offset != -1 {
                                    self.exception_handlers.pop();
                                    // Push exception value for catch block
                                    // Keep current_exception set for potential RETHROW
                                    let exc = self.current_exception.as_ref().unwrap().clone();
                                    self.stack.push(exc)?;
                                    ip = handler.catch_offset as usize;
                                    break;
                                }

                                // No catch or finally, remove handler and continue unwinding
                                self.exception_handlers.pop();
                            } else {
                                // No handler found, propagate error
                                return Err(VmError::RuntimeError(format!(
                                    "Uncaught exception: {:?}",
                                    self.current_exception
                                )));
                            }
                        }
                    } else {
                        return Err(VmError::RuntimeError(
                            "RETHROW with no active exception".to_string(),
                        ));
                    }
                }

                _ => {
                    return Err(VmError::RuntimeError(format!(
                        "Unimplemented opcode: {:?}",
                        opcode
                    )));
                }
            }
        }
    }

    // ===== Helper Methods for Reading Operands =====

    #[inline]
    fn read_u8(&self, code: &[u8], ip: &mut usize) -> VmResult<u8> {
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
    fn read_u16(&self, code: &[u8], ip: &mut usize) -> VmResult<u16> {
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
    fn read_u32(&self, code: &[u8], ip: &mut usize) -> VmResult<u32> {
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
    fn read_i16(&self, code: &[u8], ip: &mut usize) -> VmResult<i16> {
        Ok(self.read_u16(code, ip)? as i16)
    }

    #[inline]
    fn read_i32(&self, code: &[u8], ip: &mut usize) -> VmResult<i32> {
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
    fn read_f64(&self, code: &[u8], ip: &mut usize) -> VmResult<f64> {
        if *ip + 7 >= code.len() {
            return Err(VmError::RuntimeError(
                "Unexpected end of bytecode".to_string(),
            ));
        }
        let value = f64::from_le_bytes([
            code[*ip],
            code[*ip + 1],
            code[*ip + 2],
            code[*ip + 3],
            code[*ip + 4],
            code[*ip + 5],
            code[*ip + 6],
            code[*ip + 7],
        ]);
        *ip += 8;
        Ok(value)
    }

    // ===== Stack Manipulation Operations =====

    /// POP - Remove top value from stack
    #[inline]
    fn op_pop(&mut self) -> VmResult<()> {
        self.stack.pop()?;
        Ok(())
    }

    /// DUP - Duplicate top stack value
    #[inline]
    fn op_dup(&mut self) -> VmResult<()> {
        let value = self.stack.peek()?;
        self.stack.push(value)?;
        Ok(())
    }

    /// SWAP - Swap top two stack values
    #[inline]
    fn op_swap(&mut self) -> VmResult<()> {
        let a = self.stack.pop()?;
        let b = self.stack.pop()?;
        self.stack.push(b)?;
        self.stack.push(a)?;
        Ok(())
    }

    // ===== Constant Operations =====

    /// CONST_NULL - Push null constant
    #[inline]
    fn op_const_null(&mut self) -> VmResult<()> {
        self.stack.push(Value::null())
    }

    /// CONST_TRUE - Push true constant
    #[inline]
    fn op_const_true(&mut self) -> VmResult<()> {
        self.stack.push(Value::bool(true))
    }

    /// CONST_FALSE - Push false constant
    #[inline]
    fn op_const_false(&mut self) -> VmResult<()> {
        self.stack.push(Value::bool(false))
    }

    /// CONST_I32 - Push 32-bit integer constant
    #[inline]
    fn op_const_i32(&mut self, value: i32) -> VmResult<()> {
        self.stack.push(Value::i32(value))
    }

    /// CONST_F64 - Push 64-bit float constant (placeholder)
    #[inline]
    fn op_const_f64(&mut self, value: f64) -> VmResult<()> {
        self.stack.push(Value::f64(value))
    }

    /// CONST_STR - Push string constant
    fn op_const_str(&mut self, s: &str) -> VmResult<()> {
        // Create a RayaString from the constant
        let raya_str = crate::object::RayaString::new(s.to_string());

        // Allocate on GC heap
        let gc_ptr = self.gc.allocate(raya_str);

        // Push as a pointer value
        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
        self.stack.push(value)
    }

    // ===== Local Variable Operations =====

    /// LOAD_LOCAL - Push local variable onto stack
    #[inline]
    fn op_load_local(&mut self, index: usize) -> VmResult<()> {
        let value = self.stack.load_local(index)?;
        self.stack.push(value)
    }

    /// STORE_LOCAL - Pop stack, store in local variable
    #[inline]
    fn op_store_local(&mut self, index: usize) -> VmResult<()> {
        let value = self.stack.pop()?;
        self.stack.store_local(index, value)
    }

    /// LOAD_GLOBAL (indexed) - Push global variable onto stack
    #[inline]
    fn op_load_global_index(&mut self, index: usize) -> VmResult<()> {
        let value = self.globals_by_index.get(index).copied().unwrap_or(Value::null());
        self.stack.push(value)
    }

    /// STORE_GLOBAL (indexed) - Pop stack, store in global variable
    #[inline]
    fn op_store_global_index(&mut self, index: usize) -> VmResult<()> {
        let value = self.stack.pop()?;
        // Grow the globals array if needed
        if index >= self.globals_by_index.len() {
            self.globals_by_index.resize(index + 1, Value::null());
        }
        self.globals_by_index[index] = value;
        Ok(())
    }

    // ===== Integer Arithmetic Operations =====

    /// IADD - Add two integers
    #[inline]
    fn op_iadd(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::i32(a.wrapping_add(b)))
    }

    /// ISUB - Subtract two integers
    #[inline]
    fn op_isub(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::i32(a.wrapping_sub(b)))
    }

    /// IMUL - Multiply two integers
    #[inline]
    fn op_imul(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::i32(a.wrapping_mul(b)))
    }

    /// IDIV - Divide two integers
    #[inline]
    fn op_idiv(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;

        if b == 0 {
            return Err(VmError::RuntimeError("Division by zero".to_string()));
        }

        self.stack.push(Value::i32(a / b))
    }

    /// IMOD - Modulo two integers
    #[inline]
    fn op_imod(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;

        if b == 0 {
            return Err(VmError::RuntimeError("Modulo by zero".to_string()));
        }

        self.stack.push(Value::i32(a % b))
    }

    /// INEG - Negate an integer
    #[inline]
    fn op_ineg(&mut self) -> VmResult<()> {
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::i32(-a))
    }

    /// IPOW - Power of two integers
    #[inline]
    fn op_ipow(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;

        if b < 0 {
            // Negative exponent for integer - return as float
            self.stack.push(Value::f64((a as f64).powi(b)))
        } else {
            self.stack.push(Value::i32(a.wrapping_pow(b as u32)))
        }
    }

    /// ISHL - Shift left two integers
    #[inline]
    fn op_ishl(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        // JavaScript shift semantics: shift amount is masked to 5 bits
        self.stack.push(Value::i32(a << (b & 0x1f)))
    }

    /// ISHR - Signed shift right two integers
    #[inline]
    fn op_ishr(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        // JavaScript shift semantics: shift amount is masked to 5 bits
        self.stack.push(Value::i32(a >> (b & 0x1f)))
    }

    /// IUSHR - Unsigned shift right two integers
    #[inline]
    fn op_iushr(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        // JavaScript shift semantics: treat a as unsigned, shift amount masked to 5 bits
        self.stack.push(Value::i32(((a as u32) >> (b & 0x1f)) as i32))
    }

    /// IAND - Bitwise AND two integers
    #[inline]
    fn op_iand(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::i32(a & b))
    }

    /// IOR - Bitwise OR two integers
    #[inline]
    fn op_ior(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::i32(a | b))
    }

    /// IXOR - Bitwise XOR two integers
    #[inline]
    fn op_ixor(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::i32(a ^ b))
    }

    /// INOT - Bitwise NOT an integer
    #[inline]
    fn op_inot(&mut self) -> VmResult<()> {
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::i32(!a))
    }

    // ===== Float Arithmetic Operations (Placeholder) =====

    /// FADD - Add two floats
    #[inline]
    fn op_fadd(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        self.stack.push(Value::f64(a + b))
    }

    /// FSUB - Subtract two floats
    #[inline]
    fn op_fsub(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        self.stack.push(Value::f64(a - b))
    }

    /// FMUL - Multiply two floats
    #[inline]
    fn op_fmul(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        self.stack.push(Value::f64(a * b))
    }

    /// FDIV - Divide two floats
    #[inline]
    fn op_fdiv(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        self.stack.push(Value::f64(a / b))
    }

    /// FNEG - Negate a float
    #[inline]
    fn op_fneg(&mut self) -> VmResult<()> {
        let a = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        self.stack.push(Value::f64(-a))
    }

    /// FPOW - Power of two floats
    #[inline]
    fn op_fpow(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        self.stack.push(Value::f64(a.powf(b)))
    }

    // ===== Number Arithmetic Operations (Generic) =====

    /// NADD - Add two numbers (i32 or f64)
    #[inline]
    fn op_nadd(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?;
        let a = self.stack.pop()?;

        // Try i32 + i32 first
        if let (Some(a_i32), Some(b_i32)) = (a.as_i32(), b.as_i32()) {
            return self.stack.push(Value::i32(a_i32.wrapping_add(b_i32)));
        }

        // Otherwise convert to f64
        let a_f64 = a
            .as_f64()
            .or_else(|| a.as_i32().map(|i| i as f64))
            .ok_or_else(|| VmError::TypeError("Expected number".to_string()))?;
        let b_f64 = b
            .as_f64()
            .or_else(|| b.as_i32().map(|i| i as f64))
            .ok_or_else(|| VmError::TypeError("Expected number".to_string()))?;
        self.stack.push(Value::f64(a_f64 + b_f64))
    }

    /// NSUB - Subtract two numbers (i32 or f64)
    #[inline]
    fn op_nsub(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?;
        let a = self.stack.pop()?;

        // Try i32 - i32 first
        if let (Some(a_i32), Some(b_i32)) = (a.as_i32(), b.as_i32()) {
            return self.stack.push(Value::i32(a_i32.wrapping_sub(b_i32)));
        }

        // Otherwise convert to f64
        let a_f64 = a
            .as_f64()
            .or_else(|| a.as_i32().map(|i| i as f64))
            .ok_or_else(|| VmError::TypeError("Expected number".to_string()))?;
        let b_f64 = b
            .as_f64()
            .or_else(|| b.as_i32().map(|i| i as f64))
            .ok_or_else(|| VmError::TypeError("Expected number".to_string()))?;
        self.stack.push(Value::f64(a_f64 - b_f64))
    }

    /// NMUL - Multiply two numbers (i32 or f64)
    #[inline]
    fn op_nmul(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?;
        let a = self.stack.pop()?;

        // Try i32 * i32 first
        if let (Some(a_i32), Some(b_i32)) = (a.as_i32(), b.as_i32()) {
            return self.stack.push(Value::i32(a_i32.wrapping_mul(b_i32)));
        }

        // Otherwise convert to f64
        let a_f64 = a
            .as_f64()
            .or_else(|| a.as_i32().map(|i| i as f64))
            .ok_or_else(|| VmError::TypeError("Expected number".to_string()))?;
        let b_f64 = b
            .as_f64()
            .or_else(|| b.as_i32().map(|i| i as f64))
            .ok_or_else(|| VmError::TypeError("Expected number".to_string()))?;
        self.stack.push(Value::f64(a_f64 * b_f64))
    }

    /// NDIV - Divide two numbers (i32 or f64)
    #[inline]
    fn op_ndiv(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?;
        let a = self.stack.pop()?;

        // Convert both to f64 for division (to match TypeScript semantics)
        let a_f64 = a
            .as_f64()
            .or_else(|| a.as_i32().map(|i| i as f64))
            .ok_or_else(|| VmError::TypeError("Expected number".to_string()))?;
        let b_f64 = b
            .as_f64()
            .or_else(|| b.as_i32().map(|i| i as f64))
            .ok_or_else(|| VmError::TypeError("Expected number".to_string()))?;
        self.stack.push(Value::f64(a_f64 / b_f64))
    }

    /// NMOD - Modulo two numbers (i32 or f64)
    #[inline]
    fn op_nmod(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?;
        let a = self.stack.pop()?;

        // Try i32 % i32 first
        if let (Some(a_i32), Some(b_i32)) = (a.as_i32(), b.as_i32()) {
            if b_i32 == 0 {
                return Err(VmError::RuntimeError("Modulo by zero".to_string()));
            }
            return self.stack.push(Value::i32(a_i32 % b_i32));
        }

        // Otherwise convert to f64
        let a_f64 = a
            .as_f64()
            .or_else(|| a.as_i32().map(|i| i as f64))
            .ok_or_else(|| VmError::TypeError("Expected number".to_string()))?;
        let b_f64 = b
            .as_f64()
            .or_else(|| b.as_i32().map(|i| i as f64))
            .ok_or_else(|| VmError::TypeError("Expected number".to_string()))?;
        self.stack.push(Value::f64(a_f64 % b_f64))
    }

    /// NNEG - Negate a number (i32 or f64)
    #[inline]
    fn op_nneg(&mut self) -> VmResult<()> {
        let a = self.stack.pop()?;

        // Try i32 first
        if let Some(a_i32) = a.as_i32() {
            return self.stack.push(Value::i32(-a_i32));
        }

        // Otherwise convert to f64
        let a_f64 = a
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected number".to_string()))?;
        self.stack.push(Value::f64(-a_f64))
    }

    /// NPOW - Power of two numbers (i32 or f64)
    #[inline]
    fn op_npow(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?;
        let a = self.stack.pop()?;

        // Try i32 ** i32 first (only if exponent is non-negative)
        if let (Some(a_i32), Some(b_i32)) = (a.as_i32(), b.as_i32()) {
            if b_i32 >= 0 {
                return self.stack.push(Value::i32(a_i32.wrapping_pow(b_i32 as u32)));
            }
        }

        // Otherwise convert to f64
        let a_f64 = a
            .as_f64()
            .or_else(|| a.as_i32().map(|i| i as f64))
            .ok_or_else(|| VmError::TypeError("Expected number".to_string()))?;
        let b_f64 = b
            .as_f64()
            .or_else(|| b.as_i32().map(|i| i as f64))
            .ok_or_else(|| VmError::TypeError("Expected number".to_string()))?;
        self.stack.push(Value::f64(a_f64.powf(b_f64)))
    }

    // ===== Comparison Operations =====

    /// IEQ - Integer equality
    #[inline]
    fn op_ieq(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::bool(a == b))
    }

    /// INE - Integer inequality
    #[inline]
    fn op_ine(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::bool(a != b))
    }

    /// ILT - Integer less than
    #[inline]
    fn op_ilt(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::bool(a < b))
    }

    /// ILE - Integer less or equal
    #[inline]
    fn op_ile(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::bool(a <= b))
    }

    /// IGT - Integer greater than
    #[inline]
    fn op_igt(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::bool(a > b))
    }

    /// IGE - Integer greater or equal
    #[inline]
    fn op_ige(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::bool(a >= b))
    }

    // ===== Float Comparison Operations =====

    /// FEQ - Float equality
    #[inline]
    fn op_feq(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        self.stack.push(Value::bool(a == b))
    }

    /// FNE - Float inequality
    #[inline]
    fn op_fne(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        self.stack.push(Value::bool(a != b))
    }

    /// FLT - Float less than
    #[inline]
    fn op_flt(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        self.stack.push(Value::bool(a < b))
    }

    /// FLE - Float less or equal
    #[inline]
    fn op_fle(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        self.stack.push(Value::bool(a <= b))
    }

    /// FGT - Float greater than
    #[inline]
    fn op_fgt(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        self.stack.push(Value::bool(a > b))
    }

    /// FGE - Float greater or equal
    #[inline]
    fn op_fge(&mut self) -> VmResult<()> {
        let b = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        let a = self
            .stack
            .pop()?
            .as_f64()
            .ok_or_else(|| VmError::TypeError("Expected f64".to_string()))?;
        self.stack.push(Value::bool(a >= b))
    }

    // ===== Generic Comparison Operations =====

    /// EQ - Generic equality (structural)
    #[inline]
    fn op_eq(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?;
        let a = self.stack.pop()?;
        // Use Value's PartialEq implementation
        self.stack.push(Value::bool(a == b))
    }

    /// NE - Generic inequality
    #[inline]
    fn op_ne(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?;
        let a = self.stack.pop()?;
        self.stack.push(Value::bool(a != b))
    }

    /// STRICT_EQ - Strict equality
    #[inline]
    fn op_strict_eq(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?;
        let a = self.stack.pop()?;
        // For now, same as generic equality
        // TODO: Implement strict equality semantics (no type coercion)
        self.stack.push(Value::bool(a == b))
    }

    /// STRICT_NE - Strict inequality
    #[inline]
    fn op_strict_ne(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?;
        let a = self.stack.pop()?;
        self.stack.push(Value::bool(a != b))
    }

    // ===== Boolean Operations =====

    /// NOT - Boolean not
    #[inline]
    fn op_not(&mut self) -> VmResult<()> {
        let value = self.stack.pop()?;
        self.stack.push(Value::bool(!value.is_truthy()))
    }

    /// AND - Boolean and
    #[inline]
    fn op_and(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?;
        let a = self.stack.pop()?;
        self.stack.push(Value::bool(a.is_truthy() && b.is_truthy()))
    }

    /// OR - Boolean or
    #[inline]
    fn op_or(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?;
        let a = self.stack.pop()?;
        self.stack.push(Value::bool(a.is_truthy() || b.is_truthy()))
    }

    // ===== Global Variable Operations =====

    /// LOAD_GLOBAL - Load global variable
    #[allow(dead_code)]
    fn op_load_global(&mut self, name: &str) -> VmResult<()> {
        let value = self
            .globals
            .get(name)
            .copied()
            .ok_or_else(|| VmError::RuntimeError(format!("Undefined global: {}", name)))?;
        self.stack.push(value)
    }

    /// STORE_GLOBAL - Store global variable
    #[allow(dead_code)]
    fn op_store_global(&mut self, name: String) -> VmResult<()> {
        let value = self.stack.pop()?;
        self.globals.insert(name, value);
        Ok(())
    }

    // ===== Object Operations =====

    /// NEW - Allocate new object
    #[allow(dead_code)]
    fn op_new(&mut self, class_index: usize) -> VmResult<()> {
        // Look up class
        let class = self.classes.get_class(class_index).ok_or_else(|| {
            VmError::RuntimeError(format!("Invalid class index: {}", class_index))
        })?;

        // Create object with correct field count
        let obj = Object::new(class_index, class.field_count);

        // Allocate on GC heap
        let gc_ptr = self.gc.allocate(obj);

        // Push GC pointer as value
        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
        self.stack.push(value)?;

        Ok(())
    }

    /// LOAD_FIELD - Load field from object
    #[allow(dead_code)]
    fn op_load_field(&mut self, field_offset: usize) -> VmResult<()> {
        // Pop object from stack
        let obj_val = self.stack.pop()?;

        // Check it's a pointer
        if !obj_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected object for field access".to_string(),
            ));
        }

        // Get object from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
        let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };

        // Load field
        let value = obj.get_field(field_offset).ok_or_else(|| {
            VmError::RuntimeError(format!("Field offset {} out of bounds", field_offset))
        })?;

        // Push field value
        self.stack.push(value)?;

        Ok(())
    }

    /// STORE_FIELD - Store value to object field
    #[allow(dead_code)]
    fn op_store_field(&mut self, field_offset: usize) -> VmResult<()> {
        // Pop value and object from stack
        let value = self.stack.pop()?;
        let obj_val = self.stack.pop()?;

        // Check it's a pointer
        if !obj_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected object for field access".to_string(),
            ));
        }

        // Get mutable object from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
        let obj = unsafe { &mut *obj_ptr.unwrap().as_ptr() };

        // Store field
        obj.set_field(field_offset, value)
            .map_err(|e| VmError::RuntimeError(e))?;

        Ok(())
    }

    /// LOAD_FIELD_FAST - Optimized field load with inline offset
    #[allow(dead_code)]
    fn op_load_field_fast(&mut self, offset: u8) -> VmResult<()> {
        // Delegate to regular LOAD_FIELD
        self.op_load_field(offset as usize)
    }

    /// STORE_FIELD_FAST - Optimized field store with inline offset
    #[allow(dead_code)]
    fn op_store_field_fast(&mut self, offset: u8) -> VmResult<()> {
        // Delegate to regular STORE_FIELD
        self.op_store_field(offset as usize)
    }

    /// OBJECT_LITERAL - Create object literal
    /// Stack: [] -> [object]
    #[allow(dead_code)]
    fn op_object_literal(&mut self, class_index: usize, field_count: u16) -> VmResult<()> {
        // Look up class
        let class = self.classes.get_class(class_index).ok_or_else(|| {
            VmError::RuntimeError(format!("Invalid class index: {}", class_index))
        })?;

        // Verify field count matches
        if class.field_count != field_count as usize {
            return Err(VmError::TypeError(format!(
                "Object literal field count mismatch: expected {}, got {}",
                class.field_count, field_count
            )));
        }

        // Create object with correct field count
        let obj = Object::new(class_index, class.field_count);

        // Allocate on GC heap
        let gc_ptr = self.gc.allocate(obj);

        // Push GC pointer as value
        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
        self.stack.push(value)?;

        Ok(())
    }

    /// INIT_OBJECT - Initialize object field
    /// Stack: [object, value] -> [object]
    #[allow(dead_code)]
    fn op_init_object(&mut self, field_offset: u16) -> VmResult<()> {
        // Pop value from stack
        let value = self.stack.pop()?;

        // Peek at object (don't pop - keep for next field initialization)
        let obj_val = self.stack.peek()?;

        // Check it's a pointer
        if !obj_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected object for field initialization".to_string(),
            ));
        }

        // Get mutable object from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
        let obj = unsafe { &mut *obj_ptr.unwrap().as_ptr() };

        // Set field
        obj.set_field(field_offset as usize, value)
            .map_err(|e| VmError::RuntimeError(e))?;

        Ok(())
    }

    /// LOAD_STATIC - Load static field from class
    /// Stack: [] -> [value]
    #[allow(dead_code)]
    fn op_load_static(&mut self, class_index: usize, field_offset: u16) -> VmResult<()> {
        // Look up class
        let class = self.classes.get_class(class_index).ok_or_else(|| {
            VmError::RuntimeError(format!("Invalid class index: {}", class_index))
        })?;

        // Load static field
        let value = class
            .get_static_field(field_offset as usize)
            .ok_or_else(|| {
                VmError::RuntimeError(format!(
                    "Static field offset {} out of bounds for class {}",
                    field_offset, class.name
                ))
            })?;

        // Push field value
        self.stack.push(value)?;

        Ok(())
    }

    /// STORE_STATIC - Store value to static field
    /// Stack: [value] -> []
    #[allow(dead_code)]
    fn op_store_static(&mut self, class_index: usize, field_offset: u16) -> VmResult<()> {
        // Pop value from stack
        let value = self.stack.pop()?;

        // Look up mutable class
        let class = self.classes.get_class_mut(class_index).ok_or_else(|| {
            VmError::RuntimeError(format!("Invalid class index: {}", class_index))
        })?;

        // Store static field
        class
            .set_static_field(field_offset as usize, value)
            .map_err(|e| VmError::RuntimeError(e))?;

        Ok(())
    }

    /// OPTIONAL_FIELD - Optional chaining field access (obj?.field)
    /// Stack: [object] -> [value or null]
    #[allow(dead_code)]
    fn op_optional_field(&mut self, field_offset: u16) -> VmResult<()> {
        // Pop object from stack
        let obj_val = self.stack.pop()?;

        // If null, push null and return
        if obj_val.is_null() {
            self.stack.push(Value::null())?;
            return Ok(());
        }

        // Check it's a pointer
        if !obj_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected object or null for optional field access".to_string(),
            ));
        }

        // Get object from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
        let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };

        // Load field
        let value = obj.get_field(field_offset as usize).ok_or_else(|| {
            VmError::RuntimeError(format!("Field offset {} out of bounds", field_offset))
        })?;

        // Push field value
        self.stack.push(value)?;

        Ok(())
    }

    // ===== Closure Operations =====

    /// MAKE_CLOSURE - Create a closure object
    /// Stack: [captures...] -> [closure]
    #[allow(dead_code)]
    fn op_make_closure(&mut self, func_index: usize, capture_count: usize) -> VmResult<()> {
        // Pop captured variables from stack (in reverse order)
        let mut captures = Vec::with_capacity(capture_count);
        for _ in 0..capture_count {
            captures.push(self.stack.pop()?);
        }
        captures.reverse(); // Restore original order

        // Create closure object
        let closure = Closure::new(func_index, captures);

        // Allocate on GC heap
        let gc_ptr = self.gc.allocate(closure);

        // Push GC pointer as value
        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
        self.stack.push(value)?;

        Ok(())
    }

    /// LOAD_CAPTURED - Load a captured variable from current closure
    /// Stack: [] -> [value]
    fn op_load_captured(&mut self, capture_index: usize) -> VmResult<()> {
        // Get the current closure from the closure stack
        let closure_val = self.closure_stack.last().ok_or_else(|| {
            VmError::RuntimeError("LoadCaptured called without active closure".to_string())
        })?;

        // Get closure from GC heap
        let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
        let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };

        // Get the captured value
        let value = closure.get_captured(capture_index).ok_or_else(|| {
            VmError::RuntimeError(format!(
                "Capture index {} out of bounds (closure has {} captures)",
                capture_index,
                closure.capture_count()
            ))
        })?;

        self.stack.push(value)?;
        Ok(())
    }

    /// STORE_CAPTURED - Store a value to a captured variable
    /// Stack: [value] -> []
    #[allow(dead_code)]
    fn op_store_captured(&mut self, _capture_index: usize) -> VmResult<()> {
        // Note: This requires tracking the current closure context
        // For now, return an error - full implementation needs closure context in call frames
        Err(VmError::RuntimeError(
            "StoreCaptured not yet fully implemented - needs closure context tracking".to_string(),
        ))
    }

    /// SET_CLOSURE_CAPTURE - Set a closure's capture to a value
    /// Stack: [closure, value] -> [closure]
    /// Used for recursive closures where the closure captures itself
    fn op_set_closure_capture(&mut self, capture_index: usize) -> VmResult<()> {
        // Pop value
        let value = self.stack.pop()?;

        // Pop closure (will be pushed back)
        let closure_val = self.stack.pop()?;

        // Check it's a pointer (closure object)
        if !closure_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected closure for SetClosureCapture".to_string(),
            ));
        }

        // Get closure from GC heap and modify its capture
        let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
        let closure = unsafe { &mut *closure_ptr.unwrap().as_ptr() };

        closure.set_captured(capture_index, value).map_err(|e| {
            VmError::RuntimeError(e)
        })?;

        // Push closure back
        self.stack.push(closure_val)?;
        Ok(())
    }

    /// NEW_REFCELL - Allocate a new RefCell with initial value
    /// Stack: [value] -> [refcell_ptr]
    fn op_new_refcell(&mut self) -> VmResult<()> {
        use crate::object::RefCell;

        // Pop initial value
        let initial_value = self.stack.pop()?;

        // Allocate RefCell on heap
        let refcell = RefCell::new(initial_value);
        let gc_ptr = self.gc.allocate(refcell);

        // Push pointer
        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
        self.stack.push(value)?;
        Ok(())
    }

    /// LOAD_REFCELL - Load value from RefCell
    /// Stack: [refcell_ptr] -> [value]
    fn op_load_refcell(&mut self) -> VmResult<()> {
        use crate::object::RefCell;

        // Pop RefCell pointer
        let refcell_val = self.stack.pop()?;

        if !refcell_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected RefCell pointer for LoadRefCell".to_string(),
            ));
        }

        // Get value from RefCell
        let refcell_ptr = unsafe { refcell_val.as_ptr::<RefCell>() };
        let refcell = unsafe { &*refcell_ptr.unwrap().as_ptr() };
        let value = refcell.get();

        // Push value
        self.stack.push(value)?;
        Ok(())
    }

    /// STORE_REFCELL - Store value to RefCell
    /// Stack: [refcell_ptr, value] -> []
    fn op_store_refcell(&mut self) -> VmResult<()> {
        use crate::object::RefCell;

        // Pop value and RefCell pointer
        let value = self.stack.pop()?;
        let refcell_val = self.stack.pop()?;

        if !refcell_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected RefCell pointer for StoreRefCell".to_string(),
            ));
        }

        // Set value in RefCell
        let refcell_ptr = unsafe { refcell_val.as_ptr::<RefCell>() };
        let refcell = unsafe { &mut *refcell_ptr.unwrap().as_ptr() };
        refcell.set(value);

        Ok(())
    }

    /// Call a closure
    /// Stack: [closure, args...] -> [result]
    fn op_call_closure(&mut self, arg_count: usize, module: &Module) -> VmResult<Value> {
        // Pop arguments (in reverse order)
        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(self.stack.pop()?);
        }
        args.reverse();

        // Pop closure
        let closure_val = self.stack.pop()?;

        // Check it's a pointer (closure object)
        if !closure_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected closure for call".to_string(),
            ));
        }

        // Get closure from GC heap
        let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
        let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };

        // Get the function
        let func_index = closure.func_id();
        if func_index >= module.functions.len() {
            return Err(VmError::RuntimeError(format!(
                "Invalid function index in closure: {}",
                func_index
            )));
        }

        // Push arguments back onto stack for the function call
        for arg in args {
            self.stack.push(arg)?;
        }

        // Push closure onto closure stack for LoadCaptured access
        self.closure_stack.push(closure_val);

        // Execute the closure's function
        let callee = &module.functions[func_index];
        let result = self.execute_function(callee, module);

        // Pop closure from closure stack
        self.closure_stack.pop();

        result
    }

    // ===== Array Operations =====

    /// NEW_ARRAY - Allocate new array
    #[allow(dead_code)]
    fn op_new_array(&mut self, type_index: usize) -> VmResult<()> {
        // Pop length from stack
        let length_val = self.stack.pop()?;
        let length = length_val
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Array length must be a number".to_string()))?
            as usize;

        // Bounds check (reasonable maximum)
        if length > 10_000_000 {
            return Err(VmError::RuntimeError(format!(
                "Array length {} too large",
                length
            )));
        }

        // Create array
        let arr = Array::new(type_index, length);

        // Allocate on GC heap
        let gc_ptr = self.gc.allocate(arr);

        // Push GC pointer as value
        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
        self.stack.push(value)?;

        Ok(())
    }

    /// LOAD_ELEM - Load array element
    #[allow(dead_code)]
    fn op_load_elem(&mut self) -> VmResult<()> {
        // Pop index and array from stack
        let index_val = self.stack.pop()?;
        let array_val = self.stack.pop()?;

        // Check array is a pointer
        if !array_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected array for element access".to_string(),
            ));
        }

        // Check index is a number
        let index = index_val
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Array index must be a number".to_string()))?
            as usize;

        // Get array from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
        let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };

        // Load element
        let value = arr.get(index).ok_or_else(|| {
            VmError::RuntimeError(format!(
                "Array index {} out of bounds (length: {})",
                index,
                arr.len()
            ))
        })?;

        // Push element value
        self.stack.push(value)?;

        Ok(())
    }

    /// STORE_ELEM - Store array element
    #[allow(dead_code)]
    fn op_store_elem(&mut self) -> VmResult<()> {
        // Pop value, index, and array from stack
        let value = self.stack.pop()?;
        let index_val = self.stack.pop()?;
        let array_val = self.stack.pop()?;

        // Check array is a pointer
        if !array_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected array for element access".to_string(),
            ));
        }

        // Check index is a number
        let index = index_val
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Array index must be a number".to_string()))?
            as usize;

        // Get mutable array from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
        let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };

        // Store element
        arr.set(index, value)
            .map_err(|e| VmError::RuntimeError(e))?;

        Ok(())
    }

    /// ARRAY_LEN - Get array length
    #[allow(dead_code)]
    fn op_array_len(&mut self) -> VmResult<()> {
        // Pop array from stack
        let array_val = self.stack.pop()?;

        // Check array is a pointer
        if !array_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected array for length operation".to_string(),
            ));
        }

        // Get array from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
        let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };

        // Push length as i32
        self.stack.push(Value::i32(arr.len() as i32))?;

        Ok(())
    }

    /// ARRAY_LITERAL - Create array literal from stack values
    /// Stack: [elem0, elem1, ..., elemN-1] -> [array]
    #[allow(dead_code)]
    fn op_array_literal(&mut self, type_index: usize, length: u32) -> VmResult<()> {
        // Bounds check (reasonable maximum)
        if length > 10_000_000 {
            return Err(VmError::RuntimeError(format!(
                "Array length {} too large",
                length
            )));
        }

        // Pop elements from stack in reverse order (last pushed = last element)
        let mut elements = Vec::with_capacity(length as usize);
        for _ in 0..length {
            elements.push(self.stack.pop()?);
        }
        // Reverse to get correct order (first pushed = first element)
        elements.reverse();

        // Create array with the elements
        let mut arr = Array::new(type_index, length as usize);
        for (i, elem) in elements.into_iter().enumerate() {
            arr.set(i, elem).map_err(|e| VmError::RuntimeError(e))?;
        }

        // Allocate on GC heap
        let gc_ptr = self.gc.allocate(arr);

        // Push GC pointer as value
        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
        self.stack.push(value)?;

        Ok(())
    }

    /// INIT_ARRAY - Initialize array element
    /// Stack: [array, value] -> [array]
    #[allow(dead_code)]
    fn op_init_array(&mut self, index: u32) -> VmResult<()> {
        // Pop value from stack
        let value = self.stack.pop()?;

        // Peek at array (don't pop - keep for next element initialization)
        let array_val = self.stack.peek()?;

        // Check array is a pointer
        if !array_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected array for element initialization".to_string(),
            ));
        }

        // Get mutable array from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
        let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };

        // Set element
        arr.set(index as usize, value)
            .map_err(|e| VmError::RuntimeError(e))?;

        Ok(())
    }

    // ===== String Operations =====

    /// SCONCAT - Concatenate two strings
    #[allow(dead_code)]
    fn op_sconcat(&mut self) -> VmResult<()> {
        // Safepoint poll before allocation
        self.safepoint().poll();

        // Pop two strings from stack
        let str2_val = self.stack.pop()?;
        let str1_val = self.stack.pop()?;

        // Check both are pointers
        if !str1_val.is_ptr() || !str2_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected strings for concatenation".to_string(),
            ));
        }

        // Get strings from GC heap
        // SAFETY: Values are tagged as pointers, managed by GC
        let str1_ptr = unsafe { str1_val.as_ptr::<RayaString>() };
        let str2_ptr = unsafe { str2_val.as_ptr::<RayaString>() };
        let str1 = unsafe { &*str1_ptr.unwrap().as_ptr() };
        let str2 = unsafe { &*str2_ptr.unwrap().as_ptr() };

        // Concatenate
        let result = str1.concat(str2);

        // Allocate result on GC heap
        let gc_ptr = self.gc.allocate(result);

        // Push result
        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
        self.stack.push(value)?;

        Ok(())
    }

    /// SLEN - Get string length
    #[allow(dead_code)]
    fn op_slen(&mut self) -> VmResult<()> {
        // Pop string from stack
        let str_val = self.stack.pop()?;

        // Check it's a pointer
        if !str_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected string for length operation".to_string(),
            ));
        }

        // Get string from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let str_ptr = unsafe { str_val.as_ptr::<RayaString>() };
        let string = unsafe { &*str_ptr.unwrap().as_ptr() };

        // Push length as i32
        self.stack.push(Value::i32(string.len() as i32))?;

        Ok(())
    }

    /// SEQ - String equality comparison with multi-level optimization
    ///
    /// Optimization levels (in order of checking):
    /// 1. Pointer equality - Same object reference → O(1)
    /// 2. Length check - Different lengths → can't be equal → O(1)
    /// 3. Hash check - Cached hash mismatch → can't be equal → O(1) amortized
    /// 4. Character comparison - Only if all else fails → O(n)
    #[allow(dead_code)]
    fn op_seq(&mut self) -> VmResult<()> {
        let str2_val = self.stack.pop()?;
        let str1_val = self.stack.pop()?;

        if !str1_val.is_ptr() || !str2_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected strings for comparison".to_string(),
            ));
        }

        // SAFETY: Values are tagged as pointers, managed by GC
        let str1_ptr = unsafe { str1_val.as_ptr::<RayaString>() };
        let str2_ptr = unsafe { str2_val.as_ptr::<RayaString>() };

        let ptr1 = str1_ptr.unwrap().as_ptr();
        let ptr2 = str2_ptr.unwrap().as_ptr();

        // Level 1: Pointer equality (O(1))
        // Same object reference means definitely equal
        if std::ptr::eq(ptr1, ptr2) {
            return self.stack.push(Value::bool(true));
        }

        let str1 = unsafe { &*ptr1 };
        let str2 = unsafe { &*ptr2 };

        // Level 2: Length check (O(1))
        // Different lengths means definitely not equal
        if str1.len() != str2.len() {
            return self.stack.push(Value::bool(false));
        }

        // Level 3: Hash check (O(1) if cached, O(n) first time)
        // Only compute hash for strings longer than threshold
        // For short strings, direct comparison is faster
        const HASH_THRESHOLD: usize = 16;
        if str1.len() > HASH_THRESHOLD {
            if str1.hash() != str2.hash() {
                return self.stack.push(Value::bool(false));
            }
        }

        // Level 4: Character comparison (O(n))
        // Only reached if all fast paths failed
        self.stack.push(Value::bool(str1.data == str2.data))
    }

    /// SNE - String inequality comparison with multi-level optimization
    #[allow(dead_code)]
    fn op_sne(&mut self) -> VmResult<()> {
        let str2_val = self.stack.pop()?;
        let str1_val = self.stack.pop()?;

        if !str1_val.is_ptr() || !str2_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected strings for comparison".to_string(),
            ));
        }

        let str1_ptr = unsafe { str1_val.as_ptr::<RayaString>() };
        let str2_ptr = unsafe { str2_val.as_ptr::<RayaString>() };

        let ptr1 = str1_ptr.unwrap().as_ptr();
        let ptr2 = str2_ptr.unwrap().as_ptr();

        // Level 1: Pointer equality - same reference means not unequal
        if std::ptr::eq(ptr1, ptr2) {
            return self.stack.push(Value::bool(false));
        }

        let str1 = unsafe { &*ptr1 };
        let str2 = unsafe { &*ptr2 };

        // Level 2: Length check - different lengths means definitely unequal
        if str1.len() != str2.len() {
            return self.stack.push(Value::bool(true));
        }

        // Level 3: Hash check for longer strings
        const HASH_THRESHOLD: usize = 16;
        if str1.len() > HASH_THRESHOLD {
            if str1.hash() != str2.hash() {
                return self.stack.push(Value::bool(true));
            }
        }

        // Level 4: Character comparison
        self.stack.push(Value::bool(str1.data != str2.data))
    }

    /// SLT - String less than comparison
    #[allow(dead_code)]
    fn op_slt(&mut self) -> VmResult<()> {
        let str2_val = self.stack.pop()?;
        let str1_val = self.stack.pop()?;

        if !str1_val.is_ptr() || !str2_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected strings for comparison".to_string(),
            ));
        }

        let str1_ptr = unsafe { str1_val.as_ptr::<RayaString>() };
        let str2_ptr = unsafe { str2_val.as_ptr::<RayaString>() };
        let str1 = unsafe { &*str1_ptr.unwrap().as_ptr() };
        let str2 = unsafe { &*str2_ptr.unwrap().as_ptr() };

        self.stack.push(Value::bool(str1.data < str2.data))
    }

    /// SLE - String less or equal comparison
    #[allow(dead_code)]
    fn op_sle(&mut self) -> VmResult<()> {
        let str2_val = self.stack.pop()?;
        let str1_val = self.stack.pop()?;

        if !str1_val.is_ptr() || !str2_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected strings for comparison".to_string(),
            ));
        }

        let str1_ptr = unsafe { str1_val.as_ptr::<RayaString>() };
        let str2_ptr = unsafe { str2_val.as_ptr::<RayaString>() };
        let str1 = unsafe { &*str1_ptr.unwrap().as_ptr() };
        let str2 = unsafe { &*str2_ptr.unwrap().as_ptr() };

        self.stack.push(Value::bool(str1.data <= str2.data))
    }

    /// SGT - String greater than comparison
    #[allow(dead_code)]
    fn op_sgt(&mut self) -> VmResult<()> {
        let str2_val = self.stack.pop()?;
        let str1_val = self.stack.pop()?;

        if !str1_val.is_ptr() || !str2_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected strings for comparison".to_string(),
            ));
        }

        let str1_ptr = unsafe { str1_val.as_ptr::<RayaString>() };
        let str2_ptr = unsafe { str2_val.as_ptr::<RayaString>() };
        let str1 = unsafe { &*str1_ptr.unwrap().as_ptr() };
        let str2 = unsafe { &*str2_ptr.unwrap().as_ptr() };

        self.stack.push(Value::bool(str1.data > str2.data))
    }

    /// SGE - String greater or equal comparison
    #[allow(dead_code)]
    fn op_sge(&mut self) -> VmResult<()> {
        let str2_val = self.stack.pop()?;
        let str1_val = self.stack.pop()?;

        if !str1_val.is_ptr() || !str2_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected strings for comparison".to_string(),
            ));
        }

        let str1_ptr = unsafe { str1_val.as_ptr::<RayaString>() };
        let str2_ptr = unsafe { str2_val.as_ptr::<RayaString>() };
        let str1 = unsafe { &*str1_ptr.unwrap().as_ptr() };
        let str2 = unsafe { &*str2_ptr.unwrap().as_ptr() };

        self.stack.push(Value::bool(str1.data >= str2.data))
    }

    /// TO_STRING - Convert a value to its string representation
    #[allow(dead_code)]
    fn op_to_string(&mut self) -> VmResult<()> {
        // Safepoint poll before allocation
        self.safepoint().poll();

        let val = self.stack.pop()?;

        // Convert value based on its type
        let result_string = if val.is_null() {
            "null".to_string()
        } else if val.is_bool() {
            if let Some(b) = val.as_bool() {
                if b { "true".to_string() } else { "false".to_string() }
            } else {
                "false".to_string()
            }
        } else if val.is_i32() {
            if let Some(i) = val.as_i32() {
                i.to_string()
            } else {
                "0".to_string()
            }
        } else if val.is_f64() {
            if let Some(f) = val.as_f64() {
                f.to_string()
            } else {
                "0".to_string()
            }
        } else if val.is_ptr() {
            // Check if it's already a string - just push it back
            let ptr = unsafe { val.as_ptr::<RayaString>() };
            if ptr.is_some() {
                // It's a string, just push it back
                self.stack.push(val)?;
                return Ok(());
            }
            // Other pointer types - use "[object]" placeholder
            "[object]".to_string()
        } else {
            // Unknown type
            "[unknown]".to_string()
        };

        // Allocate result string on GC heap
        let result = RayaString::new(result_string);
        let gc_ptr = self.gc.allocate(result);

        // Push result
        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
        self.stack.push(value)?;

        Ok(())
    }

    // ===== Method Dispatch =====

    /// CALL_METHOD - Call method via vtable dispatch or built-in method
    #[allow(dead_code)]
    fn op_call_method(
        &mut self,
        method_index: usize,
        arg_count: usize,
        module: &Module,
    ) -> VmResult<()> {
        let method_id = method_index as u16;

        // Check for built-in array methods
        if builtin::is_array_method(method_id) {
            return self.call_array_method(method_id, arg_count);
        }

        // Check for built-in string methods
        if builtin::is_string_method(method_id) {
            return self.call_string_method(method_id, arg_count);
        }

        // Fall through to vtable dispatch for user-defined methods
        // Peek at object (receiver) on stack without popping
        // Object is at stack position: stack_top - arg_count
        let receiver_pos = self
            .stack
            .depth()
            .checked_sub(arg_count + 1)
            .ok_or_else(|| VmError::StackUnderflow)?;

        let receiver_val = self.stack.peek_at(receiver_pos)?;

        // Check receiver is an object
        if !receiver_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected object for method call".to_string(),
            ));
        }

        // Get object from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let obj_ptr = unsafe { receiver_val.as_ptr::<Object>() };
        let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };

        // Look up class
        let class = self
            .classes
            .get_class(obj.class_id)
            .ok_or_else(|| VmError::RuntimeError(format!("Invalid class ID: {}", obj.class_id)))?;

        // Look up method in vtable
        let function_id = class.vtable.get_method(method_index).ok_or_else(|| {
            VmError::RuntimeError(format!("Method index {} not found in vtable", method_index))
        })?;

        // Get function from module
        let function = module.functions.get(function_id).ok_or_else(|| {
            VmError::RuntimeError(format!("Invalid function ID: {}", function_id))
        })?;

        // Execute function (implementation same as CALL)
        // Arguments are already on stack in correct order
        self.execute_function(function, module)?;

        Ok(())
    }

    /// Execute built-in array method
    fn call_array_method(&mut self, method_id: u16, arg_count: usize) -> VmResult<()> {
        // Stack layout: [array, arg1, arg2, ...] (arg_count arguments)
        // For push: [array, value] -> [new_length]
        // For pop: [array] -> [popped_value]

        match method_id {
            builtin::array::PUSH => {
                // push(value) - arg_count should be 1
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.push expects 1 argument, got {}",
                        arg_count
                    )));
                }

                let value = self.stack.pop()?;
                let array_val = self.stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError(
                        "Expected array for push method".to_string(),
                    ));
                }

                // Get mutable array reference
                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };

                // Push element and return new length
                let new_len = arr.push(value);
                self.stack.push(Value::i32(new_len as i32))?;

                Ok(())
            }

            builtin::array::POP => {
                // pop() - arg_count should be 0
                if arg_count != 0 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.pop expects 0 arguments, got {}",
                        arg_count
                    )));
                }

                let array_val = self.stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError(
                        "Expected array for pop method".to_string(),
                    ));
                }

                // Get mutable array reference
                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };

                // Pop element and return it (or null if empty)
                let result = arr.pop().unwrap_or(Value::null());
                self.stack.push(result)?;

                Ok(())
            }

            builtin::array::SHIFT => {
                // shift() - arg_count should be 0
                if arg_count != 0 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.shift expects 0 arguments, got {}",
                        arg_count
                    )));
                }

                let array_val = self.stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError(
                        "Expected array for shift method".to_string(),
                    ));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };

                let result = arr.shift().unwrap_or(Value::null());
                self.stack.push(result)?;

                Ok(())
            }

            builtin::array::UNSHIFT => {
                // unshift(value) - arg_count should be 1
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.unshift expects 1 argument, got {}",
                        arg_count
                    )));
                }

                let value = self.stack.pop()?;
                let array_val = self.stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError(
                        "Expected array for unshift method".to_string(),
                    ));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };

                let new_len = arr.unshift(value);
                self.stack.push(Value::i32(new_len as i32))?;

                Ok(())
            }

            builtin::array::INDEX_OF => {
                // indexOf(value) - arg_count should be 1
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.indexOf expects 1 argument, got {}",
                        arg_count
                    )));
                }

                let value = self.stack.pop()?;
                let array_val = self.stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError(
                        "Expected array for indexOf method".to_string(),
                    ));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };

                let index = arr.index_of(value);
                self.stack.push(Value::i32(index))?;

                Ok(())
            }

            builtin::array::INCLUDES => {
                // includes(value) - arg_count should be 1
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.includes expects 1 argument, got {}",
                        arg_count
                    )));
                }

                let value = self.stack.pop()?;
                let array_val = self.stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError(
                        "Expected array for includes method".to_string(),
                    ));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };

                let result = arr.includes(value);
                self.stack.push(Value::bool(result))?;

                Ok(())
            }

            _ => Err(VmError::RuntimeError(format!(
                "Unimplemented array method: 0x{:04X}",
                method_id
            ))),
        }
    }

    /// Execute built-in string method (placeholder)
    fn call_string_method(&mut self, method_id: u16, _arg_count: usize) -> VmResult<()> {
        // TODO: Implement string methods
        Err(VmError::RuntimeError(format!(
            "String method 0x{:04X} not yet implemented",
            method_id
        )))
    }

    /// CALL_CONSTRUCTOR - Call class constructor
    /// Stack: [arg1, arg2, ...] -> [new object]
    #[allow(dead_code)]
    fn op_call_constructor(
        &mut self,
        class_index: usize,
        arg_count: usize,
        module: &Module,
    ) -> VmResult<()> {
        // Look up class
        let class = self.classes.get_class(class_index).ok_or_else(|| {
            VmError::RuntimeError(format!("Invalid class index: {}", class_index))
        })?;

        // Create new object
        let obj = Object::new(class_index, class.field_count);

        // Allocate on GC heap
        let gc_ptr = self.gc.allocate(obj);
        let obj_value =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };

        // If class has a constructor, call it with the new object as receiver
        if let Some(constructor_id) = class.constructor_id {
            // Push object onto stack (as receiver)
            self.stack.push(obj_value)?;

            // Get constructor function
            let function = module.functions.get(constructor_id).ok_or_else(|| {
                VmError::RuntimeError(format!(
                    "Invalid constructor function ID: {}",
                    constructor_id
                ))
            })?;

            // Arguments are already on stack before the object
            // Need to move object before arguments for `this` binding
            // Stack before: [arg1, arg2, ..., obj]
            // Stack after: [obj, arg1, arg2, ...]

            // Pop object first
            let obj = self.stack.pop()?;

            // Pop arguments
            let mut args = Vec::new();
            for _ in 0..arg_count {
                args.push(self.stack.pop()?);
            }

            // Push in correct order: object first, then arguments (reversed)
            self.stack.push(obj)?;
            for arg in args.iter().rev() {
                self.stack.push(*arg)?;
            }

            // Execute constructor function
            self.execute_function(function, module)?;

            // Constructor returns null, but we return the object
            // Pop constructor's null result
            self.stack.pop()?;
            // Push the object as the result
            self.stack.push(obj_value)?;
        } else {
            // No constructor, just push the new object
            self.stack.push(obj_value)?;
        }

        Ok(())
    }

    /// CALL_SUPER - Call parent class constructor
    /// Stack: [this, arg1, arg2, ...] -> []
    #[allow(dead_code)]
    fn op_call_super(
        &mut self,
        class_index: usize,
        arg_count: usize,
        module: &Module,
    ) -> VmResult<()> {
        // Look up class
        let class = self.classes.get_class(class_index).ok_or_else(|| {
            VmError::RuntimeError(format!("Invalid class index: {}", class_index))
        })?;

        // Get parent class ID
        let parent_id = class
            .parent_id
            .ok_or_else(|| VmError::RuntimeError(format!("Class {} has no parent", class.name)))?;

        // Look up parent class
        let parent_class = self.classes.get_class(parent_id).ok_or_else(|| {
            VmError::RuntimeError(format!("Invalid parent class ID: {}", parent_id))
        })?;

        // Get parent constructor
        let parent_constructor_id = parent_class
            .constructor_id
            .ok_or_else(|| VmError::RuntimeError(format!("Parent class has no constructor")))?;

        // Get constructor function
        let function = module.functions.get(parent_constructor_id).ok_or_else(|| {
            VmError::RuntimeError(format!(
                "Invalid parent constructor function ID: {}",
                parent_constructor_id
            ))
        })?;

        // Arguments and `this` are already on stack in correct order
        // Stack: [this, arg1, arg2, ...]
        // Execute parent constructor
        self.execute_function(function, module)?;

        Ok(())
    }

    // ===== JSON Operations =====

    /// JSON_GET - Get property from JSON object
    #[allow(dead_code)]
    fn op_json_get(&mut self, property_index: usize, module: &Module) -> VmResult<()> {
        // Safepoint poll before potential allocation
        self.safepoint().poll();

        // Pop JSON value from stack
        let json_val = self.stack.pop()?;

        // Get JSON pointer
        if !json_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected JSON value for property access".to_string(),
            ));
        }

        // SAFETY: Value is tagged as pointer, managed by GC
        let json_ptr = unsafe { json_val.as_ptr::<crate::json::JsonValue>() };
        let json = unsafe { &*json_ptr.unwrap().as_ptr() };

        // Get property name from constant pool
        let property_name = module
            .constants
            .get_string(property_index as u32)
            .ok_or_else(|| {
                VmError::RuntimeError(format!("Invalid property index: {}", property_index))
            })?;

        // Get property value
        let result = json.get_property(property_name);

        // Allocate result on heap
        let result_ptr = self.gc.allocate(result);

        // Push result onto stack
        let value = unsafe {
            Value::from_ptr(std::ptr::NonNull::new_unchecked(
                result_ptr.as_ptr() as *mut u8
            ))
        };
        self.stack.push(value)?;

        Ok(())
    }

    /// JSON_INDEX - Get element from JSON array by index
    #[allow(dead_code)]
    fn op_json_index(&mut self) -> VmResult<()> {
        // Safepoint poll before potential allocation
        self.safepoint().poll();

        // Pop index from stack
        let index_val = self.stack.pop()?;
        let index = index_val
            .as_i32()
            .ok_or_else(|| VmError::TypeError("Expected integer index".to_string()))?;

        if index < 0 {
            return Err(VmError::RuntimeError(
                "Array index cannot be negative".to_string(),
            ));
        }

        // Pop JSON value from stack
        let json_val = self.stack.pop()?;

        // Get JSON pointer
        if !json_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected JSON value for index access".to_string(),
            ));
        }

        // SAFETY: Value is tagged as pointer, managed by GC
        let json_ptr = unsafe { json_val.as_ptr::<crate::json::JsonValue>() };
        let json = unsafe { &*json_ptr.unwrap().as_ptr() };

        // Get element at index
        let result = json.get_index(index as usize);

        // Allocate result on heap
        let result_ptr = self.gc.allocate(result);

        // Push result onto stack
        let value = unsafe {
            Value::from_ptr(std::ptr::NonNull::new_unchecked(
                result_ptr.as_ptr() as *mut u8
            ))
        };
        self.stack.push(value)?;

        Ok(())
    }

    /// JSON_CAST - Cast JSON value to typed object with validation
    #[allow(dead_code)]
    fn op_json_cast(&mut self, type_id: usize) -> VmResult<()> {
        // Safepoint poll before potential allocation
        self.safepoint().poll();

        // Pop JSON value from stack
        let json_val = self.stack.pop()?;

        // Get JSON pointer
        if !json_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected JSON value for type cast".to_string(),
            ));
        }

        // SAFETY: Value is tagged as pointer, managed by GC
        let json_ptr = unsafe { json_val.as_ptr::<crate::json::JsonValue>() };
        let _json = unsafe { &*json_ptr.unwrap().as_ptr() };

        // TODO: Get type schema from type registry
        // For now, just return an error indicating not implemented
        return Err(VmError::RuntimeError(format!(
            "JSON type casting not yet fully implemented for type ID: {}",
            type_id
        )));

        // Future implementation:
        // 1. Get TypeSchema from type registry using type_id
        // 2. Get TypeSchemaRegistry (needs to be added to Vm struct)
        // 3. Call validate_cast(json, schema, schema_registry, &mut self.gc)
        // 4. Push resulting typed value onto stack
    }

    // ===== Concurrency Operations =====

    /// SPAWN - Create a new Task and start it
    #[allow(dead_code)]
    fn op_spawn(
        &mut self,
        func_index: usize,
        arg_count: usize,
        module: &raya_compiler::Module,
    ) -> VmResult<()> {
        // Safepoint poll before task creation
        self.safepoint().poll();

        // Pop arguments from the stack (they were pushed in reverse order by codegen)
        // Popping gives us the correct order, so no reverse needed
        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(self.stack.pop()?);
        }

        // Create new Task with the given function and args
        let task = Arc::new(Task::with_args(
            func_index,
            Arc::new(module.clone()),
            None, // No parent task (VM is the spawner, not another task)
            args,
        ));

        // Spawn task on scheduler
        let task_id = self.scheduler.spawn(task).ok_or_else(|| {
            VmError::RuntimeError("Failed to spawn task: concurrent task limit reached".to_string())
        })?;

        // Push TaskId as u64 value onto stack
        self.stack.push(Value::u64(task_id.as_u64()))?;

        Ok(())
    }

    /// SPAWN_CLOSURE - Spawn a new Task from a closure
    #[allow(dead_code)]
    fn op_spawn_closure(
        &mut self,
        arg_count: usize,
        module: &raya_compiler::Module,
    ) -> VmResult<()> {
        // Safepoint poll before task creation
        self.safepoint().poll();

        // Pop closure from stack
        let closure_val = self.stack.pop()?;
        if !closure_val.is_ptr() {
            return Err(VmError::TypeError("Expected closure for SpawnClosure".to_string()));
        }

        let closure_ptr = unsafe { closure_val.as_ptr::<crate::object::Closure>() };
        let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };

        // Pop arguments (already in correct order after pop)
        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(self.stack.pop()?);
        }

        // Prepend captures to args - captures become first locals in the spawned task
        let mut task_args = closure.captures.clone();
        task_args.extend(args);

        // Create new Task with the closure's function
        let task = Arc::new(Task::with_args(
            closure.func_id,
            Arc::new(module.clone()),
            None,
            task_args,
        ));

        // Spawn task on scheduler
        let task_id = self.scheduler.spawn(task).ok_or_else(|| {
            VmError::RuntimeError("Failed to spawn closure task: concurrent task limit reached".to_string())
        })?;

        // Push TaskId as u64 value onto stack
        self.stack.push(Value::u64(task_id.as_u64()))?;

        Ok(())
    }

    /// AWAIT - Wait for a Task to complete and get its result
    #[allow(dead_code)]
    fn op_await(&mut self) -> VmResult<()> {
        // Safepoint poll on await entry
        self.safepoint().poll();

        // Pop TaskId from stack
        let task_id_val = self.stack.pop()?;
        let task_id_u64 = task_id_val
            .as_u64()
            .ok_or_else(|| VmError::TypeError("Expected TaskId (u64) for AWAIT".to_string()))?;

        use crate::scheduler::TaskId;
        let task_id = TaskId::from_u64(task_id_u64);

        // Wait for task to complete
        loop {
            // Get task from scheduler
            let task = self
                .scheduler
                .get_task(task_id)
                .ok_or_else(|| VmError::RuntimeError(format!("Task {:?} not found", task_id)))?;

            let state = task.state();

            match state {
                TaskState::Completed => {
                    // Get result and push to stack
                    let result = task.result().unwrap_or(Value::null());
                    self.stack.push(result)?;
                    return Ok(());
                }
                TaskState::Failed => {
                    // Check if the task has an exception to re-throw
                    if let Some(exc) = task.current_exception() {
                        // Push exception onto stack and trigger throw handling
                        self.stack.push(exc.clone())?;
                        self.current_exception = Some(exc);

                        // Begin exception unwinding
                        let current_frame = self.stack.frame_count();
                        loop {
                            if let Some(handler) = self.exception_handlers.last().cloned() {
                                if handler.frame_count < current_frame {
                                    self.stack.pop_frame()?;
                                    return Ok(());
                                }

                                while self.stack.depth() > handler.stack_size {
                                    self.stack.pop()?;
                                }

                                if handler.catch_offset != -1 {
                                    self.exception_handlers.pop();
                                    // Exception already on stack
                                    return Ok(());
                                }

                                if handler.finally_offset != -1 {
                                    self.exception_handlers.pop();
                                    return Ok(());
                                }

                                self.exception_handlers.pop();
                            } else {
                                return Err(VmError::RuntimeError(format!(
                                    "Uncaught exception from task {:?}",
                                    task_id
                                )));
                            }
                        }
                    } else {
                        return Err(VmError::RuntimeError(format!(
                            "Awaited task {:?} failed",
                            task_id
                        )));
                    }
                }
                _ => {
                    // Task still running, poll safepoint and yield
                    self.safepoint().poll();
                    std::thread::sleep(std::time::Duration::from_micros(100));
                }
            }
        }
    }

    /// WAIT_ALL - Wait for all tasks in an array to complete
    /// Stack: [array_of_task_ids] -> [array_of_results]
    #[allow(dead_code)]
    fn op_wait_all(&mut self) -> VmResult<()> {
        use crate::scheduler::TaskId;

        // Safepoint poll on wait_all entry
        self.safepoint().poll();

        // Pop array pointer from stack (array is allocated on GC heap)
        let array_val = self.stack.pop()?;

        // Get pointer to Vec<Value> from the GC'd value
        let array_ptr = unsafe {
            array_val
                .as_ptr::<Vec<Value>>()
                .ok_or_else(|| VmError::TypeError("Expected array pointer for WAIT_ALL".to_string()))?
        };

        let task_array = unsafe { &*array_ptr.as_ptr() };

        // Extract TaskIds from array
        let mut task_ids = Vec::with_capacity(task_array.len());
        for val in task_array.iter() {
            let task_id_u64 = val
                .as_u64()
                .ok_or_else(|| VmError::TypeError("Expected TaskId (u64) in array".to_string()))?;
            task_ids.push(TaskId::from_u64(task_id_u64));
        }

        // Wait for all tasks to complete
        let mut results = Vec::with_capacity(task_ids.len());
        for task_id in task_ids {
            loop {
                // Get task from scheduler
                let task = self
                    .scheduler
                    .get_task(task_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("Task {:?} not found", task_id)))?;

                let state = task.state();

                match state {
                    TaskState::Completed => {
                        // Get result
                        let result = task.result().unwrap_or(Value::null());
                        results.push(result);
                        break; // Move to next task
                    }
                    TaskState::Failed => {
                        return Err(VmError::RuntimeError(format!(
                            "Task {:?} in WAIT_ALL failed",
                            task_id
                        )));
                    }
                    _ => {
                        // Task still running, poll safepoint and yield
                        self.safepoint().poll();
                        std::thread::sleep(std::time::Duration::from_micros(100));
                    }
                }
            }
        }

        // Create result array (GC-allocated Vec<Value>) and push to stack
        let result_array_gc = self.gc.allocate(results);
        let result_ptr = unsafe { std::ptr::NonNull::new(result_array_gc.as_ptr()).unwrap() };
        self.stack.push(unsafe { Value::from_ptr(result_ptr) })?;

        Ok(())
    }
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raya_compiler::Function;

    #[test]
    fn test_vm_creation() {
        let _vm = Vm::new();
    }

    #[test]
    fn test_const_null() {
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![Opcode::ConstNull as u8, Opcode::Return as u8],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::null());
    }

    #[test]
    fn test_const_true() {
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![Opcode::ConstTrue as u8, Opcode::Return as u8],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_const_false() {
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![Opcode::ConstFalse as u8, Opcode::Return as u8],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::bool(false));
    }

    #[test]
    fn test_const_i32() {
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![Opcode::ConstI32 as u8, 42, 0, 0, 0, Opcode::Return as u8],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::i32(42));
    }

    #[test]
    fn test_simple_arithmetic() {
        // 10 + 20 = 30
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                10,
                0,
                0,
                0,
                Opcode::ConstI32 as u8,
                20,
                0,
                0,
                0,
                Opcode::Iadd as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::i32(30));
    }

    #[test]
    fn test_arithmetic_subtraction() {
        // 100 - 25 = 75
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                100,
                0,
                0,
                0,
                Opcode::ConstI32 as u8,
                25,
                0,
                0,
                0,
                Opcode::Isub as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::i32(75));
    }

    #[test]
    fn test_arithmetic_multiplication() {
        // 6 * 7 = 42
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                6,
                0,
                0,
                0,
                Opcode::ConstI32 as u8,
                7,
                0,
                0,
                0,
                Opcode::Imul as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::i32(42));
    }

    #[test]
    fn test_arithmetic_division() {
        // 100 / 5 = 20
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                100,
                0,
                0,
                0,
                Opcode::ConstI32 as u8,
                5,
                0,
                0,
                0,
                Opcode::Idiv as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::i32(20));
    }

    #[test]
    fn test_division_by_zero() {
        // 10 / 0 should error
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                10,
                0,
                0,
                0,
                Opcode::ConstI32 as u8,
                0,
                0,
                0,
                0,
                Opcode::Idiv as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), VmError::RuntimeError(_)));
    }

    #[test]
    fn test_stack_operations() {
        // Test DUP: push 42, dup, add
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                42,
                0,
                0,
                0,
                Opcode::Dup as u8,
                Opcode::Iadd as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::i32(84));
    }

    #[test]
    fn test_local_variables() {
        // local x = 42
        // local y = 10
        // return x + y
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 2,
            code: vec![
                Opcode::ConstI32 as u8,
                42,
                0,
                0,
                0,
                Opcode::StoreLocal as u8,
                0, 0, // u16 index 0
                Opcode::ConstI32 as u8,
                10,
                0,
                0,
                0,
                Opcode::StoreLocal as u8,
                1, 0, // u16 index 1
                Opcode::LoadLocal as u8,
                0, 0, // u16 index 0
                Opcode::LoadLocal as u8,
                1, 0, // u16 index 1
                Opcode::Iadd as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::i32(52));
    }

    #[test]
    fn test_comparison_equal() {
        // 42 == 42
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                42,
                0,
                0,
                0,
                Opcode::ConstI32 as u8,
                42,
                0,
                0,
                0,
                Opcode::Ieq as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_comparison_not_equal() {
        // 42 != 10
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                42,
                0,
                0,
                0,
                Opcode::ConstI32 as u8,
                10,
                0,
                0,
                0,
                Opcode::Ine as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_comparison_less_than() {
        // 5 < 10
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                5,
                0,
                0,
                0,
                Opcode::ConstI32 as u8,
                10,
                0,
                0,
                0,
                Opcode::Ilt as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_conditional_branch() {
        // if (10 > 5) { return 1 } else { return 0 }
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                10,
                0,
                0,
                0, // offset 0-4
                Opcode::ConstI32 as u8,
                5,
                0,
                0,
                0,                 // offset 5-9
                Opcode::Igt as u8, // offset 10
                Opcode::JmpIfFalse as u8,
                8,
                0, // offset 11-13, jump +8 to offset 21
                Opcode::ConstI32 as u8,
                1,
                0,
                0,
                0,                    // offset 14-18 (then branch)
                Opcode::Return as u8, // offset 19
                // else branch starts at offset 20
                Opcode::ConstI32 as u8,
                0,
                0,
                0,
                0,                    // offset 20-24
                Opcode::Return as u8, // offset 25
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::i32(1));
    }

    #[test]
    fn test_unconditional_jump() {
        // Jump over some code
        // After JMP instruction (offset 0), IP is at 1
        // After reading i16 offset (2 bytes), IP is at 3
        // Jump offset of +5 makes IP = 3 + 5 = 8 (start of second CONST_I32)
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::Jmp as u8,
                5,
                0, // offset 0-2, jump +5 to offset 8
                Opcode::ConstI32 as u8,
                99,
                0,
                0,
                0, // offset 3-7 (skipped)
                Opcode::ConstI32 as u8,
                42,
                0,
                0,
                0,                    // offset 8-12
                Opcode::Return as u8, // offset 13
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::i32(42));
    }
}
