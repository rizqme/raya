//! JSON stringifier using js_classify() for dispatch
//!
//! Converts a VM `Value` to a JSON string representation with proper escaping.
//! Uses `js_classify()` as the single dispatch entry point.

use super::view::{js_classify, JSView};
use crate::vm::object::{global_layout_names, LayoutId, PropKeyId};
use crate::vm::value::Value;
use crate::vm::{VmError, VmResult};
use std::fmt::Write;

/// Convert a VM `Value` to a JSON string.
///
/// Class metadata for struct serialization is optional.  When not provided
/// (or when the object's class isn't found), typed structs are serialized
/// as `null` (same behaviour as the previous implementation).
pub fn stringify(value: Value) -> VmResult<String> {
    stringify_with_runtime_metadata(value, |_| None, |layout_id| global_layout_names(layout_id))
}

/// Convert a VM `Value` to a JSON string using a dynamic-property key resolver.
///
/// This is the runtime path used for unified `Object + dyn_map` carriers, where
/// dynamic property names are stored as interned `PropKeyId`s.
pub fn stringify_with_prop_keys<F>(value: Value, mut resolve_prop_key: F) -> VmResult<String>
where
    F: FnMut(PropKeyId) -> Option<String>,
{
    stringify_with_runtime_metadata(value, &mut resolve_prop_key, |_| None)
}

/// Convert a VM `Value` to a JSON string using both dynamic-property and
/// structural-layout resolvers from the runtime.
pub fn stringify_with_runtime_metadata<FP, FL>(
    value: Value,
    mut resolve_prop_key: FP,
    mut resolve_layout_names: FL,
) -> VmResult<String>
where
    FP: FnMut(PropKeyId) -> Option<String>,
    FL: FnMut(LayoutId) -> Option<Vec<String>>,
{
    let mut output = String::new();
    stringify_impl(
        value,
        &mut output,
        &mut resolve_prop_key,
        &mut resolve_layout_names,
    )?;
    Ok(output)
}

/// Internal recursive stringification
fn stringify_impl<FP, FL>(
    value: Value,
    output: &mut String,
    resolve_prop_key: &mut FP,
    resolve_layout_names: &mut FL,
) -> VmResult<()>
where
    FP: FnMut(PropKeyId) -> Option<String>,
    FL: FnMut(LayoutId) -> Option<Vec<String>>,
{
    match js_classify(value) {
        JSView::Null => {
            output.push_str("null");
        }

        JSView::Bool(b) => {
            output.push_str(if b { "true" } else { "false" });
        }

        JSView::Int(i) => {
            write!(output, "{}", i).unwrap();
        }

        JSView::Number(n) => {
            if n.is_nan() || n.is_infinite() {
                return Err(VmError::RuntimeError(
                    "Cannot stringify NaN or Infinity".to_string(),
                ));
            }
            write!(output, "{}", n).unwrap();
        }

        JSView::Str(ptr) => {
            let s = unsafe { &*ptr };
            output.push('"');
            escape_string(&s.data, output);
            output.push('"');
        }

        JSView::Arr(ptr) => {
            let arr = unsafe { &*ptr };
            output.push('[');
            for (i, elem) in arr.elements.iter().enumerate() {
                if i > 0 {
                    output.push(',');
                }
                stringify_impl(*elem, output, resolve_prop_key, resolve_layout_names)?;
            }
            output.push(']');
        }

        JSView::Struct { ptr, layout_id, .. } => {
            let obj = unsafe { &*ptr };
            let fixed_names = resolve_layout_names(layout_id);
            if fixed_names.is_some() || obj.dyn_map().is_some() {
                let fixed_names = fixed_names.unwrap_or_default();
                output.push('{');
                let mut first = true;
                for (index, name) in fixed_names.iter().enumerate() {
                    let value = obj.get_field(index).unwrap_or(Value::null());
                    if !first {
                        output.push(',');
                    }
                    first = false;
                    output.push('"');
                    escape_string(name, output);
                    output.push_str("\":");
                    stringify_impl(value, output, resolve_prop_key, resolve_layout_names)?;
                }
                if let Some(dyn_map) = obj.dyn_map() {
                    for (key, val) in dyn_map {
                        let Some(name) = resolve_prop_key(*key) else {
                            continue;
                        };
                        if fixed_names.iter().any(|fixed| fixed == &name) {
                            continue;
                        }
                        if !first {
                            output.push(',');
                        }
                        first = false;
                        output.push('"');
                        escape_string(&name, output);
                        output.push_str("\":");
                        stringify_impl(*val, output, resolve_prop_key, resolve_layout_names)?;
                    }
                }
                output.push('}');
            } else {
                // Without any layout metadata we still cannot enumerate fixed slots.
                output.push_str("null");
            }
        }

        JSView::Other => {
            output.push_str("null");
        }
    }

    Ok(())
}

