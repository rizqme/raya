//! Class Metadata for Reflection
//!
//! Stores optional reflection metadata for classes including field names,
//! method names, and type information. This metadata is populated when
//! the compiler is invoked with `--emit-reflection`.

use rustc_hash::FxHashMap;

use super::{FieldInfo, MethodInfo, ConstructorInfo, TypeInfo};

/// Reflection metadata for a single class
#[derive(Debug, Clone, Default)]
pub struct ClassMetadata {
    /// Field name to index mapping
    pub field_indices: FxHashMap<String, usize>,
    /// Field names in order (by index)
    pub field_names: Vec<String>,
    /// Detailed field information (if available)
    pub fields: Vec<FieldInfo>,

    /// Method name to vtable index mapping
    pub method_indices: FxHashMap<String, usize>,
    /// Method names in order (by vtable index)
    pub method_names: Vec<String>,
    /// Detailed method information (if available)
    pub methods: Vec<MethodInfo>,

    /// Static field name to index mapping
    pub static_field_indices: FxHashMap<String, usize>,
    /// Static field names in order
    pub static_field_names: Vec<String>,

    /// Constructor info (if available)
    pub constructor: Option<ConstructorInfo>,

    /// Interfaces implemented by this class
    pub interfaces: Vec<String>,
}

impl ClassMetadata {
    /// Create new empty metadata
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a field with name
    pub fn add_field(&mut self, name: String, index: usize) {
        self.field_indices.insert(name.clone(), index);
        // Ensure field_names has enough space
        while self.field_names.len() <= index {
            self.field_names.push(String::new());
        }
        self.field_names[index] = name;
    }

    /// Add a method with name
    pub fn add_method(&mut self, name: String, vtable_index: usize) {
        self.method_indices.insert(name.clone(), vtable_index);
        while self.method_names.len() <= vtable_index {
            self.method_names.push(String::new());
        }
        self.method_names[vtable_index] = name;
    }

    /// Add a static field with name
    pub fn add_static_field(&mut self, name: String, index: usize) {
        self.static_field_indices.insert(name.clone(), index);
        while self.static_field_names.len() <= index {
            self.static_field_names.push(String::new());
        }
        self.static_field_names[index] = name;
    }

    /// Add detailed field information
    pub fn add_field_info(&mut self, field_info: FieldInfo) {
        let index = field_info.field_index;
        self.field_indices.insert(field_info.name.clone(), index);
        while self.field_names.len() <= index {
            self.field_names.push(String::new());
        }
        self.field_names[index] = field_info.name.clone();
        while self.fields.len() <= index {
            self.fields.push(FieldInfo {
                name: String::new(),
                type_info: TypeInfo::primitive("unknown"),
                declaring_class_id: 0,
                field_index: self.fields.len(),
                is_static: false,
                is_readonly: false,
            });
        }
        self.fields[index] = field_info;
    }

    /// Add detailed method information
    pub fn add_method_info(&mut self, method_info: MethodInfo) {
        let index = method_info.method_index;
        self.method_indices.insert(method_info.name.clone(), index);
        while self.method_names.len() <= index {
            self.method_names.push(String::new());
        }
        self.method_names[index] = method_info.name.clone();
        while self.methods.len() <= index {
            self.methods.push(MethodInfo {
                name: String::new(),
                return_type: TypeInfo::primitive("unknown"),
                parameters: Vec::new(),
                declaring_class_id: 0,
                method_index: self.methods.len(),
                is_static: false,
                is_async: false,
            });
        }
        self.methods[index] = method_info;
    }

    /// Get method info by name
    pub fn get_method_info(&self, name: &str) -> Option<&MethodInfo> {
        self.method_indices.get(name).and_then(|&idx| self.methods.get(idx))
    }

    /// Get all method infos
    pub fn get_all_method_infos(&self) -> &[MethodInfo] {
        &self.methods
    }

    /// Get field info by name
    pub fn get_field_info(&self, name: &str) -> Option<&FieldInfo> {
        self.field_indices.get(name).and_then(|&idx| self.fields.get(idx))
    }

    /// Get all field infos
    pub fn get_all_field_infos(&self) -> &[FieldInfo] {
        &self.fields
    }

    /// Get field index by name
    pub fn get_field_index(&self, name: &str) -> Option<usize> {
        self.field_indices.get(name).copied()
    }

