//! Declaration-module support for binary linking.
//!
//! This module parses declaration files (`.d.raya` and `.d.ts` subset), derives
//! canonical structural signatures, and builds export metadata for late-link
//! placeholder modules.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::compiler::{
    module_id_from_name, symbol_id_from_name, ModuleId, SymbolId, SymbolScope, SymbolType,
    TypeSymbolId,
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

/// Source format backing a declaration module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclarationSourceKind {
    DRaya,
    DTs,
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
    pub type_symbol_id: TypeSymbolId,
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
        } else if file_name.ends_with(".d.raya") {
            Some(Self::DRaya)
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
            message: "Declaration path must end with .d.raya or .d.ts".to_string(),
        }
    })?;

    let raw_source =
        fs::read_to_string(declaration_path).map_err(|e| DeclarationError::IoError {
            path: declaration_path.to_path_buf(),
            message: e.to_string(),
        })?;

    if source_kind == DeclarationSourceKind::DTs {
        detect_unsupported_dts_syntax(declaration_path, &raw_source)?;
    }

    let normalized_source = normalize_declaration_source(source_kind, &raw_source);

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

fn normalize_declaration_source(kind: DeclarationSourceKind, source: &str) -> String {
    let normalized = if kind == DeclarationSourceKind::DRaya {
        source.to_string()
    } else {
        let mut out = String::with_capacity(source.len());
        for line in source.lines() {
            let normalized = normalize_dts_line(line);
            out.push_str(&normalized);
            out.push('\n');
        }
        out
    };

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

fn materialize_declaration_stubs(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + 128);
    let mut class_depth = 0i32;

    for line in source.lines() {
        let mut normalized = line.to_string();
        let trimmed = normalized.trim();

        let is_signature_like =
            trimmed.ends_with(';') && trimmed.contains('(') && trimmed.contains(')');
        if is_signature_like {
            let is_top_level_fn = trimmed.starts_with("export function ")
                || trimmed.starts_with("function ")
                || trimmed.starts_with("export async function ")
                || trimmed.starts_with("async function ");
            let is_class_member = class_depth > 0
                && !trimmed.starts_with("export ")
                && !trimmed.starts_with("import ")
                && !trimmed.starts_with("type ")
                && !trimmed.starts_with("interface ")
                && !trimmed.starts_with("readonly ");
            let is_abstract = trimmed.starts_with("abstract ");

            if (is_top_level_fn || is_class_member) && !is_abstract {
                if let Some(index) = normalized.rfind(';') {
                    normalized.replace_range(
                        index..=index,
                        " { throw new Error(\"__raya_decl_stub__\"); }",
                    );
                }
            }
        }

        out.push_str(&normalized);
        out.push('\n');

        let open = line.chars().filter(|c| *c == '{').count() as i32;
        let close = line.chars().filter(|c| *c == '}').count() as i32;
        class_depth = (class_depth + open - close).max(0);
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
            type_symbol_id: signature_hash(&export.canonical_signature),
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
        DeclarationItem::TypeAlias(_) => None,
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
                        members.insert(format!(
                            "{}:{}:{}:{}:{}",
                            prefix,
                            escape(&this.interner.resolve(field.name.name).to_string()),
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
                        members.insert(format!(
                            "{}:{}:{}",
                            prefix,
                            escape(&this.interner.resolve(method.name.name).to_string()),
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
                            members.insert(format!(
                                "prop:{}:rw:req:{}",
                                escape(self.interner.resolve(method.name.name)),
                                fn_sig
                            ));
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
    let replacement = if file_name.ends_with(".d.raya") {
        let stem = file_name.trim_end_matches(".d.raya");
        format!("{}.raya", stem)
    } else if file_name.ends_with(".d.ts") {
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
        assert!(out.contains("const x: number;"));
    }

    #[test]
    fn declaration_identity_path_rewrites_suffixes() {
        let a = PathBuf::from("/tmp/mod.d.raya");
        let b = PathBuf::from("/tmp/mod.d.ts");
        assert_eq!(
            declaration_runtime_identity_path(&a).unwrap(),
            PathBuf::from("/tmp/mod.raya")
        );
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
    fn equivalent_d_raya_and_d_ts_produce_same_structural_signatures() {
        let temp = TempDir::new().expect("temp dir");
        let draya = temp.path().join("dep.d.raya");
        let dts = temp.path().join("dep.d.ts");

        std::fs::write(
            &draya,
            r#"
            export function foo(a: number, b: string): number;
            export class Box {
                value: number;
                get(): number;
            }
            "#,
        )
        .expect("write d.raya");

        std::fs::write(
            &dts,
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

        let from_draya = load_declaration_module(&draya, &module_identity, &virtual_path)
            .expect("load d.raya declaration");
        let from_dts = load_declaration_module(&dts, &module_identity, &virtual_path)
            .expect("load d.ts declaration");

        for symbol in ["foo", "Box"] {
            let left = from_draya
                .exports
                .symbols
                .get(symbol)
                .unwrap_or_else(|| panic!("missing symbol {symbol} in d.raya"));
            let right = from_dts
                .exports
                .symbols
                .get(symbol)
                .unwrap_or_else(|| panic!("missing symbol {symbol} in d.ts"));
            assert_eq!(left.type_symbol_id, right.type_symbol_id);
            assert_eq!(left.type_signature, right.type_signature);
        }
    }

    #[test]
    fn declaration_class_member_order_does_not_change_signature_hash() {
        let temp = TempDir::new().expect("temp dir");
        let a = temp.path().join("a.d.raya");
        let b = temp.path().join("b.d.raya");

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
    fn derives_specialization_template_from_monomorphized_symbol() {
        assert_eq!(
            specialization_template_from_symbol("identity__mono_abcd1234"),
            Some("identity".to_string())
        );
        assert_eq!(specialization_template_from_symbol("identity"), None);
        assert_eq!(specialization_template_from_symbol("__mono_abcd"), None);
    }
}
