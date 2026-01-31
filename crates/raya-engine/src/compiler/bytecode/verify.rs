//! Bytecode verification

use super::encoder::BytecodeReader;
use super::module::{Function, Module};
use super::opcode::Opcode;
use std::collections::HashSet;

/// Bytecode verification errors
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    /// Invalid opcode
    #[error("Invalid opcode {opcode:#x} at offset {offset}")]
    InvalidOpcode {
        /// The invalid opcode byte
        opcode: u8,
        /// Offset in bytecode
        offset: usize,
    },

    /// Stack underflow
    #[error("Stack underflow at offset {0}")]
    StackUnderflow(usize),

    /// Stack overflow
    #[error("Stack overflow at offset {0} (depth: {1})")]
    StackOverflow(usize, i32),

    /// Invalid jump target
    #[error("Invalid jump target {target} at offset {offset}")]
    InvalidJumpTarget {
        /// The invalid jump target
        target: usize,
        /// Offset in bytecode
        offset: usize,
    },

    /// Invalid constant pool reference
    #[error("Invalid constant pool reference: index {index} at offset {offset}")]
    InvalidConstantRef {
        /// The invalid constant index
        index: u32,
        /// Offset in bytecode
        offset: usize,
    },

    /// Invalid local variable reference
    #[error("Invalid local variable reference: index {index} (max {max}) at offset {offset}")]
    InvalidLocalRef {
        /// The invalid local variable index
        index: usize,
        /// Maximum allowed index
        max: usize,
        /// Offset in bytecode
        offset: usize,
    },

    /// Execution falls off end
    #[error("Execution falls off end of function at offset {0}")]
    FallOffEnd(usize),

    /// Module validation error
    #[error("Module validation error: {0}")]
    ModuleValidation(String),

    /// Decode error
    #[error("Decode error: {0}")]
    DecodeError(String),
}

/// Verify a module's bytecode
pub fn verify_module(module: &Module) -> Result<(), VerifyError> {
    // Validate module structure
    module.validate().map_err(VerifyError::ModuleValidation)?;

    // Verify each function
    for function in &module.functions {
        verify_function(function, module)?;
    }

    Ok(())
}

/// Verify a single function's bytecode
fn verify_function(function: &Function, module: &Module) -> Result<(), VerifyError> {
    // Empty functions are allowed
    if function.code.is_empty() {
        return Ok(());
    }

    // Parse all instructions and collect jump targets
    let instructions = parse_instructions(&function.code)?;
    let jump_targets = collect_jump_targets(&instructions)?;

    // Verify all jump targets are valid instruction boundaries
    for &target in &jump_targets {
        if !is_valid_instruction_boundary(target, &instructions) {
            return Err(VerifyError::InvalidJumpTarget {
                target,
                offset: target,
            });
        }
    }

    // Verify stack depth consistency
    verify_stack_depth(&instructions, &jump_targets)?;

    // Verify constant pool references
    verify_constant_refs(&instructions, module)?;

    // Verify local variable references
    verify_local_refs(&instructions, function)?;

    // Ensure function ends with a terminator
    if let Some(last_instr) = instructions.last() {
        if !last_instr.opcode.is_terminator() {
            return Err(VerifyError::FallOffEnd(last_instr.offset));
        }
    }

    Ok(())
}

/// Parsed instruction
#[derive(Debug, Clone)]
struct Instruction {
    offset: usize,
    opcode: Opcode,
    operands: Vec<u8>,
}

