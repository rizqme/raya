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
pub mod early_errors;
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
    Raya,
    /// TypeScript mode: configured by tsconfig compilerOptions.
    Ts,
    /// JS-like dynamic semantics (`any`, bare-let flow widening, JSObject fallback).
    Js,
}

/// TS-specific effective semantic flags used by binder/checker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TsTypeFlags {
    pub strict: bool,
    pub no_implicit_any: bool,
    pub no_implicit_this: bool,
    pub strict_null_checks: bool,
    pub strict_property_initialization: bool,
    pub use_unknown_in_catch_variables: bool,
    pub exact_optional_property_types: bool,
    pub no_unchecked_indexed_access: bool,
    pub strict_function_types: bool,
}

impl Default for TsTypeFlags {
    fn default() -> Self {
        Self {
            strict: true,
            no_implicit_any: true,
            no_implicit_this: true,
            strict_null_checks: true,
            strict_property_initialization: true,
            use_unknown_in_catch_variables: true,
            exact_optional_property_types: true,
            no_unchecked_indexed_access: true,
            strict_function_types: true,
        }
    }
}

/// Effective checker behavior policy derived from mode + TS flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckerPolicy {
    pub allow_explicit_any: bool,
    pub allow_implicit_any: bool,
    pub allow_bare_let: bool,
    pub allow_js_dynamic_fallback: bool,
    pub enforce_unknown_not_actionable: bool,
    pub strict_assignability: bool,
    pub no_implicit_this: bool,
    pub strict_property_initialization: bool,
    pub use_unknown_in_catch_variables: bool,
    pub exact_optional_property_types: bool,
    pub no_unchecked_indexed_access: bool,
    pub strict_function_types: bool,
}

impl CheckerPolicy {
    pub fn for_mode(mode: TypeSystemMode) -> Self {
        match mode {
            TypeSystemMode::Raya => Self {
                allow_explicit_any: false,
                allow_implicit_any: false,
                allow_bare_let: false,
                allow_js_dynamic_fallback: false,
                enforce_unknown_not_actionable: true,
                strict_assignability: true,
                no_implicit_this: true,
                strict_property_initialization: true,
                use_unknown_in_catch_variables: true,
                exact_optional_property_types: true,
                no_unchecked_indexed_access: true,
                strict_function_types: true,
            },
            TypeSystemMode::Ts => Self::for_ts(TsTypeFlags::default()),
            TypeSystemMode::Js => Self {
                allow_explicit_any: true,
                allow_implicit_any: true,
                allow_bare_let: true,
                allow_js_dynamic_fallback: true,
                enforce_unknown_not_actionable: false,
                strict_assignability: false,
                no_implicit_this: false,
                strict_property_initialization: false,
                use_unknown_in_catch_variables: false,
                exact_optional_property_types: false,
                no_unchecked_indexed_access: false,
                strict_function_types: false,
            },
        }
    }

    pub fn for_ts(flags: TsTypeFlags) -> Self {
        let strict = flags.strict;
        let no_implicit_any = strict || flags.no_implicit_any;
        let no_implicit_this = strict || flags.no_implicit_this;
        let strict_null_checks = strict || flags.strict_null_checks;
        let strict_property_initialization = strict || flags.strict_property_initialization;
        let use_unknown_in_catch_variables = strict || flags.use_unknown_in_catch_variables;
        let strict_function_types = strict || flags.strict_function_types;

        Self {
            allow_explicit_any: true,
            allow_implicit_any: !no_implicit_any,
            allow_bare_let: !no_implicit_any,
            allow_js_dynamic_fallback: false,
            enforce_unknown_not_actionable: strict_null_checks,
            strict_assignability: strict_null_checks,
            no_implicit_this,
            strict_property_initialization,
            use_unknown_in_catch_variables,
            exact_optional_property_types: flags.exact_optional_property_types,
            no_unchecked_indexed_access: flags.no_unchecked_indexed_access,
            strict_function_types,
        }
    }
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
pub use early_errors::check_early_errors;
pub use error::{BindError, CheckError, CheckWarning, SoftDiagnostic, WarningCode, WarningConfig};
pub use exhaustiveness::ExhaustivenessResult;
pub use narrowing::TypeEnv;
pub use symbols::{Scope, ScopeId, ScopeKind, Symbol, SymbolFlags, SymbolKind, SymbolTable};
pub use type_guards::TypeGuard;
