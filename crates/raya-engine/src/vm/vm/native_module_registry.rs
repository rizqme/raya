// Native module registry for tracking loaded native modules
//
// The NativeModuleRegistry maintains a mapping of loaded native modules by name,
// enabling efficient module lookups and preventing duplicate loading.

use std::collections::HashMap;
use std::sync::Arc;

// Re-export SDK types for convenience
pub use crate::vm::ffi::{NativeFn, NativeModule};

/// Registry for tracking loaded native modules
#[derive(Debug)]
pub struct NativeModuleRegistry {
    /// Native modules indexed by name
    by_name: HashMap<String, Arc<NativeModule>>,
}

impl NativeModuleRegistry {
    /// Create a new empty native module registry
    pub fn new() -> Self {
        Self {
            by_name: HashMap::new(),
        }
    }

    /// Register a native module in the registry
    ///
    /// If a module with the same name is already loaded, this returns an error.
    ///
    /// # Arguments
    /// * `module` - The native module to register
    ///
    /// # Returns
    /// * `Ok(())` - Module registered successfully
    /// * `Err(String)` - Registration failed (e.g., duplicate name)
    pub fn register(&mut self, module: Arc<NativeModule>) -> Result<(), String> {
        let name = module.name().to_string();

        // Check for duplicate
        if self.by_name.contains_key(&name) {
            return Err(format!("Native module '{}' is already registered", name));
        }

        // Register by name
        self.by_name.insert(name, module);

        Ok(())
    }

    /// Register a native module by name and Arc
    ///
    /// Convenience method for registering with explicit name.
    ///
    /// # Arguments
    /// * `name` - The module name (e.g., "std:json")
    /// * `module` - The native module
    ///
    /// # Returns
    /// * `Ok(())` - Module registered successfully
    /// * `Err(String)` - Registration failed
    pub fn register_as(&mut self, name: impl Into<String>, module: Arc<NativeModule>) -> Result<(), String> {
        let name_str = name.into();

        if self.by_name.contains_key(&name_str) {
            return Err(format!("Native module '{}' is already registered", name_str));
        }

        self.by_name.insert(name_str, module);
        Ok(())
    }

    /// Get a native module by name
    ///
    /// # Arguments
    /// * `name` - The module name to look up
    ///
    /// # Returns
    /// * `Some(&Arc<NativeModule>)` - Module found
    /// * `None` - Module not found
    pub fn get(&self, name: &str) -> Option<&Arc<NativeModule>> {
        self.by_name.get(name)
    }

    /// Check if a native module is loaded
    ///
    /// # Arguments
    /// * `name` - The module name
    ///
    /// # Returns
    /// * `true` - Module is loaded
    /// * `false` - Module is not loaded
    pub fn is_loaded(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// Get all loaded native modules
    ///
    /// # Returns
    /// A vector of all loaded native modules
    pub fn all_modules(&self) -> Vec<Arc<NativeModule>> {
        self.by_name.values().cloned().collect()
    }

    /// Get all module names
    ///
    /// # Returns
    /// A vector of all registered module names
    pub fn module_names(&self) -> Vec<String> {
        self.by_name.keys().cloned().collect()
    }

    /// Get the number of loaded native modules
    pub fn module_count(&self) -> usize {
        self.by_name.len()
    }

    /// Remove a native module from the registry
    ///
    /// # Arguments
    /// * `name` - The module name to remove
    ///
    /// # Returns
    /// * `Some(Arc<NativeModule>)` - The removed module
    /// * `None` - Module not found
    pub fn remove(&mut self, name: &str) -> Option<Arc<NativeModule>> {
        self.by_name.remove(name)
    }

    /// Clear all native modules from the registry
    pub fn clear(&mut self) {
        self.by_name.clear();
    }
}

impl Default for NativeModuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_module(name: &str) -> NativeModule {
        NativeModule::new(name, "1.0.0")
    }

    #[test]
    fn test_register_module() {
        let mut registry = NativeModuleRegistry::new();
        let module = Arc::new(create_test_module("test"));

        assert!(registry.register(module.clone()).is_ok());
        assert_eq!(registry.module_count(), 1);
    }

    #[test]
    fn test_get_by_name() {
        let mut registry = NativeModuleRegistry::new();
        let module = Arc::new(create_test_module("test"));

        registry.register(module.clone()).unwrap();

        let retrieved = registry.get("test");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name(), "test");
    }

    #[test]
    fn test_duplicate_registration() {
        let mut registry = NativeModuleRegistry::new();
        let module = Arc::new(create_test_module("test"));

        // Register first time - success
        assert!(registry.register(module.clone()).is_ok());

        // Register second time - should fail
        assert!(registry.register(module.clone()).is_err());

        // Should only be registered once
        assert_eq!(registry.module_count(), 1);
    }

    #[test]
    fn test_is_loaded() {
        let mut registry = NativeModuleRegistry::new();
        let module = Arc::new(create_test_module("test"));

        assert!(!registry.is_loaded("test"));

        registry.register(module).unwrap();

        assert!(registry.is_loaded("test"));
    }

    #[test]
    fn test_register_as() {
        let mut registry = NativeModuleRegistry::new();
        let module = Arc::new(create_test_module("internal_name"));

        // Register with custom name
        assert!(registry.register_as("std:json", module.clone()).is_ok());

        // Should be accessible by the custom name
        assert!(registry.is_loaded("std:json"));
        assert!(!registry.is_loaded("internal_name"));
    }

    #[test]
    fn test_remove() {
        let mut registry = NativeModuleRegistry::new();
        let module = Arc::new(create_test_module("test"));

        registry.register(module).unwrap();
        assert_eq!(registry.module_count(), 1);

        let removed = registry.remove("test");
        assert!(removed.is_some());
        assert_eq!(registry.module_count(), 0);
    }

    #[test]
    fn test_clear() {
        let mut registry = NativeModuleRegistry::new();

        registry.register(Arc::new(create_test_module("test1"))).unwrap();
        registry.register(Arc::new(create_test_module("test2"))).unwrap();

        assert_eq!(registry.module_count(), 2);

        registry.clear();

        assert_eq!(registry.module_count(), 0);
    }
}
