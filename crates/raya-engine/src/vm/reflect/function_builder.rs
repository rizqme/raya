//! High-level Function Builder for Reflection API
//!
//! Provides a convenient API for creating wrapper functions at runtime.
//! This is the preferred API for method decorators, built on top of BytecodeBuilder.
//!
//! ## Usage
//!
//! ```ignore
//! // Create a wrapper with hooks
//! let wrapper = FunctionWrapper::new(original_func_id)
//!     .with_before(before_hook_id)
//!     .with_after(after_hook_id)
//!     .build(&mut registry)?;
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::vm::value::Value;
use crate::vm::VmError;

/// Global counter for wrapper IDs
static NEXT_WRAPPER_ID: AtomicUsize = AtomicUsize::new(0x9000_0000);

/// Generate a unique wrapper function ID
fn generate_wrapper_id() -> usize {
    NEXT_WRAPPER_ID.fetch_add(1, Ordering::Relaxed)
}

/// Hook type for wrapper functions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookType {
    /// Called before the original method with args
    Before,
    /// Called after the original method with result
    After,
    /// Called instead of original - receives (method, args), must call method
    Around,
    /// Called if the original method throws an error
    OnError,
}

/// A hook to be called at a specific point in wrapper execution
#[derive(Debug, Clone)]
pub struct WrapperHook {
    /// Type of hook
    pub hook_type: HookType,
    /// Function ID to call (or Value::null() if using closure)
    pub function_id: Option<usize>,
    /// Closure value (alternative to function_id)
    pub closure: Option<Value>,
}

impl WrapperHook {
    /// Create a hook with a function ID
    pub fn with_function(hook_type: HookType, function_id: usize) -> Self {
        Self {
            hook_type,
            function_id: Some(function_id),
            closure: None,
        }
    }

    /// Create a hook with a closure value
    pub fn with_closure(hook_type: HookType, closure: Value) -> Self {
        Self {
            hook_type,
            function_id: None,
            closure: Some(closure),
        }
    }
}

/// Builder for creating wrapper functions
#[derive(Debug)]
pub struct FunctionWrapper {
    /// ID of the original function to wrap
    pub original_func_id: usize,
    /// Original function's parameter count
    pub param_count: usize,
    /// Hooks to apply
    pub hooks: Vec<WrapperHook>,
    /// Name for the wrapper function
    pub name: Option<String>,
}

impl FunctionWrapper {
    /// Create a new wrapper for the given function
    pub fn new(original_func_id: usize, param_count: usize) -> Self {
        Self {
            original_func_id,
            param_count,
            hooks: Vec::new(),
            name: None,
        }
    }

    /// Set wrapper function name
    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    /// Add a before hook
    pub fn with_before(mut self, function_id: usize) -> Self {
        self.hooks.push(WrapperHook::with_function(HookType::Before, function_id));
        self
    }

    /// Add an after hook
    pub fn with_after(mut self, function_id: usize) -> Self {
        self.hooks.push(WrapperHook::with_function(HookType::After, function_id));
        self
    }

    /// Add an around hook
    pub fn with_around(mut self, function_id: usize) -> Self {
        self.hooks.push(WrapperHook::with_function(HookType::Around, function_id));
        self
    }

    /// Add an error hook
    pub fn with_on_error(mut self, function_id: usize) -> Self {
        self.hooks.push(WrapperHook::with_function(HookType::OnError, function_id));
        self
    }

    /// Add a hook with closure
    pub fn with_hook_closure(mut self, hook_type: HookType, closure: Value) -> Self {
        self.hooks.push(WrapperHook::with_closure(hook_type, closure));
        self
    }

    /// Check if this wrapper has an around hook
    pub fn has_around_hook(&self) -> bool {
        self.hooks.iter().any(|h| h.hook_type == HookType::Around)
    }

