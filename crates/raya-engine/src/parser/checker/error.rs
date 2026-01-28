//! Error types for type checking and binding
//!
//! Provides structured error types with source locations for reporting
//! type checking and name binding errors.

use crate::parser::Span;
use thiserror::Error;

/// Errors that can occur during name binding
#[derive(Debug, Error, Clone)]
pub enum BindError {
    /// Duplicate symbol definition in the same scope
    #[error("Duplicate symbol '{name}'")]
    DuplicateSymbol {
        /// Symbol name
        name: String,
        /// Location of original definition
        original: Span,
        /// Location of duplicate definition
        duplicate: Span,
    },

    /// Undefined type referenced
    #[error("Undefined type '{name}'")]
    UndefinedType {
        /// Type name
        name: String,
        /// Location where type was referenced
        span: Span,
    },

    /// Name is not a type
    #[error("'{name}' is not a type")]
    NotAType {
        /// Name that was expected to be a type
        name: String,
        /// Location where it was used as a type
        span: Span,
    },

    /// Invalid type expression
    #[error("Invalid type expression")]
    InvalidTypeExpr {
        /// Error message
        message: String,
        /// Location of invalid type expression
        span: Span,
    },

    /// Invalid type arguments for built-in generic type
    #[error("Type '{name}' expects {expected} type argument(s), got {actual}")]
    InvalidTypeArguments {
        /// Type name
        name: String,
        /// Expected number of type arguments
        expected: usize,
        /// Actual number of type arguments
        actual: usize,
        /// Location of type reference
        span: Span,
    },
}

/// Errors that can occur during type checking
#[derive(Debug, Error, Clone)]
pub enum CheckError {
    /// Type mismatch
    #[error("Type mismatch: expected {expected}, got {actual}")]
    TypeMismatch {
        /// Expected type (human-readable)
        expected: String,
        /// Actual type (human-readable)
        actual: String,
        /// Location of type mismatch
        span: Span,
        /// Optional note with additional context
        note: Option<String>,
    },

    /// Undefined variable referenced
    #[error("Undefined variable '{name}'")]
    UndefinedVariable {
        /// Variable name
        name: String,
        /// Location where variable was referenced
        span: Span,
    },

    /// Attempting to call a non-function type
    #[error("Cannot call non-function type '{ty}'")]
    NotCallable {
        /// Type that was attempted to be called
        ty: String,
        /// Location of call expression
        span: Span,
    },

    /// Wrong number of arguments in function call
    #[error("Wrong number of arguments: expected {expected}, got {actual}")]
    ArgumentCountMismatch {
        /// Expected number of arguments
        expected: usize,
        /// Actual number of arguments
        actual: usize,
        /// Location of call expression
        span: Span,
    },

    /// Non-exhaustive match/switch expression
    #[error("Non-exhaustive match: missing cases {}", missing.join(", "))]
    NonExhaustiveMatch {
        /// Missing variants
        missing: Vec<String>,
        /// Location of match/switch expression
        span: Span,
    },

    /// Property does not exist on type
    #[error("Property '{property}' does not exist on type '{ty}'")]
    PropertyNotFound {
        /// Property name
        property: String,
        /// Type name
        ty: String,
        /// Location of property access
        span: Span,
    },

    /// Return type mismatch
    #[error("Return type mismatch: expected {expected}, got {actual}")]
    ReturnTypeMismatch {
        /// Expected return type
        expected: String,
        /// Actual return type
        actual: String,
        /// Location of return statement
        span: Span,
    },

    /// Invalid binary operation
    #[error("Invalid binary operation '{op}' for types {left} and {right}")]
    InvalidBinaryOp {
        /// Operator
        op: String,
        /// Left operand type
        left: String,
        /// Right operand type
        right: String,
        /// Location of binary expression
        span: Span,
    },

    /// Invalid unary operation
    #[error("Invalid unary operation '{op}' for type {ty}")]
    InvalidUnaryOp {
        /// Operator
        op: String,
        /// Operand type
        ty: String,
        /// Location of unary expression
        span: Span,
    },

    /// Break statement outside of loop
    #[error("Break statement outside of loop")]
    BreakOutsideLoop {
        /// Location of break statement
        span: Span,
    },

    /// Continue statement outside of loop
    #[error("Continue statement outside of loop")]
    ContinueOutsideLoop {
        /// Location of continue statement
        span: Span,
    },

    /// Return statement outside of function
    #[error("Return statement outside of function")]
    ReturnOutsideFunction {
        /// Location of return statement
        span: Span,
    },

    /// Generic type instantiation error
    #[error("Generic type instantiation error: {message}")]
    GenericInstantiationError {
        /// Error message
        message: String,
        /// Location of generic instantiation
        span: Span,
    },

    /// Type constraint violation
    #[error("Type constraint violation: {message}")]
    ConstraintViolation {
        /// Error message
        message: String,
        /// Location of constraint violation
        span: Span,
    },

    /// Forbidden access to internal bare union fields ($type, $value)
    #[error("Cannot access internal field '{field}' on bare union. Use typeof for type narrowing.")]
    ForbiddenFieldAccess {
        /// Field name ($type or $value)
        field: String,
        /// Location of field access
        span: Span,
    },

    /// Undefined member on a type (static or instance)
    #[error("'{member}' does not exist")]
    UndefinedMember {
        /// Member name
        member: String,
        /// Location of member access
        span: Span,
    },
}

impl CheckError {
    /// Get the span associated with this error
    pub fn span(&self) -> Span {
        match self {
            CheckError::TypeMismatch { span, .. } => *span,
            CheckError::UndefinedVariable { span, .. } => *span,
            CheckError::NotCallable { span, .. } => *span,
            CheckError::ArgumentCountMismatch { span, .. } => *span,
            CheckError::NonExhaustiveMatch { span, .. } => *span,
            CheckError::PropertyNotFound { span, .. } => *span,
            CheckError::ReturnTypeMismatch { span, .. } => *span,
            CheckError::InvalidBinaryOp { span, .. } => *span,
            CheckError::InvalidUnaryOp { span, .. } => *span,
            CheckError::BreakOutsideLoop { span } => *span,
            CheckError::ContinueOutsideLoop { span } => *span,
            CheckError::ReturnOutsideFunction { span } => *span,
            CheckError::GenericInstantiationError { span, .. } => *span,
            CheckError::ConstraintViolation { span, .. } => *span,
            CheckError::ForbiddenFieldAccess { span, .. } => *span,
            CheckError::UndefinedMember { span, .. } => *span,
        }
    }
}

impl BindError {
    /// Get the primary span associated with this error
    pub fn span(&self) -> Span {
        match self {
            BindError::DuplicateSymbol { duplicate, .. } => *duplicate,
            BindError::UndefinedType { span, .. } => *span,
            BindError::NotAType { span, .. } => *span,
            BindError::InvalidTypeExpr { span, .. } => *span,
            BindError::InvalidTypeArguments { span, .. } => *span,
        }
    }
}
