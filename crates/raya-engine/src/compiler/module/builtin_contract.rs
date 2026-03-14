use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::compiler::{module_id_from_name, symbol_id_from_name, SymbolScope};
use crate::parser::ast::{ExportDecl, Pattern, Statement};
use crate::parser::checker::{Binder, CheckerPolicy, ScopeId, Symbol, SymbolFlags, SymbolKind};
use crate::parser::types::{TypeContext, TypeId};
use crate::parser::{Interner, Parser, Span};

use super::declaration::{BuiltinSurfaceMode, DeclarationError};
use super::exports::{ExportedSymbol, ModuleExports};

static STRICT_BUILTIN_CONTRACT: OnceLock<Result<ModuleExports, String>> = OnceLock::new();
static NODE_COMPAT_BUILTIN_CONTRACT: OnceLock<Result<ModuleExports, String>> = OnceLock::new();

struct ParsedBuiltinUnit {
    logical_path: &'static str,
    ast: crate::parser::ast::Module,
    interner: Interner,
    local_names: Vec<String>,
    export_names: Vec<String>,
}

pub fn builtin_global_exports_from_source(
    mode: BuiltinSurfaceMode,
) -> Result<ModuleExports, DeclarationError> {
    let cache = match mode {
        BuiltinSurfaceMode::RayaStrict => &STRICT_BUILTIN_CONTRACT,
        BuiltinSurfaceMode::NodeCompat => &NODE_COMPAT_BUILTIN_CONTRACT,
    };

    let cached = cache.get_or_init(|| build_builtin_global_exports(mode).map_err(|err| err.to_string()));
    cached.as_ref().cloned().map_err(|message| DeclarationError::InvalidDeclaration {
        path: PathBuf::from(match mode {
            BuiltinSurfaceMode::RayaStrict => "__raya_builtin__/strict.raya",
            BuiltinSurfaceMode::NodeCompat => "__raya_builtin__/node_compat.raya",
        }),
        line: 1,
        column: 1,
        message: message.clone(),
    })
}

