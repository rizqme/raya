//! Fast JSON stringifier for JsonValue
//!
//! Converts JsonValue to JSON string representation with proper escaping.

use super::JsonValue;
use crate::{VmError, VmResult};
use std::fmt::Write;

/// Convert a JsonValue to a JSON string
///
/// This stringifier:
/// - Properly escapes strings
/// - Handles all JSON value types
/// - Returns error for NaN/Infinity
/// - Converts Undefined to null
pub fn stringify(value: &JsonValue) -> VmResult<String> {
    let mut output = String::new();
    stringify_impl(value, &mut output)?;
    Ok(output)
}

/// Internal recursive stringification
fn stringify_impl(value: &JsonValue, output: &mut String) -> VmResult<()> {
    match value {
        JsonValue::Null | JsonValue::Undefined => {
            output.push_str("null");
        }

        JsonValue::Bool(b) => {
            output.push_str(if *b { "true" } else { "false" });
        }

        JsonValue::Number(n) => {
            if n.is_nan() || n.is_infinite() {
                return Err(VmError::RuntimeError(
                    "Cannot stringify NaN or Infinity".to_string(),
                ));
            }
            // Use Rust's f64 Display which handles formatting nicely
            write!(output, "{}", n).unwrap();
        }

        JsonValue::String(s_ptr) => {
            let s = unsafe { &*s_ptr.as_ptr() };
            output.push('"');
            escape_string(&s.data, output);
            output.push('"');
        }

        JsonValue::Array(arr_ptr) => {
            let arr = unsafe { &*arr_ptr.as_ptr() };
            output.push('[');

            for (i, elem) in arr.iter().enumerate() {
                if i > 0 {
                    output.push(',');
                }
                stringify_impl(elem, output)?;
            }

            output.push(']');
        }

        JsonValue::Object(obj_ptr) => {
            let obj = unsafe { &*obj_ptr.as_ptr() };
            output.push('{');

            let mut first = true;
            for (key, value) in obj.iter() {
                if !first {
                    output.push(',');
                }
                first = false;

                output.push('"');
                escape_string(key, output);
                output.push('"');
                output.push(':');
                stringify_impl(value, output)?;
            }

            output.push('}');
        }
    }

    Ok(())
}

/// Escape a string for JSON
///
/// Escapes: " \ / \b \f \n \r \t and control characters
fn escape_string(s: &str, output: &mut String) {
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
                // Escape control characters as \uXXXX
                write!(output, "\\u{:04x}", c as u32).unwrap();
            }
            c => output.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::GarbageCollector;
    use crate::json::parser;
    use crate::object::RayaString;

    #[test]
    fn test_stringify_null() {
        let value = JsonValue::Null;
        let result = stringify(&value).unwrap();
        assert_eq!(result, "null");
    }

    #[test]
    fn test_stringify_bool() {
        let value = JsonValue::Bool(true);
        let result = stringify(&value).unwrap();
        assert_eq!(result, "true");

        let value = JsonValue::Bool(false);
        let result = stringify(&value).unwrap();
        assert_eq!(result, "false");
    }

    #[test]
    fn test_stringify_number() {
        let value = JsonValue::Number(42.0);
        let result = stringify(&value).unwrap();
        assert_eq!(result, "42");

        let value = JsonValue::Number(3.14);
        let result = stringify(&value).unwrap();
        assert_eq!(result, "3.14");
    }

    #[test]
    fn test_stringify_string() {
        let mut gc = GarbageCollector::default();

        let raya_str = RayaString {
            data: "hello".to_string(),
        };
        let str_ptr = gc.allocate(raya_str);
        let value = JsonValue::String(str_ptr);

        let result = stringify(&value).unwrap();
        assert_eq!(result, "\"hello\"");
    }

    #[test]
    fn test_stringify_string_escapes() {
        let mut gc = GarbageCollector::default();

        let raya_str = RayaString {
            data: "hello\nworld\t\"test\"".to_string(),
        };
        let str_ptr = gc.allocate(raya_str);
        let value = JsonValue::String(str_ptr);

        let result = stringify(&value).unwrap();
        assert_eq!(result, "\"hello\\nworld\\t\\\"test\\\"\"");
    }

    #[test]
    fn test_stringify_array() {
        let mut gc = GarbageCollector::default();

        let arr = vec![
            JsonValue::Number(1.0),
            JsonValue::Number(2.0),
            JsonValue::Number(3.0),
        ];
        let arr_ptr = gc.allocate(arr);
        let value = JsonValue::Array(arr_ptr);

        let result = stringify(&value).unwrap();
        assert_eq!(result, "[1,2,3]");
    }

    #[test]
    fn test_stringify_object() {
        let mut gc = GarbageCollector::default();

        let mut obj = rustc_hash::FxHashMap::default();

        let name_str = RayaString {
            data: "Alice".to_string(),
        };
        let name_ptr = gc.allocate(name_str);
        obj.insert("name".to_string(), JsonValue::String(name_ptr));
        obj.insert("age".to_string(), JsonValue::Number(30.0));

        let obj_ptr = gc.allocate(obj);
        let value = JsonValue::Object(obj_ptr);

        let result = stringify(&value).unwrap();
        // Note: HashMap order is not guaranteed, so we just check it parses back
        assert!(result.contains("\"name\""));
        assert!(result.contains("\"Alice\""));
        assert!(result.contains("\"age\""));
        assert!(result.contains("30"));
    }

    #[test]
    fn test_round_trip() {
        let mut gc = GarbageCollector::default();

        let json_str = r#"{"name":"Alice","age":30,"active":true,"tags":["admin","user"]}"#;

        // Parse
        let parsed = parser::parse(json_str, &mut gc).unwrap();

        // Stringify
        let result = stringify(&parsed).unwrap();

        // Parse again to verify structure (order might differ)
        let reparsed = parser::parse(&result, &mut gc).unwrap();

        // Check values match
        assert_eq!(
            parsed.get_property("name").as_string().map(|s| unsafe {
                let str_ref = &*s.as_ptr();
                str_ref.data.clone()
            }),
            reparsed.get_property("name").as_string().map(|s| unsafe {
                let str_ref = &*s.as_ptr();
                str_ref.data.clone()
            })
        );

        assert_eq!(
            parsed.get_property("age").as_number(),
            reparsed.get_property("age").as_number()
        );
    }

    #[test]
    fn test_stringify_nan_error() {
        let value = JsonValue::Number(f64::NAN);
        let result = stringify(&value);
        assert!(result.is_err());
    }

    #[test]
    fn test_stringify_infinity_error() {
        let value = JsonValue::Number(f64::INFINITY);
        let result = stringify(&value);
        assert!(result.is_err());
    }
}
