//! Code Generation from IR to Bytecode
//!
//! This module transforms the optimized IR into bytecode for the Raya VM.
//!
//! # Pipeline
//!
//! ```text
//! IR Module → IrCodeGenerator → Bytecode Module
//! ```
//!
//! # Phases
//!
//! 1. **Basic Emission**: Constants, locals, binary/unary ops
//! 2. **Control Flow**: Branches, loops, switches
//! 3. **Classes/Objects**: Field access, method calls, constructors
//! 4. **Closures**: Captured variables
//! 5. **Optimizations**: String comparison optimization

mod context;
pub mod emit;
mod control;

pub use context::IrCodeGenerator;

use crate::bytecode::{Function, Module, Opcode};
use crate::error::{CompileError, CompileResult};
use crate::ir::{
    BasicBlock, BasicBlockId, BinaryOp, ClassId, FunctionId, IrConstant, IrFunction, IrInstr,
    IrModule, IrValue, Register, Terminator, UnaryOp,
};
use crate::module_builder::{FunctionBuilder, ModuleBuilder};
use rustc_hash::FxHashMap;

/// Generate bytecode from an IR module
pub fn generate(ir_module: &IrModule) -> CompileResult<Module> {
    let mut generator = IrCodeGenerator::new(&ir_module.name);
    generator.generate(ir_module)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BasicBlock, IrFunction, IrModule};
    use crate::ir::block::Terminator;
    use crate::ir::value::{IrConstant, IrValue, Register, RegisterId};
    use raya_parser::TypeId;

    fn make_reg(id: u32, ty: u32) -> Register {
        Register::new(RegisterId::new(id), TypeId::new(ty))
    }

    #[test]
    fn test_generate_empty_module() {
        let mut module = IrModule::new("test");

        // Add a simple main function that returns null
        let mut main = IrFunction::new("main", vec![], TypeId::new(0));
        let mut entry = BasicBlock::new(BasicBlockId(0));
        entry.set_terminator(Terminator::Return(None));
        main.add_block(entry);
        module.add_function(main);

        let result = generate(&module);
        assert!(result.is_ok());

        let bytecode = result.unwrap();
        assert_eq!(bytecode.functions.len(), 1);
        assert_eq!(bytecode.functions[0].name, "main");
    }

    #[test]
    fn test_generate_return_constant() {
        let mut module = IrModule::new("test");

        let mut func = IrFunction::new("answer", vec![], TypeId::new(1));
        let mut entry = BasicBlock::new(BasicBlockId(0));

        // r0 = 42
        let r0 = make_reg(0, 1);
        entry.add_instr(IrInstr::Assign {
            dest: r0.clone(),
            value: IrValue::Constant(IrConstant::I32(42)),
        });
        entry.set_terminator(Terminator::Return(Some(r0)));
        func.add_block(entry);
        module.add_function(func);

        let result = generate(&module);
        assert!(result.is_ok());

        let bytecode = result.unwrap();
        assert_eq!(bytecode.functions.len(), 1);
        // Code should contain: CONST_I32 42, RETURN
        assert!(!bytecode.functions[0].code.is_empty());
    }

    #[test]
    fn test_generate_binary_add() {
        let mut module = IrModule::new("test");

        let mut func = IrFunction::new("add", vec![], TypeId::new(1));
        let mut entry = BasicBlock::new(BasicBlockId(0));

        // r0 = 10
        let r0 = make_reg(0, 1);
        entry.add_instr(IrInstr::Assign {
            dest: r0.clone(),
            value: IrValue::Constant(IrConstant::I32(10)),
        });

        // r1 = 20
        let r1 = make_reg(1, 1);
        entry.add_instr(IrInstr::Assign {
            dest: r1.clone(),
            value: IrValue::Constant(IrConstant::I32(20)),
        });

        // r2 = r0 + r1
        let r2 = make_reg(2, 1);
        entry.add_instr(IrInstr::BinaryOp {
            dest: r2.clone(),
            op: BinaryOp::Add,
            left: r0,
            right: r1,
        });

        entry.set_terminator(Terminator::Return(Some(r2)));
        func.add_block(entry);
        module.add_function(func);

        let result = generate(&module);
        assert!(result.is_ok());
    }
}
