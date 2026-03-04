//! Module linking and symbol resolution
//!
//! Handles resolving imported symbols from module exports and linking modules together.

use crate::compiler::{
    module_id_from_name, Export, Import, Module, ModuleId, SymbolId, SymbolScope, SymbolType,
    TypeSignatureHash,
};
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

    /// Module not found by symbol ID.
    #[error("Module ID not found: {0}")]
    ModuleIdNotFound(ModuleId),

    /// Type mismatch (expected function, got class, etc.)
    #[error("Symbol '{symbol}' has wrong type: expected {expected:?}, got {actual:?}")]
    TypeMismatch {
        symbol: String,
        expected: SymbolType,
        actual: SymbolType,
    },

    /// Structural type-signature mismatch across import/export contracts.
    #[error(
        "Type signature mismatch for '{symbol}': expected {expected_hash:#x} ({expected_pretty}), got {actual_hash:#x} ({actual_pretty})"
    )]
    TypeSignatureMismatch {
        symbol: String,
        expected_hash: TypeSignatureHash,
        actual_hash: TypeSignatureHash,
        expected_pretty: String,
        actual_pretty: String,
    },

    /// Missing structural type-signature hash on either import or export side.
    #[error(
        "Missing structural type signature for '{symbol}' (import hash={import_hash:#x}, export hash={export_hash:#x})"
    )]
    MissingTypeSignature {
        symbol: String,
        import_hash: TypeSignatureHash,
        export_hash: TypeSignatureHash,
    },

    /// Scope mismatch across import/export contracts.
    #[error("Scope mismatch for '{symbol}': expected {expected:?}, got {actual:?}")]
    ScopeMismatch {
        symbol: String,
        expected: SymbolScope,
        actual: SymbolScope,
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
    /// Loaded modules by stable module ID.
    modules_by_id: HashMap<ModuleId, Arc<Module>>,
    /// Export index by module ID and symbol ID.
    exports_by_symbol: HashMap<ModuleId, HashMap<SymbolId, usize>>,
}

