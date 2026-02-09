//! Runtime Type Builder for Reflection API
//!
//! Provides infrastructure for creating classes and functions at runtime via the Reflect API.
//! This module implements Phase 14: Runtime Type Creation.
//!
//! ## Native Call IDs (0x0DE0-0x0DEF)
//!
//! | ID     | Method                      | Description                          |
//! |--------|-----------------------------|------------------------------------- |
//! | 0x0DE0 | newClassBuilder             | Create a new class builder           |
//! | 0x0DE1 | builderAddField             | Add field to builder                 |
//! | 0x0DE2 | builderAddMethod            | Add method to builder                |
//! | 0x0DE3 | builderSetConstructor       | Set constructor                      |
//! | 0x0DE4 | builderSetParent            | Set parent class                     |
//! | 0x0DE5 | builderAddInterface         | Add interface                        |
//! | 0x0DE6 | builderBuild                | Finalize and register class          |
//! | 0x0DE7 | createFunction              | Create function from bytecode        |
//! | 0x0DE8 | createAsyncFunction         | Create async function                |
//! | 0x0DE9 | createClosure               | Create closure with captures         |
//! | 0x0DEA | createNativeCallback        | Register native callback             |
//! | 0x0DEB | specialize                  | Create new monomorphization          |
//! | 0x0DEC | getSpecializationCache      | Get cached specializations           |

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::vm::object::{Class, Closure, VTable};
use crate::vm::reflect::{
    ClassMetadata, FieldDefinition, FieldInfo, MethodDefinition, MethodInfo, ParameterInfo,
    SubclassDefinition, TypeInfo,
};
use crate::vm::value::Value;
use crate::vm::VmError;

/// Global counter for builder IDs
static NEXT_BUILDER_ID: AtomicUsize = AtomicUsize::new(1);

/// Generate a unique builder ID
fn generate_builder_id() -> usize {
    NEXT_BUILDER_ID.fetch_add(1, Ordering::Relaxed)
}

/// Global counter for dynamic function IDs
static NEXT_DYNAMIC_FUNC_ID: AtomicUsize = AtomicUsize::new(0x1000_0000);

/// Generate a unique dynamic function ID (starting from high range to avoid conflicts)
fn generate_dynamic_function_id() -> usize {
    NEXT_DYNAMIC_FUNC_ID.fetch_add(1, Ordering::Relaxed)
}

/// Builder for incrementally constructing a class at runtime
#[derive(Debug, Clone)]
pub struct ClassBuilder {
    /// Unique builder ID
    pub id: usize,
    /// Class name
    pub name: String,
    /// Parent class ID (if any)
    pub parent_id: Option<usize>,
    /// Fields to add
    pub fields: Vec<FieldDefinition>,
    /// Methods to add
    pub methods: Vec<MethodDefinition>,
    /// Constructor function ID
    pub constructor_id: Option<usize>,
    /// Interfaces implemented
    pub interfaces: Vec<String>,
    /// Whether this builder has been finalized
    pub finalized: bool,
}

impl ClassBuilder {
    /// Create a new class builder
    pub fn new(name: String) -> Self {
        Self {
            id: generate_builder_id(),
            name,
            parent_id: None,
            fields: Vec::new(),
            methods: Vec::new(),
            constructor_id: None,
            interfaces: Vec::new(),
            finalized: false,
        }
    }

