//! Constant Folding Optimization
//!
//! Evaluates constant expressions at compile time.

use crate::compiler::ir::{BasicBlock, BinaryOp, IrConstant, IrFunction, IrInstr, IrModule, IrValue, Register, RegisterId, UnaryOp};
use rustc_hash::FxHashMap;

/// Constant folding optimizer
pub struct ConstantFolder {
    /// Map from register ID to known constant value
    constants: FxHashMap<RegisterId, IrConstant>,
}

impl ConstantFolder {
    /// Create a new constant folder
    pub fn new() -> Self {
        Self {
            constants: FxHashMap::default(),
        }
    }

    /// Fold constants in an entire module
    pub fn fold(&self, module: &mut IrModule) {
        for func in &mut module.functions {
            self.fold_function(func);
        }
    }

    /// Fold constants in a function
    ///
    /// Each block gets its own fresh constants map. We do NOT propagate
    /// constants across block boundaries because that's unsound in the
    /// presence of loops (back-edges can redefine registers).
    fn fold_function(&self, func: &mut IrFunction) {
        for block in &mut func.blocks {
            let mut constants = FxHashMap::default();
            self.fold_block(block, &mut constants);
        }
    }

    /// Fold constants in a basic block
    fn fold_block(&self, block: &mut BasicBlock, constants: &mut FxHashMap<RegisterId, IrConstant>) {
        let mut new_instrs = Vec::with_capacity(block.instructions.len());

        for instr in &block.instructions {
            match instr {
                IrInstr::Assign { dest, value } => {
                    // Track constant assignments
                    if let IrValue::Constant(c) = value {
                        constants.insert(dest.id, c.clone());
                    }
                    new_instrs.push(instr.clone());
                }

                IrInstr::BinaryOp { dest, op, left, right } => {
                    // Try to fold binary operations with constant operands
                    let left_const = constants.get(&left.id).cloned();
                    let right_const = constants.get(&right.id).cloned();

                    if let (Some(l), Some(r)) = (left_const, right_const) {
                        if let Some(result) = self.eval_binary(*op, &l, &r) {
                            // Folded to constant
                            constants.insert(dest.id, result.clone());
                            new_instrs.push(IrInstr::Assign {
                                dest: dest.clone(),
                                value: IrValue::Constant(result),
                            });
                            continue;
                        }
                    }
                    new_instrs.push(instr.clone());
                }

                IrInstr::UnaryOp { dest, op, operand } => {
                    // Try to fold unary operations with constant operand
                    if let Some(c) = constants.get(&operand.id).cloned() {
                        if let Some(result) = self.eval_unary(*op, &c) {
                            constants.insert(dest.id, result.clone());
                            new_instrs.push(IrInstr::Assign {
                                dest: dest.clone(),
                                value: IrValue::Constant(result),
                            });
                            continue;
                        }
                    }
                    new_instrs.push(instr.clone());
                }

                // All other instructions pass through
                _ => {
                    // Clear constant info for redefined registers
                    if let Some(dest) = instr.dest() {
                        constants.remove(&dest.id);
                    }
                    new_instrs.push(instr.clone());
                }
            }
        }

        block.instructions = new_instrs;
    }

