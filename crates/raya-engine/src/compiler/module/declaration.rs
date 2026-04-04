//! Declaration-module support for binary linking.
//!
//! This module parses declaration files (`.d.ts` subset), derives
//! canonical structural signatures, and builds export metadata for late-link
//! placeholder modules.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::compiler::{
    builtins::BuiltinRegistry, module_id_from_name, symbol_id_from_name, ModuleId, SymbolId,
    SymbolScope, SymbolType, TypeSignatureHash,
};
use crate::parser::ast::{
    self, ClassDecl, ClassMember, ExportDecl, ExportSpecifier, Expression, FunctionDecl,
    FunctionTypeParam, ImportSpecifier, ObjectTypeMember, Parameter, Pattern, PrimitiveType,
    Statement, Type, TypeAliasDecl, TypeAnnotation, TypeParameter, VariableDecl,
};
use crate::parser::checker::SymbolKind;
use crate::parser::types::signature_hash;
use crate::parser::types::{TypeContext, TypeId};
use crate::parser::{Interner, Parser};

use super::exports::{ExportedSymbol, ModuleExports};

fn class_member_key_name(interner: &Interner, key: &ast::PropertyKey) -> Option<String> {
    fn expr_name(interner: &Interner, expr: &Expression) -> Option<String> {
        match expr {
            Expression::StringLiteral(lit) => Some(interner.resolve(lit.value).to_string()),
            Expression::IntLiteral(lit) => Some(lit.value.to_string()),
            Expression::Parenthesized(expr) => expr_name(interner, &expr.expression),
            _ => None,
        }
    }

    match key {
        ast::PropertyKey::Identifier(id) => Some(interner.resolve(id.name).to_string()),
        ast::PropertyKey::StringLiteral(lit) => Some(interner.resolve(lit.value).to_string()),
        ast::PropertyKey::IntLiteral(lit) => Some(lit.value.to_string()),
        ast::PropertyKey::Computed(expr) => expr_name(interner, expr),
    }
}

/// Source format backing a declaration module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclarationSourceKind {
    DTs,
}

/// Builtin declaration surface mode used for global-symbol seeding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinSurfaceMode {
    RayaStrict,
    NodeCompat,
}

/// Parsed declaration module plus exported link metadata.
#[derive(Debug, Clone)]
pub struct DeclarationModule {
    /// Original declaration file path.
    pub declaration_path: PathBuf,
    /// Declaration file format.
    pub source_kind: DeclarationSourceKind,
    /// Normalized source used for parser/extractor passes.
    pub normalized_source: String,
    /// Canonical module identity used for module/symbol IDs.
    pub module_identity: String,
    /// Export metadata projected from declarations.
    pub exports: ModuleExports,
}

/// Late-link requirement for a symbol imported from a declaration-only module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LateLinkSymbolRequirement {
    pub symbol: String,
    pub symbol_id: SymbolId,
    pub scope: SymbolScope,
    pub symbol_type: SymbolType,
    pub signature_hash: TypeSignatureHash,
    pub type_signature: String,
    /// Generic template symbol for monomorphized exports (e.g. `identity` for
    /// `identity__mono_abcd1234`), when applicable.
    pub specialization_template: Option<String>,
}

/// Late-link requirement for one declaration-only target module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LateLinkRequirement {
    pub module_identity: String,
    pub module_id: ModuleId,
    pub declaration_path: PathBuf,
    pub source_kind: DeclarationSourceKind,
    pub module_specifiers: Vec<String>,
    pub symbols: Vec<LateLinkSymbolRequirement>,
}

/// Derive the generic template symbol name from a monomorphized symbol.
///
/// Returns `Some(template_symbol)` for symbols following `<template>__mono_<hash>`.
pub fn specialization_template_from_symbol(symbol: &str) -> Option<String> {
    let (template, hash) = symbol.split_once("__mono_")?;
    if template.is_empty() || hash.is_empty() {
        return None;
    }
    Some(template.to_string())
}

#[derive(Debug, Clone)]
struct DeclarationExport {
    name: String,
    kind: SymbolKind,
    is_const: bool,
    is_async: bool,
    canonical_signature: String,
}

#[derive(Debug, Clone)]
enum DeclarationItem {
    Function(ast::FunctionDecl),
    Class(ast::ClassDecl),
    TypeAlias(ast::TypeAliasDecl),
    Variable(ast::VariableDecl),
}

#[derive(Debug, Error)]
pub enum DeclarationError {
    #[error("IO error reading declaration file '{path}': {message}")]
    IoError { path: PathBuf, message: String },

    #[error("Lexer error in declaration file '{path}': {message}")]
    LexError { path: PathBuf, message: String },

    #[error("Parse error in declaration file '{path}': {message}")]
    ParseError { path: PathBuf, message: String },

    #[error("Unsupported .d.ts syntax in '{path}' at line {line}, column {column}: {snippet}")]
    UnsupportedTsSyntax {
        path: PathBuf,
        line: u32,
        column: u32,
        snippet: String,
    },

    #[error("Invalid declaration in '{path}' at line {line}, column {column}: {message}")]
    InvalidDeclaration {
        path: PathBuf,
        line: u32,
        column: u32,
        message: String,
    },
}

impl DeclarationSourceKind {
    pub fn from_path(path: &Path) -> Option<Self> {
        let file_name = path.file_name()?.to_string_lossy();
        if file_name.ends_with(".d.ts") {
            Some(Self::DTs)
        } else {
            None
        }
    }
}

impl DeclarationItem {
    fn name(&self, interner: &Interner) -> Option<String> {
        match self {
            DeclarationItem::Function(func) => Some(interner.resolve(func.name.name).to_string()),
            DeclarationItem::Class(class) => Some(interner.resolve(class.name.name).to_string()),
            DeclarationItem::TypeAlias(alias) => {
                Some(interner.resolve(alias.name.name).to_string())
            }
            DeclarationItem::Variable(var) => match &var.pattern {
                Pattern::Identifier(ident) => Some(interner.resolve(ident.name).to_string()),
                _ => None,
            },
        }
    }
}

