//! Metadata Storage for Reflection
//!
//! Provides WeakMap-style storage for attaching metadata to objects.
//! Metadata can be attached to:
//! - Objects directly (target-level metadata)
//! - Specific properties on objects (property-level metadata)
//!
//! This implementation uses object pointer addresses as identity keys.
//! Note: This is NOT garbage-collection aware - entries persist even after
//! the target object is collected. In a full implementation, this would
//! integrate with the GC to clean up entries.

use std::collections::HashMap;

use crate::vm::Value;

/// A key for metadata - can be any string
pub type MetadataKey = String;

/// A property key - the name of a property on an object
pub type PropertyKey = String;

/// Identity key for an object - its pointer address
type TargetId = usize;

/// Metadata storage for a single target
#[derive(Debug, Default)]
struct TargetMetadata {
    /// Direct metadata on the target (key -> value)
    direct: HashMap<MetadataKey, Value>,
    /// Property-level metadata (property -> key -> value)
    properties: HashMap<PropertyKey, HashMap<MetadataKey, Value>>,
}

/// Global metadata store
///
/// Stores metadata attached to objects via `Reflect.defineMetadata`.
/// Uses object pointer addresses as identity keys for WeakMap-like behavior.
#[derive(Debug, Default)]
pub struct MetadataStore {
    /// Map from target identity to its metadata
    targets: HashMap<TargetId, TargetMetadata>,
}

impl MetadataStore {
    /// Create a new empty metadata store
    pub fn new() -> Self {
        Self {
            targets: HashMap::new(),
        }
    }

    /// Get the identity key for a target value
    ///
    /// For pointer values, uses the pointer address.
    /// For primitives, this won't work well - metadata on primitives
    /// is not meaningful since they're copied by value.
    fn target_id(target: Value) -> Option<TargetId> {
        if target.is_ptr() {
            // Use the pointer address as identity
            Some(target.raw() as usize)
        } else {
            // Primitives don't have stable identity
            // We could hash them, but it's semantically questionable
            None
        }
    }

    // ========================================================================
    // Direct (target-level) metadata operations
    // ========================================================================

    /// Define metadata on a target
    ///
    /// `Reflect.defineMetadata(key, value, target)`
    pub fn define_metadata(&mut self, key: MetadataKey, value: Value, target: Value) -> bool {
        let Some(id) = Self::target_id(target) else {
            return false;
        };

        let entry = self.targets.entry(id).or_default();
        entry.direct.insert(key, value);
        true
    }

    /// Get metadata from a target
    ///
    /// `Reflect.getMetadata(key, target)`
    pub fn get_metadata(&self, key: &str, target: Value) -> Option<Value> {
        let id = Self::target_id(target)?;
        let entry = self.targets.get(&id)?;
        entry.direct.get(key).copied()
    }

    /// Check if target has metadata
    ///
    /// `Reflect.hasMetadata(key, target)`
    pub fn has_metadata(&self, key: &str, target: Value) -> bool {
        let Some(id) = Self::target_id(target) else {
            return false;
        };
        self.targets
            .get(&id)
            .is_some_and(|e| e.direct.contains_key(key))
    }

