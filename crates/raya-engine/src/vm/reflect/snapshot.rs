//! Object Snapshot and Diff Support
//!
//! Provides infrastructure for capturing object state snapshots and computing
//! differences between snapshots for debugging and state tracking.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::vm::object::{Array, Object, RayaString};
use crate::vm::value::Value;

/// Represents a captured value in a snapshot
#[derive(Debug, Clone)]
pub enum SnapshotValue {
    /// Null value
    Null,
    /// Boolean value
    Boolean(bool),
    /// Integer value
    Integer(i32),
    /// Float value
    Float(f64),
    /// String value (cloned)
    String(String),
    /// Reference to another object by ID
    ObjectRef(usize),
    /// Array of snapshot values
    Array(Vec<SnapshotValue>),
    /// Nested object snapshot
    Object(ObjectSnapshot),
}

impl SnapshotValue {
    /// Get a type description string
    pub fn type_name(&self) -> &'static str {
        match self {
            SnapshotValue::Null => "null",
            SnapshotValue::Boolean(_) => "boolean",
            SnapshotValue::Integer(_) => "number",
            SnapshotValue::Float(_) => "number",
            SnapshotValue::String(_) => "string",
            SnapshotValue::ObjectRef(_) => "object",
            SnapshotValue::Array(_) => "array",
            SnapshotValue::Object(_) => "object",
        }
    }

    /// Check if two values are equal
    pub fn eq(&self, other: &SnapshotValue) -> bool {
        match (self, other) {
            (SnapshotValue::Null, SnapshotValue::Null) => true,
            (SnapshotValue::Boolean(a), SnapshotValue::Boolean(b)) => a == b,
            (SnapshotValue::Integer(a), SnapshotValue::Integer(b)) => a == b,
            (SnapshotValue::Float(a), SnapshotValue::Float(b)) => {
                (a - b).abs() < f64::EPSILON || (a.is_nan() && b.is_nan())
            }
            (SnapshotValue::String(a), SnapshotValue::String(b)) => a == b,
            (SnapshotValue::ObjectRef(a), SnapshotValue::ObjectRef(b)) => a == b,
            (SnapshotValue::Array(a), SnapshotValue::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.eq(y))
            }
            (SnapshotValue::Object(a), SnapshotValue::Object(b)) => a.identity == b.identity,
            _ => false,
        }
    }
}

/// A snapshot of a field's value
#[derive(Debug, Clone)]
pub struct FieldSnapshot {
    /// Field name
    pub name: String,
    /// Captured value
    pub value: SnapshotValue,
    /// Type name of the value
    pub type_name: String,
}

/// A snapshot of an object's state at a point in time
#[derive(Debug, Clone)]
pub struct ObjectSnapshot {
    /// Class name of the object
    pub class_name: String,
    /// Unique identity (pointer address)
    pub identity: usize,
    /// Timestamp when snapshot was taken (ms since epoch)
    pub timestamp: u64,
    /// Field values by name
    pub fields: HashMap<String, FieldSnapshot>,
}

impl ObjectSnapshot {
    /// Create a new empty snapshot
    pub fn new(class_name: String, identity: usize) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        Self {
            class_name,
            identity,
            timestamp,
            fields: HashMap::new(),
        }
    }

    /// Add a field to the snapshot
    pub fn add_field(&mut self, name: String, value: SnapshotValue) {
        let type_name = value.type_name().to_string();
        self.fields.insert(
            name.clone(),
            FieldSnapshot {
                name,
                value,
                type_name,
            },
        );
    }
}

/// Represents a change between two values
#[derive(Debug, Clone)]
pub struct ValueChange {
    /// Old value
    pub old: SnapshotValue,
    /// New value
    pub new: SnapshotValue,
}

/// Result of comparing two snapshots
#[derive(Debug, Clone, Default)]
pub struct ObjectDiff {
    /// Fields that were added (present in new but not old)
    pub added: Vec<String>,
    /// Fields that were removed (present in old but not new)
    pub removed: Vec<String>,
    /// Fields that changed value
    pub changed: HashMap<String, ValueChange>,
}

impl ObjectDiff {
    /// Create a new empty diff
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if there are any differences
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }

    /// Compute diff between two snapshots
    pub fn compute(old: &ObjectSnapshot, new: &ObjectSnapshot) -> Self {
        let mut diff = ObjectDiff::new();

        // Find added and changed fields
        for (name, new_field) in &new.fields {
            if let Some(old_field) = old.fields.get(name) {
                // Field exists in both - check if changed
                if !old_field.value.eq(&new_field.value) {
                    diff.changed.insert(
                        name.clone(),
                        ValueChange {
                            old: old_field.value.clone(),
                            new: new_field.value.clone(),
                        },
                    );
                }
            } else {
                // Field only in new - it was added
                diff.added.push(name.clone());
            }
        }

        // Find removed fields
        for name in old.fields.keys() {
            if !new.fields.contains_key(name) {
                diff.removed.push(name.clone());
            }
        }

        diff
    }
}

