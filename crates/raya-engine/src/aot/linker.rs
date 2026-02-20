//! Cross-module linker
//!
//! Resolves imports between modules and assigns global function IDs.
//! When module A imports function `bar` from module B, the linker:
//! 1. Resolves `bar` in B's export table
//! 2. Assigns a GlobalFuncId to `bar`
//! 3. Records the mapping so generated code can use the correct ID

use std::collections::HashMap;

use super::codegen::GlobalFuncId;

/// The AOT linker resolves cross-module references.
#[derive(Debug)]
pub struct AotLinker {
    /// Map from (module_name, symbol_name) → GlobalFuncId
    symbol_table: HashMap<(String, String), GlobalFuncId>,

    /// Map from GlobalFuncId → (module_index, func_index, func_name)
    reverse_table: HashMap<GlobalFuncId, FuncInfo>,

    /// Next module index to assign.
    next_module_index: u16,

    /// Module names in order of index.
    module_names: Vec<String>,
}

/// Information about a linked function.
#[derive(Debug, Clone)]
pub struct FuncInfo {
    /// Module index.
    pub module_index: u16,

    /// Function index within the module.
    pub func_index: u16,

    /// Fully qualified name: "module_name::func_name"
    pub qualified_name: String,
}

/// Errors that can occur during linking.
#[derive(Debug)]
pub enum LinkerError {
    /// A module tried to import a symbol that no module exports.
    UnresolvedImport {
        importing_module: String,
        symbol: String,
    },

    /// Too many modules (> 65536).
    TooManyModules,

    /// Too many functions in a module (> 65536).
    TooManyFunctions {
        module: String,
        count: usize,
    },

    /// Duplicate module name.
    DuplicateModule(String),
}

impl std::fmt::Display for LinkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkerError::UnresolvedImport { importing_module, symbol } => {
                write!(f, "Unresolved import: {} imports '{}'", importing_module, symbol)
            }
            LinkerError::TooManyModules => write!(f, "Too many modules (max 65536)"),
            LinkerError::TooManyFunctions { module, count } => {
                write!(f, "Too many functions in module '{}': {} (max 65536)", module, count)
            }
            LinkerError::DuplicateModule(name) => {
                write!(f, "Duplicate module name: {}", name)
            }
        }
    }
}

impl std::error::Error for LinkerError {}

impl AotLinker {
    /// Create a new empty linker.
    pub fn new() -> Self {
        Self {
            symbol_table: HashMap::new(),
            reverse_table: HashMap::new(),
            next_module_index: 0,
            module_names: Vec::new(),
        }
    }

    /// Register a module and its exported symbols.
    ///
    /// Returns the module index assigned to this module.
    pub fn register_module(
        &mut self,
        module_name: &str,
        exports: &[(String, u16)], // (symbol_name, func_index)
    ) -> Result<u16, LinkerError> {
        if self.module_names.contains(&module_name.to_string()) {
            return Err(LinkerError::DuplicateModule(module_name.to_string()));
        }

        if self.next_module_index == u16::MAX {
            return Err(LinkerError::TooManyModules);
        }

        let module_index = self.next_module_index;
        self.next_module_index += 1;
        self.module_names.push(module_name.to_string());

        for (symbol_name, func_index) in exports {
            let global_id = GlobalFuncId::new(module_index, *func_index);
            self.symbol_table.insert(
                (module_name.to_string(), symbol_name.clone()),
                global_id,
            );
            self.reverse_table.insert(global_id, FuncInfo {
                module_index,
                func_index: *func_index,
                qualified_name: format!("{}::{}", module_name, symbol_name),
            });
        }

        Ok(module_index)
    }

    /// Resolve an imported symbol to its GlobalFuncId.
    pub fn resolve_import(
        &self,
        from_module: &str,
        target_module: &str,
        symbol_name: &str,
    ) -> Result<GlobalFuncId, LinkerError> {
        self.symbol_table
            .get(&(target_module.to_string(), symbol_name.to_string()))
            .copied()
            .ok_or_else(|| LinkerError::UnresolvedImport {
                importing_module: from_module.to_string(),
                symbol: format!("{}::{}", target_module, symbol_name),
            })
    }

    /// Look up function info by global ID.
    pub fn get_func_info(&self, id: GlobalFuncId) -> Option<&FuncInfo> {
        self.reverse_table.get(&id)
    }

    /// Get the total number of registered modules.
    pub fn module_count(&self) -> usize {
        self.module_names.len()
    }

    /// Get module name by index.
    pub fn module_name(&self, index: u16) -> Option<&str> {
        self.module_names.get(index as usize).map(|s| s.as_str())
    }

    /// Iterate over all registered symbols.
    pub fn symbols(&self) -> impl Iterator<Item = (&(String, String), &GlobalFuncId)> {
        self.symbol_table.iter()
    }
}

impl Default for AotLinker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_module() {
        let mut linker = AotLinker::new();

        let idx = linker.register_module("math", &[
            ("add".to_string(), 0),
            ("multiply".to_string(), 1),
        ]).unwrap();

        assert_eq!(idx, 0);
        assert_eq!(linker.module_count(), 1);
    }

    #[test]
    fn test_resolve_import() {
        let mut linker = AotLinker::new();

        linker.register_module("math", &[
            ("add".to_string(), 0),
            ("multiply".to_string(), 1),
        ]).unwrap();

        let id = linker.resolve_import("main", "math", "add").unwrap();
        assert_eq!(id.module_index(), 0);
        assert_eq!(id.func_index(), 0);

        let id = linker.resolve_import("main", "math", "multiply").unwrap();
        assert_eq!(id.module_index(), 0);
        assert_eq!(id.func_index(), 1);
    }

    #[test]
    fn test_unresolved_import() {
        let linker = AotLinker::new();
        let result = linker.resolve_import("main", "math", "divide");
        assert!(matches!(result, Err(LinkerError::UnresolvedImport { .. })));
    }

    #[test]
    fn test_duplicate_module() {
        let mut linker = AotLinker::new();
        linker.register_module("math", &[]).unwrap();
        let result = linker.register_module("math", &[]);
        assert!(matches!(result, Err(LinkerError::DuplicateModule(_))));
    }

    #[test]
    fn test_cross_module_linking() {
        let mut linker = AotLinker::new();

        // Register two modules
        linker.register_module("utils", &[
            ("format_string".to_string(), 0),
        ]).unwrap();

        linker.register_module("main", &[
            ("entry".to_string(), 0),
            ("helper".to_string(), 1),
        ]).unwrap();

        // Main imports format_string from utils
        let id = linker.resolve_import("main", "utils", "format_string").unwrap();
        assert_eq!(id.module_index(), 0); // utils is module 0
        assert_eq!(id.func_index(), 0);

        // Look up the function info
        let info = linker.get_func_info(id).unwrap();
        assert_eq!(info.qualified_name, "utils::format_string");
    }
}
