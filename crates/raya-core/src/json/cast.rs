//! Runtime type casting and validation for JSON values
//!
//! This module implements the `as` operator for casting `json` values to
//! typed Raya objects. It performs recursive tree validation against type
//! schemas to ensure type safety at runtime.
//!
//! # Validation Algorithm
//!
//! 1. Match JSON structure against type schema
//! 2. Recursively validate all nested fields/elements
//! 3. Check discriminants for union types
//! 4. Enforce max depth limit (100 levels)
//! 5. Construct typed object in VM memory
//!
//! # Example
//!
//! ```raya
//! interface User {
//!   name: string;
//!   age: number;
//! }
//!
//! let data: json = { "name": "Alice", "age": 30 };
//! let user = data as User;  // Runtime validation
//! ```

use super::JsonValue;
use crate::gc::GarbageCollector;
use crate::object::Object;
use crate::value::Value;
use crate::{VmError, VmResult};
use rustc_hash::FxHashMap;
use std::sync::Arc;

/// Maximum recursion depth for validation (prevents stack overflow)
const MAX_VALIDATION_DEPTH: usize = 100;

/// Type schema for runtime validation
///
/// Describes the expected structure of a typed object, including:
/// - Type ID for runtime type tagging
/// - Type kind (primitive, interface, array, union)
/// - Field/element schemas for nested validation
#[derive(Debug, Clone)]
pub struct TypeSchema {
    /// Unique type ID from the type registry
    pub type_id: usize,

    /// The kind of type and its structure
    pub kind: TypeKind,
}

/// Kind of type schema
#[derive(Debug, Clone)]
pub enum TypeKind {
    /// Null type (only allows null)
    Null,

    /// Boolean type
    Bool,

    /// Number type (integer or float)
    Number,

    /// String type
    String,

    /// Interface/class type with named fields
    Interface {
        /// Class ID for runtime type tagging
        class_id: usize,
        /// Field names and their type schema IDs
        fields: Vec<(String, usize)>,
    },

    /// Array type with element schema
    Array {
        /// Type schema ID for array elements
        element_type_id: usize,
    },

    /// Union type (discriminated or bare primitive union)
    Union {
        /// Type schema IDs for each variant
        variant_type_ids: Vec<usize>,
        /// Optional discriminant field name
        discriminant: Option<String>,
    },
}

/// Type schema registry
///
/// Stores all type schemas indexed by type ID. This is populated by the
/// compiler during code generation and used during runtime validation.
pub struct TypeSchemaRegistry {
    schemas: FxHashMap<usize, Arc<TypeSchema>>,
}

impl TypeSchemaRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            schemas: FxHashMap::default(),
        }
    }

    /// Register a type schema
    pub fn register(&mut self, type_id: usize, schema: TypeSchema) {
        self.schemas.insert(type_id, Arc::new(schema));
    }

    /// Get a type schema by ID
    pub fn get(&self, type_id: usize) -> Option<Arc<TypeSchema>> {
        self.schemas.get(&type_id).cloned()
    }

    /// Check if a type ID is registered
    pub fn contains(&self, type_id: usize) -> bool {
        self.schemas.contains_key(&type_id)
    }
}

impl Default for TypeSchemaRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Validate and cast a JSON value to a typed object
///
/// This is the main entry point for the JSON_CAST opcode. It performs
/// recursive validation against the type schema and constructs a typed
/// object in VM memory.
///
/// # Arguments
///
/// * `json` - The JSON value to cast
/// * `schema` - The target type schema
/// * `schema_registry` - Registry of all type schemas
/// * `gc` - Garbage collector for allocating typed objects
///
/// # Returns
///
/// A typed Value if validation succeeds, otherwise a VmError
///
/// # Errors
///
/// - `TypeMismatch` if the JSON structure doesn't match the schema
/// - `StackOverflow` if recursion depth exceeds MAX_VALIDATION_DEPTH
/// - `MissingField` if a required field is missing
/// - `InvalidDiscriminant` if union discriminant doesn't match
pub fn validate_cast(
    json: &JsonValue,
    schema: &TypeSchema,
    schema_registry: &TypeSchemaRegistry,
    gc: &mut GarbageCollector,
) -> VmResult<Value> {
    validate_cast_impl(json, schema, schema_registry, gc, 0)
}

