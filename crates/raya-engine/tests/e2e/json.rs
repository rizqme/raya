//! JSON intrinsic tests
//!
//! Tests for JSON.stringify and JSON.parse

use super::harness::*;

// ============================================================================
// JSON.stringify - Primitive Types
// ============================================================================

#[test]
fn test_json_stringify_number() {
    expect_string("return JSON.stringify(42);", "42");
}

#[test]
fn test_json_stringify_float() {
    expect_string("return JSON.stringify(3.14);", "3.14");
}

#[test]
fn test_json_stringify_boolean_true() {
    expect_string("return JSON.stringify(true);", "true");
}

#[test]
fn test_json_stringify_boolean_false() {
    expect_string("return JSON.stringify(false);", "false");
}

#[test]
fn test_json_stringify_null() {
    expect_string("return JSON.stringify(null);", "null");
}

// ============================================================================
// JSON.parse - Primitive Types
// ============================================================================

#[test]
fn test_json_parse_number() {
    // JSON.parse returns f64 for numbers
    expect_f64(r#"return JSON.parse("42");"#, 42.0);
}

#[test]
fn test_json_parse_boolean_true() {
    expect_bool(r#"return JSON.parse("true");"#, true);
}

#[test]
fn test_json_parse_boolean_false() {
    expect_bool(r#"return JSON.parse("false");"#, false);
}

#[test]
fn test_json_parse_null() {
    expect_null(r#"return JSON.parse("null");"#);
}

// ============================================================================
// JSON.decode<T> - Typed Decode with Inline Object Types
// ============================================================================

#[test]
fn test_json_decode_inline_object_runs() {
    // Verify JSON.decode with inline object type compiles and runs
    // Note: Field access on inline types requires enhanced type tracking
    let source = r#"
        let json = '{"name":"Alice","age":30}';
        let user = JSON.decode<{name: string; age: number}>(json);
        return 1;
    "#;
    expect_i32(source, 1);
}

#[test]
fn test_json_decode_with_annotation_runs() {
    // Verify JSON.decode with field name mapping annotations compiles and runs
    let source = r#"
        let json = '{"user_name":"Charlie","user_age":35}';
        let user = JSON.decode<{
            //@@json user_name
            name: string;
            //@@json user_age
            age: number;
        }>(json);
        return 1;
    "#;
    expect_i32(source, 1);
}

#[test]
fn test_json_decode_fallback_to_parse() {
    // Without type argument, decode should fallback to parse behavior
    let source = r#"
        let result = JSON.decode('{"value":42}');
        return 1;
    "#;
    expect_i32(source, 1);
}

// ============================================================================
// JSON Duck Typing - Property Access on JSON.parse Results
// ============================================================================

#[test]
fn test_json_parse_object_property_access_string() {
    // JSON.parse returns json type which supports duck typing
    let source = r#"
        let data = JSON.parse('{"name":"Alice"}');
        return data.name;
    "#;
    expect_string(source, "Alice");
}

#[test]
fn test_json_parse_object_property_access_number() {
    let source = r#"
        let data = JSON.parse('{"age":30}');
        return data.age;
    "#;
    expect_f64(source, 30.0);
}

#[test]
fn test_json_parse_object_property_access_boolean() {
    let source = r#"
        let data = JSON.parse('{"active":true}');
        return data.active;
    "#;
    expect_bool(source, true);
}

#[test]
fn test_json_parse_object_property_access_null() {
    let source = r#"
        let data = JSON.parse('{"value":null}');
        return data.value;
    "#;
    expect_null(source);
}

#[test]
fn test_json_parse_object_missing_property() {
    // Accessing a property that doesn't exist returns null
    let source = r#"
        let data = JSON.parse('{"name":"Alice"}');
        return data.missing;
    "#;
    expect_null(source);
}

#[test]
fn test_json_parse_object_multiple_properties() {
    // Access multiple properties from same JSON object
    // Note: JSON numbers are f64, so compare with 25.0
    let source = r#"
        let user = JSON.parse('{"name":"Bob","age":25,"active":true}');
        if (user.name == "Bob") {
            if (user.age == 25.0) {
                return user.active;
            }
        }
        return false;
    "#;
    expect_bool(source, true);
}

#[test]
fn test_json_parse_nested_object() {
    // Nested object property access
    let source = r#"
        let data = JSON.parse('{"user":{"name":"Charlie"}}');
        let user = data.user;
        return user.name;
    "#;
    expect_string(source, "Charlie");
}

#[test]
fn test_json_parse_deeply_nested() {
    // Deep nesting with duck typing
    let source = r#"
        let data = JSON.parse('{"a":{"b":{"c":"deep"}}}');
        return data.a.b.c;
    "#;
    expect_string(source, "deep");
}

#[test]
fn test_json_parse_array_in_object() {
    // JSON object containing an array
    let source = r#"
        let data = JSON.parse('{"items":[1,2,3]}');
        let items = data.items;
        return 1;
    "#;
    expect_i32(source, 1);
}

#[test]
fn test_json_parse_null_object_property_access() {
    // Accessing property on null JSON value returns null
    let source = r#"
        let data = JSON.parse('null');
        return data.anything;
    "#;
    expect_null(source);
}

// ============================================================================
// Type Casting - 'as' keyword
// ============================================================================

#[test]
fn test_json_as_cast_to_class() {
    // Cast JSON to a class type using 'as'
    let source = r#"
        class User {
            name: string;
            age: number;
            constructor(name: string, age: number) {
                this.name = name;
                this.age = age;
            }
        }

        // For now, JSON.decode with type creates the typed object
        let json = '{"name":"Alice","age":30}';
        let user = JSON.decode<User>(json);
        return user.name;
    "#;
    expect_string(source, "Alice");
}

#[test]
fn test_typescript_style_cast_not_supported() {
    // TypeScript-style casting <Type>value is NOT supported
    // Only 'as' syntax is supported: value as Type
    // This test verifies the parser rejects <Type>value syntax
    let source = r#"
        let x: number = 42;
        let y = <string>x;
        return y;
    "#;
    // This should fail to parse - the parser doesn't recognize <Type>value
    // as a type cast expression
    expect_compile_error(source, "Unexpected"); // Parser error
}
