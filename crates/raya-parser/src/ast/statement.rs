//! Statement AST nodes
//!
//! This module defines all statement types in the Raya language, including:
//! - Variable declarations (let, const)
//! - Function and class declarations
//! - Control flow statements (if, while, for, switch, etc.)
//! - Import/export declarations

use super::*;
use crate::token::Span;

/// Top-level or block-level statement
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// Variable declaration: let/const
    VariableDecl(VariableDecl),

    /// Function declaration
    FunctionDecl(FunctionDecl),

    /// Class declaration
    ClassDecl(ClassDecl),

    /// Type alias declaration (interfaces BANNED in Raya - LANG.md §10)
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

    /// For-of loop
    ForOf(ForOfStatement),

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

    /// Block statement (DEPRECATED - only kept for legacy AST compatibility)
    /// NOTE: Raya does NOT support standalone { } blocks as statements.
    /// BlockStatement is only used in function bodies, control flow (if/while/for/try),
    /// and arrow function bodies. This variant should not be constructed by the parser.
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
            Statement::TypeAliasDecl(s) => &s.span,
            Statement::ImportDecl(s) => &s.span,
            Statement::ExportDecl(s) => s.span(),
            Statement::Expression(s) => &s.span,
            Statement::If(s) => &s.span,
            Statement::Switch(s) => &s.span,
            Statement::While(s) => &s.span,
            Statement::DoWhile(s) => &s.span,
            Statement::For(s) => &s.span,
            Statement::ForOf(s) => &s.span,
            Statement::Break(s) => &s.span,
            Statement::Continue(s) => &s.span,
            Statement::Return(s) => &s.span,
            Statement::Throw(s) => &s.span,
            Statement::Try(s) => &s.span,
            Statement::Block(s) => &s.span,
            Statement::Empty(span) => span,
        }
    }

    /// Check if this statement is a declaration
    pub fn is_declaration(&self) -> bool {
        matches!(
            self,
            Statement::VariableDecl(_)
                | Statement::FunctionDecl(_)
                | Statement::ClassDecl(_)
                | Statement::TypeAliasDecl(_)
        )
    }
}

// ============================================================================
// Variable Declaration
// ============================================================================

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

// ============================================================================
// Function Declaration
// ============================================================================

/// Function declaration
///
/// # Example
/// ```text
/// function add(x: number, y: number): number {
///     return x + y;
/// }
/// ```
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

/// Function parameter
#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    /// Decorators (@inject, @validate, etc.)
    pub decorators: Vec<Decorator>,

    pub pattern: Pattern,
    pub type_annotation: Option<TypeAnnotation>,
    /// Default value for the parameter (e.g., `x: number = 10`)
    pub default_value: Option<Expression>,
    pub span: Span,
}

// ============================================================================
// Class Declaration
// ============================================================================

/// Class declaration
///
/// # Example
/// ```text
/// @sealed
/// abstract class Shape {
///     abstract area(): number;
///
///     describe(): string {
///         return `Area: ${this.area()}`;
///     }
/// }
///
/// class Circle extends Shape {
///     constructor(public radius: number) { super(); }
///
///     area(): number {
///         return Math.PI * this.radius ** 2;
///     }
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ClassDecl {
    /// Decorators (@sealed, @logged, etc.)
    pub decorators: Vec<Decorator>,

    /// Abstract modifier
    pub is_abstract: bool,

    pub name: Identifier,
    pub type_params: Option<Vec<TypeParameter>>,
    pub extends: Option<TypeAnnotation>,

    /// Implements clauses (type aliases only, NOT interfaces - LANG.md §10)
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

/// Visibility modifier for class members (Java-like semantics)
///
/// | Modifier | Same Class | Subclass | Other Classes |
/// |----------|------------|----------|---------------|
/// | Private  | ✅         | ❌        | ❌             |
/// | Protected| ✅         | ✅        | ❌             |
/// | Public   | ✅         | ✅        | ✅             |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    /// Private - only accessible within the same class
    Private,
    /// Protected - accessible within the same class and subclasses
    Protected,
    /// Public - accessible from anywhere (default)
    #[default]
    Public,
}

