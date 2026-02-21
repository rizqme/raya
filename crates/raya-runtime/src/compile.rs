//! Source compilation pipeline.
//!
//! Parse → Bind → TypeCheck → Compile to bytecode.

use raya_engine::compiler::{Compiler, Module};
use raya_engine::parser::checker::{Binder, TypeChecker, ScopeId, CheckWarning, BindError, CheckError};
use raya_engine::parser::{Interner, LexError, ParseError, Parser, TypeContext};

use crate::builtins;
use crate::error::RuntimeError;

/// Options controlling compilation output.
#[derive(Debug, Clone)]
pub struct CompileOptions {
    /// Include source map (bytecode offset → source location) in output.
    pub sourcemap: bool,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self { sourcemap: false }
    }
}

/// Diagnostics returned from a check-only pass (no codegen).
pub struct CheckDiagnostics {
    /// Type checking errors
    pub errors: Vec<CheckError>,
    /// Binding errors
    pub bind_errors: Vec<BindError>,
    /// Warnings from type checking
    pub warnings: Vec<CheckWarning>,
    /// Full source (builtins + stdlib + user code)
    pub source: String,
    /// Byte offset where user code begins in `source`
    pub user_offset: usize,
}

/// Compile Raya source code to a bytecode module.
///
/// Prepends builtin class sources and standard library sources so that
/// user code can reference Map, Set, Channel, Logger, Math, etc.
pub fn compile_source(source: &str) -> Result<(Module, Interner), RuntimeError> {
    let builtin_src = builtins::builtin_sources();
    let std_src = builtins::std_sources();
    let user_offset = builtin_src.len() + 1 + std_src.len() + 1;
    let full_source = format!("{}\n{}\n{}", builtin_src, std_src, source);
    let prefix_lines = full_source[..user_offset].bytes().filter(|&b| b == b'\n').count();

    // Parse
    let parser = Parser::new(&full_source)
        .map_err(|errors| RuntimeError::Lex(format_lex_errors(&errors, prefix_lines)))?;
    let (ast, interner) = parser
        .parse()
        .map_err(|errors| RuntimeError::Parse(format_parse_errors(&errors, prefix_lines)))?;

    // Bind (creates symbol table)
    let mut type_ctx = TypeContext::new();
    let mut binder = Binder::new(&mut type_ctx, &interner);

    // Register only intrinsics (__NATIVE_CALL, etc.) — builtin class sources
    // are included in the source text, so their types come from parsing.
    let empty_sigs: Vec<raya_engine::parser::checker::BuiltinSignatures> = vec![];
    binder.register_builtins(&empty_sigs);
    binder.skip_top_level_duplicate_detection();

    let mut symbols = binder
        .bind_module(&ast)
        .map_err(|errors| RuntimeError::TypeCheck(
            errors.iter().map(|e| format_bind_error(e, user_offset, &full_source)).collect::<Vec<_>>().join("\n")
        ))?;

    // Type check
    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let check_result = checker
        .check_module(&ast)
        .map_err(|errors| RuntimeError::TypeCheck(
            errors.iter().map(|e| format_check_error(e, user_offset, &full_source)).collect::<Vec<_>>().join("\n")
        ))?;

    // Apply inferred types to symbol table
    for ((scope_id, name), ty) in check_result.inferred_types {
        symbols.update_type(ScopeId(scope_id), &name, ty);
    }

    // Compile via IR pipeline
    let compiler = Compiler::new(type_ctx, &interner)
        .with_expr_types(check_result.expr_types);
    let bytecode = compiler.compile_via_ir(&ast)?;

    Ok((bytecode, interner))
}

