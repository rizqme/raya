//! Source compilation pipeline.
//!
//! Parse → Bind → TypeCheck → Compile to bytecode.

use raya_engine::compiler::{Compiler, Module, MonomorphizationMode};
use raya_engine::parser::ast::{self, Statement};
use raya_engine::parser::checker::checker::CheckResult;
use raya_engine::parser::checker::{
    BindError, Binder, CheckError, CheckWarning, CheckerPolicy, ScopeId, SymbolTable, TsTypeFlags,
    TypeChecker,
};
use raya_engine::parser::{Interner, LexError, ParseError, Parser, TypeContext};
use raya_engine::semantics::{build_semantic_hir, SemanticHirModule, SemanticProfile, SourceKind};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;

use crate::builtin_manifest;
use crate::error::RuntimeError;
use crate::BuiltinMode;
use raya_engine::compiler::module::inject_ambient_exports;

/// Parsed TypeScript compiler options from tsconfig.json.
///
/// We accept the full surface via `other` and only enforce
/// checker-relevant flags at this stage.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TsCompilerOptions {
    pub strict: Option<bool>,
    pub no_implicit_any: Option<bool>,
    pub no_implicit_this: Option<bool>,
    pub strict_null_checks: Option<bool>,
    pub strict_property_initialization: Option<bool>,
    pub use_unknown_in_catch_variables: Option<bool>,
    pub exact_optional_property_types: Option<bool>,
    pub no_unchecked_indexed_access: Option<bool>,
    pub strict_function_types: Option<bool>,
    pub allow_js: Option<bool>,
    #[serde(flatten)]
    pub other: HashMap<String, JsonValue>,
}

impl TsCompilerOptions {
    pub fn effective_typecheck_flags(&self) -> TsTypeFlags {
        TsTypeFlags {
            strict: self.strict.unwrap_or(false),
            no_implicit_any: self.no_implicit_any.unwrap_or(false),
            no_implicit_this: self.no_implicit_this.unwrap_or(false),
            strict_null_checks: self.strict_null_checks.unwrap_or(false),
            strict_property_initialization: self.strict_property_initialization.unwrap_or(false),
            use_unknown_in_catch_variables: self.use_unknown_in_catch_variables.unwrap_or(false),
            exact_optional_property_types: self.exact_optional_property_types.unwrap_or(false),
            no_unchecked_indexed_access: self.no_unchecked_indexed_access.unwrap_or(false),
            strict_function_types: self.strict_function_types.unwrap_or(false),
        }
    }

    pub fn unsupported_but_parsed_flags(&self) -> Vec<String> {
        let mut names: Vec<String> = self.other.keys().cloned().collect();
        names.sort();
        names
    }
}

#[inline]
pub fn default_semantic_profile_for_builtin(mode: BuiltinMode) -> SemanticProfile {
    match mode {
        BuiltinMode::RayaStrict => SemanticProfile::raya(),
        BuiltinMode::NodeCompat => SemanticProfile::node_compat(),
    }
}

#[inline]
fn checker_policy_for_profile(
    profile: SemanticProfile,
    ts_options: Option<&TsCompilerOptions>,
) -> CheckerPolicy {
    let ts_flags = ts_options.map(TsCompilerOptions::effective_typecheck_flags);
    profile.checker_policy(ts_flags)
}

fn parser_for_profile(source: &str, profile: SemanticProfile) -> Result<Parser, Vec<LexError>> {
    Parser::new_with_mode(source, profile.parser_mode())
}

struct GraphFrontend {
    full_source: String,
    user_offset: usize,
    builtin_mode: BuiltinMode,
    profile: SemanticProfile,
    policy: CheckerPolicy,
    unsupported_ts_flags: Vec<String>,
    ast: ast::Module,
    interner: Interner,
    type_ctx: TypeContext,
}

struct BoundGraphFrontend {
    full_source: String,
    user_offset: usize,
    profile: SemanticProfile,
    policy: CheckerPolicy,
    unsupported_ts_flags: Vec<String>,
    ast: ast::Module,
    interner: Interner,
    type_ctx: TypeContext,
    symbols: SymbolTable,
}

