//! Expression AST nodes
//!
//! This module defines all expression types in the Raya language, including:
//! - Literal expressions (numbers, strings, booleans, arrays, objects)
//! - Unary and binary operations
//! - Function calls and member access
//! - Arrow functions
//! - Async/await expressions

use super::*;
use crate::token::Span;

/// Expression (produces a value)
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    /// Integer literal: 42, 0xFF, 0b1010
    IntLiteral(IntLiteral),

    /// Float literal: 3.14, 1.0e10
    FloatLiteral(FloatLiteral),

    /// String literal: "hello"
    StringLiteral(StringLiteral),

    /// Template literal: `Hello, ${name}!`
    TemplateLiteral(TemplateLiteral),

    /// Boolean literal: true, false
    BooleanLiteral(BooleanLiteral),

    /// Null literal
    NullLiteral(Span),

    /// Identifier
    Identifier(Identifier),

    /// Array literal: [1, 2, 3]
    Array(ArrayExpression),

    /// Object literal: { x: 1, y: 2 }
    Object(ObjectExpression),

    /// Unary expression: !x, -y, ++z
    Unary(UnaryExpression),

    /// Binary expression: x + y, a * b
    Binary(BinaryExpression),

    /// Assignment: x = 42, y += 1
    Assignment(AssignmentExpression),

    /// Logical expression: x && y, a || b
    Logical(LogicalExpression),

    /// Ternary: x ? y : z
    Conditional(ConditionalExpression),

    /// Function call: foo(1, 2, 3)
    Call(CallExpression),

    /// Member access: obj.prop
    Member(MemberExpression),

    /// Index access: arr[0]
    Index(IndexExpression),

    /// New expression: new Point(1, 2)
    New(NewExpression),

    /// Arrow function: (x) => x + 1
    Arrow(ArrowFunction),

    /// Await expression: await promise
    Await(AwaitExpression),

    /// Typeof expression: typeof value
    Typeof(TypeofExpression),

    /// Parenthesized: (expr)
    Parenthesized(ParenthesizedExpression),
}

impl Expression {
    /// Get the span of this expression
    pub fn span(&self) -> &Span {
        match self {
            Expression::IntLiteral(e) => &e.span,
            Expression::FloatLiteral(e) => &e.span,
            Expression::StringLiteral(e) => &e.span,
            Expression::TemplateLiteral(e) => &e.span,
            Expression::BooleanLiteral(e) => &e.span,
            Expression::NullLiteral(span) => span,
            Expression::Identifier(e) => &e.span,
            Expression::Array(e) => &e.span,
            Expression::Object(e) => &e.span,
            Expression::Unary(e) => &e.span,
            Expression::Binary(e) => &e.span,
            Expression::Assignment(e) => &e.span,
            Expression::Logical(e) => &e.span,
            Expression::Conditional(e) => &e.span,
            Expression::Call(e) => &e.span,
            Expression::Member(e) => &e.span,
            Expression::Index(e) => &e.span,
            Expression::New(e) => &e.span,
            Expression::Arrow(e) => &e.span,
            Expression::Await(e) => &e.span,
            Expression::Typeof(e) => &e.span,
            Expression::Parenthesized(e) => &e.span,
        }
    }

    /// Check if this expression is a literal
    pub fn is_literal(&self) -> bool {
        matches!(
            self,
            Expression::IntLiteral(_)
                | Expression::FloatLiteral(_)
                | Expression::StringLiteral(_)
                | Expression::TemplateLiteral(_)
                | Expression::BooleanLiteral(_)
                | Expression::NullLiteral(_)
                | Expression::Array(_)
                | Expression::Object(_)
        )
    }

    /// Check if this expression is a simple identifier
    pub fn is_identifier(&self) -> bool {
        matches!(self, Expression::Identifier(_))
    }

    /// Check if this expression is a binary operation
    pub fn is_binary(&self) -> bool {
        matches!(self, Expression::Binary(_) | Expression::Logical(_))
    }
}

// ============================================================================
// Literal Expressions
// ============================================================================

/// Integer literal: 42, 0xFF, 0b1010
#[derive(Debug, Clone, PartialEq)]
pub struct IntLiteral {
    pub value: i64,
    pub span: Span,
}

/// Float literal: 3.14, 1.0e10
#[derive(Debug, Clone, PartialEq)]
pub struct FloatLiteral {
    pub value: f64,
    pub span: Span,
}

/// String literal: "hello"
#[derive(Debug, Clone, PartialEq)]
pub struct StringLiteral {
    pub value: String,
    pub span: Span,
}

