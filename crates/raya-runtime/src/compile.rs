//! Source compilation pipeline.
//!
//! Parse → Bind → TypeCheck → Compile to bytecode.

use raya_engine::compiler::{Compiler, Module};
use raya_engine::parser::ast::Statement;
use raya_engine::parser::checker::{
    BindError, Binder, CheckError, CheckWarning, ScopeId, TypeChecker, TypeSystemMode,
};
use raya_engine::parser::{Interner, LexError, ParseError, Parser, TypeContext};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::builtin_manifest;
use crate::builtins;
use crate::error::RuntimeError;
use crate::std_prelude::build_std_prelude;
use crate::BuiltinMode;

/// Checker behavior mode, independent from builtin API surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TypeMode {
    /// Raya strict typing (`any` forbidden, untyped vars forbidden).
    #[default]
    Strict,
    /// Strict typing with explicit `any` enabled; still forbids untyped vars.
    AllowAny,
    /// JS-like dynamic typing (`any` + untyped vars + widening/escalation).
    JsMode,
}

#[inline]
pub fn default_type_mode_for_builtin(mode: BuiltinMode) -> TypeMode {
    match mode {
        BuiltinMode::RayaStrict => TypeMode::Strict,
        BuiltinMode::NodeCompat => TypeMode::JsMode,
    }
}

#[inline]
fn type_system_mode(mode: TypeMode) -> TypeSystemMode {
    match mode {
        TypeMode::Strict => TypeSystemMode::Strict,
        TypeMode::AllowAny => TypeSystemMode::AllowAny,
        TypeMode::JsMode => TypeSystemMode::JsMode,
    }
}

/// Options controlling compilation output.
#[derive(Debug, Clone, Default)]
pub struct CompileOptions {
    /// Include source map (bytecode offset → source location) in output.
    pub sourcemap: bool,
}

/// Diagnostics returned from a check-only pass (no codegen).
pub struct CheckDiagnostics {
    /// Type checking errors
    pub errors: Vec<CheckError>,
    /// Binding errors
    pub bind_errors: Vec<BindError>,
    /// Warnings from type checking
    pub warnings: Vec<CheckWarning>,
    /// Full source (builtins + user code)
    pub source: String,
    /// Byte offset where user code begins in `source`
    pub user_offset: usize,
}

/// Compile Raya source code to a bytecode module.
///
/// Prepends builtin class sources so user code can reference core globals
/// (Map, Set, Buffer, Date, Promise, etc.). Standard library modules are not
/// auto-injected; they must be imported explicitly.
pub fn compile_source(source: &str) -> Result<(Module, Interner), RuntimeError> {
    compile_source_with_mode(source, BuiltinMode::RayaStrict)
}

/// Compile Raya source code to a bytecode module with builtin API mode.
pub fn compile_source_with_mode(
    source: &str,
    builtin_mode: BuiltinMode,
) -> Result<(Module, Interner), RuntimeError> {
    compile_source_with_modes(
        source,
        builtin_mode,
        default_type_mode_for_builtin(builtin_mode),
    )
}

