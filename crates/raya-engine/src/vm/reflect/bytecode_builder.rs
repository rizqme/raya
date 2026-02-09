//! Dynamic Bytecode Generation for Reflection API
//!
//! Provides a JIT-style bytecode emission API for runtime-created functions.
//! This module implements Phase 15: Dynamic Bytecode Generation.
//!
//! ## Native Call IDs (0x0DF0-0x0DFF)
//!
//! | ID     | Method                      | Description                          |
//! |--------|-----------------------------|------------------------------------- |
//! | 0x0DF0 | newBytecodeBuilder          | Create bytecode builder              |
//! | 0x0DF1 | builderEmit                 | Emit instruction                     |
//! | 0x0DF2 | builderEmitPush             | Push constant value                  |
//! | 0x0DF3 | builderDefineLabel          | Define a new label                   |
//! | 0x0DF4 | builderMarkLabel            | Mark label position                  |
//! | 0x0DF5 | builderEmitJump             | Emit unconditional jump              |
//! | 0x0DF6 | builderEmitJumpIf           | Emit conditional jump                |
//! | 0x0DF7 | builderDeclareLocal         | Declare local variable               |
//! | 0x0DF8 | builderEmitLoadLocal        | Load local variable                  |
//! | 0x0DF9 | builderEmitStoreLocal       | Store local variable                 |
//! | 0x0DFA | builderEmitCall             | Emit function call                   |
//! | 0x0DFB | builderEmitReturn           | Emit return instruction              |
//! | 0x0DFC | builderValidate             | Validate bytecode                    |
//! | 0x0DFD | builderBuildFunction        | Build and register function          |
//! | 0x0DFE | extendModule                | Extend module with dynamic code      |

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::vm::VmError;

/// Opcode constants (mirrors compiler/bytecode/opcode.rs)
pub mod opcode {
    // Stack manipulation
    pub const NOP: u8 = 0x00;
    pub const POP: u8 = 0x01;
    pub const DUP: u8 = 0x02;
    pub const SWAP: u8 = 0x03;
    pub const CONST_NULL: u8 = 0x04;
    pub const CONST_TRUE: u8 = 0x05;
    pub const CONST_FALSE: u8 = 0x06;
    pub const CONST_I32: u8 = 0x07;
    pub const CONST_F64: u8 = 0x08;
    pub const CONST_STR: u8 = 0x09;
    pub const LOAD_CONST: u8 = 0x0A;

    // Local variables
    pub const LOAD_LOCAL: u8 = 0x10;
    pub const STORE_LOCAL: u8 = 0x11;
    pub const LOAD_LOCAL_0: u8 = 0x12;
    pub const LOAD_LOCAL_1: u8 = 0x13;
    pub const STORE_LOCAL_0: u8 = 0x14;
    pub const STORE_LOCAL_1: u8 = 0x15;

    // Integer arithmetic
    pub const IADD: u8 = 0x20;
    pub const ISUB: u8 = 0x21;
    pub const IMUL: u8 = 0x22;
    pub const IDIV: u8 = 0x23;
    pub const IMOD: u8 = 0x24;
    pub const INEG: u8 = 0x25;

    // Float arithmetic
    pub const FADD: u8 = 0x30;
    pub const FSUB: u8 = 0x31;
    pub const FMUL: u8 = 0x32;
    pub const FDIV: u8 = 0x33;
    pub const FNEG: u8 = 0x34;

    // Number arithmetic (dynamic)
    pub const NADD: u8 = 0x40;
    pub const NSUB: u8 = 0x41;
    pub const NMUL: u8 = 0x42;
    pub const NDIV: u8 = 0x43;
    pub const NMOD: u8 = 0x44;
    pub const NNEG: u8 = 0x45;

    // Integer comparison
    pub const IEQ: u8 = 0x50;
    pub const INE: u8 = 0x51;
    pub const ILT: u8 = 0x52;
    pub const ILE: u8 = 0x53;
    pub const IGT: u8 = 0x54;
    pub const IGE: u8 = 0x55;

