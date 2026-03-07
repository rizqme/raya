//! Bytecode instruction decoder
//!
//! Decodes raw bytecode bytes into typed instruction structs with parsed operands.

use crate::compiler::bytecode::Opcode;

/// Error during bytecode decoding
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("Invalid opcode byte {byte:#x} at offset {offset}")]
    InvalidOpcode { byte: u8, offset: usize },
    #[error("Unexpected end of bytecode at offset {0}")]
    UnexpectedEnd(usize),
}

/// A decoded bytecode instruction with typed operands
#[derive(Debug, Clone)]
pub struct DecodedInstr {
    /// Byte offset in the function's code array
    pub offset: usize,
    /// The opcode
    pub opcode: Opcode,
    /// Decoded operands
    pub operands: Operands,
    /// Total size in bytes (opcode + operands)
    pub size: usize,
}

/// Typed operands for each instruction format
#[derive(Debug, Clone)]
pub enum Operands {
    /// No operands (Nop, Pop, Dup, Iadd, Return, etc.)
    None,
    /// Single u16 (LoadLocal, StoreLocal, LoadFieldExact, etc.)
    U16(u16),
    /// Single u64 (CastShape)
    U64(u64),
    /// Single u32 (LoadConst, LoadGlobal, etc.)
    U32(u32),
    /// Single i32 (ConstI32, etc.)
    I32(i32),
    /// Single f64 (ConstF64)
    F64(f64),
    /// Call: func_index (u32) + arg_count (u16)
    Call { func_index: u32, arg_count: u16 },
    /// Try: catch_offset (i32) + finally_offset (i32)
    Try {
        catch_offset: i32,
        finally_offset: i32,
    },
    /// NativeCall: native_id (u16) + arg_count (u8)
    NativeCall { native_id: u16, arg_count: u8 },
    /// Nominal constructor on an existing object: nominal_type_id (u16) + arg_count (u8)
    ConstructType { nominal_type_id: u16, arg_count: u8 },
    /// MakeClosure: func_index (u32) + capture_count (u16)
    MakeClosure { func_index: u32, capture_count: u16 },
    /// Spawn: func_index (u16) + arg_count (u16) (reused for SpawnClosure: arg_count u16)
    Spawn { func_index: u16, arg_count: u16 },
    /// ArrayLiteral: type_index (u32) + length (u32)
    ArrayLiteral { type_index: u32, length: u32 },
    /// Structural shape field op: shape_id (u64) + slot (u16)
    ShapeSlot { shape_id: u64, slot: u16 },
    /// Structural shape method call: shape_id (u64) + slot (u16) + arg_count (u16)
    ShapeMethodCall {
        shape_id: u64,
        slot: u16,
        arg_count: u16,
    },
}

#[derive(Clone, Copy)]
enum JumpWidth {
    I16,
    I32,
}

/// Decode all instructions in a function's bytecode
pub fn decode_function(code: &[u8]) -> Result<Vec<DecodedInstr>, DecodeError> {
    // Support both historical encodings used across the codebase/tests.
    // Prefer i16 (current VM execution path), then fall back to i32.
    decode_function_with_jump_width(code, JumpWidth::I16)
        .or_else(|_| decode_function_with_jump_width(code, JumpWidth::I32))
}

fn decode_function_with_jump_width(
    code: &[u8],
    jump_width: JumpWidth,
) -> Result<Vec<DecodedInstr>, DecodeError> {
    let mut instrs = Vec::new();
    let mut pos = 0;

    while pos < code.len() {
        let offset = pos;
        let byte = code[pos];
        let opcode = Opcode::from_u8(byte).ok_or(DecodeError::InvalidOpcode { byte, offset })?;
        pos += 1;

        let operands = decode_operands(opcode, code, &mut pos, offset, jump_width)?;
        let size = pos - offset;

        instrs.push(DecodedInstr {
            offset,
            opcode,
            operands,
            size,
        });
    }

    Ok(instrs)
}

