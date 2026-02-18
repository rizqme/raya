//! Error types for the Raya SDK ABI

/// Result type for ABI calls
pub type AbiResult<T> = Result<T, NativeError>;

/// Native module error types
#[derive(Debug, Clone, thiserror::Error)]
pub enum NativeError {
    /// Type mismatch during conversion
    #[error("Type mismatch: expected {expected}, got {got}")]
    TypeMismatch {
        /// Expected type name
        expected: String,
        /// Actual type name
        got: String,
    },

    /// Invalid argument
    #[error("Argument error: {0}")]
    ArgumentError(String),

    /// Function panicked
    #[error("Function panicked: {0}")]
    Panic(String),

    /// Module-level error
    #[error("Module error: {0}")]
    ModuleError(String),

    /// ABI operation failed
    #[error("{0}")]
    AbiError(String),
}

impl From<String> for NativeError {
    fn from(s: String) -> Self {
        NativeError::AbiError(s)
    }
}

impl From<&str> for NativeError {
    fn from(s: &str) -> Self {
        NativeError::AbiError(s.to_string())
    }
}
