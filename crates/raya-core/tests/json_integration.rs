//! Integration tests for JSON functionality
//!
//! Tests cover:
//! - Parse JSON → access properties
//! - Array indexing

#![allow(clippy::approx_constant)]
#![allow(clippy::single_char_add_str)]
//! - Nested structures
//! - Round-trip parse/stringify
//! - Error handling

use raya_core::gc::GarbageCollector;
use raya_core::json::{parser, stringify};
use raya_core::object::RayaString;

// ============================================================================
// Runtime JSON Tests (parser, stringify, property access)
// ============================================================================

#[test]
fn test_parse_and_access_properties() {
    let mut gc = GarbageCollector::default();

    let json = r#"{
        "name": "Alice",
        "age": 30,
        "active": true,
        "balance": 1234.56
    }"#;

    let parsed = parser::parse(json, &mut gc).unwrap();

    // Test object type
    assert!(parsed.is_object(), "Parsed value should be an object");

    // Test string property
    let name = parsed.get_property("name");
    assert!(name.is_string(), "name should be a string");
    let name_str = name.as_string().unwrap();
    let name_data = unsafe { &*name_str.as_ptr() };
    assert_eq!(name_data.data, "Alice");

    // Test number property
    let age = parsed.get_property("age");
    assert_eq!(age.as_number(), Some(30.0), "age should be 30");

    // Test boolean property
    let active = parsed.get_property("active");
    assert_eq!(active.as_bool(), Some(true), "active should be true");

    // Test float property
    let balance = parsed.get_property("balance");
    assert_eq!(
        balance.as_number(),
        Some(1234.56),
        "balance should be 1234.56"
    );

    // Test missing property
    let missing = parsed.get_property("missing");
    assert!(
        missing.is_undefined(),
        "Missing property should be undefined"
    );
}

#[test]
fn test_parse_and_access_array() {
    let mut gc = GarbageCollector::default();

    let json = r#"[10, 20, 30, 40, 50]"#;

    let parsed = parser::parse(json, &mut gc).unwrap();

    // Test array type
    assert!(parsed.is_array(), "Parsed value should be an array");

    // Get array pointer and dereference
    let arr_ptr = parsed.as_array().unwrap();
    let arr = unsafe { &*arr_ptr.as_ptr() };

    // Test array length
    assert_eq!(arr.len(), 5, "Array should have 5 elements");

    // Test array indexing
    assert_eq!(arr[0].as_number(), Some(10.0), "arr[0] should be 10");
    assert_eq!(arr[1].as_number(), Some(20.0), "arr[1] should be 20");
    assert_eq!(arr[2].as_number(), Some(30.0), "arr[2] should be 30");
    assert_eq!(arr[3].as_number(), Some(40.0), "arr[3] should be 40");
    assert_eq!(arr[4].as_number(), Some(50.0), "arr[4] should be 50");
}

#[test]
fn test_parse_nested_structures() {
    let mut gc = GarbageCollector::default();

    let json = r#"{
        "user": {
            "name": "Bob",
            "email": "bob@example.com",
            "settings": {
                "theme": "dark",
                "notifications": true
            }
        },
        "posts": [
            {"id": 1, "title": "First Post"},
            {"id": 2, "title": "Second Post"}
        ]
    }"#;

    let parsed = parser::parse(json, &mut gc).unwrap();

    // Test nested object access
    let user = parsed.get_property("user");
    assert!(user.is_object(), "user should be an object");

    let name = user.get_property("name");
    let name_str = name.as_string().unwrap();
    let name_data = unsafe { &*name_str.as_ptr() };
    assert_eq!(name_data.data, "Bob");

    // Test deeply nested object
    let settings = user.get_property("settings");
    assert!(settings.is_object(), "settings should be an object");

    let theme = settings.get_property("theme");
    let theme_str = theme.as_string().unwrap();
    let theme_data = unsafe { &*theme_str.as_ptr() };
    assert_eq!(theme_data.data, "dark");

    let notifications = settings.get_property("notifications");
    assert_eq!(notifications.as_bool(), Some(true));

    // Test array of objects
    let posts = parsed.get_property("posts");
    assert!(posts.is_array(), "posts should be an array");

    let posts_arr_ptr = posts.as_array().unwrap();
    let posts_arr = unsafe { &*posts_arr_ptr.as_ptr() };
    assert_eq!(posts_arr.len(), 2);

    // Access first post
    let post1 = &posts_arr[0];
    assert!(post1.is_object());
    let post1_id = post1.get_property("id");
    assert_eq!(post1_id.as_number(), Some(1.0));

    let post1_title = post1.get_property("title");
    let post1_title_str = post1_title.as_string().unwrap();
    let post1_title_data = unsafe { &*post1_title_str.as_ptr() };
    assert_eq!(post1_title_data.data, "First Post");
}

