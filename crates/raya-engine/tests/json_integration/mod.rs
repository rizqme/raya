//! Integration tests for JSON functionality
//!
//! Tests cover:
//! - Parse JSON → access properties
//! - Array indexing
//! - Nested structures
//! - Round-trip parse/stringify
//! - Error handling

#![allow(clippy::approx_constant)]
#![allow(clippy::single_char_add_str)]

use raya_engine::vm::gc::GarbageCollector;
use raya_engine::vm::json::view::{js_classify, JSView};
use raya_engine::vm::json::{parser, stringify};
use raya_engine::vm::object::{
    global_layout_names, layout_id_from_ordered_names, register_global_layout_names, Array,
    Object, RayaString,
};
use raya_engine::vm::value::Value;

// ============================================================================
// Helper functions for working with the new Value-based API
// ============================================================================

/// Returns true if the value is a string.
fn is_string(val: Value) -> bool {
    matches!(js_classify(val), JSView::Str(_))
}

/// Returns true if the value is an array.
fn is_array(val: Value) -> bool {
    matches!(js_classify(val), JSView::Arr(_))
}

/// Returns true if the value is an object.
fn is_object(val: Value) -> bool {
    matches!(js_classify(val), JSView::Struct { .. })
}

/// Returns the numeric value (handles both Int and Number variants).
fn as_number(val: Value) -> Option<f64> {
    match js_classify(val) {
        JSView::Int(i) => Some(i as f64),
        JSView::Number(n) => Some(n),
        _ => None,
    }
}

/// Returns the string data as a Rust String (clones the underlying data).
fn get_string_data(val: Value) -> Option<String> {
    match js_classify(val) {
        JSView::Str(ptr) => Some(unsafe { &*ptr }.data.clone()),
        _ => None,
    }
}

/// Returns a raw pointer to the underlying Array (valid while value is GC-reachable).
fn get_array_ptr(val: Value) -> Option<*const Array> {
    match js_classify(val) {
        JSView::Arr(ptr) => Some(ptr),
        _ => None,
    }
}

/// Returns the property value from an object, or null if not found.
fn get_property(val: Value, key: &str) -> Value {
    match js_classify(val) {
        JSView::Struct { ptr, layout_id, .. } => {
            let obj = unsafe { &*ptr };
            global_layout_names(layout_id)
                .and_then(|field_names| field_names.iter().position(|name| name == key))
                .and_then(|index| obj.get_field(index))
                .unwrap_or(Value::null())
        }
        _ => Value::null(),
    }
}

/// Convenience: collect GC with the given Value as a root.
fn collect_json_with_root(gc: &mut GarbageCollector, value: Value, iterations: usize) {
    gc.add_root(value);
    for _ in 0..iterations {
        gc.collect();
    }
    gc.clear_stack_roots();
}

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
    assert!(is_object(parsed), "Parsed value should be an object");

    // Test string property
    let name_data = get_string_data(get_property(parsed, "name")).expect("name should be string");
    assert_eq!(name_data, "Alice");

    // Test number property
    let age = get_property(parsed, "age");
    assert_eq!(as_number(age), Some(30.0), "age should be 30");

    // Test boolean property
    let active = get_property(parsed, "active");
    assert_eq!(active.as_bool(), Some(true), "active should be true");

    // Test float property
    let balance = get_property(parsed, "balance");
    assert_eq!(
        as_number(balance),
        Some(1234.56),
        "balance should be 1234.56"
    );

    // Test missing property (returns null)
    let missing = get_property(parsed, "missing");
    assert!(missing.is_null(), "Missing property should be null");
}

