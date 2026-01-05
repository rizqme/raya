//! Object model and class system

use crate::value::Value;

/// Object instance
#[derive(Debug, Clone)]
pub struct Object {
    /// Class ID
    pub class_id: usize,
    /// Field values
    pub fields: Vec<Value>,
}

impl Object {
    /// Create a new object
    pub fn new(class_id: usize, field_count: usize) -> Self {
        Self {
            class_id,
            fields: vec![Value::null(); field_count],
        }
    }

    /// Get a field value
    pub fn get_field(&self, index: usize) -> Option<&Value> {
        self.fields.get(index)
    }

    /// Set a field value
    pub fn set_field(&mut self, index: usize, value: Value) -> bool {
        if index < self.fields.len() {
            self.fields[index] = value;
            true
        } else {
            false
        }
    }
}

/// Class definition metadata
#[derive(Debug, Clone)]
pub struct Class {
    /// Class name
    pub name: String,
    /// Number of fields
    pub field_count: usize,
    /// Method vtable
    pub vtable: VTable,
}

/// Virtual method table
#[derive(Debug, Clone)]
pub struct VTable {
    /// Method function IDs
    pub methods: Vec<usize>,
}

impl VTable {
    /// Create a new empty vtable
    pub fn new() -> Self {
        Self {
            methods: Vec::new(),
        }
    }

    /// Add a method to the vtable
    pub fn add_method(&mut self, function_id: usize) {
        self.methods.push(function_id);
    }
}

impl Default for VTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_creation() {
        let obj = Object::new(0, 2);
        assert_eq!(obj.fields.len(), 2);
    }

    #[test]
    fn test_object_fields() {
        let mut obj = Object::new(0, 2);
        let value = Value::i32(42);
        assert!(obj.set_field(0, value));
        assert_eq!(*obj.get_field(0).unwrap(), value);
    }
}