#[test]
fn test_round_trip_parse_stringify() {
    let mut gc = GarbageCollector::default();

    let original_json = r#"{"name":"Charlie","age":25,"hobbies":["coding","gaming"]}"#;

    // Parse
    let parsed = parser::parse(original_json, &mut gc).unwrap();

    // Stringify
    let stringified = stringify::stringify(&parsed).unwrap();

    // Parse again
    let reparsed = parser::parse(&stringified, &mut gc).unwrap();

    // Verify structure is preserved
    let name1 = parsed.get_property("name").as_string().unwrap();
    let name2 = reparsed.get_property("name").as_string().unwrap();
    let name1_data = unsafe { &*name1.as_ptr() };
    let name2_data = unsafe { &*name2.as_ptr() };
    assert_eq!(name1_data.data, name2_data.data);

    let age1 = parsed.get_property("age").as_number();
    let age2 = reparsed.get_property("age").as_number();
    assert_eq!(age1, age2);

    let hobbies1 = parsed.get_property("hobbies").as_array().unwrap();
    let hobbies2 = reparsed.get_property("hobbies").as_array().unwrap();
    let hobbies1_arr = unsafe { &*hobbies1.as_ptr() };
    let hobbies2_arr = unsafe { &*hobbies2.as_ptr() };
    assert_eq!(hobbies1_arr.len(), hobbies2_arr.len());
}

#[test]
fn test_parse_error_invalid_json() {
    let mut gc = GarbageCollector::default();

    // Missing closing brace
    let invalid1 = r#"{"name": "test""#;
    assert!(parser::parse(invalid1, &mut gc).is_err());

    // Invalid number
    let invalid2 = r#"{"value": 12.34.56}"#;
    assert!(parser::parse(invalid2, &mut gc).is_err());

    // Trailing comma
    let invalid3 = r#"{"name": "test",}"#;
    assert!(parser::parse(invalid3, &mut gc).is_err());

    // Invalid escape sequence
    let invalid5 = r#"{"name": "test\x"}"#;
    assert!(parser::parse(invalid5, &mut gc).is_err());
}

#[test]
fn test_stringify_error_nan_infinity() {
    use raya_core::json::JsonValue;

    // NaN should error
    let nan_value = JsonValue::Number(f64::NAN);
    assert!(stringify::stringify(&nan_value).is_err());

    // Infinity should error
    let inf_value = JsonValue::Number(f64::INFINITY);
    assert!(stringify::stringify(&inf_value).is_err());

    // Negative infinity should error
    let neg_inf_value = JsonValue::Number(f64::NEG_INFINITY);
    assert!(stringify::stringify(&neg_inf_value).is_err());
}