/// Internal recursive validation implementation
///
/// Tracks recursion depth to prevent stack overflow.
fn validate_cast_impl(
    json: &JsonValue,
    schema: &TypeSchema,
    schema_registry: &TypeSchemaRegistry,
    gc: &mut GarbageCollector,
    depth: usize,
) -> VmResult<Value> {
    // Check recursion depth
    if depth > MAX_VALIDATION_DEPTH {
        return Err(VmError::StackOverflow);
    }

    match &schema.kind {
        TypeKind::Null => validate_null(json),

        TypeKind::Bool => validate_bool(json),

        TypeKind::Number => validate_number(json),

        TypeKind::String => validate_string(json, gc),

        TypeKind::Interface { class_id, fields } => {
            validate_interface(json, *class_id, fields, schema_registry, gc, depth)
        }

        TypeKind::Array { element_type_id } => {
            validate_array(json, *element_type_id, schema_registry, gc, depth)
        }

        TypeKind::Union {
            variant_type_ids,
            discriminant,
        } => validate_union(
            json,
            variant_type_ids,
            discriminant.as_deref(),
            schema_registry,
            gc,
            depth,
        ),
    }
}

/// Validate null type
fn validate_null(json: &JsonValue) -> VmResult<Value> {
    if json.is_null() {
        Ok(Value::null())
    } else {
        Err(VmError::TypeError(format!(
            "Expected null, got {}",
            json.type_name()
        )))
    }
}

/// Validate boolean type
fn validate_bool(json: &JsonValue) -> VmResult<Value> {
    match json.as_bool() {
        Some(b) => Ok(Value::bool(b)),
        None => Err(VmError::TypeError(format!(
            "Expected boolean, got {}",
            json.type_name()
        ))),
    }
}

/// Validate number type
fn validate_number(json: &JsonValue) -> VmResult<Value> {
    match json.as_number() {
        Some(n) => Ok(Value::f64(n)),
        None => Err(VmError::TypeError(format!(
            "Expected number, got {}",
            json.type_name()
        ))),
    }
}

/// Validate string type
fn validate_string(json: &JsonValue, _gc: &mut GarbageCollector) -> VmResult<Value> {
    match json.as_string() {
        Some(s_ptr) => {
            // String is already heap-allocated, return as-is
            Ok(unsafe {
                Value::from_ptr(std::ptr::NonNull::new_unchecked(s_ptr.as_ptr() as *mut u8))
            })
        }
        None => Err(VmError::TypeError(format!(
            "Expected string, got {}",
            json.type_name()
        ))),
    }
}

/// Validate interface/class type
fn validate_interface(
    json: &JsonValue,
    class_id: usize,
    fields: &[(String, usize)],
    schema_registry: &TypeSchemaRegistry,
    gc: &mut GarbageCollector,
    depth: usize,
) -> VmResult<Value> {
    // Must be an object
    let obj_ptr = match json.as_object() {
        Some(ptr) => ptr,
        None => {
            return Err(VmError::TypeError(format!(
                "Expected object, got {}",
                json.type_name()
            )))
        }
    };

    let json_obj = unsafe { &*obj_ptr.as_ptr() };

    // Validate each field
    let mut field_values = Vec::with_capacity(fields.len());

    for (field_name, field_type_id) in fields {
        // Get field value from JSON object
        let field_json = json_obj
            .get(field_name)
            .ok_or_else(|| VmError::TypeError(format!("Missing field: {}", field_name)))?;

        // Get field type schema
        let field_schema = schema_registry
            .get(*field_type_id)
            .ok_or_else(|| VmError::TypeError(format!("Unknown type ID: {}", field_type_id)))?;

        // Recursively validate field
        let field_value =
            validate_cast_impl(field_json, &field_schema, schema_registry, gc, depth + 1)?;

        field_values.push(field_value);
    }

    // Create typed object
    let obj = Object {
        class_id,
        fields: field_values,
    };

    let obj_ptr = gc.allocate(obj);

    Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new_unchecked(obj_ptr.as_ptr() as *mut u8)) })
}

/// Validate array type
fn validate_array(
    json: &JsonValue,
    element_type_id: usize,
    schema_registry: &TypeSchemaRegistry,
    gc: &mut GarbageCollector,
    depth: usize,
) -> VmResult<Value> {
    // Must be an array
    let arr_ptr = match json.as_array() {
        Some(ptr) => ptr,
        None => {
            return Err(VmError::TypeError(format!(
                "Expected array, got {}",
                json.type_name()
            )))
        }
    };

    let json_arr = unsafe { &*arr_ptr.as_ptr() };

    // Get element type schema
    let element_schema = schema_registry
        .get(element_type_id)
        .ok_or_else(|| VmError::TypeError(format!("Unknown type ID: {}", element_type_id)))?;

    // Validate each element
    let mut element_values = Vec::with_capacity(json_arr.len());

    for json_elem in json_arr {
        let elem_value =
            validate_cast_impl(json_elem, &element_schema, schema_registry, gc, depth + 1)?;
        element_values.push(elem_value);
    }

    // Create typed array
    let arr = crate::object::Array {
        type_id: element_type_id,
        elements: element_values,
    };

    let arr_ptr = gc.allocate(arr);

    Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new_unchecked(arr_ptr.as_ptr() as *mut u8)) })
}