/// Compile Raya source code to a bytecode module with options.
///
/// Same as `compile_source` but allows controlling compilation output
/// (e.g., source map generation).
pub fn compile_source_with_options(
    source: &str,
    options: &CompileOptions,
) -> Result<(Module, Interner), RuntimeError> {
    let builtin_src = builtins::builtin_sources();
    let std_src = builtins::std_sources();
    let user_offset = builtin_src.len() + 1 + std_src.len() + 1;
    let full_source = format!("{}\n{}\n{}", builtin_src, std_src, source);
    let prefix_lines = full_source[..user_offset].bytes().filter(|&b| b == b'\n').count();

    // Parse
    let parser = Parser::new(&full_source)
        .map_err(|errors| RuntimeError::Lex(format_lex_errors(&errors, prefix_lines)))?;
    let (ast, interner) = parser
        .parse()
        .map_err(|errors| RuntimeError::Parse(format_parse_errors(&errors, prefix_lines)))?;

    // Bind (creates symbol table)
    let mut type_ctx = TypeContext::new();
    let mut binder = Binder::new(&mut type_ctx, &interner);
    let empty_sigs: Vec<raya_engine::parser::checker::BuiltinSignatures> = vec![];
    binder.register_builtins(&empty_sigs);
    binder.skip_top_level_duplicate_detection();

    let mut symbols = binder
        .bind_module(&ast)
        .map_err(|errors| RuntimeError::TypeCheck(
            errors.iter().map(|e| format_bind_error(e, user_offset, &full_source)).collect::<Vec<_>>().join("\n")
        ))?;

    // Type check
    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let check_result = checker
        .check_module(&ast)
        .map_err(|errors| RuntimeError::TypeCheck(
            errors.iter().map(|e| format_check_error(e, user_offset, &full_source)).collect::<Vec<_>>().join("\n")
        ))?;

    // Apply inferred types to symbol table
    for ((scope_id, name), ty) in check_result.inferred_types {
        symbols.update_type(ScopeId(scope_id), &name, ty);
    }

    // Compile via IR pipeline
    let compiler = Compiler::new(type_ctx, &interner)
        .with_expr_types(check_result.expr_types)
        .with_sourcemap(options.sourcemap);
    let bytecode = compiler.compile_via_ir(&ast)?;

    Ok((bytecode, interner))
}

/// Type-check Raya source code without generating bytecode.
///
/// Runs Parse → Bind → TypeCheck and returns all errors and warnings.
/// Does not perform IR lowering, optimization, or code generation.
pub fn check_source(source: &str) -> Result<CheckDiagnostics, RuntimeError> {
    let builtin_src = builtins::builtin_sources();
    let std_src = builtins::std_sources();
    let user_offset = builtin_src.len() + 1 + std_src.len() + 1; // +1 for \n separators

    let full_source = format!("{}\n{}\n{}", builtin_src, std_src, source);

    let prefix_lines = full_source[..user_offset].bytes().filter(|&b| b == b'\n').count();

    // Parse
    let parser = Parser::new(&full_source)
        .map_err(|errors| RuntimeError::Lex(format_lex_errors(&errors, prefix_lines)))?;
    let (ast, interner) = parser
        .parse()
        .map_err(|errors| RuntimeError::Parse(format_parse_errors(&errors, prefix_lines)))?;

    // Bind
    let mut type_ctx = TypeContext::new();
    let mut binder = Binder::new(&mut type_ctx, &interner);
    let empty_sigs: Vec<raya_engine::parser::checker::BuiltinSignatures> = vec![];
    binder.register_builtins(&empty_sigs);
    binder.skip_top_level_duplicate_detection();

    let bind_result = binder.bind_module(&ast);

    let (bind_errors, check_errors, warnings) = match bind_result {
        Err(bind_errs) => {
            // Binding failed — return bind errors, no type checking
            (bind_errs, vec![], vec![])
        }
        Ok(symbols) => {
            // Binding succeeded — run type checker
            let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
            match checker.check_module(&ast) {
                Ok(result) => (vec![], vec![], result.warnings),
                Err(check_errs) => (vec![], check_errs, vec![]),
            }
        }
    };

    Ok(CheckDiagnostics {
        errors: check_errors,
        bind_errors,
        warnings,
        source: full_source,
        user_offset,
    })
}