fn decode_operands(
    opcode: Opcode,
    code: &[u8],
    pos: &mut usize,
    offset: usize,
    jump_width: JumpWidth,
) -> Result<Operands, DecodeError> {
    match opcode {
        // No operands (1 byte total)
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
        | Opcode::GetArgCount
        | Opcode::LoadArgLocal
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
        | Opcode::NewMutex
        | Opcode::NewChannel
        | Opcode::MutexLock
        | Opcode::MutexUnlock
        | Opcode::NewSemaphore
        | Opcode::SemAcquire
        | Opcode::SemRelease
        | Opcode::WaitAll
        | Opcode::Sleep
        | Opcode::TaskCancel
        | Opcode::DynGetKeyed
        | Opcode::DynSetKeyed
        | Opcode::DynNewObject
        | Opcode::DynKeys
        | Opcode::DynHas
        | Opcode::DynNewObject
        | Opcode::DynKeys
        | Opcode::DynKeys
        | Opcode::Throw
        | Opcode::EndTry
        | Opcode::Rethrow
        | Opcode::TupleGet
        | Opcode::NewRefCell
        | Opcode::LoadRefCell
        | Opcode::StoreRefCell
        | Opcode::ArrayPush
        | Opcode::ArrayPop
        | Opcode::Debugger => Ok(Operands::None),

        // u16 operand (3 bytes total)
        Opcode::LoadLocal
        | Opcode::StoreLocal
        | Opcode::LoadFieldExact
        | Opcode::StoreFieldExact
        | Opcode::OptionalFieldExact
        | Opcode::ConstStr
        | Opcode::CloseVar
        | Opcode::LoadCaptured
        | Opcode::StoreCaptured
        | Opcode::SetClosureCapture
        | Opcode::InitObject
        | Opcode::InitArray
        | Opcode::InitTuple
        | Opcode::Trap
        | Opcode::BindMethod
        | Opcode::NewType
        | Opcode::IsNominal
        | Opcode::CastTupleLen
        | Opcode::CastObjectMinFields
        | Opcode::CastArrayElemKind
        | Opcode::CastKindMask
        | Opcode::Cast
        | Opcode::CastNominal => {
            let v = read_u16(code, pos, offset)?;
            Ok(Operands::U16(v))
        }

        Opcode::CastShape | Opcode::ImplementsShape => {
            let shape_id = read_u64(code, pos, offset)?;
            Ok(Operands::U64(shape_id))
        }

        // SpawnClosure: u16 arg_count
        Opcode::SpawnClosure => {
            let arg_count = read_u16(code, pos, offset)?;
            Ok(Operands::U16(arg_count))
        }

        // Jump offsets are historically encoded as either i16 or i32 depending on producer.
        Opcode::Jmp
        | Opcode::JmpIfFalse
        | Opcode::JmpIfTrue
        | Opcode::JmpIfNull
        | Opcode::JmpIfNotNull => {
            let v = match jump_width {
                // Compiler/VM i16 jump offsets are relative to the instruction
                // pointer *after* reading the i16 immediate.
                // CFG currently resolves relative offsets from opcode start,
                // so normalize i16 by adding instruction size (opcode+i16 = 3).
                JumpWidth::I16 => read_i16(code, pos, offset)? as i32 + 3,
                JumpWidth::I32 => read_i32(code, pos, offset)?,
            };
            Ok(Operands::I32(v))
        }

        // i32 operand — ConstI32
        Opcode::ConstI32 => {
            let v = read_i32(code, pos, offset)?;
            Ok(Operands::I32(v))
        }

        // f64 operand
        Opcode::ConstF64 => {
            let v = read_f64(code, pos, offset)?;
            Ok(Operands::F64(v))
        }

        // u32 operand (5 bytes total)
        Opcode::LoadConst
        | Opcode::NewArray
        | Opcode::TaskThen
        | Opcode::LoadModule
        | Opcode::LoadGlobal
        | Opcode::StoreGlobal
        | Opcode::LoadStatic
        | Opcode::StoreStatic
        | Opcode::DynGet
        | Opcode::DynSet
        | Opcode::DynDelete => {
            let v = read_u32(code, pos, offset)?;
            Ok(Operands::U32(v))
        }

        // u32 + u16 operands — calls (7 bytes total)
        Opcode::Call
        | Opcode::CallMethodExact
        | Opcode::OptionalCallMethodExact
        | Opcode::CallConstructor
        | Opcode::CallSuper
        | Opcode::CallStatic => {
            let func_index = read_u32(code, pos, offset)?;
            let arg_count = read_u16(code, pos, offset)?;
            Ok(Operands::Call {
                func_index,
                arg_count,
            })
        }

        Opcode::ConstructType => {
            let nominal_type_id = read_u16(code, pos, offset)?;
            let arg_count = read_u8(code, pos, offset)?;
            Ok(Operands::ConstructType {
                nominal_type_id,
                arg_count,
            })
        }

        // u32 + u16 — Spawn
        Opcode::Spawn => {
            let raw = read_u32(code, pos, offset)?;
            let func_index = raw as u16;
            let arg_count = read_u16(code, pos, offset)?;
            Ok(Operands::Spawn {
                func_index,
                arg_count,
            })
        }

        // u32 + u16 — ObjectLiteral, TupleLiteral
        Opcode::ObjectLiteral | Opcode::TupleLiteral => {
            let type_index = read_u32(code, pos, offset)?;
            let count = read_u16(code, pos, offset)?;
            Ok(Operands::Call {
                func_index: type_index,
                arg_count: count,
            })
        }

        // u32 + u16 — MakeClosure
        Opcode::MakeClosure => {
            let func_index = read_u32(code, pos, offset)?;
            let capture_count = read_u16(code, pos, offset)?;
            Ok(Operands::MakeClosure {
                func_index,
                capture_count,
            })
        }

        // u32 + u32 — ArrayLiteral
        Opcode::ArrayLiteral => {
            let type_index = read_u32(code, pos, offset)?;
            let length = read_u32(code, pos, offset)?;
            Ok(Operands::ArrayLiteral { type_index, length })
        }

        // i32 + i32 — Try
        Opcode::Try => {
            let catch_offset = read_i32(code, pos, offset)?;
            let finally_offset = read_i32(code, pos, offset)?;
            Ok(Operands::Try {
                catch_offset,
                finally_offset,
            })
        }

        // u16 + u8 — NativeCall, ModuleNativeCall
        Opcode::NativeCall | Opcode::ModuleNativeCall => {
            let native_id = read_u16(code, pos, offset)?;
            let arg_count = read_u8(code, pos, offset)?;
            Ok(Operands::NativeCall {
                native_id,
                arg_count,
            })
        }

        // u64 + u16 — structural shape field operations
        Opcode::LoadFieldShape | Opcode::StoreFieldShape | Opcode::OptionalFieldShape => {
            let shape_id = read_u64(code, pos, offset)?;
            let slot = read_u16(code, pos, offset)?;
            Ok(Operands::ShapeSlot { shape_id, slot })
        }

        Opcode::CallMethodShape | Opcode::OptionalCallMethodShape => {
            let shape_id = read_u64(code, pos, offset)?;
            let slot = read_u16(code, pos, offset)?;
            let arg_count = read_u16(code, pos, offset)?;
            Ok(Operands::ShapeMethodCall {
                shape_id,
                slot,
                arg_count,
            })
        }
    }
}