    // Float comparison
    pub const FEQ: u8 = 0x60;
    pub const FNE: u8 = 0x61;
    pub const FLT: u8 = 0x62;
    pub const FLE: u8 = 0x63;
    pub const FGT: u8 = 0x64;
    pub const FGE: u8 = 0x65;

    // Generic comparison
    pub const EQ: u8 = 0x70;
    pub const NE: u8 = 0x71;
    pub const NOT: u8 = 0x74;

    // String operations
    pub const SCONCAT: u8 = 0x80;
    pub const SLEN: u8 = 0x81;

    // Control flow
    pub const JMP: u8 = 0x90;
    pub const JMP_IF_FALSE: u8 = 0x91;
    pub const JMP_IF_TRUE: u8 = 0x92;
    pub const JMP_IF_NULL: u8 = 0x93;
    pub const JMP_IF_NOT_NULL: u8 = 0x94;

    // Function calls
    pub const CALL: u8 = 0xA0;
    pub const CALL_METHOD: u8 = 0xA1;
    pub const RETURN: u8 = 0xA2;
    pub const RETURN_VOID: u8 = 0xA3;

    // Object operations
    pub const NEW: u8 = 0xB0;
    pub const LOAD_FIELD: u8 = 0xB1;
    pub const STORE_FIELD: u8 = 0xB2;

    // Array operations
    pub const NEW_ARRAY: u8 = 0xC0;
    pub const ARRAY_LOAD: u8 = 0xC1;
    pub const ARRAY_STORE: u8 = 0xC2;
    pub const ARRAY_LENGTH: u8 = 0xC3;

    // Native calls
    pub const NATIVE_CALL: u8 = 0xA8;
}

/// Global counter for builder IDs
static NEXT_BUILDER_ID: AtomicUsize = AtomicUsize::new(1);

/// Generate a unique builder ID
fn generate_builder_id() -> usize {
    NEXT_BUILDER_ID.fetch_add(1, Ordering::Relaxed)
}

/// Global counter for dynamic function IDs (starting from high range)
static NEXT_DYNAMIC_FUNC_ID: AtomicUsize = AtomicUsize::new(0x8000_0000);

/// Generate a unique dynamic function ID
fn generate_dynamic_function_id() -> usize {
    NEXT_DYNAMIC_FUNC_ID.fetch_add(1, Ordering::Relaxed)
}

/// A label for jump targets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Label {
    /// Unique label ID within the builder
    pub id: usize,
}

/// Unresolved jump that needs label patching
#[derive(Debug, Clone)]
struct UnresolvedJump {
    /// Position in bytecode where the offset needs to be written
    offset_position: usize,
    /// Target label
    target_label: Label,
}

/// Type tracked on the operand stack for validation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackType {
    /// Integer (i32/i64)
    Integer,
    /// Float (f64)
    Float,
    /// Number (could be int or float)
    Number,
    /// Boolean
    Boolean,
    /// String
    String,
    /// Object reference
    Object,
    /// Array reference
    Array,
    /// Null
    Null,
    /// Unknown/any type
    Unknown,
}

/// Local variable information
#[derive(Debug, Clone)]
pub struct LocalVariable {
    /// Variable name (optional, for debugging)
    pub name: Option<String>,
    /// Variable type
    pub var_type: StackType,
    /// Index in local variable table
    pub index: usize,
}

/// Builder for constructing bytecode programmatically
#[derive(Debug)]
pub struct BytecodeBuilder {
    /// Unique builder ID
    pub id: usize,
    /// Function name
    pub name: String,
    /// Parameter count
    pub param_count: usize,
    /// Return type name
    pub return_type: String,
    /// Bytecode buffer (pre-allocated)
    bytecode: Vec<u8>,
    /// Local variables
    locals: Vec<LocalVariable>,
    /// Next label ID
    next_label_id: usize,
    /// Label positions (label ID -> bytecode offset)
    label_positions: HashMap<usize, usize>,
    /// Unresolved jumps that need patching
    unresolved_jumps: Vec<UnresolvedJump>,
    /// Operand stack for validation (tracks types)
    type_stack: Vec<StackType>,
    /// Maximum stack depth reached
    max_stack_depth: usize,
    /// Constant pool (for strings and other constants)
    constants: Vec<ConstantValue>,
    /// Whether build() has been called
    finalized: bool,
    /// Validation errors accumulated
    errors: Vec<String>,
}