/// Parse all instructions from bytecode
fn parse_instructions(code: &[u8]) -> Result<Vec<Instruction>, VerifyError> {
    let mut instructions = Vec::new();
    let mut reader = BytecodeReader::new(code);

    while reader.has_more() {
        let offset = reader.position();
        let byte = reader
            .read_u8()
            .map_err(|e| VerifyError::DecodeError(e.to_string()))?;

        let opcode = Opcode::from_u8(byte).ok_or(VerifyError::InvalidOpcode {
            opcode: byte,
            offset,
        })?;

        // Read operands based on opcode
        let operand_size = get_operand_size(opcode);
        let operands = if operand_size > 0 {
            reader
                .read_bytes(operand_size)
                .map_err(|e| VerifyError::DecodeError(e.to_string()))?
        } else {
            Vec::new()
        };

        instructions.push(Instruction {
            offset,
            opcode,
            operands,
        });
    }

    Ok(instructions)
}

/// Get the operand size for an opcode (in bytes)
fn get_operand_size(opcode: Opcode) -> usize {
    match opcode {
        // No operands
        Opcode::Nop
        | Opcode::Pop
        | Opcode::Dup
        | Opcode::Swap
        | Opcode::ConstNull
        | Opcode::ConstTrue
        | Opcode::ConstFalse
        | Opcode::LoadLocal0
        | Opcode::LoadLocal1
        | Opcode::StoreLocal0
        | Opcode::StoreLocal1
        | Opcode::Iadd
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
        | Opcode::Fmod
        | Opcode::Nadd
        | Opcode::Nsub
        | Opcode::Nmul
        | Opcode::Ndiv
        | Opcode::Nmod
        | Opcode::Nneg
        | Opcode::Npow
        | Opcode::Ieq
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
        | Opcode::Eq
        | Opcode::Ne
        | Opcode::StrictEq
        | Opcode::StrictNe
        | Opcode::Not
        | Opcode::And
        | Opcode::Or
        | Opcode::Typeof
        | Opcode::Sconcat
        | Opcode::Slen
        | Opcode::Seq
        | Opcode::Sne
        | Opcode::Slt
        | Opcode::Sle
        | Opcode::Sgt
        | Opcode::Sge
        | Opcode::ToString
        | Opcode::Return
        | Opcode::ReturnVoid
        | Opcode::LoadElem
        | Opcode::StoreElem
        | Opcode::ArrayLen
        | Opcode::Await
        | Opcode::Yield
        | Opcode::Sleep
        | Opcode::NewMutex
        | Opcode::NewChannel
        | Opcode::MutexLock
        | Opcode::MutexUnlock
        | Opcode::Throw
        | Opcode::JsonIndex
        | Opcode::JsonIndexSet
        | Opcode::JsonPush
        | Opcode::JsonPop
        | Opcode::JsonNewObject
        | Opcode::JsonNewArray
        | Opcode::JsonKeys
        | Opcode::JsonLength
        | Opcode::NewSemaphore
        | Opcode::SemAcquire
        | Opcode::SemRelease
        | Opcode::WaitAll
        | Opcode::TaskCancel
        | Opcode::NewRefCell
        | Opcode::LoadRefCell
        | Opcode::StoreRefCell
        | Opcode::ArrayPush
        | Opcode::ArrayPop
        | Opcode::InstanceOf
        | Opcode::Cast => 0,

        // 2-byte operands (u16)
        Opcode::LoadLocal
        | Opcode::StoreLocal
        | Opcode::LoadField
        | Opcode::StoreField
        | Opcode::LoadFieldFast
        | Opcode::StoreFieldFast
        | Opcode::OptionalField
        | Opcode::InitObject
        | Opcode::InitArray
        | Opcode::InitTuple
        | Opcode::CloseVar
        | Opcode::LoadCaptured
        | Opcode::StoreCaptured
        | Opcode::SetClosureCapture
        | Opcode::Trap => 2,

        // 4-byte operands (i32 or u32)
        Opcode::ConstI32 => 4,
        Opcode::Jmp
        | Opcode::JmpIfFalse
        | Opcode::JmpIfTrue
        | Opcode::JmpIfNull
        | Opcode::JmpIfNotNull => 4,
        Opcode::ConstStr
        | Opcode::LoadConst
        | Opcode::LoadGlobal
        | Opcode::StoreGlobal
        | Opcode::New
        | Opcode::NewArray
        | Opcode::LoadModule
        | Opcode::TaskThen
        | Opcode::JsonGet
        | Opcode::JsonSet
        | Opcode::JsonDelete => 4,

        // 8-byte operands (f64)
        Opcode::ConstF64 => 8,

        // 6-byte operands (u32 + u16)
        Opcode::Call
        | Opcode::CallMethod
        | Opcode::CallConstructor
        | Opcode::CallSuper
        | Opcode::CallStatic
        | Opcode::ObjectLiteral
        | Opcode::Spawn
        | Opcode::MakeClosure => 6,

        // 8-byte operands (u32 + u32)
        Opcode::ArrayLiteral => 8,

        // 6-byte operands (u32 + u16)
        Opcode::TupleLiteral => 6,

        // Special cases
        Opcode::LoadStatic | Opcode::StoreStatic => 4,
        Opcode::TupleGet => 0,

        // Exception handling (8 bytes: i32 catch_offset + i32 finally_offset)
        Opcode::Try => 8,
        Opcode::EndTry | Opcode::Rethrow => 0,

        // SpawnClosure has 2-byte operand (u16 argCount)
        Opcode::SpawnClosure => 2,

        // NativeCall has 3-byte operand (u16 nativeId + u8 argCount)
        Opcode::NativeCall => 3,
    }
}

