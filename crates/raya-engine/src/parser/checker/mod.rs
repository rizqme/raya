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

pub mod binder;
pub mod builtins;
pub mod captures;
pub mod checker;
pub mod diagnostic;
pub mod error;
pub mod exhaustiveness;
pub mod narrowing;
pub mod symbols;
pub mod type_guards;

/// Type system behavior mode for checker/binder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TypeSystemMode {
    /// Raya strict mode: no `any`, stricter inference/usage rules.
    #[default]
    Strict,
    /// Strict semantics but explicit/implicit `any` is permitted.
    AllowAny,
    /// JS-like dynamic semantics (`any`, bare-let flow widening, JSObject fallback).
    JsMode,
}

// Re-export main types
pub use binder::Binder;
pub use builtins::{
    BuiltinClass, BuiltinFunction, BuiltinMethod, BuiltinProperty, BuiltinPropertyDescriptor,
    BuiltinSignatures,
};
pub use captures::{CaptureInfo, ClosureCaptures, ClosureId, ModuleCaptureInfo};
pub use checker::{CheckResult, InferredTypes, TypeChecker};
pub use diagnostic::{create_files, Diagnostic, ErrorCode, SimpleFiles};
pub use error::{BindError, CheckError, CheckWarning, WarningCode, WarningConfig};
pub use exhaustiveness::ExhaustivenessResult;
pub use narrowing::TypeEnv;
pub use symbols::{Scope, ScopeId, ScopeKind, Symbol, SymbolFlags, SymbolKind, SymbolTable};
pub use type_guards::TypeGuard;