/// Constant value in the constant pool
#[derive(Debug, Clone)]
pub enum ConstantValue {
    /// String constant
    String(String),
    /// Integer constant
    Integer(i64),
    /// Float constant
    Float(f64),
}

impl BytecodeBuilder {
    /// Create a new bytecode builder with pre-allocated capacity
    pub fn new(name: String, param_count: usize, return_type: String) -> Self {
        Self {
            id: generate_builder_id(),
            name,
            param_count,
            return_type,
            bytecode: Vec::with_capacity(256), // Pre-allocate for common case
            locals: Vec::with_capacity(16),
            next_label_id: 0,
            label_positions: HashMap::new(),
            unresolved_jumps: Vec::new(),
            type_stack: Vec::with_capacity(16),
            max_stack_depth: 0,
            constants: Vec::new(),
            finalized: false,
            errors: Vec::new(),
        }
    }

    /// Get current bytecode offset
    fn current_offset(&self) -> usize {
        self.bytecode.len()
    }

    /// Track stack push for validation
    fn push_type(&mut self, t: StackType) {
        self.type_stack.push(t);
        if self.type_stack.len() > self.max_stack_depth {
            self.max_stack_depth = self.type_stack.len();
        }
    }

    /// Track stack pop for validation
    fn pop_type(&mut self) -> Option<StackType> {
        self.type_stack.pop()
    }

    /// Emit a single byte
    fn emit_byte(&mut self, byte: u8) {
        self.bytecode.push(byte);
    }

    /// Emit a u16 in little-endian
    fn emit_u16(&mut self, value: u16) {
        self.bytecode.extend_from_slice(&value.to_le_bytes());
    }

    /// Emit a u32 in little-endian
    fn emit_u32(&mut self, value: u32) {
        self.bytecode.extend_from_slice(&value.to_le_bytes());
    }

    /// Emit an i32 in little-endian
    fn emit_i32(&mut self, value: i32) {
        self.bytecode.extend_from_slice(&value.to_le_bytes());
    }

    /// Emit a f64 in little-endian
    fn emit_f64(&mut self, value: f64) {
        self.bytecode.extend_from_slice(&value.to_le_bytes());
    }

    // ===== Public Emission Methods =====