    /// Set the parent class
    pub fn set_parent(&mut self, parent_id: usize) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized ClassBuilder".to_string(),
            ));
        }
        self.parent_id = Some(parent_id);
        Ok(())
    }

    /// Add a field to the class
    pub fn add_field(
        &mut self,
        name: String,
        type_name: &str,
        is_static: bool,
        is_readonly: bool,
    ) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized ClassBuilder".to_string(),
            ));
        }

        // Check for duplicate field
        if self.fields.iter().any(|f| f.name == name) {
            return Err(VmError::RuntimeError(format!(
                "Field '{}' already exists in ClassBuilder",
                name
            )));
        }

        let mut field = FieldDefinition::new(name, type_name);
        if is_static {
            field = field.as_static();
        }
        if is_readonly {
            field = field.as_readonly();
        }
        self.fields.push(field);
        Ok(())
    }

    /// Add a method to the class
    pub fn add_method(
        &mut self,
        name: String,
        function_id: usize,
        is_static: bool,
        is_async: bool,
    ) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized ClassBuilder".to_string(),
            ));
        }

        // Check for duplicate method
        if self.methods.iter().any(|m| m.name == name) {
            return Err(VmError::RuntimeError(format!(
                "Method '{}' already exists in ClassBuilder",
                name
            )));
        }

        let mut method = MethodDefinition::new(name, function_id);
        if is_static {
            method = method.as_static();
        }
        if is_async {
            method = method.as_async();
        }
        self.methods.push(method);
        Ok(())
    }

    /// Set the constructor
    pub fn set_constructor(&mut self, function_id: usize) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized ClassBuilder".to_string(),
            ));
        }
        self.constructor_id = Some(function_id);
        Ok(())
    }

    /// Add an interface implementation
    pub fn add_interface(&mut self, interface_name: String) -> Result<(), VmError> {
        if self.finalized {
            return Err(VmError::RuntimeError(
                "Cannot modify finalized ClassBuilder".to_string(),
            ));
        }

        if !self.interfaces.contains(&interface_name) {
            self.interfaces.push(interface_name);
        }
        Ok(())
    }

    /// Convert to SubclassDefinition for building
    pub fn to_definition(&self) -> SubclassDefinition {
        let mut def = SubclassDefinition::new();

        for field in &self.fields {
            def = def.add_field(field.clone());
        }

        for method in &self.methods {
            def = def.add_method(method.clone());
        }

        if let Some(ctor_id) = self.constructor_id {
            def = def.with_constructor(ctor_id);
        }

        for interface in &self.interfaces {
            def = def.implements(interface.clone());
        }

        def
    }

    /// Mark as finalized
    pub fn finalize(&mut self) {
        self.finalized = true;
    }
}

/// Registry for active ClassBuilders
#[derive(Debug, Default)]
pub struct ClassBuilderRegistry {
    /// Active builders by ID
    builders: HashMap<usize, ClassBuilder>,
}

impl ClassBuilderRegistry {
    /// Create a new registry
    pub fn new() -> Self {
        Self {
            builders: HashMap::new(),
        }
    }

    /// Create and register a new builder
    pub fn create_builder(&mut self, name: String) -> usize {
        let builder = ClassBuilder::new(name);
        let id = builder.id;
        self.builders.insert(id, builder);
        id
    }

    /// Get a builder by ID
    pub fn get(&self, id: usize) -> Option<&ClassBuilder> {
        self.builders.get(&id)
    }

    /// Get a mutable builder by ID
    pub fn get_mut(&mut self, id: usize) -> Option<&mut ClassBuilder> {
        self.builders.get_mut(&id)
    }

    /// Remove a builder (after it's been built)
    pub fn remove(&mut self, id: usize) -> Option<ClassBuilder> {
        self.builders.remove(&id)
    }

    /// Check if a builder exists
    pub fn contains(&self, id: usize) -> bool {
        self.builders.contains_key(&id)
    }
}

/// Definition for a dynamically created function
#[derive(Debug, Clone)]
pub struct DynamicFunction {
    /// Unique function ID
    pub id: usize,
    /// Function name
    pub name: String,
    /// Parameter count
    pub param_count: usize,
    /// Local variable count
    pub local_count: usize,
    /// Maximum stack depth
    pub max_stack: usize,
    /// Bytecode instructions
    pub bytecode: Vec<u8>,
    /// Whether this is an async function
    pub is_async: bool,
    /// Constant pool (for string constants, etc.)
    pub constants: Vec<Value>,
}