struct CheckedGraphFrontend {
    full_source: String,
    user_offset: usize,
    unsupported_ts_flags: Vec<String>,
    ast: ast::Module,
    interner: Interner,
    type_ctx: TypeContext,
    check_result: CheckResult,
}

enum FrontendBindError {
    Bind(Vec<BindError>),
    Runtime(RuntimeError),
}

impl GraphFrontend {
    fn new(
        source: &str,
        builtin_mode: BuiltinMode,
        profile: SemanticProfile,
        ts_options: Option<&TsCompilerOptions>,
    ) -> Result<Self, RuntimeError> {
        validate_profile_constraints(builtin_mode, profile, ts_options)?;
        if matches!(profile.typing, raya_engine::semantics::TypingDiscipline::StrictTs)
            && ts_options.is_some_and(|options| options.strict == Some(false))
        {
            return Err(RuntimeError::TypeCheck(
                "TypeScript semantic profile requires strict TS compiler options.".to_string(),
            ));
        }
        precheck_user_top_level_duplicates(source, profile)?;
        precheck_node_compat_symbol_usage(source, builtin_mode)?;

        let full_source = source.to_string();
        let user_offset = 0usize;
        if let Ok(path) = std::env::var("RAYA_DEBUG_DUMP_SOURCE") {
            let _ = std::fs::write(path, &full_source);
        }
        let prefix_lines = full_source[..user_offset]
            .bytes()
            .filter(|&b| b == b'\n')
            .count();

        let parser = parser_for_profile(&full_source, profile)
            .map_err(|errors| RuntimeError::Lex(format_lex_errors(&errors, prefix_lines)))?;
        let (ast, interner) = parser
            .parse()
            .map_err(|errors| RuntimeError::Parse(format_parse_errors(&errors, prefix_lines)))?;

        let unsupported_ts_flags = if matches!(profile.source_kind, SourceKind::Ts) {
            ts_options
                .map(TsCompilerOptions::unsupported_but_parsed_flags)
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        Ok(Self {
            full_source,
            user_offset,
            builtin_mode,
            profile,
            policy: checker_policy_for_profile(profile, ts_options),
            unsupported_ts_flags,
            ast,
            interner,
            type_ctx: TypeContext::new(),
        })
    }

    fn bind(self) -> Result<BoundGraphFrontend, FrontendBindError> {
        let Self {
            full_source,
            user_offset,
            builtin_mode,
            profile,
            policy,
            unsupported_ts_flags,
            ast,
            interner,
            mut type_ctx,
        } = self;

        let mut binder = Binder::new(&mut type_ctx, &interner)
            .with_mode(profile.checker_mode())
            .with_policy(policy);
        binder.register_builtins(&[]);
        let builtin_exports = crate::Runtime::builtin_global_exports_for_mode(builtin_mode)
            .map_err(FrontendBindError::Runtime)?;
        inject_ambient_exports(&mut binder, &ast, &interner, &builtin_exports);
        let symbols = binder.bind_module(&ast).map_err(FrontendBindError::Bind)?;

        Ok(BoundGraphFrontend {
            full_source,
            user_offset,
            profile,
            policy,
            unsupported_ts_flags,
            ast,
            interner,
            type_ctx,
            symbols,
        })
    }

    fn diagnostics(self) -> Result<CheckDiagnostics, RuntimeError> {
        let source = self.full_source.clone();
        let user_offset = self.user_offset;
        match self.bind() {
            Err(FrontendBindError::Bind(bind_errors)) => Ok(CheckDiagnostics {
                errors: Vec::new(),
                bind_errors,
                warnings: Vec::new(),
                source,
                user_offset,
            }),
            Err(FrontendBindError::Runtime(error)) => Err(error),
            Ok(bound) => Ok(bound.diagnostics()),
        }
    }

    fn compile_checked(self) -> Result<CheckedGraphFrontend, RuntimeError> {
        let source = self.full_source.clone();
        let user_offset = self.user_offset;
        match self.bind() {
            Ok(bound) => bound.compile_checked(),
            Err(FrontendBindError::Bind(errors)) => Err(RuntimeError::TypeCheck(
                errors
                    .iter()
                    .map(|error| format_bind_error(error, user_offset, &source))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )),
            Err(FrontendBindError::Runtime(error)) => Err(error),
        }
    }
}

impl BoundGraphFrontend {
    fn check(mut self) -> Result<CheckedGraphFrontend, Vec<CheckError>> {
        let checker = TypeChecker::new(&mut self.type_ctx, &self.symbols, &self.interner)
            .with_mode(self.profile.checker_mode())
            .with_policy(self.policy);
        let check_result = checker.check_module(&self.ast)?;

        for ((scope_id, name), ty) in &check_result.inferred_types {
            self.symbols
                .update_type(ScopeId(*scope_id), name.as_str(), *ty);
        }

        Ok(CheckedGraphFrontend {
            full_source: self.full_source,
            user_offset: self.user_offset,
            unsupported_ts_flags: self.unsupported_ts_flags,
            ast: self.ast,
            interner: self.interner,
            type_ctx: self.type_ctx,
            check_result,
        })
    }

