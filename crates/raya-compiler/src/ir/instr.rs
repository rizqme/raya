//! IR Instructions
//!
//! Three-address code instructions for the IR.

use super::block::BasicBlockId;
use super::value::{IrValue, Register};
use raya_parser::TypeId;

/// Function identifier in the IR
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionId(pub u32);

impl FunctionId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for FunctionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "fn{}", self.0)
    }
}

/// Class identifier in the IR
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClassId(pub u32);

impl ClassId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for ClassId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "class{}", self.0)
    }
}

/// IR instruction (Three-Address Code)
#[derive(Debug, Clone)]
pub enum IrInstr {
    /// Assignment: dest = value
    Assign {
        dest: Register,
        value: IrValue,
    },

    /// Binary operation: dest = left op right
    BinaryOp {
        dest: Register,
        op: BinaryOp,
        left: Register,
        right: Register,
    },

    /// Unary operation: dest = op operand
    UnaryOp {
        dest: Register,
        op: UnaryOp,
        operand: Register,
    },

    /// Function call: dest = func(args)
    Call {
        dest: Option<Register>,
        func: FunctionId,
        args: Vec<Register>,
    },

    /// Method call: dest = object.method(args)
    CallMethod {
        dest: Option<Register>,
        object: Register,
        method: u16,
        args: Vec<Register>,
    },

    /// Load from local variable: dest = locals[index]
    LoadLocal {
        dest: Register,
        index: u16,
    },

    /// Store to local variable: locals[index] = value
    StoreLocal {
        index: u16,
        value: Register,
    },

    /// Load object field: dest = object.field
    LoadField {
        dest: Register,
        object: Register,
        field: u16,
    },

    /// Store object field: object.field = value
    StoreField {
        object: Register,
        field: u16,
        value: Register,
    },

    /// Load array element: dest = array[index]
    LoadElement {
        dest: Register,
        array: Register,
        index: Register,
    },

    /// Store array element: array[index] = value
    StoreElement {
        array: Register,
        index: Register,
        value: Register,
    },

    /// Create new object: dest = new class
    NewObject {
        dest: Register,
        class: ClassId,
    },

    /// Create new array: dest = new elem_ty[len]
    NewArray {
        dest: Register,
        len: Register,
        elem_ty: TypeId,
    },

    /// Array literal: dest = [elements...]
    ArrayLiteral {
        dest: Register,
        elements: Vec<Register>,
        elem_ty: TypeId,
    },

    /// Object literal: dest = { field: value, ... }
    ObjectLiteral {
        dest: Register,
        class: ClassId,
        fields: Vec<(u16, Register)>,
    },

    /// Get array length: dest = array.length
    ArrayLen {
        dest: Register,
        array: Register,
    },

    /// Typeof operation: dest = typeof operand
    Typeof {
        dest: Register,
        operand: Register,
    },

    /// Phi node (for SSA form - future use)
    Phi {
        dest: Register,
        sources: Vec<(BasicBlockId, Register)>,
    },
}

impl IrInstr {
    /// Get the destination register if this instruction produces a value
    pub fn dest(&self) -> Option<&Register> {
        match self {
            IrInstr::Assign { dest, .. }
            | IrInstr::BinaryOp { dest, .. }
            | IrInstr::UnaryOp { dest, .. }
            | IrInstr::LoadLocal { dest, .. }
            | IrInstr::LoadField { dest, .. }
            | IrInstr::LoadElement { dest, .. }
            | IrInstr::NewObject { dest, .. }
            | IrInstr::NewArray { dest, .. }
            | IrInstr::ArrayLiteral { dest, .. }
            | IrInstr::ObjectLiteral { dest, .. }
            | IrInstr::ArrayLen { dest, .. }
            | IrInstr::Typeof { dest, .. }
            | IrInstr::Phi { dest, .. } => Some(dest),
            IrInstr::Call { dest, .. } | IrInstr::CallMethod { dest, .. } => dest.as_ref(),
            IrInstr::StoreLocal { .. }
            | IrInstr::StoreField { .. }
            | IrInstr::StoreElement { .. } => None,
        }
    }

    /// Check if this instruction has side effects
    pub fn has_side_effects(&self) -> bool {
        matches!(
            self,
            IrInstr::Call { .. }
                | IrInstr::CallMethod { .. }
                | IrInstr::StoreLocal { .. }
                | IrInstr::StoreField { .. }
                | IrInstr::StoreElement { .. }
                | IrInstr::NewObject { .. }
                | IrInstr::NewArray { .. }
                | IrInstr::ArrayLiteral { .. }
                | IrInstr::ObjectLiteral { .. }
        )
    }
}

/// Binary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,

    // Comparison
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,

    // Logical
    And,
    Or,

    // Bitwise
    BitAnd,
    BitOr,
    BitXor,
    ShiftLeft,
    ShiftRight,
    UnsignedShiftRight,

    // String
    Concat,
}

impl BinaryOp {
    /// Check if this is an arithmetic operator
    pub fn is_arithmetic(&self) -> bool {
        matches!(
            self,
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod
        )
    }

    /// Check if this is a comparison operator
    pub fn is_comparison(&self) -> bool {
        matches!(
            self,
            BinaryOp::Equal
                | BinaryOp::NotEqual
                | BinaryOp::Less
                | BinaryOp::LessEqual
                | BinaryOp::Greater
                | BinaryOp::GreaterEqual
        )
    }

    /// Check if this is a logical operator
    pub fn is_logical(&self) -> bool {
        matches!(self, BinaryOp::And | BinaryOp::Or)
    }
}

impl std::fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Mod => "%",
            BinaryOp::Equal => "==",
            BinaryOp::NotEqual => "!=",
            BinaryOp::Less => "<",
            BinaryOp::LessEqual => "<=",
            BinaryOp::Greater => ">",
            BinaryOp::GreaterEqual => ">=",
            BinaryOp::And => "&&",
            BinaryOp::Or => "||",
            BinaryOp::BitAnd => "&",
            BinaryOp::BitOr => "|",
            BinaryOp::BitXor => "^",
            BinaryOp::ShiftLeft => "<<",
            BinaryOp::ShiftRight => ">>",
            BinaryOp::UnsignedShiftRight => ">>>",
            BinaryOp::Concat => "++",
        };
        write!(f, "{}", s)
    }
}

/// Unary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// Numeric negation (-)
    Neg,
    /// Logical not (!)
    Not,
    /// Bitwise not (~)
    BitNot,
}

impl std::fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "!",
            UnaryOp::BitNot => "~",
        };
        write!(f, "{}", s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_op_display() {
        assert_eq!(format!("{}", BinaryOp::Add), "+");
        assert_eq!(format!("{}", BinaryOp::Equal), "==");
        assert_eq!(format!("{}", BinaryOp::And), "&&");
    }

    #[test]
    fn test_binary_op_categories() {
        assert!(BinaryOp::Add.is_arithmetic());
        assert!(!BinaryOp::Add.is_comparison());

        assert!(BinaryOp::Equal.is_comparison());
        assert!(!BinaryOp::Equal.is_arithmetic());

        assert!(BinaryOp::And.is_logical());
        assert!(!BinaryOp::Add.is_logical());
    }

    #[test]
    fn test_unary_op_display() {
        assert_eq!(format!("{}", UnaryOp::Neg), "-");
        assert_eq!(format!("{}", UnaryOp::Not), "!");
    }
}