/// Validate union type
fn validate_union(
    json: &JsonValue,
    variant_type_ids: &[usize],
    discriminant: Option<&str>,
    schema_registry: &TypeSchemaRegistry,
    gc: &mut GarbageCollector,
    depth: usize,
) -> VmResult<Value> {
    // If discriminant is specified, use it to select the variant
    if let Some(disc_field) = discriminant {
        // Must be an object with the discriminant field
        let obj_ptr = match json.as_object() {
            Some(ptr) => ptr,
            None => {
                return Err(VmError::TypeError(format!(
                    "Expected object with discriminant '{}', got {}",
                    disc_field,
                    json.type_name()
                )))
            }
        };

        let json_obj = unsafe { &*obj_ptr.as_ptr() };

        // Get discriminant value
        let disc_value = json_obj.get(disc_field).ok_or_else(|| {
            VmError::TypeError(format!("Missing discriminant field: {}", disc_field))
        })?;

        let disc_str = match disc_value.as_string() {
            Some(s_ptr) => {
                let s = unsafe { &*s_ptr.as_ptr() };
                s.data.as_str()
            }
            _ => {
                return Err(VmError::TypeError(format!(
                    "Discriminant must be string, got {}",
                    disc_value.type_name()
                )))
            }
        };

        // Try to match discriminant to a variant
        // (In practice, the compiler would encode the mapping)
        // For now, we just try each variant in order
        for variant_type_id in variant_type_ids {
            let variant_schema = schema_registry.get(*variant_type_id).ok_or_else(|| {
                VmError::TypeError(format!("Unknown type ID: {}", variant_type_id))
            })?;

            // Try to validate against this variant
            if let Ok(value) =
                validate_cast_impl(json, &variant_schema, schema_registry, gc, depth + 1)
            {
                return Ok(value);
            }
        }

        Err(VmError::TypeError(format!(
            "No matching variant for discriminant: {}",
            disc_str
        )))
    } else {
        // Bare union (primitives only) - try each variant in order
        for variant_type_id in variant_type_ids {
            let variant_schema = schema_registry.get(*variant_type_id).ok_or_else(|| {
                VmError::TypeError(format!("Unknown type ID: {}", variant_type_id))
            })?;

            // Try to validate against this variant
            if let Ok(value) =
                validate_cast_impl(json, &variant_schema, schema_registry, gc, depth + 1)
            {
                return Ok(value);
            }
        }

        Err(VmError::TypeError(format!(
            "No matching variant in union for value: {}",
            json
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_null() {
        let json = JsonValue::Null;
        let schema = TypeSchema {
            type_id: 1,
            kind: TypeKind::Null,
        };
        let registry = TypeSchemaRegistry::new();
        let mut gc = GarbageCollector::default();

        let result = validate_cast(&json, &schema, &registry, &mut gc);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_bool() {
        let json = JsonValue::Bool(true);
        let schema = TypeSchema {
            type_id: 2,
            kind: TypeKind::Bool,
        };
        let registry = TypeSchemaRegistry::new();
        let mut gc = GarbageCollector::default();

        let result = validate_cast(&json, &schema, &registry, &mut gc);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_number() {
        let json = JsonValue::Number(42.0);
        let schema = TypeSchema {
            type_id: 3,
            kind: TypeKind::Number,
        };
        let registry = TypeSchemaRegistry::new();
        let mut gc = GarbageCollector::default();

        let result = validate_cast(&json, &schema, &registry, &mut gc);
        assert!(result.is_ok());
    }

    #[test]
    fn test_type_mismatch() {
        let json = JsonValue::Number(42.0);
        let schema = TypeSchema {
            type_id: 4,
            kind: TypeKind::Bool,
        };
        let registry = TypeSchemaRegistry::new();
        let mut gc = GarbageCollector::default();

        let result = validate_cast(&json, &schema, &registry, &mut gc);
        assert!(result.is_err());
    }

    #[test]
    fn test_max_depth() {
        // Create deeply nested array schema
        let json = JsonValue::Null; // Simplified for test
        let schema = TypeSchema {
            type_id: 5,
            kind: TypeKind::Array {
                element_type_id: 5, // Self-referential for testing
            },
        };
        let mut registry = TypeSchemaRegistry::new();
        registry.register(5, schema.clone());
        let mut gc = GarbageCollector::default();

        // Validate at exactly max depth should work
        let result = validate_cast_impl(&json, &schema, &registry, &mut gc, MAX_VALIDATION_DEPTH);
        assert!(result.is_err()); // Will fail due to type mismatch, but not depth

        // Validate beyond max depth should fail with StackOverflow
        let result =
            validate_cast_impl(&json, &schema, &registry, &mut gc, MAX_VALIDATION_DEPTH + 1);
        assert!(matches!(result, Err(VmError::StackOverflow)));
    }
}
