//! Module exports tracking
//!
//! Tracks exported symbols from compiled modules for cross-module type checking.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::compiler::bytecode::{Module as BytecodeModule, SymbolType as BytecodeSymbolType};
use crate::compiler::{
    module_id_from_name, symbol_id_from_name, ModuleId, SymbolId, SymbolScope, SymbolType,
    TypeSignatureHash,
};
use crate::parser::ast::{ExportDecl, Expression, Module as AstModule, Pattern, Statement};
use crate::parser::checker::{Binder, ScopeId, ScopeKind, Symbol, SymbolKind};
use crate::parser::types::{canonical_type_signature, TypeContext, TypeId};
use crate::parser::{Interner, Span};

/// An exported symbol from a module
#[derive(Debug, Clone)]
pub struct ExportedSymbol {
    /// The exported name (may differ from local name if aliased)
    pub name: String,
    /// The local name in the source module
    pub local_name: String,
    /// Symbol kind (Variable, Function, Class, etc.)
    pub kind: SymbolKind,
    /// Type of the exported symbol
    pub ty: TypeId,
    /// Whether this is a const binding
    pub is_const: bool,
    /// Whether this is an async function
    pub is_async: bool,
    /// Module identity string used for deterministic symbol IDs.
    pub module_name: String,
    /// Stable module ID.
    pub module_id: ModuleId,
    /// Stable symbol ID.
    pub symbol_id: SymbolId,
    /// Stable type symbol ID.
    pub signature_hash: TypeSignatureHash,
    /// Canonical structural signature string for deterministic diagnostics.
    pub type_signature: String,
    /// Export symbol scope class.
    pub scope: SymbolScope,
}

impl ExportedSymbol {
    fn symbol_type_for_kind(kind: SymbolKind) -> SymbolType {
        match kind {
            SymbolKind::Function => SymbolType::Function,
            SymbolKind::Class | SymbolKind::Interface => SymbolType::Class,
            SymbolKind::Variable
            | SymbolKind::TypeAlias
            | SymbolKind::TypeParameter
            | SymbolKind::EnumMember => SymbolType::Constant,
        }
    }

    /// Create an ExportedSymbol from a Symbol
    pub fn from_symbol(
        symbol: &Symbol,
        module_name: &str,
        scope: SymbolScope,
        type_ctx: &TypeContext,
    ) -> Self {
        let module_id = module_id_from_name(module_name);
        let symbol_id = symbol_id_from_name(module_name, scope, &symbol.name);
        let structural_sig = canonical_type_signature(symbol.ty, type_ctx);
        let symbol_type = Self::symbol_type_for_kind(symbol.kind);
        Self {
            name: symbol.name.clone(),
            local_name: symbol.name.clone(),
            kind: symbol.kind,
            ty: symbol.ty,
            is_const: symbol.flags.is_const,
            is_async: symbol.flags.is_async,
            module_name: module_name.to_string(),
            module_id,
            symbol_id,
            signature_hash: structural_sig.hash,
            type_signature: structural_sig.canonical,
            scope,
        }
    }

    /// Create with an alias
    pub fn with_alias(
        symbol: &Symbol,
        alias: String,
        module_name: &str,
        scope: SymbolScope,
        type_ctx: &TypeContext,
    ) -> Self {
        let module_id = module_id_from_name(module_name);
        let symbol_id = symbol_id_from_name(module_name, scope, &alias);
        let structural_sig = canonical_type_signature(symbol.ty, type_ctx);
        let symbol_type = Self::symbol_type_for_kind(symbol.kind);
        Self {
            name: alias,
            local_name: symbol.name.clone(),
            kind: symbol.kind,
            ty: symbol.ty,
            is_const: symbol.flags.is_const,
            is_async: symbol.flags.is_async,
            module_name: module_name.to_string(),
            module_id,
            symbol_id,
            signature_hash: structural_sig.hash,
            type_signature: structural_sig.canonical,
            scope,
        }
    }

