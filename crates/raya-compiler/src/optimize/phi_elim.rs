//! PHI Node Elimination
//!
//! This pass eliminates PHI nodes by inserting copy instructions at the end
//! of predecessor blocks. This is necessary because PHI nodes are only valid
//! in SSA form, but the bytecode generator expects regular three-address code.
//!
//! ## Algorithm
//!
//! For each PHI instruction `dest = phi [(bb1, r1), (bb2, r2), ...]`:
//! 1. For each source `(predecessor_block, source_register)`:
//!    - Insert `dest = source_register` at the end of `predecessor_block`
//!      (just before its terminator)
//! 2. Remove the PHI instruction
//!
//! ## Example
//!
//! Before:
//! ```text
//! bb0:
//!   r0 = true
//!   branch r0 ? bb1 : bb2
//!
//! bb1:
//!   r1 = 10
//!   jump bb3
//!
//! bb2:
//!   r2 = 20
//!   jump bb3
//!
//! bb3:
//!   r3 = phi [(bb1, r1), (bb2, r2)]
//!   return r3
//! ```
//!
//! After:
//! ```text
//! bb0:
//!   r0 = true
//!   branch r0 ? bb1 : bb2
//!
//! bb1:
//!   r1 = 10
//!   r3 = r1        <- inserted copy
//!   jump bb3
//!
//! bb2:
//!   r2 = 20
//!   r3 = r2        <- inserted copy
//!   jump bb3
//!
//! bb3:
//!   return r3      <- phi removed
//! ```

use crate::ir::{BasicBlockId, IrFunction, IrInstr, IrModule, IrValue};
use rustc_hash::FxHashMap;

/// PHI elimination pass
pub struct PhiEliminator;

impl PhiEliminator {
    /// Create a new PHI eliminator
    pub fn new() -> Self {
        Self
    }

    /// Eliminate all PHI nodes in the module
    pub fn eliminate(&self, module: &mut IrModule) {
        for func in &mut module.functions {
            self.eliminate_in_function(func);
        }
    }

    /// Eliminate PHI nodes in a single function
    fn eliminate_in_function(&self, func: &mut IrFunction) {
        // First, collect all PHI instructions and their copy targets
        // Map from predecessor block ID to list of (dest, source) copies to insert
        let mut copies_to_insert: FxHashMap<BasicBlockId, Vec<(crate::ir::Register, crate::ir::Register)>> =
            FxHashMap::default();

        // Also track which blocks have PHIs to remove
        let mut blocks_with_phis: Vec<(BasicBlockId, Vec<usize>)> = Vec::new();

        // First pass: collect PHI information
        for block in func.blocks() {
            let mut phi_indices = Vec::new();

            for (idx, instr) in block.instructions.iter().enumerate() {
                if let IrInstr::Phi { dest, sources } = instr {
                    phi_indices.push(idx);

                    // For each source, schedule a copy in the predecessor block
                    for (pred_block_id, source_reg) in sources {
                        copies_to_insert
                            .entry(*pred_block_id)
                            .or_default()
                            .push((dest.clone(), source_reg.clone()));
                    }
                }
            }

            if !phi_indices.is_empty() {
                blocks_with_phis.push((block.id, phi_indices));
            }
        }

        // Second pass: insert copy instructions at the end of predecessor blocks
        for (pred_block_id, copies) in copies_to_insert {
            if let Some(block) = func.get_block_mut(pred_block_id) {
                // Insert copies before the terminator
                for (dest, source) in copies {
                    block.instructions.push(IrInstr::Assign {
                        dest,
                        value: IrValue::Register(source),
                    });
                }
            }
        }

        // Third pass: remove PHI instructions from blocks
        // We need to do this in reverse order to maintain correct indices
        for (block_id, phi_indices) in blocks_with_phis {
            if let Some(block) = func.get_block_mut(block_id) {
                // Remove in reverse order to keep indices valid
                for idx in phi_indices.into_iter().rev() {
                    block.instructions.remove(idx);
                }
            }
        }
    }
}

