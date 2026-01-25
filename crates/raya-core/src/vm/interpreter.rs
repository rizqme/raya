//! Virtual machine interpreter

use super::{ClassRegistry, SafepointCoordinator};
use crate::{
    gc::GarbageCollector,
    object::{Array, Object, RayaString},
    scheduler::{ExceptionHandler, Scheduler, Task, TaskState},
    stack::Stack,
    value::Value,
    VmError, VmResult,
};
use raya_bytecode::{Module, Opcode};
use std::sync::Arc;

/// Raya virtual machine
pub struct Vm {
    /// Garbage collector
    gc: GarbageCollector,
    /// Operand stack
    stack: Stack,
    /// Global variables
    globals: rustc_hash::FxHashMap<String, Value>,
    /// Class registry
    pub classes: ClassRegistry,
    /// Task scheduler
    scheduler: Scheduler,
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
            classes: ClassRegistry::new(),
            scheduler,
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
        function: &raya_bytecode::module::Function,
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

        // Exception handler stack for this execution
        let mut exception_handlers: Vec<ExceptionHandler> = Vec::new();
        let mut current_exception: Option<Value> = None;

        // Track mutexes held during this execution (for auto-unlock on exception)
        let mut held_mutexes: Vec<crate::sync::MutexId> = Vec::new();

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

                // Arithmetic - Integer
                Opcode::Iadd => self.op_iadd()?,
                Opcode::Isub => self.op_isub()?,
                Opcode::Imul => self.op_imul()?,
                Opcode::Idiv => self.op_idiv()?,
                Opcode::Imod => self.op_imod()?,
                Opcode::Ineg => self.op_ineg()?,

                // Arithmetic - Float
                Opcode::Fadd => self.op_fadd()?,
                Opcode::Fsub => self.op_fsub()?,
                Opcode::Fmul => self.op_fmul()?,
                Opcode::Fdiv => self.op_fdiv()?,
                Opcode::Fneg => self.op_fneg()?,

                // Arithmetic - Number (generic)
                Opcode::Nadd => self.op_nadd()?,
                Opcode::Nsub => self.op_nsub()?,
                Opcode::Nmul => self.op_nmul()?,
                Opcode::Ndiv => self.op_ndiv()?,
                Opcode::Nmod => self.op_nmod()?,
                Opcode::Nneg => self.op_nneg()?,

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

                    let func_index = self.read_u16(code, &mut ip)?;
                    if func_index as usize >= module.functions.len() {
                        return Err(VmError::RuntimeError(format!(
                            "Invalid function index: {}",
                            func_index
                        )));
                    }
                    let callee = &module.functions[func_index as usize];

                    // Execute callee (recursive call)
                    let result = self.execute_function(callee, module)?;

                    // Push result
                    self.stack.push(result)?;
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

                    let type_index = self.read_u16(code, &mut ip)? as usize;
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

                // Method dispatch
                Opcode::CallMethod => {
                    let method_index = self.read_u16(code, &mut ip)? as usize;
                    let arg_count = self.read_u8(code, &mut ip)? as usize;
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
                    self.op_spawn(func_index, module)?;
                }
                Opcode::Await => {
                    self.op_await()?;
                }
                Opcode::WaitAll => {
                    self.op_wait_all()?;
                }