    /// Evaluate a binary operation on constants
    fn eval_binary(&self, op: BinaryOp, left: &IrConstant, right: &IrConstant) -> Option<IrConstant> {
        match (op, left, right) {
            // Integer arithmetic
            (BinaryOp::Add, IrConstant::I32(a), IrConstant::I32(b)) => {
                Some(IrConstant::I32(a.wrapping_add(*b)))
            }
            (BinaryOp::Sub, IrConstant::I32(a), IrConstant::I32(b)) => {
                Some(IrConstant::I32(a.wrapping_sub(*b)))
            }
            (BinaryOp::Mul, IrConstant::I32(a), IrConstant::I32(b)) => {
                Some(IrConstant::I32(a.wrapping_mul(*b)))
            }
            (BinaryOp::Div, IrConstant::I32(a), IrConstant::I32(b)) if *b != 0 => {
                Some(IrConstant::I32(a / b))
            }
            (BinaryOp::Mod, IrConstant::I32(a), IrConstant::I32(b)) if *b != 0 => {
                Some(IrConstant::I32(a % b))
            }
            (BinaryOp::Pow, IrConstant::I32(a), IrConstant::I32(b)) if *b >= 0 => {
                Some(IrConstant::I32(a.wrapping_pow(*b as u32)))
            }
            (BinaryOp::Pow, IrConstant::I32(a), IrConstant::I32(b)) if *b < 0 => {
                // Negative exponent with integer base returns float
                Some(IrConstant::F64((*a as f64).powi(*b)))
            }

            // Float arithmetic
            (BinaryOp::Add, IrConstant::F64(a), IrConstant::F64(b)) => {
                Some(IrConstant::F64(a + b))
            }
            (BinaryOp::Sub, IrConstant::F64(a), IrConstant::F64(b)) => {
                Some(IrConstant::F64(a - b))
            }
            (BinaryOp::Mul, IrConstant::F64(a), IrConstant::F64(b)) => {
                Some(IrConstant::F64(a * b))
            }
            (BinaryOp::Div, IrConstant::F64(a), IrConstant::F64(b)) => {
                Some(IrConstant::F64(a / b))
            }
            (BinaryOp::Pow, IrConstant::F64(a), IrConstant::F64(b)) => {
                Some(IrConstant::F64(a.powf(*b)))
            }

            // String addition (concatenation)
            (BinaryOp::Add, IrConstant::String(a), IrConstant::String(b)) => {
                Some(IrConstant::String(format!("{}{}", a, b)))
            }

            // Integer comparisons
            (BinaryOp::Equal, IrConstant::I32(a), IrConstant::I32(b)) => {
                Some(IrConstant::Boolean(a == b))
            }
            (BinaryOp::NotEqual, IrConstant::I32(a), IrConstant::I32(b)) => {
                Some(IrConstant::Boolean(a != b))
            }
            (BinaryOp::Less, IrConstant::I32(a), IrConstant::I32(b)) => {
                Some(IrConstant::Boolean(a < b))
            }
            (BinaryOp::LessEqual, IrConstant::I32(a), IrConstant::I32(b)) => {
                Some(IrConstant::Boolean(a <= b))
            }
            (BinaryOp::Greater, IrConstant::I32(a), IrConstant::I32(b)) => {
                Some(IrConstant::Boolean(a > b))
            }
            (BinaryOp::GreaterEqual, IrConstant::I32(a), IrConstant::I32(b)) => {
                Some(IrConstant::Boolean(a >= b))
            }

            // Float comparisons
            (BinaryOp::Equal, IrConstant::F64(a), IrConstant::F64(b)) => {
                Some(IrConstant::Boolean((a - b).abs() < f64::EPSILON))
            }
            (BinaryOp::Less, IrConstant::F64(a), IrConstant::F64(b)) => {
                Some(IrConstant::Boolean(a < b))
            }
            (BinaryOp::LessEqual, IrConstant::F64(a), IrConstant::F64(b)) => {
                Some(IrConstant::Boolean(a <= b))
            }
            (BinaryOp::Greater, IrConstant::F64(a), IrConstant::F64(b)) => {
                Some(IrConstant::Boolean(a > b))
            }
            (BinaryOp::GreaterEqual, IrConstant::F64(a), IrConstant::F64(b)) => {
                Some(IrConstant::Boolean(a >= b))
            }

            // Boolean operations
            (BinaryOp::And, IrConstant::Boolean(a), IrConstant::Boolean(b)) => {
                Some(IrConstant::Boolean(*a && *b))
            }
            (BinaryOp::Or, IrConstant::Boolean(a), IrConstant::Boolean(b)) => {
                Some(IrConstant::Boolean(*a || *b))
            }

            // Bitwise operations
            (BinaryOp::BitAnd, IrConstant::I32(a), IrConstant::I32(b)) => {
                Some(IrConstant::I32(a & b))
            }
            (BinaryOp::BitOr, IrConstant::I32(a), IrConstant::I32(b)) => {
                Some(IrConstant::I32(a | b))
            }
            (BinaryOp::BitXor, IrConstant::I32(a), IrConstant::I32(b)) => {
                Some(IrConstant::I32(a ^ b))
            }
            (BinaryOp::ShiftLeft, IrConstant::I32(a), IrConstant::I32(b)) => {
                Some(IrConstant::I32(a << (b & 31)))
            }
            (BinaryOp::ShiftRight, IrConstant::I32(a), IrConstant::I32(b)) => {
                Some(IrConstant::I32(a >> (b & 31)))
            }

            // String concatenation
            (BinaryOp::Concat, IrConstant::String(a), IrConstant::String(b)) => {
                Some(IrConstant::String(format!("{}{}", a, b)))
            }

            _ => None,
        }
    }