impl Default for PhiEliminator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BasicBlock, IrInstr, IrModule, IrFunction, IrValue, IrConstant, Terminator};
    use crate::ir::value::{Register, RegisterId};
    use raya_parser::TypeId;

    fn make_reg(id: u32, ty: u32) -> Register {
        Register::new(RegisterId::new(id), TypeId::new(ty))
    }

    #[test]
    fn test_phi_elimination_simple() {
        let mut module = IrModule::new("test");

        // Create function with a simple PHI
        let mut func = IrFunction::new("test_fn", vec![], TypeId::new(0));

        // bb0: branch to bb1 or bb2
        let mut bb0 = BasicBlock::new(BasicBlockId(0));
        let cond_reg = make_reg(0, 4); // bool
        bb0.add_instr(IrInstr::Assign {
            dest: cond_reg.clone(),
            value: IrValue::Constant(IrConstant::Boolean(true)),
        });
        bb0.set_terminator(Terminator::Branch {
            cond: cond_reg,
            then_block: BasicBlockId(1),
            else_block: BasicBlockId(2),
        });
        func.add_block(bb0);

        // bb1: produces r1
        let mut bb1 = BasicBlock::new(BasicBlockId(1));
        let r1 = make_reg(1, 1); // int
        bb1.add_instr(IrInstr::Assign {
            dest: r1.clone(),
            value: IrValue::Constant(IrConstant::I32(10)),
        });
        bb1.set_terminator(Terminator::Jump(BasicBlockId(3)));
        func.add_block(bb1);

        // bb2: produces r2
        let mut bb2 = BasicBlock::new(BasicBlockId(2));
        let r2 = make_reg(2, 1); // int
        bb2.add_instr(IrInstr::Assign {
            dest: r2.clone(),
            value: IrValue::Constant(IrConstant::I32(20)),
        });
        bb2.set_terminator(Terminator::Jump(BasicBlockId(3)));
        func.add_block(bb2);

        // bb3: PHI node merging r1 and r2
        let mut bb3 = BasicBlock::new(BasicBlockId(3));
        let r3 = make_reg(3, 1); // int
        bb3.add_instr(IrInstr::Phi {
            dest: r3.clone(),
            sources: vec![(BasicBlockId(1), r1.clone()), (BasicBlockId(2), r2.clone())],
        });
        bb3.set_terminator(Terminator::Return(Some(r3.clone())));
        func.add_block(bb3);

        // Save IDs for later verification
        let r1_id = r1.id;
        let r2_id = r2.id;
        let r3_id = r3.id;

        module.add_function(func);

        // Run PHI elimination
        let eliminator = PhiEliminator::new();
        eliminator.eliminate(&mut module);

        // Verify: bb3 should have no PHI instructions
        let func = &module.functions[0];
        let bb3 = func.get_block(BasicBlockId(3)).unwrap();
        assert!(
            !bb3.instructions.iter().any(|i| matches!(i, IrInstr::Phi { .. })),
            "PHI should be eliminated"
        );

        // Verify: bb1 should have a copy instruction at the end
        let bb1 = func.get_block(BasicBlockId(1)).unwrap();
        assert!(bb1.instructions.len() >= 2, "bb1 should have copy inserted");
        let last_instr = bb1.instructions.last().unwrap();
        assert!(
            matches!(last_instr, IrInstr::Assign { dest, value: IrValue::Register(src) }
                if dest.id == r3_id && src.id == r1_id),
            "bb1 should have copy from r1 to r3"
        );

        // Verify: bb2 should have a copy instruction at the end
        let bb2 = func.get_block(BasicBlockId(2)).unwrap();
        assert!(bb2.instructions.len() >= 2, "bb2 should have copy inserted");
        let last_instr = bb2.instructions.last().unwrap();
        assert!(
            matches!(last_instr, IrInstr::Assign { dest, value: IrValue::Register(src) }
                if dest.id == r3_id && src.id == r2_id),
            "bb2 should have copy from r2 to r3"
        );
    }

    #[test]
    fn test_no_phis_unchanged() {
        let mut module = IrModule::new("test");

        let mut func = IrFunction::new("test_fn", vec![], TypeId::new(1));
        let mut bb0 = BasicBlock::new(BasicBlockId(0));
        let r0 = make_reg(0, 1);
        bb0.add_instr(IrInstr::Assign {
            dest: r0.clone(),
            value: IrValue::Constant(IrConstant::I32(42)),
        });
        bb0.set_terminator(Terminator::Return(Some(r0)));
        func.add_block(bb0);

        module.add_function(func);

        // Run PHI elimination on a module with no PHIs
        let eliminator = PhiEliminator::new();
        eliminator.eliminate(&mut module);

        // Should be unchanged
        let func = &module.functions[0];
        let bb0 = func.get_block(BasicBlockId(0)).unwrap();
        assert_eq!(bb0.instructions.len(), 1);
    }
}
