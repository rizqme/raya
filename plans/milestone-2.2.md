# Milestone 2.2: AST Definition

**Duration:** 1-2 weeks
**Status:** 🔄 Not Started
**Dependencies:** Milestone 2.1 (Lexer) ✅ Complete
**Next Milestone:** 2.3 (Parser Implementation)

---

## Table of Contents

1. [Overview](#overview)
2. [Goals](#goals)
3. [Non-Goals](#non-goals)
4. [Design Principles](#design-principles)
5. [Phase 1: Core AST Nodes](#phase-1-core-ast-nodes-days-1-2)
6. [Phase 2: Expression Nodes](#phase-2-expression-nodes-days-3-4)
7. [Phase 3: Type Nodes](#phase-3-type-nodes-days-5-6)
8. [Phase 4: Visitor Pattern & Utilities](#phase-4-visitor-pattern--utilities-days-7-8)
9. [Success Criteria](#success-criteria)
10. [Testing Strategy](#testing-strategy)
11. [References](#references)

---

## Overview

Define a complete Abstract Syntax Tree (AST) for the Raya programming language. The AST will represent the syntactic structure of Raya programs after lexical analysis but before type checking or code generation.

This milestone focuses solely on **defining the data structures**. Parsing (constructing the AST from tokens) will be handled in Milestone 2.3.

### What is an AST?

An AST is a tree representation of the syntactic structure of source code. Each node represents a construct in the source code (statement, expression, type annotation, etc.) with source location information attached.

**Example:**
```typescript
const x = 42 + 10;
```

**AST:**
```
Module
└── VariableDeclaration
    ├── pattern: Identifier("x")
    ├── type: None
    └── initializer: BinaryExpression
        ├── operator: Plus
        ├── left: IntLiteral(42)
        └── right: IntLiteral(10)
```

---

## Goals

### Primary Goals

1. **Complete AST Coverage**: Define nodes for every Raya language construct from LANG.md
2. **Type Safety**: Use Rust's type system to make illegal states unrepresentable
3. **Source Tracking**: Every node must have a `Span` for error reporting
4. **Memory Efficiency**: Minimize allocations with `Box` and `Rc` where appropriate
5. **Traversal Support**: Enable visitor pattern for AST traversal
6. **Pretty Printing**: Support debug formatting for AST visualization

### Secondary Goals

1. **Documentation**: Comprehensive doc comments for all public types
2. **Examples**: Document expected AST structure for common patterns
3. **Testing**: Unit tests for AST construction and traversal

---

## Non-Goals

1. **Parsing**: Building AST from tokens (Milestone 2.3)
2. **Type Checking**: Validating types (Milestone 2.4)
3. **Code Generation**: Emitting bytecode (Milestone 3.1)
4. **Optimization**: AST transformations (Milestone 3.2)
5. **Pretty Printing Source**: Unparsing AST back to source code

---

## Design Principles

### 1. Mirror the Language Specification

Every language construct in LANG.md should have a corresponding AST node:
- Statements (if, while, for, return, etc.)
- Expressions (literals, binary ops, function calls, etc.)
- Declarations (function, class, interface, type alias, etc.)
- Types (primitives, unions, functions, generics, etc.)

### 2. Prefer Enums Over Inheritance

Use Rust's enums instead of OOP inheritance for node types:

```rust
// ✅ GOOD: Enum-based
pub enum Statement {
    VariableDecl(VariableDecl),
    FunctionDecl(FunctionDecl),
    ClassDecl(ClassDecl),
    Expression(Expression),
    // ...
}

// ❌ BAD: Trait-based (harder to exhaustively match)
pub trait Statement { /* ... */ }
```

### 3. Box Large or Recursive Types

Use `Box` for:
- Recursive types (expressions containing expressions)
- Large types that would bloat parent structs

```rust
pub struct BinaryExpression {
    pub left: Box<Expression>,   // Box: recursive
    pub operator: BinaryOperator, // No box: small enum
    pub right: Box<Expression>,   // Box: recursive
    pub span: Span,
}
```

### 4. Attach Spans Everywhere

Every node must have a `Span` for error reporting:

```rust
pub struct FunctionDecl {
    pub name: Identifier,
    pub params: Vec<Parameter>,
    pub return_type: Option<TypeAnnotation>,
    pub body: BlockStatement,
    pub span: Span,  // Always include!
}
```

### 5. Use Clear Naming Conventions

- **Statements**: `*Statement` or `*Decl` (e.g., `IfStatement`, `FunctionDecl`)
- **Expressions**: `*Expression` (e.g., `BinaryExpression`, `CallExpression`)
- **Types**: `TypeAnnotation` with `Type` enum (e.g., `Type::Union`)
- **Patterns**: `Pattern` enum (for destructuring)

### 6. Separate Types from Expressions

Don't conflate type annotations with runtime expressions:

```rust
// ✅ GOOD: Separate types
pub struct FunctionDecl {
    pub return_type: Option<TypeAnnotation>,  // Compile-time type
    pub body: BlockStatement,                  // Runtime code
}

// ❌ BAD: Mixing types and expressions would be confusing
```

---

## Phase 1: Core AST Nodes (Days 1-2)

### Goal
Define the root module structure and core statement/declaration types.

### Deliverables

#### 1.1 Module & Program Structure

**File:** `crates/raya-parser/src/ast.rs`

```rust
use crate::token::Span;

/// Root node: a Raya source file
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    /// Top-level statements (declarations, imports, exports)
    pub statements: Vec<Statement>,

    /// Span covering the entire module
    pub span: Span,
}

impl Module {
    pub fn new(statements: Vec<Statement>, span: Span) -> Self {
        Self { statements, span }
    }
}
```

#### 1.2 Statement Enum

```rust
/// Top-level or block-level statement
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// Variable declaration: let/const
    VariableDecl(VariableDecl),

    /// Function declaration
    FunctionDecl(FunctionDecl),

    /// Class declaration
    ClassDecl(ClassDecl),

    /// Interface declaration
    InterfaceDecl(InterfaceDecl),

    /// Type alias declaration
    TypeAliasDecl(TypeAliasDecl),

    /// Import statement
    ImportDecl(ImportDecl),

    /// Export statement
    ExportDecl(ExportDecl),

    /// Expression statement (e.g., function call)
    Expression(ExpressionStatement),

    /// If statement
    If(IfStatement),

    /// Switch statement
    Switch(SwitchStatement),

    /// While loop
    While(WhileStatement),

    /// Do-while loop
    DoWhile(DoWhileStatement),

    /// For loop
    For(ForStatement),

    /// Break statement
    Break(BreakStatement),

    /// Continue statement
    Continue(ContinueStatement),

    /// Return statement
    Return(ReturnStatement),

    /// Throw statement
    Throw(ThrowStatement),

    /// Try-catch-finally
    Try(TryStatement),

    /// Block statement
    Block(BlockStatement),

    /// Empty statement (;)
    Empty(Span),
}

impl Statement {
    /// Get the span of this statement
    pub fn span(&self) -> &Span {
        match self {
            Statement::VariableDecl(s) => &s.span,
            Statement::FunctionDecl(s) => &s.span,
            Statement::ClassDecl(s) => &s.span,
            Statement::InterfaceDecl(s) => &s.span,
            Statement::TypeAliasDecl(s) => &s.span,
            Statement::ImportDecl(s) => &s.span,
            Statement::ExportDecl(s) => &s.span,
            Statement::Expression(s) => &s.span,
            Statement::If(s) => &s.span,
            Statement::Switch(s) => &s.span,
            Statement::While(s) => &s.span,
            Statement::DoWhile(s) => &s.span,
            Statement::For(s) => &s.span,
            Statement::Break(s) => &s.span,
            Statement::Continue(s) => &s.span,
            Statement::Return(s) => &s.span,
            Statement::Throw(s) => &s.span,
            Statement::Try(s) => &s.span,
            Statement::Block(s) => &s.span,
            Statement::Empty(span) => span,
        }
    }
}
```

#### 1.3 Variable Declaration

```rust
/// Variable declaration: let x = 42; or const y: number = 10;
#[derive(Debug, Clone, PartialEq)]
pub struct VariableDecl {
    /// let or const
    pub kind: VariableKind,

    /// Pattern (identifier or destructuring)
    pub pattern: Pattern,

    /// Optional type annotation
    pub type_annotation: Option<TypeAnnotation>,

    /// Initializer expression (required for const)
    pub initializer: Option<Expression>,

    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableKind {
    Let,
    Const,
}
```

#### 1.4 Function Declaration

```rust
/// Function declaration
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
    /// Function name
    pub name: Identifier,

    /// Type parameters (generics)
    pub type_params: Option<Vec<TypeParameter>>,

    /// Parameters
    pub params: Vec<Parameter>,

    /// Return type annotation
    pub return_type: Option<TypeAnnotation>,

    /// Function body
    pub body: BlockStatement,

    /// Is async function?
    pub is_async: bool,

    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub pattern: Pattern,
    pub type_annotation: Option<TypeAnnotation>,
    pub span: Span,
}
```

#### 1.5 Class Declaration

```rust
/// Class declaration
#[derive(Debug, Clone, PartialEq)]
pub struct ClassDecl {
    pub name: Identifier,
    pub type_params: Option<Vec<TypeParameter>>,
    pub extends: Option<TypeAnnotation>,
    pub implements: Vec<TypeAnnotation>,
    pub members: Vec<ClassMember>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClassMember {
    Field(FieldDecl),
    Method(MethodDecl),
    Constructor(ConstructorDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldDecl {
    pub name: Identifier,
    pub type_annotation: Option<TypeAnnotation>,
    pub initializer: Option<Expression>,
    pub is_static: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MethodDecl {
    pub name: Identifier,
    pub type_params: Option<Vec<TypeParameter>>,
    pub params: Vec<Parameter>,
    pub return_type: Option<TypeAnnotation>,
    pub body: BlockStatement,
    pub is_static: bool,
    pub is_async: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstructorDecl {
    pub params: Vec<Parameter>,
    pub body: BlockStatement,
    pub span: Span,
}
```

#### 1.6 Interface & Type Alias

```rust
/// Interface declaration
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceDecl {
    pub name: Identifier,
    pub type_params: Option<Vec<TypeParameter>>,
    pub extends: Vec<TypeAnnotation>,
    pub members: Vec<InterfaceMember>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InterfaceMember {
    Property(PropertySignature),
    Method(MethodSignature),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropertySignature {
    pub name: Identifier,
    pub type_annotation: TypeAnnotation,
    pub optional: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MethodSignature {
    pub name: Identifier,
    pub type_params: Option<Vec<TypeParameter>>,
    pub params: Vec<Parameter>,
    pub return_type: TypeAnnotation,
    pub span: Span,
}

/// Type alias: type Point = { x: number; y: number; }
#[derive(Debug, Clone, PartialEq)]
pub struct TypeAliasDecl {
    pub name: Identifier,
    pub type_params: Option<Vec<TypeParameter>>,
    pub type_annotation: TypeAnnotation,
    pub span: Span,
}
```

#### 1.7 Control Flow Statements

```rust
/// If statement
#[derive(Debug, Clone, PartialEq)]
pub struct IfStatement {
    pub condition: Expression,
    pub then_branch: Box<Statement>,
    pub else_branch: Option<Box<Statement>>,
    pub span: Span,
}

/// Switch statement
#[derive(Debug, Clone, PartialEq)]
pub struct SwitchStatement {
    pub discriminant: Expression,
    pub cases: Vec<SwitchCase>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchCase {
    /// None for default case
    pub test: Option<Expression>,
    pub consequent: Vec<Statement>,
    pub span: Span,
}

/// While loop
#[derive(Debug, Clone, PartialEq)]
pub struct WhileStatement {
    pub condition: Expression,
    pub body: Box<Statement>,
    pub span: Span,
}

/// Do-while loop
#[derive(Debug, Clone, PartialEq)]
pub struct DoWhileStatement {
    pub body: Box<Statement>,
    pub condition: Expression,
    pub span: Span,
}

/// For loop
#[derive(Debug, Clone, PartialEq)]
pub struct ForStatement {
    pub init: Option<ForInit>,
    pub test: Option<Expression>,
    pub update: Option<Expression>,
    pub body: Box<Statement>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ForInit {
    VariableDecl(VariableDecl),
    Expression(Expression),
}

/// Break statement
#[derive(Debug, Clone, PartialEq)]
pub struct BreakStatement {
    pub label: Option<Identifier>,
    pub span: Span,
}

/// Continue statement
#[derive(Debug, Clone, PartialEq)]
pub struct ContinueStatement {
    pub label: Option<Identifier>,
    pub span: Span,
}

/// Return statement
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnStatement {
    pub value: Option<Expression>,
    pub span: Span,
}

/// Throw statement
#[derive(Debug, Clone, PartialEq)]
pub struct ThrowStatement {
    pub value: Expression,
    pub span: Span,
}

/// Try-catch-finally
#[derive(Debug, Clone, PartialEq)]
pub struct TryStatement {
    pub body: BlockStatement,
    pub catch_clause: Option<CatchClause>,
    pub finally_clause: Option<BlockStatement>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatchClause {
    pub param: Option<Pattern>,
    pub body: BlockStatement,
    pub span: Span,
}

/// Block statement
#[derive(Debug, Clone, PartialEq)]
pub struct BlockStatement {
    pub statements: Vec<Statement>,
    pub span: Span,
}

/// Expression statement
#[derive(Debug, Clone, PartialEq)]
pub struct ExpressionStatement {
    pub expression: Expression,
    pub span: Span,
}
```

#### 1.8 Module System

```rust
/// Import declaration
#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub specifiers: Vec<ImportSpecifier>,
    pub source: StringLiteral,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportSpecifier {
    /// import { foo }
    Named { name: Identifier, alias: Option<Identifier> },
    /// import * as foo
    Namespace(Identifier),
    /// import foo (default)
    Default(Identifier),
}

/// Export declaration
#[derive(Debug, Clone, PartialEq)]
pub enum ExportDecl {
    /// export const x = 42;
    Declaration(Box<Statement>),

    /// export { foo, bar };
    Named { specifiers: Vec<ExportSpecifier>, source: Option<StringLiteral>, span: Span },

    /// export * from "./foo";
    All { source: StringLiteral, span: Span },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportSpecifier {
    pub name: Identifier,
    pub alias: Option<Identifier>,
}
```

### Tasks for Phase 1

- [ ] Create `ast.rs` module
- [ ] Define `Module` struct
- [ ] Define `Statement` enum with all variants
- [ ] Define variable, function, class, interface, type alias declarations
- [ ] Define all control flow statements
- [ ] Define import/export declarations
- [ ] Add doc comments for all types
- [ ] Write unit tests for struct construction

---

## Phase 2: Expression Nodes (Days 3-4)

### Goal
Define all expression types, from literals to complex function calls.

### Deliverables

#### 2.1 Expression Enum

```rust
/// Expression (produces a value)
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    /// Literals
    IntLiteral(IntLiteral),
    FloatLiteral(FloatLiteral),
    StringLiteral(StringLiteral),
    TemplateLiteral(TemplateLiteral),
    BooleanLiteral(BooleanLiteral),
    NullLiteral(Span),

    /// Identifier
    Identifier(Identifier),

    /// Array literal: [1, 2, 3]
    Array(ArrayExpression),

    /// Object literal: { x: 1, y: 2 }
    Object(ObjectExpression),

    /// Unary expression: !x, -y, ++z
    Unary(UnaryExpression),

    /// Binary expression: x + y, a && b
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
}
```

#### 2.2 Literal Expressions

```rust
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
    Expression(Expression),
}

/// Boolean literal: true, false
#[derive(Debug, Clone, PartialEq)]
pub struct BooleanLiteral {
    pub value: bool,
    pub span: Span,
}
```

#### 2.3 Identifier & Patterns

```rust
/// Identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identifier {
    pub name: String,
    pub span: Span,
}

impl Identifier {
    pub fn new(name: String, span: Span) -> Self {
        Self { name, span }
    }
}

/// Pattern (for destructuring)
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// Simple identifier: x
    Identifier(Identifier),

    /// Array destructuring: [x, y]
    Array(ArrayPattern),

    /// Object destructuring: { x, y }
    Object(ObjectPattern),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArrayPattern {
    pub elements: Vec<Option<Pattern>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjectPattern {
    pub properties: Vec<ObjectPatternProperty>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjectPatternProperty {
    pub key: Identifier,
    pub value: Pattern,
    pub span: Span,
}
```

#### 2.4 Unary & Binary Expressions

```rust
/// Unary expression: !x, -y, ++z
#[derive(Debug, Clone, PartialEq)]
pub struct UnaryExpression {
    pub operator: UnaryOperator,
    pub operand: Box<Expression>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOperator {
    Plus,         // +x
    Minus,        // -x
    Not,          // !x
    BitwiseNot,   // ~x
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
    Add,          // +
    Subtract,     // -
    Multiply,     // *
    Divide,       // /
    Modulo,       // %
    Exponent,     // **

    // Comparison
    Equal,        // ==
    NotEqual,     // !=
    StrictEqual,  // ===
    StrictNotEqual, // !==
    LessThan,     // <
    LessEqual,    // <=
    GreaterThan,  // >
    GreaterEqual, // >=

    // Bitwise
    BitwiseAnd,   // &
    BitwiseOr,    // |
    BitwiseXor,   // ^
    LeftShift,    // <<
    RightShift,   // >>
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
    And,          // &&
    Or,           // ||
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
    Assign,       // =
    AddAssign,    // +=
    SubAssign,    // -=
    MulAssign,    // *=
    DivAssign,    // /=
    ModAssign,    // %=
    AndAssign,    // &=
    OrAssign,     // |=
    XorAssign,    // ^=
    LeftShiftAssign,  // <<=
    RightShiftAssign, // >>=
    UnsignedRightShiftAssign, // >>>=
}
```

#### 2.5 Complex Expressions

```rust
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
    pub optional: bool,  // obj?.prop
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
```

### Tasks for Phase 2

- [ ] Define `Expression` enum with all variants
- [ ] Define all literal types
- [ ] Define identifier and pattern types
- [ ] Define unary, binary, logical expressions
- [ ] Define assignment expressions
- [ ] Define complex expressions (call, member, index, etc.)
- [ ] Add doc comments with examples
- [ ] Write unit tests for expression construction

---

## Phase 3: Type Nodes (Days 5-6)

### Goal
Define type annotation structures for the Raya type system.

### Deliverables

#### 3.1 Type Annotation Structure

```rust
/// Type annotation (compile-time type)
#[derive(Debug, Clone, PartialEq)]
pub struct TypeAnnotation {
    pub ty: Type,
    pub span: Span,
}

/// Type
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// Primitive types: number, string, boolean, null, void
    Primitive(PrimitiveType),

    /// Type reference: MyClass, Point<T>
    Reference(TypeReference),

    /// Union type: number | string | null
    Union(UnionType),

    /// Function type: (x: number) => number
    Function(FunctionType),

    /// Array type: number[]
    Array(ArrayType),

    /// Tuple type: [number, string]
    Tuple(TupleType),

    /// Object type: { x: number; y: string }
    Object(ObjectType),

    /// Typeof type: typeof value (only for bare unions)
    Typeof(TypeofType),

    /// Parenthesized type: (number | string)
    Parenthesized(Box<TypeAnnotation>),
}
```

#### 3.2 Type Variants

```rust
/// Primitive type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveType {
    Number,   // number
    String,   // string
    Boolean,  // boolean
    Null,     // null
    Void,     // void
}

/// Type reference: Point, Map<K, V>
#[derive(Debug, Clone, PartialEq)]
pub struct TypeReference {
    pub name: Identifier,
    pub type_args: Option<Vec<TypeAnnotation>>,
}

/// Union type: A | B | C
#[derive(Debug, Clone, PartialEq)]
pub struct UnionType {
    pub types: Vec<TypeAnnotation>,
}

/// Function type: (x: number, y: string) => number
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionType {
    pub params: Vec<FunctionTypeParam>,
    pub return_type: Box<TypeAnnotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionTypeParam {
    pub name: Option<Identifier>,
    pub ty: TypeAnnotation,
}

/// Array type: T[]
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayType {
    pub element_type: Box<TypeAnnotation>,
}

/// Tuple type: [number, string, boolean]
#[derive(Debug, Clone, PartialEq)]
pub struct TupleType {
    pub element_types: Vec<TypeAnnotation>,
}

/// Object type: { x: number; y: string }
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectType {
    pub members: Vec<ObjectTypeMember>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ObjectTypeMember {
    Property(ObjectTypeProperty),
    Method(ObjectTypeMethod),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjectTypeProperty {
    pub name: Identifier,
    pub ty: TypeAnnotation,
    pub optional: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjectTypeMethod {
    pub name: Identifier,
    pub params: Vec<FunctionTypeParam>,
    pub return_type: TypeAnnotation,
    pub span: Span,
}

/// Typeof type: typeof value (for bare unions only)
#[derive(Debug, Clone, PartialEq)]
pub struct TypeofType {
    pub argument: Expression,
}

/// Type parameter (generic): T, K extends string
#[derive(Debug, Clone, PartialEq)]
pub struct TypeParameter {
    pub name: Identifier,
    pub constraint: Option<TypeAnnotation>,
    pub default: Option<TypeAnnotation>,
    pub span: Span,
}
```

### Tasks for Phase 3

- [ ] Define `TypeAnnotation` and `Type` enum
- [ ] Define all type variants (primitive, reference, union, function, etc.)
- [ ] Define type parameters for generics
- [ ] Add comprehensive doc comments
- [ ] Write unit tests for type construction

---

## Phase 4: Visitor Pattern & Utilities (Days 7-8)

### Goal
Provide utilities for traversing and debugging the AST.

### Deliverables

#### 4.1 Visitor Trait

**File:** `crates/raya-parser/src/ast/visitor.rs`

```rust
use super::*;

/// AST visitor trait
pub trait Visitor: Sized {
    fn visit_module(&mut self, module: &Module) {
        walk_module(self, module);
    }

    fn visit_statement(&mut self, stmt: &Statement) {
        walk_statement(self, stmt);
    }

    fn visit_expression(&mut self, expr: &Expression) {
        walk_expression(self, expr);
    }

    fn visit_type(&mut self, ty: &TypeAnnotation) {
        walk_type(self, ty);
    }

    // ... more visit methods
}

/// Walk helpers (default traversal)
pub fn walk_module<V: Visitor>(visitor: &mut V, module: &Module) {
    for stmt in &module.statements {
        visitor.visit_statement(stmt);
    }
}

pub fn walk_statement<V: Visitor>(visitor: &mut V, stmt: &Statement) {
    match stmt {
        Statement::VariableDecl(decl) => {
            if let Some(init) = &decl.initializer {
                visitor.visit_expression(init);
            }
        }
        Statement::FunctionDecl(decl) => {
            visitor.visit_statement(&Statement::Block(decl.body.clone()));
        }
        Statement::Expression(stmt) => {
            visitor.visit_expression(&stmt.expression);
        }
        // ... other statements
        _ => {}
    }
}

pub fn walk_expression<V: Visitor>(visitor: &mut V, expr: &Expression) {
    match expr {
        Expression::Binary(bin) => {
            visitor.visit_expression(&bin.left);
            visitor.visit_expression(&bin.right);
        }
        Expression::Call(call) => {
            visitor.visit_expression(&call.callee);
            for arg in &call.arguments {
                visitor.visit_expression(arg);
            }
        }
        // ... other expressions
        _ => {}
    }
}

pub fn walk_type<V: Visitor>(visitor: &mut V, ty: &TypeAnnotation) {
    match &ty.ty {
        Type::Union(union) => {
            for t in &union.types {
                visitor.visit_type(t);
            }
        }
        Type::Function(func) => {
            for param in &func.params {
                visitor.visit_type(&param.ty);
            }
            visitor.visit_type(&func.return_type);
        }
        // ... other types
        _ => {}
    }
}
```

#### 4.2 Pretty Printer

**File:** `crates/raya-parser/src/ast/display.rs`

```rust
use super::*;
use std::fmt;

/// Pretty-print the AST for debugging
pub struct AstPrinter {
    indent: usize,
}

impl AstPrinter {
    pub fn new() -> Self {
        Self { indent: 0 }
    }

    pub fn print_module(&mut self, module: &Module) -> String {
        let mut output = String::new();
        output.push_str("Module {\n");
        self.indent += 2;
        for stmt in &module.statements {
            output.push_str(&self.print_statement(stmt));
            output.push('\n');
        }
        self.indent -= 2;
        output.push_str("}\n");
        output
    }

    pub fn print_statement(&self, stmt: &Statement) -> String {
        let indent = " ".repeat(self.indent);
        match stmt {
            Statement::VariableDecl(decl) => {
                format!("{}VariableDecl {{ kind: {:?}, name: {:?}, ... }}",
                    indent, decl.kind, decl.pattern)
            }
            Statement::FunctionDecl(decl) => {
                format!("{}FunctionDecl {{ name: {:?}, params: {}, ... }}",
                    indent, decl.name.name, decl.params.len())
            }
            // ... other statements
            _ => format!("{}Statement::{:?}", indent, stmt),
        }
    }

    pub fn print_expression(&self, expr: &Expression) -> String {
        match expr {
            Expression::IntLiteral(lit) => format!("{}", lit.value),
            Expression::StringLiteral(lit) => format!("{:?}", lit.value),
            Expression::Identifier(id) => id.name.clone(),
            Expression::Binary(bin) => {
                format!("({} {:?} {})",
                    self.print_expression(&bin.left),
                    bin.operator,
                    self.print_expression(&bin.right))
            }
            // ... other expressions
            _ => format!("{:?}", expr),
        }
    }
}
```

#### 4.3 Utilities

**File:** `crates/raya-parser/src/ast/utils.rs`

```rust
use super::*;

impl Module {
    /// Count total nodes in the AST
    pub fn node_count(&self) -> usize {
        let mut counter = NodeCounter::new();
        counter.visit_module(self);
        counter.count
    }
}

struct NodeCounter {
    count: usize,
}

impl NodeCounter {
    fn new() -> Self {
        Self { count: 0 }
    }
}

impl Visitor for NodeCounter {
    fn visit_statement(&mut self, stmt: &Statement) {
        self.count += 1;
        walk_statement(self, stmt);
    }

    fn visit_expression(&mut self, expr: &Expression) {
        self.count += 1;
        walk_expression(self, expr);
    }
}

impl Statement {
    /// Check if this statement is a declaration
    pub fn is_declaration(&self) -> bool {
        matches!(self,
            Statement::VariableDecl(_)
            | Statement::FunctionDecl(_)
            | Statement::ClassDecl(_)
            | Statement::InterfaceDecl(_)
            | Statement::TypeAliasDecl(_)
        )
    }
}

impl Expression {
    /// Check if this expression is a literal
    pub fn is_literal(&self) -> bool {
        matches!(self,
            Expression::IntLiteral(_)
            | Expression::FloatLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
        )
    }
}
```

### Tasks for Phase 4

- [ ] Define `Visitor` trait
- [ ] Implement walk helpers for all node types
- [ ] Create `AstPrinter` for debugging
- [ ] Add utility methods to AST nodes
- [ ] Write tests for visitor traversal
- [ ] Write tests for pretty printing

---

## Success Criteria

### Must Have

- [x] Complete AST node definitions for all Raya language constructs
- [x] All nodes have `Span` for source tracking
- [x] `Statement` enum covers all statement types (20+ variants)
- [x] `Expression` enum covers all expression types (20+ variants)
- [x] `Type` enum covers all type annotations (8+ variants)
- [x] Pattern support for destructuring
- [x] Type parameter support for generics
- [x] Module system (import/export) nodes
- [x] Visitor pattern for AST traversal
- [x] Pretty printer for debugging
- [x] Comprehensive doc comments
- [x] 50+ unit tests

### Should Have

- [x] Node count utility
- [x] Node classification helpers (is_declaration, is_literal, etc.)
- [x] Example AST structures in documentation
- [x] Integration with lexer's `Span` type

### Nice to Have

- [ ] AST serialization (JSON/binary)
- [ ] AST comparison utilities
- [ ] Source code unparsing (AST → source)
- [ ] AST optimization utilities

---

## Testing Strategy

### Unit Tests

**File:** `crates/raya-parser/tests/ast_tests.rs`

```rust
use raya_parser::ast::*;
use raya_parser::token::Span;

#[test]
fn test_variable_decl_construction() {
    let decl = VariableDecl {
        kind: VariableKind::Let,
        pattern: Pattern::Identifier(Identifier::new(
            "x".to_string(),
            Span::new(4, 5, 1, 5),
        )),
        type_annotation: None,
        initializer: Some(Expression::IntLiteral(IntLiteral {
            value: 42,
            span: Span::new(8, 10, 1, 9),
        })),
        span: Span::new(0, 11, 1, 1),
    };

    assert_eq!(decl.kind, VariableKind::Let);
    assert!(decl.initializer.is_some());
}

#[test]
fn test_binary_expression_construction() {
    let expr = BinaryExpression {
        operator: BinaryOperator::Add,
        left: Box::new(Expression::IntLiteral(IntLiteral {
            value: 1,
            span: Span::new(0, 1, 1, 1),
        })),
        right: Box::new(Expression::IntLiteral(IntLiteral {
            value: 2,
            span: Span::new(4, 5, 1, 5),
        })),
        span: Span::new(0, 5, 1, 1),
    };

    assert_eq!(expr.operator, BinaryOperator::Add);
}

#[test]
fn test_visitor_traversal() {
    struct CounterVisitor {
        expr_count: usize,
    }

    impl Visitor for CounterVisitor {
        fn visit_expression(&mut self, expr: &Expression) {
            self.expr_count += 1;
            walk_expression(self, expr);
        }
    }

    let module = Module::new(
        vec![Statement::Expression(ExpressionStatement {
            expression: Expression::Binary(BinaryExpression {
                operator: BinaryOperator::Add,
                left: Box::new(Expression::IntLiteral(IntLiteral {
                    value: 1,
                    span: Span::new(0, 1, 1, 1),
                })),
                right: Box::new(Expression::IntLiteral(IntLiteral {
                    value: 2,
                    span: Span::new(4, 5, 1, 5),
                })),
                span: Span::new(0, 5, 1, 1),
            }),
            span: Span::new(0, 6, 1, 1),
        })],
        Span::new(0, 6, 1, 1),
    );

    let mut visitor = CounterVisitor { expr_count: 0 };
    visitor.visit_module(&module);

    assert_eq!(visitor.expr_count, 3); // Binary + 2 literals
}

#[test]
fn test_ast_pretty_printing() {
    let expr = Expression::Binary(BinaryExpression {
        operator: BinaryOperator::Add,
        left: Box::new(Expression::IntLiteral(IntLiteral {
            value: 1,
            span: Span::new(0, 1, 1, 1),
        })),
        right: Box::new(Expression::IntLiteral(IntLiteral {
            value: 2,
            span: Span::new(4, 5, 1, 5),
        })),
        span: Span::new(0, 5, 1, 1),
    });

    let printer = AstPrinter::new();
    let output = printer.print_expression(&expr);

    assert!(output.contains("Add"));
    assert!(output.contains("1"));
    assert!(output.contains("2"));
}
```

### Test Coverage

- [x] Node construction (all major types)
- [x] Visitor traversal (depth-first)
- [x] Pretty printing (readable output)
- [x] Span propagation (every node has span)
- [x] Pattern matching (destructuring)
- [x] Type annotation construction

---

## Implementation Plan

### Week 1

**Days 1-2:** Phase 1 (Core AST Nodes)
- Define module, statement enum, declarations
- Implement control flow statements
- Add doc comments

**Days 3-4:** Phase 2 (Expression Nodes)
- Define expression enum with all variants
- Implement literals, operators, complex expressions
- Add doc comments

**Days 5-6:** Phase 3 (Type Nodes)
- Define type annotation structures
- Implement all type variants
- Add doc comments

**Days 7-8:** Phase 4 (Visitor & Utilities)
- Implement visitor pattern
- Create pretty printer
- Add utility methods
- Write comprehensive tests

### Week 2 (Buffer)

- Polish documentation
- Add more examples
- Comprehensive testing
- Integration with lexer

---

## References

### Language Specification
- [design/LANG.md](../design/LANG.md) - Complete language specification
  - Section 3: Lexical Structure
  - Section 4: Type System
  - Sections 6-7: Expressions & Statements
  - Section 8: Functions
  - Section 9: Classes
  - Section 10: Interfaces

### Related Milestones
- [Milestone 2.1](milestone-2.1.md) - Lexer (✅ Complete)
- [Milestone 2.3](milestone-2.3.md) - Parser (Next)
- [Milestone 2.4](milestone-2.4.md) - Type System

### External References
- Rust AST Design Patterns: https://rust-unofficial.github.io/patterns/
- TypeScript AST: https://github.com/microsoft/TypeScript/tree/main/src/compiler
- ESTree Spec: https://github.com/estree/estree

---

## Notes

1. **AST vs CST**: We're building an Abstract Syntax Tree (AST), not a Concrete Syntax Tree (CST). This means we discard irrelevant syntax details (parentheses, semicolons, etc.) but keep semantic structure.

2. **Memory Management**: Use `Box` for recursive types and large structs. Use `Rc` only if multiple ownership is needed (rare in AST).

3. **PartialEq**: All AST nodes derive `PartialEq` for testing. Spans are included in equality checks.

4. **Clone**: All AST nodes derive `Clone` for flexibility. This is acceptable since AST construction is not performance-critical.

5. **Future Extensions**: This AST design should support future features like decorators, async generators, and advanced pattern matching.

---

**End of Milestone 2.2 Specification**