/// Escape a string for JSON output.
pub fn escape_string(s: &str, output: &mut String) {
    for ch in s.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '/' => output.push_str("\\/"),
            '\x08' => output.push_str("\\b"),
            '\x0C' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            c if c.is_control() => {
                write!(output, "\\u{:04x}", c as u32).unwrap();
            }
            c => output.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::gc::GarbageCollector;
    use crate::vm::json::parser;
    use crate::vm::object::RayaString;

    fn make_string(gc: &mut GarbageCollector, s: &str) -> Value {
        let raya_str = RayaString::new(s.to_string());
        let ptr = gc.allocate(raya_str);
        unsafe { Value::from_ptr(std::ptr::NonNull::new(ptr.as_ptr()).unwrap()) }
    }

    #[test]
    fn test_stringify_null() {
        let result = stringify(Value::null()).unwrap();
        assert_eq!(result, "null");
    }

    #[test]
    fn test_stringify_bool() {
        assert_eq!(stringify(Value::bool(true)).unwrap(), "true");
        assert_eq!(stringify(Value::bool(false)).unwrap(), "false");
    }

    #[test]
    fn test_stringify_integer() {
        assert_eq!(stringify(Value::i32(42)).unwrap(), "42");
    }

    #[test]
    fn test_stringify_number() {
        assert_eq!(stringify(Value::f64(3.14)).unwrap(), "3.14");
    }

    #[test]
    fn test_stringify_string() {
        let mut gc = GarbageCollector::default();
        let v = make_string(&mut gc, "hello");
        assert_eq!(stringify(v).unwrap(), "\"hello\"");
    }

    #[test]
    fn test_stringify_string_escapes() {
        let mut gc = GarbageCollector::default();
        let v = make_string(&mut gc, "hello\nworld\t\"test\"");
        assert_eq!(stringify(v).unwrap(), "\"hello\\nworld\\t\\\"test\\\"\"");
    }

    #[test]
    fn test_stringify_array() {
        let mut gc = GarbageCollector::default();
        let arr = parser::parse("[1, 2, 3]", &mut gc).unwrap();
        assert_eq!(stringify(arr).unwrap(), "[1,2,3]");
    }

    #[test]
    fn test_stringify_empty_array() {
        let mut gc = GarbageCollector::default();
        let arr = parser::parse("[]", &mut gc).unwrap();
        assert_eq!(stringify(arr).unwrap(), "[]");
    }

    #[test]
    fn test_stringify_nan_error() {
        assert!(stringify(Value::f64(f64::NAN)).is_err());
    }

    #[test]
    fn test_stringify_infinity_error() {
        assert!(stringify(Value::f64(f64::INFINITY)).is_err());
    }

    #[test]
    fn test_round_trip() {
        let mut gc = GarbageCollector::default();
        let json_str = r#"{"name":"Alice","age":30}"#;
        let parsed = parser::parse(json_str, &mut gc).unwrap();
        let result = stringify(parsed).unwrap();
        // Re-parse and verify key fields
        let reparsed = parser::parse(&result, &mut gc).unwrap();
        let age_json = crate::vm::json::value_to_json_stack(reparsed).get_property("age");
        let age = crate::vm::json::json_to_value(&age_json, &mut gc);
        assert!(
            age.as_i32() == Some(30)
                || age
                    .as_f64()
                    .is_some_and(|n| (n - 30.0).abs() < f64::EPSILON),
            "expected age=30, got {:?}",
            age
        );
    }
}
