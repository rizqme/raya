//! Dead Code Elimination (DCE)
//!
//! Removes instructions whose results are never used.

use crate::ir::{BasicBlock, IrFunction, IrInstr, IrModule, IrValue, RegisterId, Terminator};
use rustc_hash::FxHashSet;

/// Dead code eliminator
pub struct DeadCodeEliminator;

impl DeadCodeEliminator {
    /// Create a new DCE pass
    pub fn new() -> Self {
        Self
    }

    /// Eliminate dead code in an entire module
    pub fn eliminate(&self, module: &mut IrModule) {
        for func in &mut module.functions {
            self.eliminate_function(func);
        }
    }

    /// Eliminate dead code in a function
    fn eliminate_function(&self, func: &mut IrFunction) {
        // Iterate until no more changes (fixed-point)
        loop {
            let used = self.collect_used_registers(func);
            let removed = self.remove_dead_instructions(func, &used);
            if !removed {
                break;
            }
        }
    }

    /// Collect all used registers in the function
    fn collect_used_registers(&self, func: &IrFunction) -> FxHashSet<RegisterId> {
        let mut used = FxHashSet::default();

        for block in &func.blocks {
            // Collect uses from instructions
            for instr in &block.instructions {
                self.collect_instruction_uses(instr, &mut used);
            }

            // Collect uses from terminator
            self.collect_terminator_uses(&block.terminator, &mut used);
        }

        used
    }

    /// Collect register uses from an instruction
    fn collect_instruction_uses(&self, instr: &IrInstr, used: &mut FxHashSet<RegisterId>) {
        match instr {
            IrInstr::Assign { value, .. } => {
                if let IrValue::Register(reg) = value {
                    used.insert(reg.id);
                }
            }
            IrInstr::BinaryOp { left, right, .. } => {
                used.insert(left.id);
                used.insert(right.id);
            }
            IrInstr::UnaryOp { operand, .. } => {
                used.insert(operand.id);
            }
            IrInstr::Call { args, .. } => {
                for arg in args {
                    used.insert(arg.id);
                }
            }
            IrInstr::CallMethod { object, args, .. } => {
                used.insert(object.id);
                for arg in args {
                    used.insert(arg.id);
                }
            }
            IrInstr::StoreLocal { value, .. } => {
                used.insert(value.id);
            }
            IrInstr::LoadField { object, .. } => {
                used.insert(object.id);
            }
            IrInstr::StoreField { object, value, .. } => {
                used.insert(object.id);
                used.insert(value.id);
            }
            IrInstr::LoadElement { array, index, .. } => {
                used.insert(array.id);
                used.insert(index.id);
            }
            IrInstr::StoreElement { array, index, value } => {
                used.insert(array.id);
                used.insert(index.id);
                used.insert(value.id);
            }
            IrInstr::NewArray { len, .. } => {
                used.insert(len.id);
            }
            IrInstr::ArrayLiteral { elements, .. } => {
                for elem in elements {
                    used.insert(elem.id);
                }
            }
            IrInstr::ObjectLiteral { fields, .. } => {
                for (_, value) in fields {
                    used.insert(value.id);
                }
            }
            IrInstr::ArrayLen { array, .. } => {
                used.insert(array.id);
            }
            IrInstr::Typeof { operand, .. } => {
                used.insert(operand.id);
            }
            IrInstr::Phi { sources, .. } => {
                for (_, reg) in sources {
                    used.insert(reg.id);
                }
            }
            IrInstr::LoadLocal { .. } | IrInstr::NewObject { .. } => {
                // No register uses
            }
        }
    }

    /// Collect register uses from a terminator
    fn collect_terminator_uses(&self, term: &Terminator, used: &mut FxHashSet<RegisterId>) {
        match term {
            Terminator::Branch { cond, .. } => {
                used.insert(cond.id);
            }
            Terminator::Return(Some(reg)) => {
                used.insert(reg.id);
            }
            Terminator::Switch { value, .. } => {
                used.insert(value.id);
            }
            Terminator::Throw(reg) => {
                used.insert(reg.id);
            }
            Terminator::Jump(_) | Terminator::Return(None) | Terminator::Unreachable => {}
        }
    }

