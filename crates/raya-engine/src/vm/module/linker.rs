//! Module linking and symbol resolution
//!
//! Handles resolving imported symbols from module exports and linking modules together.

use crate::compiler::{
    module_id_from_name, Export, Import, Module, ModuleId, SymbolId, SymbolScope, SymbolType,
    TypeSignatureHash,
};
use crate::parser::types::structural_signature_is_assignable;
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

    /// Import missing required module ID metadata.
    #[error("Import '{module_specifier}' is missing required module ID metadata")]
    MissingModuleId { module_specifier: String },

    /// Import missing required symbol ID metadata.
    #[error("Import '{module}::{symbol}' is missing required symbol ID metadata")]
    MissingSymbolId { module: String, symbol: String },

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
    fn escape_signature_atom(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace(':', "\\:")
            .replace(',', "\\,")
            .replace('|', "\\|")
    }

    fn namespace_signature_for_module(
        module: &Module,
    ) -> Result<(TypeSignatureHash, String), LinkError> {
        let mut members = Vec::new();
        for export in &module.exports {
            let Some(type_signature) = export.type_signature.as_deref() else {
                return Err(LinkError::MissingTypeSignature {
                    symbol: format!("{}::{}", module.metadata.name, export.name),
                    import_hash: 0,
                    export_hash: export.signature_hash,
                });
            };
            members.push(format!(
                "prop:{}:ro:req:{}",
                Self::escape_signature_atom(&export.name),
                type_signature
            ));
        }
        members.sort();
        let signature = format!("obj({})", members.join(","));
        let hash = crate::parser::types::signature_hash(&signature);
        Ok((hash, signature))
    }

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
        let target_module_id = if import.module_id == 0 {
            return Err(LinkError::MissingModuleId {
                module_specifier: import.module_specifier.clone(),
            });
        } else {
            import.module_id
        };

        let module = self
            .modules_by_id
            .get(&target_module_id)
            .ok_or(LinkError::ModuleIdNotFound(target_module_id))?;

        if import.symbol == "*" {
            let (actual_hash, actual_signature) = Self::namespace_signature_for_module(module)?;
            let Some(expected_signature) = import.type_signature.as_deref() else {
                return Err(LinkError::MissingTypeSignature {
                    symbol: format!("{}::*", module.metadata.name),
                    import_hash: import.signature_hash,
                    export_hash: actual_hash,
                });
            };
            if import.signature_hash == 0 {
                return Err(LinkError::MissingTypeSignature {
                    symbol: format!("{}::*", module.metadata.name),
                    import_hash: import.signature_hash,
                    export_hash: actual_hash,
                });
            }
            if import.signature_hash != actual_hash
                && !structural_signature_is_assignable(expected_signature, &actual_signature)
            {
                return Err(LinkError::TypeSignatureMismatch {
                    symbol: format!("{}::*", module.metadata.name),
                    expected_hash: import.signature_hash,
                    actual_hash,
                    expected_pretty: expected_signature.to_string(),
                    actual_pretty: actual_signature.clone(),
                });
            }
            return Ok(ResolvedSymbol {
                module: module.clone(),
                export: Export {
                    name: "*".to_string(),
                    symbol_type: SymbolType::Constant,
                    index: 0,
                    symbol_id: 0,
                    scope: import.scope,
                    signature_hash: actual_hash,
                    type_signature: Some(actual_signature),
                    nominal_type: None,
                },
                index: 0,
            });
        }

        // Resolve export by symbol ID. Name is debug-only.
        let target_symbol_id: SymbolId = if import.symbol_id == 0 {
            return Err(LinkError::MissingSymbolId {
                module: module.metadata.name.clone(),
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
            if import.signature_hash == 0 || export.signature_hash == 0 {
                return Err(LinkError::MissingTypeSignature {
                    symbol: import.symbol.clone(),
                    import_hash: import.signature_hash,
                    export_hash: export.signature_hash,
                });
            }

            if import.signature_hash != export.signature_hash {
                let signatures_assignable = match (
                    import.type_signature.as_deref(),
                    export.type_signature.as_deref(),
                ) {
                    (Some(expected), Some(actual)) => {
                        structural_signature_is_assignable(expected, actual)
                    }
                    _ => false,
                };
                if signatures_assignable {
                    // Structural compatibility allows assignable subset/superset forms
                    // even when canonical hashes differ.
                    // Keep symbol identity checks separate (symbol_id/module_id).
                } else {
                    return Err(LinkError::TypeSignatureMismatch {
                        symbol: import.symbol.clone(),
                        expected_hash: import.signature_hash,
                        actual_hash: export.signature_hash,
                        expected_pretty: import
                            .type_signature
                            .clone()
                            .unwrap_or_else(|| format!("hash:{:016x}", import.signature_hash)),
                        actual_pretty: export
                            .type_signature
                            .clone()
                            .unwrap_or_else(|| format!("hash:{:016x}", export.signature_hash)),
                    });
                }
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
    use crate::parser::types::signature_hash;

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
                structural_shapes: vec![],
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
            signature_hash: 456,
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
            signature_hash: 111,
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
            signature_hash: 42,
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
            signature_hash: 0,
            type_signature: None,
            runtime_global_slot: None,
        };

        let result = linker.resolve_import(&import, "main");
        assert!(matches!(
            result,
            Err(LinkError::MissingTypeSignature { .. })
        ));
    }

    #[test]
    fn test_resolve_namespace_import_validates_structural_signature() {
        let mut linker = ModuleLinker::new();
        let mut module = create_test_module("typed");
        module.exports.push(Export {
            name: "answer".to_string(),
            symbol_type: SymbolType::Constant,
            index: 0,
            symbol_id: 1,
            scope: SymbolScope::Module,
            signature_hash: crate::parser::types::signature_hash("number"),
            type_signature: Some("number".to_string()),
        });
        linker.add_module(Arc::new(module)).unwrap();

        let expected_signature = "obj(prop:answer:ro:req:number)".to_string();
        let import = Import {
            module_specifier: "typed".to_string(),
            symbol: "*".to_string(),
            alias: Some("typedNs".to_string()),
            module_id: module_id_from_name("typed"),
            symbol_id: 0,
            scope: crate::compiler::SymbolScope::Module,
            signature_hash: crate::parser::types::signature_hash(&expected_signature),
            type_signature: Some(expected_signature.clone()),
            runtime_global_slot: Some(0),
        };

        let resolved = linker
            .resolve_import(&import, "main")
            .expect("resolve namespace import");
        assert_eq!(resolved.export.type_signature.as_deref(), Some(expected_signature.as_str()));
        assert_eq!(resolved.export.signature_hash, import.signature_hash);
    }

    #[test]
    fn test_resolve_import_accepts_structural_object_subset() {
        let mut linker = ModuleLinker::new();
        let mut module = create_test_module("typed");
        module.constants = ConstantPool::new();
        module.exports.push(Export {
            name: "v".to_string(),
            symbol_type: SymbolType::Constant,
            index: 0,
            symbol_id: 11,
            scope: SymbolScope::Module,
            signature_hash: signature_hash(
                "obj(prop:a:rw:req:number,prop:b:rw:req:string,prop:c:rw:req:string)",
            ),
            type_signature: Some(
                "obj(prop:a:rw:req:number,prop:b:rw:req:string,prop:c:rw:req:string)".to_string(),
            ),
        });
        linker.add_module(Arc::new(module)).unwrap();

        let module_id = module_id_from_name("typed");
        let import = Import {
            module_specifier: "typed".to_string(),
            symbol: "v".to_string(),
            alias: None,
            module_id,
            symbol_id: 11,
            scope: SymbolScope::Module,
            signature_hash: signature_hash("obj(prop:a:rw:req:number,prop:b:rw:req:string)"),
            type_signature: Some("obj(prop:a:rw:req:number,prop:b:rw:req:string)".to_string()),
            runtime_global_slot: None,
        };

        assert!(linker.resolve_import(&import, "main").is_ok());
    }

    #[test]
    fn test_resolve_import_accepts_union_subset() {
        let mut linker = ModuleLinker::new();
        let mut module = create_test_module("typed");
        module.exports.push(Export {
            name: "v".to_string(),
            symbol_type: SymbolType::Constant,
            index: 0,
            symbol_id: 12,
            scope: SymbolScope::Module,
            signature_hash: signature_hash("number"),
            type_signature: Some("number".to_string()),
        });
        linker.add_module(Arc::new(module)).unwrap();

        let module_id = module_id_from_name("typed");
        let import = Import {
            module_specifier: "typed".to_string(),
            symbol: "v".to_string(),
            alias: None,
            module_id,
            symbol_id: 12,
            scope: SymbolScope::Module,
            signature_hash: signature_hash("union(number|string)"),
            type_signature: Some("union(number|string)".to_string()),
            runtime_global_slot: None,
        };

        assert!(linker.resolve_import(&import, "main").is_ok());
    }

    #[test]
    fn test_resolve_import_accepts_function_with_fewer_declared_params() {
        let mut linker = ModuleLinker::new();
        let mut module = create_test_module("typed");
        module.functions.push(crate::compiler::Function {
            name: "f".to_string(),
            param_count: 1,
            local_count: 1,
            code: vec![],
        });
        module.exports.push(Export {
            name: "f".to_string(),
            symbol_type: SymbolType::Function,
            index: 0,
            symbol_id: 13,
            scope: SymbolScope::Module,
            signature_hash: signature_hash("fn(min=1,params=[number],rest=_,ret=number)"),
            type_signature: Some("fn(min=1,params=[number],rest=_,ret=number)".to_string()),
        });
        linker.add_module(Arc::new(module)).unwrap();

        let module_id = module_id_from_name("typed");
        let import = Import {
            module_specifier: "typed".to_string(),
            symbol: "f".to_string(),
            alias: None,
            module_id,
            symbol_id: 13,
            scope: SymbolScope::Module,
            signature_hash: signature_hash("fn(min=2,params=[number,number],rest=_,ret=number)"),
            type_signature: Some("fn(min=2,params=[number,number],rest=_,ret=number)".to_string()),
            runtime_global_slot: None,
        };

        assert!(linker.resolve_import(&import, "main").is_ok());
    }
}
