//! Native JSON type runtime support
//!
//! This module implements the `json` type, which provides dynamic JSON values
//! with runtime type casting and validation. JSON values are heap-allocated
//! and garbage-collected.
//!
//! # Design Philosophy
//!
//! - **Dynamic until cast**: JSON values are opaque at compile time
//! - **Property access returns json**: No compile-time structure validation
//! - **Runtime validation on cast**: `as` operator performs tree validation
//! - **JavaScript-like behavior**: Missing properties return Undefined
//!
//! # Example
//!
//! ```raya
//! let response: json = await fetch("/api/user");
//! let user = response as User;  // Runtime validation
//! console.log(user.name.toUpperCase());  // Fully typed
//! ```

use crate::gc::GcPtr;
use crate::object::RayaString;
use rustc_hash::FxHashMap;
use std::fmt;

pub mod cast;
pub mod parser;
pub mod stringify;

// Re-export key types for easier access
pub use cast::{validate_cast, TypeKind, TypeSchema, TypeSchemaRegistry};

/// Runtime representation of JSON values
///
/// All JSON values are heap-allocated and managed by the garbage collector.
/// This enum represents the 7 possible JSON value types plus Undefined for
/// missing properties/elements.
#[derive(Debug, Clone)]
pub enum JsonValue {
    /// JSON null
    Null,

    /// JSON boolean (true/false)
    Bool(bool),

    /// JSON number (always f64, following JSON spec)
    Number(f64),

    /// JSON string (heap-allocated, GC-managed)
    String(GcPtr<RayaString>),

    /// JSON array (heap-allocated vector of JsonValues)
    Array(GcPtr<Vec<JsonValue>>),

    /// JSON object (heap-allocated hashmap, GC-managed)
    Object(GcPtr<FxHashMap<String, JsonValue>>),

    /// Undefined value (for missing properties/elements)
    /// Not part of JSON spec, but needed for JavaScript-like behavior
    Undefined,
}

impl JsonValue {
    /// Get a property from a JSON object
    ///
    /// Returns the value if the object has the property, otherwise Undefined.
    /// If called on a non-object, returns Undefined.
    ///
    /// # Example
    ///
    /// ```raya
    /// let obj: json = { "name": "Alice", "age": 30 };
    /// let name = obj.name;  // Compiles to get_property("name")
    /// ```
    pub fn get_property(&self, key: &str) -> JsonValue {
        match self {
            JsonValue::Object(obj_ptr) => {
                // Safety: GcPtr guarantees valid pointer from GC
                let obj = unsafe { &*obj_ptr.as_ptr() };
                obj.get(key).cloned().unwrap_or(JsonValue::Undefined)
            }
            _ => JsonValue::Undefined,
        }
    }

    /// Get an element from a JSON array by index
    ///
    /// Returns the element if the index is valid, otherwise Undefined.
    /// If called on a non-array, returns Undefined.
    ///
    /// # Example
    ///
    /// ```raya
    /// let arr: json = [1, 2, 3];
    /// let first = arr[0];  // Compiles to get_index(0)
    /// ```
    pub fn get_index(&self, index: usize) -> JsonValue {
        match self {
            JsonValue::Array(arr_ptr) => {
                // Safety: GcPtr guarantees valid pointer from GC
                let arr = unsafe { &*arr_ptr.as_ptr() };
                arr.get(index).cloned().unwrap_or(JsonValue::Undefined)
            }
            _ => JsonValue::Undefined,
        }
    }