fn build_builtin_global_exports(mode: BuiltinSurfaceMode) -> Result<ModuleExports, DeclarationError> {
    let mut parsed_units = Vec::new();
    for (logical_path, source) in builtin_source_modules_for_mode(mode) {
        let mut local_names = top_level_runtime_names(source, logical_path)?;
        local_names.sort();
        local_names.dedup();

        let mut export_names = explicit_runtime_export_names(source, logical_path)?;
        export_names.sort();
        export_names.dedup();

        let parser = Parser::new(source).map_err(|errors| DeclarationError::LexError {
            path: PathBuf::from(format!("__raya_builtin__/{}", logical_path)),
            message: errors
                .iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        })?;
        let (ast, interner) = parser.parse().map_err(|errors| DeclarationError::ParseError {
            path: PathBuf::from(format!("__raya_builtin__/{}", logical_path)),
            message: errors
                .iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        })?;

        parsed_units.push(ParsedBuiltinUnit {
            logical_path,
            ast,
            interner,
            local_names,
            export_names,
        });
    }

    let mut provisional_symbols = HashMap::new();
    for unit in &parsed_units {
        for local_name in &unit.local_names {
            if let Some(kind) = infer_runtime_export_symbol_type(&unit.ast, &unit.interner, local_name)
            {
                provisional_symbols.entry(local_name.clone()).or_insert(kind);
            }
        }
    }
    for name in [
        ("Reflect", crate::compiler::SymbolType::Constant),
        ("Object", crate::compiler::SymbolType::Constant),
        ("Symbol", crate::compiler::SymbolType::Constant),
        ("Boolean", crate::compiler::SymbolType::Constant),
        ("Number", crate::compiler::SymbolType::Constant),
        ("String", crate::compiler::SymbolType::Constant),
        ("Array", crate::compiler::SymbolType::Constant),
        ("Error", crate::compiler::SymbolType::Constant),
        ("AggregateError", crate::compiler::SymbolType::Constant),
        ("TypeError", crate::compiler::SymbolType::Constant),
        ("Function", crate::compiler::SymbolType::Constant),
        ("Promise", crate::compiler::SymbolType::Constant),
        ("Math", crate::compiler::SymbolType::Constant),
        ("JSON", crate::compiler::SymbolType::Constant),
        ("EventEmitter", crate::compiler::SymbolType::Class),
    ] {
        provisional_symbols
            .entry(name.0.to_string())
            .or_insert(name.1);
    }

    let checker_mode = crate::parser::checker::TypeSystemMode::Js;
    let checker_policy = CheckerPolicy::for_mode(checker_mode);
    let mut shared_type_ctx = TypeContext::new();

    for unit in &parsed_units {
        let mut binder = Binder::new(&mut shared_type_ctx, &unit.interner)
            .with_mode(checker_mode)
            .with_policy(checker_policy);
        binder.register_builtins(&[]);
        let mut ambient_symbols = provisional_symbols.clone();
        for local_name in &unit.local_names {
            ambient_symbols.remove(local_name);
        }
        seed_provisional_builtin_contract_symbols(&mut binder, &ambient_symbols);
        binder
            .bind_module(&unit.ast)
            .map_err(|errors| DeclarationError::InvalidDeclaration {
                path: PathBuf::from(format!("__raya_builtin__/{}", unit.logical_path)),
                line: 1,
                column: 1,
                message: errors
                    .iter()
                    .map(|error| error.to_string())
                    .collect::<Vec<_>>()
                    .join("; "),
            })?;
    }

    let mode_name = match mode {
        BuiltinSurfaceMode::RayaStrict => "strict",
        BuiltinSurfaceMode::NodeCompat => "node_compat",
    };
    let merged_module_identity = format!("__raya_builtin__/{}", mode_name);
    let merged_module_id = module_id_from_name(&merged_module_identity);
    let mut merged = ModuleExports::new(
        PathBuf::from(format!("__raya_builtin__/{}.raya", mode_name)),
        merged_module_identity.clone(),
    );

    for unit in &parsed_units {
        let mut contract_type_ctx = shared_type_ctx.clone();
        let mut binder = Binder::new(&mut contract_type_ctx, &unit.interner)
            .with_mode(checker_mode)
            .with_policy(checker_policy);
        binder.register_builtins(&[]);
        let mut ambient_symbols = provisional_symbols.clone();
        for local_name in &unit.local_names {
            ambient_symbols.remove(local_name);
        }
        seed_provisional_builtin_contract_symbols(&mut binder, &ambient_symbols);
        let symbols = binder
            .bind_module(&unit.ast)
            .map_err(|errors| DeclarationError::InvalidDeclaration {
                path: PathBuf::from(format!("__raya_builtin__/{}", unit.logical_path)),
                line: 1,
                column: 1,
                message: errors
                    .iter()
                    .map(|error| error.to_string())
                    .collect::<Vec<_>>()
                    .join("; "),
            })?;

        for export_name in &unit.export_names {
            let Some(symbol) = symbols.resolve(export_name) else {
                continue;
            };
            let Some(kind) = (match symbol.kind {
                SymbolKind::Function | SymbolKind::Class | SymbolKind::Variable | SymbolKind::EnumMember => {
                    Some(symbol.kind)
                }
                _ => None,
            }) else {
                continue;
            };

            if let Some(existing) = merged.symbols.get(export_name) {
                let structural = crate::parser::types::canonical_type_signature(symbol.ty, &contract_type_ctx);
                if existing.kind == kind
                    && existing.type_signature == structural.canonical
                    && existing.is_const == symbol.flags.is_const
                    && existing.is_async == symbol.flags.is_async
                {
                    continue;
                }
                return Err(DeclarationError::InvalidDeclaration {
                    path: PathBuf::from(format!("__raya_builtin__/{}", unit.logical_path)),
                    line: 1,
                    column: 1,
                    message: format!(
                        "Duplicate builtin export '{}' has incompatible signatures",
                        export_name
                    ),
                });
            }

            let symbol_id =
                symbol_id_from_name(&merged_module_identity, SymbolScope::Module, export_name);
            let structural = crate::parser::types::canonical_type_signature(symbol.ty, &contract_type_ctx);
            merged.add_symbol(ExportedSymbol {
                name: export_name.clone(),
                local_name: export_name.clone(),
                kind,
                ty: TypeId::new(TypeContext::UNKNOWN_TYPE_ID),
                is_const: symbol.flags.is_const,
                is_async: symbol.flags.is_async,
                module_name: merged_module_identity.clone(),
                module_id: merged_module_id,
                symbol_id,
                signature_hash: structural.hash,
                type_signature: structural.canonical,
                scope: SymbolScope::Module,
            });
        }
    }

    Ok(merged)
}