/// Parse declaration file and project exported symbol metadata.
pub fn load_declaration_module(
    declaration_path: &Path,
    module_identity: &str,
    virtual_module_path: &Path,
) -> Result<DeclarationModule, DeclarationError> {
    let source_kind = DeclarationSourceKind::from_path(declaration_path).ok_or_else(|| {
        DeclarationError::InvalidDeclaration {
            path: declaration_path.to_path_buf(),
            line: 1,
            column: 1,
            message: "Declaration path must end with .d.ts".to_string(),
        }
    })?;

    let raw_source =
        fs::read_to_string(declaration_path).map_err(|e| DeclarationError::IoError {
            path: declaration_path.to_path_buf(),
            message: e.to_string(),
        })?;

    load_declaration_module_from_source(
        declaration_path,
        source_kind,
        &raw_source,
        module_identity,
        virtual_module_path,
    )
}

/// Parse declaration source and project exported symbol metadata.
pub fn load_declaration_module_from_source(
    declaration_path: &Path,
    source_kind: DeclarationSourceKind,
    raw_source: &str,
    module_identity: &str,
    virtual_module_path: &Path,
) -> Result<DeclarationModule, DeclarationError> {
    if source_kind == DeclarationSourceKind::DTs {
        detect_unsupported_dts_syntax(declaration_path, raw_source)?;
    }

    let normalized_source = normalize_declaration_source(source_kind, raw_source);

    let parser = Parser::new(&normalized_source).map_err(|errors| DeclarationError::LexError {
        path: declaration_path.to_path_buf(),
        message: errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("; "),
    })?;

    let (ast, interner) = parser
        .parse()
        .map_err(|errors| DeclarationError::ParseError {
            path: declaration_path.to_path_buf(),
            message: errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        })?;

    let exports = extract_declaration_exports(
        declaration_path,
        module_identity,
        virtual_module_path,
        &ast,
        &interner,
    )?;

    Ok(DeclarationModule {
        declaration_path: declaration_path.to_path_buf(),
        source_kind,
        normalized_source,
        module_identity: module_identity.to_string(),
        exports,
    })
}

/// Load embedded builtin declaration exports for global checker seeding.
pub fn builtin_global_exports(mode: BuiltinSurfaceMode) -> Result<ModuleExports, DeclarationError> {
    let module_name = match mode {
        BuiltinSurfaceMode::RayaStrict => "__raya_builtin__/strict".to_string(),
        BuiltinSurfaceMode::NodeCompat => "__raya_builtin__/node_compat".to_string(),
    };
    let path = PathBuf::from(format!("{module_name}.raya"));
    let module_id = module_id_from_name(&module_name);
    let mut merged = ModuleExports::new(path, module_name.clone());
    let placeholder_signature = "unknown".to_string();
    let placeholder_hash = signature_hash(&placeholder_signature);

    for (global_name, descriptor) in BuiltinRegistry::shared().global_descriptors() {
        if !builtin_root_visible_in_mode(global_name, mode) {
            continue;
        }
        let kind = registry_global_symbol_kind(global_name, descriptor);
        merged.add_symbol(ExportedSymbol {
            name: global_name.to_string(),
            local_name: global_name.to_string(),
            kind,
            ty: TypeId::new(TypeContext::UNKNOWN_TYPE_ID),
            is_const: true,
            is_async: false,
            module_name: module_name.clone(),
            module_id,
            symbol_id: symbol_id_from_name(&module_name, SymbolScope::Module, global_name),
            signature_hash: placeholder_hash,
            type_signature: registry_global_type_signature(global_name, descriptor),
            scope: SymbolScope::Module,
        });
    }

    for signatures in crate::vm::builtins::get_all_signatures() {
        for class in signatures.classes {
            if class.name.starts_with("__")
                || !builtin_root_visible_in_mode(class.name, mode)
                || merged.has(class.name)
            {
                continue;
            }
            let kind = if class.constructor.is_some() {
                SymbolKind::Class
            } else {
                SymbolKind::Variable
            };
            merged.add_symbol(ExportedSymbol {
                name: class.name.to_string(),
                local_name: class.name.to_string(),
                kind,
                ty: TypeId::new(TypeContext::UNKNOWN_TYPE_ID),
                is_const: true,
                is_async: false,
                module_name: module_name.clone(),
                module_id,
                symbol_id: symbol_id_from_name(&module_name, SymbolScope::Module, class.name),
                signature_hash: signature_hash(class.name),
                type_signature: class.name.to_string(),
                scope: SymbolScope::Module,
            });
        }

        for function in signatures.functions {
            if !builtin_root_visible_in_mode(function.name, mode) || merged.has(function.name) {
                continue;
            }
            merged.add_symbol(ExportedSymbol {
                name: function.name.to_string(),
                local_name: function.name.to_string(),
                kind: SymbolKind::Function,
                ty: TypeId::new(TypeContext::UNKNOWN_TYPE_ID),
                is_const: true,
                is_async: false,
                module_name: module_name.clone(),
                module_id,
                symbol_id: symbol_id_from_name(&module_name, SymbolScope::Module, function.name),
                signature_hash: placeholder_hash,
                type_signature: placeholder_signature.clone(),
                scope: SymbolScope::Module,
            });
        }
    }

    if matches!(mode, BuiltinSurfaceMode::NodeCompat) && !merged.has("globalThis") {
        merged.add_symbol(ExportedSymbol {
            name: "globalThis".to_string(),
            local_name: "globalThis".to_string(),
            kind: SymbolKind::Variable,
            ty: TypeId::new(TypeContext::UNKNOWN_TYPE_ID),
            is_const: true,
            is_async: false,
            module_name: module_name.clone(),
            module_id,
            symbol_id: symbol_id_from_name(&module_name, SymbolScope::Module, "globalThis"),
            signature_hash: placeholder_hash,
            type_signature: placeholder_signature,
            scope: SymbolScope::Module,
        });
    }

    Ok(merged)
}

fn registry_global_symbol_kind(
    global_name: &str,
    descriptor: &crate::compiler::builtins::BuiltinGlobalDescriptor,
) -> SymbolKind {
    if descriptor.symbol_type == SymbolType::Function {
        return SymbolKind::Function;
    }
    if descriptor.symbol_type == SymbolType::Class {
        return SymbolKind::Class;
    }
    if BuiltinRegistry::shared()
        .type_descriptor(descriptor.backing_type_name)
        .is_some_and(|surface| surface.constructor.is_some())
    {
        return SymbolKind::Class;
    }
    if global_name == "globalThis" {
        return SymbolKind::Variable;
    }
    SymbolKind::Variable
}