    /// Get the type name as a string (for typeof operator)
    ///
    /// Returns:
    /// - "null" for Null
    /// - "boolean" for Bool
    /// - "number" for Number
    /// - "string" for String
    /// - "object" for Object
    /// - "object" for Array (following JavaScript convention)
    /// - "undefined" for Undefined
    pub fn type_name(&self) -> &'static str {
        match self {
            JsonValue::Null => "null",
            JsonValue::Bool(_) => "boolean",
            JsonValue::Number(_) => "number",
            JsonValue::String(_) => "string",
            JsonValue::Array(_) => "object", // JavaScript convention
            JsonValue::Object(_) => "object",
            JsonValue::Undefined => "undefined",
        }
    }

    /// Check if this is a null value
    pub fn is_null(&self) -> bool {
        matches!(self, JsonValue::Null)
    }

    /// Check if this is a boolean value
    pub fn is_bool(&self) -> bool {
        matches!(self, JsonValue::Bool(_))
    }

    /// Check if this is a number value
    pub fn is_number(&self) -> bool {
        matches!(self, JsonValue::Number(_))
    }

    /// Check if this is a string value
    pub fn is_string(&self) -> bool {
        matches!(self, JsonValue::String(_))
    }

    /// Check if this is an array value
    pub fn is_array(&self) -> bool {
        matches!(self, JsonValue::Array(_))
    }

    /// Check if this is an object value
    pub fn is_object(&self) -> bool {
        matches!(self, JsonValue::Object(_))
    }

    /// Check if this is undefined
    pub fn is_undefined(&self) -> bool {
        matches!(self, JsonValue::Undefined)
    }

    /// Get the boolean value if this is a Bool
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Get the number value if this is a Number
    pub fn as_number(&self) -> Option<f64> {
        match self {
            JsonValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Get the string pointer if this is a String
    pub fn as_string(&self) -> Option<GcPtr<RayaString>> {
        match self {
            JsonValue::String(s) => Some(*s),
            _ => None,
        }
    }

    /// Get the array pointer if this is an Array
    pub fn as_array(&self) -> Option<GcPtr<Vec<JsonValue>>> {
        match self {
            JsonValue::Array(arr) => Some(*arr),
            _ => None,
        }
    }

    /// Get the object pointer if this is an Object
    pub fn as_object(&self) -> Option<GcPtr<FxHashMap<String, JsonValue>>> {
        match self {
            JsonValue::Object(obj) => Some(*obj),
            _ => None,
        }
    }

    /// Convert to a boolean following JavaScript truthiness rules
    ///
    /// Falsy values: null, undefined, false, 0, NaN, ""
    /// Everything else is truthy
    pub fn to_bool(&self) -> bool {
        match self {
            JsonValue::Null | JsonValue::Undefined => false,
            JsonValue::Bool(b) => *b,
            JsonValue::Number(n) => *n != 0.0 && !n.is_nan(),
            JsonValue::String(s_ptr) => {
                // Safety: GcPtr guarantees valid pointer
                let s = unsafe { &*s_ptr.as_ptr() };
                !s.is_empty()
            }
            JsonValue::Array(_) | JsonValue::Object(_) => true,
        }
    }

    /// Convert to a number following JavaScript coercion rules
    ///
    /// Returns:
    /// - 0.0 for null
    /// - NaN for undefined
    /// - 0.0 or 1.0 for boolean
    /// - The number itself for number
    /// - Parsed number or NaN for string
    /// - NaN for array/object
    pub fn to_number(&self) -> f64 {
        match self {
            JsonValue::Null => 0.0,
            JsonValue::Undefined => f64::NAN,
            JsonValue::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            JsonValue::Number(n) => *n,
            JsonValue::String(s_ptr) => {
                // Safety: GcPtr guarantees valid pointer
                let s = unsafe { &*s_ptr.as_ptr() };
                s.data.as_str().trim().parse::<f64>().unwrap_or(f64::NAN)
            }
            JsonValue::Array(_) | JsonValue::Object(_) => f64::NAN,
        }
    }

    /// Convert to a string following JavaScript conversion rules
    ///
    /// This is a best-effort conversion that doesn't allocate.
    /// For actual string allocation, use to_raya_string().
    pub fn to_string_static(&self) -> &'static str {
        match self {
            JsonValue::Null => "null",
            JsonValue::Undefined => "undefined",
            JsonValue::Bool(true) => "true",
            JsonValue::Bool(false) => "false",
            JsonValue::Number(_) => "[number]",
            JsonValue::String(_) => "[string]",
            JsonValue::Array(_) => "[array]",
            JsonValue::Object(_) => "[object]",
        }
    }
}