    /// Evaluate a unary operation on a constant
    fn eval_unary(&self, op: UnaryOp, operand: &IrConstant) -> Option<IrConstant> {
        match (op, operand) {
            (UnaryOp::Neg, IrConstant::I32(v)) => Some(IrConstant::I32(-v)),
            (UnaryOp::Neg, IrConstant::F64(v)) => Some(IrConstant::F64(-v)),
            (UnaryOp::Not, IrConstant::Boolean(v)) => Some(IrConstant::Boolean(!v)),
            (UnaryOp::BitNot, IrConstant::I32(v)) => Some(IrConstant::I32(!v)),
            _ => None,
        }
    }
}

impl Default for ConstantFolder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::ir::{BasicBlock, BasicBlockId, IrFunction, IrModule, Register, Terminator};
    use crate::parser::TypeId;

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
    fn test_fold_integer_add() {
        let folder = ConstantFolder::new();

        let instrs = vec![
            IrInstr::Assign {
                dest: make_reg(0),
                value: IrValue::Constant(IrConstant::I32(10)),
            },
            IrInstr::Assign {
                dest: make_reg(1),
                value: IrValue::Constant(IrConstant::I32(32)),
            },
            IrInstr::BinaryOp {
                dest: make_reg(2),
                op: BinaryOp::Add,
                left: make_reg(0),
                right: make_reg(1),
            },
        ];

        let mut module = IrModule::new("test");
        module.add_function(make_function(instrs));
        folder.fold(&mut module);

        // The binary op should be folded to a constant assign
        let func = module.get_function(crate::ir::FunctionId::new(0)).unwrap();
        let block = func.get_block(BasicBlockId(0)).unwrap();

        // Last instruction should be an assign of 42
        if let IrInstr::Assign { value: IrValue::Constant(IrConstant::I32(42)), .. } =
            &block.instructions[2]
        {
            // Success
        } else {
            panic!("Expected folded constant 42");
        }
    }

    #[test]
    fn test_fold_comparison() {
        let folder = ConstantFolder::new();

        let instrs = vec![
            IrInstr::Assign {
                dest: make_reg(0),
                value: IrValue::Constant(IrConstant::I32(5)),
            },
            IrInstr::Assign {
                dest: make_reg(1),
                value: IrValue::Constant(IrConstant::I32(10)),
            },
            IrInstr::BinaryOp {
                dest: make_reg(2),
                op: BinaryOp::Less,
                left: make_reg(0),
                right: make_reg(1),
            },
        ];

        let mut module = IrModule::new("test");
        module.add_function(make_function(instrs));
        folder.fold(&mut module);

        let func = module.get_function(crate::ir::FunctionId::new(0)).unwrap();
        let block = func.get_block(BasicBlockId(0)).unwrap();

        // Should fold to true
        if let IrInstr::Assign {
            value: IrValue::Constant(IrConstant::Boolean(true)),
            ..
        } = &block.instructions[2]
        {
            // Success
        } else {
            panic!("Expected folded constant true");
        }
    }

    #[test]
    fn test_fold_unary_neg() {
        let folder = ConstantFolder::new();

        let instrs = vec![
            IrInstr::Assign {
                dest: make_reg(0),
                value: IrValue::Constant(IrConstant::I32(42)),
            },
            IrInstr::UnaryOp {
                dest: make_reg(1),
                op: UnaryOp::Neg,
                operand: make_reg(0),
            },
        ];

        let mut module = IrModule::new("test");
        module.add_function(make_function(instrs));
        folder.fold(&mut module);

        let func = module.get_function(crate::ir::FunctionId::new(0)).unwrap();
        let block = func.get_block(BasicBlockId(0)).unwrap();

        // Should fold to -42
        if let IrInstr::Assign {
            value: IrValue::Constant(IrConstant::I32(-42)),
            ..
        } = &block.instructions[1]
        {
            // Success
        } else {
            panic!("Expected folded constant -42");
        }
    }
}