#[test]
fn test_parse_and_access_array() {
    let mut gc = GarbageCollector::default();

    let json = r#"[10, 20, 30, 40, 50]"#;

    let parsed = parser::parse(json, &mut gc).unwrap();

    // Test array type
    assert!(is_array(parsed), "Parsed value should be an array");

    // Get array pointer and dereference
    let arr_ptr = get_array_ptr(parsed).expect("should be array");
    let arr = unsafe { &*arr_ptr };

    // Test array length
    assert_eq!(arr.len(), 5, "Array should have 5 elements");

    // Test array indexing
    assert_eq!(
        as_number(arr.get(0).unwrap()),
        Some(10.0),
        "arr[0] should be 10"
    );
    assert_eq!(
        as_number(arr.get(1).unwrap()),
        Some(20.0),
        "arr[1] should be 20"
    );
    assert_eq!(
        as_number(arr.get(2).unwrap()),
        Some(30.0),
        "arr[2] should be 30"
    );
    assert_eq!(
        as_number(arr.get(3).unwrap()),
        Some(40.0),
        "arr[3] should be 40"
    );
    assert_eq!(
        as_number(arr.get(4).unwrap()),
        Some(50.0),
        "arr[4] should be 50"
    );
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
    let user = get_property(parsed, "user");
    assert!(is_object(user), "user should be an object");

    let name_data = get_string_data(get_property(user, "name")).expect("name should be string");
    assert_eq!(name_data, "Bob");

    // Test deeply nested object
    let settings = get_property(user, "settings");
    assert!(is_object(settings), "settings should be an object");

    let theme_data =
        get_string_data(get_property(settings, "theme")).expect("theme should be string");
    assert_eq!(theme_data, "dark");

    let notifications = get_property(settings, "notifications");
    assert_eq!(notifications.as_bool(), Some(true));

    // Test array of objects
    let posts = get_property(parsed, "posts");
    assert!(is_array(posts), "posts should be an array");

    let posts_arr_ptr = get_array_ptr(posts).expect("should be array");
    let posts_arr = unsafe { &*posts_arr_ptr };
    assert_eq!(posts_arr.len(), 2);

    // Access first post
    let post1 = posts_arr.get(0).expect("first post exists");
    assert!(is_object(post1));
    let post1_id = get_property(post1, "id");
    assert_eq!(as_number(post1_id), Some(1.0));

    let post1_title =
        get_string_data(get_property(post1, "title")).expect("title should be string");
    assert_eq!(post1_title, "First Post");
}

#[test]
fn test_round_trip_parse_stringify() {
    let mut gc = GarbageCollector::default();

    let original_json = r#"{"name":"Charlie","age":25,"hobbies":["coding","gaming"]}"#;

    // Parse
    let parsed = parser::parse(original_json, &mut gc).unwrap();

    // Stringify
    let stringified = stringify::stringify(parsed).unwrap();

    // Parse again
    let reparsed = parser::parse(&stringified, &mut gc).unwrap();

    // Verify structure is preserved
    let name1 = get_string_data(get_property(parsed, "name")).expect("name should be string");
    let name2 = get_string_data(get_property(reparsed, "name")).expect("name should be string");
    assert_eq!(name1, name2);

    let age1 = as_number(get_property(parsed, "age"));
    let age2 = as_number(get_property(reparsed, "age"));
    assert_eq!(age1, age2);

    let hobbies1_ptr =
        get_array_ptr(get_property(parsed, "hobbies")).expect("hobbies should be array");
    let hobbies2_ptr =
        get_array_ptr(get_property(reparsed, "hobbies")).expect("hobbies should be array");
    let hobbies1_arr = unsafe { &*hobbies1_ptr };
    let hobbies2_arr = unsafe { &*hobbies2_ptr };
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
    // NaN should error
    assert!(stringify::stringify(Value::f64(f64::NAN)).is_err());

    // Infinity should error
    assert!(stringify::stringify(Value::f64(f64::INFINITY)).is_err());

    // Negative infinity should error
    assert!(stringify::stringify(Value::f64(f64::NEG_INFINITY)).is_err());
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
    let unicode_data =
        get_string_data(get_property(parsed, "unicode")).expect("unicode should be string");
    assert_eq!(unicode_data, "Hello 世界 🌍");

    // Test escapes
    let escapes_data =
        get_string_data(get_property(parsed, "escapes")).expect("escapes should be string");
    assert_eq!(escapes_data, "Line 1\nLine 2\tTabbed");

    // Test quotes
    let quotes_data =
        get_string_data(get_property(parsed, "quotes")).expect("quotes should be string");
    assert_eq!(quotes_data, "She said \"hello\"");
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

    assert!(is_array(parsed));
    let arr_ptr = get_array_ptr(parsed).expect("should be array");
    let arr = unsafe { &*arr_ptr };
    assert_eq!(arr.len(), 1000);

    // Verify first and last elements
    let first = arr.get(0).expect("first element exists");
    let first_id = get_property(first, "id");
    assert_eq!(as_number(first_id), Some(0.0));

    let last = arr.get(999).expect("last element exists");
    let last_id = get_property(last, "id");
    assert_eq!(as_number(last_id), Some(999.0));
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
        current = get_property(current, &format!("level_{}", i));
        if i < depth - 1 {
            assert!(is_object(current), "Should be object at level {}", i);
        }
    }

    // The deepest value should be a string
    assert!(is_string(current));
    let deep_data = get_string_data(current).expect("should be string");
    assert_eq!(deep_data, "deep_value");
}

