//! Bytecode Emission Helpers
//!
//! Helper functions for emitting common bytecode patterns.

use crate::compiler::bytecode::Opcode;

/// Estimate the size of an opcode plus its operands
pub fn opcode_size(opcode: Opcode) -> usize {
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
        | Opcode::Debugger
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
        | Opcode::JsonIndex
        | Opcode::JsonIndexSet
        | Opcode::JsonPush
        | Opcode::JsonPop
        | Opcode::JsonNewObject
        | Opcode::JsonNewArray
        | Opcode::JsonKeys
        | Opcode::JsonLength
        | Opcode::Throw
        | Opcode::EndTry
        | Opcode::Rethrow
        | Opcode::TupleGet
        | Opcode::NewRefCell
        | Opcode::LoadRefCell
        | Opcode::StoreRefCell
        | Opcode::ArrayPush
        | Opcode::ArrayPop
        | Opcode::InstanceOf
        | Opcode::Cast => 1,

        // u16 operand (BindMethod: opcode + u16 method_slot)
        Opcode::BindMethod => 1 + 2,

        // u16 operand
        Opcode::LoadLocal
        | Opcode::StoreLocal
        | Opcode::LoadField
        | Opcode::StoreField
        | Opcode::LoadFieldFast
        | Opcode::StoreFieldFast
        | Opcode::OptionalField
        | Opcode::ConstStr
        | Opcode::CloseVar
        | Opcode::LoadCaptured
        | Opcode::StoreCaptured
        | Opcode::SetClosureCapture
        | Opcode::InitObject
        | Opcode::InitArray
        | Opcode::InitTuple
        | Opcode::SpawnClosure
        | Opcode::Trap => 1 + 2,

        // i32 operand (jumps)
        Opcode::Jmp
        | Opcode::JmpIfFalse
        | Opcode::JmpIfTrue
        | Opcode::JmpIfNull
        | Opcode::JmpIfNotNull => 1 + 4,

        // i32 operand (constants)
        Opcode::ConstI32 => 1 + 4,

        // f64 operand
        Opcode::ConstF64 => 1 + 8,

        // u32 operand
        Opcode::LoadConst
        | Opcode::New
        | Opcode::NewArray
        | Opcode::TaskThen
        | Opcode::LoadModule
        | Opcode::LoadGlobal
        | Opcode::StoreGlobal
        | Opcode::LoadStatic
        | Opcode::StoreStatic
        | Opcode::JsonGet
        | Opcode::JsonSet
        | Opcode::JsonDelete => 1 + 4,

        // u32 + u16 operands
        Opcode::Call
        | Opcode::CallMethod
        | Opcode::CallConstructor
        | Opcode::CallSuper
        | Opcode::CallStatic
        | Opcode::Spawn
        | Opcode::ObjectLiteral
        | Opcode::TupleLiteral
        | Opcode::MakeClosure => 1 + 4 + 2,

        // u32 + u32 operands
        Opcode::ArrayLiteral => 1 + 4 + 4,

        // i32 + i32 operands (try block)
        Opcode::Try => 1 + 4 + 4,

        // u16 + u8 operands (native call)
        Opcode::NativeCall => 1 + 2 + 1,

        // u16 + u8 operands (module native call)
        Opcode::ModuleNativeCall => 1 + 2 + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opcode_sizes() {
        assert_eq!(opcode_size(Opcode::Nop), 1);
        assert_eq!(opcode_size(Opcode::ConstI32), 5);
        assert_eq!(opcode_size(Opcode::ConstF64), 9);
        assert_eq!(opcode_size(Opcode::Jmp), 5);
        assert_eq!(opcode_size(Opcode::Call), 7);
    }
}