impl fmt::Display for JsonValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonValue::Null => write!(f, "null"),
            JsonValue::Bool(b) => write!(f, "{}", b),
            JsonValue::Number(n) => write!(f, "{}", n),
            JsonValue::String(s_ptr) => {
                // Safety: GcPtr guarantees valid pointer
                let s = unsafe { &*s_ptr.as_ptr() };
                write!(f, "\"{}\"", s.data.as_str())
            }
            JsonValue::Array(_) => write!(f, "[Array]"),
            JsonValue::Object(_) => write!(f, "[Object]"),
            JsonValue::Undefined => write!(f, "undefined"),
        }
    }
}

impl PartialEq for JsonValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (JsonValue::Null, JsonValue::Null) => true,
            (JsonValue::Undefined, JsonValue::Undefined) => true,
            (JsonValue::Bool(a), JsonValue::Bool(b)) => a == b,
            (JsonValue::Number(a), JsonValue::Number(b)) => {
                // Handle NaN specially (NaN != NaN in IEEE 754)
                if a.is_nan() && b.is_nan() {
                    true
                } else {
                    a == b
                }
            }
            (JsonValue::String(a), JsonValue::String(b)) => {
                // Compare string contents
                let a_str = unsafe { &*a.as_ptr() };
                let b_str = unsafe { &*b.as_ptr() };
                a_str.data.as_str() == b_str.data.as_str()
            }
            (JsonValue::Array(a), JsonValue::Array(b)) => {
                // Compare array contents
                let a_vec = unsafe { &*a.as_ptr() };
                let b_vec = unsafe { &*b.as_ptr() };
                a_vec == b_vec
            }
            (JsonValue::Object(a), JsonValue::Object(b)) => {
                // Compare object contents
                let a_map = unsafe { &*a.as_ptr() };
                let b_map = unsafe { &*b.as_ptr() };
                a_map == b_map
            }
            _ => false,
        }
    }
}

impl Eq for JsonValue {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_value_type_name() {
        assert_eq!(JsonValue::Null.type_name(), "null");
        assert_eq!(JsonValue::Bool(true).type_name(), "boolean");
        assert_eq!(JsonValue::Number(42.0).type_name(), "number");
        assert_eq!(JsonValue::Undefined.type_name(), "undefined");
    }

    #[test]
    fn test_json_value_is_checks() {
        assert!(JsonValue::Null.is_null());
        assert!(JsonValue::Bool(true).is_bool());
        assert!(JsonValue::Number(42.0).is_number());
        assert!(JsonValue::Undefined.is_undefined());
    }

    #[test]
    fn test_json_value_as_checks() {
        assert_eq!(JsonValue::Bool(true).as_bool(), Some(true));
        assert_eq!(JsonValue::Number(42.0).as_number(), Some(42.0));
        assert_eq!(JsonValue::Null.as_bool(), None);
    }

    #[test]
    fn test_json_value_to_bool() {
        assert!(!JsonValue::Null.to_bool());
        assert!(!JsonValue::Undefined.to_bool());
        assert!(!JsonValue::Bool(false).to_bool());
        assert!(JsonValue::Bool(true).to_bool());
        assert!(!JsonValue::Number(0.0).to_bool());
        assert!(JsonValue::Number(42.0).to_bool());
    }

    #[test]
    fn test_json_value_to_number() {
        assert_eq!(JsonValue::Null.to_number(), 0.0);
        assert!(JsonValue::Undefined.to_number().is_nan());
        assert_eq!(JsonValue::Bool(false).to_number(), 0.0);
        assert_eq!(JsonValue::Bool(true).to_number(), 1.0);
        assert_eq!(JsonValue::Number(42.5).to_number(), 42.5);
    }

    #[test]
    fn test_json_value_equality() {
        assert_eq!(JsonValue::Null, JsonValue::Null);
        assert_eq!(JsonValue::Bool(true), JsonValue::Bool(true));
        assert_eq!(JsonValue::Number(42.0), JsonValue::Number(42.0));
        assert_ne!(JsonValue::Null, JsonValue::Undefined);
        assert_ne!(JsonValue::Bool(true), JsonValue::Bool(false));
    }

    #[test]
    fn test_json_value_display() {
        assert_eq!(format!("{}", JsonValue::Null), "null");
        assert_eq!(format!("{}", JsonValue::Bool(true)), "true");
        assert_eq!(format!("{}", JsonValue::Number(42.0)), "42");
        assert_eq!(format!("{}", JsonValue::Undefined), "undefined");
    }
}