impl DynamicFunction {
    /// Create a new dynamic function
    pub fn new(name: String, param_count: usize, bytecode: Vec<u8>) -> Self {
        Self {
            id: generate_dynamic_function_id(),
            name,
            param_count,
            local_count: param_count, // Start with params as locals
            max_stack: 16,            // Default stack depth
            bytecode,
            is_async: false,
            constants: Vec::new(),
        }
    }

    /// Create an async function
    pub fn new_async(name: String, param_count: usize, bytecode: Vec<u8>) -> Self {
        let mut func = Self::new(name, param_count, bytecode);
        func.is_async = true;
        func
    }

    /// Set local count
    pub fn with_locals(mut self, local_count: usize) -> Self {
        self.local_count = local_count;
        self
    }

    /// Set max stack
    pub fn with_max_stack(mut self, max_stack: usize) -> Self {
        self.max_stack = max_stack;
        self
    }

    /// Add a constant
    pub fn add_constant(&mut self, value: Value) -> usize {
        let idx = self.constants.len();
        self.constants.push(value);
        idx
    }
}

/// Registry for dynamically created functions
#[derive(Debug, Default)]
pub struct DynamicFunctionRegistry {
    /// Functions by ID
    functions: HashMap<usize, DynamicFunction>,
}

impl DynamicFunctionRegistry {
    /// Create a new registry
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
        }
    }

    /// Register a new function
    pub fn register(&mut self, func: DynamicFunction) -> usize {
        let id = func.id;
        self.functions.insert(id, func);
        id
    }

    /// Get a function by ID
    pub fn get(&self, id: usize) -> Option<&DynamicFunction> {
        self.functions.get(&id)
    }

    /// Check if a function exists
    pub fn contains(&self, id: usize) -> bool {
        self.functions.contains_key(&id)
    }

    /// Get all function IDs
    pub fn function_ids(&self) -> impl Iterator<Item = usize> + '_ {
        self.functions.keys().copied()
    }
}

/// Dynamic closure with captured values
#[derive(Debug, Clone)]
pub struct DynamicClosure {
    /// The function this closure wraps
    pub function_id: usize,
    /// Captured variable names and values
    pub captures: Vec<(String, Value)>,
}

impl DynamicClosure {
    /// Create a new dynamic closure
    pub fn new(function_id: usize) -> Self {
        Self {
            function_id,
            captures: Vec::new(),
        }
    }

    /// Add a captured variable
    pub fn capture(&mut self, name: String, value: Value) {
        self.captures.push((name, value));
    }

    /// Get captured values as a vector
    pub fn capture_values(&self) -> Vec<Value> {
        self.captures.iter().map(|(_, v)| *v).collect()
    }

    /// Convert to Closure object
    pub fn to_closure(&self) -> Closure {
        Closure::new(self.function_id, self.capture_values())
    }
}

/// Cache for generic specializations
#[derive(Debug, Default)]
pub struct SpecializationCache {
    /// Maps generic name + type args -> specialized class ID
    /// Key format: "GenericName<TypeArg1,TypeArg2,...>"
    cache: HashMap<String, usize>,
    /// Reverse mapping: specialized class ID -> origin info
    origins: HashMap<usize, GenericOrigin>,
}

impl SpecializationCache {
    /// Create a new cache
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            origins: HashMap::new(),
        }
    }

    /// Generate cache key from generic name and type arguments
    fn make_key(generic_name: &str, type_args: &[String]) -> String {
        if type_args.is_empty() {
            generic_name.to_string()
        } else {
            format!("{}<{}>", generic_name, type_args.join(","))
        }
    }

    /// Check if a specialization exists
    pub fn get(&self, generic_name: &str, type_args: &[String]) -> Option<usize> {
        let key = Self::make_key(generic_name, type_args);
        self.cache.get(&key).copied()
    }

    /// Register a new specialization
    pub fn register(
        &mut self,
        generic_name: &str,
        type_args: Vec<String>,
        class_id: usize,
    ) {
        let key = Self::make_key(generic_name, &type_args);
        self.cache.insert(key, class_id);
        self.origins.insert(
            class_id,
            GenericOrigin {
                name: generic_name.to_string(),
                type_parameters: vec![], // Would need to be filled from generic definition
                type_arguments: type_args,
            },
        );
    }

    /// Get origin info for a specialized class
    pub fn get_origin(&self, class_id: usize) -> Option<&GenericOrigin> {
        self.origins.get(&class_id)
    }

    /// Find all specializations of a generic
    pub fn find_specializations(&self, generic_name: &str) -> Vec<usize> {
        self.origins
            .iter()
            .filter(|(_, origin)| origin.name == generic_name)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Get all cached entries
    pub fn entries(&self) -> impl Iterator<Item = (&String, &usize)> {
        self.cache.iter()
    }
}