#[test]
fn test_stringify_preserves_types() {
    let mut gc = GarbageCollector::default();

    // Null
    assert_eq!(stringify::stringify(Value::null()).unwrap(), "null");

    // Boolean
    assert_eq!(stringify::stringify(Value::bool(true)).unwrap(), "true");

    // Number (integer — stored as i32 in the VM)
    assert_eq!(stringify::stringify(Value::i32(42)).unwrap(), "42");

    // Number (float)
    let float_stringified = stringify::stringify(Value::f64(3.14)).unwrap();
    assert!(float_stringified.starts_with("3.14"));

    // String
    let str_ptr = gc.allocate(RayaString::new("hello".to_string()));
    let str_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(str_ptr.as_ptr()).unwrap()) };
    assert_eq!(stringify::stringify(str_val).unwrap(), r#""hello""#);

    // Empty array
    let empty_arr = Array {
        type_id: 0,
        elements: vec![],
    };
    let arr_ptr = gc.allocate(empty_arr);
    let arr_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) };
    assert_eq!(stringify::stringify(arr_val).unwrap(), "[]");

    // Empty object
    let empty_layout = layout_id_from_ordered_names(&[]);
    register_global_layout_names(empty_layout, &[]);
    let empty_obj = Object::new_structural(empty_layout, 0);
    let obj_ptr = gc.allocate(empty_obj);
    let obj_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) };
    assert_eq!(stringify::stringify(obj_val).unwrap(), "{}");
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
    assert_eq!(as_number(single_num), Some(42.0));

    let single_str = parser::parse(r#""hello""#, &mut gc).unwrap();
    assert!(is_string(single_str));

    let single_bool = parser::parse("true", &mut gc).unwrap();
    assert_eq!(single_bool.as_bool(), Some(true));

    // Array with mixed types
    let mixed = parser::parse(r#"[1, "two", true, null, {"key": "value"}]"#, &mut gc).unwrap();
    assert!(is_array(mixed));
    let arr_ptr = get_array_ptr(mixed).expect("should be array");
    let arr = unsafe { &*arr_ptr };
    assert_eq!(arr.len(), 5);
    assert_eq!(as_number(arr.get(0).unwrap()), Some(1.0));
    assert!(is_string(arr.get(1).unwrap()));
    assert_eq!(arr.get(2).unwrap().as_bool(), Some(true));
    assert!(arr.get(3).unwrap().is_null());
    assert!(is_object(arr.get(4).unwrap()));
}

#[test]
fn test_stringify_special_characters() {
    let mut gc = GarbageCollector::default();

    // Create string with special characters
    let special = RayaString::new("Line1\nLine2\tTab\rReturn\"Quote\\Backslash".to_string());
    let str_ptr = gc.allocate(special);
    let str_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(str_ptr.as_ptr()).unwrap()) };

    let stringified = stringify::stringify(str_val).unwrap();

    // Should have escaped sequences
    assert!(stringified.contains("\\n"));
    assert!(stringified.contains("\\t"));
    assert!(stringified.contains("\\r"));
    assert!(stringified.contains("\\\""));
    assert!(stringified.contains("\\\\"));

    // Round trip to verify
    let reparsed = parser::parse(&stringified, &mut gc).unwrap();
    let reparsed_data = get_string_data(reparsed).expect("should be string");
    assert_eq!(reparsed_data, "Line1\nLine2\tTab\rReturn\"Quote\\Backslash");
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

    // Trigger multiple collections (Value is Copy, passed as root)
    collect_json_with_root(&mut gc, parsed, 5);

    // Verify structure is still accessible and intact
    assert!(is_object(parsed));
    let name = get_property(parsed, "name");
    assert!(is_string(name));
    let data = get_property(parsed, "data");
    assert!(is_array(data));
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
        json.push_str("}");
    }

    let parsed = parser::parse(&json, &mut gc).unwrap();
    assert!(is_object(parsed));

    // Trigger GC - all nested objects should survive if rooted
    collect_json_with_root(&mut gc, parsed, 1);

    // Navigate to verify structure survived
    let mut current = parsed;
    for i in 0..20 {
        current = get_property(current, &format!("level{}", i));
        if i < 19 {
            assert!(is_object(current) || is_string(current));
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
    assert!(is_array(parsed));

    // Trigger GC multiple times
    collect_json_with_root(&mut gc, parsed, 5);

    // Verify structure is intact
    let arr_ptr = get_array_ptr(parsed).expect("should be array");
    let arr = unsafe { &*arr_ptr };
    assert_eq!(arr.len(), 3);

    for i in 0..3 {
        let obj = arr.get(i).expect("element exists");
        assert!(is_object(obj));
        let id = get_property(obj, "id");
        assert_eq!(as_number(id), Some((i + 1) as f64));

        let items = get_property(obj, "items");
        assert!(is_array(items));
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
    assert!(is_array(parsed));

    // Trigger GC - should handle the large structure
    collect_json_with_root(&mut gc, parsed, 1);

    // Verify structure is intact
    let arr_ptr = get_array_ptr(parsed).expect("should be array");
    let arr = unsafe { &*arr_ptr };
    assert_eq!(arr.len(), 100);

    // Verify first and last elements
    let first = arr.get(0).expect("first element exists");
    assert_eq!(as_number(get_property(first, "id")), Some(0.0));
    let last = arr.get(99).expect("last element exists");
    assert_eq!(as_number(get_property(last, "id")), Some(99.0));
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
    assert!(is_object(parsed));

    // Trigger GC
    collect_json_with_root(&mut gc, parsed, 1);

    // Verify all types survived
    assert!(get_property(parsed, "null_val").is_null());
    assert_eq!(get_property(parsed, "bool_true").as_bool(), Some(true));
    assert_eq!(get_property(parsed, "bool_false").as_bool(), Some(false));
    assert_eq!(as_number(get_property(parsed, "number_int")), Some(42.0));
    assert_eq!(as_number(get_property(parsed, "number_float")), Some(3.14));
    assert!(is_string(get_property(parsed, "string")));
    assert!(is_array(get_property(parsed, "array")));
    assert!(is_object(get_property(parsed, "object")));
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
    assert!(is_object(parsed));

    // Trigger GC
    collect_json_with_root(&mut gc, parsed, 1);

    // Verify all strings are accessible
    assert!(is_string(get_property(parsed, "key1")));
    assert!(is_string(get_property(parsed, "key2")));
    assert!(is_string(get_property(parsed, "key3")));

    let array = get_property(parsed, "array");
    assert!(is_array(array));
    let arr_ptr = get_array_ptr(array).expect("should be array");
    let arr = unsafe { &*arr_ptr };
    assert_eq!(arr.len(), 3);
    assert!(is_string(arr.get(0).unwrap()));
    assert!(is_string(arr.get(1).unwrap()));
    assert!(is_string(arr.get(2).unwrap()));
}
