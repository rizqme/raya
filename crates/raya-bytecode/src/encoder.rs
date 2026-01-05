//! Bytecode encoding and decoding utilities
//!
//! This module provides tools for encoding and decoding Raya bytecode instructions.

use crate::opcode::Opcode;
use thiserror::Error;

/// Errors that can occur during bytecode decoding
#[derive(Debug, Error)]
pub enum DecodeError {
    /// Unexpected end of bytecode stream
    #[error("Unexpected end of bytecode at offset {0}")]
    UnexpectedEnd(usize),

    /// Invalid UTF-8 string
    #[error("Invalid UTF-8 string at offset {0}")]
    InvalidUtf8(usize),

    /// Invalid opcode
    #[error("Invalid opcode {0} at offset {1}")]
    InvalidOpcode(u8, usize),
}

/// Bytecode writer for encoding instructions
///
/// Provides methods for emitting opcodes and their operands into a binary buffer.
pub struct BytecodeWriter {
    /// Internal buffer containing the bytecode
    pub(crate) buffer: Vec<u8>,
}

impl BytecodeWriter {
    /// Create a new bytecode writer
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Create a new bytecode writer with capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
        }
    }

    /// Get the current bytecode buffer
    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    /// Consume the writer and return the bytecode buffer
    pub fn into_bytes(self) -> Vec<u8> {
        self.buffer
    }

    /// Get the current offset (length of bytecode)
    pub fn offset(&self) -> usize {
        self.buffer.len()
    }

    /// Clear the buffer
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    // ===== Basic Emission =====

    /// Emit a raw byte
    pub fn emit_u8(&mut self, value: u8) {
        self.buffer.push(value);
    }

    /// Emit a 16-bit unsigned integer (little-endian)
    pub fn emit_u16(&mut self, value: u16) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    /// Emit a 32-bit unsigned integer (little-endian)
    pub fn emit_u32(&mut self, value: u32) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    /// Emit a 32-bit signed integer (little-endian)
    pub fn emit_i32(&mut self, value: i32) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    /// Emit a 64-bit float (little-endian)
    pub fn emit_f64(&mut self, value: f64) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    // ===== Opcode Emission =====

    /// Emit an opcode without operands
    pub fn emit_opcode(&mut self, opcode: Opcode) {
        self.emit_u8(opcode.to_u8());
    }

    // ===== Stack Manipulation & Constants =====

    /// Emit NOP instruction
    pub fn emit_nop(&mut self) {
        self.emit_opcode(Opcode::Nop);
    }

    /// Emit POP instruction
    pub fn emit_pop(&mut self) {
        self.emit_opcode(Opcode::Pop);
    }

    /// Emit DUP instruction
    pub fn emit_dup(&mut self) {
        self.emit_opcode(Opcode::Dup);
    }

    /// Emit SWAP instruction
    pub fn emit_swap(&mut self) {
        self.emit_opcode(Opcode::Swap);
    }

    /// Emit CONST_NULL instruction
    pub fn emit_const_null(&mut self) {
        self.emit_opcode(Opcode::ConstNull);
    }

    /// Emit CONST_TRUE instruction
    pub fn emit_const_true(&mut self) {
        self.emit_opcode(Opcode::ConstTrue);
    }

    /// Emit CONST_FALSE instruction
    pub fn emit_const_false(&mut self) {
        self.emit_opcode(Opcode::ConstFalse);
    }

    /// Emit CONST_I32 instruction with value
    pub fn emit_const_i32(&mut self, value: i32) {
        self.emit_opcode(Opcode::ConstI32);
        self.emit_i32(value);
    }

    /// Emit CONST_F64 instruction with value
    pub fn emit_const_f64(&mut self, value: f64) {
        self.emit_opcode(Opcode::ConstF64);
        self.emit_f64(value);
    }

    /// Emit CONST_STR instruction with constant pool index
    pub fn emit_const_str(&mut self, index: u32) {
        self.emit_opcode(Opcode::ConstStr);
        self.emit_u32(index);
    }

    /// Emit LOAD_CONST instruction with constant pool index
    pub fn emit_load_const(&mut self, index: u32) {
        self.emit_opcode(Opcode::LoadConst);
        self.emit_u32(index);
    }

    // ===== Local Variables =====

    /// Emit LOAD_LOCAL instruction
    pub fn emit_load_local(&mut self, index: u16) {
        self.emit_opcode(Opcode::LoadLocal);
        self.emit_u16(index);
    }

    /// Emit STORE_LOCAL instruction
    pub fn emit_store_local(&mut self, index: u16) {
        self.emit_opcode(Opcode::StoreLocal);
        self.emit_u16(index);
    }

    /// Emit LOAD_LOCAL_0 instruction (optimized)
    pub fn emit_load_local_0(&mut self) {
        self.emit_opcode(Opcode::LoadLocal0);
    }

    /// Emit LOAD_LOCAL_1 instruction (optimized)
    pub fn emit_load_local_1(&mut self) {
        self.emit_opcode(Opcode::LoadLocal1);
    }

    /// Emit STORE_LOCAL_0 instruction (optimized)
    pub fn emit_store_local_0(&mut self) {
        self.emit_opcode(Opcode::StoreLocal0);
    }

    /// Emit STORE_LOCAL_1 instruction (optimized)
    pub fn emit_store_local_1(&mut self) {
        self.emit_opcode(Opcode::StoreLocal1);
    }

    // ===== Integer Arithmetic =====

    /// Emit IADD instruction
    pub fn emit_iadd(&mut self) {
        self.emit_opcode(Opcode::Iadd);
    }

    /// Emit ISUB instruction
    pub fn emit_isub(&mut self) {
        self.emit_opcode(Opcode::Isub);
    }

    /// Emit IMUL instruction
    pub fn emit_imul(&mut self) {
        self.emit_opcode(Opcode::Imul);
    }

    /// Emit IDIV instruction
    pub fn emit_idiv(&mut self) {
        self.emit_opcode(Opcode::Idiv);
    }

    /// Emit IMOD instruction
    pub fn emit_imod(&mut self) {
        self.emit_opcode(Opcode::Imod);
    }

    /// Emit INEG instruction
    pub fn emit_ineg(&mut self) {
        self.emit_opcode(Opcode::Ineg);
    }

    // ===== Float Arithmetic =====

    /// Emit FADD instruction
    pub fn emit_fadd(&mut self) {
        self.emit_opcode(Opcode::Fadd);
    }

    /// Emit FSUB instruction
    pub fn emit_fsub(&mut self) {
        self.emit_opcode(Opcode::Fsub);
    }

    /// Emit FMUL instruction
    pub fn emit_fmul(&mut self) {
        self.emit_opcode(Opcode::Fmul);
    }

    /// Emit FDIV instruction
    pub fn emit_fdiv(&mut self) {
        self.emit_opcode(Opcode::Fdiv);
    }

    /// Emit FNEG instruction
    pub fn emit_fneg(&mut self) {
        self.emit_opcode(Opcode::Fneg);
    }

    // ===== Number Arithmetic =====

    /// Emit NADD instruction
    pub fn emit_nadd(&mut self) {
        self.emit_opcode(Opcode::Nadd);
    }

    /// Emit NSUB instruction
    pub fn emit_nsub(&mut self) {
        self.emit_opcode(Opcode::Nsub);
    }

    /// Emit NMUL instruction
    pub fn emit_nmul(&mut self) {
        self.emit_opcode(Opcode::Nmul);
    }

    /// Emit NDIV instruction
    pub fn emit_ndiv(&mut self) {
        self.emit_opcode(Opcode::Ndiv);
    }

    /// Emit NMOD instruction
    pub fn emit_nmod(&mut self) {
        self.emit_opcode(Opcode::Nmod);
    }

    /// Emit NNEG instruction
    pub fn emit_nneg(&mut self) {
        self.emit_opcode(Opcode::Nneg);
    }

    // ===== Control Flow =====

    /// Emit JMP instruction
    pub fn emit_jmp(&mut self, offset: i32) {
        self.emit_opcode(Opcode::Jmp);
        self.emit_i32(offset);
    }

    /// Emit JMP_IF_FALSE instruction
    pub fn emit_jmp_if_false(&mut self, offset: i32) {
        self.emit_opcode(Opcode::JmpIfFalse);
        self.emit_i32(offset);
    }

    /// Emit JMP_IF_TRUE instruction
    pub fn emit_jmp_if_true(&mut self, offset: i32) {
        self.emit_opcode(Opcode::JmpIfTrue);
        self.emit_i32(offset);
    }

    /// Emit JMP_IF_NULL instruction
    pub fn emit_jmp_if_null(&mut self, offset: i32) {
        self.emit_opcode(Opcode::JmpIfNull);
        self.emit_i32(offset);
    }

    /// Emit JMP_IF_NOT_NULL instruction
    pub fn emit_jmp_if_not_null(&mut self, offset: i32) {
        self.emit_opcode(Opcode::JmpIfNotNull);
        self.emit_i32(offset);
    }

    // ===== Function Calls =====

    /// Emit CALL instruction
    pub fn emit_call(&mut self, func_index: u32, arg_count: u16) {
        self.emit_opcode(Opcode::Call);
        self.emit_u32(func_index);
        self.emit_u16(arg_count);
    }

    /// Emit CALL_METHOD instruction
    pub fn emit_call_method(&mut self, method_index: u32, arg_count: u16) {
        self.emit_opcode(Opcode::CallMethod);
        self.emit_u32(method_index);
        self.emit_u16(arg_count);
    }

    /// Emit RETURN instruction
    pub fn emit_return(&mut self) {
        self.emit_opcode(Opcode::Return);
    }

    /// Emit RETURN_VOID instruction
    pub fn emit_return_void(&mut self) {
        self.emit_opcode(Opcode::ReturnVoid);
    }

    /// Emit CALL_CONSTRUCTOR instruction
    pub fn emit_call_constructor(&mut self, ctor_index: u32, arg_count: u16) {
        self.emit_opcode(Opcode::CallConstructor);
        self.emit_u32(ctor_index);
        self.emit_u16(arg_count);
    }

    /// Emit CALL_SUPER instruction
    pub fn emit_call_super(&mut self, super_ctor_index: u32, arg_count: u16) {
        self.emit_opcode(Opcode::CallSuper);
        self.emit_u32(super_ctor_index);
        self.emit_u16(arg_count);
    }

    /// Emit CALL_STATIC instruction
    pub fn emit_call_static(&mut self, method_index: u32, arg_count: u16) {
        self.emit_opcode(Opcode::CallStatic);
        self.emit_u32(method_index);
        self.emit_u16(arg_count);
    }

    // ===== Object Operations =====

    /// Emit NEW instruction
    pub fn emit_new(&mut self, class_index: u32) {
        self.emit_opcode(Opcode::New);
        self.emit_u32(class_index);
    }

    /// Emit LOAD_FIELD instruction
    pub fn emit_load_field(&mut self, field_offset: u16) {
        self.emit_opcode(Opcode::LoadField);
        self.emit_u16(field_offset);
    }

    /// Emit STORE_FIELD instruction
    pub fn emit_store_field(&mut self, field_offset: u16) {
        self.emit_opcode(Opcode::StoreField);
        self.emit_u16(field_offset);
    }

    /// Emit LOAD_FIELD_FAST instruction
    pub fn emit_load_field_fast(&mut self, offset: u16) {
        self.emit_opcode(Opcode::LoadFieldFast);
        self.emit_u16(offset);
    }

    /// Emit STORE_FIELD_FAST instruction
    pub fn emit_store_field_fast(&mut self, offset: u16) {
        self.emit_opcode(Opcode::StoreFieldFast);
        self.emit_u16(offset);
    }

    /// Emit OBJECT_LITERAL instruction
    pub fn emit_object_literal(&mut self, type_index: u32, field_count: u16) {
        self.emit_opcode(Opcode::ObjectLiteral);
        self.emit_u32(type_index);
        self.emit_u16(field_count);
    }

    /// Emit INIT_OBJECT instruction
    pub fn emit_init_object(&mut self, count: u16) {
        self.emit_opcode(Opcode::InitObject);
        self.emit_u16(count);
    }

    /// Emit OPTIONAL_FIELD instruction
    pub fn emit_optional_field(&mut self, offset: u16) {
        self.emit_opcode(Opcode::OptionalField);
        self.emit_u16(offset);
    }

    // ===== Array Operations =====

    /// Emit NEW_ARRAY instruction
    pub fn emit_new_array(&mut self, type_index: u32) {
        self.emit_opcode(Opcode::NewArray);
        self.emit_u32(type_index);
    }

    /// Emit LOAD_ELEM instruction
    pub fn emit_load_elem(&mut self) {
        self.emit_opcode(Opcode::LoadElem);
    }

    /// Emit STORE_ELEM instruction
    pub fn emit_store_elem(&mut self) {
        self.emit_opcode(Opcode::StoreElem);
    }

    /// Emit ARRAY_LEN instruction
    pub fn emit_array_len(&mut self) {
        self.emit_opcode(Opcode::ArrayLen);
    }

    /// Emit ARRAY_LITERAL instruction
    pub fn emit_array_literal(&mut self, type_index: u32, length: u32) {
        self.emit_opcode(Opcode::ArrayLiteral);
        self.emit_u32(type_index);
        self.emit_u32(length);
    }

    /// Emit INIT_ARRAY instruction
    pub fn emit_init_array(&mut self, count: u16) {
        self.emit_opcode(Opcode::InitArray);
        self.emit_u16(count);
    }

    // ===== Task & Concurrency =====

    /// Emit SPAWN instruction
    pub fn emit_spawn(&mut self, func_index: u32, arg_count: u16) {
        self.emit_opcode(Opcode::Spawn);
        self.emit_u32(func_index);
        self.emit_u16(arg_count);
    }

    /// Emit AWAIT instruction
    pub fn emit_await(&mut self) {
        self.emit_opcode(Opcode::Await);
    }

    /// Emit YIELD instruction
    pub fn emit_yield(&mut self) {
        self.emit_opcode(Opcode::Yield);
    }

    /// Emit TASK_THEN instruction
    pub fn emit_task_then(&mut self, func_index: u32) {
        self.emit_opcode(Opcode::TaskThen);
        self.emit_u32(func_index);
    }

    // ===== Synchronization & Error Handling =====

    /// Emit NEW_MUTEX instruction
    pub fn emit_new_mutex(&mut self) {
        self.emit_opcode(Opcode::NewMutex);
    }

    /// Emit MUTEX_LOCK instruction
    pub fn emit_mutex_lock(&mut self) {
        self.emit_opcode(Opcode::MutexLock);
    }

    /// Emit MUTEX_UNLOCK instruction
    pub fn emit_mutex_unlock(&mut self) {
        self.emit_opcode(Opcode::MutexUnlock);
    }

    /// Emit THROW instruction
    pub fn emit_throw(&mut self) {
        self.emit_opcode(Opcode::Throw);
    }

    /// Emit TRAP instruction
    pub fn emit_trap(&mut self, error_code: u16) {
        self.emit_opcode(Opcode::Trap);
        self.emit_u16(error_code);
    }

    // ===== Global Variables =====

    /// Emit LOAD_GLOBAL instruction
    pub fn emit_load_global(&mut self, index: u32) {
        self.emit_opcode(Opcode::LoadGlobal);
        self.emit_u32(index);
    }

    /// Emit STORE_GLOBAL instruction
    pub fn emit_store_global(&mut self, index: u32) {
        self.emit_opcode(Opcode::StoreGlobal);
        self.emit_u32(index);
    }

    // ===== Closures =====

    /// Emit MAKE_CLOSURE instruction
    pub fn emit_make_closure(&mut self, func_index: u32, capture_count: u16) {
        self.emit_opcode(Opcode::MakeClosure);
        self.emit_u32(func_index);
        self.emit_u16(capture_count);
    }

    /// Emit CLOSE_VAR instruction
    pub fn emit_close_var(&mut self, local_index: u16) {
        self.emit_opcode(Opcode::CloseVar);
        self.emit_u16(local_index);
    }

    /// Emit LOAD_CAPTURED instruction
    pub fn emit_load_captured(&mut self, index: u16) {
        self.emit_opcode(Opcode::LoadCaptured);
        self.emit_u16(index);
    }

    /// Emit STORE_CAPTURED instruction
    pub fn emit_store_captured(&mut self, index: u16) {
        self.emit_opcode(Opcode::StoreCaptured);
        self.emit_u16(index);
    }

    // ===== Module Operations =====

    /// Emit LOAD_MODULE instruction
    pub fn emit_load_module(&mut self, module_index: u32) {
        self.emit_opcode(Opcode::LoadModule);
        self.emit_u32(module_index);
    }

    // ===== Patching (for forward jumps) =====

    /// Patch a previously emitted i32 value at the given offset
    pub fn patch_i32(&mut self, offset: usize, value: i32) {
        let bytes = value.to_le_bytes();
        self.buffer[offset..offset + 4].copy_from_slice(&bytes);
    }

    /// Patch a previously emitted u32 value at the given offset
    pub fn patch_u32(&mut self, offset: usize, value: u32) {
        let bytes = value.to_le_bytes();
        self.buffer[offset..offset + 4].copy_from_slice(&bytes);
    }

    /// Reserve space for an i32 value (returns offset for later patching)
    pub fn reserve_i32(&mut self) -> usize {
        let offset = self.offset();
        self.emit_i32(0);
        offset
    }

    /// Reserve space for a u32 value (returns offset for later patching)
    pub fn reserve_u32(&mut self) -> usize {
        let offset = self.offset();
        self.emit_u32(0);
        offset
    }
}