/// Collect all jump targets from instructions
fn collect_jump_targets(instructions: &[Instruction]) -> Result<HashSet<usize>, VerifyError> {
    let mut targets = HashSet::new();

    for instr in instructions {
        if instr.opcode.is_jump() && !instr.operands.is_empty() {
            // Parse jump offset (i32)
            if instr.operands.len() >= 4 {
                let offset_bytes: [u8; 4] = [
                    instr.operands[0],
                    instr.operands[1],
                    instr.operands[2],
                    instr.operands[3],
                ];
                let jump_offset = i32::from_le_bytes(offset_bytes);

                // Calculate absolute target
                let target = (instr.offset as i32 + 1 + 4 + jump_offset) as usize;
                targets.insert(target);
            }
        }
    }

    Ok(targets)
}

/// Check if an offset is a valid instruction boundary
fn is_valid_instruction_boundary(offset: usize, instructions: &[Instruction]) -> bool {
    instructions.iter().any(|instr| instr.offset == offset)
}

/// Verify stack depth consistency using abstract interpretation
fn verify_stack_depth(
    instructions: &[Instruction],
    _jump_targets: &HashSet<usize>,
) -> Result<(), VerifyError> {
    let mut stack_depth = 0i32;
    const MAX_STACK_DEPTH: i32 = 1024;

    for instr in instructions {
        // Calculate stack effect
        let (pops, pushes) = get_stack_effect(instr.opcode);

        // Check for underflow
        if stack_depth < pops {
            return Err(VerifyError::StackUnderflow(instr.offset));
        }

        stack_depth -= pops;
        stack_depth += pushes;

        // Check for overflow
        if stack_depth > MAX_STACK_DEPTH {
            return Err(VerifyError::StackOverflow(instr.offset, stack_depth));
        }
    }

    Ok(())
}

