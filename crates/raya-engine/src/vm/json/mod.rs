//! JSON support: parsing, stringification, type dispatch
//!
//! # Design
//!
//! - `parser::parse()` produces native VM `Value` using `DynObject`/`Array`/`RayaString`
//! - `stringify::stringify()` uses `js_classify()` for dispatch
//! - `JSView` / `js_classify()` are the single dispatch entry point for all type checks
//! - `JsonValue` is kept as a **stack-only** internal type for the `cast` module
//!   (never GC-heap-allocated)

use crate::vm::gc::{GarbageCollector, GcPtr};
use crate::vm::object::{Array, RayaString};
use crate::vm::value::Value;

pub mod cast;
pub mod parser;
pub mod stringify;
pub mod view;

// Re-export key types and functions
pub use cast::{validate_cast, TypeKind, TypeSchema, TypeSchemaRegistry};
pub use view::{js_classify, JSView};

/// Stack-only representation of a JSON value.
///
/// Used internally by `cast.rs` for runtime type validation.
/// **Never** allocated on the GC heap — no `GcPtr<JsonValue>` should exist.
///
/// The `String` and `Array` variants hold `GcPtr` handles pointing to
/// GC-managed objects that must remain reachable via other roots (e.g. the
/// VM value stack) while a `JsonValue` lives on the Rust stack.
#[derive(Debug, Clone)]
pub enum JsonValue {
    /// JSON null / undefined
    Null,

    /// JSON boolean
    Bool(bool),

    /// JSON number (always f64 following JSON spec)
    Number(f64),

    /// JSON string — points to a GC-managed RayaString
    String(GcPtr<RayaString>),

    /// JSON array — points to a GC-managed Array of Values
    Array(GcPtr<Array>),

    /// JSON object — represented as a DynObject value
    ///
    /// We store it as a `Value` (pointer to GcPtr<DynObject>) so that cast.rs
    /// can pass it to property accessors without knowing the concrete type.
    Object(Value),

    /// Undefined (missing property)
    Undefined,
}

impl JsonValue {
    /// Get a property from a JSON object.
    pub fn get_property(&self, key: &str) -> JsonValue {
        match self {
            JsonValue::Object(val) => {
                use view::JSView;
                match js_classify(*val) {
                    JSView::Dyn(ptr) => {
                        let obj = unsafe { &*ptr };
                        match obj.get(key) {
                            Some(v) => value_to_json_stack(v),
                            None => JsonValue::Undefined,
                        }
                    }
                    _ => JsonValue::Undefined,
                }
            }
            _ => JsonValue::Undefined,
        }
    }

    /// Get an element from a JSON array by index.
    pub fn get_index(&self, index: usize) -> JsonValue {
        match self {
            JsonValue::Array(arr_ptr) => {
                let arr = unsafe { &*arr_ptr.as_ptr() };
                match arr.get(index) {
                    Some(v) => value_to_json_stack(v),
                    None => JsonValue::Undefined,
                }
            }
            _ => JsonValue::Undefined,
        }
    }

    pub fn is_null(&self) -> bool { matches!(self, JsonValue::Null) }
    pub fn is_bool(&self) -> bool { matches!(self, JsonValue::Bool(_)) }
    pub fn is_number(&self) -> bool { matches!(self, JsonValue::Number(_)) }
    pub fn is_string(&self) -> bool { matches!(self, JsonValue::String(_)) }
    pub fn is_array(&self) -> bool { matches!(self, JsonValue::Array(_)) }
    pub fn is_object(&self) -> bool { matches!(self, JsonValue::Object(_)) }
    pub fn is_undefined(&self) -> bool { matches!(self, JsonValue::Undefined) }

    /// Length of the array (0 if not an array).
    pub fn array_len(&self) -> usize {
        match self {
            JsonValue::Array(arr_ptr) => unsafe { &*arr_ptr.as_ptr() }.len(),
            _ => 0,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self { JsonValue::Bool(b) => Some(*b), _ => None }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self { JsonValue::Number(n) => Some(*n), _ => None }
    }

    pub fn as_string(&self) -> Option<GcPtr<RayaString>> {
        match self { JsonValue::String(s) => Some(*s), _ => None }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            JsonValue::Null => "null",
            JsonValue::Bool(_) => "boolean",
            JsonValue::Number(_) => "number",
            JsonValue::String(_) => "string",
            JsonValue::Array(_) => "object",
            JsonValue::Object(_) => "object",
            JsonValue::Undefined => "undefined",
        }
    }

