//! Dynamic Module System
//!
//! Enables runtime creation of modules with functions, classes, and globals.
//! Part of Phase 17: Dynamic VM Bootstrap.
//!
//! ## Native Call IDs (0x0E10-0x0E17)
//!
//! | ID     | Method              | Description                    |
//! |--------|---------------------|--------------------------------|
//! | 0x0E10 | createModule        | Create empty dynamic module    |
//! | 0x0E11 | moduleAddFunction   | Add function to module         |
//! | 0x0E12 | moduleAddClass      | Add class to module            |
//! | 0x0E13 | moduleAddGlobal     | Add global variable            |
//! | 0x0E14 | moduleSeal          | Finalize module for execution  |
//! | 0x0E15 | moduleLink          | Resolve imports                |
//! | 0x0E16 | getModule           | Get module info by ID          |
//! | 0x0E17 | getModuleByName     | Get module by name             |

use std::collections::HashMap;

use crate::vm::value::Value;
use crate::vm::VmError;

use super::bytecode_builder::CompiledFunction;

/// Base ID for dynamic functions (high bit set to avoid conflicts with static functions)
pub const DYNAMIC_FUNCTION_BASE: usize = 0x80000000;

/// Base ID for dynamic modules
pub const DYNAMIC_MODULE_BASE: usize = 0x40000000;

/// Import resolution for dynamic modules
#[derive(Debug, Clone)]
pub enum ImportResolution {
    /// Import from another dynamic module
    DynamicModule {
        module_id: usize,
        export_name: String,
    },
    /// Import from a static module
    StaticModule {
        module_name: String,
        export_name: String,
    },
    /// Import a native function
    Native {
        native_id: u16,
    },
}

/// Dynamic module state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleState {
    /// Module is being built
    Building,
    /// Module is sealed and ready for execution
    Sealed,
    /// Module has been linked with imports resolved
    Linked,
}

/// A dynamically created module
#[derive(Debug)]
pub struct DynamicModule {
    /// Unique module ID
    pub id: usize,
    /// Module name
    pub name: String,
    /// Module state
    pub state: ModuleState,

    /// Functions in this module: local_function_id -> CompiledFunction
    pub functions: HashMap<usize, CompiledFunction>,
    /// Function name to ID mapping
    pub function_names: HashMap<String, usize>,

    /// Classes in this module: local_class_id -> global_class_id
    pub classes: HashMap<usize, usize>,
    /// Class name to ID mapping
    pub class_names: HashMap<String, usize>,

    /// Global variables: name -> Value
    pub globals: HashMap<String, Value>,

    /// Exports: name -> export info
    pub exports: HashMap<String, DynamicExport>,

    /// Import resolutions: import_name -> resolution
    pub imports: HashMap<String, ImportResolution>,
}

/// Export from a dynamic module
#[derive(Debug, Clone)]
pub enum DynamicExport {
    Function(usize),
    Class(usize),
    Global(String),
}

impl DynamicModule {
    /// Create a new dynamic module
    pub fn new(id: usize, name: String) -> Self {
        Self {
            id,
            name,
            state: ModuleState::Building,
            functions: HashMap::new(),
            function_names: HashMap::new(),
            classes: HashMap::new(),
            class_names: HashMap::new(),
            globals: HashMap::new(),
            exports: HashMap::new(),
            imports: HashMap::new(),
        }
    }

    /// Add a function to the module
    pub fn add_function(&mut self, func: CompiledFunction) -> Result<usize, VmError> {
        if self.state != ModuleState::Building {
            return Err(VmError::RuntimeError(
                "Cannot add function to sealed module".to_string(),
            ));
        }

        let func_id = func.function_id;
        let func_name = func.name.clone();

        self.functions.insert(func_id, func);
        self.function_names.insert(func_name.clone(), func_id);

        // Auto-export the function
        self.exports.insert(func_name, DynamicExport::Function(func_id));

        Ok(func_id)
    }

    /// Add a class to the module
    pub fn add_class(&mut self, local_id: usize, global_id: usize, name: String) -> Result<(), VmError> {
        if self.state != ModuleState::Building {
            return Err(VmError::RuntimeError(
                "Cannot add class to sealed module".to_string(),
            ));
        }

        self.classes.insert(local_id, global_id);
        self.class_names.insert(name.clone(), global_id);

        // Auto-export the class
        self.exports.insert(name, DynamicExport::Class(global_id));

        Ok(())
    }

    /// Add a global variable to the module
    pub fn add_global(&mut self, name: String, value: Value) -> Result<(), VmError> {
        if self.state != ModuleState::Building {
            return Err(VmError::RuntimeError(
                "Cannot add global to sealed module".to_string(),
            ));
        }

        self.globals.insert(name.clone(), value);
        self.exports.insert(name.clone(), DynamicExport::Global(name));

        Ok(())
    }

    /// Seal the module, making it ready for execution
    pub fn seal(&mut self) -> Result<(), VmError> {
        if self.state != ModuleState::Building {
            return Err(VmError::RuntimeError(
                "Module is already sealed".to_string(),
            ));
        }

        self.state = ModuleState::Sealed;
        Ok(())
    }

