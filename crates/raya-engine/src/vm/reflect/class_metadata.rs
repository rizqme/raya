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
        self.metadata.entry(class_id).or_insert_with(ClassMetadata::new)
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
