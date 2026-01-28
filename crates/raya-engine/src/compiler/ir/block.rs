//! Basic Blocks and Control Flow
//!
//! Basic blocks are sequences of instructions with a single entry point
//! and a single exit point (the terminator).

use super::instr::IrInstr;
use super::value::Register;

/// Basic block identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BasicBlockId(pub u32);

impl BasicBlockId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for BasicBlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bb{}", self.0)
    }
}

/// A basic block: sequence of instructions with single entry and exit
#[derive(Debug, Clone)]
pub struct BasicBlock {
    /// Unique identifier for this block
    pub id: BasicBlockId,
    /// Optional label for debugging
    pub label: Option<String>,
    /// Instructions in this block (excluding terminator)
    pub instructions: Vec<IrInstr>,
    /// How this block exits (must be set before code generation)
    pub terminator: Terminator,
}

impl BasicBlock {
    /// Create a new empty basic block
    pub fn new(id: BasicBlockId) -> Self {
        Self {
            id,
            label: None,
            instructions: Vec::new(),
            terminator: Terminator::Unreachable,
        }
    }

    /// Create a new basic block with a label
    pub fn with_label(id: BasicBlockId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: Some(label.into()),
            instructions: Vec::new(),
            terminator: Terminator::Unreachable,
        }
    }

    /// Add an instruction to this block
    pub fn add_instr(&mut self, instr: IrInstr) {
        self.instructions.push(instr);
    }

    /// Set the terminator for this block
    pub fn set_terminator(&mut self, term: Terminator) {
        self.terminator = term;
    }

    /// Get the successor blocks
    pub fn successors(&self) -> Vec<BasicBlockId> {
        self.terminator.successors()
    }

    /// Check if this block is terminated (not unreachable)
    pub fn is_terminated(&self) -> bool {
        !matches!(self.terminator, Terminator::Unreachable)
    }

    /// Get the number of instructions (excluding terminator)
    pub fn len(&self) -> usize {
        self.instructions.len()
    }

    /// Check if this block has no instructions
    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }
}

/// Control flow terminator (ends a basic block)
#[derive(Debug, Clone)]
pub enum Terminator {
    /// Unconditional jump to target block
    Jump(BasicBlockId),

    /// Conditional branch based on condition register (truthy/falsy)
    Branch {
        cond: Register,
        then_block: BasicBlockId,
        else_block: BasicBlockId,
    },

    /// Conditional branch based on null check
    BranchIfNull {
        value: Register,
        null_block: BasicBlockId,
        not_null_block: BasicBlockId,
    },

    /// Return from function with optional value
    Return(Option<Register>),

    /// Switch/match on a value
    Switch {
        value: Register,
        cases: Vec<(i32, BasicBlockId)>,
        default: BasicBlockId,
    },

    /// Unreachable (placeholder before terminator is set)
    Unreachable,

    /// Throw an exception
    Throw(Register),
}

impl Terminator {
    /// Get all successor blocks
    pub fn successors(&self) -> Vec<BasicBlockId> {
        match self {
            Terminator::Jump(target) => vec![*target],
            Terminator::Branch {
                then_block,
                else_block,
                ..
            } => vec![*then_block, *else_block],
            Terminator::BranchIfNull {
                null_block,
                not_null_block,
                ..
            } => vec![*null_block, *not_null_block],
            Terminator::Return(_) => vec![],
            Terminator::Switch { cases, default, .. } => {
                let mut succs: Vec<_> = cases.iter().map(|(_, block)| *block).collect();
                succs.push(*default);
                succs
            }
            Terminator::Unreachable => vec![],
            Terminator::Throw(_) => vec![],
        }
    }

    /// Check if this terminator can fall through to subsequent code
    pub fn can_fall_through(&self) -> bool {
        matches!(self, Terminator::Unreachable)
    }
}

impl std::fmt::Display for Terminator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Terminator::Jump(target) => write!(f, "jump {}", target),
            Terminator::Branch {
                cond,
                then_block,
                else_block,
            } => write!(f, "branch {} ? {} : {}", cond, then_block, else_block),
            Terminator::BranchIfNull {
                value,
                null_block,
                not_null_block,
            } => write!(
                f,
                "branch_if_null {} ? {} : {}",
                value, null_block, not_null_block
            ),
            Terminator::Return(None) => write!(f, "return"),
            Terminator::Return(Some(reg)) => write!(f, "return {}", reg),
            Terminator::Switch {
                value,
                cases,
                default,
            } => {
                write!(f, "switch {} [", value)?;
                for (i, (val, block)) in cases.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{} => {}", val, block)?;
                }
                write!(f, ", _ => {}]", default)
            }
            Terminator::Unreachable => write!(f, "unreachable"),
            Terminator::Throw(reg) => write!(f, "throw {}", reg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::ir::value::RegisterId;
    use crate::parser::TypeId;

    fn make_reg(id: u32) -> Register {
        Register::new(RegisterId::new(id), TypeId::new(0))
    }

    #[test]
    fn test_basic_block_new() {
        let block = BasicBlock::new(BasicBlockId(0));
        assert_eq!(block.id, BasicBlockId(0));
        assert!(block.instructions.is_empty());
        assert!(matches!(block.terminator, Terminator::Unreachable));
    }

    #[test]
    fn test_basic_block_with_label() {
        let block = BasicBlock::with_label(BasicBlockId(1), "entry");
        assert_eq!(block.label, Some("entry".to_string()));
    }

    #[test]
    fn test_terminator_successors() {
        let jump = Terminator::Jump(BasicBlockId(1));
        assert_eq!(jump.successors(), vec![BasicBlockId(1)]);

        let branch = Terminator::Branch {
            cond: make_reg(0),
            then_block: BasicBlockId(1),
            else_block: BasicBlockId(2),
        };
        assert_eq!(branch.successors(), vec![BasicBlockId(1), BasicBlockId(2)]);

        let ret = Terminator::Return(None);
        assert!(ret.successors().is_empty());
    }

    #[test]
    fn test_terminator_display() {
        let jump = Terminator::Jump(BasicBlockId(1));
        assert_eq!(format!("{}", jump), "jump bb1");

        let ret = Terminator::Return(None);
        assert_eq!(format!("{}", ret), "return");

        let ret_val = Terminator::Return(Some(make_reg(0)));
        assert_eq!(format!("{}", ret_val), "return r0:0");
    }
}