impl Default for BytecodeWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Bytecode reader for decoding instructions
///
/// Provides methods for reading opcodes and their operands from a binary buffer.
pub struct BytecodeReader<'a> {
    buffer: &'a [u8],
    position: usize,
}

impl<'a> BytecodeReader<'a> {
    /// Create a new bytecode reader
    pub fn new(buffer: &'a [u8]) -> Self {
        Self {
            buffer,
            position: 0,
        }
    }

    /// Get the current position in the buffer
    pub fn position(&self) -> usize {
        self.position
    }

    /// Get the remaining bytes in the buffer
    pub fn remaining(&self) -> usize {
        self.buffer.len().saturating_sub(self.position)
    }

    /// Check if there are more bytes to read
    pub fn has_more(&self) -> bool {
        self.position < self.buffer.len()
    }

    /// Seek to a specific position
    pub fn seek(&mut self, position: usize) {
        self.position = position;
    }

    // ===== Basic Reading =====

    /// Read a single byte
    pub fn read_u8(&mut self) -> Result<u8, DecodeError> {
        if self.position >= self.buffer.len() {
            return Err(DecodeError::UnexpectedEnd(self.position));
        }
        let value = self.buffer[self.position];
        self.position += 1;
        Ok(value)
    }

    /// Read a 16-bit unsigned integer (little-endian)
    pub fn read_u16(&mut self) -> Result<u16, DecodeError> {
        if self.position + 2 > self.buffer.len() {
            return Err(DecodeError::UnexpectedEnd(self.position));
        }
        let bytes = [self.buffer[self.position], self.buffer[self.position + 1]];
        self.position += 2;
        Ok(u16::from_le_bytes(bytes))
    }

