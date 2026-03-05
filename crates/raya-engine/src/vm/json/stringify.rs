//! JSON stringifier using js_classify() for dispatch
//!
//! Converts a VM `Value` to a JSON string representation with proper escaping.
//! Uses `js_classify()` as the single dispatch entry point.

use super::view::{js_classify, JSView};
use crate::vm::value::Value;
use crate::vm::{VmError, VmResult};
use std::fmt::Write;

/// Convert a VM `Value` to a JSON string.
///
/// Class metadata for struct serialization is optional.  When not provided
/// (or when the object's class isn't found), typed structs are serialized
/// as `null` (same behaviour as the previous implementation).
pub fn stringify(value: Value) -> VmResult<String> {
    let mut output = String::new();
    stringify_impl(value, &mut output)?;
    Ok(output)
}

/// Internal recursive stringification
fn stringify_impl(value: Value, output: &mut String) -> VmResult<()> {
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
                stringify_impl(*elem, output)?;
            }
            output.push(']');
        }

        JSView::Dyn(ptr) => {
            let obj = unsafe { &*ptr };
            output.push('{');
            let mut first = true;
            for (key, val) in &obj.props {
                if !first {
                    output.push(',');
                }
                first = false;
                output.push('"');
                escape_string(key, output);
                output.push_str("\":");
                stringify_impl(*val, output)?;
            }
            output.push('}');
        }

        JSView::Struct { .. } => {
            // Typed structs require class metadata to enumerate field names.
            // Without it (e.g. in standalone tests), emit null for now.
            // Full struct serialization is handled by the interpreter which
            // passes class metadata via stringify_with_meta().
            output.push_str("null");
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
        match crate::vm::json::view::js_classify(reparsed) {
            crate::vm::json::view::JSView::Dyn(ptr) => {
                let obj = unsafe { &*ptr };
                let age = obj.get("age").expect("age field");
                assert!(
                    age.as_i32() == Some(30)
                        || age
                            .as_f64()
                            .is_some_and(|n| (n - 30.0).abs() < f64::EPSILON),
                    "expected age=30, got {:?}",
                    age
                );
            }
            _ => panic!("Expected DynObject"),
        }
    }
}