    /// Get all metadata keys on a target
    ///
    /// `Reflect.getMetadataKeys(target)`
    pub fn get_metadata_keys(&self, target: Value) -> Vec<MetadataKey> {
        let Some(id) = Self::target_id(target) else {
            return Vec::new();
        };
        self.targets
            .get(&id)
            .map(|e| e.direct.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Delete metadata from a target
    ///
    /// `Reflect.deleteMetadata(key, target)`
    /// Returns true if the metadata existed and was deleted
    pub fn delete_metadata(&mut self, key: &str, target: Value) -> bool {
        let Some(id) = Self::target_id(target) else {
            return false;
        };
        self.targets
            .get_mut(&id)
            .is_some_and(|e| e.direct.remove(key).is_some())
    }

    // ========================================================================
    // Property-level metadata operations
    // ========================================================================

    /// Define metadata on a property of a target
    ///
    /// `Reflect.defineMetadata(key, value, target, propertyKey)`
    pub fn define_metadata_property(
        &mut self,
        key: MetadataKey,
        value: Value,
        target: Value,
        property_key: PropertyKey,
    ) -> bool {
        let Some(id) = Self::target_id(target) else {
            return false;
        };

        let entry = self.targets.entry(id).or_default();
        let prop_entry = entry.properties.entry(property_key).or_default();
        prop_entry.insert(key, value);
        true
    }

    /// Get metadata from a property of a target
    ///
    /// `Reflect.getMetadata(key, target, propertyKey)`
    pub fn get_metadata_property(
        &self,
        key: &str,
        target: Value,
        property_key: &str,
    ) -> Option<Value> {
        let id = Self::target_id(target)?;
        let entry = self.targets.get(&id)?;
        let prop_entry = entry.properties.get(property_key)?;
        prop_entry.get(key).copied()
    }

    /// Check if property has metadata
    ///
    /// `Reflect.hasMetadata(key, target, propertyKey)`
    pub fn has_metadata_property(&self, key: &str, target: Value, property_key: &str) -> bool {
        let Some(id) = Self::target_id(target) else {
            return false;
        };
        self.targets.get(&id).is_some_and(|e| {
            e.properties
                .get(property_key)
                .is_some_and(|p| p.contains_key(key))
        })
    }

    /// Get all metadata keys on a property
    ///
    /// `Reflect.getMetadataKeys(target, propertyKey)`
    pub fn get_metadata_keys_property(&self, target: Value, property_key: &str) -> Vec<MetadataKey> {
        let Some(id) = Self::target_id(target) else {
            return Vec::new();
        };
        self.targets
            .get(&id)
            .and_then(|e| e.properties.get(property_key))
            .map(|p| p.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Delete metadata from a property
    ///
    /// `Reflect.deleteMetadata(key, target, propertyKey)`
    /// Returns true if the metadata existed and was deleted
    pub fn delete_metadata_property(
        &mut self,
        key: &str,
        target: Value,
        property_key: &str,
    ) -> bool {
        let Some(id) = Self::target_id(target) else {
            return false;
        };
        self.targets.get_mut(&id).is_some_and(|e| {
            e.properties
                .get_mut(property_key)
                .is_some_and(|p| p.remove(key).is_some())
        })
    }

    /// Clear all metadata for a target (useful for testing or cleanup)
    #[allow(dead_code)]
    pub fn clear_target(&mut self, target: Value) -> bool {
        let Some(id) = Self::target_id(target) else {
            return false;
        };
        self.targets.remove(&id).is_some()
    }

    /// Get total number of targets with metadata
    #[allow(dead_code)]
    pub fn target_count(&self) -> usize {
        self.targets.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a fake pointer value for testing
    fn fake_ptr(id: usize) -> Value {
        // Create a value that looks like a pointer for identity purposes
        // NaN-boxed pointer format: 0xFFF8_0000_0000_0000 | (addr & 0x0000_FFFF_FFFF_FFFF)
        const NAN_BOX_BASE: u64 = 0xFFF8_0000_0000_0000;
        const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
        // Use a non-zero address (id + 0x1000) to avoid null-like values
        let addr = ((id + 0x1000) as u64) & PAYLOAD_MASK;
        unsafe { Value::from_raw(NAN_BOX_BASE | addr) }
    }

    #[test]
    fn test_define_and_get_metadata() {
        let mut store = MetadataStore::new();
        let target = fake_ptr(1);
        let value = Value::i32(42);

        assert!(store.define_metadata("key".to_string(), value, target));
        assert_eq!(store.get_metadata("key", target), Some(value));
        assert_eq!(store.get_metadata("nonexistent", target), None);
    }

    #[test]
    fn test_has_metadata() {
        let mut store = MetadataStore::new();
        let target = fake_ptr(2);
        let value = Value::bool(true);

        assert!(!store.has_metadata("key", target));
        store.define_metadata("key".to_string(), value, target);
        assert!(store.has_metadata("key", target));
        assert!(!store.has_metadata("other", target));
    }

    #[test]
    fn test_get_metadata_keys() {
        let mut store = MetadataStore::new();
        let target = fake_ptr(3);

        assert!(store.get_metadata_keys(target).is_empty());

        store.define_metadata("key1".to_string(), Value::null(), target);
        store.define_metadata("key2".to_string(), Value::null(), target);

        let keys = store.get_metadata_keys(target);
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"key1".to_string()));
        assert!(keys.contains(&"key2".to_string()));
    }

    #[test]
    fn test_delete_metadata() {
        let mut store = MetadataStore::new();
        let target = fake_ptr(4);
        let value = Value::i32(100);

        store.define_metadata("key".to_string(), value, target);
        assert!(store.has_metadata("key", target));

        assert!(store.delete_metadata("key", target));
        assert!(!store.has_metadata("key", target));

        // Deleting non-existent returns false
        assert!(!store.delete_metadata("key", target));
    }

    #[test]
    fn test_property_metadata() {
        let mut store = MetadataStore::new();
        let target = fake_ptr(5);
        let value = Value::i32(999);

        // Define property metadata
        assert!(store.define_metadata_property(
            "type".to_string(),
            value,
            target,
            "name".to_string()
        ));

        // Get property metadata
        assert_eq!(
            store.get_metadata_property("type", target, "name"),
            Some(value)
        );
        assert_eq!(store.get_metadata_property("type", target, "other"), None);

        // Has property metadata
        assert!(store.has_metadata_property("type", target, "name"));
        assert!(!store.has_metadata_property("type", target, "other"));
        assert!(!store.has_metadata_property("other", target, "name"));

        // Get property metadata keys
        let keys = store.get_metadata_keys_property(target, "name");
        assert_eq!(keys, vec!["type".to_string()]);

        // Delete property metadata
        assert!(store.delete_metadata_property("type", target, "name"));
        assert!(!store.has_metadata_property("type", target, "name"));
    }

    #[test]
    fn test_separate_targets() {
        let mut store = MetadataStore::new();
        let target1 = fake_ptr(10);
        let target2 = fake_ptr(20);

        store.define_metadata("key".to_string(), Value::i32(1), target1);
        store.define_metadata("key".to_string(), Value::i32(2), target2);

        assert_eq!(store.get_metadata("key", target1), Some(Value::i32(1)));
        assert_eq!(store.get_metadata("key", target2), Some(Value::i32(2)));
    }

    #[test]
    fn test_primitive_targets_rejected() {
        let mut store = MetadataStore::new();

        // Primitives should not be valid targets
        assert!(!store.define_metadata("key".to_string(), Value::i32(1), Value::i32(42)));
        assert!(!store.define_metadata("key".to_string(), Value::i32(1), Value::bool(true)));
        assert!(!store.define_metadata("key".to_string(), Value::i32(1), Value::null()));
    }
}