    /// Check if the module is sealed
    pub fn is_sealed(&self) -> bool {
        self.state == ModuleState::Sealed || self.state == ModuleState::Linked
    }

    /// Get a function by ID
    pub fn get_function(&self, id: usize) -> Option<&CompiledFunction> {
        self.functions.get(&id)
    }

    /// Get a function by name
    pub fn get_function_by_name(&self, name: &str) -> Option<&CompiledFunction> {
        self.function_names.get(name).and_then(|id| self.functions.get(id))
    }

    /// Get a global variable
    pub fn get_global(&self, name: &str) -> Option<Value> {
        self.globals.get(name).copied()
    }

    /// Set a global variable (allowed even after sealing)
    pub fn set_global(&mut self, name: &str, value: Value) -> Result<(), VmError> {
        if !self.globals.contains_key(name) {
            return Err(VmError::RuntimeError(
                format!("Global '{}' not found in module", name),
            ));
        }
        self.globals.insert(name.to_string(), value);
        Ok(())
    }

    /// Get module info as a map
    pub fn get_info(&self) -> DynamicModuleInfo {
        DynamicModuleInfo {
            id: self.id,
            name: self.name.clone(),
            is_sealed: self.is_sealed(),
            function_count: self.functions.len(),
            class_count: self.classes.len(),
            global_count: self.globals.len(),
            function_names: self.function_names.keys().cloned().collect(),
            class_names: self.class_names.keys().cloned().collect(),
            global_names: self.globals.keys().cloned().collect(),
        }
    }
}

/// Module information for introspection
#[derive(Debug, Clone)]
pub struct DynamicModuleInfo {
    pub id: usize,
    pub name: String,
    pub is_sealed: bool,
    pub function_count: usize,
    pub class_count: usize,
    pub global_count: usize,
    pub function_names: Vec<String>,
    pub class_names: Vec<String>,
    pub global_names: Vec<String>,
}

/// Registry for dynamic modules
#[derive(Debug, Default)]
pub struct DynamicModuleRegistry {
    /// Modules by ID
    modules: HashMap<usize, DynamicModule>,
    /// Module name to ID mapping
    module_names: HashMap<String, usize>,
    /// Next module ID
    next_module_id: usize,
    /// Next function ID (within dynamic range)
    next_function_id: usize,
}

