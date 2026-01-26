//! Module linking and symbol resolution
//!
//! Handles resolving imported symbols from module exports and linking modules together.

use raya_compiler::{Export, Import, Module, SymbolType};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

/// Errors that can occur during module linking
#[derive(Debug, Error)]
pub enum LinkError {
    /// Symbol not found in module exports
    #[error("Symbol '{symbol}' not found in module '{module}'")]
    SymbolNotFound { module: String, symbol: String },

    /// Module not found
    #[error("Module not found: {0}")]
    ModuleNotFound(String),

    /// Type mismatch (expected function, got class, etc.)
    #[error("Symbol '{symbol}' has wrong type: expected {expected:?}, got {actual:?}")]
    TypeMismatch {
        symbol: String,
        expected: SymbolType,
        actual: SymbolType,
    },

    /// Circular dependency during linking
    #[error("Circular dependency detected during linking: {0}")]
    CircularDependency(String),

    /// Export index out of bounds
    #[error("Export index {index} out of bounds for {symbol_type:?} (max: {max})")]
    IndexOutOfBounds {
        index: usize,
        symbol_type: SymbolType,
        max: usize,
    },
}

/// Resolved symbol reference
#[derive(Debug, Clone)]
pub struct ResolvedSymbol {
    /// Module containing the symbol
    pub module: Arc<Module>,
    /// Export definition
    pub export: Export,
    /// Actual index in the module's functions/classes/constants
    pub index: usize,
}

/// Module linker
///
/// Resolves imports to exports and validates symbol types.
pub struct ModuleLinker {
    /// Loaded modules by name
    modules: HashMap<String, Arc<Module>>,
}

impl ModuleLinker {
    /// Create a new module linker
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
        }
    }

    /// Add a module to the linker
    ///
    /// # Arguments
    /// * `module` - Module to add
    ///
    /// # Returns
    /// * `Ok(())` - Module added successfully
    /// * `Err(String)` - Module name conflict
    pub fn add_module(&mut self, module: Arc<Module>) -> Result<(), String> {
        let name = module.metadata.name.clone();
        if self.modules.contains_key(&name) {
            return Err(format!("Module '{}' already loaded", name));
        }
        self.modules.insert(name, module);
        Ok(())
    }

    /// Resolve an import to its exported symbol
    ///
    /// # Arguments
    /// * `import` - Import to resolve
    /// * `current_module` - Name of the module doing the importing (for error messages)
    ///
    /// # Returns
    /// * `Ok(ResolvedSymbol)` - Successfully resolved symbol
    /// * `Err(LinkError)` - Resolution failed
    ///
    /// # Example
    /// ```no_run
    /// # use raya_core::module::ModuleLinker;
    /// # use raya_compiler::Import;
    /// # let mut linker = ModuleLinker::new();
    /// # let import = Import {
    /// #     module_specifier: "logging".to_string(),
    /// #     symbol: "info".to_string(),
    /// #     alias: None,
    /// #     version_constraint: None,
    /// # };
    /// let resolved = linker.resolve_import(&import, "main").unwrap();
    /// ```
    pub fn resolve_import(
        &self,
        import: &Import,
        _current_module: &str,
    ) -> Result<ResolvedSymbol, LinkError> {
        // Extract module name from specifier
        // For now, use the full specifier as the module name
        // In Phase 6, we'll parse version constraints and resolve
        let module_name = Self::extract_module_name(&import.module_specifier);

        // Find the module
        let module = self
            .modules
            .get(&module_name)
            .ok_or_else(|| LinkError::ModuleNotFound(module_name.clone()))?;

        // Find the export
        let export = module
            .exports
            .iter()
            .find(|e| e.name == import.symbol)
            .ok_or_else(|| LinkError::SymbolNotFound {
                module: module_name.clone(),
                symbol: import.symbol.clone(),
            })?;

        // Validate index bounds
        let max_index = match export.symbol_type {
            SymbolType::Function => module.functions.len(),
            SymbolType::Class => module.classes.len(),
            SymbolType::Constant => {
                module.constants.strings.len()
                    + module.constants.integers.len()
                    + module.constants.floats.len()
            }
        };

        if export.index >= max_index {
            return Err(LinkError::IndexOutOfBounds {
                index: export.index,
                symbol_type: export.symbol_type.clone(),
                max: max_index,
            });
        }

        Ok(ResolvedSymbol {
            module: module.clone(),
            export: export.clone(),
            index: export.index,
        })
    }

    /// Link all imports for a module
    ///
    /// # Arguments
    /// * `module` - Module whose imports to resolve
    ///
    /// # Returns
    /// * `Ok(Vec<ResolvedSymbol>)` - All resolved imports (in order)
    /// * `Err(LinkError)` - Linking failed
    pub fn link_module(&self, module: &Module) -> Result<Vec<ResolvedSymbol>, LinkError> {
        let mut resolved = Vec::new();

        for import in &module.imports {
            let symbol = self.resolve_import(import, &module.metadata.name)?;
            resolved.push(symbol);
        }

        Ok(resolved)
    }

    /// Extract module name from specifier
    ///
    /// Handles:
    /// - "logging" → "logging"
    /// - "logging@1.2.3" → "logging"
    /// - "@org/package@^2.0.0" → "@org/package"
    fn extract_module_name(specifier: &str) -> String {
        // For scoped packages, find @ after the first character
        if specifier.starts_with('@') {
            if let Some(at_pos) = specifier[1..].find('@') {
                return specifier[..at_pos + 1].to_string();
            }
            return specifier.to_string();
        }

        // For regular packages, split at @
        if let Some(at_pos) = specifier.find('@') {
            return specifier[..at_pos].to_string();
        }

        specifier.to_string()
    }

    /// Get a module by name
    pub fn get_module(&self, name: &str) -> Option<&Arc<Module>> {
        self.modules.get(name)
    }

    /// Check if a module is loaded
    pub fn has_module(&self, name: &str) -> bool {
        self.modules.contains_key(name)
    }

    /// Get all loaded module names
    pub fn module_names(&self) -> Vec<String> {
        self.modules.keys().cloned().collect()
    }
}

