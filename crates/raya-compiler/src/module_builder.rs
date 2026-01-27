//! Module builder for constructing bytecode modules

use crate::error::{CompileError, CompileResult};
use crate::bytecode::{ClassDef, Function, Module, Opcode};
use rustc_hash::FxHashMap;

/// Helper for building bytecode modules
pub struct ModuleBuilder {
    name: String,
    functions: Vec<Function>,
    classes: Vec<ClassDef>,
    constants: Vec<Vec<u8>>,
    constant_map: FxHashMap<Vec<u8>, u16>,
}

impl ModuleBuilder {
    pub fn new(name: String) -> Self {
        Self {
            name,
            functions: Vec::new(),
            classes: Vec::new(),
            constants: Vec::new(),
            constant_map: FxHashMap::default(),
        }
    }

    /// Add a function to the module
    pub fn add_function(&mut self, function: Function) {
        self.functions.push(function);
    }

    /// Add a class to the module
    pub fn add_class(&mut self, class: ClassDef) {
        self.classes.push(class);
    }

    /// Add a string constant to the constant pool, returning its index
    pub fn add_string(&mut self, s: String) -> CompileResult<u16> {
        let data = s.as_bytes().to_vec();
        if let Some(&index) = self.constant_map.get(&data) {
            return Ok(index);
        }

        if self.constants.len() >= 65535 {
            return Err(CompileError::TooManyConstants);
        }

        let index = self.constants.len() as u16;
        self.constant_map.insert(data, index);
        self.constants.push(s.into_bytes());
        Ok(index)
    }

    /// Build the final module
    pub fn build(self) -> Module {
        let mut module = Module::new(self.name);
        module.functions = self.functions;
        module.classes = self.classes;
        // Add string constants to constant pool
        for data in self.constants {
            if let Ok(s) = String::from_utf8(data) {
                module.constants.add_string(s);
            }
        }
        module
    }
}

/// Helper for building function bytecode
pub struct FunctionBuilder {
    name: String,
    param_count: u8,
    code: Vec<u8>,
    local_count: u16,
    locals: FxHashMap<String, u16>,
}

impl FunctionBuilder {
    pub fn new(name: String, param_count: u8) -> Self {
        let mut locals = FxHashMap::default();
        // Reserve slots for parameters
        for i in 0..param_count {
            locals.insert(format!("param_{}", i), i as u16);
        }

        Self {
            name,
            param_count,
            code: Vec::new(),
            local_count: param_count as u16,
            locals,
        }
    }

    /// Allocate a new local variable, returning its index
    pub fn add_local(&mut self, name: String) -> CompileResult<u16> {
        if let Some(&index) = self.locals.get(&name) {
            return Ok(index);
        }

        if self.local_count >= 65535 {
            return Err(CompileError::TooManyLocals);
        }

        let index = self.local_count;
        self.local_count += 1;
        self.locals.insert(name, index);
        Ok(index)
    }

    /// Get the index of a local variable
    pub fn get_local(&self, name: &str) -> Option<u16> {
        self.locals.get(name).copied()
    }

    /// Emit a single-byte opcode
    pub fn emit(&mut self, opcode: Opcode) {
        self.code.push(opcode as u8);
    }

    /// Emit a u8 operand
    pub fn emit_u8(&mut self, value: u8) {
        self.code.push(value);
    }

    /// Emit a u16 operand (little-endian)
    pub fn emit_u16(&mut self, value: u16) {
        self.code.extend_from_slice(&value.to_le_bytes());
    }

    /// Emit an i16 operand (little-endian)
    pub fn emit_i16(&mut self, value: i16) {
        self.code.extend_from_slice(&value.to_le_bytes());
    }

    /// Emit an i32 operand (little-endian)
    pub fn emit_i32(&mut self, value: i32) {
        self.code.extend_from_slice(&value.to_le_bytes());
    }

    /// Emit an f64 operand (little-endian)
    pub fn emit_f64(&mut self, value: f64) {
        self.code.extend_from_slice(&value.to_le_bytes());
    }

    /// Get current code position (for jump offsets)
    pub fn current_position(&self) -> usize {
        self.code.len()
    }

    /// Get mutable access to the code buffer
    pub fn code_mut(&mut self) -> &mut Vec<u8> {
        &mut self.code
    }

    /// Set the local count (for IR-based code generation)
    pub fn set_local_count(&mut self, count: u16) {
        self.local_count = count;
    }

    /// Patch a jump offset at a given position
    pub fn patch_jump(&mut self, position: usize, offset: i16) {
        self.code[position..position + 2].copy_from_slice(&offset.to_le_bytes());
    }

    /// Build the final function
    pub fn build(self) -> Function {
        Function {
            name: self.name,
            param_count: self.param_count as usize,
            local_count: self.local_count as usize,
            code: self.code,
        }
    }
}
