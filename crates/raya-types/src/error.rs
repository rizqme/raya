//! Type system errors

use thiserror::Error;

/// Errors that can occur during type checking and type operations
#[derive(Debug, Clone, Error, PartialEq)]
pub enum TypeError {
    /// Type mismatch between expected and actual types
    #[error("Type mismatch: expected {expected}, got {actual}")]
    Mismatch {
        /// Expected type
        expected: String,
        /// Actual type
        actual: String,
    },

    /// Undefined type reference
    #[error("Undefined type: {name}")]
    UndefinedType {
        /// Type name that was not found
        name: String,
    },

    /// Generic type error
    #[error("Generic type error: {message}")]
    Generic {
        /// Error message
        message: String,
    },

    /// Circular type reference
    #[error("Circular type reference detected: {cycle}")]
    CircularReference {
        /// Description of the cycle
        cycle: String,
    },

    /// Invalid type argument count
    #[error("Invalid type argument count: expected {expected}, got {actual}")]
    InvalidTypeArgCount {
        /// Expected count
        expected: usize,
        /// Actual count
        actual: usize,
    },

    /// Type constraint violation
    #[error("Type constraint violation: {constraint}")]
    ConstraintViolation {
        /// Constraint that was violated
        constraint: String,
    },

    /// Subtyping error
    #[error("Subtyping error: {sub} is not a subtype of {sup}")]
    NotSubtype {
        /// Subtype
        sub: String,
        /// Supertype
        sup: String,
    },

    /// Invalid union type
    #[error("Invalid union type: {reason}")]
    InvalidUnion {
        /// Reason for invalidity
        reason: String,
    },

    /// Function type error
    #[error("Function type error: {reason}")]
    FunctionTypeError {
        /// Reason for error
        reason: String,
    },
}