/// Get the stack effect of an opcode (pops, pushes)
fn get_stack_effect(opcode: Opcode) -> (i32, i32) {
    match opcode {
        Opcode::Nop => (0, 0),
        Opcode::Pop => (1, 0),
        Opcode::Dup => (1, 2),
        Opcode::Swap => (2, 2),
        Opcode::ConstNull | Opcode::ConstTrue | Opcode::ConstFalse => (0, 1),
        Opcode::ConstI32 | Opcode::ConstF64 | Opcode::ConstStr | Opcode::LoadConst => (0, 1),
        Opcode::LoadLocal | Opcode::LoadLocal0 | Opcode::LoadLocal1 => (0, 1),
        Opcode::StoreLocal | Opcode::StoreLocal0 | Opcode::StoreLocal1 => (1, 0),
        Opcode::Iadd | Opcode::Isub | Opcode::Imul | Opcode::Idiv | Opcode::Imod | Opcode::Ipow => (2, 1),
        Opcode::Ishl | Opcode::Ishr | Opcode::Iushr | Opcode::Iand | Opcode::Ior | Opcode::Ixor => (2, 1),
        Opcode::Ineg | Opcode::Fneg | Opcode::Nneg | Opcode::Inot => (1, 1),
        Opcode::Fadd | Opcode::Fsub | Opcode::Fmul | Opcode::Fdiv | Opcode::Fpow | Opcode::Fmod => (2, 1),
        Opcode::Nadd | Opcode::Nsub | Opcode::Nmul | Opcode::Ndiv | Opcode::Nmod | Opcode::Npow => (2, 1),
        Opcode::Ieq | Opcode::Ine | Opcode::Ilt | Opcode::Ile | Opcode::Igt | Opcode::Ige => (2, 1),
        Opcode::Feq | Opcode::Fne | Opcode::Flt | Opcode::Fle | Opcode::Fgt | Opcode::Fge => (2, 1),
        Opcode::Eq | Opcode::Ne | Opcode::StrictEq | Opcode::StrictNe => (2, 1),
        Opcode::Not | Opcode::Typeof => (1, 1),
        Opcode::And | Opcode::Or => (2, 1),
        Opcode::Sconcat => (2, 1),
        Opcode::Slen | Opcode::ToString => (1, 1),
        Opcode::Seq | Opcode::Sne | Opcode::Slt | Opcode::Sle | Opcode::Sgt | Opcode::Sge => (2, 1),
        Opcode::Jmp => (0, 0),
        Opcode::JmpIfFalse | Opcode::JmpIfTrue | Opcode::JmpIfNull | Opcode::JmpIfNotNull => (1, 0),
        Opcode::Return => (1, 0),
        Opcode::ReturnVoid => (0, 0),
        Opcode::Call => (0, 1), // Simplified - actual depends on arg count
        Opcode::CallMethod => (1, 1), // Simplified
        Opcode::CallConstructor | Opcode::CallSuper | Opcode::CallStatic => (0, 1),
        Opcode::New => (0, 1),
        Opcode::LoadField => (1, 1),
        Opcode::StoreField => (2, 0),
        Opcode::LoadFieldFast => (1, 1),
        Opcode::StoreFieldFast => (2, 0),
        Opcode::ObjectLiteral => (0, 1),
        Opcode::InitObject => (0, 0), // Simplified
        Opcode::OptionalField => (1, 1),
        Opcode::LoadStatic => (0, 1),
        Opcode::StoreStatic => (1, 0),
        Opcode::NewArray => (1, 1),
        Opcode::LoadElem => (2, 1),
        Opcode::StoreElem => (3, 0),
        Opcode::ArrayLen => (1, 1),
        Opcode::ArrayLiteral => (0, 1),
        Opcode::InitArray => (0, 0),
        Opcode::TupleLiteral => (0, 1),
        Opcode::InitTuple => (0, 0),
        Opcode::TupleGet => (2, 1),
        Opcode::Spawn => (0, 1),       // pops args (dynamic), pushes TaskHandle
        Opcode::SpawnClosure => (1, 1), // pops closure + args (dynamic), pushes TaskHandle
        Opcode::Await => (1, 1),
        Opcode::Yield => (0, 0),
        Opcode::Sleep => (1, 0),  // pops duration_ms
        Opcode::TaskThen => (1, 1),
        Opcode::NewMutex => (0, 1),
        Opcode::NewChannel => (1, 1), // pops capacity, pushes channel reference
        Opcode::MutexLock | Opcode::MutexUnlock => (1, 0),
        Opcode::Throw => (1, 0),
        Opcode::Trap => (0, 0),
        Opcode::LoadGlobal => (0, 1),
        Opcode::StoreGlobal => (1, 0),
        Opcode::MakeClosure => (0, 1),
        Opcode::CloseVar => (1, 1),
        Opcode::LoadCaptured => (0, 1),
        Opcode::StoreCaptured => (1, 0),
        Opcode::SetClosureCapture => (2, 1), // Pop closure + value, push closure
        Opcode::LoadModule => (0, 1),

        // RefCell operations (for capture-by-reference)
        Opcode::NewRefCell => (1, 1),    // Pop initial value, push refcell ptr
        Opcode::LoadRefCell => (1, 1),   // Pop refcell ptr, push value
        Opcode::StoreRefCell => (2, 0),  // Pop refcell ptr + value

        // JSON operations (parse/stringify use NativeCall)
        Opcode::JsonGet => (1, 1),         // Pop json object, push value
        Opcode::JsonSet => (2, 0),         // Pop value + object (mutates)
        Opcode::JsonDelete => (1, 0),      // Pop object (mutates)
        Opcode::JsonIndex => (2, 1),       // Pop index + json array, push element
        Opcode::JsonIndexSet => (3, 0),    // Pop value + index + array (mutates)
        Opcode::JsonPush => (2, 0),        // Pop value + array (mutates)
        Opcode::JsonPop => (1, 1),         // Pop array, push popped element
        Opcode::JsonNewObject => (0, 1),   // Push new empty json object
        Opcode::JsonNewArray => (0, 1),    // Push new empty json array
        Opcode::JsonKeys => (1, 1),        // Pop json object, push string array
        Opcode::JsonLength => (1, 1),      // Pop json, push length

        // Semaphore operations
        Opcode::NewSemaphore => (1, 1),    // Pop permit count, push semaphore
        Opcode::SemAcquire => (2, 0),      // Pop count, pop semaphore (may block)
        Opcode::SemRelease => (2, 0),      // Pop count, pop semaphore

        // Task waiting
        Opcode::WaitAll => (1, 1),         // Pop task array, push result array

        // Task cancellation
        Opcode::TaskCancel => (1, 0),      // Pop task handle

        // Exception handling
        Opcode::Try => (0, 0),      // Installs handler, no stack effect
        Opcode::EndTry => (0, 0),   // Removes handler, no stack effect
        Opcode::Rethrow => (0, 0),  // Rethrows, unwinds stack

        // Array primitive operations
        Opcode::ArrayPush => (2, 0),  // Pop value + array, mutates array
        Opcode::ArrayPop => (1, 1),   // Pop array, push popped element

        // Type operations
        Opcode::InstanceOf => (2, 1), // Pop class_id + object, push bool
        Opcode::Cast => (2, 1),       // Pop class_id + object, push object (or throw)

        // Native call
        Opcode::NativeCall => (0, 1), // Simplified - pops args (dynamic), pushes result
    }
}