    /// Emit a raw opcode with operands
    pub fn emit(&mut self, opcode: u8, operands: &[u8]) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized BytecodeBuilder".to_string(),
            ));
        }
        self.emit_byte(opcode);
        for &op in operands {
            self.emit_byte(op);
        }
        Ok(())
    }

    /// Emit NOP
    pub fn emit_nop(&mut self) -> Result<(), VmError> {
        self.emit(opcode::NOP, &[])
    }

    /// Emit POP
    pub fn emit_pop(&mut self) -> Result<(), VmError> {
        self.pop_type();
        self.emit(opcode::POP, &[])
    }

    /// Emit DUP
    pub fn emit_dup(&mut self) -> Result<(), VmError> {
        if let Some(t) = self.type_stack.last().copied() {
            self.push_type(t);
        }
        self.emit(opcode::DUP, &[])
    }

    /// Push null constant
    pub fn emit_push_null(&mut self) -> Result<(), VmError> {
        self.push_type(StackType::Null);
        self.emit(opcode::CONST_NULL, &[])
    }

    /// Push boolean constant
    pub fn emit_push_bool(&mut self, value: bool) -> Result<(), VmError> {
        self.push_type(StackType::Boolean);
        if value {
            self.emit(opcode::CONST_TRUE, &[])
        } else {
            self.emit(opcode::CONST_FALSE, &[])
        }
    }

    /// Push i32 constant
    pub fn emit_push_i32(&mut self, value: i32) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized BytecodeBuilder".to_string(),
            ));
        }
        self.push_type(StackType::Integer);
        self.emit_byte(opcode::CONST_I32);
        self.emit_i32(value);
        Ok(())
    }

    /// Push f64 constant
    pub fn emit_push_f64(&mut self, value: f64) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized BytecodeBuilder".to_string(),
            ));
        }
        self.push_type(StackType::Float);
        self.emit_byte(opcode::CONST_F64);
        self.emit_f64(value);
        Ok(())
    }

    /// Push string constant
    pub fn emit_push_string(&mut self, value: String) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized BytecodeBuilder".to_string(),
            ));
        }
        let index = self.constants.len();
        self.constants.push(ConstantValue::String(value));
        self.push_type(StackType::String);
        self.emit_byte(opcode::CONST_STR);
        self.emit_u32(index as u32);
        Ok(())
    }

    // ===== Local Variables =====

    /// Declare a local variable
    pub fn declare_local(&mut self, name: Option<String>, var_type: StackType) -> Result<usize, VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized BytecodeBuilder".to_string(),
            ));
        }
        let index = self.locals.len();
        self.locals.push(LocalVariable {
            name,
            var_type,
            index,
        });
        Ok(index)
    }

    /// Emit load local variable
    pub fn emit_load_local(&mut self, index: usize) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized BytecodeBuilder".to_string(),
            ));
        }
        // Track type if we know it
        let var_type = self.locals.get(index).map(|l| l.var_type).unwrap_or(StackType::Unknown);
        self.push_type(var_type);

        // Use optimized opcodes for indices 0 and 1
        match index {
            0 => self.emit_byte(opcode::LOAD_LOCAL_0),
            1 => self.emit_byte(opcode::LOAD_LOCAL_1),
            _ => {
                self.emit_byte(opcode::LOAD_LOCAL);
                self.emit_u16(index as u16);
            }
        }
        Ok(())
    }

    /// Emit store local variable
    pub fn emit_store_local(&mut self, index: usize) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized BytecodeBuilder".to_string(),
            ));
        }
        self.pop_type();

        match index {
            0 => self.emit_byte(opcode::STORE_LOCAL_0),
            1 => self.emit_byte(opcode::STORE_LOCAL_1),
            _ => {
                self.emit_byte(opcode::STORE_LOCAL);
                self.emit_u16(index as u16);
            }
        }
        Ok(())
    }

    // ===== Labels and Control Flow =====

    /// Define a new label (returns label that can be used for jumps)
    pub fn define_label(&mut self) -> Label {
        let label = Label { id: self.next_label_id };
        self.next_label_id += 1;
        label
    }

    /// Mark the current position with a label
    pub fn mark_label(&mut self, label: Label) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized BytecodeBuilder".to_string(),
            ));
        }
        self.label_positions.insert(label.id, self.current_offset());
        Ok(())
    }

    /// Emit unconditional jump to label
    pub fn emit_jump(&mut self, label: Label) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized BytecodeBuilder".to_string(),
            ));
        }
        self.emit_byte(opcode::JMP);
        let offset_position = self.current_offset();
        self.emit_i32(0); // Placeholder, will be patched
        self.unresolved_jumps.push(UnresolvedJump {
            offset_position,
            target_label: label,
        });
        Ok(())
    }

    /// Emit jump if false (pops condition)
    pub fn emit_jump_if_false(&mut self, label: Label) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized BytecodeBuilder".to_string(),
            ));
        }
        self.pop_type();
        self.emit_byte(opcode::JMP_IF_FALSE);
        let offset_position = self.current_offset();
        self.emit_i32(0); // Placeholder
        self.unresolved_jumps.push(UnresolvedJump {
            offset_position,
            target_label: label,
        });
        Ok(())
    }

    /// Emit jump if true (pops condition)
    pub fn emit_jump_if_true(&mut self, label: Label) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized BytecodeBuilder".to_string(),
            ));
        }
        self.pop_type();
        self.emit_byte(opcode::JMP_IF_TRUE);
        let offset_position = self.current_offset();
        self.emit_i32(0); // Placeholder
        self.unresolved_jumps.push(UnresolvedJump {
            offset_position,
            target_label: label,
        });
        Ok(())
    }

    // ===== Arithmetic Operations =====

    /// Emit integer addition
    pub fn emit_iadd(&mut self) -> Result<(), VmError> {
        self.pop_type();
        self.pop_type();
        self.push_type(StackType::Integer);
        self.emit(opcode::IADD, &[])
    }

    /// Emit integer subtraction
    pub fn emit_isub(&mut self) -> Result<(), VmError> {
        self.pop_type();
        self.pop_type();
        self.push_type(StackType::Integer);
        self.emit(opcode::ISUB, &[])
    }

    /// Emit integer multiplication
    pub fn emit_imul(&mut self) -> Result<(), VmError> {
        self.pop_type();
        self.pop_type();
        self.push_type(StackType::Integer);
        self.emit(opcode::IMUL, &[])
    }

    /// Emit integer division
    pub fn emit_idiv(&mut self) -> Result<(), VmError> {
        self.pop_type();
        self.pop_type();
        self.push_type(StackType::Integer);
        self.emit(opcode::IDIV, &[])
    }

    /// Emit float addition
    pub fn emit_fadd(&mut self) -> Result<(), VmError> {
        self.pop_type();
        self.pop_type();
        self.push_type(StackType::Float);
        self.emit(opcode::FADD, &[])
    }

    /// Emit float subtraction
    pub fn emit_fsub(&mut self) -> Result<(), VmError> {
        self.pop_type();
        self.pop_type();
        self.push_type(StackType::Float);
        self.emit(opcode::FSUB, &[])
    }

    /// Emit number addition (dynamic)
    pub fn emit_nadd(&mut self) -> Result<(), VmError> {
        self.pop_type();
        self.pop_type();
        self.push_type(StackType::Number);
        self.emit(opcode::NADD, &[])
    }

    /// Emit number subtraction (dynamic)
    pub fn emit_nsub(&mut self) -> Result<(), VmError> {
        self.pop_type();
        self.pop_type();
        self.push_type(StackType::Number);
        self.emit(opcode::NSUB, &[])
    }

    // ===== Comparison Operations =====

    /// Emit integer equality comparison
    pub fn emit_ieq(&mut self) -> Result<(), VmError> {
        self.pop_type();
        self.pop_type();
        self.push_type(StackType::Boolean);
        self.emit(opcode::IEQ, &[])
    }

    /// Emit integer less than comparison
    pub fn emit_ilt(&mut self) -> Result<(), VmError> {
        self.pop_type();
        self.pop_type();
        self.push_type(StackType::Boolean);
        self.emit(opcode::ILT, &[])
    }

    /// Emit integer greater than comparison
    pub fn emit_igt(&mut self) -> Result<(), VmError> {
        self.pop_type();
        self.pop_type();
        self.push_type(StackType::Boolean);
        self.emit(opcode::IGT, &[])
    }

    /// Emit generic equality
    pub fn emit_eq(&mut self) -> Result<(), VmError> {
        self.pop_type();
        self.pop_type();
        self.push_type(StackType::Boolean);
        self.emit(opcode::EQ, &[])
    }

    /// Emit logical NOT
    pub fn emit_not(&mut self) -> Result<(), VmError> {
        self.pop_type();
        self.push_type(StackType::Boolean);
        self.emit(opcode::NOT, &[])
    }

    // ===== Function Calls =====

    /// Emit function call
    pub fn emit_call(&mut self, function_id: u32, arg_count: u16) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized BytecodeBuilder".to_string(),
            ));
        }
        // Pop arguments
        for _ in 0..arg_count {
            self.pop_type();
        }
        // Push return value (unknown type)
        self.push_type(StackType::Unknown);

        self.emit_byte(opcode::CALL);
        self.emit_u32(function_id);
        self.emit_u16(arg_count);
        Ok(())
    }

    /// Emit native call
    pub fn emit_native_call(&mut self, native_id: u16, arg_count: u16) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized BytecodeBuilder".to_string(),
            ));
        }
        // Pop arguments
        for _ in 0..arg_count {
            self.pop_type();
        }
        // Push return value
        self.push_type(StackType::Unknown);

        self.emit_byte(opcode::NATIVE_CALL);
        self.emit_u16(native_id);
        self.emit_u16(arg_count);
        Ok(())
    }

    /// Emit return with value
    pub fn emit_return(&mut self) -> Result<(), VmError> {
        self.pop_type();
        self.emit(opcode::RETURN, &[])
    }

    /// Emit return void
    pub fn emit_return_void(&mut self) -> Result<(), VmError> {
        self.emit(opcode::RETURN_VOID, &[])
    }

    // ===== Object Operations =====

    /// Emit new object allocation
    pub fn emit_new(&mut self, class_id: u32) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized BytecodeBuilder".to_string(),
            ));
        }
        self.push_type(StackType::Object);
        self.emit_byte(opcode::NEW);
        self.emit_u32(class_id);
        Ok(())
    }

    /// Emit load field
    pub fn emit_load_field(&mut self, field_offset: u16) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized BytecodeBuilder".to_string(),
            ));
        }
        self.pop_type(); // Pop object
        self.push_type(StackType::Unknown); // Push field value
        self.emit_byte(opcode::LOAD_FIELD);
        self.emit_u16(field_offset);
        Ok(())
    }

    /// Emit store field
    pub fn emit_store_field(&mut self, field_offset: u16) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized BytecodeBuilder".to_string(),
            ));
        }
        self.pop_type(); // Pop value
        self.pop_type(); // Pop object
        self.emit_byte(opcode::STORE_FIELD);
        self.emit_u16(field_offset);
        Ok(())
    }

    // ===== Array Operations =====

    /// Emit new array
    pub fn emit_new_array(&mut self, element_type: u32) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized BytecodeBuilder".to_string(),
            ));
        }
        self.pop_type(); // Pop length
        self.push_type(StackType::Array);
        self.emit_byte(opcode::NEW_ARRAY);
        self.emit_u32(element_type);
        Ok(())
    }

    /// Emit array load
    pub fn emit_array_load(&mut self) -> Result<(), VmError> {
        self.pop_type(); // Pop index
        self.pop_type(); // Pop array
        self.push_type(StackType::Unknown);
        self.emit(opcode::ARRAY_LOAD, &[])
    }

    /// Emit array store
    pub fn emit_array_store(&mut self) -> Result<(), VmError> {
        self.pop_type(); // Pop value
        self.pop_type(); // Pop index
        self.pop_type(); // Pop array
        self.emit(opcode::ARRAY_STORE, &[])
    }

    // ===== Validation =====

    /// Validate the bytecode
    pub fn validate(&mut self) -> ValidationResult {
        self.errors.clear();

        // Check all labels are marked
        for jump in &self.unresolved_jumps {
            if !self.label_positions.contains_key(&jump.target_label.id) {
                self.errors.push(format!(
                    "Label {} is used but never marked",
                    jump.target_label.id
                ));
            }
        }

        // Check stack is balanced (should be 0 or 1 for return value)
        if self.type_stack.len() > 1 {
            self.errors.push(format!(
                "Stack not balanced: {} values remaining",
                self.type_stack.len()
            ));
        }

        ValidationResult {
            is_valid: self.errors.is_empty(),
            errors: self.errors.clone(),
        }
    }

    /// Build the function, resolving all labels
    pub fn build(&mut self) -> Result<CompiledFunction, VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "BytecodeBuilder already finalized".to_string(),
            ));
        }

        // Validate first
        let validation = self.validate();
        if !validation.is_valid {
            return Err(VmError::RuntimeError(format!(
                "Bytecode validation failed: {}",
                validation.errors.join("; ")
            )));
        }

        // Resolve all jump labels
        for jump in &self.unresolved_jumps {
            let target_offset = self.label_positions[&jump.target_label.id];
            // Calculate relative offset from the instruction after the jump
            let relative_offset = (target_offset as i32) - ((jump.offset_position + 4) as i32);
            let bytes = relative_offset.to_le_bytes();
            self.bytecode[jump.offset_position] = bytes[0];
            self.bytecode[jump.offset_position + 1] = bytes[1];
            self.bytecode[jump.offset_position + 2] = bytes[2];
            self.bytecode[jump.offset_position + 3] = bytes[3];
        }

        self.finalized = true;

        Ok(CompiledFunction {
            function_id: generate_dynamic_function_id(),
            name: self.name.clone(),
            param_count: self.param_count,
            local_count: self.locals.len().max(self.param_count),
            max_stack: self.max_stack_depth,
            bytecode: self.bytecode.clone(),
            constants: self.constants.clone(),
        })
    }
}

