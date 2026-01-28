//! Expression AST nodes
//!
//! This module defines all expression types in the Raya language, including:
//! - Literal expressions (numbers, strings, booleans, arrays, objects)
//! - Unary and binary operations
//! - Function calls and member access
//! - Arrow functions
//! - Async/await expressions

use super::*;
use crate::parser::token::Span;

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

    /// Async call: async foo() - wraps any call in a Task
    AsyncCall(AsyncCallExpression),

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

    /// JSX element: <div>content</div>
    JsxElement(JsxElement),

    /// JSX fragment: <>content</>
    JsxFragment(JsxFragment),

    /// This expression: this
    This(Span),

    /// Super expression: super (for parent class access)
    Super(Span),

    /// InstanceOf expression: expr instanceof ClassName
    InstanceOf(InstanceOfExpression),

    /// Type cast expression: expr as TypeName
    TypeCast(TypeCastExpression),
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
            Expression::AsyncCall(e) => &e.span,
            Expression::Member(e) => &e.span,
            Expression::Index(e) => &e.span,
            Expression::New(e) => &e.span,
            Expression::Arrow(e) => &e.span,
            Expression::Await(e) => &e.span,
            Expression::Typeof(e) => &e.span,
            Expression::Parenthesized(e) => &e.span,
            Expression::JsxElement(e) => &e.span,
            Expression::JsxFragment(e) => &e.span,
            Expression::This(span) => span,
            Expression::Super(span) => span,
            Expression::InstanceOf(e) => &e.span,
            Expression::TypeCast(e) => &e.span,
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
    pub value: crate::parser::interner::Symbol,
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
    String(crate::parser::interner::Symbol),
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

/// Array expression: [1, 2, 3], [...arr1, ...arr2]
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayExpression {
    pub elements: Vec<Option<ArrayElement>>,
    pub span: Span,
}

/// Array element (expression or spread)
#[derive(Debug, Clone, PartialEq)]
pub enum ArrayElement {
    /// Regular expression: 42
    Expression(Expression),
    /// Spread element: ...arr
    Spread(Expression),
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
    /// Computed property name: [expr]
    Computed(Expression),
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

/// Async call: async foo() - wraps any function call in a Task
/// This converts a non-async function call into an async Task.
/// If the function is already async, this has no additional effect.
#[derive(Debug, Clone, PartialEq)]
pub struct AsyncCallExpression {
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

// ============================================================================
// JSX / TSX Support
// ============================================================================

/// JSX element: <div className="foo">Hello</div>
#[derive(Debug, Clone, PartialEq)]
pub struct JsxElement {
    /// Opening tag with name and attributes
    pub opening: JsxOpeningElement,

    /// Children elements, text, or expressions
    pub children: Vec<JsxChild>,

    /// Optional closing tag (None for self-closing)
    pub closing: Option<JsxClosingElement>,

    pub span: Span,
}

/// JSX opening tag: <div className="foo">
#[derive(Debug, Clone, PartialEq)]
pub struct JsxOpeningElement {
    /// Element name (div, Button, etc.)
    pub name: JsxElementName,

    /// Attributes
    pub attributes: Vec<JsxAttribute>,

    /// Self-closing? <div />
    pub self_closing: bool,

    pub span: Span,
}

/// JSX closing tag: </div>
#[derive(Debug, Clone, PartialEq)]
pub struct JsxClosingElement {
    pub name: JsxElementName,
    pub span: Span,
}

/// JSX element name
#[derive(Debug, Clone, PartialEq)]
pub enum JsxElementName {
    /// Simple identifier: div, span, Button
    Identifier(Identifier),

    /// Namespaced: svg:path
    Namespaced {
        namespace: Identifier,
        name: Identifier,
    },

    /// Member expression: React.Fragment, UI.Button
    MemberExpression {
        object: Box<JsxElementName>,
        property: Identifier,
    },
}

impl JsxElementName {
    /// Get the string representation of the name
    pub fn to_string(&self, interner: &crate::parser::interner::Interner) -> String {
        match self {
            JsxElementName::Identifier(id) => interner.resolve(id.name).to_string(),
            JsxElementName::Namespaced { namespace, name } => {
                format!("{}:{}", interner.resolve(namespace.name), interner.resolve(name.name))
            }
            JsxElementName::MemberExpression { object, property } => {
                format!("{}.{}", object.to_string(interner), interner.resolve(property.name))
            }
        }
    }

    /// Check if this is an intrinsic element (lowercase HTML tag)
    pub fn is_intrinsic(&self, interner: &crate::parser::interner::Interner) -> bool {
        match self {
            JsxElementName::Identifier(id) => {
                interner.resolve(id.name).chars().next().map_or(false, |c| c.is_lowercase())
            }
            _ => false,
        }
    }
}

/// JSX attribute
#[derive(Debug, Clone, PartialEq)]
pub enum JsxAttribute {
    /// Regular attribute: className="foo"
    Attribute {
        name: JsxAttributeName,
        value: Option<JsxAttributeValue>,
        span: Span,
    },

    /// Spread attribute: {...props}
    Spread { argument: Expression, span: Span },
}

/// JSX attribute name
#[derive(Debug, Clone, PartialEq)]
pub enum JsxAttributeName {
    /// Simple: className
    Identifier(Identifier),

    /// Namespaced: xml:lang
    Namespaced {
        namespace: Identifier,
        name: Identifier,
    },
}

/// JSX attribute value
#[derive(Debug, Clone, PartialEq)]
pub enum JsxAttributeValue {
    /// String literal: "value"
    StringLiteral(StringLiteral),

    /// Expression: {value}
    Expression(Expression),

    /// Nested element: <Component prop={<div />} />
    JsxElement(Box<JsxElement>),

    /// Fragment: <Component prop={<>...</>} />
    JsxFragment(Box<JsxFragment>),
}

/// JSX child node
#[derive(Debug, Clone, PartialEq)]
pub enum JsxChild {
    /// Text content
    Text(JsxText),

    /// Element: <div />
    Element(JsxElement),

    /// Fragment: <>...</>
    Fragment(JsxFragment),

    /// Expression: {value}
    Expression(JsxExpression),
}

/// JSX text content
#[derive(Debug, Clone, PartialEq)]
pub struct JsxText {
    pub value: String,
    pub raw: String, // Preserves whitespace
    pub span: Span,
}

/// JSX expression: {value}
#[derive(Debug, Clone, PartialEq)]
pub struct JsxExpression {
    pub expression: Option<Expression>, // None for empty {}
    pub span: Span,
}

/// JSX fragment: <>children</>
#[derive(Debug, Clone, PartialEq)]
pub struct JsxFragment {
    /// Opening: <>
    pub opening: JsxOpeningFragment,

    /// Children
    pub children: Vec<JsxChild>,

    /// Closing: </>
    pub closing: JsxClosingFragment,

    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JsxOpeningFragment {
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JsxClosingFragment {
    pub span: Span,
}

/// InstanceOf expression: expr instanceof ClassName
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceOfExpression {
    /// The expression to check
    pub object: Box<Expression>,
    /// The type to check against (class name)
    pub type_name: TypeAnnotation,
    pub span: Span,
}

/// Type cast expression: expr as TypeName
#[derive(Debug, Clone, PartialEq)]
pub struct TypeCastExpression {
    /// The expression to cast
    pub object: Box<Expression>,
    /// The target type
    pub target_type: TypeAnnotation,
    pub span: Span,
}
