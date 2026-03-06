//! Class registry for managing runtime class metadata

use crate::vm::object::{Class, LayoutId, STRUCTURAL_LAYOUT_ID_TAG};
use rustc_hash::FxHashMap;

/// Physical layout metadata for nominal runtime types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutInfo {
    pub id: LayoutId,
    pub field_count: usize,
    pub nominal_type_id: Option<usize>,
    pub name: Option<String>,
}

/// Class registry for the VM
#[derive(Debug)]
pub struct ClassRegistry {
    /// Classes indexed by ID
    classes: Vec<Option<Class>>,
    /// Class name to ID mapping
    name_to_id: FxHashMap<String, usize>,
    /// Physical layout metadata indexed by layout ID.
    layouts: FxHashMap<LayoutId, LayoutInfo>,
    /// Next internally allocated nominal class ID.
    next_id: usize,
    /// Next internally allocated nominal-object layout ID.
    next_layout_id: LayoutId,
}

impl ClassRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            classes: Vec::new(),
            name_to_id: FxHashMap::default(),
            layouts: FxHashMap::default(),
            next_id: 1,
            next_layout_id: 1,
        }
    }

    fn allocate_layout_id(&mut self) -> LayoutId {
        if self.next_layout_id >= STRUCTURAL_LAYOUT_ID_TAG {
            panic!("exhausted nominal layout id space");
        }
        let layout_id = self.next_layout_id;
        self.next_layout_id = self
            .next_layout_id
            .checked_add(1)
            .expect("nominal layout id overflow");
        layout_id
    }

    /// Register a new class
    pub fn register_class(&mut self, mut class: Class) -> usize {
        if class.layout_id == 0 {
            class.set_layout_id(self.allocate_layout_id());
        }
        let id = class.id;
        let name = class.name.clone();

        if id >= self.classes.len() {
            self.classes.resize_with(id + 1, || None);
        }

        if let Some(existing) = self.classes[id].as_ref() {
            self.name_to_id.remove(&existing.name);
            self.layouts.remove(&existing.layout_id);
        }

        self.layouts.insert(
            class.layout_id,
            LayoutInfo {
                id: class.layout_id,
                field_count: class.field_count,
                nominal_type_id: Some(id),
                name: Some(name.clone()),
            },
        );
        self.classes[id] = Some(class);
        self.name_to_id.insert(name, id);
        self.next_id = self.next_id.max(id.saturating_add(1));

        id
    }

    /// Get class by ID
    pub fn get_class(&self, id: usize) -> Option<&Class> {
        self.classes.get(id).and_then(|class| class.as_ref())
    }

    /// Get mutable class by ID
    pub fn get_class_mut(&mut self, id: usize) -> Option<&mut Class> {
        self.classes.get_mut(id).and_then(|class| class.as_mut())
    }

    /// Get class by name
    pub fn get_class_by_name(&self, name: &str) -> Option<&Class> {
        self.name_to_id.get(name).and_then(|id| self.get_class(*id))
    }

    /// Get next available class ID
    pub fn next_class_id(&self) -> usize {
        self.next_id
    }

    /// Get class by ID (alias for get_class)
    pub fn get(&self, id: usize) -> Option<&Class> {
        self.get_class(id)
    }

    /// Get physical layout metadata by layout ID.
    pub fn get_layout(&self, layout_id: LayoutId) -> Option<&LayoutInfo> {
        self.layouts.get(&layout_id)
    }

    /// Iterate over all classes with their IDs
    pub fn iter(&self) -> impl Iterator<Item = (usize, &Class)> {
        self.classes
            .iter()
            .enumerate()
            .filter_map(|(id, class)| class.as_ref().map(|class| (id, class)))
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
    use crate::vm::object::{Class, STRUCTURAL_LAYOUT_ID_TAG};

    #[test]
    fn test_register_class() {
        let mut registry = ClassRegistry::new();
        let class = Class::new(1, "Point".to_string(), 2);

        let id = registry.register_class(class);
        assert_eq!(id, 1);
    }

    #[test]
    fn test_get_class_by_id() {
        let mut registry = ClassRegistry::new();
        let class = Class::new(1, "Point".to_string(), 2);
        registry.register_class(class);

        let retrieved = registry.get_class(1).unwrap();
        assert_eq!(retrieved.name, "Point");
        assert_eq!(retrieved.field_count, 2);
    }

    #[test]
    fn test_get_class_by_name() {
        let mut registry = ClassRegistry::new();
        let class = Class::new(1, "Point".to_string(), 2);
        registry.register_class(class);

        let retrieved = registry.get_class_by_name("Point").unwrap();
        assert_eq!(retrieved.id, 1);
        assert_eq!(retrieved.field_count, 2);
    }

    #[test]
    fn test_multiple_classes() {
        let mut registry = ClassRegistry::new();

        let class1 = Class::new(1, "Point".to_string(), 2);
        let class2 = Class::new(2, "Circle".to_string(), 3);

        registry.register_class(class1);
        registry.register_class(class2);

        assert_eq!(registry.get_class(1).unwrap().name, "Point");
        assert_eq!(registry.get_class(2).unwrap().name, "Circle");
        assert_eq!(registry.next_class_id(), 3);
    }

    #[test]
    fn test_next_class_id() {
        let mut registry = ClassRegistry::new();
        assert_eq!(registry.next_class_id(), 1);

        let class = Class::new(1, "Point".to_string(), 2);
        registry.register_class(class);
        assert_eq!(registry.next_class_id(), 2);
    }

    #[test]
    fn test_register_class_assigns_independent_nominal_layout_id() {
        let mut registry = ClassRegistry::new();
        registry.register_class(Class::new(42, "Point".to_string(), 2));

        let class = registry.get_class(42).expect("registered class");
        assert_ne!(class.layout_id, 0);
        assert!(class.layout_id < STRUCTURAL_LAYOUT_ID_TAG);
        assert_ne!(class.layout_id as usize, class.id);
        let layout = registry
            .get_layout(class.layout_id)
            .expect("registered layout info");
        assert_eq!(layout.field_count, 2);
        assert_eq!(layout.nominal_type_id, Some(42));
        assert_eq!(layout.name.as_deref(), Some("Point"));
    }
}
