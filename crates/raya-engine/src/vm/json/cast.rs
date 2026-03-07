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
use crate::vm::gc::GarbageCollector;
use crate::vm::object::{LayoutId, Object, PropKeyId};
use crate::vm::value::Value;
use crate::vm::{VmError, VmResult};
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
        /// Physical layout ID for typed object allocation.
        layout_id: crate::vm::object::LayoutId,
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
    validate_cast_with_runtime_metadata(
        json,
        schema,
        schema_registry,
        gc,
        &mut |_| None,
        &mut |_| None,
    )
}

/// Validate and cast a JSON value to a typed object using runtime metadata
/// resolvers for unified object carriers.
pub fn validate_cast_with_runtime_metadata<FP, FL>(
    json: &JsonValue,
    schema: &TypeSchema,
    schema_registry: &TypeSchemaRegistry,
    gc: &mut GarbageCollector,
    resolve_prop_key: &mut FP,
    resolve_layout_names: &mut FL,
) -> VmResult<Value>
where
    FP: FnMut(&str) -> Option<PropKeyId>,
    FL: FnMut(LayoutId) -> Option<Vec<String>>,
{
    validate_cast_impl(
        json,
        schema,
        schema_registry,
        gc,
        0,
        resolve_prop_key,
        resolve_layout_names,
    )
}

/// Internal recursive validation implementation
///
/// Tracks recursion depth to prevent stack overflow.
fn validate_cast_impl<FP, FL>(
    json: &JsonValue,
    schema: &TypeSchema,
    schema_registry: &TypeSchemaRegistry,
    gc: &mut GarbageCollector,
    depth: usize,
    resolve_prop_key: &mut FP,
    resolve_layout_names: &mut FL,
) -> VmResult<Value>
where
    FP: FnMut(&str) -> Option<PropKeyId>,
    FL: FnMut(LayoutId) -> Option<Vec<String>>,
{
    // Check recursion depth
    if depth > MAX_VALIDATION_DEPTH {
        return Err(VmError::StackOverflow);
    }

    match &schema.kind {
        TypeKind::Null => validate_null(json),

        TypeKind::Bool => validate_bool(json),

        TypeKind::Number => validate_number(json),

        TypeKind::String => validate_string(json, gc),

        TypeKind::Interface {
            class_id,
            layout_id,
            fields,
        } => validate_interface(
            json,
            *class_id,
            *layout_id,
            fields,
            schema_registry,
            gc,
            depth,
            resolve_prop_key,
            resolve_layout_names,
        ),

        TypeKind::Array { element_type_id } => {
            validate_array(
                json,
                *element_type_id,
                schema_registry,
                gc,
                depth,
                resolve_prop_key,
                resolve_layout_names,
            )
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
            resolve_prop_key,
            resolve_layout_names,
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
fn validate_interface<FP, FL>(
    json: &JsonValue,
    class_id: usize,
    layout_id: crate::vm::object::LayoutId,
    fields: &[(String, usize)],
    schema_registry: &TypeSchemaRegistry,
    gc: &mut GarbageCollector,
    depth: usize,
    resolve_prop_key: &mut FP,
    resolve_layout_names: &mut FL,
) -> VmResult<Value>
where
    FP: FnMut(&str) -> Option<PropKeyId>,
    FL: FnMut(LayoutId) -> Option<Vec<String>>,
{
    // Must be an object
    if !json.is_object() {
        return Err(VmError::TypeError(format!(
            "Expected object, got {}",
            json.type_name()
        )));
    }

    // Validate each field using get_property() (converts Value → JsonValue)
    let mut field_values = Vec::with_capacity(fields.len());

    for (field_name, field_type_id) in fields {
        let field_json =
            json.get_property_with_runtime_metadata(field_name, resolve_prop_key, resolve_layout_names);
        if field_json.is_undefined() {
            return Err(VmError::TypeError(format!("Missing field: {}", field_name)));
        }

        // Get field type schema
        let field_schema = schema_registry
            .get(*field_type_id)
            .ok_or_else(|| VmError::TypeError(format!("Unknown type ID: {}", field_type_id)))?;

        // Recursively validate field
        let field_value = validate_cast_impl(
            &field_json,
            &field_schema,
            schema_registry,
            gc,
            depth + 1,
            resolve_prop_key,
            resolve_layout_names,
        )?;

        field_values.push(field_value);
    }

    // Create typed object
    let mut obj = Object::new_nominal(layout_id, class_id as u32, field_values.len());
    obj.fields = field_values;

    let obj_ptr = gc.allocate(obj);

    Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new_unchecked(obj_ptr.as_ptr() as *mut u8)) })
}

