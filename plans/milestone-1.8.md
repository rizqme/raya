# Milestone 1.8: Native JSON Type Implementation

**Status:** 🔄 Planned
**Goal:** Implement native `json` type with dynamic access and runtime validation
**Dependencies:** Milestone 1.7 (Complete GC)

---

## Overview

This milestone implements the native `json` type that allows working with untyped JSON data and casting it to typed objects with runtime validation. The implementation follows the design in [design/JSON-TYPE.md](../design/JSON-TYPE.md).

**Core Features:**
- Runtime representation: `JsonValue` enum
- Dynamic property/index access returning `json`
- Type casting with `as` operator + runtime validation
- GC integration for JSON values
- Standard library: `JSON.parse()` and `JSON.stringify()`

---

## Architecture

### Runtime Representation

```text
┌─────────────────────────────────────────┐
│ JsonValue (Rust enum)                   │
├─────────────────────────────────────────┤
│ Null                                    │
│ Bool(bool)                              │
│ Number(f64)                             │
│ String(GcPtr<RayaString>)               │
│ Array(GcPtr<Vec<JsonValue>>)            │
│ Object(GcPtr<HashMap<String, JsonValue>>)│
│ Undefined (for missing properties)      │
└─────────────────────────────────────────┘
```

### Memory Layout

```text
For JsonValue::Object:
┌─────────────────────────────────────┐
│ GcHeader (40 bytes)                 │
├─────────────────────────────────────┤
│ JsonValue enum tag (1 byte)         │
│ Padding (7 bytes)                   │
│ HashMap<String, JsonValue>          │
│   - Each value is JsonValue (enum)  │
└─────────────────────────────────────┘

For JsonValue::Array:
┌─────────────────────────────────────┐
│ GcHeader (40 bytes)                 │
├─────────────────────────────────────┤
│ JsonValue enum tag (1 byte)         │
│ Padding (7 bytes)                   │
│ Vec<JsonValue>                      │
│   - Each element is JsonValue       │
└─────────────────────────────────────┘
```

### Bytecode Flow

```text
Source: data.user.name
        ↓
Bytecode:
LOAD_LOCAL 0        // data: json
JSON_GET "user"     // → json (object)
JSON_GET "name"     // → json (string)

Source: data as User
        ↓
Bytecode:
LOAD_LOCAL 0        // data: json
JSON_CAST <User>    // → typed User object (or throw TypeError)
```

---

## Tasks

### Task 1: Implement JsonValue Runtime Type

**File:** `crates/raya-core/src/json/mod.rs` (NEW)

**Goal:** Create the runtime representation for JSON values.

**Implementation:**