/// Result of bytecode validation
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Whether validation passed
    pub is_valid: bool,
    /// Validation errors
    pub errors: Vec<String>,
}

/// A compiled function ready for execution
#[derive(Debug, Clone)]
pub struct CompiledFunction {
    /// Function ID assigned by the builder
    pub function_id: usize,
    /// Function name
    pub name: String,
    /// Parameter count
    pub param_count: usize,
    /// Local variable count
    pub local_count: usize,
    /// Maximum stack depth
    pub max_stack: usize,
    /// Compiled bytecode
    pub bytecode: Vec<u8>,
    /// Constant pool
    pub constants: Vec<ConstantValue>,
}

/// Registry for active BytecodeBuilders
#[derive(Debug, Default)]
pub struct BytecodeBuilderRegistry {
    /// Active builders by ID
    builders: HashMap<usize, BytecodeBuilder>,
    /// Compiled functions by ID
    functions: HashMap<usize, CompiledFunction>,
}

impl BytecodeBuilderRegistry {
    /// Create a new registry
    pub fn new() -> Self {
        Self {
            builders: HashMap::new(),
            functions: HashMap::new(),
        }
    }

    /// Create and register a new builder
    pub fn create_builder(&mut self, name: String, param_count: usize, return_type: String) -> usize {
        let builder = BytecodeBuilder::new(name, param_count, return_type);
        let id = builder.id;
        self.builders.insert(id, builder);
        id
    }