    fn compile_checked(self) -> Result<CheckedGraphFrontend, RuntimeError> {
        let source = self.full_source.clone();
        let user_offset = self.user_offset;
        self.check().map_err(|errors| {
            RuntimeError::TypeCheck(
                errors
                    .iter()
                    .map(|error| format_check_error(error, user_offset, &source))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        })
    }

    fn diagnostics(self) -> CheckDiagnostics {
        let source = self.full_source.clone();
        let user_offset = self.user_offset;
        match self.check() {
            Ok(mut checked) => {
                for flag in checked.unsupported_ts_flags.drain(..) {
                    checked
                        .check_result
                        .warnings
                        .push(CheckWarning::UnsupportedTsFlag {
                            flag,
                            span: raya_engine::parser::Span::new(
                                user_offset,
                                user_offset + 1,
                                1,
                                1,
                            ),
                        });
                }
                CheckDiagnostics {
                    errors: Vec::new(),
                    bind_errors: Vec::new(),
                    warnings: checked.check_result.warnings,
                    source,
                    user_offset,
                }
            }
            Err(check_errors) => CheckDiagnostics {
                errors: check_errors,
                bind_errors: Vec::new(),
                warnings: Vec::new(),
                source,
                user_offset,
            },
        }
    }
}

/// Options controlling compilation output.
#[derive(Debug, Clone, Default)]
pub struct CompileOptions {
    /// Include source map (bytecode offset → source location) in output.
    pub sourcemap: bool,
    /// Emit generic template metadata into output artifacts.
    pub emit_generic_templates: bool,
    /// Monomorphization strategy for generic specialization.
    pub monomorphization_mode: MonomorphizationMode,
}

/// Diagnostics returned from a check-only pass (no codegen).
#[derive(Debug)]
pub struct CheckDiagnostics {
    /// Type checking errors
    pub errors: Vec<CheckError>,
    /// Binding errors
    pub bind_errors: Vec<BindError>,
    /// Warnings from type checking
    pub warnings: Vec<CheckWarning>,
    /// Full source text submitted for checking.
    pub source: String,
    /// Byte offset where user code begins in `source`.
    ///
    /// With binary-link-only compilation this is always 0.
    pub user_offset: usize,
}

/// Build an inspectable semantic-HIR summary for a source snippet.
pub fn inspect_semantic_hir_with_profile(
    source: &str,
    profile: SemanticProfile,
) -> Result<SemanticHirModule, RuntimeError> {
    let parser = parser_for_profile(source, profile)
        .map_err(|errors| RuntimeError::Lex(format!("{errors:?}")))?;
    let (ast, interner) = parser
        .parse()
        .map_err(|errors| RuntimeError::Parse(format!("{errors:?}")))?;
    Ok(build_semantic_hir(&ast, &interner, profile))
}

/// Compile Raya source code to a bytecode module.
pub fn compile_source(source: &str) -> Result<(Module, Interner), RuntimeError> {
    compile_source_with_profile(source, BuiltinMode::RayaStrict, SemanticProfile::raya())
}

/// Compile Raya source code to a bytecode module with builtin API mode.
pub fn compile_source_with_mode(
    source: &str,
    builtin_mode: BuiltinMode,
) -> Result<(Module, Interner), RuntimeError> {
    compile_source_with_profile(
        source,
        builtin_mode,
        default_semantic_profile_for_builtin(builtin_mode),
    )
}

/// Compile source using an explicit semantic profile.
pub fn compile_source_with_profile(
    source: &str,
    builtin_mode: BuiltinMode,
    profile: SemanticProfile,
) -> Result<(Module, Interner), RuntimeError> {
    compile_source_with_profile_and_ts_options(source, builtin_mode, profile, None)
}

/// Compile source with explicit builtin surface + semantic profile + optional TS options.
///
/// This always goes through the binary module-graph compiler.
pub fn compile_source_with_profile_and_ts_options(
    source: &str,
    builtin_mode: BuiltinMode,
    profile: SemanticProfile,
    ts_options: Option<&TsCompilerOptions>,
) -> Result<(Module, Interner), RuntimeError> {
    validate_profile_constraints(builtin_mode, profile, ts_options)?;
    precheck_user_top_level_duplicates(source, profile)?;
    precheck_node_compat_symbol_usage(source, builtin_mode)?;

    use crate::module_system::ProgramCompiler;
    let virtual_entry = match profile.source_kind {
        SourceKind::Js => Path::new("<inline>.js"),
        SourceKind::Ts => Path::new("<inline>.ts"),
        SourceKind::Raya => Path::new("<inline>.raya"),
    };
    let compiler = ProgramCompiler {
        builtin_mode,
        semantic_profile: profile,
        ts_options: ts_options.cloned(),
        compile_options: None,
    };
    let program = compiler.compile_program_source(source, &virtual_entry)?;
    let module = program.entry.module;
    let interner = program
        .entry
        .interner
        .expect("compile_program_source always sets interner");
    Ok((module, interner))
}

/// Compile Raya source code to a bytecode module with options.
///
/// Same as `compile_source` but allows controlling compilation output
/// (e.g., source map generation).
pub fn compile_source_with_options(
    source: &str,
    options: &CompileOptions,
) -> Result<(Module, Interner), RuntimeError> {
    compile_source_with_options_and_profile(
        source,
        options,
        BuiltinMode::RayaStrict,
        SemanticProfile::raya(),
    )
}

/// Compile source with explicit compile options and builtin compatibility mode.
pub fn compile_source_with_options_and_mode(
    source: &str,
    options: &CompileOptions,
    builtin_mode: BuiltinMode,
) -> Result<(Module, Interner), RuntimeError> {
    compile_source_with_options_and_profile(
        source,
        options,
        builtin_mode,
        default_semantic_profile_for_builtin(builtin_mode),
    )
}

/// Compile source with explicit compile options, builtin mode, and semantic profile.
pub fn compile_source_with_options_and_profile(
    source: &str,
    options: &CompileOptions,
    builtin_mode: BuiltinMode,
    profile: SemanticProfile,
) -> Result<(Module, Interner), RuntimeError> {
    compile_source_with_options_and_profile_and_ts_options(
        source,
        options,
        builtin_mode,
        profile,
        None,
    )
}

pub fn compile_source_with_options_and_profile_and_ts_options(
    source: &str,
    options: &CompileOptions,
    builtin_mode: BuiltinMode,
    profile: SemanticProfile,
    ts_options: Option<&TsCompilerOptions>,
) -> Result<(Module, Interner), RuntimeError> {
    if options.sourcemap
        || options.emit_generic_templates
        || !matches!(
            options.monomorphization_mode,
            MonomorphizationMode::ConsumerLink
        )
    {
        return Err(RuntimeError::Dependency(
            "compile_source_with_options now requires binary-link-compatible defaults (no sourcemap, no emit_generic_templates, monomorphization=ConsumerLink)".to_string(),
        ));
    }
    compile_source_with_profile_and_ts_options(source, builtin_mode, profile, ts_options)
}

/// Compile already-linked module-graph source with an explicit semantic profile.
pub fn compile_graph_source_with_profile_and_ts_options(
    source: &str,
    builtin_mode: BuiltinMode,
    profile: SemanticProfile,
    ts_options: Option<&TsCompilerOptions>,
) -> Result<(Module, Interner), RuntimeError> {
    compile_graph_source_with_options_and_profile_and_ts_options(
        source,
        &CompileOptions::default(),
        builtin_mode,
        profile,
        ts_options,
    )
}

/// Compile already-linked module-graph source with an explicit semantic profile.
pub fn compile_graph_source_with_options_and_profile_and_ts_options(
    source: &str,
    options: &CompileOptions,
    builtin_mode: BuiltinMode,
    profile: SemanticProfile,
    ts_options: Option<&TsCompilerOptions>,
) -> Result<(Module, Interner), RuntimeError> {
    let checked =
        GraphFrontend::new(source, builtin_mode, profile, ts_options)?.compile_checked()?;

    let compiler = Compiler::new(checked.type_ctx, &checked.interner)
        .with_semantic_profile(profile)
        .with_expr_types(checked.check_result.expr_types)
        .with_type_annotation_types(checked.check_result.type_annotation_types)
        .with_sourcemap(options.sourcemap)
        .with_emit_generic_templates(options.emit_generic_templates)
        .with_monomorphization_mode(options.monomorphization_mode)
        .with_source_text(checked.full_source.clone());
    let bytecode = compiler.compile_via_ir(&checked.ast)?;

    Ok((bytecode, checked.interner))
}

/// Type-check Raya source code without generating bytecode.
///
/// Runs Parse → Bind → TypeCheck and returns all errors and warnings.
/// Does not perform IR lowering, optimization, or code generation.
pub fn check_source(source: &str) -> Result<CheckDiagnostics, RuntimeError> {
    check_source_with_profile(source, BuiltinMode::RayaStrict, SemanticProfile::raya())
}

/// Type-check source using a specific builtin compatibility mode.
pub fn check_source_with_mode(
    source: &str,
    builtin_mode: BuiltinMode,
) -> Result<CheckDiagnostics, RuntimeError> {
    check_source_with_profile(
        source,
        builtin_mode,
        default_semantic_profile_for_builtin(builtin_mode),
    )
}

/// Type-check source using an explicit semantic profile.
pub fn check_source_with_profile(
    source: &str,
    builtin_mode: BuiltinMode,
    profile: SemanticProfile,
) -> Result<CheckDiagnostics, RuntimeError> {
    check_source_with_profile_and_ts_options(source, builtin_mode, profile, None)
}

pub fn check_source_with_profile_and_ts_options(
    source: &str,
    builtin_mode: BuiltinMode,
    profile: SemanticProfile,
    ts_options: Option<&TsCompilerOptions>,
) -> Result<CheckDiagnostics, RuntimeError> {
    check_graph_source_with_profile_and_ts_options(source, builtin_mode, profile, ts_options)
}

/// Type-check already-linked module-graph source with an explicit semantic profile.
pub fn check_graph_source_with_profile_and_ts_options(
    source: &str,
    builtin_mode: BuiltinMode,
    profile: SemanticProfile,
    ts_options: Option<&TsCompilerOptions>,
) -> Result<CheckDiagnostics, RuntimeError> {
    GraphFrontend::new(source, builtin_mode, profile, ts_options)?.diagnostics()
}

fn validate_profile_constraints(
    builtin_mode: BuiltinMode,
    profile: SemanticProfile,
    ts_options: Option<&TsCompilerOptions>,
) -> Result<(), RuntimeError> {
    if builtin_mode == BuiltinMode::RayaStrict && !matches!(profile.source_kind, SourceKind::Raya) {
        return Err(RuntimeError::TypeCheck(
            "JS and TS semantic profiles require node-compat builtin mode.".to_string(),
        ));
    }
    let _ = ts_options;
    Ok(())
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
        format!("{} (line {})", error, abs_line)
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
        format!("{} (line {})", error, abs_line)
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

/// Detect duplicate top-level declarations in the submitted source.
fn precheck_user_top_level_duplicates(
    source: &str,
    profile: SemanticProfile,
) -> Result<(), RuntimeError> {
    let parser = parser_for_profile(source, profile).map_err(|errors| {
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

    // Linked module-graph source includes std module bodies. In strict mode we only
    // enforce node-compat symbol usage against user-authored modules, not std internals.
    let precheck_source = mask_linked_std_module_bodies(source);
    if let Some(found) =
        builtin_manifest::find_first_node_compat_symbol_usage(precheck_source.as_ref())
    {
        return Err(RuntimeError::TypeCheck(format!(
            "E_STRICT_NODE_COMPAT_SYMBOL: '{}' is node-compat-only (line {}). {}",
            found.symbol, found.line, found.hint
        )));
    }

    Ok(())
}

fn mask_linked_std_module_bodies(source: &str) -> Cow<'_, str> {
    if !source.contains("// module: std:") {
        return Cow::Borrowed(source);
    }

    let mut masked = String::with_capacity(source.len());
    let mut in_std_module_body = false;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(module_name) = trimmed.strip_prefix("// module: ") {
            in_std_module_body = module_name.trim().starts_with("std:");
        } else if trimmed.starts_with("// entry module: ") {
            in_std_module_body = false;
        }

        if in_std_module_body && !trimmed.starts_with("// module: ") {
            if line.ends_with('\n') {
                masked.push('\n');
            }
        } else {
            masked.push_str(line);
        }
    }

    Cow::Owned(masked)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn permissive_ts_options() -> TsCompilerOptions {
        TsCompilerOptions {
            strict: Some(false),
            no_implicit_any: Some(false),
            no_implicit_this: Some(false),
            strict_null_checks: Some(false),
            strict_property_initialization: Some(false),
            use_unknown_in_catch_variables: Some(false),
            exact_optional_property_types: Some(false),
            no_unchecked_indexed_access: Some(false),
            strict_function_types: Some(false),
            allow_js: None,
            other: HashMap::new(),
        }
    }

    #[test]
    fn test_check_valid_source() {
        let diag = check_source("let x = 1 + 2;").unwrap();
        assert!(diag.bind_errors.is_empty(), "Expected no bind errors");
        assert!(diag.errors.is_empty(), "Expected no check errors");
    }

    #[test]
    fn test_check_returns_user_offset() {
        let diag = check_source("let x = 1;").unwrap();
        assert_eq!(
            diag.user_offset, 0,
            "user_offset should be 0 in binary-link-only mode"
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
    fn test_check_source_returns_original_source() {
        let diag = check_source("let x = 1;").unwrap();
        assert_eq!(diag.source, "let x = 1;");
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
        let result = compile_source_with_mode(
            r#"
            import path from "node:path";
            return path.join("a", "b");
            "#,
            BuiltinMode::NodeCompat,
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
            "std:pm import should compile with transitive std dependencies, got: {:?}",
            result.err()
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
            "mixed std imports should compile without symbol collisions, got: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_std_encoding_default_import_compiles_in_strict_mode() {
        let result = compile_source(
            r#"
            import encoding from "std:encoding";
            const obj = encoding.json.newObject();
            obj.set("ok", encoding.json.fromBool(true));
            const ok = obj.get("ok").bool();
            obj.release();
            return ok;
            "#,
        );
        assert!(
            result.is_ok(),
            "std:encoding default import should compile in strict mode without recursive inference blowups, got: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_std_url_default_import_compiles_in_strict_mode() {
        let result = compile_source(
            r#"
            import url from "std:url";
            const encoded = url.encode("a b");
            const decoded = url.decode(encoded);
            return decoded == "a b";
            "#,
        );
        assert!(
            result.is_ok(),
            "std:url default import should compile in strict mode without recursive inference blowups"
        );
    }

    #[test]
    fn test_std_encoding_url_combined_import_compiles_in_strict_mode() {
        let result = compile_source(
            r#"
            import encoding from "std:encoding";
            import url from "std:url";
            const raw = url.decode("a%20b");
            const j = encoding.json.newObject();
            j.set("v", encoding.json.fromString(raw));
            const out = j.get("v").string();
            j.release();
            return out == "a b";
            "#,
        );
        assert!(
            result.is_ok(),
            "combined std:encoding/std:url graph should compile in strict mode, got: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_linked_program_allows_shadowing_builtin_names() {
        let result = compile_source_with_mode(
            r#"
            import path from "node:path";
            const Promise: number = 7;
            return Promise + path.sep.length;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "linked module source should allow shadowing builtin names"
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
        assert!(
            result.is_ok(),
            "namespace std import should be supported, got: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_node_events_import_is_supported() {
        let result = compile_source_with_mode(
            r#"
            import EventEmitter from "node:events";
            return EventEmitter != null;
            "#,
            BuiltinMode::NodeCompat,
        );
        assert!(
            result.is_ok(),
            "node:events should provide EventEmitter shim, got: {:?}",
            result.err()
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
            r#"import EventEmitter from "node:events"; return EventEmitter != null;"#,
        ];

        for source in cases {
            let result = compile_source_with_mode(source, BuiltinMode::NodeCompat);
            assert!(
                result.is_ok(),
                "node import smoke case failed: {source}, got: {:?}",
                result.err()
            );
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
        let ts_options = permissive_ts_options();
        let result = compile_source_with_profile_and_ts_options(
            r#"
            function id(x) { return x; }
            return id(1);
            "#,
            BuiltinMode::NodeCompat,
            SemanticProfile::ts_strict(),
            Some(&ts_options),
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
        let result = compile_source_with_profile(
            r#"
            function add(a: number, b: number): number { return a + b; }
            let plusOne = add.bind(null, 1);
            let x = plusOne(2);
            let y = add.call(null, 3, 4);
            let z = add.apply(null, [5, 6]);
            return x + y + z;
            "#,
            BuiltinMode::NodeCompat,
            SemanticProfile::raya(),
        );
        assert!(
            result.is_err(),
            "bind/call/apply are currently not resolved in strict dispatch mode and should fail fast"
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
        let any_result = compile_source_with_profile(
            r#"
            let x: any = 1;
            return x;
            "#,
            BuiltinMode::NodeCompat,
            SemanticProfile::raya(),
        );
        assert!(
            any_result.is_err(),
            "strict type mode should forbid explicit any"
        );

        let bare_let_result = compile_source_with_profile(
            r#"
            let x;
            x = 1;
            return x;
            "#,
            BuiltinMode::NodeCompat,
            SemanticProfile::raya(),
        );
        assert!(
            bare_let_result.is_err(),
            "strict type mode should forbid bare let even in node-compat builtins"
        );
    }

    #[test]
    fn test_nodecompat_allow_any_still_forbids_bare_let() {
        let any_result = compile_source_with_profile(
            r#"
            let x: any = 1;
            x = "ok";
            return x;
            "#,
            BuiltinMode::NodeCompat,
            SemanticProfile::ts_strict(),
        );
        assert!(
            any_result.is_ok(),
            "allowAny mode should allow explicit any"
        );

        let bare_let_result = compile_source_with_profile(
            r#"
            let x;
            x = 1;
            return x;
            "#,
            BuiltinMode::NodeCompat,
            SemanticProfile::ts_strict(),
        );
        assert!(
            bare_let_result.is_err(),
            "allowAny mode should still forbid untyped bare-let declarations"
        );
    }

    #[test]
    fn test_nodecompat_js_mode_allows_bare_let_and_any() {
        let result = compile_source_with_profile(
            r#"
            let x;
            x = 1;
            let y: any = x;
            return y;
            "#,
            BuiltinMode::NodeCompat,
            SemanticProfile::js(),
        );
        assert!(
            result.is_ok(),
            "jsMode should allow untyped variables and any semantics, got: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_nodecompat_js_mode_rejects_dot_monkeypatch_without_any_cast() {
        let result = compile_source_with_profile(
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
            SemanticProfile::js(),
        );
        assert!(
            result.is_err(),
            "dot field writes should be rejected unless object is explicitly any-casted"
        );
    }

    #[test]
    fn test_jsobject_wrapper_preserves_known_fields_from_base_type() {
        let result = compile_source_with_profile(
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
            SemanticProfile::js(),
        );
        assert!(
            result.is_ok(),
            "JSObject<T> should preserve known fields from T with normal typing"
        );
    }

    #[test]
    fn test_jsobject_wrapper_unknown_member_is_dynamic_any() {
        let result = check_source_with_profile(
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
            SemanticProfile::js(),
        )
        .expect("check_source_with_profile should produce diagnostics");

        assert!(
            result.errors.is_empty() && result.bind_errors.is_empty(),
            "unknown members on JSObject<T> should be dynamic in jsMode"
        );
    }

    #[test]
    fn test_jsobject_wrapper_keeps_known_monkeypatched_field_type() {
        let result = check_source_with_profile(
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
            SemanticProfile::ts_strict(),
        )
        .expect("check_source_with_profile should produce diagnostics");

        assert!(
            result.errors.is_empty() && result.bind_errors.is_empty(),
            "known monkeypatched fields should be tracked with concrete assigned type; check_errors={:?} bind_errors={:?}",
            result.errors,
            result.bind_errors
        );
    }

    #[test]
    fn test_allow_any_dot_write_existing_field_compiles_and_runs() {
        let compiled = compile_source_with_profile(
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
            SemanticProfile::ts_strict(),
        );
        assert!(
            compiled.is_ok(),
            "dot writes through explicit any should compile without lowering internal errors"
        );
    }

    #[test]
    fn test_allow_any_dot_write_unknown_field_compiles_without_internal_error() {
        let compiled = compile_source_with_profile(
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
            SemanticProfile::ts_strict(),
        );
        assert!(
            compiled.is_ok(),
            "unknown dot writes in dynamic-any flows should not fail lowering with internal compiler errors"
        );
    }

    #[test]
    fn test_nodecompat_allow_any_check_allows_dot_write_after_any_cast() {
        let result = check_source_with_profile(
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
            SemanticProfile::ts_strict(),
        )
        .expect("check_source_with_profile should return diagnostics");

        assert!(
            result.errors.is_empty() && result.bind_errors.is_empty(),
            "explicit any cast/annotation should allow dot monkeypatch at checker level"
        );
    }

    #[test]
    fn test_strict_ts_profile_rejects_loose_ts_options() {
        let err = check_source_with_profile_and_ts_options(
            "let value = 42; return value;",
            BuiltinMode::NodeCompat,
            SemanticProfile::ts_strict(),
            Some(&TsCompilerOptions {
                strict: Some(false),
                ..TsCompilerOptions::default()
            }),
        )
        .expect_err("strict TS semantic profile should reject loose compiler options");

        assert!(
            err.to_string().contains("strict TS compiler options"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_semantic_hir_tracks_raya_callable_and_await() {
        let hir = inspect_semantic_hir_with_profile(
            r#"
            function main(x: number): number {
                let y = await Promise.resolve(x + 1);
                return y;
            }
            "#,
            SemanticProfile::raya(),
        )
        .expect("semantic HIR should build");

        assert_eq!(hir.profile, SemanticProfile::raya());
        assert!(hir
            .callables
            .iter()
            .any(|callable| callable.name.as_deref() == Some("main")));
        assert!(hir.bindings.iter().any(|binding| {
            binding.name == "x" && binding.kind == raya_engine::BindingKind::Parameter
        }));
        assert!(hir
            .suspension_points
            .iter()
            .any(|point| point.kind == raya_engine::SuspensionKind::Await));
    }

    #[test]
    fn test_semantic_hir_tracks_js_reference_and_loop_plans() {
        let hir = inspect_semantic_hir_with_profile(
            r#"
            function main(source, obj) {
                let { [source()]: value = 1, plain } = obj;
                for (let i = 0; i < 3; i++) {
                    obj.count = i;
                }
                eval("value");
                return obj.count++;
            }
            "#,
            SemanticProfile::js(),
        )
        .expect("semantic HIR should build");

        assert!(hir.references.iter().any(|reference| {
            reference.kind == raya_engine::ReferenceExprKind::PropertyNamed
                && reference.name.as_deref() == Some("count")
        }));
        assert!(hir.call_ops.iter().any(|call| {
            call.kind == raya_engine::CallOpKind::DirectEval
        }));
        assert!(hir.update_ops.iter().any(|op| {
            op.kind == raya_engine::UpdateOpKind::PostfixIncrement
        }));
        assert!(hir.destructuring_plans.iter().any(|plan| {
            plan.has_computed_keys && plan.has_defaults && plan.binding_names.iter().any(|name| name == "value")
        }));
        assert!(hir.loop_scopes.iter().any(|plan| {
            plan.creates_per_iteration_env && plan.binding_names.iter().any(|name| name == "i")
        }));
    }

    #[test]
    fn test_parser_for_profile_uses_js_mode_for_contextual_keywords() {
        let parser = parser_for_profile(
            r#""use strict"; var public = 1; public;"#,
            SemanticProfile::js(),
        )
        .expect("js profile parser should lex");

        parser
            .parse()
            .expect("js profile parser should treat contextual keywords as identifier-like");
    }
}
