//! Runtime error types.

use raya_engine::compiler::CompileError;
use raya_engine::vm::VmError;

/// Errors that can occur during compilation, loading, or execution.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// File I/O error
    #[error("{0}")]
    Io(#[from] std::io::Error),

    /// Lexer error
    #[error("Lexer error: {0}")]
    Lex(String),

    /// Parse error
    #[error("Parse error: {0}")]
    Parse(String),

    /// Type checking or binding error
    #[error("Type error: {0}")]
    TypeCheck(String),

    /// Bytecode compilation error
    #[error("Compile error: {0}")]
    Compile(#[from] CompileError),

    /// Bytecode decoding error
    #[error("Bytecode error: {0}")]
    Bytecode(String),

    /// VM execution error
    #[error("Runtime error: {0}")]
    Vm(#[from] VmError),

    /// Dependency resolution error
    #[error("{0}")]
    Dependency(String),
}
