//! Module exports tracking
//!
//! Tracks exported symbols from compiled modules for cross-module type checking.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::parser::checker::{Symbol, SymbolKind};
use crate::parser::types::TypeId;
use crate::parser::Span;

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
}

impl ExportedSymbol {
    /// Create an ExportedSymbol from a Symbol
    pub fn from_symbol(symbol: &Symbol) -> Self {
        Self {
            name: symbol.name.clone(),
            local_name: symbol.name.clone(),
            kind: symbol.kind,
            ty: symbol.ty,
            is_const: symbol.flags.is_const,
            is_async: symbol.flags.is_async,
        }
    }

    /// Create with an alias
    pub fn with_alias(symbol: &Symbol, alias: String) -> Self {
        Self {
            name: alias,
            local_name: symbol.name.clone(),
            kind: symbol.kind,
            ty: symbol.ty,
            is_const: symbol.flags.is_const,
            is_async: symbol.flags.is_async,
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
    /// Exported symbols by name
    pub symbols: HashMap<String, ExportedSymbol>,
    /// Re-exported modules (export * from "./other")
    pub reexports: Vec<PathBuf>,
}

impl ModuleExports {
    /// Create new empty module exports
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
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
        let mut exports = ModuleExports::new(PathBuf::from("/test.raya"));

        let symbol = ExportedSymbol {
            name: "foo".to_string(),
            local_name: "foo".to_string(),
            kind: SymbolKind::Function,
            ty: TypeId(1),
            is_const: false,
            is_async: false,
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

        let mut exports = ModuleExports::new(PathBuf::from("/utils.raya"));
        exports.add_symbol(ExportedSymbol {
            name: "helper".to_string(),
            local_name: "helper".to_string(),
            kind: SymbolKind::Function,
            ty: TypeId(2),
            is_const: false,
            is_async: false,
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
        };

        let imported = exported.to_import_symbol(ScopeId(0));

        assert_eq!(imported.name, "myFunc");
        assert_eq!(imported.kind, SymbolKind::Function);
        assert_eq!(imported.ty, TypeId(5));
        assert!(imported.flags.is_async);
        assert!(!imported.flags.is_exported);
    }
}