fn collect_pattern_names(pattern: &Pattern, interner: &Interner, out: &mut Vec<String>) {
    match pattern {
        Pattern::Identifier(identifier) => out.push(interner.resolve(identifier.name).to_string()),
        Pattern::Array(array) => {
            for element in array.elements.iter().flatten() {
                collect_pattern_names(&element.pattern, interner, out);
            }
            if let Some(rest) = &array.rest {
                collect_pattern_names(rest, interner, out);
            }
        }
        Pattern::Object(object) => {
            for property in &object.properties {
                collect_pattern_names(&property.value, interner, out);
            }
            if let Some(rest) = &object.rest {
                out.push(interner.resolve(rest.name).to_string());
            }
        }
        Pattern::Rest(rest) => collect_pattern_names(&rest.argument, interner, out),
    }
}

fn explicit_runtime_export_names(
    source: &str,
    logical_path: &str,
) -> Result<Vec<String>, DeclarationError> {
    let parser = Parser::new(source).map_err(|errors| DeclarationError::LexError {
        path: PathBuf::from(format!("__raya_builtin__/{}", logical_path)),
        message: errors
            .iter()
            .map(|error| error.to_string())
            .collect::<Vec<_>>()
            .join("; "),
    })?;
    let (ast, interner) = parser.parse().map_err(|errors| DeclarationError::ParseError {
        path: PathBuf::from(format!("__raya_builtin__/{}", logical_path)),
        message: errors
            .iter()
            .map(|error| error.to_string())
            .collect::<Vec<_>>()
            .join("; "),
    })?;
    let mut names = Vec::new();
    for stmt in &ast.statements {
        match stmt {
            Statement::ExportDecl(ExportDecl::Declaration(inner)) => match inner.as_ref() {
                Statement::ClassDecl(class_decl) => {
                    names.push(interner.resolve(class_decl.name.name).to_string());
                }
                Statement::FunctionDecl(func_decl) => {
                    names.push(interner.resolve(func_decl.name.name).to_string());
                }
                Statement::VariableDecl(var_decl) => {
                    collect_pattern_names(&var_decl.pattern, &interner, &mut names);
                }
                _ => {}
            },
            Statement::ExportDecl(ExportDecl::Named {
                specifiers,
                source: None,
                ..
            }) => {
                for specifier in specifiers {
                    names.push(
                        specifier
                            .alias
                            .as_ref()
                            .map(|alias| interner.resolve(alias.name).to_string())
                            .unwrap_or_else(|| interner.resolve(specifier.name.name).to_string()),
                    );
                }
            }
            Statement::ExportDecl(ExportDecl::Default { .. }) => names.push("default".to_string()),
            _ => {}
        }
    }
    names.retain(|name| !name.is_empty());
    Ok(names)
}

