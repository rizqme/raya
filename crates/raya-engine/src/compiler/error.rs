//! Compilation errors

use thiserror::Error;

pub type CompileResult<T> = Result<T, CompileError>;

#[derive(Debug, Error)]
pub enum CompileError {
    #[error("Parse error: {message}")]
    Parse { message: String },

    #[error("Undefined variable: {name}")]
    UndefinedVariable { name: String },

    #[error("Undefined function: {name}")]
    UndefinedFunction { name: String },

    #[error("Undefined type: {name}")]
    UndefinedType { name: String },

    #[error("Function {name} not found")]
    FunctionNotFound { name: String },

    #[error("Too many local variables (max 65535)")]
    TooManyLocals,

    #[error("Too many constants (max 65535)")]
    TooManyConstants,

    #[error("Too many parameters (max 255)")]
    TooManyParameters,

    #[error("Jump offset too large")]
    JumpTooLarge,

    #[error("Invalid break statement (not in loop)")]
    InvalidBreak,

    #[error("Invalid continue statement (not in loop)")]
    InvalidContinue,

    #[error("Invalid return statement (not in function)")]
    InvalidReturn,

    #[error("Unsupported feature: {feature}")]
    UnsupportedFeature { feature: String },

    #[error("Internal compiler error: {message}")]
    InternalError { message: String },

    #[error("Bytecode verification failed: {message}")]
    Verification { message: String },
}