                // Exception handling
                Opcode::Try => {
                    let catch_offset = self.read_i32(code, &mut ip)?;
                    let finally_offset = self.read_i32(code, &mut ip)?;

                    // Install exception handler
                    let handler = ExceptionHandler {
                        catch_offset,
                        finally_offset,
                        stack_size: self.stack.depth(),
                        frame_count: self.stack.frame_count(),
                        mutex_count: held_mutexes.len(),
                    };
                    exception_handlers.push(handler);
                }
                Opcode::EndTry => {
                    // Remove exception handler from stack
                    // Note: If there's a finally block, the compiler should place
                    // the finally code inline after END_TRY so it executes naturally
                    exception_handlers.pop();
                }
                Opcode::Throw => {
                    // Pop exception value from stack
                    let exception = self.stack.pop()?;
                    current_exception = Some(exception);

                    // Begin exception unwinding
                    loop {
                        if let Some(handler) = exception_handlers.last().cloned() {
                            // Unwind stack to handler's saved state
                            while self.stack.depth() > handler.stack_size {
                                self.stack.pop()?;
                            }

                            // Auto-unlock mutexes acquired after this handler was installed
                            // This ensures mutexes are released during exception unwinding
                            if held_mutexes.len() > handler.mutex_count {
                                // Remove and unlock mutexes in reverse order
                                while held_mutexes.len() > handler.mutex_count {
                                    if let Some(_mutex_id) = held_mutexes.pop() {
                                        // Note: Actual mutex unlock would happen via UNLOCK opcode
                                        // or we'd need access to the mutex registry here
                                        // For now, we just track that they should be unlocked
                                    }
                                }
                            }

                            // Execute finally block if present
                            if handler.finally_offset != -1 {
                                exception_handlers.pop();
                                ip = handler.finally_offset as usize;
                                break;
                            }

                            // Jump to catch block if present
                            if handler.catch_offset != -1 {
                                exception_handlers.pop();
                                // Push exception value for catch block
                                // Keep current_exception set for potential RETHROW
                                let exc = current_exception.as_ref().unwrap().clone();
                                self.stack.push(exc)?;
                                ip = handler.catch_offset as usize;
                                break;
                            }

                            // No catch or finally, remove handler and continue unwinding
                            exception_handlers.pop();
                        } else {
                            // No handler found, propagate error
                            return Err(VmError::RuntimeError(format!(
                                "Uncaught exception: {:?}",
                                current_exception
                            )));
                        }
                    }
                }
                Opcode::Rethrow => {
                    // Re-raise the current exception
                    if let Some(_exception) = current_exception.as_ref() {
                        // Begin exception unwinding (same logic as THROW)
                        loop {
                            if let Some(handler) = exception_handlers.last().cloned() {
                                // Unwind stack to handler's saved state
                                while self.stack.depth() > handler.stack_size {
                                    self.stack.pop()?;
                                }

                                // Auto-unlock mutexes acquired after this handler was installed
                                if held_mutexes.len() > handler.mutex_count {
                                    while held_mutexes.len() > handler.mutex_count {
                                        if let Some(_mutex_id) = held_mutexes.pop() {
                                            // Mutex unlock tracking
                                        }
                                    }
                                }

                                // Execute finally block if present
                                if handler.finally_offset != -1 {
                                    exception_handlers.pop();
                                    ip = handler.finally_offset as usize;
                                    break;
                                }

                                // Jump to catch block if present
                                if handler.catch_offset != -1 {
                                    exception_handlers.pop();
                                    // Push exception value for catch block
                                    // Keep current_exception set for potential RETHROW
                                    let exc = current_exception.as_ref().unwrap().clone();
                                    self.stack.push(exc)?;
                                    ip = handler.catch_offset as usize;
                                    break;
                                }

                                // No catch or finally, remove handler and continue unwinding
                                exception_handlers.pop();
                            } else {
                                // No handler found, propagate error
                                return Err(VmError::RuntimeError(format!(
                                    "Uncaught exception: {:?}",
                                    current_exception
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

    /// ARRAY_LITERAL - Create array literal
    /// Stack: [] -> [array]
    #[allow(dead_code)]
    fn op_array_literal(&mut self, type_index: usize, length: u32) -> VmResult<()> {
        // Bounds check (reasonable maximum)
        if length > 10_000_000 {
            return Err(VmError::RuntimeError(format!(
                "Array length {} too large",
                length
            )));
        }

        // Create array with null elements
        let arr = Array::new(type_index, length as usize);

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

    // ===== Method Dispatch =====

    /// CALL_METHOD - Call method via vtable dispatch
    #[allow(dead_code)]
    fn op_call_method(
        &mut self,
        method_index: usize,
        arg_count: usize,
        module: &Module,
    ) -> VmResult<()> {
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
    fn op_spawn(&mut self, func_index: usize, module: &raya_bytecode::Module) -> VmResult<()> {
        // Safepoint poll before task creation
        self.safepoint().poll();

        // Create new Task with the given function
        let task = Arc::new(Task::new(
            func_index,
            Arc::new(module.clone()),
            None, // No parent task (VM is the spawner, not another task)
        ));

        // Spawn task on scheduler
        let task_id = self.scheduler.spawn(task).ok_or_else(|| {
            VmError::RuntimeError("Failed to spawn task: concurrent task limit reached".to_string())
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
                    return Err(VmError::RuntimeError(format!(
                        "Awaited task {:?} failed",
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
    use raya_bytecode::module::Function;

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