impl Default for ModuleLinker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raya_compiler::{ConstantPool, Metadata};

    fn create_test_module(name: &str) -> Module {
        Module {
            magic: *b"RAYA",
            version: 1,
            flags: 0,
            constants: ConstantPool::new(),
            functions: vec![],
            classes: vec![],
            metadata: Metadata {
                name: name.to_string(),
                source_file: Some(format!("{}.raya", name)),
            },
            exports: vec![],
            imports: vec![],
            checksum: [0; 32],
        }
    }

    #[test]
    fn test_extract_module_name() {
        assert_eq!(ModuleLinker::extract_module_name("logging"), "logging");
        assert_eq!(
            ModuleLinker::extract_module_name("logging@1.2.3"),
            "logging"
        );
        assert_eq!(
            ModuleLinker::extract_module_name("@org/package@^2.0.0"),
            "@org/package"
        );
        assert_eq!(
            ModuleLinker::extract_module_name("@org/package"),
            "@org/package"
        );
    }

    #[test]
    fn test_add_module() {
        let mut linker = ModuleLinker::new();
        let module = Arc::new(create_test_module("test"));

        assert!(linker.add_module(module.clone()).is_ok());
        assert!(linker.has_module("test"));

        // Adding again should fail
        assert!(linker.add_module(module).is_err());
    }

    #[test]
    fn test_resolve_import_not_found() {
        let linker = ModuleLinker::new();
        let import = Import {
            module_specifier: "missing".to_string(),
            symbol: "foo".to_string(),
            alias: None,
            version_constraint: None,
        };

        let result = linker.resolve_import(&import, "main");
        assert!(matches!(result, Err(LinkError::ModuleNotFound(_))));
    }

    #[test]
    fn test_resolve_symbol_not_found() {
        let mut linker = ModuleLinker::new();
        let module = Arc::new(create_test_module("logging"));
        linker.add_module(module).unwrap();

        let import = Import {
            module_specifier: "logging".to_string(),
            symbol: "missing_function".to_string(),
            alias: None,
            version_constraint: None,
        };

        let result = linker.resolve_import(&import, "main");
        assert!(matches!(result, Err(LinkError::SymbolNotFound { .. })));
    }
}