    /// Get method vtable index by name
    pub fn get_method_index(&self, name: &str) -> Option<usize> {
        self.method_indices.get(name).copied()
    }

    /// Get static field index by name
    pub fn get_static_field_index(&self, name: &str) -> Option<usize> {
        self.static_field_indices.get(name).copied()
    }

    /// Check if field exists
    pub fn has_field(&self, name: &str) -> bool {
        self.field_indices.contains_key(name)
    }

    /// Check if method exists
    pub fn has_method(&self, name: &str) -> bool {
        self.method_indices.contains_key(name)
    }

    /// Add an interface implementation
    pub fn add_interface(&mut self, interface_name: String) {
        if !self.interfaces.contains(&interface_name) {
            self.interfaces.push(interface_name);
        }
    }

    /// Check if class implements an interface
    pub fn implements_interface(&self, interface_name: &str) -> bool {
        self.interfaces.iter().any(|i| i == interface_name)
    }

    /// Get all implemented interfaces
    pub fn get_interfaces(&self) -> &[String] {
        &self.interfaces
    }
}

/// Registry of class metadata for reflection
#[derive(Debug, Default)]
pub struct ClassMetadataRegistry {
    /// Metadata indexed by class ID
    metadata: FxHashMap<usize, ClassMetadata>,
}

impl ClassMetadataRegistry {
    /// Create new empty registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Register metadata for a class
    pub fn register(&mut self, class_id: usize, metadata: ClassMetadata) {
        self.metadata.insert(class_id, metadata);
    }

    /// Get metadata for a class
    pub fn get(&self, class_id: usize) -> Option<&ClassMetadata> {
        self.metadata.get(&class_id)
    }

    /// Get mutable metadata for a class
    pub fn get_mut(&mut self, class_id: usize) -> Option<&mut ClassMetadata> {
        self.metadata.get_mut(&class_id)
    }

    /// Get or create metadata for a class
    pub fn get_or_create(&mut self, class_id: usize) -> &mut ClassMetadata {
        self.metadata.entry(class_id).or_default()
    }

    /// Check if a class has metadata
    pub fn has_metadata(&self, class_id: usize) -> bool {
        self.metadata.contains_key(&class_id)
    }

    /// Get number of classes with metadata
    pub fn len(&self) -> usize {
        self.metadata.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.metadata.is_empty()
    }