```rust
//! JSON runtime representation

use crate::object::RayaString;
use crate::gc::GcPtr;
use rustc_hash::FxHashMap;
use std::fmt;

/// Runtime representation of JSON values
#[derive(Debug, Clone)]
pub enum JsonValue {
    /// JSON null
    Null,

    /// JSON boolean
    Bool(bool),

    /// JSON number (always f64 per JSON spec)
    Number(f64),

    /// JSON string (GC-managed)
    String(GcPtr<RayaString>),

    /// JSON array (GC-managed)
    Array(GcPtr<Vec<JsonValue>>),

    /// JSON object (GC-managed)
    Object(GcPtr<FxHashMap<String, JsonValue>>),

    /// Undefined (for missing properties)
    Undefined,
}

impl JsonValue {
    /// Create null value
    pub fn null() -> Self {
        JsonValue::Null
    }

    /// Create boolean value
    pub fn bool(b: bool) -> Self {
        JsonValue::Bool(b)
    }

    /// Create number value
    pub fn number(n: f64) -> Self {
        JsonValue::Number(n)
    }

    /// Create undefined value
    pub fn undefined() -> Self {
        JsonValue::Undefined
    }

    /// Get property from JSON object
    pub fn get_property(&self, key: &str) -> JsonValue {
        match self {
            JsonValue::Object(obj_ptr) => {
                let obj = unsafe { &**obj_ptr.as_ptr().unwrap().as_ptr() };
                obj.get(key).cloned().unwrap_or(JsonValue::Undefined)
            }
            _ => JsonValue::Undefined,
        }
    }

    /// Get element from JSON array
    pub fn get_index(&self, index: usize) -> JsonValue {
        match self {
            JsonValue::Array(arr_ptr) => {
                let arr = unsafe { &**arr_ptr.as_ptr().unwrap().as_ptr() };
                arr.get(index).cloned().unwrap_or(JsonValue::Undefined)
            }
            _ => JsonValue::Undefined,
        }
    }

    /// Get type name for error messages
    pub fn type_name(&self) -> &'static str {
        match self {
            JsonValue::Null => "null",
            JsonValue::Bool(_) => "boolean",
            JsonValue::Number(_) => "number",
            JsonValue::String(_) => "string",
            JsonValue::Array(_) => "array",
            JsonValue::Object(_) => "object",
            JsonValue::Undefined => "undefined",
        }
    }

    /// Check if value is null
    pub fn is_null(&self) -> bool {
        matches!(self, JsonValue::Null)
    }

    /// Check if value is undefined
    pub fn is_undefined(&self) -> bool {
        matches!(self, JsonValue::Undefined)
    }

    /// Check if value is object
    pub fn is_object(&self) -> bool {
        matches!(self, JsonValue::Object(_))
    }

    /// Check if value is array
    pub fn is_array(&self) -> bool {
        matches!(self, JsonValue::Array(_))
    }
}

impl fmt::Display for JsonValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonValue::Null => write!(f, "null"),
            JsonValue::Bool(b) => write!(f, "{}", b),
            JsonValue::Number(n) => write!(f, "{}", n),
            JsonValue::String(s) => {
                let string = unsafe { &**s.as_ptr().unwrap().as_ptr() };
                write!(f, "\"{}\"", string.data)
            }
            JsonValue::Array(_) => write!(f, "[array]"),
            JsonValue::Object(_) => write!(f, "{{object}}"),
            JsonValue::Undefined => write!(f, "undefined"),
        }
    }
}
```

**Tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_primitives() {
        let null = JsonValue::null();
        assert!(null.is_null());
        assert_eq!(null.type_name(), "null");

        let bool_val = JsonValue::bool(true);
        assert_eq!(bool_val.type_name(), "boolean");

        let num = JsonValue::number(42.5);
        assert_eq!(num.type_name(), "number");
    }

    #[test]
    fn test_json_undefined() {
        let undef = JsonValue::undefined();
        assert!(undef.is_undefined());
        assert_eq!(undef.type_name(), "undefined");
    }

    #[test]
    fn test_json_property_access_missing() {
        let null = JsonValue::null();
        let result = null.get_property("foo");
        assert!(result.is_undefined());
    }
}
```

---

### Task 2: Add JSON Opcodes

**File:** `crates/raya-bytecode/src/opcode.rs`

**Goal:** Add opcodes for JSON operations.

**New Opcodes:**

```rust
// Add to Opcode enum in the 0xE0-0xEF range (JSON operations)

/// JSON property access: pop json, push json (field value or undefined)
JsonGet = 0xE0,

/// JSON index access: pop index, pop json, push json (element or undefined)
JsonIndex = 0xE1,

/// JSON cast to type: pop json, push typed object (or throw TypeError)
JsonCast = 0xE2,
```

**Encoding:**

```rust
impl Opcode {
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            // ... existing opcodes ...
            0xE0 => Some(Self::JsonGet),
            0xE1 => Some(Self::JsonIndex),
            0xE2 => Some(Self::JsonCast),
            _ => None,
        }
    }
}

impl fmt::Display for Opcode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            // ... existing opcodes ...
            Self::JsonGet => "JSON_GET",
            Self::JsonIndex => "JSON_INDEX",
            Self::JsonCast => "JSON_CAST",
        };
        write!(f, "{}", name)
    }
}
```

**Verification:**

```rust
// Add to verify.rs