    /// Read a 32-bit unsigned integer (little-endian)
    pub fn read_u32(&mut self) -> Result<u32, DecodeError> {
        if self.position + 4 > self.buffer.len() {
            return Err(DecodeError::UnexpectedEnd(self.position));
        }
        let bytes = [
            self.buffer[self.position],
            self.buffer[self.position + 1],
            self.buffer[self.position + 2],
            self.buffer[self.position + 3],
        ];
        self.position += 4;
        Ok(u32::from_le_bytes(bytes))
    }

    /// Read a 64-bit unsigned integer (little-endian)
    pub fn read_u64(&mut self) -> Result<u64, DecodeError> {
        if self.position + 8 > self.buffer.len() {
            return Err(DecodeError::UnexpectedEnd(self.position));
        }
        let bytes = [
            self.buffer[self.position],
            self.buffer[self.position + 1],
            self.buffer[self.position + 2],
            self.buffer[self.position + 3],
            self.buffer[self.position + 4],
            self.buffer[self.position + 5],
            self.buffer[self.position + 6],
            self.buffer[self.position + 7],
        ];
        self.position += 8;
        Ok(u64::from_le_bytes(bytes))
    }

    /// Read a 32-bit signed integer (little-endian)
    pub fn read_i32(&mut self) -> Result<i32, DecodeError> {
        if self.position + 4 > self.buffer.len() {
            return Err(DecodeError::UnexpectedEnd(self.position));
        }
        let bytes = [
            self.buffer[self.position],
            self.buffer[self.position + 1],
            self.buffer[self.position + 2],
            self.buffer[self.position + 3],
        ];
        self.position += 4;
        Ok(i32::from_le_bytes(bytes))
    }