    pub fn to_bool(&self) -> bool {
        match self {
            JsonValue::Null | JsonValue::Undefined => false,
            JsonValue::Bool(b) => *b,
            JsonValue::Number(n) => *n != 0.0 && !n.is_nan(),
            JsonValue::String(s_ptr) => !unsafe { &*s_ptr.as_ptr() }.is_empty(),
            JsonValue::Array(_) | JsonValue::Object(_) => true,
        }
    }

    pub fn to_number(&self) -> f64 {
        match self {
            JsonValue::Null => 0.0,
            JsonValue::Undefined => f64::NAN,
            JsonValue::Bool(b) => if *b { 1.0 } else { 0.0 },
            JsonValue::Number(n) => *n,
            JsonValue::String(s_ptr) => {
                let s = unsafe { &*s_ptr.as_ptr() };
                s.data.as_str().trim().parse::<f64>().unwrap_or(f64::NAN)
            }
            JsonValue::Array(_) | JsonValue::Object(_) => f64::NAN,
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
                if a.is_nan() && b.is_nan() { true } else { a == b }
            }
            (JsonValue::String(a), JsonValue::String(b)) => {
                let a_str = unsafe { &*a.as_ptr() };
                let b_str = unsafe { &*b.as_ptr() };
                a_str.data == b_str.data
            }
            _ => false,
        }
    }
}
impl Eq for JsonValue {}

/// Convert a `Value` to a stack-only `JsonValue` (no GC allocation).
///
/// Used by `cast.rs` to inspect parsed JSON values.
/// The returned `JsonValue` borrows GC objects via `GcPtr` handles
/// that must remain reachable while the `JsonValue` is live.
pub fn value_to_json_stack(value: Value) -> JsonValue {
    use view::JSView;
    match js_classify(value) {
        JSView::Null => JsonValue::Null,
        JSView::Bool(b) => JsonValue::Bool(b),
        JSView::Int(i) => JsonValue::Number(i as f64),
        JSView::Number(n) => JsonValue::Number(n),
        JSView::Str(ptr) => {
            let gc_ptr = unsafe { GcPtr::new(std::ptr::NonNull::new(ptr as *mut RayaString).unwrap()) };
            JsonValue::String(gc_ptr)
        }
        JSView::Arr(ptr) => {
            let gc_ptr = unsafe { GcPtr::new(std::ptr::NonNull::new(ptr as *mut Array).unwrap()) };
            JsonValue::Array(gc_ptr)
        }
        JSView::Dyn(_) => JsonValue::Object(value),
        JSView::Struct { .. } => JsonValue::Object(value),
        JSView::Other => JsonValue::Null,
    }
}

/// Convert a parsed `Value` (produced by `parser::parse()`) to a `Value`.
///
/// Since `parser::parse()` already returns a native `Value`, this is now
/// a no-op identity function kept for call-site compatibility.
#[inline]
pub fn json_to_value(json: &JsonValue, gc: &mut GarbageCollector) -> Value {
    match json {
        JsonValue::Null | JsonValue::Undefined => Value::null(),
        JsonValue::Bool(b) => Value::bool(*b),
        JsonValue::Number(n) => Value::f64(*n),
        JsonValue::String(s_ptr) => {
            unsafe { Value::from_ptr(std::ptr::NonNull::new(s_ptr.as_ptr()).unwrap()) }
        }
        JsonValue::Array(arr_ptr) => {
            unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
        }
        JsonValue::Object(val) => *val,
    }
}

/// Convert a VM `Value` to a `JsonValue` for use by `cast.rs`.
///
/// Does NOT allocate on the GC heap.
#[inline]
pub fn value_to_json(value: Value, _gc: &mut GarbageCollector) -> JsonValue {
    value_to_json_stack(value)
}

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
}
