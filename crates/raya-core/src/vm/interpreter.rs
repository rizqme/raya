//! Virtual machine interpreter

use raya_bytecode::{Module, Opcode};
use crate::{
    gc::GarbageCollector,
    object::{Array, Object, RayaString},
    stack::Stack,
    value::Value,
    VmError, VmResult,
};
use super::ClassRegistry;

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
}

impl Vm {
    /// Create a new VM
    pub fn new() -> Self {
        Self {
            gc: GarbageCollector::default(),
            stack: Stack::new(),
            globals: rustc_hash::FxHashMap::default(),
            classes: ClassRegistry::new(),
        }
    }

    /// Execute a module
    pub fn execute(&mut self, module: &Module) -> VmResult<Value> {
        // Validate module
        module.validate()
            .map_err(|e| VmError::RuntimeError(e))?;

        // Find main function
        let main_fn = module.functions
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
        self.stack.push_frame(
            0, // function_id (will be used later for call stack)
            0, // return IP (none for entry point)
            function.local_count,
            function.param_count,
        )?;

        let mut ip = 0;
        let code = &function.code;

        loop {
            // Bounds check
            if ip >= code.len() {
                return Err(VmError::RuntimeError(
                    "Instruction pointer out of bounds".to_string(),
                ));
            }

            // Fetch opcode
            let opcode_byte = code[ip];
            let opcode = Opcode::from_u8(opcode_byte)
                .ok_or(VmError::InvalidOpcode(opcode_byte))?;

            ip += 1;

            // Dispatch and execute
            match opcode {
                // Stack manipulation
                Opcode::Nop => {},
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
                    let index = self.read_u8(code, &mut ip)?;
                    self.op_load_local(index as usize)?;
                }
                Opcode::StoreLocal => {
                    let index = self.read_u8(code, &mut ip)?;
                    self.op_store_local(index as usize)?;
                }

                // Arithmetic - Integer
                Opcode::Iadd => self.op_iadd()?,
                Opcode::Isub => self.op_isub()?,
                Opcode::Imul => self.op_imul()?,
                Opcode::Idiv => self.op_idiv()?,
                Opcode::Imod => self.op_imod()?,
                Opcode::Ineg => self.op_ineg()?,

                // Arithmetic - Float (placeholder)
                Opcode::Fadd => self.op_fadd()?,
                Opcode::Fsub => self.op_fsub()?,
                Opcode::Fmul => self.op_fmul()?,
                Opcode::Fdiv => self.op_fdiv()?,
                Opcode::Fneg => self.op_fneg()?,

                // Comparisons - Integer
                Opcode::Ieq => self.op_ieq()?,
                Opcode::Ine => self.op_ine()?,
                Opcode::Ilt => self.op_ilt()?,
                Opcode::Ile => self.op_ile()?,
                Opcode::Igt => self.op_igt()?,
                Opcode::Ige => self.op_ige()?,

                // Control flow
                Opcode::Jmp => {
                    let offset = self.read_i16(code, &mut ip)?;
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

                // Function calls
                Opcode::Call => {
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

                // Object operations
                Opcode::New => {
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

                // Array operations
                Opcode::NewArray => {
                    let type_index = self.read_u16(code, &mut ip)? as usize;
                    self.op_new_array(type_index)?;
                }
                Opcode::LoadElem => self.op_load_elem()?,
                Opcode::StoreElem => self.op_store_elem()?,
                Opcode::ArrayLen => self.op_array_len()?,

                // String operations
                Opcode::Sconcat => self.op_sconcat()?,
                Opcode::Slen => self.op_slen()?,

                // Method dispatch
                Opcode::CallMethod => {
                    let method_index = self.read_u16(code, &mut ip)? as usize;
                    let arg_count = self.read_u8(code, &mut ip)? as usize;
                    self.op_call_method(method_index, arg_count, module)?;
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
            return Err(VmError::RuntimeError("Unexpected end of bytecode".to_string()));
        }
        let value = code[*ip];
        *ip += 1;
        Ok(value)
    }

    #[inline]
    fn read_u16(&self, code: &[u8], ip: &mut usize) -> VmResult<u16> {
        if *ip + 1 >= code.len() {
            return Err(VmError::RuntimeError("Unexpected end of bytecode".to_string()));
        }
        let value = u16::from_le_bytes([code[*ip], code[*ip + 1]]);
        *ip += 2;
        Ok(value)
    }

    #[inline]
    fn read_i16(&self, code: &[u8], ip: &mut usize) -> VmResult<i16> {
        Ok(self.read_u16(code, ip)? as i16)
    }

    #[inline]
    fn read_i32(&self, code: &[u8], ip: &mut usize) -> VmResult<i32> {
        if *ip + 3 >= code.len() {
            return Err(VmError::RuntimeError("Unexpected end of bytecode".to_string()));
        }
        let value = i32::from_le_bytes([
            code[*ip],
            code[*ip + 1],
            code[*ip + 2],
            code[*ip + 3],
        ]);
        *ip += 4;
        Ok(value)
    }

    #[inline]
    fn read_f64(&self, code: &[u8], ip: &mut usize) -> VmResult<f64> {
        if *ip + 7 >= code.len() {
            return Err(VmError::RuntimeError("Unexpected end of bytecode".to_string()));
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
        self.stack.push(a)?;
        self.stack.push(b)?;
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
    fn op_const_f64(&mut self, _value: f64) -> VmResult<()> {
        // TODO: Add f64 support to Value
        Err(VmError::RuntimeError("f64 not yet supported".to_string()))
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
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::i32(a.wrapping_add(b)))
    }

    /// ISUB - Subtract two integers
    #[inline]
    fn op_isub(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::i32(a.wrapping_sub(b)))
    }

    /// IMUL - Multiply two integers
    #[inline]
    fn op_imul(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::i32(a.wrapping_mul(b)))
    }

    /// IDIV - Divide two integers
    #[inline]
    fn op_idiv(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;

        if b == 0 {
            return Err(VmError::RuntimeError("Division by zero".to_string()));
        }

        self.stack.push(Value::i32(a / b))
    }

    /// IMOD - Modulo two integers
    #[inline]
    fn op_imod(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;

        if b == 0 {
            return Err(VmError::RuntimeError("Modulo by zero".to_string()));
        }

        self.stack.push(Value::i32(a % b))
    }

    /// INEG - Negate an integer
    #[inline]
    fn op_ineg(&mut self) -> VmResult<()> {
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::i32(-a))
    }

    // ===== Float Arithmetic Operations (Placeholder) =====

    /// FADD - Add two floats (TODO: implement when f64 added to Value)
    #[inline]
    fn op_fadd(&mut self) -> VmResult<()> {
        Err(VmError::RuntimeError("Float operations not yet supported".to_string()))
    }

    /// FSUB - Subtract two floats
    #[inline]
    fn op_fsub(&mut self) -> VmResult<()> {
        Err(VmError::RuntimeError("Float operations not yet supported".to_string()))
    }

    /// FMUL - Multiply two floats
    #[inline]
    fn op_fmul(&mut self) -> VmResult<()> {
        Err(VmError::RuntimeError("Float operations not yet supported".to_string()))
    }

    /// FDIV - Divide two floats
    #[inline]
    fn op_fdiv(&mut self) -> VmResult<()> {
        Err(VmError::RuntimeError("Float operations not yet supported".to_string()))
    }

    /// FNEG - Negate a float
    #[inline]
    fn op_fneg(&mut self) -> VmResult<()> {
        Err(VmError::RuntimeError("Float operations not yet supported".to_string()))
    }

    // ===== Comparison Operations =====

    /// IEQ - Integer equality
    #[inline]
    fn op_ieq(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::bool(a == b))
    }

    /// INE - Integer inequality
    #[inline]
    fn op_ine(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::bool(a != b))
    }