    /// Read a 64-bit signed integer (little-endian)
    pub fn read_i64(&mut self) -> Result<i64, DecodeError> {
        if self.position + 8 > self.buffer.len() {
            return Err(DecodeError::UnexpectedEnd(self.position));
        }
        let bytes = [
            self.buffer[self.position],
            self.buffer[self.position + 1],
            self.buffer[self.position + 2],
            self.buffer[self.position + 3],
            self.buffer[self.position + 4],
            self.buffer[self.position + 5],
            self.buffer[self.position + 6],
            self.buffer[self.position + 7],
        ];
        self.position += 8;
        Ok(i64::from_le_bytes(bytes))
    }

    /// Read a 32-bit float (little-endian)
    pub fn read_f32(&mut self) -> Result<f32, DecodeError> {
        if self.position + 4 > self.buffer.len() {
            return Err(DecodeError::UnexpectedEnd(self.position));
        }
        let bytes = [
            self.buffer[self.position],
            self.buffer[self.position + 1],
            self.buffer[self.position + 2],
            self.buffer[self.position + 3],
        ];
        self.position += 4;
        Ok(f32::from_le_bytes(bytes))
    }

    /// Read a 64-bit float (little-endian)
    pub fn read_f64(&mut self) -> Result<f64, DecodeError> {
        if self.position + 8 > self.buffer.len() {
            return Err(DecodeError::UnexpectedEnd(self.position));
        }
        let bytes = [
            self.buffer[self.position],
            self.buffer[self.position + 1],
            self.buffer[self.position + 2],
            self.buffer[self.position + 3],
            self.buffer[self.position + 4],
            self.buffer[self.position + 5],
            self.buffer[self.position + 6],
            self.buffer[self.position + 7],
        ];
        self.position += 8;
        Ok(f64::from_le_bytes(bytes))
    }