impl DynamicModuleRegistry {
    /// Create a new registry
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
            module_names: HashMap::new(),
            next_module_id: 0,
            next_function_id: 0,
        }
    }

    /// Create a new dynamic module
    pub fn create_module(&mut self, name: String) -> Result<usize, VmError> {
        if self.module_names.contains_key(&name) {
            return Err(VmError::RuntimeError(
                format!("Module '{}' already exists", name),
            ));
        }

        let id = DYNAMIC_MODULE_BASE + self.next_module_id;
        self.next_module_id += 1;

        let module = DynamicModule::new(id, name.clone());
        self.modules.insert(id, module);
        self.module_names.insert(name, id);

        Ok(id)
    }

    /// Get a module by ID
    pub fn get(&self, id: usize) -> Option<&DynamicModule> {
        self.modules.get(&id)
    }

    /// Get a module by ID (mutable)
    pub fn get_mut(&mut self, id: usize) -> Option<&mut DynamicModule> {
        self.modules.get_mut(&id)
    }

    /// Get a module by name
    pub fn get_by_name(&self, name: &str) -> Option<&DynamicModule> {
        self.module_names.get(name).and_then(|id| self.modules.get(id))
    }

    /// Get a module by name (mutable)
    pub fn get_by_name_mut(&mut self, name: &str) -> Option<&mut DynamicModule> {
        if let Some(&id) = self.module_names.get(name) {
            self.modules.get_mut(&id)
        } else {
            None
        }
    }

    /// Allocate a new function ID in the dynamic range
    pub fn allocate_function_id(&mut self) -> usize {
        let id = DYNAMIC_FUNCTION_BASE + self.next_function_id;
        self.next_function_id += 1;
        id
    }

    /// Check if a function ID is in the dynamic range
    pub fn is_dynamic_function(function_id: usize) -> bool {
        function_id >= DYNAMIC_FUNCTION_BASE
    }

    /// Check if a module ID is in the dynamic range
    pub fn is_dynamic_module(module_id: usize) -> bool {
        module_id >= DYNAMIC_MODULE_BASE
    }

    /// Get all module IDs
    pub fn module_ids(&self) -> Vec<usize> {
        self.modules.keys().copied().collect()
    }

    /// Get all module names
    pub fn module_names(&self) -> Vec<String> {
        self.module_names.keys().cloned().collect()
    }

    /// Get a function from any dynamic module by function ID
    pub fn get_function(&self, function_id: usize) -> Option<&CompiledFunction> {
        for module in self.modules.values() {
            if let Some(func) = module.get_function(function_id) {
                return Some(func);
            }
        }
        None
    }

    /// Get the module containing a function
    pub fn get_module_for_function(&self, function_id: usize) -> Option<&DynamicModule> {
        self.modules.values().find(|&module| module.functions.contains_key(&function_id)).map(|v| v as _)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_module() {
        let mut registry = DynamicModuleRegistry::new();
        let id = registry.create_module("test".to_string()).unwrap();

        assert!(DynamicModuleRegistry::is_dynamic_module(id));

        let module = registry.get(id).unwrap();
        assert_eq!(module.name, "test");
        assert!(!module.is_sealed());
    }

    #[test]
    fn test_duplicate_module_name() {
        let mut registry = DynamicModuleRegistry::new();
        registry.create_module("test".to_string()).unwrap();

        let result = registry.create_module("test".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_add_function() {
        let mut registry = DynamicModuleRegistry::new();
        let module_id = registry.create_module("test".to_string()).unwrap();

        let func = CompiledFunction {
            function_id: registry.allocate_function_id(),
            name: "add".to_string(),
            param_count: 2,
            local_count: 2,
            max_stack: 4,
            bytecode: vec![],
            constants: vec![],
        };

        let module = registry.get_mut(module_id).unwrap();
        let func_id = module.add_function(func).unwrap();

        assert!(DynamicModuleRegistry::is_dynamic_function(func_id));
        assert!(module.get_function(func_id).is_some());
        assert!(module.get_function_by_name("add").is_some());
    }

    #[test]
    fn test_seal_module() {
        let mut registry = DynamicModuleRegistry::new();
        let module_id = registry.create_module("test".to_string()).unwrap();

        let module = registry.get_mut(module_id).unwrap();
        module.seal().unwrap();

        assert!(module.is_sealed());

        // Cannot seal again
        let result = module.seal();
        assert!(result.is_err());
    }

    #[test]
    fn test_cannot_add_to_sealed_module() {
        let mut registry = DynamicModuleRegistry::new();
        let module_id = registry.create_module("test".to_string()).unwrap();

        let module = registry.get_mut(module_id).unwrap();
        module.seal().unwrap();

        let func = CompiledFunction {
            function_id: DYNAMIC_FUNCTION_BASE,
            name: "test".to_string(),
            param_count: 0,
            local_count: 0,
            max_stack: 0,
            bytecode: vec![],
            constants: vec![],
        };

        let result = module.add_function(func);
        assert!(result.is_err());
    }

    #[test]
    fn test_add_global() {
        let mut registry = DynamicModuleRegistry::new();
        let module_id = registry.create_module("test".to_string()).unwrap();

        let module = registry.get_mut(module_id).unwrap();
        module.add_global("PI".to_string(), Value::f64(3.14159)).unwrap();

        assert_eq!(module.get_global("PI"), Some(Value::f64(3.14159)));
    }

    #[test]
    fn test_module_info() {
        let mut registry = DynamicModuleRegistry::new();
        let module_id = registry.create_module("mymodule".to_string()).unwrap();

        let func_id = registry.allocate_function_id();
        let func = CompiledFunction {
            function_id: func_id,
            name: "hello".to_string(),
            param_count: 0,
            local_count: 0,
            max_stack: 0,
            bytecode: vec![],
            constants: vec![],
        };

        let module = registry.get_mut(module_id).unwrap();
        module.add_function(func).unwrap();
        module.add_global("VERSION".to_string(), Value::i32(1)).unwrap();

        let info = module.get_info();
        assert_eq!(info.name, "mymodule");
        assert!(!info.is_sealed);
        assert_eq!(info.function_count, 1);
        assert_eq!(info.global_count, 1);
        assert!(info.function_names.contains(&"hello".to_string()));
        assert!(info.global_names.contains(&"VERSION".to_string()));
    }

    #[test]
    fn test_get_function_across_modules() {
        let mut registry = DynamicModuleRegistry::new();

        let module1_id = registry.create_module("module1".to_string()).unwrap();
        let func1_id = registry.allocate_function_id();
        let func1 = CompiledFunction {
            function_id: func1_id,
            name: "func1".to_string(),
            param_count: 0,
            local_count: 0,
            max_stack: 0,
            bytecode: vec![1, 2, 3],
            constants: vec![],
        };

        let module1 = registry.get_mut(module1_id).unwrap();
        module1.add_function(func1).unwrap();

        // Should find function across all modules
        let found = registry.get_function(func1_id);
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "func1");
    }

    #[test]
    fn test_allocate_function_ids() {
        let mut registry = DynamicModuleRegistry::new();

        let id1 = registry.allocate_function_id();
        let id2 = registry.allocate_function_id();
        let id3 = registry.allocate_function_id();

        assert!(DynamicModuleRegistry::is_dynamic_function(id1));
        assert!(DynamicModuleRegistry::is_dynamic_function(id2));
        assert!(DynamicModuleRegistry::is_dynamic_function(id3));
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
    }

    #[test]
    fn test_get_by_name() {
        let mut registry = DynamicModuleRegistry::new();
        registry.create_module("mymod".to_string()).unwrap();

        let module = registry.get_by_name("mymod");
        assert!(module.is_some());
        assert_eq!(module.unwrap().name, "mymod");

        let not_found = registry.get_by_name("nonexistent");
        assert!(not_found.is_none());
    }
}