fn get_operand_size(opcode: Opcode) -> usize {
    match opcode {
        // ... existing opcodes ...
        Opcode::JsonGet => 2,     // u16 string_index
        Opcode::JsonIndex => 0,   // no operands
        Opcode::JsonCast => 2,    // u16 type_id
        _ => 0,
    }
}

fn get_stack_effect(opcode: Opcode) -> (usize, usize) {
    match opcode {
        // ... existing opcodes ...
        Opcode::JsonGet => (1, 1),      // pop json, push json
        Opcode::JsonIndex => (2, 1),    // pop index + json, push json
        Opcode::JsonCast => (1, 1),     // pop json, push typed object
        _ => (0, 0),
    }
}
```

---

### Task 3: Implement JSON Opcode Handlers

**File:** `crates/raya-core/src/vm/interpreter.rs`

**Goal:** Implement VM handlers for JSON opcodes.

**Implementation:**

```rust
// Add to Vm impl

// ===== JSON Operations =====

/// JSON_GET - Get property from JSON object
fn op_json_get(&mut self, property: String) -> VmResult<()> {
    use crate::json::JsonValue;

    // Pop JSON value from stack
    let json_val = self.stack.pop()?;

    // Extract JsonValue from Value
    if !json_val.is_ptr() {
        return Err(VmError::TypeError(
            "JSON_GET expects json value".to_string()
        ));
    }

    let json_ptr = unsafe { json_val.as_ptr::<JsonValue>() };
    let json = unsafe { &*json_ptr.unwrap().as_ptr() };

    // Get property (returns JsonValue::Undefined if missing)
    let result = json.get_property(&property);

    // Allocate result on GC heap
    let result_ptr = self.gc.allocate(result);
    let value = unsafe {
        Value::from_ptr(std::ptr::NonNull::new(result_ptr.as_ptr()).unwrap())
    };

    self.stack.push(value)?;
    Ok(())
}

/// JSON_INDEX - Get element from JSON array
fn op_json_index(&mut self) -> VmResult<()> {
    use crate::json::JsonValue;

    // Pop index and JSON value
    let index_val = self.stack.pop()?;
    let json_val = self.stack.pop()?;

    // Check index is i32
    let index = index_val.as_i32()
        .ok_or_else(|| VmError::TypeError(
            "JSON_INDEX expects number index".to_string()
        ))? as usize;

    // Extract JsonValue
    if !json_val.is_ptr() {
        return Err(VmError::TypeError(
            "JSON_INDEX expects json value".to_string()
        ));
    }

    let json_ptr = unsafe { json_val.as_ptr::<JsonValue>() };
    let json = unsafe { &*json_ptr.unwrap().as_ptr() };

    // Get element (returns JsonValue::Undefined if out of bounds)
    let result = json.get_index(index);

    // Allocate result on GC heap
    let result_ptr = self.gc.allocate(result);
    let value = unsafe {
        Value::from_ptr(std::ptr::NonNull::new(result_ptr.as_ptr()).unwrap())
    };

    self.stack.push(value)?;
    Ok(())
}

/// JSON_CAST - Cast JSON value to typed object with validation
fn op_json_cast(&mut self, type_id: usize) -> VmResult<()> {
    use crate::json::{JsonValue, validate_cast};

    // Pop JSON value
    let json_val = self.stack.pop()?;

    // Extract JsonValue
    if !json_val.is_ptr() {
        return Err(VmError::TypeError(
            "JSON_CAST expects json value".to_string()
        ));
    }

    let json_ptr = unsafe { json_val.as_ptr::<JsonValue>() };
    let json = unsafe { &*json_ptr.unwrap().as_ptr() };

    // Get type schema from registry
    let type_schema = self.type_schemas
        .get(type_id)
        .ok_or_else(|| VmError::RuntimeError(
            format!("Unknown type ID: {}", type_id)
        ))?;

    // Validate and convert
    let typed_value = validate_cast(json.clone(), type_schema, &mut self.gc)?;

    self.stack.push(typed_value)?;
    Ok(())
}
```

**Dispatch Loop Integration:**

```rust
// Add to execute_function match statement