/// Information about a generic class origin
#[derive(Debug, Clone)]
pub struct GenericOrigin {
    /// Original generic name (e.g., "Box")
    pub name: String,
    /// Type parameter names (e.g., ["T"])
    pub type_parameters: Vec<String>,
    /// Actual type arguments (e.g., ["number"])
    pub type_arguments: Vec<String>,
}

/// Native callback registration
#[derive(Debug, Clone, Copy)]
pub struct NativeCallbackId(pub usize);

/// Registry for native callbacks
#[derive(Debug, Default)]
pub struct NativeCallbackRegistry {
    next_id: usize,
    // In a real implementation, this would store function pointers or similar
    // For now, we just track registered IDs
    registered: Vec<usize>,
}

impl NativeCallbackRegistry {
    /// Create a new registry
    pub fn new() -> Self {
        Self {
            next_id: 0,
            registered: Vec::new(),
        }
    }

    /// Register a new native callback
    pub fn register(&mut self) -> NativeCallbackId {
        let id = self.next_id;
        self.next_id += 1;
        self.registered.push(id);
        NativeCallbackId(id)
    }

    /// Check if a callback is registered
    pub fn is_registered(&self, id: NativeCallbackId) -> bool {
        self.registered.contains(&id.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_class_builder_basic() {
        let mut builder = ClassBuilder::new("TestClass".to_string());
        assert_eq!(builder.name, "TestClass");
        assert!(!builder.finalized);
    }

    #[test]
    fn test_class_builder_add_field() {
        let mut builder = ClassBuilder::new("TestClass".to_string());
        builder.add_field("name".to_string(), "string", false, false).unwrap();
        builder.add_field("age".to_string(), "number", false, true).unwrap();

        assert_eq!(builder.fields.len(), 2);
        assert_eq!(builder.fields[0].name, "name");
        assert!(!builder.fields[0].is_readonly);
        assert_eq!(builder.fields[1].name, "age");
        assert!(builder.fields[1].is_readonly);
    }

    #[test]
    fn test_class_builder_add_method() {
        let mut builder = ClassBuilder::new("TestClass".to_string());
        builder.add_method("greet".to_string(), 42, false, false).unwrap();
        builder.add_method("compute".to_string(), 43, true, true).unwrap();

        assert_eq!(builder.methods.len(), 2);
        assert_eq!(builder.methods[0].name, "greet");
        assert!(!builder.methods[0].is_static);
        assert_eq!(builder.methods[1].name, "compute");
        assert!(builder.methods[1].is_static);
        assert!(builder.methods[1].is_async);
    }

    #[test]
    fn test_class_builder_duplicate_field() {
        let mut builder = ClassBuilder::new("TestClass".to_string());
        builder.add_field("name".to_string(), "string", false, false).unwrap();
        let result = builder.add_field("name".to_string(), "number", false, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_class_builder_finalized() {
        let mut builder = ClassBuilder::new("TestClass".to_string());
        builder.finalize();

        let result = builder.add_field("name".to_string(), "string", false, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_class_builder_set_parent() {
        let mut builder = ClassBuilder::new("ChildClass".to_string());
        builder.set_parent(5).unwrap();
        assert_eq!(builder.parent_id, Some(5));
    }

    #[test]
    fn test_class_builder_interfaces() {
        let mut builder = ClassBuilder::new("TestClass".to_string());
        builder.add_interface("Serializable".to_string()).unwrap();
        builder.add_interface("Comparable".to_string()).unwrap();
        builder.add_interface("Serializable".to_string()).unwrap(); // Duplicate

        assert_eq!(builder.interfaces.len(), 2);
    }

    #[test]
    fn test_class_builder_registry() {
        let mut registry = ClassBuilderRegistry::new();
        let id = registry.create_builder("TestClass".to_string());

        assert!(registry.contains(id));
        assert!(registry.get(id).is_some());

        let builder = registry.get_mut(id).unwrap();
        builder.add_field("x".to_string(), "number", false, false).unwrap();

        let removed = registry.remove(id);
        assert!(removed.is_some());
        assert!(!registry.contains(id));
    }

    #[test]
    fn test_dynamic_function() {
        let bytecode = vec![0x01, 0x02, 0x03];
        let func = DynamicFunction::new("test".to_string(), 2, bytecode.clone());

        assert_eq!(func.name, "test");
        assert_eq!(func.param_count, 2);
        assert_eq!(func.bytecode, bytecode);
        assert!(!func.is_async);
    }

    #[test]
    fn test_dynamic_async_function() {
        let func = DynamicFunction::new_async("asyncTest".to_string(), 1, vec![]);
        assert!(func.is_async);
    }

    #[test]
    fn test_dynamic_function_registry() {
        let mut registry = DynamicFunctionRegistry::new();
        let func = DynamicFunction::new("test".to_string(), 0, vec![]);
        let id = func.id;

        registry.register(func);
        assert!(registry.contains(id));
        assert!(registry.get(id).is_some());
    }

    #[test]
    fn test_dynamic_closure() {
        let mut closure = DynamicClosure::new(42);
        closure.capture("x".to_string(), Value::i32(10));
        closure.capture("y".to_string(), Value::i32(20));

        assert_eq!(closure.function_id, 42);
        assert_eq!(closure.captures.len(), 2);

        let values = closure.capture_values();
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn test_specialization_cache() {
        let mut cache = SpecializationCache::new();

        // Register Box<number>
        cache.register("Box", vec!["number".to_string()], 10);

        // Register Box<string>
        cache.register("Box", vec!["string".to_string()], 11);

        // Look up
        assert_eq!(cache.get("Box", &["number".to_string()]), Some(10));
        assert_eq!(cache.get("Box", &["string".to_string()]), Some(11));
        assert_eq!(cache.get("Box", &["boolean".to_string()]), None);

        // Get origin
        let origin = cache.get_origin(10).unwrap();
        assert_eq!(origin.name, "Box");
        assert_eq!(origin.type_arguments, vec!["number".to_string()]);

        // Find all specializations
        let specs = cache.find_specializations("Box");
        assert_eq!(specs.len(), 2);
        assert!(specs.contains(&10));
        assert!(specs.contains(&11));
    }

    #[test]
    fn test_to_definition() {
        let mut builder = ClassBuilder::new("TestClass".to_string());
        builder.add_field("x".to_string(), "number", false, false).unwrap();
        builder.add_method("greet".to_string(), 42, false, false).unwrap();
        builder.set_constructor(100).unwrap();
        builder.add_interface("Serializable".to_string()).unwrap();

        let def = builder.to_definition();
        assert_eq!(def.fields.len(), 1);
        assert_eq!(def.methods.len(), 1);
        assert_eq!(def.constructor_id, Some(100));
        assert_eq!(def.interfaces.len(), 1);
    }

    #[test]
    fn test_native_callback_registry() {
        let mut registry = NativeCallbackRegistry::new();

        let id1 = registry.register();
        let id2 = registry.register();

        assert!(registry.is_registered(id1));
        assert!(registry.is_registered(id2));
        assert!(!registry.is_registered(NativeCallbackId(999)));
    }
}