    /// ILT - Integer less than
    #[inline]
    fn op_ilt(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::bool(a < b))
    }

    /// ILE - Integer less or equal
    #[inline]
    fn op_ile(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::bool(a <= b))
    }

    /// IGT - Integer greater than
    #[inline]
    fn op_igt(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::bool(a > b))
    }

    /// IGE - Integer greater or equal
    #[inline]
    fn op_ige(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::bool(a >= b))
    }

    // ===== Global Variable Operations =====

    /// LOAD_GLOBAL - Load global variable
    #[allow(dead_code)]
    fn op_load_global(&mut self, name: &str) -> VmResult<()> {
        let value = self.globals
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
        let class = self.classes
            .get_class(class_index)
            .ok_or_else(|| VmError::RuntimeError(
                format!("Invalid class index: {}", class_index)
            ))?;

        // Create object with correct field count
        let obj = Object::new(class_index, class.field_count);

        // Allocate on GC heap
        let gc_ptr = self.gc.allocate(obj);

        // Push GC pointer as value
        let value = unsafe {
            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
        };
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
                "Expected object for field access".to_string()
            ));
        }

        // Get object from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
        let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };

        // Load field
        let value = obj.get_field(field_offset)
            .ok_or_else(|| VmError::RuntimeError(
                format!("Field offset {} out of bounds", field_offset)
            ))?;

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
                "Expected object for field access".to_string()
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

    // ===== Array Operations =====

    /// NEW_ARRAY - Allocate new array
    #[allow(dead_code)]
    fn op_new_array(&mut self, type_index: usize) -> VmResult<()> {
        // Pop length from stack
        let length_val = self.stack.pop()?;
        let length = length_val.as_i32()
            .ok_or_else(|| VmError::TypeError(
                "Array length must be a number".to_string()
            ))? as usize;

        // Bounds check (reasonable maximum)
        if length > 10_000_000 {
            return Err(VmError::RuntimeError(
                format!("Array length {} too large", length)
            ));
        }

        // Create array
        let arr = Array::new(type_index, length);

        // Allocate on GC heap
        let gc_ptr = self.gc.allocate(arr);

        // Push GC pointer as value
        let value = unsafe {
            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
        };
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
                "Expected array for element access".to_string()
            ));
        }

        // Check index is a number
        let index = index_val.as_i32()
            .ok_or_else(|| VmError::TypeError(
                "Array index must be a number".to_string()
            ))? as usize;

        // Get array from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
        let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };

        // Load element
        let value = arr.get(index)
            .ok_or_else(|| VmError::RuntimeError(
                format!("Array index {} out of bounds (length: {})", index, arr.len())
            ))?;

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
                "Expected array for element access".to_string()
            ));
        }

        // Check index is a number
        let index = index_val.as_i32()
            .ok_or_else(|| VmError::TypeError(
                "Array index must be a number".to_string()
            ))? as usize;

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
                "Expected array for length operation".to_string()
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

    // ===== String Operations =====

    /// SCONCAT - Concatenate two strings
    #[allow(dead_code)]
    fn op_sconcat(&mut self) -> VmResult<()> {
        // Pop two strings from stack
        let str2_val = self.stack.pop()?;
        let str1_val = self.stack.pop()?;

        // Check both are pointers
        if !str1_val.is_ptr() || !str2_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected strings for concatenation".to_string()
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
        let value = unsafe {
            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
        };
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
                "Expected string for length operation".to_string()
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
        let receiver_pos = self.stack.depth().checked_sub(arg_count + 1)
            .ok_or_else(|| VmError::StackUnderflow)?;

        let receiver_val = self.stack.peek_at(receiver_pos)?;

        // Check receiver is an object
        if !receiver_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected object for method call".to_string()
            ));
        }

        // Get object from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let obj_ptr = unsafe { receiver_val.as_ptr::<Object>() };
        let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };

        // Look up class
        let class = self.classes
            .get_class(obj.class_id)
            .ok_or_else(|| VmError::RuntimeError(
                format!("Invalid class ID: {}", obj.class_id)
            ))?;

        // Look up method in vtable
        let function_id = class.vtable.get_method(method_index)
            .ok_or_else(|| VmError::RuntimeError(
                format!("Method index {} not found in vtable", method_index)
            ))?;

        // Get function from module
        let function = module.functions
            .get(function_id)
            .ok_or_else(|| VmError::RuntimeError(
                format!("Invalid function ID: {}", function_id)
            ))?;

        // Execute function (implementation same as CALL)
        // Arguments are already on stack in correct order
        self.execute_function(function, module)?;

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
            code: vec![
                Opcode::ConstNull as u8,
                Opcode::Return as u8,
            ],
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
            code: vec![
                Opcode::ConstTrue as u8,
                Opcode::Return as u8,
            ],
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
            code: vec![
                Opcode::ConstFalse as u8,
                Opcode::Return as u8,
            ],
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
            code: vec![
                Opcode::ConstI32 as u8, 42, 0, 0, 0,
                Opcode::Return as u8,
            ],
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
                Opcode::ConstI32 as u8, 10, 0, 0, 0,
                Opcode::ConstI32 as u8, 20, 0, 0, 0,
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
                Opcode::ConstI32 as u8, 100, 0, 0, 0,
                Opcode::ConstI32 as u8, 25, 0, 0, 0,
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
                Opcode::ConstI32 as u8, 6, 0, 0, 0,
                Opcode::ConstI32 as u8, 7, 0, 0, 0,
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
                Opcode::ConstI32 as u8, 100, 0, 0, 0,
                Opcode::ConstI32 as u8, 5, 0, 0, 0,
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
                Opcode::ConstI32 as u8, 10, 0, 0, 0,
                Opcode::ConstI32 as u8, 0, 0, 0, 0,
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
                Opcode::ConstI32 as u8, 42, 0, 0, 0,
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
                Opcode::ConstI32 as u8, 42, 0, 0, 0,
                Opcode::StoreLocal as u8, 0,
                Opcode::ConstI32 as u8, 10, 0, 0, 0,
                Opcode::StoreLocal as u8, 1,
                Opcode::LoadLocal as u8, 0,
                Opcode::LoadLocal as u8, 1,
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
                Opcode::ConstI32 as u8, 42, 0, 0, 0,
                Opcode::ConstI32 as u8, 42, 0, 0, 0,
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
                Opcode::ConstI32 as u8, 42, 0, 0, 0,
                Opcode::ConstI32 as u8, 10, 0, 0, 0,
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
                Opcode::ConstI32 as u8, 5, 0, 0, 0,
                Opcode::ConstI32 as u8, 10, 0, 0, 0,
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
                Opcode::ConstI32 as u8, 10, 0, 0, 0,  // offset 0-4
                Opcode::ConstI32 as u8, 5, 0, 0, 0,   // offset 5-9
                Opcode::Igt as u8,                     // offset 10
                Opcode::JmpIfFalse as u8, 8, 0,       // offset 11-13, jump +8 to offset 21
                Opcode::ConstI32 as u8, 1, 0, 0, 0,   // offset 14-18 (then branch)
                Opcode::Return as u8,                  // offset 19
                // else branch starts at offset 20
                Opcode::ConstI32 as u8, 0, 0, 0, 0,   // offset 20-24
                Opcode::Return as u8,                  // offset 25
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
                Opcode::Jmp as u8, 5, 0,              // offset 0-2, jump +5 to offset 8
                Opcode::ConstI32 as u8, 99, 0, 0, 0,  // offset 3-7 (skipped)
                Opcode::ConstI32 as u8, 42, 0, 0, 0,  // offset 8-12
                Opcode::Return as u8,                  // offset 13
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::i32(42));
    }
}