    /// Convert to a Symbol for import into another module
    pub fn to_import_symbol(&self, scope_id: crate::parser::checker::ScopeId) -> Symbol {
        Symbol {
            name: self.name.clone(),
            kind: self.kind,
            ty: self.ty,
            flags: crate::parser::checker::SymbolFlags {
                is_exported: false, // Not exported from the importing module
                is_const: self.is_const,
                is_async: self.is_async,
                is_readonly: false,
                is_imported: false,
            },
            scope_id,
            span: Span::new(0, 0, 0, 0), // Imported symbols don't have a local span
            referenced: false,
        }
    }
}

/// Exports from a single module
#[derive(Debug, Clone, Default)]
pub struct ModuleExports {
    /// Path to the module
    pub path: PathBuf,
    /// Canonical module identity string for symbol ID derivation.
    pub module_name: String,
    /// Exported symbols by name
    pub symbols: HashMap<String, ExportedSymbol>,
    /// Re-exported modules (export * from "./other")
    pub reexports: Vec<PathBuf>,
}

impl ModuleExports {
    /// Create new empty module exports
    pub fn new(path: PathBuf, module_name: String) -> Self {
        Self {
            path,
            module_name,
            symbols: HashMap::new(),
            reexports: Vec::new(),
        }
    }

    /// Add an exported symbol
    pub fn add_symbol(&mut self, symbol: ExportedSymbol) {
        self.symbols.insert(symbol.name.clone(), symbol);
    }

    /// Get an exported symbol by name
    pub fn get(&self, name: &str) -> Option<&ExportedSymbol> {
        self.symbols.get(name)
    }

    /// Check if a symbol is exported
    pub fn has(&self, name: &str) -> bool {
        self.symbols.contains_key(name)
    }

    /// Get all exported symbol names
    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.symbols.keys()
    }

    /// Add a re-export (export * from "./other")
    pub fn add_reexport(&mut self, path: PathBuf) {
        self.reexports.push(path);
    }
}

pub fn module_exports_from_bytecode(path: &std::path::Path, module: &BytecodeModule) -> ModuleExports {
    let module_name = module.metadata.name.clone();
    let module_id = module_id_from_name(&module_name);
    let mut exports = ModuleExports::new(path.to_path_buf(), module_name.clone());

    for export in &module.exports {
        let kind = match export.symbol_type {
            BytecodeSymbolType::Function => SymbolKind::Function,
            BytecodeSymbolType::Class => SymbolKind::Class,
            BytecodeSymbolType::Constant => SymbolKind::Variable,
        };
        let type_signature = export
            .type_signature
            .clone()
            .unwrap_or_else(|| "any".to_string());
        let signature_hash = if export.signature_hash == 0 {
            crate::parser::types::signature_hash(&type_signature)
        } else {
            export.signature_hash
        };

        exports.add_symbol(ExportedSymbol {
            name: export.name.clone(),
            local_name: export.name.clone(),
            kind,
            ty: TypeId::new(TypeContext::UNKNOWN_TYPE_ID),
            is_const: !matches!(kind, SymbolKind::Function),
            is_async: false,
            module_name: module_name.clone(),
            module_id,
            symbol_id: if export.symbol_id == 0 {
                symbol_id_from_name(&module_name, export.scope, &export.name)
            } else {
                export.symbol_id
            },
            signature_hash,
            type_signature,
            scope: export.scope,
        });
    }

    exports
}

fn scope_kind_to_symbol_scope(kind: ScopeKind) -> SymbolScope {
    match kind {
        ScopeKind::Global => SymbolScope::Global,
        ScopeKind::Module => SymbolScope::Module,
        ScopeKind::Function | ScopeKind::Block | ScopeKind::Class | ScopeKind::Loop => {
            SymbolScope::Local
        }
    }
}

