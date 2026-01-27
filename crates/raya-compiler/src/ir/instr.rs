//! IR Instructions
//!
//! Three-address code instructions for the IR.

use super::block::BasicBlockId;
use super::value::{IrValue, Register, ValueOrigin};
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

    /// Pop from stack to local variable (for catch parameters)
    /// The VM pushes the exception value before jumping to catch block
    PopToLocal {
        index: u16,
    },

    /// Load from global variable (for static fields): dest = globals[index]
    LoadGlobal {
        dest: Register,
        index: u16,
    },

    /// Store to global variable (for static fields): globals[index] = value
    StoreGlobal {
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

    /// Get string length: dest = string.length
    StringLen {
        dest: Register,
        string: Register,
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

    /// Create a closure: dest = closure(func, captures)
    MakeClosure {
        dest: Register,
        func: FunctionId,
        captures: Vec<Register>,
    },

    /// Load a captured variable: dest = captured[index]
    LoadCaptured {
        dest: Register,
        index: u16,
    },

    /// Store to a captured variable: captured[index] = value
    StoreCaptured {
        index: u16,
        value: Register,
    },

    /// Set a closure's capture: closure.captures[index] = value
    /// Used for recursive closures where the closure captures itself
    SetClosureCapture {
        closure: Register,
        index: u16,
        value: Register,
    },

    /// Allocate a new RefCell: dest = RefCell(initial_value)
    /// RefCell is used for capture-by-reference semantics
    NewRefCell {
        dest: Register,
        initial_value: Register,
    },

    /// Load value from RefCell: dest = refcell.value
    LoadRefCell {
        dest: Register,
        refcell: Register,
    },

    /// Store value to RefCell: refcell.value = value
    StoreRefCell {
        refcell: Register,
        value: Register,
    },

    /// Call a closure: dest = closure(args)
    CallClosure {
        dest: Option<Register>,
        closure: Register,
        args: Vec<Register>,
    },

    /// Optimized string comparison
    /// This instruction encodes the optimization decision made during IR generation
    StringCompare {
        dest: Register,
        left: Register,
        right: Register,
        /// Comparison mode determining which opcode to emit
        mode: StringCompareMode,
        /// Whether this is an equality (==) or inequality (!=) check
        negate: bool,
    },

    /// Convert value to string: dest = String(operand)
    ToString {
        dest: Register,
        operand: Register,
    },

    /// Spawn a new task: dest = spawn func(args)
    /// Creates a new green thread (Task) that executes the function
    Spawn {
        dest: Register,
        func: FunctionId,
        args: Vec<Register>,
    },

    /// Spawn a closure as a new task: dest = spawn closure(args)
    SpawnClosure {
        dest: Register,
        closure: Register,
        args: Vec<Register>,
    },

    /// Await a task: dest = await task
    /// Suspends current task until the awaited task completes
    Await {
        dest: Register,
        task: Register,
    },

    /// Await multiple tasks: dest = await [tasks]
    /// Suspends current task until all tasks complete, returns array of results
    AwaitAll {
        dest: Register,
        tasks: Register,
    },

    /// Sleep for a duration in milliseconds
    /// Suspends the current task for the specified duration
    Sleep {
        duration_ms: Register,
    },

    /// Yield execution to the scheduler
    /// Allows other tasks to run
    Yield,

    /// Set up exception handler for try block
    /// catch_block: BasicBlockId to jump to on exception (receives exception value)
    /// finally_block: Optional BasicBlockId for finally clause
    SetupTry {
        catch_block: BasicBlockId,
        finally_block: Option<BasicBlockId>,
    },

    /// End of try block - removes exception handler
    EndTry,
}

impl IrInstr {
    /// Get the destination register if this instruction produces a value
    pub fn dest(&self) -> Option<&Register> {
        match self {
            IrInstr::Assign { dest, .. }
            | IrInstr::BinaryOp { dest, .. }
            | IrInstr::UnaryOp { dest, .. }
            | IrInstr::LoadLocal { dest, .. }
            | IrInstr::LoadGlobal { dest, .. }
            | IrInstr::LoadField { dest, .. }
            | IrInstr::LoadElement { dest, .. }
            | IrInstr::NewObject { dest, .. }
            | IrInstr::NewArray { dest, .. }
            | IrInstr::ArrayLiteral { dest, .. }
            | IrInstr::ObjectLiteral { dest, .. }
            | IrInstr::ArrayLen { dest, .. }
            | IrInstr::StringLen { dest, .. }
            | IrInstr::Typeof { dest, .. }
            | IrInstr::Phi { dest, .. }
            | IrInstr::MakeClosure { dest, .. }
            | IrInstr::LoadCaptured { dest, .. }
            | IrInstr::NewRefCell { dest, .. }
            | IrInstr::LoadRefCell { dest, .. }
            | IrInstr::StringCompare { dest, .. }
            | IrInstr::ToString { dest, .. }
            | IrInstr::Spawn { dest, .. }
            | IrInstr::SpawnClosure { dest, .. }
            | IrInstr::Await { dest, .. }
            | IrInstr::AwaitAll { dest, .. } => Some(dest),
            IrInstr::Call { dest, .. }
            | IrInstr::CallMethod { dest, .. }
            | IrInstr::CallClosure { dest, .. } => dest.as_ref(),
            IrInstr::StoreLocal { .. }
            | IrInstr::StoreGlobal { .. }
            | IrInstr::StoreField { .. }
            | IrInstr::StoreElement { .. }
            | IrInstr::StoreCaptured { .. }
            | IrInstr::SetClosureCapture { .. }
            | IrInstr::StoreRefCell { .. }
            | IrInstr::PopToLocal { .. }
            | IrInstr::SetupTry { .. }
            | IrInstr::EndTry
            | IrInstr::Sleep { .. }
            | IrInstr::Yield => None,
        }
    }

    /// Check if this instruction has side effects
    pub fn has_side_effects(&self) -> bool {
        matches!(
            self,
            IrInstr::Call { .. }
                | IrInstr::CallMethod { .. }
                | IrInstr::CallClosure { .. }
                | IrInstr::StoreLocal { .. }
                | IrInstr::PopToLocal { .. }
                | IrInstr::StoreGlobal { .. }
                | IrInstr::StoreField { .. }
                | IrInstr::StoreElement { .. }
                | IrInstr::StoreCaptured { .. }
                | IrInstr::SetClosureCapture { .. }
                | IrInstr::NewRefCell { .. }
                | IrInstr::StoreRefCell { .. }
                | IrInstr::NewObject { .. }
                | IrInstr::NewArray { .. }
                | IrInstr::ArrayLiteral { .. }
                | IrInstr::ObjectLiteral { .. }
                | IrInstr::MakeClosure { .. }
                | IrInstr::Spawn { .. }
                | IrInstr::SpawnClosure { .. }
                | IrInstr::Await { .. }
                | IrInstr::AwaitAll { .. }
                | IrInstr::SetupTry { .. }
                | IrInstr::EndTry
                | IrInstr::Sleep { .. }
                | IrInstr::Yield
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
    Pow,

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
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod | BinaryOp::Pow
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
            BinaryOp::Pow => "**",
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

/// String comparison mode
///
/// Determines which opcode to use for string comparison based on type analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringCompareMode {
    /// Use IEQ/INE - O(1) index comparison
    /// Used when both operands are known constants or have string literal union types
    Index,
    /// Use SEQ/SNE - O(n) full string comparison
    /// Used when operands might be computed strings
    Full,
}

impl StringCompareMode {
    /// Create mode from value origins
    pub fn from_origins(left: ValueOrigin, right: ValueOrigin) -> Self {
        if left.allows_index_comparison() && right.allows_index_comparison() {
            StringCompareMode::Index
        } else {
            StringCompareMode::Full
        }
    }
}

impl std::fmt::Display for StringCompareMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StringCompareMode::Index => write!(f, "index"),
            StringCompareMode::Full => write!(f, "full"),
        }
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

    #[test]
    fn test_string_compare_mode_display() {
        assert_eq!(format!("{}", StringCompareMode::Index), "index");
        assert_eq!(format!("{}", StringCompareMode::Full), "full");
    }

    #[test]
    fn test_string_compare_mode_from_origins() {
        // Both constants -> Index mode
        assert_eq!(
            StringCompareMode::from_origins(ValueOrigin::Constant(0), ValueOrigin::Constant(1)),
            StringCompareMode::Index
        );

        // Both literal unions -> Index mode
        assert_eq!(
            StringCompareMode::from_origins(ValueOrigin::LiteralUnion, ValueOrigin::LiteralUnion),
            StringCompareMode::Index
        );

        // Mixed constant and literal union -> Index mode
        assert_eq!(
            StringCompareMode::from_origins(ValueOrigin::Constant(0), ValueOrigin::LiteralUnion),
            StringCompareMode::Index
        );

        // One computed -> Full mode
        assert_eq!(
            StringCompareMode::from_origins(ValueOrigin::Computed, ValueOrigin::Constant(0)),
            StringCompareMode::Full
        );

        // Both computed -> Full mode
        assert_eq!(
            StringCompareMode::from_origins(ValueOrigin::Computed, ValueOrigin::Computed),
            StringCompareMode::Full
        );
    }
}