fn registry_global_type_signature(
    global_name: &str,
    descriptor: &crate::compiler::builtins::BuiltinGlobalDescriptor,
) -> String {
    match registry_global_symbol_kind(global_name, descriptor) {
        SymbolKind::Class => descriptor.backing_type_name.to_string(),
        _ => "unknown".to_string(),
    }
}

fn builtin_root_visible_in_mode(name: &str, mode: BuiltinSurfaceMode) -> bool {
    if matches!(mode, BuiltinSurfaceMode::NodeCompat) {
        return true;
    }
    !matches!(
        name,
        "ArrayBuffer"
            | "DataView"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Int8Array"
            | "Int16Array"
            | "Int32Array"
            | "Uint16Array"
            | "Uint32Array"
            | "Float32Array"
            | "Float16Array"
            | "Float64Array"
            | "BigInt"
            | "BigInt64Array"
            | "BigUint64Array"
            | "TypedArray"
            | "SharedArrayBuffer"
            | "Atomics"
            | "parseInt"
            | "parseFloat"
            | "isNaN"
            | "isFinite"
            | "eval"
            | "Function"
            | "AsyncFunction"
            | "Generator"
            | "GeneratorFunction"
            | "AsyncGenerator"
            | "AsyncGeneratorFunction"
            | "AsyncIterator"
            | "Proxy"
            | "Reflect"
            | "WeakMap"
            | "WeakSet"
            | "WeakRef"
            | "FinalizationRegistry"
            | "DisposableStack"
            | "AsyncDisposableStack"
            | "Intl"
            | "globalThis"
            | "escape"
            | "unescape"
    )
}

fn detect_unsupported_dts_syntax(path: &Path, source: &str) -> Result<(), DeclarationError> {
    const UNSUPPORTED_PREFIXES: &[&str] = &[
        "enum ",
        "export enum ",
        "namespace ",
        "export namespace ",
        "declare global",
        "module ",
        "export =",
        "import type ",
    ];

    for (line_index, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        for marker in UNSUPPORTED_PREFIXES {
            if trimmed.starts_with(marker) {
                let column = (line.len() - trimmed.len()) as u32 + 1;
                return Err(DeclarationError::UnsupportedTsSyntax {
                    path: path.to_path_buf(),
                    line: line_index as u32 + 1,
                    column,
                    snippet: trimmed.to_string(),
                });
            }
        }
    }

    Ok(())
}

fn normalize_declaration_source(_kind: DeclarationSourceKind, source: &str) -> String {
    let mut normalized = String::with_capacity(source.len());
    for line in source.lines() {
        let line = normalize_dts_line(line);
        normalized.push_str(&line);
        normalized.push('\n');
    }
    materialize_declaration_stubs(&normalized)
}

fn normalize_dts_line(line: &str) -> String {
    let trimmed_start = line.trim_start();
    let indent_len = line.len().saturating_sub(trimmed_start.len());
    let indent = &line[..indent_len];

    if let Some(rest) = trimmed_start.strip_prefix("export declare ") {
        return format!("{}export {}", indent, rest);
    }
    if let Some(rest) = trimmed_start.strip_prefix("declare ") {
        return format!("{}{}", indent, rest);
    }

    line.to_string()
}

fn is_class_declaration_start(trimmed_start: &str) -> bool {
    (trimmed_start.starts_with("class ")
        || trimmed_start.starts_with("abstract class ")
        || trimmed_start.starts_with("export class ")
        || trimmed_start.starts_with("export abstract class "))
        && trimmed_start.contains('{')
}