fn read_u8(code: &[u8], pos: &mut usize, offset: usize) -> Result<u8, DecodeError> {
    if *pos >= code.len() {
        return Err(DecodeError::UnexpectedEnd(offset));
    }
    let v = code[*pos];
    *pos += 1;
    Ok(v)
}

fn read_u16(code: &[u8], pos: &mut usize, offset: usize) -> Result<u16, DecodeError> {
    if *pos + 2 > code.len() {
        return Err(DecodeError::UnexpectedEnd(offset));
    }
    let v = u16::from_le_bytes([code[*pos], code[*pos + 1]]);
    *pos += 2;
    Ok(v)
}

fn read_u32(code: &[u8], pos: &mut usize, offset: usize) -> Result<u32, DecodeError> {
    if *pos + 4 > code.len() {
        return Err(DecodeError::UnexpectedEnd(offset));
    }
    let v = u32::from_le_bytes([code[*pos], code[*pos + 1], code[*pos + 2], code[*pos + 3]]);
    *pos += 4;
    Ok(v)
}

fn read_u64(code: &[u8], pos: &mut usize, offset: usize) -> Result<u64, DecodeError> {
    if *pos + 8 > code.len() {
        return Err(DecodeError::UnexpectedEnd(offset));
    }
    let v = u64::from_le_bytes([
        code[*pos],
        code[*pos + 1],
        code[*pos + 2],
        code[*pos + 3],
        code[*pos + 4],
        code[*pos + 5],
        code[*pos + 6],
        code[*pos + 7],
    ]);
    *pos += 8;
    Ok(v)
}

fn read_i32(code: &[u8], pos: &mut usize, offset: usize) -> Result<i32, DecodeError> {
    if *pos + 4 > code.len() {
        return Err(DecodeError::UnexpectedEnd(offset));
    }
    let v = i32::from_le_bytes([code[*pos], code[*pos + 1], code[*pos + 2], code[*pos + 3]]);
    *pos += 4;
    Ok(v)
}

fn read_i16(code: &[u8], pos: &mut usize, offset: usize) -> Result<i16, DecodeError> {
    if *pos + 2 > code.len() {
        return Err(DecodeError::UnexpectedEnd(offset));
    }
    let v = i16::from_le_bytes([code[*pos], code[*pos + 1]]);
    *pos += 2;
    Ok(v)
}

