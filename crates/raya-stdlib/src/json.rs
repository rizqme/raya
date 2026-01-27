//! JSON parsing and stringification implementation
//!
//! This module provides native implementations of JSON.parse() and JSON.stringify()
//! using Raya's custom high-performance JSON parser.

use raya_core::gc::GarbageCollector;
use raya_core::json::JsonValue;
use raya_core::object::RayaString;
use raya_core::VmResult;

/// Parse a JSON string into a JsonValue
///
/// This function uses Raya's custom JSON parser that directly creates
/// JsonValue with GC-managed allocations, avoiding intermediate representations.
///
/// # Arguments
///
/// * `json_str` - The JSON string to parse
/// * `gc` - Garbage collector for allocating heap objects
///
/// # Returns
///
/// A JsonValue on success, or a VmError on parse failure
///
/// # Example
///
/// ```raya
/// let data = JSON.parse('{"name": "Alice", "age": 30}');
/// console.log(data.name);  // "Alice"
/// ```
pub fn parse(json_str: &RayaString, gc: &mut GarbageCollector) -> VmResult<JsonValue> {
    raya_core::json::parser::parse(&json_str.data, gc)
}

/// Convert a JsonValue to a JSON string
///
/// This function uses Raya's custom JSON stringifier that efficiently
/// converts JsonValue to JSON string representation.
///
/// # Arguments
///
/// * `json_value` - The JsonValue to stringify
/// * `gc` - Garbage collector (unused but kept for API consistency)
///
/// # Returns
///
/// A RayaString containing the JSON representation
///
/// # Example
///
/// ```raya
/// let obj: json = { name: "Alice", age: 30 };
/// let str = JSON.stringify(obj);  // '{"name":"Alice","age":30}'
/// ```
pub fn stringify(json_value: &JsonValue, _gc: &mut GarbageCollector) -> VmResult<RayaString> {
    let json_str = raya_core::json::stringify::stringify(json_value)?;
    Ok(RayaString::new(json_str))
}

#[cfg(test)]
mod tests {
    use super::*;
    use raya_core::gc::GarbageCollector;

    #[test]
    fn test_parse_primitives() {
        let mut gc = GarbageCollector::default();

        // Null
        let input = RayaString::new("null".to_string());
        let result = parse(&input, &mut gc).unwrap();
        assert!(result.is_null());

        // Boolean
        let input = RayaString::new("true".to_string());
        let result = parse(&input, &mut gc).unwrap();
        assert_eq!(result.as_bool(), Some(true));

        // Number
        let input = RayaString::new("42.5".to_string());
        let result = parse(&input, &mut gc).unwrap();
        assert_eq!(result.as_number(), Some(42.5));

        // String
        let input = RayaString::new("\"hello\"".to_string());
        let result = parse(&input, &mut gc).unwrap();
        assert!(result.is_string());
    }

    #[test]
    fn test_parse_array() {
        let mut gc = GarbageCollector::default();

        let input = RayaString::new("[1, 2, 3]".to_string());
        let result = parse(&input, &mut gc).unwrap();

        assert!(result.is_array());
        let arr_ptr = result.as_array().unwrap();
        let arr = unsafe { &*arr_ptr.as_ptr() };
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0].as_number(), Some(1.0));
        assert_eq!(arr[1].as_number(), Some(2.0));
        assert_eq!(arr[2].as_number(), Some(3.0));
    }

    #[test]
    fn test_parse_object() {
        let mut gc = GarbageCollector::default();

        let input = RayaString::new("{\"name\": \"Alice\", \"age\": 30}".to_string());
        let result = parse(&input, &mut gc).unwrap();

        assert!(result.is_object());

        let name = result.get_property("name");
        assert!(name.is_string());

        let age = result.get_property("age");
        assert_eq!(age.as_number(), Some(30.0));
    }

    #[test]
    fn test_stringify_primitives() {
        let mut gc = GarbageCollector::default();

        // Null
        let value = JsonValue::Null;
        let result = stringify(&value, &mut gc).unwrap();
        assert_eq!(result.data, "null");

        // Boolean
        let value = JsonValue::Bool(true);
        let result = stringify(&value, &mut gc).unwrap();
        assert_eq!(result.data, "true");

        // Number
        let value = JsonValue::Number(42.5);
        let result = stringify(&value, &mut gc).unwrap();
        assert_eq!(result.data, "42.5");
    }

    #[test]
    fn test_round_trip() {
        let mut gc = GarbageCollector::default();

        let json_str = "{\"name\":\"Alice\",\"age\":30,\"active\":true}";
        let input = RayaString::new(json_str.to_string());

        // Parse
        let parsed = parse(&input, &mut gc).unwrap();

        // Stringify
        let result = stringify(&parsed, &mut gc).unwrap();

        // Parse again to compare structure
        let reparsed_input = RayaString::new(result.data.clone());
        let reparsed = parse(&reparsed_input, &mut gc).unwrap();

        // Verify structure
        assert_eq!(
            parsed.get_property("age").as_number(),
            reparsed.get_property("age").as_number()
        );
        assert_eq!(
            parsed.get_property("active").as_bool(),
            reparsed.get_property("active").as_bool()
        );
    }

    #[test]
    fn test_parse_error() {
        let mut gc = GarbageCollector::default();

        let input = RayaString::new("{invalid json}".to_string());
        let result = parse(&input, &mut gc);

        assert!(result.is_err());
    }

    #[test]
    fn test_complex_nested() {
        let mut gc = GarbageCollector::default();

        let json_str = r#"
        {
            "users": [
                {"name": "Alice", "role": "admin"},
                {"name": "Bob", "role": "user"}
            ],
            "count": 2,
            "metadata": {
                "version": "1.0",
                "timestamp": 1234567890
            }
        }
        "#;

        let input = RayaString::new(json_str.to_string());

        // Parse
        let parsed = parse(&input, &mut gc).unwrap();
        assert!(parsed.is_object());

        // Stringify
        let result = stringify(&parsed, &mut gc).unwrap();

        // Verify it can be parsed again
        let reparsed_input = RayaString::new(result.data.clone());
        let reparsed = parse(&reparsed_input, &mut gc).unwrap();

        // Check structure
        assert!(reparsed.get_property("users").is_array());
        assert_eq!(reparsed.get_property("count").as_number(), Some(2.0));
        assert!(reparsed.get_property("metadata").is_object());
    }
}