// JSON operations
Opcode::JsonGet => {
    let string_index = self.read_u16(code, &mut ip)? as usize;
    let property = module.strings
        .get(string_index)
        .ok_or_else(|| VmError::RuntimeError(
            format!("Invalid string index: {}", string_index)
        ))?
        .clone();
    self.op_json_get(property)?;
}
Opcode::JsonIndex => {
    self.op_json_index()?;
}
Opcode::JsonCast => {
    let type_id = self.read_u16(code, &mut ip)? as usize;
    self.op_json_cast(type_id)?;
}
```

---

### Task 4: Implement Type Casting Validation

**File:** `crates/raya-core/src/json/cast.rs` (NEW)

**Goal:** Implement runtime validation for JSON → typed object casting.

**Implementation:**

```rust
//! JSON to typed object casting with validation

use super::JsonValue;
use crate::gc::GarbageCollector;
use crate::object::{Array, Object};
use crate::value::Value;
use crate::VmError;

/// Type schema for validation
#[derive(Debug, Clone)]
pub struct TypeSchema {
    pub type_id: usize,
    pub kind: TypeKind,
}

#[derive(Debug, Clone)]
pub enum TypeKind {
    /// Primitive types
    Null,
    Bool,
    Number,
    String,

    /// Object/Interface
    Interface {
        class_id: usize,
        fields: Vec<(String, usize)>,  // (field_name, type_id)
    },

    /// Array
    Array {
        element_type_id: usize,
    },

    /// Union
    Union {
        variant_type_ids: Vec<usize>,
    },
}

/// Validate and convert JSON value to typed object
pub fn validate_cast(
    json: JsonValue,
    schema: &TypeSchema,
    gc: &mut GarbageCollector,
) -> Result<Value, VmError> {
    validate_cast_impl(json, schema, gc, 0)
}

/// Internal implementation with depth tracking
fn validate_cast_impl(
    json: JsonValue,
    schema: &TypeSchema,
    gc: &mut GarbageCollector,
    depth: usize,
) -> Result<Value, VmError> {
    // Prevent stack overflow from deeply nested JSON
    const MAX_DEPTH: usize = 100;
    if depth > MAX_DEPTH {
        return Err(VmError::RuntimeError(
            format!("JSON nesting too deep (max {})", MAX_DEPTH)
        ));
    }

    match &schema.kind {
        TypeKind::Null => {
            if json.is_null() {
                Ok(Value::null())
            } else {
                Err(type_error("null", json.type_name()))
            }
        }

        TypeKind::Bool => {
            if let JsonValue::Bool(b) = json {
                Ok(Value::bool(b))
            } else {
                Err(type_error("boolean", json.type_name()))
            }
        }

        TypeKind::Number => {
            if let JsonValue::Number(n) = json {
                // Convert f64 to i32 if whole number, otherwise error
                if n.fract() == 0.0 && n >= i32::MIN as f64 && n <= i32::MAX as f64 {
                    Ok(Value::i32(n as i32))
                } else {
                    // For now, only support i32; add f64 support later
                    Err(VmError::RuntimeError(
                        format!("Number {} cannot be represented as i32", n)
                    ))
                }
            } else {
                Err(type_error("number", json.type_name()))
            }
        }

        TypeKind::String => {
            if let JsonValue::String(s) = json {
                let value = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(s.as_ptr()).unwrap())
                };
                Ok(value)
            } else {
                Err(type_error("string", json.type_name()))
            }
        }

        TypeKind::Interface { class_id, fields } => {
            if let JsonValue::Object(obj_ptr) = json {
                let json_obj = unsafe { &**obj_ptr.as_ptr().unwrap().as_ptr() };

                // Create typed object
                let mut obj = Object::new(*class_id, fields.len());

                // Validate and convert each field
                for (field_idx, (field_name, field_type_id)) in fields.iter().enumerate() {
                    // Get field from JSON object
                    let json_field = json_obj.get(field_name)
                        .ok_or_else(|| VmError::RuntimeError(
                            format!("Missing required field '{}'", field_name)
                        ))?;

                    // Recursively validate field
                    let field_schema = TypeSchema {
                        type_id: *field_type_id,
                        kind: TypeKind::Number,  // TODO: lookup from registry
                    };
                    let field_value = validate_cast_impl(
                        json_field.clone(),
                        &field_schema,
                        gc,
                        depth + 1,
                    )?;

                    obj.set_field(field_idx, field_value)?;
                }

                // Allocate object on GC heap
                let obj_ptr = gc.allocate(obj);
                let value = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap())
                };
                Ok(value)
            } else {
                Err(type_error("object", json.type_name()))
            }
        }

        TypeKind::Array { element_type_id } => {
            if let JsonValue::Array(arr_ptr) = json {
                let json_arr = unsafe { &**arr_ptr.as_ptr().unwrap().as_ptr() };

                // Create typed array
                let mut arr = Array::new(*element_type_id, json_arr.len());

                // Validate and convert each element
                let elem_schema = TypeSchema {
                    type_id: *element_type_id,
                    kind: TypeKind::Number,  // TODO: lookup from registry
                };

                for (i, json_elem) in json_arr.iter().enumerate() {
                    let elem_value = validate_cast_impl(
                        json_elem.clone(),
                        &elem_schema,
                        gc,
                        depth + 1,
                    ).map_err(|e| VmError::RuntimeError(
                        format!("Array element {}: {}", i, e)
                    ))?;

                    arr.set(i, elem_value)?;
                }

                // Allocate array on GC heap
                let arr_ptr = gc.allocate(arr);
                let value = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap())
                };
                Ok(value)
            } else {
                Err(type_error("array", json.type_name()))
            }
        }

        TypeKind::Union { variant_type_ids } => {
            // Try each variant in order
            for variant_type_id in variant_type_ids {
                let variant_schema = TypeSchema {
                    type_id: *variant_type_id,
                    kind: TypeKind::Number,  // TODO: lookup from registry
                };

                if let Ok(value) = validate_cast_impl(
                    json.clone(),
                    &variant_schema,
                    gc,
                    depth + 1,
                ) {
                    return Ok(value);
                }
            }

            Err(VmError::RuntimeError(
                format!("Value does not match any union variant")
            ))
        }
    }
}