    /// Read a length-prefixed string (u32 length + UTF-8 bytes)
    pub fn read_string(&mut self) -> Result<String, DecodeError> {
        let len = self.read_u32()? as usize;
        if self.position + len > self.buffer.len() {
            return Err(DecodeError::UnexpectedEnd(self.position));
        }
        let bytes = &self.buffer[self.position..self.position + len];
        self.position += len;
        String::from_utf8(bytes.to_vec()).map_err(|_| DecodeError::InvalidUtf8(self.position - len))
    }

    /// Read a fixed number of bytes
    pub fn read_bytes(&mut self, count: usize) -> Result<Vec<u8>, DecodeError> {
        if self.position + count > self.buffer.len() {
            return Err(DecodeError::UnexpectedEnd(self.position));
        }
        let bytes = self.buffer[self.position..self.position + count].to_vec();
        self.position += count;
        Ok(bytes)
    }

    /// Read an opcode
    pub fn read_opcode(&mut self) -> Result<Opcode, DecodeError> {
        let byte = self.read_u8()?;
        Opcode::from_u8(byte).ok_or(DecodeError::InvalidOpcode(byte, self.position - 1))
    }
}

#[cfg(test)]
#[allow(clippy::approx_constant)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_emission() {
        let mut writer = BytecodeWriter::new();
        writer.emit_u8(0x42);
        writer.emit_u16(0x1234);
        writer.emit_u32(0xABCD_EF01);

        let bytes = writer.buffer();
        assert_eq!(bytes[0], 0x42);
        assert_eq!(bytes[1], 0x34); // Little-endian
        assert_eq!(bytes[2], 0x12);
        assert_eq!(bytes[3], 0x01); // Little-endian
        assert_eq!(bytes[4], 0xEF);
        assert_eq!(bytes[5], 0xCD);
        assert_eq!(bytes[6], 0xAB);
    }

    #[test]
    fn test_opcode_emission() {
        let mut writer = BytecodeWriter::new();
        writer.emit_nop();
        writer.emit_iadd();
        writer.emit_return();

        let bytes = writer.buffer();
        assert_eq!(bytes[0], Opcode::Nop.to_u8());
        assert_eq!(bytes[1], Opcode::Iadd.to_u8());
        assert_eq!(bytes[2], Opcode::Return.to_u8());
    }

    #[test]
    fn test_const_emission() {
        let mut writer = BytecodeWriter::new();
        writer.emit_const_i32(42);
        writer.emit_const_f64(3.14);

        let bytes = writer.buffer();
        assert_eq!(bytes[0], Opcode::ConstI32.to_u8());
        // Verify i32 value
        let value = i32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
        assert_eq!(value, 42);

        assert_eq!(bytes[5], Opcode::ConstF64.to_u8());
        // Verify f64 value
        let value = f64::from_le_bytes([
            bytes[6], bytes[7], bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13],
        ]);
        assert!((value - 3.14).abs() < 0.001);
    }

    #[test]
    fn test_call_emission() {
        let mut writer = BytecodeWriter::new();
        writer.emit_call(123, 4);

        let bytes = writer.buffer();
        assert_eq!(bytes[0], Opcode::Call.to_u8());
        let func_index = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
        assert_eq!(func_index, 123);
        let arg_count = u16::from_le_bytes([bytes[5], bytes[6]]);
        assert_eq!(arg_count, 4);
    }

    #[test]
    fn test_jump_patching() {
        let mut writer = BytecodeWriter::new();
        writer.emit_opcode(Opcode::JmpIfFalse);
        let patch_offset = writer.reserve_i32();
        writer.emit_const_i32(42);

        // Calculate jump offset
        let jump_target = writer.offset();
        let jump_offset = jump_target as i32 - (patch_offset as i32 + 4);
        writer.patch_i32(patch_offset, jump_offset);

        let bytes = writer.buffer();
        assert_eq!(bytes[0], Opcode::JmpIfFalse.to_u8());
        // Verify patched offset
        let patched = i32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
        assert_eq!(patched, jump_offset);
    }

    #[test]
    fn test_offset_tracking() {
        let mut writer = BytecodeWriter::new();
        assert_eq!(writer.offset(), 0);

        writer.emit_nop();
        assert_eq!(writer.offset(), 1);

        writer.emit_const_i32(42);
        assert_eq!(writer.offset(), 6); // 1 opcode + 4 bytes for i32

        writer.emit_call(0, 0);
        assert_eq!(writer.offset(), 13); // +1 opcode +4 u32 +2 u16
    }

    #[test]
    fn test_clear() {
        let mut writer = BytecodeWriter::new();
        writer.emit_nop();
        writer.emit_iadd();
        assert_eq!(writer.offset(), 2);

        writer.clear();
        assert_eq!(writer.offset(), 0);
        assert!(writer.buffer().is_empty());
    }

    #[test]
    fn test_into_bytes() {
        let mut writer = BytecodeWriter::new();
        writer.emit_nop();
        writer.emit_iadd();

        let bytes = writer.into_bytes();
        assert_eq!(bytes.len(), 2);
        assert_eq!(bytes[0], Opcode::Nop.to_u8());
        assert_eq!(bytes[1], Opcode::Iadd.to_u8());
    }

    // ===== BytecodeReader Tests =====

    #[test]
    fn test_reader_primitives() {
        let mut writer = BytecodeWriter::new();
        writer.emit_u8(0x42);
        writer.emit_u16(0x1234);
        writer.emit_u32(0xABCD_EF01);
        writer.emit_i32(-42);
        writer.emit_f64(3.14159);

        let bytes = writer.buffer();
        let mut reader = BytecodeReader::new(bytes);

        assert_eq!(reader.read_u8().unwrap(), 0x42);
        assert_eq!(reader.read_u16().unwrap(), 0x1234);
        assert_eq!(reader.read_u32().unwrap(), 0xABCD_EF01);
        assert_eq!(reader.read_i32().unwrap(), -42);
        assert!((reader.read_f64().unwrap() - 3.14159).abs() < 0.00001);
    }

    #[test]
    fn test_reader_bounds_checking() {
        let bytes = vec![0x01, 0x02];
        let mut reader = BytecodeReader::new(&bytes);

        assert_eq!(reader.read_u8().unwrap(), 0x01);
        assert_eq!(reader.read_u8().unwrap(), 0x02);
        assert!(reader.read_u8().is_err()); // Should fail - out of bounds
    }

    #[test]
    fn test_reader_string() {
        let mut writer = BytecodeWriter::new();
        // Manually write a string: length (u32) + UTF-8 bytes
        writer.emit_u32(5);
        writer.buffer.extend_from_slice(b"hello");

        let bytes = writer.buffer();
        let mut reader = BytecodeReader::new(bytes);

        assert_eq!(reader.read_string().unwrap(), "hello");
    }

    #[test]
    fn test_reader_position_tracking() {
        let mut writer = BytecodeWriter::new();
        writer.emit_u8(0x01);
        writer.emit_u16(0x0203);
        writer.emit_u32(0x04050607);

        let bytes = writer.buffer();
        let mut reader = BytecodeReader::new(bytes);

        assert_eq!(reader.position(), 0);
        reader.read_u8().unwrap();
        assert_eq!(reader.position(), 1);
        reader.read_u16().unwrap();
        assert_eq!(reader.position(), 3);
        reader.read_u32().unwrap();
        assert_eq!(reader.position(), 7);
    }

    #[test]
    fn test_reader_remaining() {
        let bytes = vec![0x01, 0x02, 0x03, 0x04];
        let mut reader = BytecodeReader::new(&bytes);

        assert_eq!(reader.remaining(), 4);
        reader.read_u8().unwrap();
        assert_eq!(reader.remaining(), 3);
        reader.read_u8().unwrap();
        assert_eq!(reader.remaining(), 2);
    }

    #[test]
    fn test_reader_seek() {
        let bytes = vec![0x01, 0x02, 0x03, 0x04];
        let mut reader = BytecodeReader::new(&bytes);

        reader.read_u8().unwrap(); // position = 1
        reader.seek(0); // back to start
        assert_eq!(reader.read_u8().unwrap(), 0x01);
        reader.seek(3); // jump to position 3
        assert_eq!(reader.read_u8().unwrap(), 0x04);
    }

    #[test]
    fn test_reader_opcode() {
        let mut writer = BytecodeWriter::new();
        writer.emit_nop();
        writer.emit_iadd();
        writer.emit_return();

        let bytes = writer.buffer();
        let mut reader = BytecodeReader::new(bytes);

        assert_eq!(reader.read_opcode().unwrap(), Opcode::Nop);
        assert_eq!(reader.read_opcode().unwrap(), Opcode::Iadd);
        assert_eq!(reader.read_opcode().unwrap(), Opcode::Return);
    }

    #[test]
    fn test_reader_invalid_opcode() {
        let bytes = vec![0xFF]; // Invalid opcode
        let mut reader = BytecodeReader::new(&bytes);

        assert!(reader.read_opcode().is_err());
    }

    #[test]
    fn test_roundtrip_encoding() {
        // Write various values
        let mut writer = BytecodeWriter::new();
        writer.emit_const_i32(42);
        writer.emit_const_f64(3.14);
        writer.emit_load_local(5);
        writer.emit_iadd();
        writer.emit_return();

        // Read them back
        let bytes = writer.buffer();
        let mut reader = BytecodeReader::new(bytes);

        assert_eq!(reader.read_opcode().unwrap(), Opcode::ConstI32);
        assert_eq!(reader.read_i32().unwrap(), 42);
        assert_eq!(reader.read_opcode().unwrap(), Opcode::ConstF64);
        assert!((reader.read_f64().unwrap() - 3.14).abs() < 0.001);
        assert_eq!(reader.read_opcode().unwrap(), Opcode::LoadLocal);
        assert_eq!(reader.read_u16().unwrap(), 5);
        assert_eq!(reader.read_opcode().unwrap(), Opcode::Iadd);
        assert_eq!(reader.read_opcode().unwrap(), Opcode::Return);
    }
}