    /// Get all class IDs that implement a given interface
    pub fn get_implementors(&self, interface_name: &str) -> Vec<usize> {
        self.metadata
            .iter()
            .filter(|(_, meta)| meta.implements_interface(interface_name))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Iterate over all class metadata
    pub fn iter(&self) -> impl Iterator<Item = (usize, &ClassMetadata)> {
        self.metadata.iter().map(|(id, meta)| (*id, meta))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::reflect::introspection::ParameterInfo;

    #[test]
    fn test_class_metadata_fields() {
        let mut meta = ClassMetadata::new();
        meta.add_field("name".to_string(), 0);
        meta.add_field("age".to_string(), 1);

        assert_eq!(meta.get_field_index("name"), Some(0));
        assert_eq!(meta.get_field_index("age"), Some(1));
        assert_eq!(meta.get_field_index("unknown"), None);
        assert!(meta.has_field("name"));
        assert!(!meta.has_field("unknown"));
    }

    #[test]
    fn test_class_metadata_methods() {
        let mut meta = ClassMetadata::new();
        meta.add_method("greet".to_string(), 0);
        meta.add_method("compute".to_string(), 1);

        assert_eq!(meta.get_method_index("greet"), Some(0));
        assert_eq!(meta.get_method_index("compute"), Some(1));
        assert!(meta.has_method("greet"));
        assert!(!meta.has_method("unknown"));
    }

    #[test]
    fn test_class_metadata_registry() {
        let mut registry = ClassMetadataRegistry::new();

        let mut meta = ClassMetadata::new();
        meta.add_field("x".to_string(), 0);
        meta.add_field("y".to_string(), 1);

        registry.register(0, meta);

        assert!(registry.has_metadata(0));
        assert!(!registry.has_metadata(1));

        let retrieved = registry.get(0).unwrap();
        assert_eq!(retrieved.get_field_index("x"), Some(0));
    }

    #[test]
    fn test_get_or_create() {
        let mut registry = ClassMetadataRegistry::new();

        let meta = registry.get_or_create(5);
        meta.add_field("test".to_string(), 0);

        assert!(registry.has_metadata(5));
        assert_eq!(registry.get(5).unwrap().get_field_index("test"), Some(0));
    }

    #[test]
    fn test_add_field_info() {
        let mut meta = ClassMetadata::new();

        let field = FieldInfo {
            name: "age".to_string(),
            type_info: TypeInfo::primitive("number"),
            declaring_class_id: 0,
            field_index: 0,
            is_static: false,
            is_readonly: false,
        };
        meta.add_field_info(field);

        let field2 = FieldInfo {
            name: "name".to_string(),
            type_info: TypeInfo::primitive("string"),
            declaring_class_id: 0,
            field_index: 1,
            is_static: false,
            is_readonly: true,
        };
        meta.add_field_info(field2);

        // Check field indices
        assert_eq!(meta.get_field_index("age"), Some(0));
        assert_eq!(meta.get_field_index("name"), Some(1));

        // Check field info retrieval
        let age_info = meta.get_field_info("age").unwrap();
        assert_eq!(age_info.name, "age");
        assert_eq!(age_info.type_info.name, "number");
        assert!(!age_info.is_readonly);

        let name_info = meta.get_field_info("name").unwrap();
        assert_eq!(name_info.name, "name");
        assert_eq!(name_info.type_info.name, "string");
        assert!(name_info.is_readonly);

        // Check get all
        let all = meta.get_all_field_infos();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_static_fields() {
        let mut meta = ClassMetadata::new();

        meta.add_static_field("CONSTANT".to_string(), 0);
        meta.add_static_field("DEFAULT_VALUE".to_string(), 1);

        assert_eq!(meta.get_static_field_index("CONSTANT"), Some(0));
        assert_eq!(meta.get_static_field_index("DEFAULT_VALUE"), Some(1));
        assert_eq!(meta.get_static_field_index("unknown"), None);

        assert_eq!(meta.static_field_names.len(), 2);
        assert_eq!(meta.static_field_names[0], "CONSTANT");
        assert_eq!(meta.static_field_names[1], "DEFAULT_VALUE");
    }

    #[test]
    fn test_field_info_not_found() {
        let meta = ClassMetadata::new();
        assert!(meta.get_field_info("nonexistent").is_none());
    }

    #[test]
    fn test_add_method_info() {
        let mut meta = ClassMetadata::new();

        let method = MethodInfo {
            name: "greet".to_string(),
            return_type: TypeInfo::primitive("string"),
            parameters: vec![
                ParameterInfo {
                    name: "name".to_string(),
                    type_info: TypeInfo::primitive("string"),
                    index: 0,
                    is_optional: false,
                },
            ],
            declaring_class_id: 0,
            method_index: 0,
            is_static: false,
            is_async: false,
        };
        meta.add_method_info(method);

        let method2 = MethodInfo {
            name: "computeAsync".to_string(),
            return_type: TypeInfo::primitive("number"),
            parameters: vec![],
            declaring_class_id: 0,
            method_index: 1,
            is_static: false,
            is_async: true,
        };
        meta.add_method_info(method2);

        // Check method indices
        assert_eq!(meta.get_method_index("greet"), Some(0));
        assert_eq!(meta.get_method_index("computeAsync"), Some(1));

        // Check method info retrieval
        let greet_info = meta.get_method_info("greet").unwrap();
        assert_eq!(greet_info.name, "greet");
        assert_eq!(greet_info.return_type.name, "string");
        assert_eq!(greet_info.parameters.len(), 1);
        assert!(!greet_info.is_async);

        let compute_info = meta.get_method_info("computeAsync").unwrap();
        assert_eq!(compute_info.name, "computeAsync");
        assert!(compute_info.is_async);

        // Check get all
        let all = meta.get_all_method_infos();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_method_info_not_found() {
        let meta = ClassMetadata::new();
        assert!(meta.get_method_info("nonexistent").is_none());
    }

    #[test]
    fn test_static_method_info() {
        let mut meta = ClassMetadata::new();

        let static_method = MethodInfo {
            name: "create".to_string(),
            return_type: TypeInfo::class("MyClass", 0),
            parameters: vec![],
            declaring_class_id: 0,
            method_index: 0,
            is_static: true,
            is_async: false,
        };
        meta.add_method_info(static_method);

        let method_info = meta.get_method_info("create").unwrap();
        assert!(method_info.is_static);
        assert_eq!(method_info.return_type.name, "MyClass");
    }

    #[test]
    fn test_constructor_info() {
        use super::ConstructorInfo;

        let mut meta = ClassMetadata::new();

        // Set constructor info
        meta.constructor = Some(ConstructorInfo {
            parameters: vec![
                ParameterInfo {
                    name: "name".to_string(),
                    type_info: TypeInfo::primitive("string"),
                    index: 0,
                    is_optional: false,
                },
                ParameterInfo {
                    name: "age".to_string(),
                    type_info: TypeInfo::primitive("number"),
                    index: 1,
                    is_optional: true,
                },
            ],
            declaring_class_id: 5,
            function_id: 42,
        });

        let ctor = meta.constructor.as_ref().unwrap();
        assert_eq!(ctor.parameters.len(), 2);
        assert_eq!(ctor.declaring_class_id, 5);
        assert_eq!(ctor.function_id, 42);
        assert_eq!(ctor.parameters[0].name, "name");
        assert_eq!(ctor.parameters[1].name, "age");
        assert!(ctor.parameters[1].is_optional);
    }

    #[test]
    fn test_constructor_info_none() {
        let meta = ClassMetadata::new();
        assert!(meta.constructor.is_none());
    }

    #[test]
    fn test_add_interface() {
        let mut meta = ClassMetadata::new();

        meta.add_interface("Serializable".to_string());
        meta.add_interface("Comparable".to_string());

        assert!(meta.implements_interface("Serializable"));
        assert!(meta.implements_interface("Comparable"));
        assert!(!meta.implements_interface("Unknown"));

        let interfaces = meta.get_interfaces();
        assert_eq!(interfaces.len(), 2);
        assert!(interfaces.contains(&"Serializable".to_string()));
        assert!(interfaces.contains(&"Comparable".to_string()));
    }

    #[test]
    fn test_add_interface_no_duplicates() {
        let mut meta = ClassMetadata::new();

        meta.add_interface("Serializable".to_string());
        meta.add_interface("Serializable".to_string()); // duplicate

        let interfaces = meta.get_interfaces();
        assert_eq!(interfaces.len(), 1);
    }

    #[test]
    fn test_empty_interfaces() {
        let meta = ClassMetadata::new();

        assert!(!meta.implements_interface("Any"));
        assert!(meta.get_interfaces().is_empty());
    }

    #[test]
    fn test_registry_get_implementors() {
        let mut registry = ClassMetadataRegistry::new();

        // Create metadata for class 0 implementing Serializable
        let mut meta0 = ClassMetadata::new();
        meta0.add_interface("Serializable".to_string());
        registry.register(0, meta0);

        // Create metadata for class 1 implementing Serializable and Comparable
        let mut meta1 = ClassMetadata::new();
        meta1.add_interface("Serializable".to_string());
        meta1.add_interface("Comparable".to_string());
        registry.register(1, meta1);

        // Create metadata for class 2 with no interfaces
        let meta2 = ClassMetadata::new();
        registry.register(2, meta2);

        // Get implementors of Serializable
        let serializable_impls = registry.get_implementors("Serializable");
        assert_eq!(serializable_impls.len(), 2);
        assert!(serializable_impls.contains(&0));
        assert!(serializable_impls.contains(&1));

        // Get implementors of Comparable
        let comparable_impls = registry.get_implementors("Comparable");
        assert_eq!(comparable_impls.len(), 1);
        assert!(comparable_impls.contains(&1));

        // Get implementors of unknown interface
        let unknown_impls = registry.get_implementors("Unknown");
        assert!(unknown_impls.is_empty());
    }

    #[test]
    fn test_registry_iter() {
        let mut registry = ClassMetadataRegistry::new();

        let mut meta0 = ClassMetadata::new();
        meta0.add_field("name".to_string(), 0);
        registry.register(0, meta0);

        let mut meta1 = ClassMetadata::new();
        meta1.add_field("age".to_string(), 0);
        registry.register(1, meta1);

        let entries: Vec<_> = registry.iter().collect();
        assert_eq!(entries.len(), 2);
    }
}