/// Compile Raya source code to a bytecode module with explicit builtin + type modes.
pub fn compile_source_with_modes(
    source: &str,
    builtin_mode: BuiltinMode,
    type_mode: TypeMode,
) -> Result<(Module, Interner), RuntimeError> {
    precheck_user_top_level_duplicates(source)?;
    precheck_node_compat_symbol_usage(source, builtin_mode)?;

    let std_prelude = build_std_prelude(source)?;
    let builtin_src = builtins::builtin_sources_for_mode(builtin_mode);
    let builtin_offset = builtin_src.len() + 1;
    let prelude_src = if std_prelude.prelude_source.is_empty() {
        builtin_src.to_string()
    } else {
        format!("{}\n{}", builtin_src, std_prelude.prelude_source)
    };
    let user_offset = prelude_src.len() + 1;
    let full_source = format!("{}\n{}", prelude_src, std_prelude.rewritten_user_source);
    if let Ok(path) = std::env::var("RAYA_DEBUG_DUMP_SOURCE") {
        let _ = std::fs::write(path, &full_source);
    }
    let prefix_lines = full_source[..user_offset]
        .bytes()
        .filter(|&b| b == b'\n')
        .count();

    // Parse
    let parser = Parser::new(&full_source)
        .map_err(|errors| RuntimeError::Lex(format_lex_errors(&errors, prefix_lines)))?;
    let (ast, interner) = parser
        .parse()
        .map_err(|errors| RuntimeError::Parse(format_parse_errors(&errors, prefix_lines)))?;

    // Bind (creates symbol table)
    let mut type_ctx = TypeContext::new();
    let mut binder = Binder::new(&mut type_ctx, &interner).with_mode(type_system_mode(type_mode));

    // Register only intrinsics (__NATIVE_CALL, etc.) — builtin class sources
    // are included in the source text, so their types come from parsing.
    let empty_sigs: Vec<raya_engine::parser::checker::BuiltinSignatures> = vec![];
    binder.register_builtins(&empty_sigs);
    binder.skip_top_level_duplicate_detection();

    let mut symbols = binder.bind_module(&ast).map_err(|errors| {
        RuntimeError::TypeCheck(
            errors
                .iter()
                .map(|e| format_bind_error(e, user_offset, &full_source))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })?;

    // Type check
    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner)
        .with_mode(type_system_mode(type_mode))
        .with_skip_class_bodies_before(builtin_offset);
    let check_result = checker.check_module(&ast).map_err(|errors| {
        RuntimeError::TypeCheck(
            errors
                .iter()
                .map(|e| format_check_error(e, user_offset, &full_source))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })?;

    // Apply inferred types to symbol table
    for ((scope_id, name), ty) in check_result.inferred_types {
        symbols.update_type(ScopeId(scope_id), &name, ty);
    }

    // Compile via IR pipeline
    let compiler = Compiler::new(type_ctx, &interner)
        .with_expr_types(check_result.expr_types)
        .with_js_this_binding_compat(true);
    let bytecode = compiler.compile_via_ir(&ast)?;

    Ok((bytecode, interner))
}

/// Load a Raya file and inline all transitive local (`./`, `../`) imports.
///
/// This keeps runtime compilation compatible with the single-module pipeline
/// while supporting multi-file projects that use local imports.
pub fn load_source_with_local_imports(path: &Path) -> Result<String, RuntimeError> {
    let mut visited = HashSet::new();
    let mut ordered_sources = Vec::new();
    collect_local_sources(path, &mut visited, &mut ordered_sources)?;
    Ok(ordered_sources.join("\n"))
}

/// Compile Raya source code to a bytecode module with options.
///
/// Same as `compile_source` but allows controlling compilation output
/// (e.g., source map generation).
pub fn compile_source_with_options(
    source: &str,
    options: &CompileOptions,
) -> Result<(Module, Interner), RuntimeError> {
    compile_source_with_options_and_mode(source, options, BuiltinMode::RayaStrict)
}

/// Compile source with explicit compile options and builtin compatibility mode.
pub fn compile_source_with_options_and_mode(
    source: &str,
    options: &CompileOptions,
    builtin_mode: BuiltinMode,
) -> Result<(Module, Interner), RuntimeError> {
    compile_source_with_options_and_modes(
        source,
        options,
        builtin_mode,
        default_type_mode_for_builtin(builtin_mode),
    )
}

/// Compile source with explicit compile options, builtin mode, and type mode.
pub fn compile_source_with_options_and_modes(
    source: &str,
    options: &CompileOptions,
    builtin_mode: BuiltinMode,
    type_mode: TypeMode,
) -> Result<(Module, Interner), RuntimeError> {
    precheck_user_top_level_duplicates(source)?;
    precheck_node_compat_symbol_usage(source, builtin_mode)?;

    let std_prelude = build_std_prelude(source)?;
    let builtin_src = builtins::builtin_sources_for_mode(builtin_mode);
    let builtin_offset = builtin_src.len() + 1;
    let prelude_src = if std_prelude.prelude_source.is_empty() {
        builtin_src.to_string()
    } else {
        format!("{}\n{}", builtin_src, std_prelude.prelude_source)
    };
    let user_offset = prelude_src.len() + 1;
    let full_source = format!("{}\n{}", prelude_src, std_prelude.rewritten_user_source);
    if let Ok(path) = std::env::var("RAYA_DEBUG_DUMP_SOURCE") {
        let _ = std::fs::write(path, &full_source);
    }
    let prefix_lines = full_source[..user_offset]
        .bytes()
        .filter(|&b| b == b'\n')
        .count();

    // Parse
    let parser = Parser::new(&full_source)
        .map_err(|errors| RuntimeError::Lex(format_lex_errors(&errors, prefix_lines)))?;
    let (ast, interner) = parser
        .parse()
        .map_err(|errors| RuntimeError::Parse(format_parse_errors(&errors, prefix_lines)))?;

    // Bind (creates symbol table)
    let mut type_ctx = TypeContext::new();
    let mut binder = Binder::new(&mut type_ctx, &interner).with_mode(type_system_mode(type_mode));
    let empty_sigs: Vec<raya_engine::parser::checker::BuiltinSignatures> = vec![];
    binder.register_builtins(&empty_sigs);
    binder.skip_top_level_duplicate_detection();

    let mut symbols = binder.bind_module(&ast).map_err(|errors| {
        RuntimeError::TypeCheck(
            errors
                .iter()
                .map(|e| format_bind_error(e, user_offset, &full_source))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })?;

    // Type check
    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner)
        .with_mode(type_system_mode(type_mode))
        .with_skip_class_bodies_before(builtin_offset);
    let check_result = checker.check_module(&ast).map_err(|errors| {
        RuntimeError::TypeCheck(
            errors
                .iter()
                .map(|e| format_check_error(e, user_offset, &full_source))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })?;

    // Apply inferred types to symbol table
    for ((scope_id, name), ty) in check_result.inferred_types {
        symbols.update_type(ScopeId(scope_id), &name, ty);
    }

    // Compile via IR pipeline
    let compiler = Compiler::new(type_ctx, &interner)
        .with_expr_types(check_result.expr_types)
        .with_sourcemap(options.sourcemap)
        .with_js_this_binding_compat(true);
    let bytecode = compiler.compile_via_ir(&ast)?;

    Ok((bytecode, interner))
}

/// Type-check Raya source code without generating bytecode.
///
/// Runs Parse → Bind → TypeCheck and returns all errors and warnings.
/// Does not perform IR lowering, optimization, or code generation.
pub fn check_source(source: &str) -> Result<CheckDiagnostics, RuntimeError> {
    check_source_with_mode(source, BuiltinMode::RayaStrict)
}

/// Type-check source using a specific builtin compatibility mode.
pub fn check_source_with_mode(
    source: &str,
    builtin_mode: BuiltinMode,
) -> Result<CheckDiagnostics, RuntimeError> {
    check_source_with_modes(
        source,
        builtin_mode,
        default_type_mode_for_builtin(builtin_mode),
    )
}

/// Type-check source using explicit builtin compatibility + type mode.
pub fn check_source_with_modes(
    source: &str,
    builtin_mode: BuiltinMode,
    type_mode: TypeMode,
) -> Result<CheckDiagnostics, RuntimeError> {
    precheck_user_top_level_duplicates(source)?;
    precheck_node_compat_symbol_usage(source, builtin_mode)?;

    let std_prelude = build_std_prelude(source)?;
    let builtin_src = builtins::builtin_sources_for_mode(builtin_mode);
    let builtin_offset = builtin_src.len() + 1;
    let prelude_src = if std_prelude.prelude_source.is_empty() {
        builtin_src.to_string()
    } else {
        format!("{}\n{}", builtin_src, std_prelude.prelude_source)
    };
    let user_offset = prelude_src.len() + 1; // +1 for \n separator

    let full_source = format!("{}\n{}", prelude_src, std_prelude.rewritten_user_source);

    let prefix_lines = full_source[..user_offset]
        .bytes()
        .filter(|&b| b == b'\n')
        .count();

    // Parse
    let parser = Parser::new(&full_source)
        .map_err(|errors| RuntimeError::Lex(format_lex_errors(&errors, prefix_lines)))?;
    let (ast, interner) = parser
        .parse()
        .map_err(|errors| RuntimeError::Parse(format_parse_errors(&errors, prefix_lines)))?;

    // Bind
    let mut type_ctx = TypeContext::new();
    let mut binder = Binder::new(&mut type_ctx, &interner).with_mode(type_system_mode(type_mode));
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
            let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner)
                .with_mode(type_system_mode(type_mode))
                .with_skip_class_bodies_before(builtin_offset);
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
        .count()
        + 1
}

/// Format a CheckError with line number relative to user code.
fn format_check_error(error: &CheckError, user_offset: usize, full_source: &str) -> String {
    let span = error.span();
    let line = compute_user_line(span.start, user_offset, full_source);
    if line > 0 {
        format!("{} (line {})", error, line)
    } else {
        let abs_line = full_source[..span.start.min(full_source.len())]
            .bytes()
            .filter(|&b| b == b'\n')
            .count()
            + 1;
        format!("{} (prelude line {})", error, abs_line)
    }
}

/// Format a BindError with line number relative to user code.
fn format_bind_error(error: &BindError, user_offset: usize, full_source: &str) -> String {
    let span = error.span();
    let line = compute_user_line(span.start, user_offset, full_source);
    if line > 0 {
        format!("{} (line {})", error, line)
    } else {
        let abs_line = full_source[..span.start.min(full_source.len())]
            .bytes()
            .filter(|&b| b == b'\n')
            .count()
            + 1;
        format!("{} (prelude line {})", error, abs_line)
    }
}

/// Format lex errors with line numbers relative to user code.
fn format_lex_errors(errors: &[LexError], prefix_lines: usize) -> String {
    errors
        .iter()
        .map(|e| {
            let span = e.span();
            let user_line = (span.line as usize).saturating_sub(prefix_lines);
            if user_line > 0 {
                format!("{} (line {}:{})", e.description(), user_line, span.column)
            } else {
                e.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Format parse errors with line numbers relative to user code.
fn format_parse_errors(errors: &[ParseError], prefix_lines: usize) -> String {
    errors
        .iter()
        .map(|e| {
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
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Detect duplicate top-level declarations within user source before builtin/std
/// sources are prepended.
///
/// Binder duplicate detection is disabled in the main pipeline to allow user
/// symbols to shadow builtins. This precheck still rejects duplicates that
/// occur entirely within user code (e.g., repeated pasted REPL blocks).
fn precheck_user_top_level_duplicates(source: &str) -> Result<(), RuntimeError> {
    let parser = Parser::new(source).map_err(|errors| {
        RuntimeError::Lex(
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })?;
    let (ast, interner) = parser.parse().map_err(|errors| {
        RuntimeError::Parse(
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })?;

    let mut seen_functions: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut seen_classes: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for stmt in &ast.statements {
        match stmt {
            Statement::FunctionDecl(f) => {
                let name = interner.resolve(f.name.name).to_string();
                let line = f.name.span.line as usize;
                if let Some(original_line) = seen_functions.insert(name.clone(), line) {
                    return Err(RuntimeError::TypeCheck(format!(
                        "Duplicate function declaration '{}': first at line {}, again at line {}",
                        name, original_line, line
                    )));
                }
            }
            Statement::ClassDecl(c) => {
                let name = interner.resolve(c.name.name).to_string();
                let line = c.name.span.line as usize;
                if let Some(original_line) = seen_classes.insert(name.clone(), line) {
                    return Err(RuntimeError::TypeCheck(format!(
                        "Duplicate class declaration '{}': first at line {}, again at line {}",
                        name, original_line, line
                    )));
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn precheck_node_compat_symbol_usage(
    source: &str,
    builtin_mode: BuiltinMode,
) -> Result<(), RuntimeError> {
    if builtin_mode != BuiltinMode::RayaStrict {
        return Ok(());
    }

    if let Some(found) = builtin_manifest::find_first_node_compat_symbol_usage(source) {
        return Err(RuntimeError::TypeCheck(format!(
            "E_STRICT_NODE_COMPAT_SYMBOL: '{}' is node-compat-only (line {}). {}",
            found.symbol, found.line, found.hint
        )));
    }

    Ok(())
}

fn collect_local_sources(
    path: &Path,
    visited: &mut HashSet<PathBuf>,
    ordered_sources: &mut Vec<String>,
) -> Result<(), RuntimeError> {
    let canonical = path.canonicalize()?;
    if visited.contains(&canonical) {
        return Ok(());
    }
    visited.insert(canonical.clone());

    let source = std::fs::read_to_string(&canonical)?;
    let parser =
        Parser::new(&source).map_err(|errors| RuntimeError::Lex(format_lex_errors(&errors, 0)))?;
    let (ast, interner) = parser
        .parse()
        .map_err(|errors| RuntimeError::Parse(format_parse_errors(&errors, 0)))?;

    for stmt in &ast.statements {
        if let Statement::ImportDecl(import) = stmt {
            let specifier = interner.resolve(import.source.value).to_string();
            if is_local_import(&specifier) {
                let resolved = resolve_local_import(&canonical, &specifier)?;
                collect_local_sources(&resolved, visited, ordered_sources)?;
            }
        }
    }

    ordered_sources.push(source);
    Ok(())
}

fn is_local_import(specifier: &str) -> bool {
    specifier.starts_with("./") || specifier.starts_with("../")
}

fn resolve_local_import(from_file: &Path, specifier: &str) -> Result<PathBuf, RuntimeError> {
    let base_dir = from_file.parent().ok_or_else(|| {
        RuntimeError::Dependency(format!(
            "Cannot resolve import '{}' from '{}': no parent directory",
            specifier,
            from_file.display()
        ))
    })?;

    let candidate = base_dir.join(specifier);
    let mut tried = Vec::new();

    if candidate.extension().is_some() {
        tried.push(candidate.clone());
        if candidate.exists() {
            return Ok(candidate);
        }
    } else {
        let with_ext = candidate.with_extension("raya");
        tried.push(with_ext.clone());
        if with_ext.exists() {
            return Ok(with_ext);
        }
    }

    let index_candidate = candidate.join("index.raya");
    tried.push(index_candidate.clone());
    if index_candidate.exists() {
        return Ok(index_candidate);
    }

    Err(RuntimeError::Dependency(format!(
        "Module not found: '{}' from '{}'. Tried: {}",
        specifier,
        from_file.display(),
        tried
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )))
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
        assert!(
            diag.user_offset > 0,
            "user_offset should be > 0 (builtins are prepended)"
        );
    }

    #[test]
    fn test_check_unused_variable_warning() {
        let diag = check_source("let x = 1;").unwrap();
        // The variable `x` is unused, so we should get a warning
        let unused_vars: Vec<_> = diag
            .warnings
            .iter()
            .filter(|w| matches!(w, CheckWarning::UnusedVariable { name, .. } if name == "x"))
            .collect();
        assert!(
            !unused_vars.is_empty(),
            "Expected unused variable warning for 'x'"
        );
    }

    #[test]
    fn test_check_underscore_prefix_no_warning() {
        let diag = check_source("let _x = 1;").unwrap();
        let unused_vars: Vec<_> = diag
            .warnings
            .iter()
            .filter(|w| matches!(w, CheckWarning::UnusedVariable { name, .. } if name == "_x"))
            .collect();
        assert!(
            unused_vars.is_empty(),
            "Underscore-prefixed variables should not generate warnings"
        );
    }

    #[test]
    fn test_check_source_full_source_includes_builtins() {
        let diag = check_source("let x = 1;").unwrap();
        // The full source should include builtin classes
        assert!(
            diag.source.contains("class Map"),
            "Full source should include Map builtin"
        );
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

    #[test]
    fn test_node_path_import_is_supported() {
        let result = compile_source(
            r#"
            import path from "node:path";
            return path.join("a", "b");
            "#,
        );
        assert!(result.is_ok(), "node:path should map to std:path");
    }

    #[test]
    fn test_std_pm_import_compiles_with_transitive_named_exports() {
        let result = compile_source(
            r#"
            import pm from "std:pm";
            return pm != null;
            "#,
        );
        assert!(
            result.is_ok(),
            "std:pm import should compile with transitive std dependencies"
        );
    }

    #[test]
    fn test_mixed_std_imports_compile_without_symbol_collision() {
        let result = compile_source(
            r#"
            import path from "std:path";
            import fs from "std:fs";
            import env from "std:env";
            let base = env.cwd();
            let full = path.join(base, "tmp");
            return fs != null && full.length >= 0;
            "#,
        );
        assert!(
            result.is_ok(),
            "mixed std imports should compile without prelude symbol collisions"
        );
    }

    #[test]
    fn test_namespace_std_import_is_supported() {
        let result = compile_source(
            r#"
            import * as p from "std:path";
            return p.join("a", "b");
            "#,
        );
        assert!(result.is_ok(), "namespace std import should be supported");
    }

    #[test]
    fn test_node_events_import_is_supported() {
        let result = compile_source(
            r#"
            import EventEmitter from "node:events";
            let e = new EventEmitter<{ tick: [number] }>();
            e.on("tick", (n: number): void => {});
            e.emit("tick", 1);
            return e.listenerCount("tick");
            "#,
        );
        assert!(
            result.is_ok(),
            "node:events should provide EventEmitter shim"
        );
    }

    #[test]
    fn test_unsupported_node_import_has_explicit_error() {
        let result = compile_source(
            r#"
            import nope from "node:not_a_core_module";
            return 1;
            "#,
        );
        assert!(result.is_err(), "unsupported node module should fail");
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            msg.contains("Unsupported node module import 'node:not_a_core_module'"),
            "expected explicit unsupported-node error, got: {msg}"
        );
        assert!(
            msg.contains("node:fs"),
            "error should include supported node module guidance, got: {msg}"
        );
    }

    #[test]
    fn test_supported_node_module_import_smoke_suite() {
        let cases = [
            r#"import fs from "node:fs"; return fs != null;"#,
            r#"import fsp from "node:fs/promises"; return fsp != null;"#,
            r#"import path from "node:path"; return path != null;"#,
            r#"import os from "node:os"; return os != null;"#,
            r#"import process from "node:process"; return process != null;"#,
            r#"import dns from "node:dns"; return dns != null;"#,
            r#"import net from "node:net"; return net != null;"#,
            r#"import { HttpServer, HttpRequest } from "node:http"; return HttpServer != null && HttpRequest != null;"#,
            r#"import https from "node:https"; return https != null;"#,
            r#"import crypto from "node:crypto"; return crypto != null;"#,
            r#"import url from "node:url"; return url != null;"#,
            r#"import { ReadableStream, WritableStream, TransformStream } from "node:stream"; return ReadableStream != null && WritableStream != null && TransformStream != null;"#,
            r#"import EventEmitter from "node:events"; return EventEmitter != null;"#,
            r#"import assert from "node:assert"; return assert != null;"#,
            r#"import util from "node:util"; return util != null;"#,
            r#"import moduleShim from "node:module"; return moduleShim != null;"#,
            r#"import childProcess from "node:child_process"; return childProcess != null;"#,
            r#"import test from "node:test"; return test != null;"#,
            r#"import reporters from "node:test/reporters"; return reporters != null;"#,
            r#"import timers from "node:timers"; return timers != null;"#,
            r#"import timersPromises from "node:timers/promises"; return timersPromises != null;"#,
            r#"import BufferCtor from "node:buffer"; return BufferCtor != null;"#,
            r#"import stringDecoder from "node:string_decoder"; return stringDecoder != null;"#,
            r#"import streamPromises from "node:stream/promises"; return streamPromises != null;"#,
            r#"import streamWeb from "node:stream/web"; return streamWeb != null;"#,
            r#"import workerThreads from "node:worker_threads"; return workerThreads != null;"#,
            r#"import vm from "node:vm"; return vm != null;"#,
            r#"import http2 from "node:http2"; return http2 != null;"#,
            r#"import inspector from "node:inspector"; return inspector != null;"#,
            r#"import inspectorPromises from "node:inspector/promises"; return inspectorPromises != null;"#,
            r#"import asyncHooks from "node:async_hooks"; return asyncHooks != null;"#,
            r#"import diagnosticsChannel from "node:diagnostics_channel"; return diagnosticsChannel != null;"#,
            r#"import v8 from "node:v8"; return v8 != null;"#,
            r#"import dgram from "node:dgram"; return dgram != null;"#,
            r#"import cluster from "node:cluster"; return cluster != null;"#,
            r#"import repl from "node:repl"; return repl != null;"#,
            r#"import perfHooks from "node:perf_hooks"; return perfHooks != null;"#,
            r#"import sqlite from "node:sqlite"; return sqlite != null;"#,
        ];

        for source in cases {
            let result = compile_source(source);
            assert!(result.is_ok(), "node import smoke case failed: {source}");
        }
    }

    #[test]
    fn test_object_define_property_not_available_in_strict_mode() {
        let result = compile_source(
            r#"
            let obj = new Object();
            let desc = new Object();
            Object.defineProperty(obj, "x", desc);
            return obj;
            "#,
        );
        assert!(
            result.is_err(),
            "Object.defineProperty should not be available in strict mode"
        );
    }

    #[test]
    fn test_object_define_property_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let obj = new Object();
            let desc = new Object();
            Object.defineProperty(obj, "x", desc);
            return obj;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "Object.defineProperty should be available in node-compat mode"
        );
    }

    #[test]
    fn test_object_get_own_property_descriptor_not_available_in_strict_mode() {
        let result = compile_source(
            r#"
            let obj = new Object();
            Object.getOwnPropertyDescriptor(obj, "x");
            return obj;
            "#,
        );
        assert!(
            result.is_err(),
            "Object.getOwnPropertyDescriptor should not be available in strict mode"
        );
    }

    #[test]
    fn test_object_get_own_property_descriptor_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let obj = new Object();
            Object.getOwnPropertyDescriptor(obj, "x");
            return obj;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "Object.getOwnPropertyDescriptor should be available in node-compat mode"
        );
    }

    #[test]
    fn test_object_define_properties_not_available_in_strict_mode() {
        let result = compile_source(
            r#"
            let obj = new Object();
            let descriptors = {};
            Object.defineProperties(obj, descriptors);
            return obj;
            "#,
        );
        assert!(
            result.is_err(),
            "Object.defineProperties should not be available in strict mode"
        );
    }

    #[test]
    fn test_object_define_properties_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let obj = new Object();
            let descriptors = {};
            Object.defineProperties(obj, descriptors);
            return obj;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "Object.defineProperties should be available in node-compat mode"
        );
    }

    #[test]
    fn test_arraybuffer_not_available_in_strict_mode_with_explicit_error_code() {
        let result = compile_source(
            r#"
            let b = new ArrayBuffer(8);
            return b.byteLength;
            "#,
        );
        assert!(result.is_err(), "ArrayBuffer should be strict-incompatible");
        let msg = match result {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("E_STRICT_NODE_COMPAT_SYMBOL"),
            "expected strict compat error code, got: {msg}"
        );
        assert!(
            msg.contains("ArrayBuffer"),
            "expected symbol in error message, got: {msg}"
        );
    }

    #[test]
    fn test_arraybuffer_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let b = new ArrayBuffer(8);
            return b.byteLength;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "ArrayBuffer should be available in node-compat mode"
        );
    }

    #[test]
    fn test_extended_typed_arrays_not_available_in_strict_mode_with_explicit_error_code() {
        let result = compile_source(
            r#"
            let a = new Uint8ClampedArray(2);
            return a.length;
            "#,
        );
        assert!(
            result.is_err(),
            "extended typed arrays should be strict-incompatible"
        );
        let msg = match result {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("E_STRICT_NODE_COMPAT_SYMBOL"),
            "expected strict compat error code, got: {msg}"
        );
        assert!(
            msg.contains("Uint8ClampedArray"),
            "expected symbol in error message, got: {msg}"
        );
    }

    #[test]
    fn test_extended_typed_arrays_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let a = new Uint16Array(2);
            let b = new Int16Array(2);
            let c = new Uint32Array(2);
            let d = new Float32Array(2);
            let e = new Float16Array(2);
            let f = new BigInt64Array(2);
            let g = new BigUint64Array(2);
            let h = new TypedArray<number>(2);
            return a.length + b.length + c.length + d.length + e.length + f.length + g.length + h.length;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "extended typed arrays should be available in node-compat mode"
        );
    }

    #[test]
    fn test_event_emitter_available_in_strict_mode() {
        let result = compile_source(
            r#"
            let emitter = new EventEmitter<{ tick: [number] }>();
            return emitter.listenerCount("tick");
            "#,
        );
        assert!(
            result.is_ok(),
            "EventEmitter should be available in strict mode"
        );
    }

    #[test]
    fn test_event_emitter_typed_usage_compiles() {
        let result = compile_source(
            r#"
            let emitter = new EventEmitter<{ tick: [number] }>();
            emitter.on("tick", (payload: number): void => {
                let x: number = payload;
            });
            emitter.emit("tick", 42);
            return emitter.listenerCount("tick");
            "#,
        );
        assert!(
            result.is_ok(),
            "Typed EventEmitter<{{ tick: [number] }}> usage should compile"
        );
    }

    #[test]
    fn test_event_emitter_emit_wrong_payload_type_fails() {
        let result = compile_source(
            r#"
            let emitter = new EventEmitter<{ tick: [number] }>();
            emitter.emit("tick", "oops");
            return 0;
            "#,
        );
        assert!(
            result.is_err(),
            "EventEmitter<{{ tick: [number] }}>.emit should reject non-number payloads"
        );
    }

    #[test]
    fn test_event_emitter_listener_wrong_param_type_fails() {
        let result = compile_source(
            r#"
            let emitter = new EventEmitter<{ tick: [number] }>();
            emitter.on("tick", (payload: string): void => {});
            return 0;
            "#,
        );
        assert!(
            result.is_err(),
            "EventEmitter<{{ tick: [number] }}>.on should reject listener with wrong payload type"
        );
    }

    #[test]
    fn test_event_emitter_listener_count_missing_arg_fails() {
        let result = compile_source(
            r#"
            let emitter = new EventEmitter<{ tick: [number] }>();
            return emitter.listenerCount();
            "#,
        );
        assert!(
            result.is_err(),
            "listenerCount should require an event name argument"
        );
    }

    #[test]
    fn test_event_emitter_set_max_listeners_wrong_arg_type_fails() {
        let result = compile_source(
            r#"
            let emitter = new EventEmitter<{ tick: [number] }>();
            emitter.setMaxListeners("10");
            return 0;
            "#,
        );
        assert!(
            result.is_err(),
            "setMaxListeners should require a number argument"
        );
    }

    #[test]
    fn test_parseint_not_available_in_strict_mode_with_explicit_error_code() {
        let result = compile_source(
            r#"
            return parseInt("42");
            "#,
        );
        assert!(result.is_err(), "parseInt should be strict-incompatible");
        let msg = match result {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("E_STRICT_NODE_COMPAT_SYMBOL"),
            "expected strict compat error code, got: {msg}"
        );
        assert!(
            msg.contains("parseInt"),
            "expected parseInt symbol in error message, got: {msg}"
        );
    }

    #[test]
    fn test_parseint_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            return parseInt("42");
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "parseInt should be available in node-compat mode"
        );
    }

    #[test]
    fn test_globalthis_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            return globalThis != null;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "globalThis should be available in node-compat mode"
        );
    }

    #[test]
    fn test_reflect_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let o = new Object();
            Reflect.set(o, "x", 1);
            return Reflect.has(o, "x");
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "Reflect global should be available in node-compat mode"
        );
    }

    #[test]
    fn test_intl_not_available_in_strict_mode_with_explicit_error_code() {
        let result = compile_source(
            r#"
            let nf = Intl.NumberFormat("en-US", null);
            return nf.format(1.5);
            "#,
        );
        assert!(result.is_err(), "Intl should be strict-incompatible");
        let msg = match result {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("E_STRICT_NODE_COMPAT_SYMBOL"),
            "expected strict compat error code, got: {msg}"
        );
        assert!(
            msg.contains("Intl"),
            "expected Intl symbol in error message, got: {msg}"
        );
    }

    #[test]
    fn test_intl_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let nf = Intl.NumberFormat("en-US", null);
            let df = Intl.DateTimeFormat("en-US", null);
            let d = new Date();
            return nf.format(1.5) != "" && df.format(d) != "";
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "Intl should be available in node-compat mode"
        );
    }

    #[test]
    fn test_temporal_available_in_strict_mode() {
        let result = compile_source(
            r#"
            let inst = Temporal.Instant(0);
            let d = Temporal.PlainDate(2026, 2, 26);
            return inst.toString() != "" && d.toString() != "";
            "#,
        );
        assert!(
            result.is_ok(),
            "Temporal should be available in strict mode"
        );
    }

    #[test]
    fn test_temporal_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let t = Temporal.PlainTime(1, 2, 3, 4);
            let z = Temporal.ZonedDateTime(0, "UTC");
            return t.toString() != "" && z.toString() != "";
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "Temporal should be available in node-compat mode"
        );
    }

    #[test]
    fn test_temporal_plain_date_wrong_arity_fails() {
        let result = compile_source(
            r#"
            let d = Temporal.PlainDate(2026, 2);
            return d;
            "#,
        );
        assert!(
            result.is_err(),
            "Temporal.PlainDate arity should be enforced"
        );
    }

    #[test]
    fn test_iterator_available_in_strict_mode() {
        let result = compile_source(
            r#"
            let it = Iterator.fromArray<number>([1, 2, 3]);
            let r = it.next();
            return !r.done;
            "#,
        );
        assert!(
            result.is_ok(),
            "Iterator should be available in strict mode"
        );
    }

    #[test]
    fn test_function_constructor_not_available_in_strict_mode_with_explicit_error_code() {
        let result = compile_source(
            r#"
            let f = new Function("return 1;");
            return f;
            "#,
        );
        assert!(
            result.is_err(),
            "Function constructor should be strict-incompatible"
        );
        let msg = match result {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("E_STRICT_NODE_COMPAT_SYMBOL"),
            "expected strict compat error code, got: {msg}"
        );
        assert!(
            msg.contains("Function"),
            "expected Function symbol in error message, got: {msg}"
        );
    }

    #[test]
    fn test_function_constructor_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let f = new Function("return 1;");
            let g = new GeneratorFunction("yield 1;");
            let af = new AsyncFunction("return 1;");
            return f != null && g != null && af != null;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "Function/generator constructor families should be available in node-compat mode"
        );
    }

    #[test]
    fn test_disposable_stack_not_available_in_strict_mode_with_explicit_error_code() {
        let result = compile_source(
            r#"
            let s = new DisposableStack();
            return s;
            "#,
        );
        assert!(
            result.is_err(),
            "DisposableStack should be strict-incompatible"
        );
        let msg = match result {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("E_STRICT_NODE_COMPAT_SYMBOL"),
            "expected strict compat error code, got: {msg}"
        );
        assert!(
            msg.contains("DisposableStack"),
            "expected DisposableStack symbol in error message, got: {msg}"
        );
    }

    #[test]
    fn test_disposable_stack_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let s = new DisposableStack();
            s.defer((): void => {});
            s.dispose();
            return true;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "DisposableStack should be available in node-compat mode"
        );
    }

    #[test]
    fn test_async_disposable_stack_not_available_in_strict_mode_with_explicit_error_code() {
        let result = compile_source(
            r#"
            let s = new AsyncDisposableStack();
            return s;
            "#,
        );
        assert!(
            result.is_err(),
            "AsyncDisposableStack should be strict-incompatible"
        );
        let msg = match result {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("E_STRICT_NODE_COMPAT_SYMBOL"),
            "expected strict compat error code, got: {msg}"
        );
        assert!(
            msg.contains("AsyncDisposableStack"),
            "expected AsyncDisposableStack symbol in error message, got: {msg}"
        );
    }

    #[test]
    fn test_async_disposable_stack_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let s = new AsyncDisposableStack();
            s.defer(async (): Promise<void> => {});
            await s.disposeAsync();
            return true;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "AsyncDisposableStack should be available in node-compat mode"
        );
    }

    #[test]
    fn test_shared_array_buffer_not_available_in_strict_mode_with_explicit_error_code() {
        let result = compile_source(
            r#"
            let sab = new SharedArrayBuffer(16);
            return sab.byteLength;
            "#,
        );
        assert!(
            result.is_err(),
            "SharedArrayBuffer should be strict-incompatible"
        );
        let msg = match result {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("E_STRICT_NODE_COMPAT_SYMBOL"),
            "expected strict compat error code, got: {msg}"
        );
        assert!(
            msg.contains("SharedArrayBuffer"),
            "expected SharedArrayBuffer symbol in error message, got: {msg}"
        );
    }

    #[test]
    fn test_shared_array_buffer_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let sab = new SharedArrayBuffer(16);
            return sab.byteLength;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "SharedArrayBuffer should be available in node-compat mode"
        );
    }

    #[test]
    fn test_atomics_not_available_in_strict_mode_with_explicit_error_code() {
        let result = compile_source(
            r#"
            return Atomics != null;
            "#,
        );
        assert!(result.is_err(), "Atomics should be strict-incompatible");
        let msg = match result {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("E_STRICT_NODE_COMPAT_SYMBOL"),
            "expected strict compat error code, got: {msg}"
        );
        assert!(
            msg.contains("Atomics"),
            "expected Atomics symbol in error message, got: {msg}"
        );
    }

    #[test]
    fn test_atomics_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let sab = new SharedArrayBuffer(16);
            let a = new Int32Array(sab);
            Atomics.store(a, 0, 41);
            let old = Atomics.add(a, 0, 1);
            return old == 41 && Atomics.load(a, 0) == 42;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "Atomics should be available in node-compat mode"
        );
    }

    #[test]
    fn test_uri_helpers_available_in_strict_mode() {
        let result = compile_source(
            r#"
            let e = encodeURI("a b");
            let d = decodeURI(e);
            let ec = encodeURIComponent("x y");
            let dc = decodeURIComponent(ec);
            return d == "a b" && dc == "x y";
            "#,
        );
        assert!(
            result.is_ok(),
            "URI helpers should be available in strict mode"
        );
    }

    #[test]
    fn test_shared_numeric_constants_and_undefined_available_in_strict_mode() {
        let result = compile_source(
            r#"
            let infOk = Infinity > 1.0;
            let nanOk = NaN != NaN;
            let undefOk = undefined == null;
            return infOk && nanOk && undefOk;
            "#,
        );
        assert!(
            result.is_ok(),
            "Infinity/NaN/undefined should be available in strict mode"
        );
    }

    #[test]
    fn test_constructor_globals_available_in_strict_mode() {
        let result = compile_source(
            r#"
            let b = Boolean("x");
            let n = Number("42");
            let s = String(42);
            let a = new Array<number>(2);
            return b && n == 42 && s == "42" && a.length == 2;
            "#,
        );
        assert!(
            result.is_ok(),
            "Boolean/Number/String and Array constructors should be available in strict mode, got: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_std_symbols_not_available_without_import() {
        let result = compile_source(
            r#"
            return math.abs(-1);
            "#,
        );
        assert!(
            result.is_err(),
            "std symbols should not be available without explicit import"
        );
    }

    #[test]
    fn test_shadowing_node_compat_symbol_in_strict_mode_works() {
        let result = compile_source(
            r#"
            function parseInt(v: string): number { return 7; }
            return parseInt("ignored");
            "#,
        );
        assert!(
            result.is_ok(),
            "shadowing node-compat symbol should be allowed in strict mode"
        );
    }

    #[test]
    fn test_reflect_not_available_in_strict_mode_with_explicit_error_code() {
        let result = compile_source(
            r#"
            let o = new Object();
            return Reflect.has(o, "x");
            "#,
        );
        assert!(result.is_err(), "Reflect should be strict-incompatible");
        let msg = match result {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("E_STRICT_NODE_COMPAT_SYMBOL"),
            "expected strict compat error code, got: {msg}"
        );
        assert!(
            msg.contains("Reflect"),
            "expected Reflect symbol in error message, got: {msg}"
        );
    }

    #[test]
    fn test_proxy_not_available_in_strict_mode_with_explicit_error_code() {
        let result = compile_source(
            r#"
            let t = new Object();
            let h = new Object();
            let p = new Proxy<Object>(t, h);
            return p;
            "#,
        );
        assert!(result.is_err(), "Proxy should be strict-incompatible");
        let msg = match result {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("E_STRICT_NODE_COMPAT_SYMBOL"),
            "expected strict compat error code, got: {msg}"
        );
        assert!(
            msg.contains("Proxy"),
            "expected Proxy symbol in error message, got: {msg}"
        );
    }

    #[test]
    fn test_proxy_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let t = new Object();
            let h = new Object();
            let p = new Proxy<Object>(t, h);
            return p.isProxy();
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "Proxy should be available in node-compat mode"
        );
    }

    #[test]
    fn test_weakmap_not_available_in_strict_mode_with_explicit_error_code() {
        let result = compile_source(
            r#"
            let w = new WeakMap<number>();
            let k = new Object();
            w.set(k, 1);
            return w.has(k);
            "#,
        );
        assert!(result.is_err(), "WeakMap should be strict-incompatible");
        let msg = match result {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("E_STRICT_NODE_COMPAT_SYMBOL"),
            "expected strict compat error code, got: {msg}"
        );
        assert!(
            msg.contains("WeakMap"),
            "expected WeakMap symbol in error message, got: {msg}"
        );
    }

    #[test]
    fn test_weakmap_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let w = new WeakMap<number>();
            let k = new Object();
            w.set(k, 7);
            return w.has(k);
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "WeakMap should be available in node-compat mode"
        );
    }

    #[test]
    fn test_weakset_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let ws = new WeakSet<Object>();
            let o = new Object();
            ws.add(o);
            return ws.has(o);
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "WeakSet should be available in node-compat mode"
        );
    }

    #[test]
    fn test_weakref_not_available_in_strict_mode_with_explicit_error_code() {
        let result = compile_source(
            r#"
            let o = new Object();
            let wr = new WeakRef<Object>(o);
            return wr.deref() != null;
            "#,
        );
        assert!(result.is_err(), "WeakRef should be strict-incompatible");
        let msg = match result {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("E_STRICT_NODE_COMPAT_SYMBOL"),
            "expected strict compat error code, got: {msg}"
        );
        assert!(
            msg.contains("WeakRef"),
            "expected WeakRef symbol in error message, got: {msg}"
        );
    }

    #[test]
    fn test_weakref_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let o = new Object();
            let wr = new WeakRef<Object>(o);
            return wr.deref() != null;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "WeakRef should be available in node-compat mode"
        );
    }

    #[test]
    fn test_finalization_registry_not_available_in_strict_mode_with_explicit_error_code() {
        let result = compile_source(
            r#"
            let reg = new FinalizationRegistry<string>((heldValue: string): void => {});
            return reg;
            "#,
        );
        assert!(
            result.is_err(),
            "FinalizationRegistry should be strict-incompatible"
        );
        let msg = match result {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("E_STRICT_NODE_COMPAT_SYMBOL"),
            "expected strict compat error code, got: {msg}"
        );
        assert!(
            msg.contains("FinalizationRegistry"),
            "expected FinalizationRegistry symbol in error message, got: {msg}"
        );
    }

    #[test]
    fn test_finalization_registry_available_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let reg = new FinalizationRegistry<string>((heldValue: string): void => {});
            let o = new Object();
            reg.register(o, "held", o);
            return reg.unregister(o);
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "FinalizationRegistry should be available in node-compat mode"
        );
    }

    #[test]
    fn test_shadowing_reflect_symbol_in_strict_mode_works() {
        let result = compile_source(
            r#"
            class Reflect {
                has(o: Object, k: string): boolean { return true; }
            }
            let r = new Reflect();
            return r.has(new Object(), "x");
            "#,
        );
        assert!(
            result.is_ok(),
            "shadowing Reflect symbol should be allowed in strict mode"
        );
    }

    #[test]
    fn test_shadowing_arraybuffer_symbol_in_strict_mode_works() {
        let result = compile_source(
            r#"
            class ArrayBuffer {
                byteLength: number;
                constructor(n: number) { this.byteLength = n; }
            }
            let b = new ArrayBuffer(8);
            return b.byteLength;
            "#,
        );
        assert!(
            result.is_ok(),
            "shadowing ArrayBuffer symbol should be allowed in strict mode"
        );
    }

    #[test]
    fn test_any_forbidden_in_strict_mode() {
        let result = compile_source(
            r#"
            let x: any = 1;
            return x;
            "#,
        );
        assert!(result.is_err(), "`any` should be forbidden in strict mode");
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            msg.contains("E_STRICT_ANY_FORBIDDEN"),
            "expected strict-any error code, got: {msg}"
        );
    }

    #[test]
    fn test_any_allowed_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let x: any = 1;
            x = "ok";
            return x;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "`any` should be allowed in node-compat mode"
        );
    }

    #[test]
    fn test_bare_let_forbidden_in_strict_mode() {
        let result = compile_source(
            r#"
            let x;
            x = 1;
            return x;
            "#,
        );
        assert!(
            result.is_err(),
            "bare let should be forbidden in strict mode"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            msg.contains("E_STRICT_BARE_LET_FORBIDDEN"),
            "expected strict bare-let error code, got: {msg}"
        );
    }

    #[test]
    fn test_bare_let_allowed_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            let x;
            x = 1;
            return x;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "bare let should be allowed in node-compat mode"
        );
    }

    #[test]
    fn test_no_implicit_this_in_strict_mode() {
        let result = compile_source(
            r#"
            function f(): number {
                return this as number;
            }
            return f();
            "#,
        );
        assert!(
            result.is_err(),
            "implicit this should be forbidden in strict mode"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            msg.contains("E_STRICT_NO_IMPLICIT_THIS"),
            "expected strict implicit-this error, got: {msg}"
        );
    }

    #[test]
    fn test_no_implicit_any_parameter_in_strict_mode() {
        let result = compile_source(
            r#"
            function id(x) { return x; }
            return id(1);
            "#,
        );
        assert!(
            result.is_err(),
            "implicit any parameter should be forbidden in strict mode"
        );
    }

    #[test]
    fn test_implicit_any_parameter_allowed_in_allow_any_mode() {
        let result = compile_source_with_modes(
            r#"
            function id(x) { return x; }
            return id(1);
            "#,
            BuiltinMode::NodeCompat,
            TypeMode::AllowAny,
        );
        assert!(
            result.is_ok(),
            "implicit any parameter should be allowed in allowAny mode"
        );
    }

    #[test]
    fn test_unknown_not_actionable_in_strict_mode() {
        let result = compile_source(
            r#"
            let x: unknown = 1;
            return x.toString();
            "#,
        );
        assert!(
            result.is_err(),
            "unknown member access should be forbidden in strict mode"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            msg.contains("E_STRICT_UNKNOWN_NOT_ACTIONABLE"),
            "expected unknown-not-actionable error, got: {msg}"
        );
    }

    #[test]
    fn test_strict_property_initialization_required_in_strict_mode() {
        let result = compile_source(
            r#"
            class User {
                name: string;
            }
            return 0;
            "#,
        );
        assert!(
            result.is_err(),
            "uninitialized instance field should fail in strict mode"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            msg.contains("E_STRICT_PROPERTY_INITIALIZATION"),
            "expected strict property initialization error, got: {msg}"
        );
    }

    #[test]
    fn test_strict_property_initialization_not_required_in_node_compat_mode() {
        let result = compile_source_with_mode(
            r#"
            class User {
                name: string;
            }
            return 0;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "uninitialized instance field should be allowed in node-compat mode"
        );
    }

    #[test]
    fn test_strict_bind_call_apply_valid() {
        let result = compile_source(
            r#"
            function add(a: number, b: number): number { return a + b; }
            let plusOne = add.bind(null, 1);
            let x = plusOne(2);
            let y = add.call(null, 3, 4);
            let z = add.apply(null, [5, 6]);
            return x + y + z;
            "#,
        );
        assert!(
            result.is_ok(),
            "bind/call/apply should type-check for compatible signatures in strict mode"
        );
    }

    #[test]
    fn test_strict_call_rejects_wrong_args() {
        let result = compile_source(
            r#"
            function add(a: number, b: number): number { return a + b; }
            return add.call(null, "x", 2);
            "#,
        );
        assert!(
            result.is_err(),
            "strict call should reject wrong argument type"
        );
    }

    #[test]
    fn test_strict_apply_rejects_non_array_args_list() {
        let result = compile_source(
            r#"
            function add(a: number, b: number): number { return a + b; }
            return add.apply(null, 1);
            "#,
        );
        assert!(
            result.is_err(),
            "strict apply should require tuple/array args list"
        );
    }

    #[test]
    fn test_strict_call_rejects_wrong_this_for_extracted_method() {
        let result = compile_source(
            r#"
            class Counter {
                value: number;
                constructor(v: number) { this.value = v; }
                get(): number { return this.value; }
            }
            let c = new Counter(1);
            let f = c.get;
            return f.call("not-counter");
            "#,
        );
        assert!(
            result.is_err(),
            "strict call should reject incompatible thisArg for extracted methods"
        );
    }

    #[test]
    fn test_strict_bind_rejects_wrong_this_for_extracted_method() {
        let result = compile_source(
            r#"
            class Counter {
                value: number;
                constructor(v: number) { this.value = v; }
                get(): number { return this.value; }
            }
            let c = new Counter(1);
            let f = c.get;
            let g = f.bind("not-counter");
            return g();
            "#,
        );
        assert!(
            result.is_err(),
            "strict bind should reject incompatible thisArg for extracted methods"
        );
    }

    #[test]
    fn test_strict_apply_rejects_wrong_this_for_extracted_method() {
        let result = compile_source(
            r#"
            class Counter {
                value: number;
                constructor(v: number) { this.value = v; }
                get(): number { return this.value; }
            }
            let c = new Counter(1);
            let f = c.get;
            return f.apply("not-counter", []);
            "#,
        );
        assert!(
            result.is_err(),
            "strict apply should reject incompatible thisArg for extracted methods"
        );
    }

    #[test]
    fn test_strict_null_checks_reject_null_to_string() {
        let result = compile_source(
            r#"
            let s: string = null;
            return s;
            "#,
        );
        assert!(
            result.is_err(),
            "strict mode should reject null assignment to string"
        );
    }

    #[test]
    fn test_node_compat_allows_null_to_string_assignment() {
        let result = compile_source_with_mode(
            r#"
            let s: string = null;
            return s;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "node-compat should allow non-strict null coercion behavior"
        );
    }

    #[test]
    fn test_strict_function_types_reject_unsafe_parameter_variance() {
        let result = compile_source(
            r#"
            class Animal { name: string = "a"; }
            class Dog extends Animal { breed: string = "b"; }

            let dogOnly: (d: Dog) => void = (d: Dog): void => {};
            let bad: (a: Animal) => void = dogOnly;
            bad(new Animal());
            return 0;
            "#,
        );
        assert!(
            result.is_err(),
            "strict function types should reject unsafe callback variance"
        );
    }

    #[test]
    fn test_strict_catch_variable_unknown_requires_narrowing() {
        let result = compile_source(
            r#"
            try {
                throw "x";
            } catch (e) {
                return e.toString();
            }
            "#,
        );
        assert!(
            result.is_err(),
            "strict catch variable should be unknown and not directly actionable"
        );
    }

    #[test]
    fn test_node_compat_reassignment_infers_union() {
        let result = compile_source_with_mode(
            r#"
            let a = 10;
            a = "hello";
            return a;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "node-compat should widen contradictory reassignments to a union"
        );
    }

    #[test]
    fn test_strict_reassignment_keeps_initial_inference() {
        let result = compile_source(
            r#"
            let a = 10;
            a = "hello";
            return a;
            "#,
        );
        assert!(
            result.is_err(),
            "strict mode should not auto-widen inferred variable to union on reassignment"
        );
    }

    #[test]
    fn test_node_compat_dynamic_index_write_allowed_on_inferred_object() {
        let result = compile_source_with_mode(
            r#"
            class User { name: string = "a"; }
            let o = new User();
            let k = "dynamic";
            o[k] = "ok";
            return 0;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "node-compat should permit dynamic index writes via JSObject fallback inference"
        );
    }

    #[test]
    fn test_node_compat_bare_let_flow_infers_union() {
        let result = compile_source_with_mode(
            r#"
            let a;
            a = 10;
            a = "hello";
            return a;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "node-compat bare-let flow inference should widen contradictory assignments to a union"
        );
    }

    #[test]
    fn test_node_compat_constructor_flow_allows_dynamic_monkey_patch() {
        let result = compile_source_with_mode(
            r#"
            class User {
                name: string;
                constructor(name: string) { this.name = name; }
            }
            let obj;
            obj = new User("alice");
            let dynamicKey = "extra";
            obj[dynamicKey] = 42;
            return 0;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "node-compat should allow constructor-initialized values to escalate for dynamic monkey patch writes"
        );
    }

    #[test]
    fn test_nodecompat_strict_forbids_any_and_bare_let() {
        let any_result = compile_source_with_modes(
            r#"
            let x: any = 1;
            return x;
            "#,
            BuiltinMode::NodeCompat,
            TypeMode::Strict,
        );
        assert!(
            any_result.is_err(),
            "strict type mode should forbid explicit any"
        );

        let bare_let_result = compile_source_with_modes(
            r#"
            let x;
            x = 1;
            return x;
            "#,
            BuiltinMode::NodeCompat,
            TypeMode::Strict,
        );
        assert!(
            bare_let_result.is_err(),
            "strict type mode should forbid bare let even in node-compat builtins"
        );
    }

    #[test]
    fn test_nodecompat_allow_any_still_forbids_bare_let() {
        let any_result = compile_source_with_modes(
            r#"
            let x: any = 1;
            x = "ok";
            return x;
            "#,
            BuiltinMode::NodeCompat,
            TypeMode::AllowAny,
        );
        assert!(
            any_result.is_ok(),
            "allowAny mode should allow explicit any"
        );

        let bare_let_result = compile_source_with_modes(
            r#"
            let x;
            x = 1;
            return x;
            "#,
            BuiltinMode::NodeCompat,
            TypeMode::AllowAny,
        );
        assert!(
            bare_let_result.is_err(),
            "allowAny mode should still forbid untyped bare-let declarations"
        );
    }

    #[test]
    fn test_nodecompat_js_mode_allows_bare_let_and_any() {
        let result = compile_source_with_modes(
            r#"
            let x;
            x = 1;
            let y: any = x;
            return y;
            "#,
            BuiltinMode::NodeCompat,
            TypeMode::JsMode,
        );
        assert!(
            result.is_ok(),
            "jsMode should allow untyped variables and any semantics"
        );
    }

    #[test]
    fn test_nodecompat_js_mode_rejects_dot_monkeypatch_without_any_cast() {
        let result = compile_source_with_modes(
            r#"
            class User {
                name: string;
                constructor(name: string) { this.name = name; }
            }
            let u = new User("a");
            u.extra = 1;
            return 0;
            "#,
            BuiltinMode::NodeCompat,
            TypeMode::JsMode,
        );
        assert!(
            result.is_err(),
            "dot field writes should be rejected unless object is explicitly any-casted"
        );
    }

    #[test]
    fn test_jsobject_wrapper_preserves_known_fields_from_base_type() {
        let result = compile_source_with_modes(
            r#"
            class User {
                name: string;
                constructor(name: string) { this.name = name; }
            }
            let u = new User("a");
            let k = "extra";
            u[k] = 1;
            let n: string = u.name;
            return n;
            "#,
            BuiltinMode::NodeCompat,
            TypeMode::JsMode,
        );
        assert!(
            result.is_ok(),
            "JSObject<T> should preserve known fields from T with normal typing"
        );
    }

    #[test]
    fn test_jsobject_wrapper_unknown_member_is_dynamic_any() {
        let result = check_source_with_modes(
            r#"
            class User {
                name: string;
                constructor(name: string) { this.name = name; }
            }
            let u = new User("a");
            let k = "extra";
            u[k] = 1;
            let z = u.nonExisting;
            z();
            return 0;
            "#,
            BuiltinMode::NodeCompat,
            TypeMode::JsMode,
        )
        .expect("check_source_with_modes should produce diagnostics");

        assert!(
            result.errors.is_empty() && result.bind_errors.is_empty(),
            "unknown members on JSObject<T> should be dynamic in jsMode"
        );
    }

    #[test]
    fn test_jsobject_wrapper_keeps_known_monkeypatched_field_type() {
        let result = check_source_with_modes(
            r#"
            class User {
                name: string;
                constructor(name: string) { this.name = name; }
            }
            let u = new User("a");
            let dynU: any = u;
            dynU.extra = 123;
            let x: int = dynU.extra;
            return x;
            "#,
            BuiltinMode::NodeCompat,
            TypeMode::AllowAny,
        )
        .expect("check_source_with_modes should produce diagnostics");

        assert!(
            result.errors.is_empty() && result.bind_errors.is_empty(),
            "known monkeypatched fields should be tracked with concrete assigned type; check_errors={:?} bind_errors={:?}",
            result.errors,
            result.bind_errors
        );
    }

    #[test]
    fn test_allow_any_dot_write_existing_field_compiles_and_runs() {
        let compiled = compile_source_with_modes(
            r#"
            class User {
                name: string;
                constructor(name: string) { this.name = name; }
            }
            let u = new User("a");
            let dynU: any = u;
            dynU.name = "b";
            return dynU.name;
            "#,
            BuiltinMode::NodeCompat,
            TypeMode::AllowAny,
        );
        assert!(
            compiled.is_ok(),
            "dot writes through explicit any should compile without lowering internal errors"
        );
    }

    #[test]
    fn test_allow_any_dot_write_unknown_field_compiles_without_internal_error() {
        let compiled = compile_source_with_modes(
            r#"
            class User {
                name: string;
                constructor(name: string) { this.name = name; }
            }
            let u = new User("a");
            let dynU: any = u;
            dynU.extra = 1;
            return 0;
            "#,
            BuiltinMode::NodeCompat,
            TypeMode::AllowAny,
        );
        assert!(
            compiled.is_ok(),
            "unknown dot writes in dynamic-any flows should not fail lowering with internal compiler errors"
        );
    }

    #[test]
    fn test_nodecompat_allow_any_check_allows_dot_write_after_any_cast() {
        let result = check_source_with_modes(
            r#"
            class User {
                name: string;
                constructor(name: string) { this.name = name; }
            }
            let u = new User("a");
            let dynU: any = u;
            dynU.extra = 1;
            return 0;
            "#,
            BuiltinMode::NodeCompat,
            TypeMode::AllowAny,
        )
        .expect("check_source_with_modes should return diagnostics");

        assert!(
            result.errors.is_empty() && result.bind_errors.is_empty(),
            "explicit any cast/annotation should allow dot monkeypatch at checker level"
        );
    }
}