/// Validate array type
fn validate_array<FP, FL>(
    json: &JsonValue,
    element_type_id: usize,
    schema_registry: &TypeSchemaRegistry,
    gc: &mut GarbageCollector,
    depth: usize,
    resolve_prop_key: &mut FP,
    resolve_layout_names: &mut FL,
) -> VmResult<Value>
where
    FP: FnMut(&str) -> Option<PropKeyId>,
    FL: FnMut(LayoutId) -> Option<Vec<String>>,
{
    // Must be an array
    if !json.is_array() {
        return Err(VmError::TypeError(format!(
            "Expected array, got {}",
            json.type_name()
        )));
    }

    // Get element type schema
    let element_schema = schema_registry
        .get(element_type_id)
        .ok_or_else(|| VmError::TypeError(format!("Unknown type ID: {}", element_type_id)))?;

    // Validate each element using get_index() (converts Value → JsonValue)
    let len = json.array_len();
    let mut element_values = Vec::with_capacity(len);

    for i in 0..len {
        let json_elem = json.get_index(i);
        let elem_value = validate_cast_impl(
            &json_elem,
            &element_schema,
            schema_registry,
            gc,
            depth + 1,
            resolve_prop_key,
            resolve_layout_names,
        )?;
        element_values.push(elem_value);
    }

    // Create typed array
    let arr = crate::vm::object::Array {
        type_id: element_type_id,
        elements: element_values,
    };

    let arr_ptr = gc.allocate(arr);

    Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new_unchecked(arr_ptr.as_ptr() as *mut u8)) })
}

