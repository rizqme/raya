//! Class registry for managing runtime class metadata

use crate::vm::object::{register_global_layout_names, Class, LayoutId, STRUCTURAL_LAYOUT_ID_TAG};
use rustc_hash::{FxHashMap, FxHashSet};

/// Physical layout metadata for nominal runtime types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutInfo {
    pub id: LayoutId,
    pub field_count: usize,
    pub nominal_type_id: Option<usize>,
    pub name: Option<String>,
    pub field_names: Option<Vec<String>>,
    pub epoch: u32,
}

/// Dedicated runtime registry for physical object layouts.
#[derive(Debug)]
pub struct RuntimeLayoutRegistry {
    layouts: FxHashMap<LayoutId, LayoutInfo>,
    nominal_to_layout: FxHashMap<usize, LayoutId>,
    next_layout_id: LayoutId,
}

impl RuntimeLayoutRegistry {
    pub fn new() -> Self {
        Self {
            layouts: FxHashMap::default(),
            nominal_to_layout: FxHashMap::default(),
            next_layout_id: 1,
        }
    }

    pub fn allocate_nominal_layout_id(&mut self) -> LayoutId {
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

    pub fn register_nominal_layout(
        &mut self,
        nominal_type_id: usize,
        layout_id: LayoutId,
        field_count: usize,
        name: impl Into<Option<String>>,
    ) {
        if layout_id == 0 {
            return;
        }
        let name = name.into();
        self.nominal_to_layout.insert(nominal_type_id, layout_id);
        self.layouts
            .entry(layout_id)
            .and_modify(|layout| {
                layout.field_count = field_count;
                layout.nominal_type_id = Some(nominal_type_id);
                if layout.name.is_none() {
                    layout.name = name.clone();
                }
                layout.epoch = layout.epoch.wrapping_add(1);
            })
            .or_insert_with(|| LayoutInfo {
                id: layout_id,
                field_count,
                nominal_type_id: Some(nominal_type_id),
                name,
                field_names: None,
                epoch: 1,
            });
    }

    pub fn get_layout(&self, layout_id: LayoutId) -> Option<&LayoutInfo> {
        self.layouts.get(&layout_id)
    }

    pub fn register_layout_shape(&mut self, layout_id: LayoutId, field_names: &[String]) {
        if layout_id == 0 {
            return;
        }
        let field_count = field_names.len();
        let owned_names = field_names.to_vec();
        register_global_layout_names(layout_id, &owned_names);
        self.layouts
            .entry(layout_id)
            .and_modify(|layout| {
                layout.field_count = layout.field_count.max(field_count);
                if layout.field_names.is_none() {
                    layout.field_names = Some(owned_names.clone());
                }
                layout.epoch = layout.epoch.wrapping_add(1);
            })
            .or_insert_with(|| LayoutInfo {
                id: layout_id,
                field_count,
                nominal_type_id: None,
                name: Some(format!("Layout{}", layout_id)),
                field_names: Some(owned_names),
                epoch: 1,
            });
    }

    pub fn layout_field_names(&self, layout_id: LayoutId) -> Option<&[String]> {
        self.layouts
            .get(&layout_id)
            .and_then(|layout| layout.field_names.as_deref())
    }

    pub fn nominal_layout_id(&self, nominal_type_id: usize) -> Option<LayoutId> {
        self.nominal_to_layout.get(&nominal_type_id).copied()
    }

    pub fn nominal_allocation(&self, nominal_type_id: usize) -> Option<(LayoutId, usize)> {
        let layout_id = *self.nominal_to_layout.get(&nominal_type_id)?;
        let layout = self.layouts.get(&layout_id)?;
        Some((layout.id, layout.field_count))
    }

    pub fn layout_epoch(&self, layout_id: LayoutId) -> Option<u32> {
        self.layouts.get(&layout_id).map(|layout| layout.epoch)
    }

    pub fn set_nominal_field_count(&mut self, nominal_type_id: usize, field_count: usize) -> bool {
        let Some(layout_id) = self.nominal_to_layout.get(&nominal_type_id).copied() else {
            return false;
        };
        let Some(layout) = self.layouts.get_mut(&layout_id) else {
            return false;
        };
        layout.field_count = field_count;
        layout.epoch = layout.epoch.wrapping_add(1);
        true
    }
}

impl Default for RuntimeLayoutRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Nominal type registry for the VM.
#[derive(Debug)]
pub struct ClassRegistry {
    /// Nominal classes indexed by runtime type ID.
    classes: Vec<Option<Class>>,
    /// Class name to nominal type ID mapping.
    name_to_id: FxHashMap<String, usize>,
    /// Nominal IDs reserved internally but not yet populated with class metadata.
    reserved_ids: FxHashSet<usize>,
    /// Next internally allocated nominal type ID.
    next_id: usize,
}

impl ClassRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            classes: Vec::new(),
            name_to_id: FxHashMap::default(),
            reserved_ids: FxHashSet::default(),
            next_id: 1,
        }
    }

    /// Allocate one fresh nominal runtime type ID.
    pub fn allocate_nominal_type_id(&mut self) -> usize {
        self.reserve_nominal_type_range(1)
    }

    /// Reserve a contiguous nominal runtime type range and return its base ID.
    pub fn reserve_nominal_type_range(&mut self, len: usize) -> usize {
        let base = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(len)
            .expect("nominal type id overflow");
        for id in base..base + len {
            self.reserved_ids.insert(id);
        }
        base
    }

    /// Register a new class definition.
    pub fn register_class(&mut self, mut class: Class) -> usize {
        if class.id == 0 {
            class.id = self.allocate_nominal_type_id();
            assert!(
                self.reserved_ids.remove(&class.id),
                "fresh nominal type id must be reserved"
            );
        } else if !self.reserved_ids.remove(&class.id) {
            panic!(
                "nominal type id {} was not reserved by ClassRegistry; callers must use registry-allocated IDs",
                class.id
            );
        }
        let id = class.id;
        let name = class.name.clone();

        if id >= self.classes.len() {
            self.classes.resize_with(id + 1, || None);
        }

        if let Some(existing) = self.classes[id].as_ref() {
            self.name_to_id.remove(&existing.name);
        }

        self.classes[id] = Some(class);
        self.name_to_id.insert(name, id);
        self.next_id = self.next_id.max(id.saturating_add(1));

        id
    }

    /// Get class by nominal type ID.
    pub fn get_class(&self, id: usize) -> Option<&Class> {
        self.classes.get(id).and_then(|class| class.as_ref())
    }

    /// Get mutable class by nominal type ID.
    pub fn get_class_mut(&mut self, id: usize) -> Option<&mut Class> {
        self.classes.get_mut(id).and_then(|class| class.as_mut())
    }

    /// Get class by name
    pub fn get_class_by_name(&self, name: &str) -> Option<&Class> {
        self.name_to_id.get(name).and_then(|id| self.get_class(*id))
    }

    /// Get the next available nominal type ID.
    pub fn next_nominal_type_id(&self) -> usize {
        self.next_id
    }

    /// Get class by nominal type ID (alias for `get_class`).
    pub fn get(&self, id: usize) -> Option<&Class> {
        self.get_class(id)
    }

    /// Iterate over all classes with their IDs
    pub fn iter(&self) -> impl Iterator<Item = (usize, &Class)> {
        self.classes
            .iter()
            .enumerate()
            .filter_map(|(id, class)| class.as_ref().map(|class| (id, class)))
    }

    /// Update the instance field count for a nominal runtime type.
    pub fn set_nominal_field_count(&mut self, nominal_type_id: usize, field_count: usize) -> bool {
        let Some(class) = self.get_class_mut(nominal_type_id) else {
            return false;
        };
        class.field_count = field_count;
        true
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

    fn class_with_id(id: usize, name: &str, field_count: usize) -> Class {
        Class::new(id, name.to_string(), field_count)
    }

    fn class_with_parent(id: usize, name: &str, field_count: usize, parent_id: usize) -> Class {
        Class::with_parent(id, name.to_string(), field_count, parent_id)
    }

    fn register_auto_class(registry: &mut ClassRegistry, name: &str, field_count: usize) -> usize {
        registry.register_class(class_with_id(0, name, field_count))
    }

    #[test]
    fn test_register_class() {
        let mut registry = ClassRegistry::new();
        let id = register_auto_class(&mut registry, "Point", 2);
        assert_eq!(id, 1);
    }

    #[test]
    fn test_get_class_by_id() {
        let mut registry = ClassRegistry::new();
        let id = register_auto_class(&mut registry, "Point", 2);

        let retrieved = registry.get_class(id).unwrap();
        assert_eq!(retrieved.name, "Point");
        assert_eq!(retrieved.field_count, 2);
    }

    #[test]
    fn test_get_class_by_name() {
        let mut registry = ClassRegistry::new();
        let id = register_auto_class(&mut registry, "Point", 2);

        let retrieved = registry.get_class_by_name("Point").unwrap();
        assert_eq!(retrieved.id, id);
        assert_eq!(retrieved.field_count, 2);
    }

    #[test]
    fn test_multiple_classes() {
        let mut registry = ClassRegistry::new();

        let id1 = register_auto_class(&mut registry, "Point", 2);
        let id2 = register_auto_class(&mut registry, "Circle", 3);

        assert_eq!(registry.get_class(id1).unwrap().name, "Point");
        assert_eq!(registry.get_class(id2).unwrap().name, "Circle");
        assert_eq!(registry.next_nominal_type_id(), 3);
    }

    #[test]
    fn test_next_nominal_type_id() {
        let mut registry = ClassRegistry::new();
        assert_eq!(registry.next_nominal_type_id(), 1);

        register_auto_class(&mut registry, "Point", 2);
        assert_eq!(registry.next_nominal_type_id(), 2);
    }

    #[test]
    fn test_nominal_layout_lookup_tracks_registered_nominal_layout() {
        let mut registry = RuntimeLayoutRegistry::new();
        registry.register_nominal_layout(42, 7, 2, Some("Point".to_string()));

        assert_eq!(registry.nominal_layout_id(42), Some(7));
        assert_eq!(registry.nominal_allocation(42), Some((7, 2)));
    }

    #[test]
    fn test_register_class_auto_assigns_nominal_type_id() {
        let mut registry = ClassRegistry::new();
        let id = registry.register_class(class_with_id(0, "Point", 2));

        assert_eq!(id, 1);
        assert_eq!(registry.get_class(id).map(|class| class.id), Some(1));
        assert_eq!(registry.next_nominal_type_id(), 2);
    }

    #[test]
    fn test_register_class_no_longer_requires_layout_id() {
        let mut registry = ClassRegistry::new();
        let id = registry.register_class(Class::new(0, "Point".to_string(), 2));
        assert_eq!(id, 1);
    }

    #[test]
    fn test_reserve_nominal_type_range_advances_allocator() {
        let mut registry = ClassRegistry::new();
        let base = registry.reserve_nominal_type_range(3);

        assert_eq!(base, 1);
        assert_eq!(registry.next_nominal_type_id(), 4);

        let id = registry.register_class(class_with_id(2, "Reserved", 1));
        assert_eq!(id, 2);
        assert_eq!(registry.next_nominal_type_id(), 4);
    }

    #[test]
    fn test_register_layout_shape_for_structural_layout() {
        let mut registry = RuntimeLayoutRegistry::new();
        let layout_id = STRUCTURAL_LAYOUT_ID_TAG | 42;
        let names = vec!["a".to_string(), "b".to_string()];

        registry.register_layout_shape(layout_id, &names);

        let layout = registry.get_layout(layout_id).expect("layout");
        assert_eq!(layout.nominal_type_id, None);
        assert_eq!(layout.field_count, 2);
        assert_eq!(layout.field_names.as_deref(), Some(names.as_slice()));
        assert_eq!(
            registry.layout_field_names(layout_id),
            Some(names.as_slice())
        );
    }

    #[test]
    fn test_runtime_layout_registry_allocates_nominal_layout_ids() {
        let mut registry = RuntimeLayoutRegistry::new();
        let first = registry.allocate_nominal_layout_id();
        let second = registry.allocate_nominal_layout_id();
        assert_eq!(first, 1);
        assert_eq!(second, 2);
        assert!(second < STRUCTURAL_LAYOUT_ID_TAG);
    }

    #[test]
    fn test_runtime_layout_registry_tracks_nominal_allocation() {
        let mut registry = RuntimeLayoutRegistry::new();
        registry.register_nominal_layout(4, 9, 3, Some("Point".to_string()));
        assert_eq!(registry.nominal_allocation(4), Some((9, 3)));
        assert!(registry.set_nominal_field_count(4, 6));
        assert_eq!(registry.nominal_allocation(4), Some((9, 6)));
        let layout = registry.get_layout(9).expect("layout");
        assert_eq!(layout.nominal_type_id, Some(4));
    }
}