    /// Build the wrapper function and return its ID
    ///
    /// The generated wrapper has this structure:
    /// ```ignore
    /// function wrapper(...args) {
    ///     // Call before hooks with args
    ///     for hook in before_hooks { hook(args); }
    ///
    ///     // Call original (or around hook)
    ///     let result;
    ///     try {
    ///         if (around_hook) {
    ///             result = around_hook(original, args);
    ///         } else {
    ///             result = original(...args);
    ///         }
    ///     } catch (e) {
    ///         // Call error hooks
    ///         for hook in error_hooks { result = hook(e); }
    ///     }
    ///
    ///     // Call after hooks with result
    ///     for hook in after_hooks { hook(result); }
    ///
    ///     return result;
    /// }
    /// ```
    pub fn build(self) -> Result<WrapperFunction, VmError> {
        let wrapper_id = generate_wrapper_id();
        let name = self.name.unwrap_or_else(|| format!("wrapper_{}", wrapper_id));

        // For now, we create a simple wrapper structure
        // The actual bytecode generation would require more infrastructure
        // Instead, we store the wrapper metadata for runtime interpretation

        let wrapper = WrapperFunction {
            id: wrapper_id,
            name,
            original_func_id: self.original_func_id,
            param_count: self.param_count,
            before_hooks: self.hooks.iter()
                .filter(|h| h.hook_type == HookType::Before)
                .cloned()
                .collect(),
            after_hooks: self.hooks.iter()
                .filter(|h| h.hook_type == HookType::After)
                .cloned()
                .collect(),
            around_hook: self.hooks.iter()
                .find(|h| h.hook_type == HookType::Around)
                .cloned(),
            error_hooks: self.hooks.iter()
                .filter(|h| h.hook_type == HookType::OnError)
                .cloned()
                .collect(),
        };

        Ok(wrapper)
    }
}

/// A compiled wrapper function
#[derive(Debug, Clone)]
pub struct WrapperFunction {
    /// Unique wrapper ID
    pub id: usize,
    /// Wrapper function name
    pub name: String,
    /// ID of the original function being wrapped
    pub original_func_id: usize,
    /// Number of parameters
    pub param_count: usize,
    /// Before hooks (called with args before original)
    pub before_hooks: Vec<WrapperHook>,
    /// After hooks (called with result after original)
    pub after_hooks: Vec<WrapperHook>,
    /// Around hook (replaces original call, receives method + args)
    pub around_hook: Option<WrapperHook>,
    /// Error hooks (called if original throws)
    pub error_hooks: Vec<WrapperHook>,
}

impl WrapperFunction {
    /// Check if this wrapper has any hooks
    pub fn has_hooks(&self) -> bool {
        !self.before_hooks.is_empty()
            || !self.after_hooks.is_empty()
            || self.around_hook.is_some()
            || !self.error_hooks.is_empty()
    }

    /// Check if this is a pass-through wrapper (no hooks)
    pub fn is_passthrough(&self) -> bool {
        !self.has_hooks()
    }
}

/// Registry for wrapper functions
#[derive(Debug, Default)]
pub struct WrapperFunctionRegistry {
    /// Wrappers by ID
    wrappers: HashMap<usize, WrapperFunction>,
}

impl WrapperFunctionRegistry {
    /// Create a new registry
    pub fn new() -> Self {
        Self {
            wrappers: HashMap::new(),
        }
    }

    /// Register a wrapper function
    pub fn register(&mut self, wrapper: WrapperFunction) -> usize {
        let id = wrapper.id;
        self.wrappers.insert(id, wrapper);
        id
    }

    /// Get a wrapper by ID
    pub fn get(&self, id: usize) -> Option<&WrapperFunction> {
        self.wrappers.get(&id)
    }

    /// Check if a wrapper exists
    pub fn contains(&self, id: usize) -> bool {
        self.wrappers.contains_key(&id)
    }

    /// Remove a wrapper
    pub fn remove(&mut self, id: usize) -> Option<WrapperFunction> {
        self.wrappers.remove(&id)
    }

    /// Get all wrapper IDs
    pub fn wrapper_ids(&self) -> impl Iterator<Item = usize> + '_ {
        self.wrappers.keys().copied()
    }
}

/// Decorator metadata tracking
#[derive(Debug, Clone)]
pub struct DecoratorApplication {
    /// Decorator function name
    pub name: String,
    /// Arguments passed to decorator factory (if any)
    pub args: Vec<Value>,
    /// Target type: class, method, field, or parameter
    pub target_type: DecoratorTargetType,
    /// Property key for method/field/parameter decorators
    pub property_key: Option<String>,
    /// Parameter index for parameter decorators
    pub parameter_index: Option<usize>,
}