impl ModuleLinker {
    /// Create a new module linker
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
            modules_by_id: HashMap::new(),
            exports_by_symbol: HashMap::new(),
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
        let module_id = module_id_from_name(&name);
        if self.modules.contains_key(&name) {
            return Err(format!("Module '{}' already loaded", name));
        }
        if self.modules_by_id.contains_key(&module_id) {
            return Err(format!("Module ID '{}' already loaded", module_id));
        }
        let mut by_symbol = HashMap::new();
        for (index, export) in module.exports.iter().enumerate() {
            if let Some(existing_index) = by_symbol.insert(export.symbol_id, index) {
                return Err(format!(
                    "Module '{}' has duplicate symbol id {} at export indexes {} and {}",
                    name, export.symbol_id, existing_index, index
                ));
            }
        }
        self.modules.insert(name, module.clone());
        self.modules_by_id.insert(module_id, module);
        self.exports_by_symbol.insert(module_id, by_symbol);
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
    /// ```ignore
    /// use raya_engine::vm::module::ModuleLinker;
    /// use raya_engine::compiler::Import;
    ///
    /// let mut linker = ModuleLinker::new();
    /// let import = Import {
    ///     module_specifier: "logging".to_string(),
    ///     symbol: "info".to_string(),
    ///     alias: None,
    /// };
    /// let resolved = linker.resolve_import(&import, "main").unwrap();
    /// ```
    pub fn resolve_import(
        &self,
        import: &Import,
        _current_module: &str,
    ) -> Result<ResolvedSymbol, LinkError> {
        // Resolve module by stable ID (fallback derives ID from import specifier).
        let module_name = Self::extract_module_name(&import.module_specifier);
        let target_module_id = if import.module_id == 0 {
            module_id_from_name(&module_name)
        } else {
            import.module_id
        };

        let module = self
            .modules_by_id
            .get(&target_module_id)
            .ok_or(LinkError::ModuleIdNotFound(target_module_id))?;

        // Resolve export by symbol ID. Name is debug-only.
        let target_symbol_id: SymbolId = if import.symbol_id == 0 {
            return Err(LinkError::SymbolNotFound {
                module: module_name,
                symbol: import.symbol.clone(),
            });
        } else {
            import.symbol_id
        };
        let export_index = self
            .exports_by_symbol
            .get(&target_module_id)
            .and_then(|by_symbol| by_symbol.get(&target_symbol_id))
            .copied()
            .ok_or_else(|| LinkError::SymbolNotFound {
                module: module.metadata.name.clone(),
                symbol: import.symbol.clone(),
            })?;
        let export = &module.exports[export_index];

        if import.scope != export.scope {
            return Err(LinkError::ScopeMismatch {
                symbol: import.symbol.clone(),
                expected: import.scope,
                actual: export.scope,
            });
        }

        // For concrete symbol imports, structural signature hash is required.
        if import.symbol != "*" {
            if import.type_symbol_id == 0 || export.type_symbol_id == 0 {
                return Err(LinkError::MissingTypeSignature {
                    symbol: import.symbol.clone(),
                    import_hash: import.type_symbol_id,
                    export_hash: export.type_symbol_id,
                });
            }

            if import.type_symbol_id != export.type_symbol_id {
                return Err(LinkError::TypeSignatureMismatch {
                    symbol: import.symbol.clone(),
                    expected_hash: import.type_symbol_id,
                    actual_hash: export.type_symbol_id,
                    expected_pretty: import
                        .type_signature
                        .clone()
                        .unwrap_or_else(|| format!("hash:{:016x}", import.type_symbol_id)),
                    actual_pretty: export
                        .type_signature
                        .clone()
                        .unwrap_or_else(|| format!("hash:{:016x}", export.type_symbol_id)),
                });
            }
        }

        // Validate index bounds for function/class exports.
        // Constant exports can be either constant-pool-backed OR module-global-slot-backed,
        // so linker-time bounds checks are deferred to runtime materialization.
        let max_index = match export.symbol_type {
            SymbolType::Function => module.functions.len(),
            SymbolType::Class => module.classes.len(),
            SymbolType::Constant => usize::MAX,
        };

        if !matches!(export.symbol_type, SymbolType::Constant) && export.index >= max_index {
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
        if let Some(stripped) = specifier.strip_prefix('@') {
            if let Some(at_pos) = stripped.find('@') {
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

    /// Get a module by stable module ID.
    pub fn get_module_by_id(&self, module_id: ModuleId) -> Option<&Arc<Module>> {
        self.modules_by_id.get(&module_id)
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
    use crate::compiler::{ConstantPool, Metadata};

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
                generic_templates: vec![],
                template_symbol_table: vec![],
                mono_debug_map: vec![],
            },
            exports: vec![],
            imports: vec![],
            checksum: [0; 32],
            native_functions: vec![],
            jit_hints: vec![],
            reflection: None,
            debug_info: None,
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
        let module_id = module_id_from_name("missing");
        let import = Import {
            module_specifier: "missing".to_string(),
            symbol: "foo".to_string(),
            alias: None,
            module_id,
            symbol_id: 123,
            scope: crate::compiler::SymbolScope::Module,
            type_symbol_id: 456,
            type_signature: None,
            runtime_global_slot: None,
        };

        let result = linker.resolve_import(&import, "main");
        assert!(matches!(result, Err(LinkError::ModuleIdNotFound(_))));
    }

    #[test]
    fn test_resolve_symbol_not_found() {
        let mut linker = ModuleLinker::new();
        let module = Arc::new(create_test_module("logging"));
        linker.add_module(module).unwrap();

        let module_id = module_id_from_name("logging");
        let import = Import {
            module_specifier: "logging".to_string(),
            symbol: "missing_function".to_string(),
            alias: None,
            module_id,
            symbol_id: 999,
            scope: crate::compiler::SymbolScope::Module,
            type_symbol_id: 111,
            type_signature: None,
            runtime_global_slot: None,
        };

        let result = linker.resolve_import(&import, "main");
        assert!(matches!(result, Err(LinkError::SymbolNotFound { .. })));
    }

    #[test]
    fn test_resolve_import_requires_structural_type_hash() {
        let mut linker = ModuleLinker::new();
        let mut module = create_test_module("typed");
        module.functions.push(crate::compiler::Function {
            name: "f".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![],
        });
        module.exports.push(Export {
            name: "f".to_string(),
            symbol_type: SymbolType::Function,
            index: 0,
            symbol_id: 10,
            scope: SymbolScope::Module,
            type_symbol_id: 42,
            type_signature: Some("fn(min=0,params=[],rest=_,ret=number)".to_string()),
        });
        linker.add_module(Arc::new(module)).unwrap();

        let module_id = module_id_from_name("typed");
        let import = Import {
            module_specifier: "typed".to_string(),
            symbol: "f".to_string(),
            alias: None,
            module_id,
            symbol_id: 10,
            scope: SymbolScope::Module,
            type_symbol_id: 0,
            type_signature: None,
            runtime_global_slot: None,
        };

        let result = linker.resolve_import(&import, "main");
        assert!(matches!(
            result,
            Err(LinkError::MissingTypeSignature { .. })
        ));
    }
}
