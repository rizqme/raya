//! End-to-end tests for the std:reflect module
//!
//! Tests verify that reflect methods compile and execute correctly
//! through the NativeCall dispatch pipeline.
//!
//! Notes:
//! - Reflect methods use `number` for opaque Value parameters.
//!   For testing with non-number types (strings, objects, booleans),
//!   we use `__NATIVE_CALL` directly to bypass type checking.
//! - MetadataStore requires object (pointer) targets, not primitives.
//! - Avoid `let x: number = __NATIVE_CALL(...)` when classes are defined
//!   in the same compilation unit (triggers type checker issue). Use
//!   `let x = __NATIVE_CALL(...)` without explicit type annotation instead.
//! - Field access (has/get/set) requires ClassMetadataRegistry to be populated,
//!   which is not yet connected to the e2e compilation pipeline.
//! - Proxy, permissions, and bootstrap handlers are in handlers/reflect.rs
//!   but not yet wired to the nested call dispatch path.

use super::harness::{
    compile_and_run_with_builtins, expect_bool_with_builtins, expect_i32_with_builtins,
};

// ============================================================================
// Import & Smoke Tests
// ============================================================================

#[test]
fn test_reflect_import() {
    let result = compile_and_run_with_builtins(
        r#"
        import reflect from "std:reflect";
        return 1;
    "#,
    );
    assert!(
        result.is_ok(),
        "Reflect should be importable from std:reflect: {:?}",
        result.err()
    );
}

// ============================================================================
// Type Guards (via reflect methods - number args)
// ============================================================================

#[test]
fn test_reflect_is_number_true() {
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        return reflect.isNumber(42);
    "#,
        true,
    );
}

#[test]
fn test_reflect_is_number_zero() {
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        return reflect.isNumber(0);
    "#,
        true,
    );
}

#[test]
fn test_reflect_is_number_float() {
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        return reflect.isNumber(3.14);
    "#,
        true,
    );
}

#[test]
fn test_reflect_is_boolean_with_number() {
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        return reflect.isBoolean(42);
    "#,
        false,
    );
}

#[test]
fn test_reflect_is_string_with_number() {
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        return reflect.isString(42);
    "#,
        false,
    );
}

#[test]
fn test_reflect_is_null_with_number() {
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        return reflect.isNull(42);
    "#,
        false,
    );
}

#[test]
fn test_reflect_is_object_with_number() {
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        return reflect.isObject(42);
    "#,
        false,
    );
}

#[test]
fn test_reflect_is_array_with_number() {
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        return reflect.isArray(42);
    "#,
        false,
    );
}

// ============================================================================
// Type Guards (via __NATIVE_CALL - non-number args)
// ============================================================================

#[test]
fn test_reflect_is_string_true() {
    // REFLECT_IS_STRING = 0x0D50
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        return __NATIVE_CALL(0x0D50, "hello");
    "#,
        true,
    );
}

#[test]
fn test_reflect_is_boolean_true() {
    // REFLECT_IS_BOOLEAN = 0x0D52
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        return __NATIVE_CALL(0x0D52, true);
    "#,
        true,
    );
}

#[test]
fn test_reflect_is_null_true() {
    // REFLECT_IS_NULL = 0x0D53
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        return __NATIVE_CALL(0x0D53, null);
    "#,
        true,
    );
}

#[test]
fn test_reflect_is_object_true() {
    // REFLECT_IS_OBJECT = 0x0D56, test with an actual object
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        class Obj { x: number = 1; }
        let o: Obj = new Obj();
        return __NATIVE_CALL(0x0D56, o);
    "#,
        true,
    );
}

// ============================================================================
// Metadata Operations (using object targets via __NATIVE_CALL)
// ============================================================================

// MetadataStore requires pointer (object) targets. We use __NATIVE_CALL
// to pass class instances directly to the metadata handlers.

#[test]
fn test_reflect_define_and_has_metadata() {
    // REFLECT_DEFINE_METADATA = 0x0D00, REFLECT_HAS_METADATA = 0x0D04
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        class Meta1 { x: number = 0; }
        let target: Meta1 = new Meta1();
        __NATIVE_CALL(0x0D00, "key", 42, target);
        return __NATIVE_CALL(0x0D04, "key", target);
    "#,
        true,
    );
}

#[test]
fn test_reflect_has_metadata_nonexistent() {
    // REFLECT_HAS_METADATA = 0x0D04
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        class Meta2 { x: number = 0; }
        let target: Meta2 = new Meta2();
        return __NATIVE_CALL(0x0D04, "nonexistent", target);
    "#,
        false,
    );
}

