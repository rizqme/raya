//! Class registry for managing runtime class metadata

use crate::vm::object::Class;
use rustc_hash::FxHashMap;

/// Class registry for the VM
#[derive(Debug)]
pub struct ClassRegistry {
    /// Classes indexed by ID
    classes: Vec<Class>,
    /// Class name to ID mapping
    name_to_id: FxHashMap<String, usize>,
}

impl ClassRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            classes: Vec::new(),
            name_to_id: FxHashMap::default(),
        }
    }

    /// Register a new class
    pub fn register_class(&mut self, class: Class) -> usize {
        let id = class.id;
        let name = class.name.clone();

        self.classes.push(class);
        self.name_to_id.insert(name, id);

        id
    }

    /// Get class by ID
    pub fn get_class(&self, id: usize) -> Option<&Class> {
        self.classes.get(id)
    }

    /// Get mutable class by ID
    pub fn get_class_mut(&mut self, id: usize) -> Option<&mut Class> {
        self.classes.get_mut(id)
    }

    /// Get class by name
    pub fn get_class_by_name(&self, name: &str) -> Option<&Class> {
        self.name_to_id
            .get(name)
            .and_then(|id| self.classes.get(*id))
    }

    /// Get next available class ID
    pub fn next_class_id(&self) -> usize {
        self.classes.len()
    }

    /// Get class by ID (alias for get_class)
    pub fn get(&self, id: usize) -> Option<&Class> {
        self.classes.get(id)
    }

    /// Iterate over all classes with their IDs
    pub fn iter(&self) -> impl Iterator<Item = (usize, &Class)> {
        self.classes.iter().enumerate()
    }
}

impl Default for ClassRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::object::Class;

    #[test]
    fn test_register_class() {
        let mut registry = ClassRegistry::new();
        let class = Class::new(0, "Point".to_string(), 2);

        let id = registry.register_class(class);
        assert_eq!(id, 0);
    }

    #[test]
    fn test_get_class_by_id() {
        let mut registry = ClassRegistry::new();
        let class = Class::new(0, "Point".to_string(), 2);
        registry.register_class(class);

        let retrieved = registry.get_class(0).unwrap();
        assert_eq!(retrieved.name, "Point");
        assert_eq!(retrieved.field_count, 2);
    }

    #[test]
    fn test_get_class_by_name() {
        let mut registry = ClassRegistry::new();
        let class = Class::new(0, "Point".to_string(), 2);
        registry.register_class(class);

        let retrieved = registry.get_class_by_name("Point").unwrap();
        assert_eq!(retrieved.id, 0);
        assert_eq!(retrieved.field_count, 2);
    }

    #[test]
    fn test_multiple_classes() {
        let mut registry = ClassRegistry::new();

        let class1 = Class::new(0, "Point".to_string(), 2);
        let class2 = Class::new(1, "Circle".to_string(), 3);

        registry.register_class(class1);
        registry.register_class(class2);

        assert_eq!(registry.get_class(0).unwrap().name, "Point");
        assert_eq!(registry.get_class(1).unwrap().name, "Circle");
        assert_eq!(registry.next_class_id(), 2);
    }

    #[test]
    fn test_next_class_id() {
        let mut registry = ClassRegistry::new();
        assert_eq!(registry.next_class_id(), 0);

        let class = Class::new(0, "Point".to_string(), 2);
        registry.register_class(class);
        assert_eq!(registry.next_class_id(), 1);
    }
}