fn top_level_module_scope_id(symbols: &crate::parser::checker::SymbolTable) -> Option<ScopeId> {
    for idx in 0..symbols.scope_count() {
        let scope_id = ScopeId(idx as u32);
        let scope = symbols.get_scope(scope_id);
        if scope.kind == ScopeKind::Module && scope.parent == Some(ScopeId(0)) {
            return Some(scope_id);
        }
    }
    None
}

fn resolve_exported_symbol<'a>(
    ast: &AstModule,
    interner: &Interner,
    symbols: &'a crate::parser::checker::SymbolTable,
    local_name: &str,
    export_offset: usize,
) -> Option<&'a Symbol> {
    if has_top_level_declaration_before_offset(ast, interner, local_name, export_offset) {
        if let Some(module_scope_id) = top_level_module_scope_id(symbols) {
            if let Some(symbol) = symbols.resolve_from_scope(local_name, module_scope_id) {
                if symbols.get_scope(symbol.scope_id).kind == ScopeKind::Module {
                    return Some(symbol);
                }
            }
        }
    }
    symbols.resolve(local_name)
}

fn collect_pattern_binding_names(pattern: &Pattern, interner: &Interner, out: &mut Vec<String>) {
    match pattern {
        Pattern::Identifier(ident) => out.push(interner.resolve(ident.name).to_string()),
        Pattern::Array(array) => {
            for element in &array.elements {
                if let Some(element) = element {
                    collect_pattern_binding_names(&element.pattern, interner, out);
                }
            }
            if let Some(rest) = &array.rest {
                collect_pattern_binding_names(rest, interner, out);
            }
        }
        Pattern::Object(object) => {
            for property in &object.properties {
                collect_pattern_binding_names(&property.value, interner, out);
            }
            if let Some(rest) = &object.rest {
                out.push(interner.resolve(rest.name).to_string());
            }
        }
        Pattern::Rest(rest) => {
            collect_pattern_binding_names(&rest.argument, interner, out);
        }
    }
}

fn top_level_declaration_stmt(stmt: &Statement) -> Option<&Statement> {
    match stmt {
        Statement::ExportDecl(ExportDecl::Declaration(inner)) => Some(inner.as_ref()),
        _ => Some(stmt),
    }
}

pub fn has_top_level_declaration_before_offset(
    ast: &AstModule,
    interner: &Interner,
    name: &str,
    offset: usize,
) -> bool {
    for stmt in &ast.statements {
        if stmt.span().start >= offset {
            continue;
        }
        let Some(stmt) = top_level_declaration_stmt(stmt) else {
            continue;
        };
        match stmt {
            Statement::FunctionDecl(function) => {
                if interner.resolve(function.name.name) == name {
                    return true;
                }
            }
            Statement::ClassDecl(class) => {
                if interner.resolve(class.name.name) == name {
                    return true;
                }
            }
            Statement::VariableDecl(variable) => {
                let mut names = Vec::new();
                collect_pattern_binding_names(&variable.pattern, interner, &mut names);
                if names.iter().any(|candidate| candidate == name) {
                    return true;
                }
            }
            _ => {}
        }
    }

    false
}