/// Validate union type
fn validate_union<FP, FL>(
    json: &JsonValue,
    variant_type_ids: &[usize],
    discriminant: Option<&str>,
    schema_registry: &TypeSchemaRegistry,
    gc: &mut GarbageCollector,
    depth: usize,
    resolve_prop_key: &mut FP,
    resolve_layout_names: &mut FL,
) -> VmResult<Value>
where
    FP: FnMut(&str) -> Option<PropKeyId>,
    FL: FnMut(LayoutId) -> Option<Vec<String>>,
{
    // If discriminant is specified, use it to select the variant
    if let Some(disc_field) = discriminant {
        // Must be an object with the discriminant field
        if !json.is_object() {
            return Err(VmError::TypeError(format!(
                "Expected object with discriminant '{}', got {}",
                disc_field,
                json.type_name()
            )));
        }

        // Get discriminant value using get_property() (returns JsonValue)
        let disc_value =
            json.get_property_with_runtime_metadata(disc_field, resolve_prop_key, resolve_layout_names);
        if disc_value.is_undefined() {
            return Err(VmError::TypeError(format!(
                "Missing discriminant field: {}",
                disc_field
            )));
        }

        let disc_str = match &disc_value {
            JsonValue::String(s_ptr) => unsafe { &*s_ptr.as_ptr() }.data.clone(),
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

            let _ = disc_str.as_str(); // suppress unused warning

            // Try to validate against this variant
            if let Ok(value) =
                validate_cast_impl(
                    json,
                    &variant_schema,
                    schema_registry,
                    gc,
                    depth + 1,
                    resolve_prop_key,
                    resolve_layout_names,
                )
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
                validate_cast_impl(
                    json,
                    &variant_schema,
                    schema_registry,
                    gc,
                    depth + 1,
                    resolve_prop_key,
                    resolve_layout_names,
                )
            {
                return Ok(value);
            }
        }

        Err(VmError::TypeError(format!(
            "No matching variant in union for value of type: {}",
            json.type_name()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::json::value_to_json_stack;
    use crate::vm::object::{layout_id_from_ordered_names, Object, RayaString};
    use rustc_hash::FxHashMap;

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
        let mut no_prop_keys = |_name: &str| None;
        let mut no_layout_names = |_layout_id: LayoutId| None;

        // Validate at exactly max depth should work
        let result = validate_cast_impl(
            &json,
            &schema,
            &registry,
            &mut gc,
            MAX_VALIDATION_DEPTH,
            &mut no_prop_keys,
            &mut no_layout_names,
        );
        assert!(result.is_err()); // Will fail due to type mismatch, but not depth

        // Validate beyond max depth should fail with StackOverflow
        let result = validate_cast_impl(
            &json,
            &schema,
            &registry,
            &mut gc,
            MAX_VALIDATION_DEPTH + 1,
            &mut no_prop_keys,
            &mut no_layout_names,
        );
        assert!(matches!(result, Err(VmError::StackOverflow)));
    }

    fn make_string(gc: &mut GarbageCollector, s: &str) -> Value {
        let raya_str = RayaString::new(s.to_string());
        let ptr = gc.allocate(raya_str);
        unsafe { Value::from_ptr(std::ptr::NonNull::new(ptr.as_ptr()).unwrap()) }
    }

    fn user_schema_registry() -> (TypeSchemaRegistry, TypeSchema) {
        let mut registry = TypeSchemaRegistry::new();
        registry.register(
            1,
            TypeSchema {
                type_id: 1,
                kind: TypeKind::String,
            },
        );
        registry.register(
            2,
            TypeSchema {
                type_id: 2,
                kind: TypeKind::Number,
            },
        );
        let schema = TypeSchema {
            type_id: 3,
            kind: TypeKind::Interface {
                class_id: 9,
                layout_id: layout_id_from_ordered_names(&["name".to_string(), "age".to_string()]),
                fields: vec![("name".to_string(), 1), ("age".to_string(), 2)],
            },
        };
        registry.register(3, schema.clone());
        (registry, schema)
    }

    #[test]
    fn test_validate_interface_with_unified_dynamic_object_metadata() {
        let mut gc = GarbageCollector::default();
        let mut prop_keys: FxHashMap<String, PropKeyId> = FxHashMap::default();
        let name_key: PropKeyId = 1;
        let age_key: PropKeyId = 2;
        prop_keys.insert("name".to_string(), name_key);
        prop_keys.insert("age".to_string(), age_key);
        let mut object = Object::new_dynamic(layout_id_from_ordered_names(&[]), 0);
        {
            let dyn_map = object.ensure_dyn_map();
            dyn_map.insert(name_key, make_string(&mut gc, "Alice"));
            dyn_map.insert(age_key, Value::f64(30.0));
        }
        let object_ptr = gc.allocate(object);
        let json = value_to_json_stack(unsafe {
            Value::from_ptr(std::ptr::NonNull::new(object_ptr.as_ptr()).unwrap())
        });
        let (registry, schema) = user_schema_registry();

        let mut resolve_prop_key = |name: &str| prop_keys.get(name).copied();
        let mut resolve_layout_names = |_layout_id: LayoutId| None;
        let result = validate_cast_with_runtime_metadata(
            &json,
            &schema,
            &registry,
            &mut gc,
            &mut resolve_prop_key,
            &mut resolve_layout_names,
        )
        .expect("validated cast");

        let obj = unsafe { &*result.as_ptr::<Object>().unwrap().as_ptr() };
        assert_eq!(obj.field_count(), 2);
        let name = unsafe { &*obj.get_field(0).unwrap().as_ptr::<RayaString>().unwrap().as_ptr() };
        assert_eq!(name.data, "Alice");
        assert_eq!(obj.get_field(1).unwrap().as_f64(), Some(30.0));
    }

    #[test]
    fn test_validate_interface_with_structural_layout_metadata() {
        let mut gc = GarbageCollector::default();
        let names = vec!["name".to_string(), "age".to_string()];
        let layout_id = layout_id_from_ordered_names(&names);
        let mut object = Object::new_structural(layout_id, 2);
        object.set_field(0, make_string(&mut gc, "Bob")).unwrap();
        object.set_field(1, Value::f64(25.0)).unwrap();
        let object_ptr = gc.allocate(object);
        let json = value_to_json_stack(unsafe {
            Value::from_ptr(std::ptr::NonNull::new(object_ptr.as_ptr()).unwrap())
        });
        let (registry, schema) = user_schema_registry();

        let mut resolve_prop_key = |_name: &str| None;
        let layout_names = names.clone();
        let mut resolve_layout_names = move |candidate: LayoutId| {
            (candidate == layout_id).then_some(layout_names.clone())
        };
        let result = validate_cast_with_runtime_metadata(
            &json,
            &schema,
            &registry,
            &mut gc,
            &mut resolve_prop_key,
            &mut resolve_layout_names,
        )
        .expect("validated cast");

        let obj = unsafe { &*result.as_ptr::<Object>().unwrap().as_ptr() };
        let name = unsafe { &*obj.get_field(0).unwrap().as_ptr::<RayaString>().unwrap().as_ptr() };
        assert_eq!(name.data, "Bob");
        assert_eq!(obj.get_field(1).unwrap().as_f64(), Some(25.0));
    }
}
