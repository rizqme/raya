//! SSA builder helpers
//!
//! Utilities for constructing JIT IR during the lifting phase.

use super::instr::{JitBlockId, JitFunction, JitInstr, JitTerminator, Reg};
use super::types::JitType;

/// Builder that simplifies JIT IR construction
pub struct JitBuilder<'a> {
    func: &'a mut JitFunction,
    current_block: JitBlockId,
}

impl<'a> JitBuilder<'a> {
    /// Create a builder targeting an existing function
    pub fn new(func: &'a mut JitFunction) -> Self {
        let entry = func.entry;
        JitBuilder {
            func,
            current_block: entry,
        }
    }

    /// Switch to emitting into a different block
    pub fn switch_to_block(&mut self, block: JitBlockId) {
        self.current_block = block;
    }

    /// Get the current block ID
    pub fn current_block(&self) -> JitBlockId {
        self.current_block
    }

    /// Allocate a new virtual register with the given type
    pub fn alloc_reg(&mut self, ty: JitType) -> Reg {
        self.func.alloc_reg(ty)
    }

    /// Create a new basic block
    pub fn create_block(&mut self) -> JitBlockId {
        self.func.add_block()
    }

    /// Emit an instruction into the current block
    pub fn emit(&mut self, instr: JitInstr) {
        self.func.block_mut(self.current_block).instrs.push(instr);
    }

    /// Set the terminator for the current block
    pub fn terminate(&mut self, term: JitTerminator) {
        self.func.block_mut(self.current_block).terminator = term;
    }

    /// Emit a constant i32 and return the destination register
    pub fn const_i32(&mut self, value: i32) -> Reg {
        let dest = self.alloc_reg(JitType::I32);
        self.emit(JitInstr::ConstI32 { dest, value });
        dest
    }

    /// Emit a constant f64 and return the destination register
    pub fn const_f64(&mut self, value: f64) -> Reg {
        let dest = self.alloc_reg(JitType::F64);
        self.emit(JitInstr::ConstF64 { dest, value });
        dest
    }

    /// Emit a constant bool and return the destination register
    pub fn const_bool(&mut self, value: bool) -> Reg {
        let dest = self.alloc_reg(JitType::Bool);
        self.emit(JitInstr::ConstBool { dest, value });
        dest
    }

    /// Emit a null constant
    pub fn const_null(&mut self) -> Reg {
        let dest = self.alloc_reg(JitType::Value);
        self.emit(JitInstr::ConstNull { dest });
        dest
    }

    /// Emit BoxI32: wrap i32 register into NaN-boxed Value
    pub fn box_i32(&mut self, src: Reg) -> Reg {
        let dest = self.alloc_reg(JitType::Value);
        self.emit(JitInstr::BoxI32 { dest, src });
        dest
    }

    /// Emit UnboxI32: extract i32 from NaN-boxed Value
    pub fn unbox_i32(&mut self, src: Reg) -> Reg {
        let dest = self.alloc_reg(JitType::I32);
        self.emit(JitInstr::UnboxI32 { dest, src });
        dest
    }

    /// Emit BoxF64: wrap f64 register into NaN-boxed Value
    pub fn box_f64(&mut self, src: Reg) -> Reg {
        let dest = self.alloc_reg(JitType::Value);
        self.emit(JitInstr::BoxF64 { dest, src });
        dest
    }

    /// Emit UnboxF64: extract f64 from NaN-boxed Value
    pub fn unbox_f64(&mut self, src: Reg) -> Reg {
        let dest = self.alloc_reg(JitType::F64);
        self.emit(JitInstr::UnboxF64 { dest, src });
        dest
    }

    /// Emit a LoadLocal instruction
    pub fn load_local(&mut self, index: u16) -> Reg {
        let dest = self.alloc_reg(JitType::Value);
        self.emit(JitInstr::LoadLocal { dest, index });
        dest
    }

    /// Emit a StoreLocal instruction
    pub fn store_local(&mut self, index: u16, value: Reg) {
        self.emit(JitInstr::StoreLocal { index, value });
    }

    /// Access the underlying function
    pub fn func(&self) -> &JitFunction {
        self.func
    }

    /// Access the underlying function mutably
    pub fn func_mut(&mut self) -> &mut JitFunction {
        self.func
    }
}