#[test]
fn test_reflect_define_and_get_metadata() {
    // REFLECT_DEFINE_METADATA = 0x0D00, REFLECT_GET_METADATA = 0x0D02
    expect_i32_with_builtins(
        r#"
        import reflect from "std:reflect";
        class Meta3 { x: number = 0; }
        let target: Meta3 = new Meta3();
        __NATIVE_CALL(0x0D00, "answer", 42, target);
        return __NATIVE_CALL(0x0D02, "answer", target);
    "#,
        42,
    );
}

#[test]
fn test_reflect_delete_metadata() {
    // REFLECT_DEFINE_METADATA = 0x0D00, REFLECT_DELETE_METADATA = 0x0D08,
    // REFLECT_HAS_METADATA = 0x0D04
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        class Meta4 { x: number = 0; }
        let target: Meta4 = new Meta4();
        __NATIVE_CALL(0x0D00, "key", 100, target);
        __NATIVE_CALL(0x0D08, "key", target);
        return __NATIVE_CALL(0x0D04, "key", target);
    "#,
        false,
    );
}

#[test]
fn test_reflect_delete_metadata_returns_true() {
    // REFLECT_DEFINE_METADATA = 0x0D00, REFLECT_DELETE_METADATA = 0x0D08
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        class Meta5 { x: number = 0; }
        let target: Meta5 = new Meta5();
        __NATIVE_CALL(0x0D00, "key", 100, target);
        return __NATIVE_CALL(0x0D08, "key", target);
    "#,
        true,
    );
}

#[test]
fn test_reflect_delete_metadata_nonexistent() {
    // REFLECT_DELETE_METADATA = 0x0D08
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        class Meta6 { x: number = 0; }
        let target: Meta6 = new Meta6();
        return __NATIVE_CALL(0x0D08, "nonexistent", target);
    "#,
        false,
    );
}

#[test]
fn test_reflect_metadata_overwrite() {
    // REFLECT_DEFINE_METADATA = 0x0D00, REFLECT_GET_METADATA = 0x0D02
    expect_i32_with_builtins(
        r#"
        import reflect from "std:reflect";
        class Meta7 { x: number = 0; }
        let target: Meta7 = new Meta7();
        __NATIVE_CALL(0x0D00, "key", 10, target);
        __NATIVE_CALL(0x0D00, "key", 20, target);
        return __NATIVE_CALL(0x0D02, "key", target);
    "#,
        20,
    );
}

#[test]
fn test_reflect_metadata_different_targets() {
    // Each object is a separate metadata target
    // REFLECT_DEFINE_METADATA = 0x0D00, REFLECT_GET_METADATA = 0x0D02
    expect_i32_with_builtins(
        r#"
        import reflect from "std:reflect";
        class Meta8 { x: number = 0; }
        let t1: Meta8 = new Meta8();
        let t2: Meta8 = new Meta8();
        __NATIVE_CALL(0x0D00, "key", 10, t1);
        __NATIVE_CALL(0x0D00, "key", 20, t2);
        return __NATIVE_CALL(0x0D02, "key", t1);
    "#,
        10,
    );
}

// ============================================================================
// Metadata Property Operations
// ============================================================================

#[test]
fn test_reflect_define_and_has_metadata_prop() {
    // REFLECT_DEFINE_METADATA_PROP = 0x0D01, REFLECT_HAS_METADATA_PROP = 0x0D05
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        class Meta9 { x: number = 0; }
        let target: Meta9 = new Meta9();
        __NATIVE_CALL(0x0D01, "type", 42, target, "name");
        return __NATIVE_CALL(0x0D05, "type", target, "name");
    "#,
        true,
    );
}

#[test]
fn test_reflect_define_and_get_metadata_prop() {
    // REFLECT_DEFINE_METADATA_PROP = 0x0D01, REFLECT_GET_METADATA_PROP = 0x0D03
    expect_i32_with_builtins(
        r#"
        import reflect from "std:reflect";
        class Meta10 { x: number = 0; }
        let target: Meta10 = new Meta10();
        __NATIVE_CALL(0x0D01, "type", 99, target, "field");
        return __NATIVE_CALL(0x0D03, "type", target, "field");
    "#,
        99,
    );
}

// ============================================================================
// Class Introspection (via __NATIVE_CALL)
// ============================================================================