/// Compute the user-relative line number for a byte offset in the full source.
fn compute_user_line(byte_offset: usize, user_offset: usize, full_source: &str) -> usize {
    if byte_offset < user_offset {
        return 0;
    }
    let relative_offset = byte_offset - user_offset;
    let user_src = &full_source[user_offset..];
    user_src[..relative_offset.min(user_src.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count() + 1
}

/// Format a CheckError with line number relative to user code.
fn format_check_error(error: &CheckError, user_offset: usize, full_source: &str) -> String {
    let span = error.span();
    let line = compute_user_line(span.start as usize, user_offset, full_source);
    if line > 0 {
        format!("{} (line {})", error, line)
    } else {
        error.to_string()
    }
}

/// Format a BindError with line number relative to user code.
fn format_bind_error(error: &BindError, user_offset: usize, full_source: &str) -> String {
    let span = error.span();
    let line = compute_user_line(span.start as usize, user_offset, full_source);
    if line > 0 {
        format!("{} (line {})", error, line)
    } else {
        error.to_string()
    }
}

/// Format lex errors with line numbers relative to user code.
fn format_lex_errors(errors: &[LexError], prefix_lines: usize) -> String {
    errors.iter().map(|e| {
        let span = e.span();
        let user_line = (span.line as usize).saturating_sub(prefix_lines);
        if user_line > 0 {
            format!("{} (line {}:{})", e.description(), user_line, span.column)
        } else {
            e.to_string()
        }
    }).collect::<Vec<_>>().join("\n")
}

/// Format parse errors with line numbers relative to user code.
fn format_parse_errors(errors: &[ParseError], prefix_lines: usize) -> String {
    errors.iter().map(|e| {
        let user_line = (e.span.line as usize).saturating_sub(prefix_lines);
        if user_line > 0 {
            let mut msg = format!("{} (line {}:{})", e.message, user_line, e.span.column);
            if let Some(suggestion) = &e.suggestion {
                msg.push_str(&format!("\n  Suggestion: {}", suggestion));
            }
            msg
        } else {
            e.to_string()
        }
    }).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_valid_source() {
        let diag = check_source("let x = 1 + 2;").unwrap();
        assert!(diag.bind_errors.is_empty(), "Expected no bind errors");
        assert!(diag.errors.is_empty(), "Expected no check errors");
    }

    #[test]
    fn test_check_returns_user_offset() {
        let diag = check_source("let x = 1;").unwrap();
        assert!(diag.user_offset > 0, "user_offset should be > 0 (builtins are prepended)");
    }

    #[test]
    fn test_check_unused_variable_warning() {
        let diag = check_source("let x = 1;").unwrap();
        // The variable `x` is unused, so we should get a warning
        let unused_vars: Vec<_> = diag.warnings.iter().filter(|w| {
            matches!(w, CheckWarning::UnusedVariable { name, .. } if name == "x")
        }).collect();
        assert!(!unused_vars.is_empty(), "Expected unused variable warning for 'x'");
    }

    #[test]
    fn test_check_underscore_prefix_no_warning() {
        let diag = check_source("let _x = 1;").unwrap();
        let unused_vars: Vec<_> = diag.warnings.iter().filter(|w| {
            matches!(w, CheckWarning::UnusedVariable { name, .. } if name == "_x")
        }).collect();
        assert!(unused_vars.is_empty(), "Underscore-prefixed variables should not generate warnings");
    }

    #[test]
    fn test_check_source_full_source_includes_builtins() {
        let diag = check_source("let x = 1;").unwrap();
        // The full source should include builtin classes
        assert!(diag.source.contains("class Map"), "Full source should include Map builtin");
    }

    #[test]
    fn test_check_empty_source() {
        let diag = check_source("").unwrap();
        assert!(diag.bind_errors.is_empty());
        assert!(diag.errors.is_empty());
    }

    #[test]
    fn test_compile_source_still_works() {
        // Ensure compile_source is unaffected
        let result = compile_source("return 42;");
        assert!(result.is_ok(), "compile_source should still work");
    }
}
