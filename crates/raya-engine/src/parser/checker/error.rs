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

    /// Required parameter after optional parameter
    #[error("Required parameter '{name}' cannot follow an optional parameter")]
    RequiredAfterOptional {
        /// Parameter name
        name: String,
        /// Location of the required parameter
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
        /// Maximum expected number of arguments (total params)
        expected: usize,
        /// Minimum required number of arguments (params without defaults/optional)
        min_expected: usize,
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

    /// Cannot instantiate an abstract class
    #[error("Cannot instantiate abstract class '{name}'")]
    AbstractClassInstantiation {
        /// Class name
        name: String,
        /// Location of new expression
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

    /// Cannot assign to readonly property
    #[error("Cannot assign to readonly property '{property}'")]
    ReadonlyAssignment {
        /// Property name
        property: String,
        /// Location of assignment
        span: Span,
    },

    /// Cannot assign to const variable
    #[error("Cannot assign to const variable '{name}'")]
    ConstReassignment {
        /// Variable name
        name: String,
        /// Location of assignment
        span: Span,
    },

    /// Cannot use 'new' on a non-class type
    #[error("Cannot use 'new' with non-class type '{name}'")]
    NewNonClass {
        /// Name of the non-class identifier
        name: String,
        /// Location of new expression
        span: Span,
    },

    // ========================================================================
    // Decorator Errors
    // ========================================================================

    /// Decorator is not a valid decorator type
    #[error("Expression is not a valid decorator")]
    InvalidDecorator {
        /// Type of the decorator expression
        ty: String,
        /// Expected decorator type (e.g., "ClassDecorator<T>")
        expected: String,
        /// Location of decorator
        span: Span,
    },

    /// Decorator signature mismatch for method decorator
    #[error("Method signature does not match decorator constraint")]
    DecoratorSignatureMismatch {
        /// Expected method signature from MethodDecorator<F>
        expected_signature: String,
        /// Actual method signature
        actual_signature: String,
        /// Location of decorator
        span: Span,
    },

    /// Decorator return type mismatch
    #[error("Decorator return type mismatch")]
    DecoratorReturnMismatch {
        /// Expected return type
        expected: String,
        /// Actual return type
        actual: String,
        /// Location of decorator
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
            CheckError::AbstractClassInstantiation { span, .. } => *span,
            CheckError::UndefinedMember { span, .. } => *span,
            CheckError::ReadonlyAssignment { span, .. } => *span,
            CheckError::ConstReassignment { span, .. } => *span,
            CheckError::NewNonClass { span, .. } => *span,
            CheckError::InvalidDecorator { span, .. } => *span,
            CheckError::DecoratorSignatureMismatch { span, .. } => *span,
            CheckError::DecoratorReturnMismatch { span, .. } => *span,
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
            BindError::RequiredAfterOptional { span, .. } => *span,
        }
    }
}

// ========================================================================
// Warnings
// ========================================================================

/// Warning codes for configurable warnings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WarningCode {
    /// Unused variable (W1001)
    UnusedVariable,
    /// Unused import (W1002)
    UnusedImport,
    /// Unused parameter (W1003)
    UnusedParameter,
    /// Unreachable code (W1004)
    UnreachableCode,
    /// Shadowed variable (W1005)
    ShadowedVariable,
}

impl WarningCode {
    /// Get the warning code string (e.g., "W1001")
    pub fn as_str(&self) -> &'static str {
        match self {
            WarningCode::UnusedVariable => "W1001",
            WarningCode::UnusedImport => "W1002",
            WarningCode::UnusedParameter => "W1003",
            WarningCode::UnreachableCode => "W1004",
            WarningCode::ShadowedVariable => "W1005",
        }
    }

    /// Parse a warning code from a CLI flag name
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "unused-variable" => Some(WarningCode::UnusedVariable),
            "unused-import" => Some(WarningCode::UnusedImport),
            "unused-parameter" => Some(WarningCode::UnusedParameter),
            "unreachable-code" => Some(WarningCode::UnreachableCode),
            "shadowed-variable" => Some(WarningCode::ShadowedVariable),
            _ => None,
        }
    }
}

/// Warnings emitted during type checking
#[derive(Debug, Clone)]
pub enum CheckWarning {
    /// Variable declared but never used
    UnusedVariable {
        /// Variable name
        name: String,
        /// Location of declaration
        span: Span,
    },

    /// Code after return/throw/break/continue that will never execute
    UnreachableCode {
        /// Location of unreachable code
        span: Span,
    },

    /// Variable in inner scope shadows an outer variable
    ShadowedVariable {
        /// Variable name
        name: String,
        /// Location of original declaration
        original: Span,
        /// Location of shadowing declaration
        shadow: Span,
    },
}

impl CheckWarning {
    /// Get the primary span associated with this warning
    pub fn span(&self) -> Span {
        match self {
            CheckWarning::UnusedVariable { span, .. } => *span,
            CheckWarning::UnreachableCode { span } => *span,
            CheckWarning::ShadowedVariable { shadow, .. } => *shadow,
        }
    }

    /// Get the warning code for this warning
    pub fn code(&self) -> WarningCode {
        match self {
            CheckWarning::UnusedVariable { .. } => WarningCode::UnusedVariable,
            CheckWarning::UnreachableCode { .. } => WarningCode::UnreachableCode,
            CheckWarning::ShadowedVariable { .. } => WarningCode::ShadowedVariable,
        }
    }
}