/// Template literal: `Hello, ${name}!`
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateLiteral {
    pub parts: Vec<TemplatePart>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TemplatePart {
    String(String),
    Expression(Box<Expression>),
}

/// Boolean literal: true, false
#[derive(Debug, Clone, PartialEq)]
pub struct BooleanLiteral {
    pub value: bool,
    pub span: Span,
}

// ============================================================================
// Array and Object Expressions
// ============================================================================

/// Array expression: [1, 2, 3]
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayExpression {
    pub elements: Vec<Option<Expression>>,
    pub span: Span,
}

/// Object expression: { x: 1, y: 2 }
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectExpression {
    pub properties: Vec<ObjectProperty>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ObjectProperty {
    Property(Property),
    Spread(SpreadProperty),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub key: PropertyKey,
    pub value: Expression,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PropertyKey {
    Identifier(Identifier),
    StringLiteral(StringLiteral),
    IntLiteral(IntLiteral),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpreadProperty {
    pub argument: Expression,
    pub span: Span,
}

// ============================================================================
// Unary & Binary Expressions
// ============================================================================

/// Unary expression: !x, -y, ++z
#[derive(Debug, Clone, PartialEq)]
pub struct UnaryExpression {
    pub operator: UnaryOperator,
    pub operand: Box<Expression>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOperator {
    Plus,             // +x
    Minus,            // -x
    Not,              // !x
    BitwiseNot,       // ~x
    PrefixIncrement,  // ++x
    PrefixDecrement,  // --x
    PostfixIncrement, // x++
    PostfixDecrement, // x--
}

/// Binary expression: x + y, a * b
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryExpression {
    pub operator: BinaryOperator,
    pub left: Box<Expression>,
    pub right: Box<Expression>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOperator {
    // Arithmetic
    Add,      // +
    Subtract, // -
    Multiply, // *
    Divide,   // /
    Modulo,   // %
    Exponent, // **

    // Comparison
    Equal,          // ==
    NotEqual,       // !=
    StrictEqual,    // ===
    StrictNotEqual, // !==
    LessThan,       // <
    LessEqual,      // <=
    GreaterThan,    // >
    GreaterEqual,   // >=

    // Bitwise
    BitwiseAnd,         // &
    BitwiseOr,          // |
    BitwiseXor,         // ^
    LeftShift,          // <<
    RightShift,         // >>
    UnsignedRightShift, // >>>
}

/// Logical expression: x && y, a || b
#[derive(Debug, Clone, PartialEq)]
pub struct LogicalExpression {
    pub operator: LogicalOperator,
    pub left: Box<Expression>,
    pub right: Box<Expression>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOperator {
    And,               // &&
    Or,                // ||
    NullishCoalescing, // ??
}

/// Assignment expression: x = 42, y += 1
#[derive(Debug, Clone, PartialEq)]
pub struct AssignmentExpression {
    pub operator: AssignmentOperator,
    pub left: Box<Expression>,
    pub right: Box<Expression>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentOperator {
    Assign,                   // =
    AddAssign,                // +=
    SubAssign,                // -=
    MulAssign,                // *=
    DivAssign,                // /=
    ModAssign,                // %=
    AndAssign,                // &=
    OrAssign,                 // |=
    XorAssign,                // ^=
    LeftShiftAssign,          // <<=
    RightShiftAssign,         // >>=
    UnsignedRightShiftAssign, // >>>=
}

// ============================================================================
// Complex Expressions
// ============================================================================

/// Conditional (ternary): x ? y : z
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionalExpression {
    pub test: Box<Expression>,
    pub consequent: Box<Expression>,
    pub alternate: Box<Expression>,
    pub span: Span,
}

/// Function call: foo(1, 2, 3)
#[derive(Debug, Clone, PartialEq)]
pub struct CallExpression {
    pub callee: Box<Expression>,
    pub type_args: Option<Vec<TypeAnnotation>>,
    pub arguments: Vec<Expression>,
    pub span: Span,
}

/// Member access: obj.prop, obj?.prop
#[derive(Debug, Clone, PartialEq)]
pub struct MemberExpression {
    pub object: Box<Expression>,
    pub property: Identifier,
    pub optional: bool, // obj?.prop
    pub span: Span,
}

/// Index access: arr[0]
#[derive(Debug, Clone, PartialEq)]
pub struct IndexExpression {
    pub object: Box<Expression>,
    pub index: Box<Expression>,
    pub span: Span,
}

/// New expression: new Point(1, 2)
#[derive(Debug, Clone, PartialEq)]
pub struct NewExpression {
    pub callee: Box<Expression>,
    pub type_args: Option<Vec<TypeAnnotation>>,
    pub arguments: Vec<Expression>,
    pub span: Span,
}

/// Arrow function: (x) => x + 1
#[derive(Debug, Clone, PartialEq)]
pub struct ArrowFunction {
    pub params: Vec<Parameter>,
    pub return_type: Option<TypeAnnotation>,
    pub body: ArrowBody,
    pub is_async: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ArrowBody {
    Expression(Box<Expression>),
    Block(BlockStatement),
}

/// Await expression: await promise
#[derive(Debug, Clone, PartialEq)]
pub struct AwaitExpression {
    pub argument: Box<Expression>,
    pub span: Span,
}

/// Typeof expression: typeof value
#[derive(Debug, Clone, PartialEq)]
pub struct TypeofExpression {
    pub argument: Box<Expression>,
    pub span: Span,
}

/// Parenthesized expression: (expr)
#[derive(Debug, Clone, PartialEq)]
pub struct ParenthesizedExpression {
    pub expression: Box<Expression>,
    pub span: Span,
}