    /// Get a builder by ID
    pub fn get(&self, id: usize) -> Option<&BytecodeBuilder> {
        self.builders.get(&id)
    }

    /// Get a mutable builder by ID
    pub fn get_mut(&mut self, id: usize) -> Option<&mut BytecodeBuilder> {
        self.builders.get_mut(&id)
    }

    /// Remove a builder (after it's been built)
    pub fn remove(&mut self, id: usize) -> Option<BytecodeBuilder> {
        self.builders.remove(&id)
    }

    /// Register a compiled function
    pub fn register_function(&mut self, func: CompiledFunction) -> usize {
        let id = func.function_id;
        self.functions.insert(id, func);
        id
    }

    /// Get a compiled function by ID
    pub fn get_function(&self, id: usize) -> Option<&CompiledFunction> {
        self.functions.get(&id)
    }

    /// Get all function IDs
    pub fn function_ids(&self) -> impl Iterator<Item = usize> + '_ {
        self.functions.keys().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytecode_builder_creation() {
        let builder = BytecodeBuilder::new("test".to_string(), 2, "number".to_string());
        assert_eq!(builder.name, "test");
        assert_eq!(builder.param_count, 2);
        assert!(!builder.finalized);
    }

    #[test]
    fn test_emit_constants() {
        let mut builder = BytecodeBuilder::new("test".to_string(), 0, "void".to_string());

        builder.emit_push_null().unwrap();
        builder.emit_push_bool(true).unwrap();
        builder.emit_push_bool(false).unwrap();
        builder.emit_push_i32(42).unwrap();

        assert!(builder.bytecode.len() > 0);
        assert_eq!(builder.type_stack.len(), 4);
    }