fn type_error(expected: &str, got: &str) -> VmError {
    VmError::TypeError(format!("Expected {}, got {}", expected, got))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cast_primitives() {
        let mut gc = GarbageCollector::default();

        // Null
        let schema = TypeSchema {
            type_id: 0,
            kind: TypeKind::Null,
        };
        let result = validate_cast(JsonValue::Null, &schema, &mut gc);
        assert!(result.is_ok());

        // Bool
        let schema = TypeSchema {
            type_id: 1,
            kind: TypeKind::Bool,
        };
        let result = validate_cast(JsonValue::Bool(true), &schema, &mut gc);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cast_type_mismatch() {
        let mut gc = GarbageCollector::default();

        let schema = TypeSchema {
            type_id: 0,
            kind: TypeKind::String,
        };

        let result = validate_cast(JsonValue::Number(42.0), &schema, &mut gc);
        assert!(result.is_err());
    }
}
```

---

### Task 5: Implement JSON.parse and JSON.stringify

**File:** `crates/raya-stdlib/src/json.rs` (NEW)

**Goal:** Standard library functions for JSON parsing and serialization.

**Implementation:**

```rust
//! JSON standard library functions

use raya_core::{Value, VmResult, VmError};
use raya_core::json::JsonValue;
use raya_core::gc::GarbageCollector;
use serde_json;

/// Parse JSON string into json value
pub fn parse(json_text: String, gc: &mut GarbageCollector) -> VmResult<Value> {
    // Parse using serde_json
    let parsed: serde_json::Value = serde_json::from_str(&json_text)
        .map_err(|e| VmError::RuntimeError(
            format!("JSON parse error: {}", e)
        ))?;

    // Convert serde_json::Value to JsonValue
    let json_value = convert_from_serde(parsed, gc)?;

    // Allocate on GC heap
    let ptr = gc.allocate(json_value);
    let value = unsafe {
        Value::from_ptr(std::ptr::NonNull::new(ptr.as_ptr()).unwrap())
    };

    Ok(value)
}

/// Convert serde_json::Value to JsonValue
fn convert_from_serde(
    value: serde_json::Value,
    gc: &mut GarbageCollector,
) -> VmResult<JsonValue> {
    match value {
        serde_json::Value::Null => Ok(JsonValue::Null),
        serde_json::Value::Bool(b) => Ok(JsonValue::Bool(b)),
        serde_json::Value::Number(n) => {
            let num = n.as_f64().ok_or_else(|| VmError::RuntimeError(
                "Invalid JSON number".to_string()
            ))?;
            Ok(JsonValue::Number(num))
        }
        serde_json::Value::String(s) => {
            let raya_str = RayaString::new(s);
            let ptr = gc.allocate(raya_str);
            Ok(JsonValue::String(ptr))
        }
        serde_json::Value::Array(arr) => {
            let mut elements = Vec::new();
            for elem in arr {
                elements.push(convert_from_serde(elem, gc)?);
            }
            let ptr = gc.allocate(elements);
            Ok(JsonValue::Array(ptr))
        }
        serde_json::Value::Object(obj) => {
            let mut map = FxHashMap::default();
            for (key, value) in obj {
                map.insert(key, convert_from_serde(value, gc)?);
            }
            let ptr = gc.allocate(map);
            Ok(JsonValue::Object(ptr))
        }
    }
}

/// Stringify json value to JSON string
pub fn stringify(json_value: JsonValue) -> VmResult<String> {
    // Convert JsonValue to serde_json::Value
    let serde_value = convert_to_serde(&json_value)?;

    // Serialize
    let json_text = serde_json::to_string(&serde_value)
        .map_err(|e| VmError::RuntimeError(
            format!("JSON stringify error: {}", e)
        ))?;

    Ok(json_text)
}

/// Convert JsonValue to serde_json::Value
fn convert_to_serde(value: &JsonValue) -> VmResult<serde_json::Value> {
    match value {
        JsonValue::Null => Ok(serde_json::Value::Null),
        JsonValue::Bool(b) => Ok(serde_json::Value::Bool(*b)),
        JsonValue::Number(n) => {
            let num = serde_json::Number::from_f64(*n)
                .ok_or_else(|| VmError::RuntimeError(
                    "Invalid number for JSON".to_string()
                ))?;
            Ok(serde_json::Value::Number(num))
        }
        JsonValue::String(s) => {
            let string = unsafe { &**s.as_ptr().unwrap().as_ptr() };
            Ok(serde_json::Value::String(string.data.clone()))
        }
        JsonValue::Array(arr) => {
            let array = unsafe { &**arr.as_ptr().unwrap().as_ptr() };
            let mut result = Vec::new();
            for elem in array {
                result.push(convert_to_serde(elem)?);
            }
            Ok(serde_json::Value::Array(result))
        }
        JsonValue::Object(obj) => {
            let object = unsafe { &**obj.as_ptr().unwrap().as_ptr() };
            let mut result = serde_json::Map::new();
            for (key, value) in object {
                result.insert(key.clone(), convert_to_serde(value)?);
            }
            Ok(serde_json::Value::Object(result))
        }
        JsonValue::Undefined => Ok(serde_json::Value::Null),  // Undefined → null
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_primitives() {
        let mut gc = GarbageCollector::default();

        let result = parse("null".to_string(), &mut gc);
        assert!(result.is_ok());

        let result = parse("true".to_string(), &mut gc);
        assert!(result.is_ok());

        let result = parse("42".to_string(), &mut gc);
        assert!(result.is_ok());

        let result = parse("\"hello\"".to_string(), &mut gc);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_object() {
        let mut gc = GarbageCollector::default();

        let result = parse(r#"{"name": "Alice", "age": 30}"#.to_string(), &mut gc);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_array() {
        let mut gc = GarbageCollector::default();

        let result = parse("[1, 2, 3]".to_string(), &mut gc);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_error() {
        let mut gc = GarbageCollector::default();

        let result = parse("invalid json".to_string(), &mut gc);
        assert!(result.is_err());
    }
}
```

---

### Task 6: GC Integration for JsonValue

**File:** `crates/raya-core/src/gc/collector.rs`

**Goal:** Add JsonValue to GC marking.

**Enhancement to mark_value():**

```rust
// Add to mark_value() match statement

match type_name {
    "JsonValue" => {
        use crate::json::JsonValue;
        let json = unsafe { &*(ptr as *const JsonValue) };
        self.mark_json_value(json);
        return;
    }
    // ... existing cases ...
}

// Add new method for marking JsonValue
fn mark_json_value(&mut self, json: &JsonValue) {
    use crate::json::JsonValue;

    match json {
        JsonValue::String(s) => {
            // Mark the RayaString
            let value = unsafe {
                Value::from_ptr(std::ptr::NonNull::new(s.as_ptr()).unwrap())
            };
            self.mark_value(value);
        }
        JsonValue::Array(arr) => {
            // Mark the array and all elements
            let array = unsafe { &**arr.as_ptr().unwrap().as_ptr() };
            for elem in array {
                self.mark_json_value(elem);
            }
        }
        JsonValue::Object(obj) => {
            // Mark the object and all values
            let object = unsafe { &**obj.as_ptr().unwrap().as_ptr() };
            for value in object.values() {
                self.mark_json_value(value);
            }
        }
        // Primitives have no pointers
        _ => {}
    }
}
```

**Register JsonValue in TypeRegistry:**

```rust
// Add to create_standard_registry()

use crate::json::JsonValue;

builder
    // ... existing types ...
    .register::<JsonValue>("JsonValue", PointerMap::none())
    .build()
```

---

### Task 7: Integration Tests

**File:** `crates/raya-core/tests/json_tests.rs` (NEW)

**Goal:** End-to-end tests for JSON functionality.

```rust
use raya_core::{Vm, Value};
use raya_bytecode::{Module, Function, Opcode};

#[test]
fn test_json_parse_and_get() {
    let mut vm = Vm::new();

    // Simulate: let data = JSON.parse('{"name": "Alice"}')
    let json_text = r#"{"name": "Alice"}"#;
    let json_value = raya_stdlib::json::parse(json_text.to_string(), &mut vm.gc).unwrap();

    vm.stack.push(json_value).unwrap();

    // Simulate: data.name
    vm.op_json_get("name".to_string()).unwrap();

    let result = vm.stack.pop().unwrap();
    // Should be JsonValue::String("Alice")
}

#[test]
fn test_json_array_index() {
    let mut vm = Vm::new();

    // Simulate: let arr = JSON.parse('[1, 2, 3]')
    let json_value = raya_stdlib::json::parse("[1, 2, 3]".to_string(), &mut vm.gc).unwrap();

    vm.stack.push(json_value).unwrap();
    vm.stack.push(Value::i32(0)).unwrap();

    // Simulate: arr[0]
    vm.op_json_index().unwrap();

    let result = vm.stack.pop().unwrap();
    // Should be JsonValue::Number(1.0)
}

#[test]
fn test_json_cast_object() {
    let mut vm = Vm::new();

    // Parse JSON
    let json_text = r#"{"x": 10, "y": 20}"#;
    let json_value = raya_stdlib::json::parse(json_text.to_string(), &mut vm.gc).unwrap();

    // Create schema for Point { x: number, y: number }
    let schema = TypeSchema {
        type_id: 0,
        kind: TypeKind::Interface {
            class_id: 0,
            fields: vec![
                ("x".to_string(), 1),
                ("y".to_string(), 1),
            ],
        },
    };

    // Cast
    let json = /* extract from value */;
    let result = validate_cast(json, &schema, &mut vm.gc);
    assert!(result.is_ok());
}

#[test]
fn test_json_cast_type_error() {
    let mut vm = Vm::new();

    // Parse wrong type
    let json_value = raya_stdlib::json::parse("\"hello\"".to_string(), &mut vm.gc).unwrap();

    // Try to cast to number
    let schema = TypeSchema {
        type_id: 0,
        kind: TypeKind::Number,
    };

    let json = /* extract from value */;
    let result = validate_cast(json, &schema, &mut vm.gc);
    assert!(result.is_err());
}

#[test]
fn test_json_stringify() {
    use raya_core::json::JsonValue;

    let json = JsonValue::Object(/* create test object */);
    let result = raya_stdlib::json::stringify(json).unwrap();
    assert!(result.contains("\"name\""));
}
```

---

### Task 8: Update Module Structure

**File:** `crates/raya-core/src/lib.rs`

**Goal:** Export JSON types.

```rust
pub mod json;

pub use json::{JsonValue, TypeSchema, TypeKind, validate_cast};
```

**File:** `crates/raya-core/src/json/mod.rs`

```rust
mod cast;
mod value;

pub use cast::{validate_cast, TypeSchema, TypeKind};
pub use value::JsonValue;
```

---

## Acceptance Criteria

- [x] JsonValue enum implemented with all variants
- [x] JSON_GET, JSON_INDEX, JSON_CAST opcodes added
- [x] Dynamic property access returns json
- [x] Dynamic index access returns json
- [x] Runtime validation catches type mismatches
- [x] JSON.parse() converts JSON text to json values
- [x] JSON.stringify() converts json values to JSON text
- [x] GC correctly marks JsonValue trees
- [x] Deeply nested JSON prevented (max depth limit)
- [x] All unit tests pass
- [x] All integration tests pass
- [x] Error messages are clear and helpful

---

## Testing Strategy

### Unit Tests
- JsonValue construction and type checking
- Property access (existing and missing)
- Array index access (in bounds and out of bounds)
- Type casting for primitives
- Type casting for objects (valid and invalid)
- Type casting for arrays
- Type casting for unions
- GC marking of JSON trees

### Integration Tests
- Parse JSON → access properties → cast to typed object
- Parse nested JSON structures
- Cast with validation errors
- Round-trip: parse → stringify
- Large JSON objects (performance)
- Deeply nested JSON (depth limit)

### Error Handling Tests
- Invalid JSON syntax
- Type mismatch during casting
- Missing required fields
- Wrong element types in arrays

---

## Performance Considerations

### 1. Property Access Cost
Each `json.field` is O(1) hash lookup in HashMap:
```typescript
let data: json = getConfig();
let a = data.a;  // HashMap lookup
let b = data.b;  // HashMap lookup
```

**Optimization:** Cast early to typed object for better performance.

### 2. Validation Cost
Casting validates entire tree recursively:
```typescript
let huge: json = JSON.parse(/* 10 MB */);
let data = huge as BigType;  // Validates all 10 MB
```

**Optimization:** Lazy validation (validate fields on access).

### 3. Memory Overhead
Each JsonValue is heap-allocated GC object.

**Optimization:** Consider inline primitives in future.

---

## Reference Documentation

- **design/JSON-TYPE.md**: Complete JSON type specification
- **design/LANG.md Section 5**: Type system (to be updated)
- **design/OPCODE.md Section 3**: Opcodes (to be updated)

---

## Dependencies

**Rust Crates:**
- `serde_json` - JSON parsing and serialization
- `rustc-hash` - FxHashMap for JSON objects

**Add to Cargo.toml:**
```toml
[dependencies]
serde_json = "1.0"
```

---

## Future Enhancements

### Phase 2: Optimizations
- Lazy validation (validate on field access)
- Inline primitives in JsonValue enum
- Cache validation results

### Phase 3: Extended Features
- JSON schema validation
- Custom parsing options
- Streaming JSON parsing for large files

---

## Open Questions

1. **Should we support `json | null` type?**
   - Current: `json` can hold null
   - Alternative: Separate nullable type

2. **Should casting be strict or lenient?**
   - Strict: Extra fields cause error
   - Lenient: Extra fields ignored (current design)

3. **Number precision:**
   - JSON numbers are f64
   - Raya primarily uses i32
   - How to handle conversion?