/// Type of decorator target
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoratorTargetType {
    Class,
    Method,
    Field,
    Parameter,
}

impl DecoratorTargetType {
    /// Convert to string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            DecoratorTargetType::Class => "class",
            DecoratorTargetType::Method => "method",
            DecoratorTargetType::Field => "field",
            DecoratorTargetType::Parameter => "parameter",
        }
    }
}

/// Registry for tracking decorator applications on classes
#[derive(Debug, Default)]
pub struct DecoratorRegistry {
    /// Decorators applied to classes: class_id -> Vec<DecoratorApplication>
    class_decorators: HashMap<usize, Vec<DecoratorApplication>>,
    /// Decorators applied to methods: (class_id, method_name) -> Vec<DecoratorApplication>
    method_decorators: HashMap<(usize, String), Vec<DecoratorApplication>>,
    /// Decorators applied to fields: (class_id, field_name) -> Vec<DecoratorApplication>
    field_decorators: HashMap<(usize, String), Vec<DecoratorApplication>>,
    /// Decorators applied to parameters: (class_id, method_name, param_index) -> Vec<DecoratorApplication>
    parameter_decorators: HashMap<(usize, String, usize), Vec<DecoratorApplication>>,
    /// Index: decorator_name -> Vec<class_id>
    classes_by_decorator: HashMap<String, Vec<usize>>,
}

impl DecoratorRegistry {
    /// Create a new registry
    pub fn new() -> Self {
        Self {
            class_decorators: HashMap::new(),
            method_decorators: HashMap::new(),
            field_decorators: HashMap::new(),
            parameter_decorators: HashMap::new(),
            classes_by_decorator: HashMap::new(),
        }
    }

    /// Register a decorator on a class
    pub fn register_class_decorator(&mut self, class_id: usize, decorator: DecoratorApplication) {
        // Update index
        self.classes_by_decorator
            .entry(decorator.name.clone())
            .or_default()
            .push(class_id);

        // Store decorator
        self.class_decorators
            .entry(class_id)
            .or_default()
            .push(decorator);
    }

    /// Register a decorator on a method
    pub fn register_method_decorator(
        &mut self,
        class_id: usize,
        method_name: String,
        decorator: DecoratorApplication,
    ) {
        self.method_decorators
            .entry((class_id, method_name))
            .or_default()
            .push(decorator);
    }

    /// Register a decorator on a field
    pub fn register_field_decorator(
        &mut self,
        class_id: usize,
        field_name: String,
        decorator: DecoratorApplication,
    ) {
        self.field_decorators
            .entry((class_id, field_name))
            .or_default()
            .push(decorator);
    }

    /// Register a decorator on a parameter
    pub fn register_parameter_decorator(
        &mut self,
        class_id: usize,
        method_name: String,
        param_index: usize,
        decorator: DecoratorApplication,
    ) {
        self.parameter_decorators
            .entry((class_id, method_name, param_index))
            .or_default()
            .push(decorator);
    }

    /// Get all classes with a specific decorator
    pub fn get_classes_with_decorator(&self, decorator_name: &str) -> Vec<usize> {
        self.classes_by_decorator
            .get(decorator_name)
            .cloned()
            .unwrap_or_default()
    }

