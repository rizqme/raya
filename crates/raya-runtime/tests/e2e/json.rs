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
// JSON surface is JS-compatible: stringify + parse only
// ============================================================================

#[test]
fn test_json_decode_is_not_supported() {
    expect_compile_error(
        r#"
        let json = '{"name":"Alice","age":30}';
        let user = JSON.decode<{name: string; age: number}>(json);
        return user;
    "#,
        "does not exist",
    );
}

#[test]
fn test_json_encode_is_not_supported() {
    expect_compile_error(
        r#"
        return JSON.encode({ name: "Alice" });
    "#,
        "does not exist",
    );
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
    // Missing properties behave like JS property access and surface undefined.
    let source = r#"
        let data = JSON.parse('{"name":"Alice"}');
        return typeof data.missing == "undefined";
    "#;
    expect_bool(source, true);
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
    // Accessing a property on null follows JS semantics and throws.
    expect_runtime_error(r#"
        let data = JSON.parse('null');
        return data.anything;
    "#, "Cannot read properties of null");
}

#[test]
fn test_json_parse_then_stringify_round_trip_single_property() {
    expect_string(
        r#"return JSON.stringify(JSON.parse('{"name":"Alice"}'));"#,
        r#"{"name":"Alice"}"#,
    );
}

#[test]
fn test_json_parse_then_stringify_round_trip_empty_object() {
    expect_string(r#"return JSON.stringify(JSON.parse('{}'));"#, "{}");
}

#[test]
fn test_json_parse_structural_cast_projects_dynamic_properties() {
    let source = r#"
        const data = JSON.parse('{"name":"Alice","age":30}');
        const user = data as { name: string; age: number };
        return user.name;
    "#;
    expect_string(source, "Alice");
}

#[test]
fn test_json_parse_nested_structural_cast_projects_dynamic_properties() {
    let source = r#"
        const data = JSON.parse('{"user":{"name":"Charlie"}}');
        const typed = data as { user: { name: string } };
        return typed.user.name;
    "#;
    expect_string(source, "Charlie");
}

#[test]
fn test_json_parse_structural_cast_can_be_spread_into_structural_object() {
    let source = r#"
        let dest = { a: 0 };
        let src = JSON.parse('{"a":2,"b":3}') as { a: number; b: number };
        let merged = { ...dest, ...src };
        return merged.a == 2 && merged.b == 3;
    "#;
    expect_bool(source, true);
}

// ============================================================================
// Primitive Type Regression
// ============================================================================

#[test]
fn test_class_constructor_string_param_stays_primitive() {
    let source = r#"
        class User {
            name: string;

            constructor(name: string) {
                this.name = name;
            }
        }

        return new User("Alice").name;
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
