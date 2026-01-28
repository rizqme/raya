//! Module registry for tracking loaded modules
//!
//! The ModuleRegistry maintains a mapping of loaded modules by both name and checksum,
//! enabling deduplication and efficient module lookups.

use crate::compiler::Module;
use std::collections::HashMap;
use std::sync::Arc;

/// Registry for tracking loaded modules
#[derive(Debug)]
pub struct ModuleRegistry {
    /// Modules indexed by name
    by_name: HashMap<String, Arc<Module>>,
    /// Modules indexed by SHA-256 checksum (for deduplication)
    by_checksum: HashMap<[u8; 32], Arc<Module>>,
}

impl ModuleRegistry {
    /// Create a new empty module registry
    pub fn new() -> Self {
        Self {
            by_name: HashMap::new(),
            by_checksum: HashMap::new(),
        }
    }

    /// Register a module in the registry
    ///
    /// If a module with the same checksum is already loaded, this is a no-op.
    /// This enables deduplication of identical modules.
    ///
    /// # Arguments
    /// * `module` - The module to register
    ///
    /// # Returns
    /// * `Ok(())` - Module registered successfully
    /// * `Err(String)` - Registration failed
    pub fn register(&mut self, module: Arc<Module>) -> Result<(), String> {
        let checksum = module.checksum;

        // Check if already loaded by checksum (deduplication)
        if self.by_checksum.contains_key(&checksum) {
            return Ok(()); // Already loaded, skip
        }

        let name = module.metadata.name.clone();

        // Register by both name and checksum
        self.by_name.insert(name, module.clone());
        self.by_checksum.insert(checksum, module);

        Ok(())
    }

    /// Get a module by name
    ///
    /// # Arguments
    /// * `name` - The module name to look up
    ///
    /// # Returns
    /// * `Some(&Arc<Module>)` - Module found
    /// * `None` - Module not found
    pub fn get_by_name(&self, name: &str) -> Option<&Arc<Module>> {
        self.by_name.get(name)
    }

    /// Get a module by SHA-256 checksum
    ///
    /// # Arguments
    /// * `checksum` - The 32-byte SHA-256 checksum
    ///
    /// # Returns
    /// * `Some(&Arc<Module>)` - Module found
    /// * `None` - Module not found
    pub fn get_by_checksum(&self, checksum: &[u8; 32]) -> Option<&Arc<Module>> {
        self.by_checksum.get(checksum)
    }

    /// Check if a module is loaded by checksum
    ///
    /// # Arguments
    /// * `checksum` - The 32-byte SHA-256 checksum
    ///
    /// # Returns
    /// * `true` - Module is loaded
    /// * `false` - Module is not loaded
    pub fn is_loaded(&self, checksum: &[u8; 32]) -> bool {
        self.by_checksum.contains_key(checksum)
    }

    /// Get all loaded modules
    ///
    /// # Returns
    /// A vector of all loaded modules
    pub fn all_modules(&self) -> Vec<Arc<Module>> {
        self.by_checksum.values().cloned().collect()
    }

    /// Get the number of loaded modules
    pub fn module_count(&self) -> usize {
        self.by_checksum.len()
    }
}

impl Default for ModuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_module(name: &str) -> Module {
        Module::new(name.to_string())
    }

    #[test]
    fn test_register_module() {
        let mut registry = ModuleRegistry::new();
        let module = Arc::new(create_test_module("test"));

        assert!(registry.register(module.clone()).is_ok());
        assert_eq!(registry.module_count(), 1);
    }

    #[test]
    fn test_get_by_name() {
        let mut registry = ModuleRegistry::new();
        let module = Arc::new(create_test_module("test"));
        let checksum = module.checksum;

        registry.register(module.clone()).unwrap();

        let retrieved = registry.get_by_name("test");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().checksum, checksum);
    }

    #[test]
    fn test_get_by_checksum() {
        let mut registry = ModuleRegistry::new();
        let module = Arc::new(create_test_module("test"));
        let checksum = module.checksum;

        registry.register(module.clone()).unwrap();

        let retrieved = registry.get_by_checksum(&checksum);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().metadata.name, "test");
    }

    #[test]
    fn test_duplicate_registration() {
        let mut registry = ModuleRegistry::new();
        let module = Arc::new(create_test_module("test"));

        // Register twice
        registry.register(module.clone()).unwrap();
        registry.register(module.clone()).unwrap();

        // Should only be registered once
        assert_eq!(registry.module_count(), 1);
    }

    #[test]
    fn test_is_loaded() {
        let mut registry = ModuleRegistry::new();
        let module = Arc::new(create_test_module("test"));
        let checksum = module.checksum;

        assert!(!registry.is_loaded(&checksum));

        registry.register(module).unwrap();

        assert!(registry.is_loaded(&checksum));
    }

    #[test]
    fn test_all_modules() {
        let mut registry = ModuleRegistry::new();

        // Create modules with different checksums by encoding them
        let mut module1 = create_test_module("test1");
        let bytes1 = module1.encode();
        let decoded1 = Module::decode(&bytes1).unwrap();

        let mut module2 = create_test_module("test2");
        let bytes2 = module2.encode();
        let decoded2 = Module::decode(&bytes2).unwrap();

        registry.register(Arc::new(decoded1)).unwrap();
        registry.register(Arc::new(decoded2)).unwrap();

        let all = registry.all_modules();
        assert_eq!(all.len(), 2);
    }
}