/// Snapshot capture context
pub struct SnapshotContext {
    /// Track visited objects to handle circular references
    visited: HashMap<usize, ObjectSnapshot>,
    /// Maximum depth to traverse
    max_depth: usize,
}

impl SnapshotContext {
    /// Create a new snapshot context
    pub fn new(max_depth: usize) -> Self {
        Self {
            visited: HashMap::new(),
            max_depth,
        }
    }

    /// Capture a value as a SnapshotValue
    pub fn capture_value(&mut self, value: Value, depth: usize) -> SnapshotValue {
        if depth > self.max_depth {
            return SnapshotValue::Null;
        }

        if value.is_null() {
            return SnapshotValue::Null;
        }

        if let Some(b) = value.as_bool() {
            return SnapshotValue::Boolean(b);
        }

        if let Some(i) = value.as_i32() {
            return SnapshotValue::Integer(i);
        }

        if let Some(f) = value.as_f64() {
            return SnapshotValue::Float(f);
        }

        if !value.is_ptr() {
            return SnapshotValue::Null;
        }

        // String
        if let Some(ptr) = unsafe { value.as_ptr::<RayaString>() } {
            let s = unsafe { &*ptr.as_ptr() };
            return SnapshotValue::String(s.data.clone());
        }

        // Array
        if let Some(ptr) = unsafe { value.as_ptr::<Array>() } {
            let arr = unsafe { &*ptr.as_ptr() };
            let mut elements = Vec::with_capacity(arr.len());
            for i in 0..arr.len() {
                if let Some(elem) = arr.get(i) {
                    elements.push(self.capture_value(elem, depth + 1));
                }
            }
            return SnapshotValue::Array(elements);
        }

        // Object
        if let Some(ptr) = unsafe { value.as_ptr::<Object>() } {
            let obj = unsafe { &*ptr.as_ptr() };
            let obj_id = ptr.as_ptr() as usize;

            // Check if already visited (circular reference)
            if self.visited.contains_key(&obj_id) {
                return SnapshotValue::ObjectRef(obj_id);
            }

            // Create snapshot and mark as visited before recursing
            let mut snapshot = ObjectSnapshot::new(
                format!("Class{}", obj.class_id),
                obj_id,
            );

            // Capture fields (by index since we may not have names)
            for (i, &field_value) in obj.fields.iter().enumerate() {
                let field_name = format!("field_{}", i);
                let field_snapshot = self.capture_value(field_value, depth + 1);
                snapshot.add_field(field_name, field_snapshot);
            }

            self.visited.insert(obj_id, snapshot.clone());
            return SnapshotValue::Object(snapshot);
        }

        SnapshotValue::Null
    }