fn materialize_declaration_stubs(source: &str) -> String {
    let mut out_lines = Vec::<String>::new();
    let mut class_depth = 0i32;
    let mut pending_callable_line: Option<usize> = None;

    for line in source.lines() {
        let mut normalized = line.to_string();
        let trimmed = normalized.trim();
        let trimmed_start = normalized.trim_start();

        let starts_class_decl = is_class_declaration_start(trimmed_start);
        let open = line.chars().filter(|c| *c == '{').count() as i32;
        let close = line.chars().filter(|c| *c == '}').count() as i32;

        if let Some(start_idx) = pending_callable_line {
            let _ = start_idx;
            if trimmed.ends_with(';') {
                if let Some(index) = normalized.rfind(';') {
                    normalized.replace_range(
                        index..=index,
                        " { throw new Error(\"__raya_decl_stub__\"); }",
                    );
                }
                pending_callable_line = None;
            }
            out_lines.push(normalized);
        } else {
            let is_variable_signature = trimmed.ends_with(';')
                && !trimmed.contains('=')
                && trimmed.contains(':')
                && (trimmed.starts_with("export const ")
                    || trimmed.starts_with("const ")
                    || trimmed.starts_with("export let ")
                    || trimmed.starts_with("let "));

            let is_top_level_fn_start = trimmed.starts_with("export function ")
                || trimmed.starts_with("function ")
                || trimmed.starts_with("export async function ")
                || trimmed.starts_with("async function ");

            let is_class_member_start = class_depth > 0
                && !trimmed.is_empty()
                && !trimmed.starts_with('}')
                && !trimmed.starts_with('{')
                && !trimmed.starts_with("export ")
                && !trimmed.starts_with("import ")
                && !trimmed.starts_with("type ")
                && !trimmed.starts_with("interface ")
                && !trimmed.starts_with("//")
                && !trimmed.starts_with("/*")
                && !trimmed.starts_with('*');

            let starts_with_member_token = trimmed
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');

            // Class fields can legally have function types:
            //   field: (x: T) => U;
            // Those are not callable declarations and must not receive
            // stub bodies. Distinguish by checking whether the first colon
            // appears before the first opening parenthesis.
            let first_colon = trimmed.find(':');
            let first_paren = trimmed.find('(');
            let has_field_type_annotation = matches!(
                (first_colon, first_paren),
                (Some(colon), Some(paren)) if colon < paren
            );

            let callable_signature_start = trimmed.contains('(')
                && !trimmed.starts_with("abstract ")
                && !trimmed.contains('{')
                && starts_with_member_token
                && !has_field_type_annotation
                && (is_top_level_fn_start || is_class_member_start);

            if callable_signature_start {
                if trimmed.ends_with(';') {
                    if let Some(index) = normalized.rfind(';') {
                        normalized.replace_range(
                            index..=index,
                            " { throw new Error(\"__raya_decl_stub__\"); }",
                        );
                    }
                } else {
                    pending_callable_line = Some(out_lines.len());
                }
            } else if is_variable_signature {
                if let Some(index) = normalized.rfind(';') {
                    normalized.replace_range(index..=index, " = null;");
                }
            }

            out_lines.push(normalized);
        }

        if class_depth > 0 || starts_class_decl {
            class_depth = (class_depth + open - close).max(0);
        }
    }

    let mut out = String::with_capacity(source.len() + 128);
    for line in out_lines {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

fn extract_declaration_exports(
    declaration_path: &Path,
    module_identity: &str,
    virtual_module_path: &Path,
    ast: &ast::Module,
    interner: &Interner,
) -> Result<ModuleExports, DeclarationError> {
    let mut locals = HashMap::<String, DeclarationItem>::new();

    for stmt in &ast.statements {
        if let Some(item) = declaration_item_from_statement(stmt) {
            if let Some(name) = item.name(interner) {
                locals.insert(name, item);
            }
            continue;
        }

        if let Statement::ExportDecl(ExportDecl::Declaration(inner)) = stmt {
            if let Some(item) = declaration_item_from_statement(inner) {
                if let Some(name) = item.name(interner) {
                    locals.insert(name, item);
                }
            }
        }
    }

    let mut explicit_exports = Vec::<DeclarationExport>::new();
    for stmt in &ast.statements {
        if let Statement::ExportDecl(export_decl) = stmt {
            match export_decl {
                ExportDecl::Declaration(inner) => {
                    let export = export_from_statement(inner, interner, &locals, declaration_path)?;
                    if let Some(export) = export {
                        explicit_exports.push(export);
                    }
                }
                ExportDecl::Named {
                    specifiers,
                    source: None,
                    ..
                } => {
                    for spec in specifiers {
                        let local_name = interner.resolve(spec.name.name).to_string();
                        let exported_name = spec
                            .alias
                            .as_ref()
                            .map(|alias| interner.resolve(alias.name).to_string())
                            .unwrap_or_else(|| local_name.clone());
                        let Some(local_item) = locals.get(&local_name) else {
                            return Err(DeclarationError::InvalidDeclaration {
                                path: declaration_path.to_path_buf(),
                                line: spec.name.span.line,
                                column: spec.name.span.column,
                                message: format!(
                                    "Named export '{}' does not reference a declaration in this file",
                                    local_name
                                ),
                            });
                        };

                        if let Some(export) =
                            export_from_item(local_item, &exported_name, interner, &locals)?
                        {
                            explicit_exports.push(export);
                        }
                    }
                }
                ExportDecl::Named {
                    source: Some(source),
                    ..
                }
                | ExportDecl::All { source, .. } => {
                    return Err(DeclarationError::InvalidDeclaration {
                        path: declaration_path.to_path_buf(),
                        line: source.span.line,
                        column: source.span.column,
                        message:
                            "Re-exports are not currently supported in declaration-only modules"
                                .to_string(),
                    });
                }
                ExportDecl::Default { expression, span } => {
                    let Expression::Identifier(identifier) = expression.as_ref() else {
                        return Err(DeclarationError::InvalidDeclaration {
                            path: declaration_path.to_path_buf(),
                            line: span.line,
                            column: span.column,
                            message:
                                "Default export in declaration files must reference a declared symbol"
                                    .to_string(),
                        });
                    };
                    let local_name = interner.resolve(identifier.name).to_string();
                    let Some(local_item) = locals.get(&local_name) else {
                        return Err(DeclarationError::InvalidDeclaration {
                            path: declaration_path.to_path_buf(),
                            line: identifier.span.line,
                            column: identifier.span.column,
                            message: format!(
                                "Default export '{}' does not reference a declaration in this file",
                                local_name
                            ),
                        });
                    };
                    if let Some(export) =
                        export_from_item(local_item, "default", interner, &locals)?
                    {
                        explicit_exports.push(export);
                    }
                }
            }
        }
    }

    let mut module_exports = ModuleExports::new(
        virtual_module_path.to_path_buf(),
        module_identity.to_string(),
    );

    for export in explicit_exports {
        let symbol_id = symbol_id_from_name(module_identity, SymbolScope::Module, &export.name);
        module_exports.add_symbol(ExportedSymbol {
            name: export.name.clone(),
            local_name: export.name,
            kind: export.kind,
            ty: TypeId::new(TypeContext::UNKNOWN_TYPE_ID),
            is_const: export.is_const,
            is_async: export.is_async,
            module_name: module_identity.to_string(),
            module_id: module_id_from_name(module_identity),
            symbol_id,
            signature_hash: signature_hash(&export.canonical_signature),
            type_signature: export.canonical_signature,
            scope: SymbolScope::Module,
        });
    }

    Ok(module_exports)
}

fn declaration_item_from_statement(stmt: &Statement) -> Option<DeclarationItem> {
    match stmt {
        Statement::FunctionDecl(func) => Some(DeclarationItem::Function(func.clone())),
        Statement::ClassDecl(class) => Some(DeclarationItem::Class(class.clone())),
        Statement::TypeAliasDecl(alias) => Some(DeclarationItem::TypeAlias(alias.clone())),
        Statement::VariableDecl(var) => Some(DeclarationItem::Variable(var.clone())),
        _ => None,
    }
}

fn export_from_statement(
    stmt: &Statement,
    interner: &Interner,
    locals: &HashMap<String, DeclarationItem>,
    path: &Path,
) -> Result<Option<DeclarationExport>, DeclarationError> {
    let Some(item) = declaration_item_from_statement(stmt) else {
        return Ok(None);
    };
    let Some(name) = item.name(interner) else {
        return Err(DeclarationError::InvalidDeclaration {
            path: path.to_path_buf(),
            line: stmt.span().line,
            column: stmt.span().column,
            message: "Destructuring exports are not supported in declaration files".to_string(),
        });
    };
    export_from_item(&item, &name, interner, locals)
}

fn export_from_item(
    item: &DeclarationItem,
    export_name: &str,
    interner: &Interner,
    locals: &HashMap<String, DeclarationItem>,
) -> Result<Option<DeclarationExport>, DeclarationError> {
    let mut canonicalizer = DeclarationCanonicalizer::new(interner, locals);

    let export = match item {
        DeclarationItem::Function(func) => {
            let canonical_signature = canonicalizer.canonical_function_decl(func);
            Some(DeclarationExport {
                name: export_name.to_string(),
                kind: SymbolKind::Function,
                is_const: true,
                is_async: func.is_async,
                canonical_signature,
            })
        }
        DeclarationItem::Class(class) => {
            let canonical_signature = canonicalizer.canonical_class_decl(class);
            Some(DeclarationExport {
                name: export_name.to_string(),
                kind: SymbolKind::Class,
                is_const: true,
                is_async: false,
                canonical_signature,
            })
        }
        DeclarationItem::Variable(var) => {
            let Pattern::Identifier(identifier) = &var.pattern else {
                return Ok(None);
            };
            let Some(type_annotation) = &var.type_annotation else {
                return Ok(None);
            };
            let _ = identifier;
            let canonical_signature = canonicalizer.canonical_type_annotation(type_annotation);
            Some(DeclarationExport {
                name: export_name.to_string(),
                kind: SymbolKind::Variable,
                is_const: var.kind == ast::VariableKind::Const,
                is_async: false,
                canonical_signature,
            })
        }
        DeclarationItem::TypeAlias(alias) => {
            let canonical_signature = canonicalizer
                .with_type_params(alias.type_params.as_deref(), |this| {
                    this.canonical_type_annotation(&alias.type_annotation)
                });
            Some(DeclarationExport {
                name: export_name.to_string(),
                kind: SymbolKind::TypeAlias,
                is_const: true,
                is_async: false,
                canonical_signature,
            })
        }
    };

    Ok(export)
}

#[derive(Debug, Clone)]
struct TypeVarBinding {
    canonical_name: String,
    constraint: String,
    default: String,
}

struct DeclarationCanonicalizer<'a> {
    interner: &'a Interner,
    locals: &'a HashMap<String, DeclarationItem>,
    alias_stack: HashSet<String>,
    type_var_scopes: Vec<HashMap<String, TypeVarBinding>>,
    next_type_var: usize,
}

impl<'a> DeclarationCanonicalizer<'a> {
    fn new(interner: &'a Interner, locals: &'a HashMap<String, DeclarationItem>) -> Self {
        Self {
            interner,
            locals,
            alias_stack: HashSet::new(),
            type_var_scopes: Vec::new(),
            next_type_var: 0,
        }
    }

    fn canonical_function_decl(&mut self, func: &FunctionDecl) -> String {
        self.with_type_params(func.type_params.as_deref(), |this| {
            this.canonical_callable_shape(
                &func.params,
                func.return_type.as_ref(),
                func.is_async,
                true,
            )
        })
    }

    fn canonical_class_decl(&mut self, class: &ClassDecl) -> String {
        self.with_type_params(class.type_params.as_deref(), |this| {
            let mut members = BTreeSet::new();

            for member in &class.members {
                match member {
                    ClassMember::Field(field) => {
                        if field.visibility != ast::Visibility::Public {
                            continue;
                        }
                        let readonly = if field.is_readonly { "ro" } else { "rw" };
                        let optional = "req";
                        let ty = field
                            .type_annotation
                            .as_ref()
                            .map(|ann| this.canonical_type_annotation(ann))
                            .unwrap_or_else(|| "unknown".to_string());
                        let prefix = if field.is_static {
                            "static_prop"
                        } else {
                            "inst_prop"
                        };
                        let field_name = class_member_key_name(this.interner, &field.name)
                            .unwrap_or_else(|| "[computed]".to_string());
                        members.insert(format!(
                            "{}:{}:{}:{}:{}",
                            prefix,
                            escape(&field_name),
                            readonly,
                            optional,
                            ty
                        ));
                    }
                    ClassMember::Method(method) => {
                        if method.visibility != ast::Visibility::Public {
                            continue;
                        }
                        let method_sig =
                            this.with_type_params(method.type_params.as_deref(), |this| {
                                this.canonical_callable_shape(
                                    &method.params,
                                    method.return_type.as_ref(),
                                    method.is_async,
                                    true,
                                )
                            });
                        let prefix = if method.is_static {
                            "static_method"
                        } else {
                            "inst_method"
                        };
                        let method_name = class_member_key_name(this.interner, &method.name)
                            .unwrap_or_else(|| "[computed]".to_string());
                        members.insert(format!(
                            "{}:{}:{}",
                            prefix,
                            escape(&method_name),
                            method_sig
                        ));
                    }
                    ClassMember::Constructor(_) | ClassMember::StaticBlock(_) => {
                        // Constructors/static blocks are not part of structural public API hash.
                    }
                }
            }

            if let Some(extends) = &class.extends {
                members.insert(format!(
                    "extends:{}",
                    this.canonical_type_annotation(extends)
                ));
            }

            if !class.implements.is_empty() {
                let mut impls = class
                    .implements
                    .iter()
                    .map(|ty| this.canonical_type_annotation(ty))
                    .collect::<Vec<_>>();
                impls.sort_unstable();
                impls.dedup();
                members.insert(format!("implements:[{}]", impls.join(",")));
            }

            format!(
                "class_pub({})",
                members.into_iter().collect::<Vec<_>>().join(",")
            )
        })
    }

    fn canonical_callable_shape(
        &mut self,
        params: &[Parameter],
        return_type: Option<&TypeAnnotation>,
        is_async: bool,
        include_default_values: bool,
    ) -> String {
        let mut min_params = 0usize;
        let mut saw_optional = false;
        let mut positional = Vec::new();
        let mut rest = None;

        for param in params {
            let ty = param
                .type_annotation
                .as_ref()
                .map(|ann| self.canonical_type_annotation(ann))
                .unwrap_or_else(|| "unknown".to_string());

            if param.is_rest {
                rest = Some(ty);
                continue;
            }

            if !saw_optional
                && !param.optional
                && (!include_default_values || param.default_value.is_none())
            {
                min_params += 1;
            } else {
                saw_optional = true;
            }

            positional.push(ty);
        }

        let rest = rest.unwrap_or_else(|| "_".to_string());
        let ret = if is_async {
            let resolved = return_type
                .map(|ret| self.extract_promise_inner(ret))
                .unwrap_or_else(|| "void".to_string());
            format!("Promise<{}>", resolved)
        } else {
            return_type
                .map(|ret| self.canonical_type_annotation(ret))
                .unwrap_or_else(|| "void".to_string())
        };

        format!(
            "fn(min={},params=[{}],rest={},ret={})",
            min_params,
            positional.join(","),
            rest,
            ret
        )
    }

    fn extract_promise_inner(&mut self, return_type: &TypeAnnotation) -> String {
        if let Type::Reference(reference) = &return_type.ty {
            let name = self.interner.resolve(reference.name.name);
            if name == "Promise" {
                if let Some(args) = &reference.type_args {
                    if let Some(inner) = args.first() {
                        return self.canonical_type_annotation(inner);
                    }
                }
            }
        }

        self.canonical_type_annotation(return_type)
    }

    fn canonical_type_annotation(&mut self, type_ann: &TypeAnnotation) -> String {
        match &type_ann.ty {
            Type::Primitive(primitive) => canonical_primitive(*primitive).to_string(),
            Type::StringLiteral(symbol) => {
                format!("strlit({})", escape(self.interner.resolve(*symbol)))
            }
            Type::NumberLiteral(value) => format!("numlit({:x})", value.to_bits()),
            Type::BooleanLiteral(value) => format!("boollit({})", if *value { "1" } else { "0" }),
            Type::Array(array) => {
                format!(
                    "arr({})",
                    self.canonical_type_annotation(&array.element_type)
                )
            }
            Type::Tuple(tuple) => format!(
                "tuple({})",
                tuple
                    .element_types
                    .iter()
                    .map(|elem| self.canonical_type_annotation(elem))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Type::Object(object) => {
                let mut members = BTreeSet::new();
                for member in &object.members {
                    match member {
                        ObjectTypeMember::Property(prop) => {
                            let readonly = if prop.readonly { "ro" } else { "rw" };
                            let optional = if prop.optional { "opt" } else { "req" };
                            members.insert(format!(
                                "prop:{}:{}:{}:{}",
                                escape(self.interner.resolve(prop.name.name)),
                                readonly,
                                optional,
                                self.canonical_type_annotation(&prop.ty)
                            ));
                        }
                        ObjectTypeMember::Method(method) => {
                            let fn_sig = self.canonical_function_type_params(
                                &method.params,
                                &method.return_type,
                            );
                            let optional = if method.optional { "opt" } else { "req" };
                            members.insert(format!(
                                "prop:{}:rw:{}:{}",
                                escape(self.interner.resolve(method.name.name)),
                                optional,
                                fn_sig
                            ));
                        }
                        ObjectTypeMember::IndexSignature(index) => {
                            members.insert(format!(
                                "index:{}:{}",
                                escape(self.interner.resolve(index.key_name.name)),
                                self.canonical_type_annotation(&index.value_type)
                            ));
                        }
                        ObjectTypeMember::CallSignature(call_sig) => {
                            let fn_sig = self.canonical_function_type_params(
                                &call_sig.params,
                                &call_sig.return_type,
                            );
                            members.insert(format!("call:{}", fn_sig));
                        }
                        ObjectTypeMember::ConstructSignature(ctor_sig) => {
                            let fn_sig = self.canonical_function_type_params(
                                &ctor_sig.params,
                                &ctor_sig.return_type,
                            );
                            members.insert(format!("ctor:{}", fn_sig));
                        }
                    }
                }
                format!("obj({})", members.into_iter().collect::<Vec<_>>().join(","))
            }
            Type::Function(function_type) => {
                let mut min_params = 0usize;
                let mut saw_optional = false;
                let mut positional = Vec::new();
                let mut rest = None;

                for param in &function_type.params {
                    let ty = self.canonical_type_annotation(&param.ty);
                    if param.is_rest {
                        rest = Some(ty);
                        continue;
                    }
                    if !saw_optional && !param.optional {
                        min_params += 1;
                    } else {
                        saw_optional = true;
                    }
                    positional.push(ty);
                }

                let rest = rest.unwrap_or_else(|| "_".to_string());
                let ret = self.canonical_type_annotation(&function_type.return_type);
                format!(
                    "fn(min={},params=[{}],rest={},ret={})",
                    min_params,
                    positional.join(","),
                    rest,
                    ret
                )
            }
            Type::Union(union) => self.canonical_union(union),
            Type::Intersection(intersection) => {
                let mut members = intersection
                    .types
                    .iter()
                    .map(|member| self.canonical_type_annotation(member))
                    .collect::<Vec<_>>();
                members.sort_unstable();
                members.dedup();
                format!("intersection({})", members.join("&"))
            }
            Type::Reference(reference) => self.canonical_reference(reference),
            Type::Keyof(keyof) => format!(
                "keyof({})",
                self.canonical_type_annotation(keyof.target.as_ref())
            ),
            Type::IndexedAccess(indexed) => format!(
                "index({}, {})",
                self.canonical_type_annotation(indexed.object.as_ref()),
                self.canonical_type_annotation(indexed.index.as_ref())
            ),
            Type::Parenthesized(inner) => self.canonical_type_annotation(inner),
            Type::Typeof(_) => "unknown".to_string(),
        }
    }

    fn canonical_function_type_params(
        &mut self,
        params: &[FunctionTypeParam],
        return_type: &TypeAnnotation,
    ) -> String {
        let mut min_params = 0usize;
        let mut saw_optional = false;
        let mut positional = Vec::new();
        let mut rest = None;

        for param in params {
            let ty = self.canonical_type_annotation(&param.ty);
            if param.is_rest {
                rest = Some(ty);
                continue;
            }
            if !saw_optional && !param.optional {
                min_params += 1;
            } else {
                saw_optional = true;
            }
            positional.push(ty);
        }

        let rest = rest.unwrap_or_else(|| "_".to_string());
        let ret = self.canonical_type_annotation(return_type);
        format!(
            "fn(min={},params=[{}],rest={},ret={})",
            min_params,
            positional.join(","),
            rest,
            ret
        )
    }

    fn canonical_union(&mut self, union: &ast::UnionType) -> String {
        let mut flattened = Vec::new();
        let mut seen = HashSet::new();
        self.collect_union_members(union, &mut flattened, &mut seen);
        flattened.sort_unstable();
        format!("union({})", flattened.join("|"))
    }

    fn collect_union_members(
        &mut self,
        union: &ast::UnionType,
        out: &mut Vec<String>,
        seen: &mut HashSet<String>,
    ) {
        for member in &union.types {
            if let Type::Union(nested) = &member.ty {
                self.collect_union_members(nested, out, seen);
                continue;
            }
            let canonical = self.canonical_type_annotation(member);
            if seen.insert(canonical.clone()) {
                out.push(canonical);
            }
        }
    }

    fn canonical_reference(&mut self, reference: &ast::TypeReference) -> String {
        let name = self.interner.resolve(reference.name.name).to_string();

        if let Some(binding) = self.lookup_type_var(&name) {
            return format!(
                "tv({};extends={};default={})",
                binding.canonical_name, binding.constraint, binding.default
            );
        }

        if let Some(type_args) = &reference.type_args {
            if name == "Array" && type_args.len() == 1 {
                return format!("arr({})", self.canonical_type_annotation(&type_args[0]));
            }
            if name == "Promise" && type_args.len() == 1 {
                return format!("Promise<{}>", self.canonical_type_annotation(&type_args[0]));
            }
            if name == "Map" && type_args.len() == 2 {
                return format!(
                    "Map<{},{}>",
                    self.canonical_type_annotation(&type_args[0]),
                    self.canonical_type_annotation(&type_args[1])
                );
            }
            if name == "Set" && type_args.len() == 1 {
                return format!("Set<{}>", self.canonical_type_annotation(&type_args[0]));
            }
            if name == "Channel" && type_args.len() == 1 {
                return format!("Channel<{}>", self.canonical_type_annotation(&type_args[0]));
            }

            let rendered_args = type_args
                .iter()
                .map(|arg| self.canonical_type_annotation(arg))
                .collect::<Vec<_>>()
                .join(",");
            return format!("ref({},[{}])", escape(&name), rendered_args);
        }

        if let Some(DeclarationItem::TypeAlias(alias)) = self.locals.get(&name) {
            if self.alias_stack.insert(name.clone()) {
                let canonical = format!(
                    "alias({})",
                    self.canonical_type_annotation(&alias.type_annotation)
                );
                self.alias_stack.remove(&name);
                return canonical;
            }
        }

        format!("ref({},[])", escape(&name))
    }

    fn with_type_params<F>(&mut self, params: Option<&[TypeParameter]>, f: F) -> String
    where
        F: FnOnce(&mut Self) -> String,
    {
        let Some(params) = params else {
            return f(self);
        };

        let mut scope = HashMap::<String, TypeVarBinding>::new();
        for param in params {
            let name = self.interner.resolve(param.name.name).to_string();
            let canonical_name = format!("T{}", self.next_type_var);
            self.next_type_var += 1;
            scope.insert(
                name,
                TypeVarBinding {
                    canonical_name,
                    constraint: "_".to_string(),
                    default: "_".to_string(),
                },
            );
        }

        self.type_var_scopes.push(scope);

        let mut updates = Vec::new();
        for param in params {
            let param_name = self.interner.resolve(param.name.name).to_string();
            let constraint = param
                .constraint
                .as_ref()
                .map(|constraint| self.canonical_type_annotation(constraint))
                .unwrap_or_else(|| "_".to_string());
            let default = param
                .default
                .as_ref()
                .map(|default| self.canonical_type_annotation(default))
                .unwrap_or_else(|| "_".to_string());
            updates.push((param_name, constraint, default));
        }

        if let Some(active) = self.type_var_scopes.last_mut() {
            for (param_name, constraint, default) in updates {
                if let Some(binding) = active.get_mut(&param_name) {
                    binding.constraint = constraint;
                    binding.default = default;
                }
            }
        }

        let rendered = f(self);
        self.type_var_scopes.pop();
        rendered
    }

    fn lookup_type_var(&self, name: &str) -> Option<&TypeVarBinding> {
        for scope in self.type_var_scopes.iter().rev() {
            if let Some(binding) = scope.get(name) {
                return Some(binding);
            }
        }
        None
    }
}

fn canonical_primitive(primitive: PrimitiveType) -> &'static str {
    match primitive {
        PrimitiveType::Number => "number",
        PrimitiveType::Int => "int",
        PrimitiveType::String => "string",
        PrimitiveType::Boolean => "boolean",
        PrimitiveType::Null => "null",
        PrimitiveType::Void => "void",
    }
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace(':', "\\:")
        .replace(',', "\\,")
        .replace('|', "\\|")
}

/// Compute canonical runtime module identity path for a declaration file.
pub fn declaration_runtime_identity_path(declaration_path: &Path) -> Option<PathBuf> {
    let file_name = declaration_path.file_name()?.to_string_lossy();
    let replacement = if file_name.ends_with(".d.ts") {
        let stem = file_name.trim_end_matches(".d.ts");
        format!("{}.raya", stem)
    } else {
        return None;
    };

    Some(declaration_path.with_file_name(replacement))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn normalizes_export_declare_lines_in_dts() {
        let src = "export declare function add(a: number): number;\ndeclare const x: number;\n";
        let out = normalize_declaration_source(DeclarationSourceKind::DTs, src);
        assert!(out.contains("export function add"));
        assert!(out.contains("const x: number = null;"));
    }

    #[test]
    fn declaration_identity_path_rewrites_suffixes() {
        let b = PathBuf::from("/tmp/mod.d.ts");
        assert_eq!(
            declaration_runtime_identity_path(&b).unwrap(),
            PathBuf::from("/tmp/mod.raya")
        );
    }

    #[test]
    fn reports_unsupported_dts_syntax_with_line() {
        let src = "export enum Mode { A, B }\n";
        let err = detect_unsupported_dts_syntax(Path::new("mod.d.ts"), src).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("line 1"), "unexpected: {msg}");
        assert!(msg.contains("export enum"), "unexpected: {msg}");
    }

    #[test]
    fn equivalent_d_ts_inputs_produce_same_structural_signatures() {
        let temp = TempDir::new().expect("temp dir");
        let dts_a = temp.path().join("dep_a.d.ts");
        let dts_b = temp.path().join("dep_b.d.ts");

        std::fs::write(
            &dts_a,
            r#"
            export declare function foo(a: number, b: string): number;
            export declare class Box {
                value: number;
                get(): number;
            }
            "#,
        )
        .expect("write first d.ts");

        std::fs::write(
            &dts_b,
            r#"
            export declare function foo(a: number, b: string): number;
            export declare class Box {
                value: number;
                get(): number;
            }
            "#,
        )
        .expect("write d.ts");

        let module_identity = temp.path().join("dep.raya").to_string_lossy().to_string();
        let virtual_path = temp.path().join("__virtual_dep__.raya");

        let from_dts_a = load_declaration_module(&dts_a, &module_identity, &virtual_path)
            .expect("load first d.ts declaration");
        let from_dts_b = load_declaration_module(&dts_b, &module_identity, &virtual_path)
            .expect("load second d.ts declaration");

        for symbol in ["foo", "Box"] {
            let left = from_dts_a
                .exports
                .symbols
                .get(symbol)
                .unwrap_or_else(|| panic!("missing symbol {symbol} in first d.ts"));
            let right = from_dts_b
                .exports
                .symbols
                .get(symbol)
                .unwrap_or_else(|| panic!("missing symbol {symbol} in d.ts"));
            assert_eq!(left.signature_hash, right.signature_hash);
            assert_eq!(left.type_signature, right.type_signature);
        }
    }

    #[test]
    fn declaration_class_member_order_does_not_change_signature_hash() {
        let temp = TempDir::new().expect("temp dir");
        let a = temp.path().join("a.d.ts");
        let b = temp.path().join("b.d.ts");

        std::fs::write(
            &a,
            r#"
            export class Pair {
                first: number;
                second: string;
                describe(): string;
            }
            "#,
        )
        .expect("write a");

        std::fs::write(
            &b,
            r#"
            export class Pair {
                describe(): string;
                second: string;
                first: number;
            }
            "#,
        )
        .expect("write b");

        let module_identity = temp.path().join("pair.raya").to_string_lossy().to_string();
        let left =
            load_declaration_module(&a, &module_identity, &temp.path().join("a_virtual.raya"))
                .expect("load a");
        let right =
            load_declaration_module(&b, &module_identity, &temp.path().join("b_virtual.raya"))
                .expect("load b");

        let left_sig = &left
            .exports
            .symbols
            .get("Pair")
            .expect("left pair")
            .type_signature;
        let right_sig = &right
            .exports
            .symbols
            .get("Pair")
            .expect("right pair")
            .type_signature;
        assert_eq!(left_sig, right_sig);
    }

    #[test]
    fn declaration_interface_optional_method_affects_signature() {
        let temp = TempDir::new().expect("temp dir");
        let a = temp.path().join("a.d.ts");
        let b = temp.path().join("b.d.ts");

        std::fs::write(
            &a,
            r#"
            export interface Handler {
                run?(): number;
            }
            export const h: Handler;
            "#,
        )
        .expect("write a");

        std::fs::write(
            &b,
            r#"
            export interface Handler {
                run(): number;
            }
            export const h: Handler;
            "#,
        )
        .expect("write b");

        let module_identity = temp
            .path()
            .join("handler.raya")
            .to_string_lossy()
            .to_string();
        let left =
            load_declaration_module(&a, &module_identity, &temp.path().join("a_virtual.raya"))
                .expect("load a");
        let right =
            load_declaration_module(&b, &module_identity, &temp.path().join("b_virtual.raya"))
                .expect("load b");

        let left_sig = &left
            .exports
            .symbols
            .get("h")
            .expect("left h")
            .type_signature;
        let right_sig = &right
            .exports
            .symbols
            .get("h")
            .expect("right h")
            .type_signature;
        assert_ne!(left_sig, right_sig);
    }

    #[test]
    fn declaration_interface_members_do_not_get_function_bodies() {
        let src = r#"
            export interface Handler {
                run?(): number;
            }
            export declare class Box {
                get(): number;
            }
        "#;
        let normalized = normalize_declaration_source(DeclarationSourceKind::DTs, src);
        assert!(normalized.contains("run?(): number;"));
        assert!(
            normalized.contains("get(): number { throw new Error(\"__raya_decl_stub__\"); }"),
            "class methods should still be stubbed"
        );
    }

    #[test]
    fn declaration_class_function_typed_fields_do_not_get_stub_bodies() {
        let src = r#"
            export class Object {
                get: (() => Object) | null;
                set: ((value: Object) => void) | null;
                toString(): string;
            }
        "#;
        let normalized = normalize_declaration_source(DeclarationSourceKind::DTs, src);

        assert!(
            normalized.contains("get: (() => Object) | null;"),
            "function-typed field 'get' must remain a field declaration"
        );
        assert!(
            normalized.contains("set: ((value: Object) => void) | null;"),
            "function-typed field 'set' must remain a field declaration"
        );
        assert!(
            normalized.contains("toString(): string { throw new Error(\"__raya_decl_stub__\"); }"),
            "methods should still be stubbed"
        );
    }

    #[test]
    fn declaration_interface_call_and_construct_signatures_are_canonicalized() {
        let temp = TempDir::new().expect("temp dir");
        let a = temp.path().join("a.d.ts");
        let b = temp.path().join("b.d.ts");

        std::fs::write(
            &a,
            r#"
            export interface Factory {
                name: string;
                (value: number): string;
                new (value: number): { value: number };
            }
            export const f: Factory;
            "#,
        )
        .expect("write a");

        std::fs::write(
            &b,
            r#"
            export interface Factory {
                new (value: number): { value: number };
                (value: number): string;
                name: string;
            }
            export const f: Factory;
            "#,
        )
        .expect("write b");

        let module_identity = temp
            .path()
            .join("factory.raya")
            .to_string_lossy()
            .to_string();
        let left =
            load_declaration_module(&a, &module_identity, &temp.path().join("a_virtual.raya"))
                .expect("load a");
        let right =
            load_declaration_module(&b, &module_identity, &temp.path().join("b_virtual.raya"))
                .expect("load b");

        let left_sig = &left
            .exports
            .symbols
            .get("f")
            .expect("left f")
            .type_signature;
        let right_sig = &right
            .exports
            .symbols
            .get("f")
            .expect("right f")
            .type_signature;
        assert_eq!(left_sig, right_sig);
    }

    #[test]
    fn derives_specialization_template_from_monomorphized_symbol() {
        assert_eq!(
            specialization_template_from_symbol("identity__mono_abcd1234"),
            Some("identity".to_string())
        );
        assert_eq!(specialization_template_from_symbol("identity"), None);
        assert_eq!(specialization_template_from_symbol("__mono_abcd"), None);
    }
}