/// Verify constant pool references in instructions
fn verify_constant_refs(instructions: &[Instruction], module: &Module) -> Result<(), VerifyError> {
    for instr in instructions {
        match instr.opcode {
            Opcode::ConstStr | Opcode::LoadConst => {
                if instr.operands.len() >= 4 {
                    let index_bytes: [u8; 4] = [
                        instr.operands[0],
                        instr.operands[1],
                        instr.operands[2],
                        instr.operands[3],
                    ];
                    let index = u32::from_le_bytes(index_bytes);

                    // Check if index is valid
                    if instr.opcode == Opcode::ConstStr
                        && module.constants.get_string(index).is_none()
                    {
                        return Err(VerifyError::InvalidConstantRef {
                            index,
                            offset: instr.offset,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

/// Verify local variable references in instructions
fn verify_local_refs(instructions: &[Instruction], function: &Function) -> Result<(), VerifyError> {
    let max_locals = function.local_count;

    for instr in instructions {
        match instr.opcode {
            Opcode::LoadLocal | Opcode::StoreLocal => {
                if instr.operands.len() >= 2 {
                    let index_bytes: [u8; 2] = [instr.operands[0], instr.operands[1]];
                    let index = u16::from_le_bytes(index_bytes) as usize;

                    if index >= max_locals {
                        return Err(VerifyError::InvalidLocalRef {
                            index,
                            max: max_locals,
                            offset: instr.offset,
                        });
                    }
                }
            }
            Opcode::LoadLocal0 | Opcode::StoreLocal0 => {
                if max_locals == 0 {
                    return Err(VerifyError::InvalidLocalRef {
                        index: 0,
                        max: max_locals,
                        offset: instr.offset,
                    });
                }
            }
            Opcode::LoadLocal1 | Opcode::StoreLocal1 => {
                if 1 >= max_locals {
                    return Err(VerifyError::InvalidLocalRef {
                        index: 1,
                        max: max_locals,
                        offset: instr.offset,
                    });
                }
            }
            _ => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::bytecode::encoder::BytecodeWriter;
    use crate::compiler::bytecode::module::Function;

    #[test]
    fn test_verify_empty_module() {
        let module = Module::new("test".to_string());
        assert!(verify_module(&module).is_ok());
    }

    #[test]
    fn test_verify_simple_function() {
        let mut module = Module::new("test".to_string());

        let mut writer = BytecodeWriter::new();
        writer.emit_const_i32(42);
        writer.emit_return();

        module.functions.push(Function {
            name: "test".to_string(),
            param_count: 0,
            local_count: 1,
            code: writer.into_bytes(),
        });

        assert!(verify_module(&module).is_ok());
    }

    #[test]
    fn test_verify_invalid_opcode() {
        let mut module = Module::new("test".to_string());

        module.functions.push(Function {
            name: "test".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![0xFE], // Invalid opcode (unassigned)
        });

        let result = verify_module(&module);
        assert!(matches!(result, Err(VerifyError::InvalidOpcode { .. })));
    }

    #[test]
    fn test_verify_stack_underflow() {
        let mut module = Module::new("test".to_string());

        let mut writer = BytecodeWriter::new();
        writer.emit_iadd(); // Requires 2 values on stack
        writer.emit_return();

        module.functions.push(Function {
            name: "test".to_string(),
            param_count: 0,
            local_count: 0,
            code: writer.into_bytes(),
        });

        let result = verify_module(&module);
        assert!(matches!(result, Err(VerifyError::StackUnderflow(_))));
    }

    #[test]
    fn test_verify_invalid_local_ref() {
        let mut module = Module::new("test".to_string());

        let mut writer = BytecodeWriter::new();
        writer.emit_load_local(5); // Only 2 locals available
        writer.emit_return();

        module.functions.push(Function {
            name: "test".to_string(),
            param_count: 0,
            local_count: 2,
            code: writer.into_bytes(),
        });

        let result = verify_module(&module);
        assert!(matches!(result, Err(VerifyError::InvalidLocalRef { .. })));
    }

    #[test]
    fn test_verify_valid_locals() {
        let mut module = Module::new("test".to_string());

        let mut writer = BytecodeWriter::new();
        writer.emit_load_local_0();
        writer.emit_load_local_1();
        writer.emit_iadd();
        writer.emit_return();

        module.functions.push(Function {
            name: "test".to_string(),
            param_count: 2,
            local_count: 3,
            code: writer.into_bytes(),
        });

        assert!(verify_module(&module).is_ok());
    }

    #[test]
    fn test_verify_function_without_terminator() {
        let mut module = Module::new("test".to_string());

        let mut writer = BytecodeWriter::new();
        writer.emit_const_i32(42);
        // Missing return!

        module.functions.push(Function {
            name: "test".to_string(),
            param_count: 0,
            local_count: 0,
            code: writer.into_bytes(),
        });

        let result = verify_module(&module);
        assert!(matches!(result, Err(VerifyError::FallOffEnd(_))));
    }
}