fn top_level_runtime_names(source: &str, logical_path: &str) -> Result<Vec<String>, DeclarationError> {
    let parser = Parser::new(source).map_err(|errors| DeclarationError::LexError {
        path: PathBuf::from(format!("__raya_builtin__/{}", logical_path)),
        message: errors
            .iter()
            .map(|error| error.to_string())
            .collect::<Vec<_>>()
            .join("; "),
    })?;
    let (ast, interner) = parser.parse().map_err(|errors| DeclarationError::ParseError {
        path: PathBuf::from(format!("__raya_builtin__/{}", logical_path)),
        message: errors
            .iter()
            .map(|error| error.to_string())
            .collect::<Vec<_>>()
            .join("; "),
    })?;
    let mut names = Vec::new();
    for stmt in &ast.statements {
        match stmt {
            Statement::ClassDecl(class_decl) => {
                names.push(interner.resolve(class_decl.name.name).to_string());
            }
            Statement::FunctionDecl(func_decl) => {
                names.push(interner.resolve(func_decl.name.name).to_string());
            }
            Statement::VariableDecl(var_decl) => {
                collect_pattern_names(&var_decl.pattern, &interner, &mut names);
            }
            _ => {}
        }
    }
    names.retain(|name| !name.is_empty());
    Ok(names)
}

fn infer_runtime_export_symbol_type(
    ast: &crate::parser::ast::Module,
    interner: &Interner,
    export_name: &str,
) -> Option<crate::compiler::SymbolType> {
    for stmt in &ast.statements {
        match stmt {
            Statement::ClassDecl(class_decl) if interner.resolve(class_decl.name.name) == export_name => {
                return Some(crate::compiler::SymbolType::Class);
            }
            Statement::FunctionDecl(func_decl)
                if interner.resolve(func_decl.name.name) == export_name =>
            {
                return Some(crate::compiler::SymbolType::Function);
            }
            Statement::VariableDecl(var_decl) => {
                let mut names = Vec::new();
                collect_pattern_names(&var_decl.pattern, interner, &mut names);
                if names.iter().any(|name| name == export_name) {
                    return Some(crate::compiler::SymbolType::Constant);
                }
            }
            _ => {}
        }
    }
    None
}

fn seed_provisional_builtin_contract_symbols(
    binder: &mut Binder<'_>,
    symbols: &HashMap<String, crate::compiler::SymbolType>,
) {
    let any_ty = binder.any_type_id();
    for (name, symbol_type) in symbols {
        match symbol_type {
            crate::compiler::SymbolType::Class => binder.register_external_class(name),
            crate::compiler::SymbolType::Function | crate::compiler::SymbolType::Constant => {
                let kind = match symbol_type {
                    crate::compiler::SymbolType::Function => SymbolKind::Function,
                    crate::compiler::SymbolType::Constant => SymbolKind::Variable,
                    crate::compiler::SymbolType::Class => unreachable!(),
                };
                let _ = binder.define_imported(Symbol {
                    name: name.clone(),
                    kind,
                    ty: any_ty,
                    flags: SymbolFlags {
                        is_exported: false,
                        is_const: true,
                        is_async: false,
                        is_readonly: true,
                        is_imported: false,
                    },
                    scope_id: ScopeId(0),
                    span: Span::new(0, 0, 0, 0),
                    referenced: false,
                });
            }
        }
    }
}

pub fn builtin_source_modules_for_mode(
    mode: BuiltinSurfaceMode,
) -> &'static [(&'static str, &'static str)] {
    match mode {
        BuiltinSurfaceMode::RayaStrict => strict_builtin_source_modules(),
        BuiltinSurfaceMode::NodeCompat => node_compat_builtin_source_modules(),
    }
}

