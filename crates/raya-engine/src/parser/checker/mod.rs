//! Raya Type Checker
//!
//! Type checking and control flow analysis for Raya.
//!
//! This crate provides:
//! - Symbol tables with scope management
//! - Name binding (AST → Symbol Table)
//! - Type checking for expressions and statements
//! - Control flow-based type narrowing
//! - Exhaustiveness checking for discriminated unions
//! - Closure capture analysis

pub mod symbols;
pub mod binder;
pub mod checker;
pub mod error;
pub mod type_guards;
pub mod narrowing;
pub mod exhaustiveness;
pub mod diagnostic;
pub mod captures;
pub mod builtins;

// Re-export main types
pub use symbols::{Symbol, SymbolKind, SymbolTable, SymbolFlags, Scope, ScopeId, ScopeKind};
pub use binder::Binder;
pub use checker::{TypeChecker, InferredTypes, CheckResult};
pub use error::{BindError, CheckError, CheckWarning, WarningCode, WarningConfig};
pub use type_guards::TypeGuard;
pub use narrowing::TypeEnv;
pub use exhaustiveness::ExhaustivenessResult;
pub use diagnostic::{Diagnostic, ErrorCode, SimpleFiles, create_files};
pub use captures::{CaptureInfo, ClosureCaptures, ClosureId, ModuleCaptureInfo};
pub use builtins::{BuiltinSignatures, BuiltinClass, BuiltinFunction, BuiltinMethod, BuiltinProperty};