pub fn extract_module_exports(
    ast: &AstModule,
    path: &std::path::Path,
    module_name: &str,
    symbols: &crate::parser::checker::SymbolTable,
    interner: &Interner,
    type_ctx: &TypeContext,
) -> ModuleExports {
    let mut exports = ModuleExports::new(path.to_path_buf(), module_name.to_string());

    for symbol in symbols.get_exported_symbols() {
        let scope_kind = symbols.get_scope(symbol.scope_id).kind;
        let scope = scope_kind_to_symbol_scope(scope_kind);
        exports.add_symbol(ExportedSymbol::from_symbol(
            symbol,
            module_name,
            scope,
            type_ctx,
        ));
    }

    for stmt in &ast.statements {
        match stmt {
            Statement::ExportDecl(ExportDecl::Named {
                specifiers,
                source: None,
                ..
            }) => {
                for specifier in specifiers {
                    let local_name = interner.resolve(specifier.name.name).to_string();
                    let exported_name = specifier
                        .alias
                        .as_ref()
                        .map(|ident| interner.resolve(ident.name).to_string())
                        .unwrap_or_else(|| local_name.clone());
                    if exports.has(&exported_name) {
                        continue;
                    }
                    let Some(symbol) = resolve_exported_symbol(
                        ast,
                        interner,
                        symbols,
                        &local_name,
                        stmt.span().start,
                    ) else {
                        continue;
                    };
                    let scope = scope_kind_to_symbol_scope(symbols.get_scope(symbol.scope_id).kind);
                    exports.add_symbol(ExportedSymbol::with_alias(
                        symbol,
                        exported_name,
                        module_name,
                        scope,
                        type_ctx,
                    ));
                }
            }
            Statement::ExportDecl(ExportDecl::Default { expression, .. }) => {
                let Expression::Identifier(identifier) = expression.as_ref() else {
                    continue;
                };
                let local_name = interner.resolve(identifier.name).to_string();
                let Some(symbol) =
                    resolve_exported_symbol(ast, interner, symbols, &local_name, stmt.span().start)
                else {
                    continue;
                };
                let scope = scope_kind_to_symbol_scope(symbols.get_scope(symbol.scope_id).kind);
                exports.add_symbol(ExportedSymbol::with_alias(
                    symbol,
                    "default".to_string(),
                    module_name,
                    scope,
                    type_ctx,
                ));
            }
            _ => {}
        }
    }

    exports
}

pub fn inject_ambient_exports(
    binder: &mut Binder<'_>,
    ast: &AstModule,
    interner: &Interner,
    exports: &ModuleExports,
) {
    let mut names = exports.symbols.keys().cloned().collect::<Vec<_>>();
    names.sort();

    for name in &names {
        if has_top_level_declaration_before_offset(ast, interner, name, usize::MAX) {
            continue;
        }
        let Some(exported) = exports.symbols.get(name) else {
            continue;
        };
        if !matches!(
            exported.kind,
            SymbolKind::Class | SymbolKind::Interface | SymbolKind::TypeAlias
        ) {
            continue;
        }
        let imported_ty = binder.hydrate_imported_signature_type(&exported.type_signature);
        binder.override_imported_named_type(name, imported_ty);
        let symbol = Symbol {
            name: name.clone(),
            kind: exported.kind,
            ty: imported_ty,
            flags: crate::parser::checker::SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: true,
                is_imported: false,
            },
            scope_id: ScopeId(0),
            span: Span::new(0, 0, 0, 0),
            referenced: false,
        };
        let _ = binder.define_imported(symbol);
    }

    for name in names {
        if has_top_level_declaration_before_offset(ast, interner, &name, usize::MAX) {
            continue;
        }
        let Some(exported) = exports.symbols.get(&name) else {
            continue;
        };
        if matches!(
            exported.kind,
            SymbolKind::Class | SymbolKind::Interface | SymbolKind::TypeAlias
        ) {
            continue;
        }
        let imported_ty = binder.hydrate_imported_signature_type(&exported.type_signature);
        let symbol = Symbol {
            name,
            kind: exported.kind,
            ty: imported_ty,
            flags: crate::parser::checker::SymbolFlags {
                is_exported: false,
                is_const: exported.is_const,
                is_async: exported.is_async,
                is_readonly: exported.is_const,
                is_imported: false,
            },
            scope_id: ScopeId(0),
            span: Span::new(0, 0, 0, 0),
            referenced: false,
        };
        let _ = binder.define_imported(symbol);
    }
}

/// Registry of module exports for cross-module type checking
#[derive(Debug, Default)]
pub struct ExportRegistry {
    /// Exports by module path
    modules: HashMap<PathBuf, ModuleExports>,
}