    /// Capture an object with field names from metadata
    pub fn capture_object_with_names(
        &mut self,
        value: Value,
        field_names: &[String],
        class_name: &str,
    ) -> ObjectSnapshot {
        let obj_id = if let Some(ptr) = unsafe { value.as_ptr::<Object>() } {
            ptr.as_ptr() as usize
        } else {
            0
        };

        let mut snapshot = ObjectSnapshot::new(class_name.to_string(), obj_id);

        if let Some(ptr) = unsafe { value.as_ptr::<Object>() } {
            let obj = unsafe { &*ptr.as_ptr() };

            for (i, &field_value) in obj.fields.iter().enumerate() {
                let field_name = field_names.get(i)
                    .cloned()
                    .unwrap_or_else(|| format!("field_{}", i));
                let field_snapshot = self.capture_value(field_value, 1);
                snapshot.add_field(field_name, field_snapshot);
            }
        }

        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_value_equality() {
        assert!(SnapshotValue::Null.eq(&SnapshotValue::Null));
        assert!(SnapshotValue::Boolean(true).eq(&SnapshotValue::Boolean(true)));
        assert!(!SnapshotValue::Boolean(true).eq(&SnapshotValue::Boolean(false)));
        assert!(SnapshotValue::Integer(42).eq(&SnapshotValue::Integer(42)));
        assert!(SnapshotValue::Float(3.14).eq(&SnapshotValue::Float(3.14)));
        assert!(SnapshotValue::String("hello".to_string()).eq(&SnapshotValue::String("hello".to_string())));
    }

    #[test]
    fn test_object_snapshot_creation() {
        let mut snapshot = ObjectSnapshot::new("User".to_string(), 12345);
        snapshot.add_field("name".to_string(), SnapshotValue::String("Alice".to_string()));
        snapshot.add_field("age".to_string(), SnapshotValue::Integer(30));

        assert_eq!(snapshot.class_name, "User");
        assert_eq!(snapshot.identity, 12345);
        assert_eq!(snapshot.fields.len(), 2);
        assert!(snapshot.fields.contains_key("name"));
        assert!(snapshot.fields.contains_key("age"));
    }

    #[test]
    fn test_diff_no_changes() {
        let mut old = ObjectSnapshot::new("User".to_string(), 1);
        old.add_field("name".to_string(), SnapshotValue::String("Alice".to_string()));

        let mut new = ObjectSnapshot::new("User".to_string(), 1);
        new.add_field("name".to_string(), SnapshotValue::String("Alice".to_string()));

        let diff = ObjectDiff::compute(&old, &new);
        assert!(diff.is_empty());
    }

    #[test]
    fn test_diff_field_changed() {
        let mut old = ObjectSnapshot::new("User".to_string(), 1);
        old.add_field("age".to_string(), SnapshotValue::Integer(30));

        let mut new = ObjectSnapshot::new("User".to_string(), 1);
        new.add_field("age".to_string(), SnapshotValue::Integer(31));

        let diff = ObjectDiff::compute(&old, &new);
        assert!(!diff.is_empty());
        assert!(diff.changed.contains_key("age"));
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
    }

    #[test]
    fn test_diff_field_added() {
        let mut old = ObjectSnapshot::new("User".to_string(), 1);
        old.add_field("name".to_string(), SnapshotValue::String("Alice".to_string()));

        let mut new = ObjectSnapshot::new("User".to_string(), 1);
        new.add_field("name".to_string(), SnapshotValue::String("Alice".to_string()));
        new.add_field("email".to_string(), SnapshotValue::String("alice@example.com".to_string()));

        let diff = ObjectDiff::compute(&old, &new);
        assert!(!diff.is_empty());
        assert!(diff.added.contains(&"email".to_string()));
        assert!(diff.changed.is_empty());
        assert!(diff.removed.is_empty());
    }

    #[test]
    fn test_diff_field_removed() {
        let mut old = ObjectSnapshot::new("User".to_string(), 1);
        old.add_field("name".to_string(), SnapshotValue::String("Alice".to_string()));
        old.add_field("legacy".to_string(), SnapshotValue::Boolean(true));

        let mut new = ObjectSnapshot::new("User".to_string(), 1);
        new.add_field("name".to_string(), SnapshotValue::String("Alice".to_string()));

        let diff = ObjectDiff::compute(&old, &new);
        assert!(!diff.is_empty());
        assert!(diff.removed.contains(&"legacy".to_string()));
        assert!(diff.added.is_empty());
        assert!(diff.changed.is_empty());
    }

    #[test]
    fn test_diff_multiple_changes() {
        let mut old = ObjectSnapshot::new("User".to_string(), 1);
        old.add_field("name".to_string(), SnapshotValue::String("Alice".to_string()));
        old.add_field("age".to_string(), SnapshotValue::Integer(30));
        old.add_field("old_field".to_string(), SnapshotValue::Null);

        let mut new = ObjectSnapshot::new("User".to_string(), 1);
        new.add_field("name".to_string(), SnapshotValue::String("Alice".to_string()));
        new.add_field("age".to_string(), SnapshotValue::Integer(31));
        new.add_field("new_field".to_string(), SnapshotValue::Boolean(true));

        let diff = ObjectDiff::compute(&old, &new);
        assert!(!diff.is_empty());
        assert!(diff.changed.contains_key("age"));
        assert!(diff.added.contains(&"new_field".to_string()));
        assert!(diff.removed.contains(&"old_field".to_string()));
    }

    #[test]
    fn test_snapshot_context_primitives() {
        let mut ctx = SnapshotContext::new(3);

        let null_val = ctx.capture_value(Value::null(), 0);
        assert!(matches!(null_val, SnapshotValue::Null));

        let bool_val = ctx.capture_value(Value::bool(true), 0);
        assert!(matches!(bool_val, SnapshotValue::Boolean(true)));

        let int_val = ctx.capture_value(Value::i32(42), 0);
        assert!(matches!(int_val, SnapshotValue::Integer(42)));

        let float_val = ctx.capture_value(Value::f64(3.14), 0);
        if let SnapshotValue::Float(f) = float_val {
            assert!((f - 3.14).abs() < 0.001);
        } else {
            panic!("Expected Float");
        }
    }
}