#[test]
fn test_reflect_get_class() {
    // REFLECT_GET_CLASS = 0x0D10, returns class ID (i32 > 0)
    let result = compile_and_run_with_builtins(
        r#"
        import reflect from "std:reflect";
        class Foo { x: number = 42; }
        let obj: Foo = new Foo();
        return __NATIVE_CALL(0x0D10, obj);
    "#,
    );
    match result {
        Ok(value) => {
            let class_id = value.as_i32().expect("Expected i32 class ID");
            assert!(class_id > 0, "Class ID should be positive, got {}", class_id);
        }
        Err(e) => panic!("getClass should work: {}", e),
    }
}

#[test]
fn test_reflect_is_instance_of() {
    // REFLECT_GET_CLASS = 0x0D10, REFLECT_IS_INSTANCE_OF = 0x0D15
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        class Bar { y: number = 10; }
        let obj: Bar = new Bar();
        let classId = __NATIVE_CALL(0x0D10, obj);
        return __NATIVE_CALL(0x0D15, obj, classId);
    "#,
        true,
    );
}

#[test]
fn test_reflect_get_type_info() {
    let result = compile_and_run_with_builtins(
        r#"
        import reflect from "std:reflect";
        return reflect.getTypeInfo(42);
    "#,
    );
    assert!(
        result.is_ok(),
        "getTypeInfo should work: {:?}",
        result.err()
    );
}

// ============================================================================
// Object Inspection
// ============================================================================

#[test]
fn test_reflect_inspect() {
    let result = compile_and_run_with_builtins(
        r#"
        import reflect from "std:reflect";
        return reflect.inspect(42);
    "#,
    );
    assert!(
        result.is_ok(),
        "inspect should work: {:?}",
        result.err()
    );
}

#[test]
fn test_reflect_get_object_id() {
    // REFLECT_GET_OBJECT_ID = 0x0D71
    let result = compile_and_run_with_builtins(
        r#"
        import reflect from "std:reflect";
        class IdTest { x: number = 1; }
        let obj: IdTest = new IdTest();
        return __NATIVE_CALL(0x0D71, obj);
    "#,
    );
    match result {
        Ok(value) => {
            let id = value.as_i32().expect("Expected i32 object ID");
            assert!(id >= 0, "Object ID should be non-negative, got {}", id);
        }
        Err(e) => panic!("getObjectId should work: {}", e),
    }
}

// ============================================================================
// Combined Operations
// ============================================================================

#[test]
fn test_reflect_native_call_typed_boolean_in_if() {
    // Verify __NATIVE_CALL<boolean> result can be used in if conditions
    expect_i32_with_builtins(
        r#"
        import reflect from "std:reflect";
        class TypedTest { x: number = 0; }
        let t: TypedTest = new TypedTest();
        __NATIVE_CALL(0x0D00, "key", 42, t);
        let hasKey: boolean = __NATIVE_CALL<boolean>(0x0D04, "key", t);
        if (hasKey) {
            return 1;
        }
        return 0;
    "#,
        1,
    );
}

#[test]
fn test_reflect_native_call_typed_number() {
    // Verify __NATIVE_CALL<number> result gets proper number type
    expect_i32_with_builtins(
        r#"
        import reflect from "std:reflect";
        class TypedNum { x: number = 0; }
        let t: TypedNum = new TypedNum();
        __NATIVE_CALL(0x0D00, "val", 99, t);
        let val: number = __NATIVE_CALL<number>(0x0D02, "val", t);
        return val;
    "#,
        99,
    );
}

#[test]
fn test_reflect_metadata_workflow() {
    // Define multiple metadata entries on an object, verify retrieval
    // REFLECT_DEFINE_METADATA = 0x0D00, REFLECT_HAS_METADATA = 0x0D04,
    // REFLECT_GET_METADATA = 0x0D02
    expect_i32_with_builtins(
        r#"
        import reflect from "std:reflect";
        class Target { x: number = 0; }
        let t: Target = new Target();
        __NATIVE_CALL(0x0D00, "version", 1, t);
        __NATIVE_CALL(0x0D00, "priority", 5, t);
        return __NATIVE_CALL<number>(0x0D02, "priority", t);
    "#,
        5,
    );
}

#[test]
fn test_reflect_class_introspection_workflow() {
    // Get class ID and verify instance check
    // REFLECT_GET_CLASS = 0x0D10, REFLECT_IS_INSTANCE_OF = 0x0D15
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        class Animal { name: string = "generic"; }
        let a: Animal = new Animal();
        let classId = __NATIVE_CALL(0x0D10, a);
        let isAnimal = __NATIVE_CALL(0x0D15, a, classId);
        return isAnimal;
    "#,
        true,
    );
}