fn strict_builtin_source_modules() -> &'static [(&'static str, &'static str)] {
    &[
        ("strict/object.raya", include_str!("../../../builtins/strict/object.raya")),
        ("strict/symbol.raya", include_str!("../../../builtins/strict/symbol.raya")),
        (
            "strict/globals.shared.raya",
            include_str!("../../../builtins/strict/globals.shared.raya"),
        ),
        ("strict/error.raya", include_str!("../../../builtins/strict/error.raya")),
        ("strict/array.raya", include_str!("../../../builtins/strict/array.raya")),
        ("strict/regexp.raya", include_str!("../../../builtins/strict/regexp.raya")),
        ("strict/map.raya", include_str!("../../../builtins/strict/map.raya")),
        ("strict/set.raya", include_str!("../../../builtins/strict/set.raya")),
        ("strict/buffer.raya", include_str!("../../../builtins/strict/buffer.raya")),
        ("strict/date.raya", include_str!("../../../builtins/strict/date.raya")),
        ("strict/channel.raya", include_str!("../../../builtins/strict/channel.raya")),
        ("strict/mutex.raya", include_str!("../../../builtins/strict/mutex.raya")),
        ("strict/promise.raya", include_str!("../../../builtins/strict/promise.raya")),
        (
            "strict/event_emitter.raya",
            include_str!("../../../builtins/strict/event_emitter.raya"),
        ),
        (
            "strict/iterator.raya",
            include_str!("../../../builtins/strict/iterator.raya"),
        ),
        (
            "strict/temporal.raya",
            include_str!("../../../builtins/strict/temporal.raya"),
        ),
    ]
}

fn node_compat_builtin_source_modules() -> &'static [(&'static str, &'static str)] {
    &[
        ("node_compat/object.raya", include_str!("../../../builtins/node_compat/object.raya")),
        ("node_compat/symbol.raya", include_str!("../../../builtins/node_compat/symbol.raya")),
        (
            "node_compat/globals.shared.raya",
            include_str!("../../../builtins/node_compat/globals.shared.raya"),
        ),
        ("node_compat/error.raya", include_str!("../../../builtins/node_compat/error.raya")),
        (
            "node_compat/function_families.raya",
            include_str!("../../../builtins/node_compat/function_families.raya"),
        ),
        (
            "node_compat/globals.raya",
            include_str!("../../../builtins/node_compat/globals.raya"),
        ),
        ("strict/array.raya", include_str!("../../../builtins/strict/array.raya")),
        ("strict/regexp.raya", include_str!("../../../builtins/strict/regexp.raya")),
        ("node_compat/map.raya", include_str!("../../../builtins/node_compat/map.raya")),
        ("node_compat/set.raya", include_str!("../../../builtins/node_compat/set.raya")),
        ("node_compat/buffer.raya", include_str!("../../../builtins/node_compat/buffer.raya")),
        ("node_compat/date.raya", include_str!("../../../builtins/node_compat/date.raya")),
        (
            "node_compat/channel.raya",
            include_str!("../../../builtins/node_compat/channel.raya"),
        ),
        ("node_compat/mutex.raya", include_str!("../../../builtins/node_compat/mutex.raya")),
        (
            "node_compat/promise.raya",
            include_str!("../../../builtins/node_compat/promise.raya"),
        ),
        (
            "node_compat/event_emitter.raya",
            include_str!("../../../builtins/node_compat/event_emitter.raya"),
        ),
        (
            "node_compat/iterator.raya",
            include_str!("../../../builtins/node_compat/iterator.raya"),
        ),
        (
            "node_compat/temporal.raya",
            include_str!("../../../builtins/node_compat/temporal.raya"),
        ),
        (
            "node_compat/typedarray.raya",
            include_str!("../../../builtins/node_compat/typedarray.raya"),
        ),
        (
            "node_compat/atomics.raya",
            include_str!("../../../builtins/node_compat/atomics.raya"),
        ),
        (
            "node_compat/dataview.raya",
            include_str!("../../../builtins/node_compat/dataview.raya"),
        ),
        (
            "node_compat/disposal.raya",
            include_str!("../../../builtins/node_compat/disposal.raya"),
        ),
        ("node_compat/intl.raya", include_str!("../../../builtins/node_compat/intl.raya")),
        (
            "node_compat/weak_collections.raya",
            include_str!("../../../builtins/node_compat/weak_collections.raya"),
        ),
        (
            "node_compat/weak_refs.raya",
            include_str!("../../../builtins/node_compat/weak_refs.raya"),
        ),
    ]
}