/// Configuration for which warnings are enabled/disabled
#[derive(Debug, Clone)]
pub struct WarningConfig {
    /// Disabled warning codes (suppressed)
    pub disabled: std::collections::HashSet<WarningCode>,
    /// Warnings promoted to errors
    pub deny: std::collections::HashSet<WarningCode>,
    /// When true, ALL warnings become errors (--strict)
    pub strict: bool,
}

impl Default for WarningConfig {
    fn default() -> Self {
        Self {
            disabled: std::collections::HashSet::new(),
            deny: std::collections::HashSet::new(),
            strict: false,
        }
    }
}

impl WarningConfig {
    /// Strict mode — all warnings are errors
    pub fn strict() -> Self {
        Self { strict: true, ..Self::default() }
    }

    /// Check if a warning should be emitted
    pub fn is_enabled(&self, code: WarningCode) -> bool {
        !self.disabled.contains(&code)
    }

    /// Check if a warning should be treated as an error
    pub fn is_denied(&self, code: WarningCode) -> bool {
        self.strict || self.deny.contains(&code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── WarningCode ─────────────────────────────────────────────────────

    #[test]
    fn test_warning_code_as_str() {
        assert_eq!(WarningCode::UnusedVariable.as_str(), "W1001");
        assert_eq!(WarningCode::UnusedImport.as_str(), "W1002");
        assert_eq!(WarningCode::UnusedParameter.as_str(), "W1003");
        assert_eq!(WarningCode::UnreachableCode.as_str(), "W1004");
        assert_eq!(WarningCode::ShadowedVariable.as_str(), "W1005");
    }

    #[test]
    fn test_warning_code_from_name() {
        assert_eq!(WarningCode::from_name("unused-variable"), Some(WarningCode::UnusedVariable));
        assert_eq!(WarningCode::from_name("unused-import"), Some(WarningCode::UnusedImport));
        assert_eq!(WarningCode::from_name("unused-parameter"), Some(WarningCode::UnusedParameter));
        assert_eq!(WarningCode::from_name("unreachable-code"), Some(WarningCode::UnreachableCode));
        assert_eq!(WarningCode::from_name("shadowed-variable"), Some(WarningCode::ShadowedVariable));
        assert_eq!(WarningCode::from_name("unknown"), None);
        assert_eq!(WarningCode::from_name(""), None);
    }

    // ── CheckWarning ────────────────────────────────────────────────────

    #[test]
    fn test_check_warning_span() {
        let span = Span::new(10, 20, 1, 5);
        let w = CheckWarning::UnusedVariable { name: "x".to_string(), span };
        assert_eq!(w.span().start, 10);
        assert_eq!(w.span().end, 20);

        let w = CheckWarning::UnreachableCode { span };
        assert_eq!(w.span().start, 10);

        let shadow_span = Span::new(30, 40, 3, 1);
        let w = CheckWarning::ShadowedVariable {
            name: "x".to_string(),
            original: span,
            shadow: shadow_span,
        };
        assert_eq!(w.span().start, 30); // Returns shadow span
    }

    #[test]
    fn test_check_warning_code() {
        let span = Span::new(0, 1, 1, 1);
        assert_eq!(CheckWarning::UnusedVariable { name: "x".into(), span }.code(), WarningCode::UnusedVariable);
        assert_eq!(CheckWarning::UnreachableCode { span }.code(), WarningCode::UnreachableCode);
        assert_eq!(
            CheckWarning::ShadowedVariable { name: "x".into(), original: span, shadow: span }.code(),
            WarningCode::ShadowedVariable
        );
    }

    // ── WarningConfig ───────────────────────────────────────────────────

    #[test]
    fn test_warning_config_default_all_enabled() {
        let config = WarningConfig::default();
        assert!(config.is_enabled(WarningCode::UnusedVariable));
        assert!(config.is_enabled(WarningCode::UnreachableCode));
        assert!(config.is_enabled(WarningCode::ShadowedVariable));
        assert!(!config.is_denied(WarningCode::UnusedVariable));
    }

    #[test]
    fn test_warning_config_strict() {
        let config = WarningConfig::strict();
        assert!(config.is_enabled(WarningCode::UnusedVariable));
        assert!(config.is_denied(WarningCode::UnusedVariable));
        assert!(config.is_denied(WarningCode::UnreachableCode));
        assert!(config.is_denied(WarningCode::ShadowedVariable));
    }

    #[test]
    fn test_warning_config_disabled() {
        let mut config = WarningConfig::default();
        config.disabled.insert(WarningCode::UnusedVariable);
        assert!(!config.is_enabled(WarningCode::UnusedVariable));
        assert!(config.is_enabled(WarningCode::UnreachableCode));
    }

    #[test]
    fn test_warning_config_deny() {
        let mut config = WarningConfig::default();
        config.deny.insert(WarningCode::ShadowedVariable);
        assert!(config.is_denied(WarningCode::ShadowedVariable));
        assert!(!config.is_denied(WarningCode::UnusedVariable));
    }

    #[test]
    fn test_warning_config_strict_overrides_deny() {
        let config = WarningConfig::strict();
        // In strict mode, everything is denied even without explicit deny
        assert!(config.is_denied(WarningCode::UnusedVariable));
        assert!(config.is_denied(WarningCode::UnusedImport));
    }
}