    /// Remove dead instructions from all blocks
    /// Returns true if any instructions were removed
    fn remove_dead_instructions(
        &self,
        func: &mut IrFunction,
        used: &FxHashSet<RegisterId>,
    ) -> bool {
        let mut removed_any = false;

        for block in &mut func.blocks {
            let before_len = block.instructions.len();

            block.instructions.retain(|instr| {
                // Keep instructions with side effects
                if instr.has_side_effects() {
                    return true;
                }

                // Keep instructions whose result is used
                if let Some(dest) = instr.dest() {
                    if used.contains(&dest.id) {
                        return true;
                    }
                }

                // Dead instruction - remove it
                false
            });

            if block.instructions.len() < before_len {
                removed_any = true;
            }
        }

        removed_any
    }
}

impl Default for DeadCodeEliminator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BasicBlock, BasicBlockId, IrConstant, IrFunction, IrModule, IrValue, Register};
    use raya_parser::TypeId;

    fn make_reg(id: u32) -> Register {
        Register::new(RegisterId::new(id), TypeId::new(1))
    }

    fn make_function(instrs: Vec<IrInstr>) -> IrFunction {
        let mut func = IrFunction::new("test", vec![], TypeId::new(0));
        let mut block = BasicBlock::new(BasicBlockId(0));
        for instr in instrs {
            block.add_instr(instr);
        }
        block.set_terminator(Terminator::Return(None));
        func.add_block(block);
        func
    }

    #[test]
    fn test_eliminate_unused_assign() {
        let dce = DeadCodeEliminator::new();

        let instrs = vec![
            IrInstr::Assign {
                dest: make_reg(0),
                value: IrValue::Constant(IrConstant::I32(42)),
            },
            // r0 is never used
        ];

        let mut module = IrModule::new("test");
        module.add_function(make_function(instrs));
        dce.eliminate(&mut module);

        let func = module.get_function(crate::ir::FunctionId::new(0)).unwrap();
        let block = func.get_block(BasicBlockId(0)).unwrap();

        // The unused assignment should be removed
        assert!(block.instructions.is_empty());
    }

    #[test]
    fn test_keep_used_value() {
        let dce = DeadCodeEliminator::new();

        let instrs = vec![IrInstr::Assign {
            dest: make_reg(0),
            value: IrValue::Constant(IrConstant::I32(42)),
        }];

        let mut func = IrFunction::new("test", vec![], TypeId::new(0));
        let mut block = BasicBlock::new(BasicBlockId(0));
        for instr in instrs {
            block.add_instr(instr);
        }
        // Return the value, so it's used
        block.set_terminator(Terminator::Return(Some(make_reg(0))));
        func.add_block(block);

        let mut module = IrModule::new("test");
        module.add_function(func);
        dce.eliminate(&mut module);

        let func = module.get_function(crate::ir::FunctionId::new(0)).unwrap();
        let block = func.get_block(BasicBlockId(0)).unwrap();

        // The assignment should be kept because the value is returned
        assert_eq!(block.instructions.len(), 1);
    }

    #[test]
    fn test_keep_side_effects() {
        let dce = DeadCodeEliminator::new();

        let instrs = vec![
            // Function call has side effects, should be kept
            IrInstr::Call {
                dest: None,
                func: crate::ir::FunctionId::new(0),
                args: vec![],
            },
        ];

        let mut module = IrModule::new("test");
        module.add_function(make_function(instrs));
        dce.eliminate(&mut module);

        let func = module.get_function(crate::ir::FunctionId::new(0)).unwrap();
        let block = func.get_block(BasicBlockId(0)).unwrap();

        // The call should be kept due to side effects
        assert_eq!(block.instructions.len(), 1);
    }

    #[test]
    fn test_transitive_elimination() {
        let dce = DeadCodeEliminator::new();

        // r0 = 10
        // r1 = 20
        // r2 = r0 + r1  (r2 is unused)
        // Neither r0, r1, nor r2 are used
        let instrs = vec![
            IrInstr::Assign {
                dest: make_reg(0),
                value: IrValue::Constant(IrConstant::I32(10)),
            },
            IrInstr::Assign {
                dest: make_reg(1),
                value: IrValue::Constant(IrConstant::I32(20)),
            },
            IrInstr::BinaryOp {
                dest: make_reg(2),
                op: crate::ir::BinaryOp::Add,
                left: make_reg(0),
                right: make_reg(1),
            },
        ];

        let mut module = IrModule::new("test");
        module.add_function(make_function(instrs));
        dce.eliminate(&mut module);

        let func = module.get_function(crate::ir::FunctionId::new(0)).unwrap();
        let block = func.get_block(BasicBlockId(0)).unwrap();

        // All instructions should be eliminated (r2 unused, then r0, r1 become unused)
        assert!(block.instructions.is_empty());
    }
}