/// Field declaration
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDecl {
    /// Decorators (@validate, @readonly, etc.)
    pub decorators: Vec<Decorator>,

    /// Visibility modifier (private/protected/public)
    pub visibility: Visibility,

    pub name: Identifier,
    pub type_annotation: Option<TypeAnnotation>,
    pub initializer: Option<Expression>,
    pub is_static: bool,
    pub span: Span,
}

/// Method declaration
#[derive(Debug, Clone, PartialEq)]
pub struct MethodDecl {
    /// Decorators (@logged, @memoized, etc.)
    pub decorators: Vec<Decorator>,

    /// Visibility modifier (private/protected/public)
    pub visibility: Visibility,

    /// Abstract modifier (method has no body)
    pub is_abstract: bool,

    pub name: Identifier,
    pub type_params: Option<Vec<TypeParameter>>,
    pub params: Vec<Parameter>,
    pub return_type: Option<TypeAnnotation>,

    /// None if is_abstract is true
    pub body: Option<BlockStatement>,

    pub is_static: bool,
    pub is_async: bool,
    pub span: Span,
}

/// Constructor declaration
#[derive(Debug, Clone, PartialEq)]
pub struct ConstructorDecl {
    pub params: Vec<Parameter>,
    pub body: BlockStatement,
    pub span: Span,
}

// ============================================================================
// Decorators
// ============================================================================

/// Decorator: @decorator or @decorator(arg1, arg2)
///
/// # Example
/// ```text
/// @sealed
/// class Foo { }
///
/// @logged
/// method() { }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Decorator {
    /// Decorator name/expression
    pub expression: Expression,
    pub span: Span,
}

// ============================================================================
// Type Alias (Interfaces BANNED)
// ============================================================================

/// Type alias: type Point = { x: number; y: number; }
///
/// NOTE: Raya does NOT support `interface` declarations (LANG.md §10).
/// Use type aliases for all type definitions.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeAliasDecl {
    pub name: Identifier,
    pub type_params: Option<Vec<TypeParameter>>,
    pub type_annotation: TypeAnnotation,
    pub span: Span,
}

// ============================================================================
// Control Flow Statements
// ============================================================================

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

/// For-of loop: for (const item of collection) { ... }
#[derive(Debug, Clone, PartialEq)]
pub struct ForOfStatement {
    /// Left side of the for-of (variable declaration or identifier pattern)
    pub left: ForOfLeft,
    /// Right side expression (the iterable)
    pub right: Expression,
    /// Loop body
    pub body: Box<Statement>,
    pub span: Span,
}

/// Left-hand side of a for-of statement
#[derive(Debug, Clone, PartialEq)]
pub enum ForOfLeft {
    /// let/const pattern
    VariableDecl(VariableDecl),
    /// Existing variable
    Pattern(Pattern),
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

/// Block statement - a sequence of statements wrapped in { }.
/// NOTE: In Raya, this is NOT a standalone statement type. BlockStatement is only
/// used as part of:
/// - Function bodies (FunctionDeclaration.body)
/// - Control flow constructs (if/while/for/try statements)
/// - Arrow function bodies (ArrowBody::Block)
/// At the statement level, { } is always parsed as an object literal expression.
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

// ============================================================================
// Module System
// ============================================================================

/// Import declaration
///
/// # Example
/// ```text
/// import { foo, bar } from "./module";
/// import * as utils from "./utils";
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub specifiers: Vec<ImportSpecifier>,
    pub source: StringLiteral,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportSpecifier {
    /// import { foo } or import { foo as bar }
    Named {
        name: Identifier,
        alias: Option<Identifier>,
    },
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
    Named {
        specifiers: Vec<ExportSpecifier>,
        source: Option<StringLiteral>,
        span: Span,
    },

    /// export * from "./foo";
    All { source: StringLiteral, span: Span },
}

impl ExportDecl {
    pub fn span(&self) -> &Span {
        match self {
            ExportDecl::Declaration(stmt) => stmt.span(),
            ExportDecl::Named { span, .. } => span,
            ExportDecl::All { span, .. } => span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportSpecifier {
    pub name: Identifier,
    pub alias: Option<Identifier>,
}
