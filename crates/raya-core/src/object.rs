//! Object model and class system

use crate::value::Value;

/// Object instance (heap-allocated)
#[derive(Debug, Clone)]
pub struct Object {
    /// Class ID (index into VM class registry)
    pub class_id: usize,
    /// Field values
    pub fields: Vec<Value>,
}

impl Object {
    /// Create a new object with uninitialized fields
    pub fn new(class_id: usize, field_count: usize) -> Self {
        Self {
            class_id,
            fields: vec![Value::null(); field_count],
        }
    }

    /// Get a field value by index
    pub fn get_field(&self, index: usize) -> Option<Value> {
        self.fields.get(index).copied()
    }

    /// Set a field value by index
    pub fn set_field(&mut self, index: usize, value: Value) -> Result<(), String> {
        if index < self.fields.len() {
            self.fields[index] = value;
            Ok(())
        } else {
            Err(format!(
                "Field index {} out of bounds (object has {} fields)",
                index,
                self.fields.len()
            ))
        }
    }

    /// Get number of fields
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }
}

/// Class definition metadata
#[derive(Debug, Clone)]
pub struct Class {
    /// Class ID (unique identifier)
    pub id: usize,
    /// Class name
    pub name: String,
    /// Number of fields (including inherited)
    pub field_count: usize,
    /// Parent class ID (None for root classes)
    pub parent_id: Option<usize>,
    /// Virtual method table
    pub vtable: VTable,
}

impl Class {
    /// Create a new class
    pub fn new(id: usize, name: String, field_count: usize) -> Self {
        Self {
            id,
            name,
            field_count,
            parent_id: None,
            vtable: VTable::new(),
        }
    }

    /// Create a new class with parent
    pub fn with_parent(id: usize, name: String, field_count: usize, parent_id: usize) -> Self {
        Self {
            id,
            name,
            field_count,
            parent_id: Some(parent_id),
            vtable: VTable::new(),
        }
    }

    /// Add a method to the vtable
    pub fn add_method(&mut self, function_id: usize) {
        self.vtable.add_method(function_id);
    }

    /// Get method from vtable
    pub fn get_method(&self, method_index: usize) -> Option<usize> {
        self.vtable.get_method(method_index)
    }
}

/// Virtual method table for dynamic dispatch
#[derive(Debug, Clone)]
pub struct VTable {
    /// Method function IDs (indexed by method slot)
    pub methods: Vec<usize>,
}

impl VTable {
    /// Create a new empty vtable
    pub fn new() -> Self {
        Self {
            methods: Vec::new(),
        }
    }

    /// Add a method to the vtable (appends to end)
    pub fn add_method(&mut self, function_id: usize) {
        self.methods.push(function_id);
    }

    /// Get method function ID by index
    pub fn get_method(&self, index: usize) -> Option<usize> {
        self.methods.get(index).copied()
    }

    /// Get number of methods
    pub fn method_count(&self) -> usize {
        self.methods.len()
    }

    /// Override a method at specific index
    pub fn override_method(&mut self, index: usize, function_id: usize) -> Result<(), String> {
        if index < self.methods.len() {
            self.methods[index] = function_id;
            Ok(())
        } else {
            Err(format!("Method index {} out of bounds", index))
        }
    }
}

impl Default for VTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Array object (heap-allocated)
#[derive(Debug, Clone)]
pub struct Array {
    /// Element type ID (for type checking)
    pub type_id: usize,
    /// Array elements
    pub elements: Vec<Value>,
}

impl Array {
    /// Create a new array with given length
    pub fn new(type_id: usize, length: usize) -> Self {
        Self {
            type_id,
            elements: vec![Value::null(); length],
        }
    }

    /// Get array length
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Check if array is empty
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Get element at index
    pub fn get(&self, index: usize) -> Option<Value> {
        self.elements.get(index).copied()
    }