impl ExportRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Register exports from a module
    pub fn register(&mut self, exports: ModuleExports) {
        self.modules.insert(exports.path.clone(), exports);
    }

    /// Get exports for a module
    pub fn get(&self, path: &PathBuf) -> Option<&ModuleExports> {
        self.modules.get(path)
    }

    /// Resolve a symbol from a module
    ///
    /// This handles re-exports by following the chain.
    pub fn resolve_symbol(&self, module_path: &PathBuf, name: &str) -> Option<&ExportedSymbol> {
        let exports = self.modules.get(module_path)?;

        // First check direct exports
        if let Some(symbol) = exports.get(name) {
            return Some(symbol);
        }

        // Then check re-exports
        for reexport_path in &exports.reexports {
            if let Some(symbol) = self.resolve_symbol(reexport_path, name) {
                return Some(symbol);
            }
        }

        None
    }

    /// Get all modules in the registry
    pub fn modules(&self) -> impl Iterator<Item = &PathBuf> {
        self.modules.keys()
    }

    /// Check if a module is registered
    pub fn has_module(&self, path: &PathBuf) -> bool {
        self.modules.contains_key(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::checker::ScopeId;

    #[test]
    fn test_module_exports() {
        let mut exports = ModuleExports::new(PathBuf::from("/test.raya"), "/test.raya".to_string());

        let symbol = ExportedSymbol {
            name: "foo".to_string(),
            local_name: "foo".to_string(),
            kind: SymbolKind::Function,
            ty: TypeId(1),
            is_const: false,
            is_async: false,
            module_name: "/test.raya".to_string(),
            module_id: module_id_from_name("/test.raya"),
            symbol_id: symbol_id_from_name("/test.raya", SymbolScope::Module, "foo"),
            signature_hash: 101,
            type_signature: "fn(min=0,params=[],rest=_,ret=number)".to_string(),
            scope: SymbolScope::Module,
        };

        exports.add_symbol(symbol);

        assert!(exports.has("foo"));
        assert!(!exports.has("bar"));

        let exported = exports.get("foo").unwrap();
        assert_eq!(exported.name, "foo");
        assert_eq!(exported.kind, SymbolKind::Function);
    }

    #[test]
    fn test_export_registry() {
        let mut registry = ExportRegistry::new();

        let mut exports =
            ModuleExports::new(PathBuf::from("/utils.raya"), "/utils.raya".to_string());
        exports.add_symbol(ExportedSymbol {
            name: "helper".to_string(),
            local_name: "helper".to_string(),
            kind: SymbolKind::Function,
            ty: TypeId(2),
            is_const: false,
            is_async: false,
            module_name: "/utils.raya".to_string(),
            module_id: module_id_from_name("/utils.raya"),
            symbol_id: symbol_id_from_name("/utils.raya", SymbolScope::Module, "helper"),
            signature_hash: 102,
            type_signature: "fn(min=0,params=[],rest=_,ret=number)".to_string(),
            scope: SymbolScope::Module,
        });

        registry.register(exports);

        let path = PathBuf::from("/utils.raya");
        assert!(registry.has_module(&path));

        let symbol = registry.resolve_symbol(&path, "helper").unwrap();
        assert_eq!(symbol.name, "helper");
    }

    #[test]
    fn test_to_import_symbol() {
        let exported = ExportedSymbol {
            name: "myFunc".to_string(),
            local_name: "myFunc".to_string(),
            kind: SymbolKind::Function,
            ty: TypeId(5),
            is_const: false,
            is_async: true,
            module_name: "/m.raya".to_string(),
            module_id: module_id_from_name("/m.raya"),
            symbol_id: symbol_id_from_name("/m.raya", SymbolScope::Module, "myFunc"),
            signature_hash: 103,
            type_signature: "fn(min=0,params=[],rest=_,ret=number)".to_string(),
            scope: SymbolScope::Module,
        };

        let imported = exported.to_import_symbol(ScopeId(0));

        assert_eq!(imported.name, "myFunc");
        assert_eq!(imported.kind, SymbolKind::Function);
        assert_eq!(imported.ty, TypeId(5));
        assert!(imported.flags.is_async);
        assert!(!imported.flags.is_exported);
    }
}
