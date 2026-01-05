//! Type registry for runtime type information
//!
//! The type registry maps TypeId to TypeInfo, providing metadata
//! for precise garbage collection.

use super::pointer_map::PointerMap;
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::Arc;

/// Drop function type
pub type DropFn = fn(*mut u8);

/// Runtime type information for GC
#[derive(Clone)]
pub struct TypeInfo {
    /// Type ID
    pub type_id: TypeId,

    /// Type name (for debugging)
    pub name: &'static str,

    /// Size of the type in bytes
    pub size: usize,

    /// Alignment requirement
    pub align: usize,

    /// Pointer map describing pointer locations
    pub pointer_map: PointerMap,

    /// Optional drop function
    pub drop_fn: Option<DropFn>,
}

impl TypeInfo {
    /// Create new type information
    pub fn new<T: 'static>(name: &'static str, pointer_map: PointerMap) -> Self {
        Self {
            type_id: TypeId::of::<T>(),
            name,
            size: std::mem::size_of::<T>(),
            align: std::mem::align_of::<T>(),
            pointer_map,
            drop_fn: None,
        }
    }

    /// Create type info with drop function
    pub fn with_drop<T: 'static>(
        name: &'static str,
        pointer_map: PointerMap,
        drop_fn: DropFn,
    ) -> Self {
        Self {
            type_id: TypeId::of::<T>(),
            name,
            size: std::mem::size_of::<T>(),
            align: std::mem::align_of::<T>(),
            pointer_map,
            drop_fn: Some(drop_fn),
        }
    }

    /// Check if this type has any pointers
    pub fn has_pointers(&self) -> bool {
        self.pointer_map.has_pointers()
    }

    /// Iterate over pointer offsets in an object of this type
    pub fn for_each_pointer<F>(&self, base_ptr: *mut u8, mut f: F)
    where
        F: FnMut(*mut u8),
    {
        self.pointer_map.for_each_pointer_offset(0, |offset| {
            let ptr = unsafe { base_ptr.add(offset) };
            f(ptr);
        });
    }
}

impl std::fmt::Debug for TypeInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypeInfo")
            .field("name", &self.name)
            .field("size", &self.size)
            .field("align", &self.align)
            .field("pointer_map", &self.pointer_map)
            .field("has_drop", &self.drop_fn.is_some())
            .finish()
    }
}

/// Registry of type information for GC
///
/// Thread-safe registry that maps TypeId to TypeInfo.
#[derive(Clone, Debug)]
pub struct TypeRegistry {
    types: Arc<HashMap<TypeId, TypeInfo>>,
}

impl TypeRegistry {
    /// Create a new type registry
    pub fn new() -> Self {
        Self {
            types: Arc::new(HashMap::new()),
        }
    }

    /// Create a registry builder
    pub fn builder() -> TypeRegistryBuilder {
        TypeRegistryBuilder {
            types: HashMap::new(),
        }
    }

    /// Get type information by TypeId
    pub fn get(&self, type_id: TypeId) -> Option<&TypeInfo> {
        self.types.get(&type_id)
    }

    /// Check if a type is registered
    pub fn contains(&self, type_id: TypeId) -> bool {
        self.types.contains_key(&type_id)
    }

    /// Get type information, panicking if not found
    pub fn get_or_panic(&self, type_id: TypeId) -> &TypeInfo {
        self.types
            .get(&type_id)
            .unwrap_or_else(|| panic!("Type {:?} not registered", type_id))
    }

    /// Iterate over pointer locations in an object
    pub fn for_each_pointer<F>(&self, base_ptr: *mut u8, type_id: TypeId, f: F)
    where
        F: FnMut(*mut u8),
    {
        if let Some(type_info) = self.get(type_id) {
            type_info.for_each_pointer(base_ptr, f);
        }
    }

    /// Get the number of registered types
    pub fn len(&self) -> usize {
        self.types.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }
}

impl Default for TypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for TypeRegistry
pub struct TypeRegistryBuilder {
    types: HashMap<TypeId, TypeInfo>,
}

impl TypeRegistryBuilder {
    /// Register a type
    pub fn register<T: 'static>(mut self, name: &'static str, pointer_map: PointerMap) -> Self {
        let type_info = TypeInfo::new::<T>(name, pointer_map);
        self.types.insert(type_info.type_id, type_info);
        self
    }

    /// Register a type with drop function
    pub fn register_with_drop<T: 'static>(
        mut self,
        name: &'static str,
        pointer_map: PointerMap,
        drop_fn: DropFn,
    ) -> Self {
        let type_info = TypeInfo::with_drop::<T>(name, pointer_map, drop_fn);
        self.types.insert(type_info.type_id, type_info);
        self
    }

    /// Build the registry
    pub fn build(self) -> TypeRegistry {
        TypeRegistry {
            types: Arc::new(self.types),
        }
    }
}

/// Create a standard type registry with built-in types
pub fn create_standard_registry() -> TypeRegistry {
    use crate::object::{Array, Object, RayaString};

    TypeRegistry::builder()
        // Primitives (no pointers)
        .register::<i32>("i32", PointerMap::none())
        .register::<i64>("i64", PointerMap::none())
        .register::<f32>("f32", PointerMap::none())
        .register::<f64>("f64", PointerMap::none())
        .register::<bool>("bool", PointerMap::none())
        .register::<()>("()", PointerMap::none())
        // String (no pointers in data, UTF-8 bytes)
        .register::<String>("String", PointerMap::none())
        // Raya objects (special handling in GC for dynamic fields)
        .register::<Object>("Object", PointerMap::none())
        .register::<Array>("Array", PointerMap::none())
        .register::<RayaString>("RayaString", PointerMap::none())
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_info_creation() {
        let info = TypeInfo::new::<i32>("i32", PointerMap::none());
        assert_eq!(info.name, "i32");
        assert_eq!(info.size, 4);
        assert!(!info.has_pointers());
    }

    #[test]
    fn test_type_registry_builder() {
        let registry = TypeRegistry::builder()
            .register::<i32>("i32", PointerMap::none())
            .register::<String>("String", PointerMap::none())
            .build();

        assert_eq!(registry.len(), 2);
        assert!(registry.contains(TypeId::of::<i32>()));
        assert!(registry.contains(TypeId::of::<String>()));
    }

    #[test]
    fn test_type_registry_get() {
        let registry = TypeRegistry::builder()
            .register::<i32>("i32", PointerMap::none())
            .build();

        let info = registry.get(TypeId::of::<i32>()).unwrap();
        assert_eq!(info.name, "i32");
    }

    #[test]
    fn test_standard_registry() {
        let registry = create_standard_registry();
        assert!(!registry.is_empty());
        assert!(registry.contains(TypeId::of::<i32>()));
        assert!(registry.contains(TypeId::of::<String>()));
    }

    #[test]
    fn test_type_info_with_pointers() {
        struct HasPointers {
            _a: i32,
            _ptr: *mut u8,
            _b: i32,
        }

        // Pointer is at offset 8 (after i32 + padding)
        let info = TypeInfo::new::<HasPointers>("HasPointers", PointerMap::offsets(vec![8]));

        assert!(info.has_pointers());
        assert_eq!(info.pointer_map.pointer_count(), 1);
    }
}