    /// Set element at index
    pub fn set(&mut self, index: usize, value: Value) -> Result<(), String> {
        if index < self.elements.len() {
            self.elements[index] = value;
            Ok(())
        } else {
            Err(format!(
                "Array index {} out of bounds (length: {})",
                index,
                self.elements.len()
            ))
        }
    }
}

/// String object (heap-allocated)
#[derive(Debug, Clone)]
pub struct RayaString {
    /// UTF-8 string data
    pub data: String,
}

impl RayaString {
    /// Create a new string
    pub fn new(data: String) -> Self {
        Self { data }
    }

    /// Get string length (in bytes)
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if string is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Concatenate two strings
    pub fn concat(&self, other: &RayaString) -> RayaString {
        RayaString::new(format!("{}{}", self.data, other.data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_creation() {
        let obj = Object::new(0, 3);
        assert_eq!(obj.field_count(), 3);
        assert_eq!(obj.class_id, 0);
    }

    #[test]
    fn test_object_field_access() {
        let mut obj = Object::new(0, 2);
        let value = Value::i32(42);

        obj.set_field(0, value).unwrap();
        assert_eq!(obj.get_field(0).unwrap(), value);

        obj.set_field(1, Value::bool(true)).unwrap();
        assert_eq!(obj.get_field(1).unwrap(), Value::bool(true));
    }

    #[test]
    fn test_object_field_bounds() {
        let mut obj = Object::new(0, 2);
        assert!(obj.set_field(2, Value::null()).is_err());
        assert_eq!(obj.get_field(10), None);
    }

    #[test]
    fn test_class_creation() {
        let class = Class::new(0, "Point".to_string(), 2);
        assert_eq!(class.id, 0);
        assert_eq!(class.name, "Point");
        assert_eq!(class.field_count, 2);
        assert_eq!(class.parent_id, None);
    }

    #[test]
    fn test_class_with_parent() {
        let class = Class::with_parent(1, "ColoredPoint".to_string(), 3, 0);
        assert_eq!(class.parent_id, Some(0));
        assert_eq!(class.field_count, 3);
    }

    #[test]
    fn test_vtable() {
        let mut vtable = VTable::new();
        vtable.add_method(10); // function ID 10
        vtable.add_method(20); // function ID 20

        assert_eq!(vtable.method_count(), 2);
        assert_eq!(vtable.get_method(0), Some(10));
        assert_eq!(vtable.get_method(1), Some(20));
        assert_eq!(vtable.get_method(2), None);
    }

    #[test]
    fn test_vtable_override() {
        let mut vtable = VTable::new();
        vtable.add_method(10);
        vtable.add_method(20);

        vtable.override_method(0, 30).unwrap();
        assert_eq!(vtable.get_method(0), Some(30));
    }

    #[test]
    fn test_array_creation() {
        let arr = Array::new(0, 5);
        assert_eq!(arr.len(), 5);
        assert_eq!(arr.type_id, 0);
    }

    #[test]
    fn test_array_access() {
        let mut arr = Array::new(0, 3);

        arr.set(0, Value::i32(10)).unwrap();
        arr.set(1, Value::i32(20)).unwrap();
        arr.set(2, Value::i32(30)).unwrap();

        assert_eq!(arr.get(0), Some(Value::i32(10)));
        assert_eq!(arr.get(1), Some(Value::i32(20)));
        assert_eq!(arr.get(2), Some(Value::i32(30)));
    }

    #[test]
    fn test_array_bounds() {
        let mut arr = Array::new(0, 2);
        assert!(arr.set(2, Value::null()).is_err());
        assert_eq!(arr.get(5), None);
    }

    #[test]
    fn test_string_creation() {
        let s = RayaString::new("hello".to_string());
        assert_eq!(s.len(), 5);
        assert_eq!(s.data, "hello");
    }

    #[test]
    fn test_string_concat() {
        let s1 = RayaString::new("hello".to_string());
        let s2 = RayaString::new(" world".to_string());
        let s3 = s1.concat(&s2);

        assert_eq!(s3.data, "hello world");
    }
}