fn read_f64(code: &[u8], pos: &mut usize, offset: usize) -> Result<f64, DecodeError> {
    if *pos + 8 > code.len() {
        return Err(DecodeError::UnexpectedEnd(offset));
    }
    let v = f64::from_le_bytes([
        code[*pos],
        code[*pos + 1],
        code[*pos + 2],
        code[*pos + 3],
        code[*pos + 4],
        code[*pos + 5],
        code[*pos + 6],
        code[*pos + 7],
    ]);
    *pos += 8;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_empty() {
        let instrs = decode_function(&[]).unwrap();
        assert!(instrs.is_empty());
    }

    #[test]
    fn test_decode_nop() {
        let code = [Opcode::Nop as u8];
        let instrs = decode_function(&code).unwrap();
        assert_eq!(instrs.len(), 1);
        assert_eq!(instrs[0].opcode, Opcode::Nop);
        assert_eq!(instrs[0].size, 1);
        assert!(matches!(instrs[0].operands, Operands::None));
    }

    #[test]
    fn test_decode_const_i32() {
        let mut code = vec![Opcode::ConstI32 as u8];
        code.extend_from_slice(&42i32.to_le_bytes());
        let instrs = decode_function(&code).unwrap();
        assert_eq!(instrs.len(), 1);
        assert_eq!(instrs[0].opcode, Opcode::ConstI32);
        assert_eq!(instrs[0].size, 5);
        assert!(matches!(instrs[0].operands, Operands::I32(42)));
    }

    #[test]
    fn test_decode_const_f64() {
        let mut code = vec![Opcode::ConstF64 as u8];
        code.extend_from_slice(&3.14f64.to_le_bytes());
        let instrs = decode_function(&code).unwrap();
        assert_eq!(instrs.len(), 1);
        assert_eq!(instrs[0].opcode, Opcode::ConstF64);
        assert_eq!(instrs[0].size, 9);
        if let Operands::F64(v) = instrs[0].operands {
            assert!((v - 3.14).abs() < 1e-10);
        } else {
            panic!("expected F64 operand");
        }
    }

    #[test]
    fn test_decode_load_local() {
        let mut code = vec![Opcode::LoadLocal as u8];
        code.extend_from_slice(&5u16.to_le_bytes());
        let instrs = decode_function(&code).unwrap();
        assert_eq!(instrs.len(), 1);
        assert!(matches!(instrs[0].operands, Operands::U16(5)));
    }

    #[test]
    fn test_decode_call() {
        let mut code = vec![Opcode::Call as u8];
        code.extend_from_slice(&10u32.to_le_bytes());
        code.extend_from_slice(&3u16.to_le_bytes());
        let instrs = decode_function(&code).unwrap();
        assert_eq!(instrs.len(), 1);
        assert!(matches!(
            instrs[0].operands,
            Operands::Call {
                func_index: 10,
                arg_count: 3
            }
        ));
    }

    #[test]
    fn test_decode_jmp() {
        let mut code = vec![Opcode::Jmp as u8];
        code.extend_from_slice(&(-10i32).to_le_bytes());
        let instrs = decode_function(&code).unwrap();
        assert_eq!(instrs.len(), 1);
        assert!(matches!(instrs[0].operands, Operands::I32(-10)));
    }

    #[test]
    fn test_decode_try() {
        let mut code = vec![Opcode::Try as u8];
        code.extend_from_slice(&20i32.to_le_bytes());
        code.extend_from_slice(&(-1i32).to_le_bytes());
        let instrs = decode_function(&code).unwrap();
        assert_eq!(instrs.len(), 1);
        assert!(matches!(
            instrs[0].operands,
            Operands::Try {
                catch_offset: 20,
                finally_offset: -1
            }
        ));
    }

    #[test]
    fn test_decode_native_call() {
        let mut code = vec![Opcode::NativeCall as u8];
        code.extend_from_slice(&0x0100u16.to_le_bytes());
        code.push(2);
        let instrs = decode_function(&code).unwrap();
        assert_eq!(instrs.len(), 1);
        assert!(matches!(
            instrs[0].operands,
            Operands::NativeCall {
                native_id: 0x0100,
                arg_count: 2
            }
        ));
    }

    #[test]
    fn test_decode_sequence() {
        // ConstI32 42, ConstI32 10, Iadd, Return
        let mut code = vec![Opcode::ConstI32 as u8];
        code.extend_from_slice(&42i32.to_le_bytes());
        code.push(Opcode::ConstI32 as u8);
        code.extend_from_slice(&10i32.to_le_bytes());
        code.push(Opcode::Iadd as u8);
        code.push(Opcode::Return as u8);

        let instrs = decode_function(&code).unwrap();
        assert_eq!(instrs.len(), 4);
        assert_eq!(instrs[0].opcode, Opcode::ConstI32);
        assert_eq!(instrs[1].opcode, Opcode::ConstI32);
        assert_eq!(instrs[2].opcode, Opcode::Iadd);
        assert_eq!(instrs[3].opcode, Opcode::Return);
    }

    #[test]
    fn test_decode_invalid_opcode() {
        let code = [0xFF];
        let result = decode_function(&code);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_truncated() {
        // ConstI32 but only 2 bytes of operand
        let code = [Opcode::ConstI32 as u8, 0x01, 0x02];
        let result = decode_function(&code);
        assert!(result.is_err());
    }
}