    /// Get decorators on a class
    pub fn get_class_decorators(&self, class_id: usize) -> Vec<&DecoratorApplication> {
        self.class_decorators
            .get(&class_id)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Get decorators on a method
    pub fn get_method_decorators(&self, class_id: usize, method_name: &str) -> Vec<&DecoratorApplication> {
        self.method_decorators
            .get(&(class_id, method_name.to_string()))
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Get decorators on a field
    pub fn get_field_decorators(&self, class_id: usize, field_name: &str) -> Vec<&DecoratorApplication> {
        self.field_decorators
            .get(&(class_id, field_name.to_string()))
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Get decorators on a parameter
    pub fn get_parameter_decorators(
        &self,
        class_id: usize,
        method_name: &str,
        param_index: usize,
    ) -> Vec<&DecoratorApplication> {
        self.parameter_decorators
            .get(&(class_id, method_name.to_string(), param_index))
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Check if a class has a specific decorator
    pub fn class_has_decorator(&self, class_id: usize, decorator_name: &str) -> bool {
        self.class_decorators
            .get(&class_id)
            .map(|decorators| decorators.iter().any(|d| d.name == decorator_name))
            .unwrap_or(false)
    }

    /// Get all decorator names applied to a class
    pub fn get_class_decorator_names(&self, class_id: usize) -> Vec<String> {
        self.class_decorators
            .get(&class_id)
            .map(|decorators| decorators.iter().map(|d| d.name.clone()).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_wrapper_creation() {
        let wrapper = FunctionWrapper::new(42, 2)
            .with_name("test_wrapper".to_string())
            .with_before(100)
            .with_after(101)
            .build()
            .unwrap();

        assert_eq!(wrapper.original_func_id, 42);
        assert_eq!(wrapper.param_count, 2);
        assert_eq!(wrapper.name, "test_wrapper");
        assert_eq!(wrapper.before_hooks.len(), 1);
        assert_eq!(wrapper.after_hooks.len(), 1);
        assert!(wrapper.around_hook.is_none());
        assert!(wrapper.error_hooks.is_empty());
    }

    #[test]
    fn test_wrapper_with_around_hook() {
        let wrapper = FunctionWrapper::new(42, 2)
            .with_around(200)
            .build()
            .unwrap();

        assert!(wrapper.around_hook.is_some());
        assert!(wrapper.has_hooks());
    }

    #[test]
    fn test_passthrough_wrapper() {
        let wrapper = FunctionWrapper::new(42, 2)
            .build()
            .unwrap();

        assert!(!wrapper.has_hooks());
        assert!(wrapper.is_passthrough());
    }

    #[test]
    fn test_wrapper_registry() {
        let mut registry = WrapperFunctionRegistry::new();

        let wrapper = FunctionWrapper::new(42, 2)
            .with_before(100)
            .build()
            .unwrap();
        let id = wrapper.id;

        registry.register(wrapper);

        assert!(registry.contains(id));
        assert!(registry.get(id).is_some());
    }

    #[test]
    fn test_decorator_registry_class() {
        let mut registry = DecoratorRegistry::new();

        let decorator = DecoratorApplication {
            name: "Injectable".to_string(),
            args: vec![],
            target_type: DecoratorTargetType::Class,
            property_key: None,
            parameter_index: None,
        };

        registry.register_class_decorator(1, decorator.clone());
        registry.register_class_decorator(2, decorator.clone());

        let classes = registry.get_classes_with_decorator("Injectable");
        assert_eq!(classes.len(), 2);
        assert!(classes.contains(&1));
        assert!(classes.contains(&2));
    }

    #[test]
    fn test_decorator_registry_method() {
        let mut registry = DecoratorRegistry::new();

        let decorator = DecoratorApplication {
            name: "GET".to_string(),
            args: vec![],
            target_type: DecoratorTargetType::Method,
            property_key: Some("getUsers".to_string()),
            parameter_index: None,
        };

        registry.register_method_decorator(1, "getUsers".to_string(), decorator);

        let decorators = registry.get_method_decorators(1, "getUsers");
        assert_eq!(decorators.len(), 1);
        assert_eq!(decorators[0].name, "GET");
    }

    #[test]
    fn test_class_has_decorator() {
        let mut registry = DecoratorRegistry::new();

        let decorator = DecoratorApplication {
            name: "Entity".to_string(),
            args: vec![],
            target_type: DecoratorTargetType::Class,
            property_key: None,
            parameter_index: None,
        };

        registry.register_class_decorator(1, decorator);

        assert!(registry.class_has_decorator(1, "Entity"));
        assert!(!registry.class_has_decorator(1, "Controller"));
        assert!(!registry.class_has_decorator(2, "Entity"));
    }

    #[test]
    fn test_decorator_target_type_as_str() {
        assert_eq!(DecoratorTargetType::Class.as_str(), "class");
        assert_eq!(DecoratorTargetType::Method.as_str(), "method");
        assert_eq!(DecoratorTargetType::Field.as_str(), "field");
        assert_eq!(DecoratorTargetType::Parameter.as_str(), "parameter");
    }

    #[test]
    fn test_decorator_registry_field() {
        let mut registry = DecoratorRegistry::new();

        let decorator = DecoratorApplication {
            name: "Column".to_string(),
            args: vec![],
            target_type: DecoratorTargetType::Field,
            property_key: Some("name".to_string()),
            parameter_index: None,
        };

        registry.register_field_decorator(1, "name".to_string(), decorator.clone());
        registry.register_field_decorator(1, "age".to_string(), DecoratorApplication {
            name: "Column".to_string(),
            args: vec![],
            target_type: DecoratorTargetType::Field,
            property_key: Some("age".to_string()),
            parameter_index: None,
        });

        let decorators = registry.get_field_decorators(1, "name");
        assert_eq!(decorators.len(), 1);
        assert_eq!(decorators[0].name, "Column");

        let age_decorators = registry.get_field_decorators(1, "age");
        assert_eq!(age_decorators.len(), 1);
    }

    #[test]
    fn test_decorator_registry_parameter() {
        let mut registry = DecoratorRegistry::new();

        let decorator = DecoratorApplication {
            name: "Inject".to_string(),
            args: vec![],
            target_type: DecoratorTargetType::Parameter,
            property_key: Some("constructor".to_string()),
            parameter_index: Some(0),
        };

        registry.register_parameter_decorator(1, "constructor".to_string(), 0, decorator.clone());
        registry.register_parameter_decorator(1, "constructor".to_string(), 1, DecoratorApplication {
            name: "Inject".to_string(),
            args: vec![],
            target_type: DecoratorTargetType::Parameter,
            property_key: Some("constructor".to_string()),
            parameter_index: Some(1),
        });

        let param0_decorators = registry.get_parameter_decorators(1, "constructor", 0);
        assert_eq!(param0_decorators.len(), 1);
        assert_eq!(param0_decorators[0].name, "Inject");

        let param1_decorators = registry.get_parameter_decorators(1, "constructor", 1);
        assert_eq!(param1_decorators.len(), 1);
    }

    #[test]
    fn test_multiple_decorators_on_class() {
        let mut registry = DecoratorRegistry::new();

        // Register multiple decorators on same class
        registry.register_class_decorator(1, DecoratorApplication {
            name: "Injectable".to_string(),
            args: vec![],
            target_type: DecoratorTargetType::Class,
            property_key: None,
            parameter_index: None,
        });
        registry.register_class_decorator(1, DecoratorApplication {
            name: "Singleton".to_string(),
            args: vec![],
            target_type: DecoratorTargetType::Class,
            property_key: None,
            parameter_index: None,
        });

        let decorators = registry.get_class_decorators(1);
        assert_eq!(decorators.len(), 2);

        // Class should appear in both decorator queries
        assert!(registry.get_classes_with_decorator("Injectable").contains(&1));
        assert!(registry.get_classes_with_decorator("Singleton").contains(&1));
    }

    #[test]
    fn test_multiple_decorators_on_method() {
        let mut registry = DecoratorRegistry::new();

        // Register multiple decorators on same method
        registry.register_method_decorator(1, "getUsers".to_string(), DecoratorApplication {
            name: "GET".to_string(),
            args: vec![],
            target_type: DecoratorTargetType::Method,
            property_key: Some("getUsers".to_string()),
            parameter_index: None,
        });
        registry.register_method_decorator(1, "getUsers".to_string(), DecoratorApplication {
            name: "Auth".to_string(),
            args: vec![],
            target_type: DecoratorTargetType::Method,
            property_key: Some("getUsers".to_string()),
            parameter_index: None,
        });

        let decorators = registry.get_method_decorators(1, "getUsers");
        assert_eq!(decorators.len(), 2);

        let names: Vec<_> = decorators.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"GET"));
        assert!(names.contains(&"Auth"));
    }

    #[test]
    fn test_decorator_registry_empty_queries() {
        let registry = DecoratorRegistry::new();

        // Queries on empty registry should return empty
        assert!(registry.get_class_decorators(999).is_empty());
        assert!(registry.get_method_decorators(999, "nonexistent").is_empty());
        assert!(registry.get_field_decorators(999, "nonexistent").is_empty());
        assert!(registry.get_parameter_decorators(999, "nonexistent", 0).is_empty());
        assert!(registry.get_classes_with_decorator("Unknown").is_empty());
        assert!(!registry.class_has_decorator(999, "Unknown"));
    }
}