    #[test]
    fn test_emit_arithmetic() {
        let mut builder = BytecodeBuilder::new("add".to_string(), 2, "number".to_string());

        builder.declare_local(Some("a".to_string()), StackType::Integer).unwrap();
        builder.declare_local(Some("b".to_string()), StackType::Integer).unwrap();

        builder.emit_load_local(0).unwrap();
        builder.emit_load_local(1).unwrap();
        builder.emit_iadd().unwrap();
        builder.emit_return().unwrap();

        let result = builder.build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_labels_and_jumps() {
        let mut builder = BytecodeBuilder::new("cond".to_string(), 1, "number".to_string());

        builder.declare_local(None, StackType::Boolean).unwrap();

        let else_label = builder.define_label();
        let end_label = builder.define_label();

        builder.emit_load_local(0).unwrap();
        builder.emit_jump_if_false(else_label).unwrap();

        builder.emit_push_i32(1).unwrap();
        builder.emit_jump(end_label).unwrap();

        builder.mark_label(else_label).unwrap();
        builder.emit_push_i32(0).unwrap();

        builder.mark_label(end_label).unwrap();
        builder.emit_return().unwrap();

        let result = builder.build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validation_unmarked_label() {
        let mut builder = BytecodeBuilder::new("test".to_string(), 0, "void".to_string());

        let label = builder.define_label();
        builder.emit_jump(label).unwrap();
        // Don't mark the label

        let validation = builder.validate();
        assert!(!validation.is_valid);
        assert!(validation.errors[0].contains("never marked"));
    }

    #[test]
    fn test_cannot_modify_finalized() {
        let mut builder = BytecodeBuilder::new("test".to_string(), 0, "void".to_string());
        builder.emit_return_void().unwrap();
        builder.build().unwrap();

        let result = builder.emit_push_i32(42);
        assert!(result.is_err());
    }

    #[test]
    fn test_registry() {
        let mut registry = BytecodeBuilderRegistry::new();

        let id = registry.create_builder("test".to_string(), 0, "void".to_string());
        assert!(registry.get(id).is_some());

        let builder = registry.get_mut(id).unwrap();
        builder.emit_return_void().unwrap();
        let func = builder.build().unwrap();

        let func_id = registry.register_function(func);
        assert!(registry.get_function(func_id).is_some());
    }

    #[test]
    fn test_local_variables() {
        let mut builder = BytecodeBuilder::new("locals".to_string(), 0, "void".to_string());

        let idx0 = builder.declare_local(Some("x".to_string()), StackType::Integer).unwrap();
        let idx1 = builder.declare_local(Some("y".to_string()), StackType::Integer).unwrap();

        assert_eq!(idx0, 0);
        assert_eq!(idx1, 1);

        builder.emit_push_i32(10).unwrap();
        builder.emit_store_local(0).unwrap();
        builder.emit_push_i32(20).unwrap();
        builder.emit_store_local(1).unwrap();

        builder.emit_load_local(0).unwrap();
        builder.emit_load_local(1).unwrap();
        builder.emit_iadd().unwrap();
        builder.emit_return().unwrap();

        let func = builder.build().unwrap();
        assert_eq!(func.local_count, 2);
    }

    #[test]
    fn test_function_call() {
        let mut builder = BytecodeBuilder::new("caller".to_string(), 0, "number".to_string());

        builder.emit_push_i32(1).unwrap();
        builder.emit_push_i32(2).unwrap();
        builder.emit_call(100, 2).unwrap();
        builder.emit_return().unwrap();

        let func = builder.build().unwrap();
        assert!(func.bytecode.len() > 0);
    }

    #[test]
    fn test_max_stack_depth() {
        let mut builder = BytecodeBuilder::new("stack".to_string(), 0, "number".to_string());

        // Push 5 values
        for i in 0..5 {
            builder.emit_push_i32(i).unwrap();
        }
        // Pop 4
        for _ in 0..4 {
            builder.emit_pop().unwrap();
        }
        builder.emit_return().unwrap();

        let func = builder.build().unwrap();
        assert_eq!(func.max_stack, 5);
    }

    #[test]
    fn test_string_constant() {
        let mut builder = BytecodeBuilder::new("str".to_string(), 0, "string".to_string());

        builder.emit_push_string("hello".to_string()).unwrap();
        builder.emit_return().unwrap();

        let func = builder.build().unwrap();
        assert_eq!(func.constants.len(), 1);
        match &func.constants[0] {
            ConstantValue::String(s) => assert_eq!(s, "hello"),
            _ => panic!("Expected string constant"),
        }
    }

    #[test]
    fn test_object_operations() {
        let mut builder = BytecodeBuilder::new("obj".to_string(), 0, "Object".to_string());

        builder.emit_new(10).unwrap(); // Create object of class 10
        builder.emit_dup().unwrap();
        builder.emit_push_i32(42).unwrap();
        builder.emit_store_field(0).unwrap(); // Store 42 to field 0
        builder.emit_load_field(0).unwrap(); // Load field 0
        builder.emit_return().unwrap();

        let func = builder.build().unwrap();
        assert!(func.bytecode.len() > 0);
    }
}