#[test]
fn test_parse_unicode_and_escapes() {
    let mut gc = GarbageCollector::default();

    let json = r#"{
        "unicode": "Hello 世界 🌍",
        "escapes": "Line 1\nLine 2\tTabbed",
        "quotes": "She said \"hello\""
    }"#;

    let parsed = parser::parse(json, &mut gc).unwrap();

    // Test unicode
    let unicode = parsed.get_property("unicode");
    let unicode_str = unicode.as_string().unwrap();
    let unicode_data = unsafe { &*unicode_str.as_ptr() };
    assert_eq!(unicode_data.data, "Hello 世界 🌍");

    // Test escapes
    let escapes = parsed.get_property("escapes");
    let escapes_str = escapes.as_string().unwrap();
    let escapes_data = unsafe { &*escapes_str.as_ptr() };
    assert_eq!(escapes_data.data, "Line 1\nLine 2\tTabbed");

    // Test quotes
    let quotes = parsed.get_property("quotes");
    let quotes_str = quotes.as_string().unwrap();
    let quotes_data = unsafe { &*quotes_str.as_ptr() };
    assert_eq!(quotes_data.data, "She said \"hello\"");
}

#[test]
fn test_parse_large_json() {
    let mut gc = GarbageCollector::default();

    // Build a large JSON array
    let mut json = String::from("[");
    for i in 0..1000 {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(r#"{{"id":{}, "value":"item_{}"}}"#, i, i));
    }
    json.push(']');

    let parsed = parser::parse(&json, &mut gc).unwrap();

    assert!(parsed.is_array());
    let arr_ptr = parsed.as_array().unwrap();
    let arr = unsafe { &*arr_ptr.as_ptr() };
    assert_eq!(arr.len(), 1000);

    // Verify first and last elements
    let first = &arr[0];
    let first_id = first.get_property("id");
    assert_eq!(first_id.as_number(), Some(0.0));

    let last = &arr[999];
    let last_id = last.get_property("id");
    assert_eq!(last_id.as_number(), Some(999.0));
}

#[test]
fn test_parse_deeply_nested() {
    let mut gc = GarbageCollector::default();

    // Create deeply nested object
    let mut json = String::new();
    let depth = 50;

    for i in 0..depth {
        json.push_str(&format!(r#"{{"level_{}":"#, i));
    }

    json.push_str(r#""deep_value""#);

    for _ in 0..depth {
        json.push_str("}");
    }

    let parsed = parser::parse(&json, &mut gc).unwrap();

    // Navigate to the deep value
    let mut current = parsed;
    for i in 0..depth {
        current = current.get_property(&format!("level_{}", i));
        if i < depth - 1 {
            assert!(current.is_object(), "Should be object at level {}", i);
        }
    }

    // The deepest value should be a string
    assert!(current.is_string());
    let deep_str = current.as_string().unwrap();
    let deep_data = unsafe { &*deep_str.as_ptr() };
    assert_eq!(deep_data.data, "deep_value");
}

#[test]
fn test_stringify_preserves_types() {
    use raya_core::json::JsonValue;
    let mut gc = GarbageCollector::default();

    // Null
    let null_val = JsonValue::Null;
    assert_eq!(stringify::stringify(&null_val).unwrap(), "null");

    // Boolean
    let bool_val = JsonValue::Bool(true);
    assert_eq!(stringify::stringify(&bool_val).unwrap(), "true");

    // Number (integer)
    let int_val = JsonValue::Number(42.0);
    assert_eq!(stringify::stringify(&int_val).unwrap(), "42");

    // Number (float)
    let float_val = JsonValue::Number(3.14);
    let stringified = stringify::stringify(&float_val).unwrap();
    assert!(stringified.starts_with("3.14"));

    // String
    let str_val = JsonValue::String(gc.allocate(RayaString::new("hello".to_string())));
    assert_eq!(stringify::stringify(&str_val).unwrap(), r#""hello""#);

    // Empty array
    let empty_arr = JsonValue::Array(gc.allocate(vec![]));
    assert_eq!(stringify::stringify(&empty_arr).unwrap(), "[]");

    // Empty object
    let empty_obj = JsonValue::Object(gc.allocate(rustc_hash::FxHashMap::default()));
    assert_eq!(stringify::stringify(&empty_obj).unwrap(), "{}");
}

#[test]
fn test_parse_edge_cases() {
    let mut gc = GarbageCollector::default();

    // Empty string should error
    assert!(parser::parse("", &mut gc).is_err());

    // Whitespace only
    let whitespace = parser::parse("   \n\t  null  \n  ", &mut gc).unwrap();
    assert!(whitespace.is_null());

    // Single value (not object or array)
    let single_num = parser::parse("42", &mut gc).unwrap();
    assert_eq!(single_num.as_number(), Some(42.0));

    let single_str = parser::parse(r#""hello""#, &mut gc).unwrap();
    assert!(single_str.is_string());

    let single_bool = parser::parse("true", &mut gc).unwrap();
    assert_eq!(single_bool.as_bool(), Some(true));

    // Array with mixed types
    let mixed = parser::parse(r#"[1, "two", true, null, {"key": "value"}]"#, &mut gc).unwrap();
    assert!(mixed.is_array());
    let mixed_arr_ptr = mixed.as_array().unwrap();
    let mixed_arr = unsafe { &*mixed_arr_ptr.as_ptr() };
    assert_eq!(mixed_arr.len(), 5);
    assert_eq!(mixed_arr[0].as_number(), Some(1.0));
    assert!(mixed_arr[1].is_string());
    assert_eq!(mixed_arr[2].as_bool(), Some(true));
    assert!(mixed_arr[3].is_null());
    assert!(mixed_arr[4].is_object());
}

#[test]
fn test_stringify_special_characters() {
    let mut gc = GarbageCollector::default();

    // Create string with special characters
    let special = RayaString::new("Line1\nLine2\tTab\rReturn\"Quote\\Backslash".to_string());
    let str_ptr = gc.allocate(special);
    let json_val = raya_core::json::JsonValue::String(str_ptr);

    let stringified = stringify::stringify(&json_val).unwrap();

    // Should have escaped sequences
    assert!(stringified.contains("\\n"));
    assert!(stringified.contains("\\t"));
    assert!(stringified.contains("\\r"));
    assert!(stringified.contains("\\\""));
    assert!(stringified.contains("\\\\"));

    // Round trip to verify
    let reparsed = parser::parse(&stringified, &mut gc).unwrap();
    let reparsed_str = reparsed.as_string().unwrap();
    let reparsed_data = unsafe { &*reparsed_str.as_ptr() };
    assert_eq!(
        reparsed_data.data,
        "Line1\nLine2\tTab\rReturn\"Quote\\Backslash"
    );
}

// ============================================================================
// GC Integration Tests for JSON
// ============================================================================

#[test]
fn test_json_gc_survival_simple() {
    // Test that JSON values survive garbage collection
    let mut gc = GarbageCollector::default();

    // Parse JSON - strings and containers are GC-allocated
    let json = r#"{"name":"Alice","data":[1,2,3]}"#;
    let parsed = parser::parse(json, &mut gc).unwrap();

    // Trigger multiple collections
    for _ in 0..5 {
        gc.collect();
    }

    // Verify structure is still accessible and intact
    assert!(parsed.is_object());
    let name = parsed.get_property("name");
    assert!(name.is_string());
    let data = parsed.get_property("data");
    assert!(data.is_array());
}

#[test]
fn test_json_gc_nested_structures() {
    // Test GC with deeply nested JSON structures
    let mut gc = GarbageCollector::default();

    // Create a deeply nested structure
    let mut json = String::from(r#"{"level0":"#);
    for i in 1..20 {
        json.push_str(&format!(r#"{{"level{}":"#, i));
    }
    json.push_str(r#""deep""#);
    for _ in 0..20 {
        json.push_str("}}");
    }

    let parsed = parser::parse(&json, &mut gc).unwrap();
    assert!(parsed.is_object());

    // Trigger GC - all nested objects should survive if rooted
    gc.collect();

    // Navigate to verify structure survived
    let mut current = parsed;
    for i in 0..20 {
        current = current.get_property(&format!("level{}", i));
        if i < 19 {
            assert!(current.is_object() || current.is_string());
        }
    }
}

#[test]
fn test_json_gc_array_of_objects() {
    // Test GC with JSON arrays containing complex objects
    let mut gc = GarbageCollector::default();

    let json = r#"[
        {"id": 1, "items": [1, 2, 3]},
        {"id": 2, "items": [4, 5, 6]},
        {"id": 3, "items": [7, 8, 9]}
    ]"#;

    let parsed = parser::parse(json, &mut gc).unwrap();
    assert!(parsed.is_array());

    // Trigger GC multiple times
    for _ in 0..5 {
        gc.collect();
    }

    // Verify structure is intact
    let arr_ptr = parsed.as_array().unwrap();
    let arr = unsafe { &*arr_ptr.as_ptr() };
    assert_eq!(arr.len(), 3);

    for i in 0..3 {
        let obj = &arr[i];
        assert!(obj.is_object());
        let id = obj.get_property("id");
        assert_eq!(id.as_number(), Some((i + 1) as f64));

        let items = obj.get_property("items");
        assert!(items.is_array());
    }
}

#[test]
fn test_json_gc_large_allocation() {
    // Test that large JSON structures can be allocated and collected
    let mut gc = GarbageCollector::default();

    // Build large JSON array (100 objects with nested arrays)
    let mut json = String::from("[");
    for i in 0..100 {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(r#"{{"id":{},"data":[1,2,3,4,5,6,7,8,9,10]}}"#, i));
    }
    json.push(']');

    let parsed = parser::parse(&json, &mut gc).unwrap();
    assert!(parsed.is_array());

    // Trigger GC - should handle the large structure
    gc.collect();

    // Verify structure is intact
    let arr_ptr = parsed.as_array().unwrap();
    let arr = unsafe { &*arr_ptr.as_ptr() };
    assert_eq!(arr.len(), 100);

    // Verify first and last elements
    let first = &arr[0];
    assert_eq!(first.get_property("id").as_number(), Some(0.0));
    let last = &arr[99];
    assert_eq!(last.get_property("id").as_number(), Some(99.0));
}

#[test]
fn test_json_gc_mixed_types() {
    // Test GC with JSON containing all value types
    let mut gc = GarbageCollector::default();

    let json = r#"{
        "null_val": null,
        "bool_true": true,
        "bool_false": false,
        "number_int": 42,
        "number_float": 3.14,
        "string": "hello world",
        "array": [1, 2, 3],
        "object": {"nested": "value"}
    }"#;

    let parsed = parser::parse(json, &mut gc).unwrap();
    assert!(parsed.is_object());

    // Trigger GC
    gc.collect();

    // Verify all types survived
    assert!(parsed.get_property("null_val").is_null());
    assert_eq!(parsed.get_property("bool_true").as_bool(), Some(true));
    assert_eq!(parsed.get_property("bool_false").as_bool(), Some(false));
    assert_eq!(parsed.get_property("number_int").as_number(), Some(42.0));
    assert_eq!(parsed.get_property("number_float").as_number(), Some(3.14));
    assert!(parsed.get_property("string").is_string());
    assert!(parsed.get_property("array").is_array());
    assert!(parsed.get_property("object").is_object());
}

#[test]
fn test_json_gc_string_deduplication() {
    // Test that multiple identical strings can be GC'd correctly
    let mut gc = GarbageCollector::default();

    let json = r#"{
        "key1": "hello",
        "key2": "hello",
        "key3": "hello",
        "array": ["hello", "hello", "hello"]
    }"#;

    let parsed = parser::parse(json, &mut gc).unwrap();
    assert!(parsed.is_object());

    // Trigger GC
    gc.collect();

    // Verify all strings are accessible
    let key1 = parsed.get_property("key1");
    let key2 = parsed.get_property("key2");
    let key3 = parsed.get_property("key3");
    assert!(key1.is_string());
    assert!(key2.is_string());
    assert!(key3.is_string());

    let array = parsed.get_property("array");
    assert!(array.is_array());
    let arr_ptr = array.as_array().unwrap();
    let arr = unsafe { &*arr_ptr.as_ptr() };
    assert_eq!(arr.len(), 3);
    assert!(arr[0].is_string());
    assert!(arr[1].is_string());
    assert!(arr[2].is_string());
}
