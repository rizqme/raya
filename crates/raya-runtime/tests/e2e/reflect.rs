//! End-to-end tests for the std:reflect module
//!
//! Tests verify that reflect methods compile and execute correctly
//! through the kernel/vm-native dispatch pipeline.
//!
//! Notes:
//! - Public std:reflect methods now accept real values and nominal type refs.
//!   We still use `__NATIVE_CALL` directly in a few tests where the wrapper does
//!   not expose every low-level native yet.
//! - MetadataStore requires object (pointer) targets, not primitives.
//! - Avoid `let x: number = __NATIVE_CALL(...)` when classes are defined
//!   in the same compilation unit (triggers type checker issue). Use
//!   `let x = __NATIVE_CALL(...)` without explicit type annotation instead.
//! - Proxy, permissions, and bootstrap handlers are in handlers/reflect.rs.

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

#[test]
fn test_reflect_get_field_names_user_class() {
    // REFLECT_GET_FIELD_NAMES = 0x0D23
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        class User {
            id: number;
            name: string;
            constructor(id: number, name: string) {
                this.id = id;
                this.name = name;
            }
        }
        let u: User = new User(1, "a");
        let fields = __NATIVE_CALL(0x0D23, u) as string[];
        return fields.length == 2 && fields[0] == "id" && fields[1] == "name";
    "#,
        true,
    );
}

#[test]
fn test_reflect_has_field_user_class() {
    // REFLECT_HAS = 0x0D22
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        class User {
            id: number = 1;
            name: string = "a";
        }
        let u: User = new User();
        let hasId = __NATIVE_CALL(0x0D22, u, "id") as boolean;
        let hasMissing = __NATIVE_CALL(0x0D22, u, "missing") as boolean;
        return hasId && !hasMissing;
    "#,
        true,
    );
}

#[test]
fn test_reflect_get_set_field_user_class() {
    // REFLECT_GET = 0x0D20, REFLECT_SET = 0x0D21
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        class User {
            id: number = 1;
            name: string = "a";
        }
        let u: User = new User();
        __NATIVE_CALL(0x0D21, u, "id", 42);
        let got = __NATIVE_CALL(0x0D20, u, "id") as number;
        return got == 42;
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
// Class Introspection
// ============================================================================

#[test]
fn test_reflect_get_class() {
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        class Foo { x: number = 42; }
        let obj: Foo = new Foo();
        let typeRef = reflect.getClass(obj);
        if (typeRef == null) {
            return false;
        }
        return typeRef.nominalTypeId > 0 && typeRef.name == "Foo";
    "#,
        true,
    );
}

#[test]
fn test_reflect_is_instance_of() {
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        class Bar { y: number = 10; }
        let obj: Bar = new Bar();
        let typeRef = reflect.getClass(obj);
        if (typeRef == null) {
            return false;
        }
        return reflect.isInstanceOf(obj, typeRef);
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
    assert!(result.is_ok(), "inspect should work: {:?}", result.err());
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
    // Get nominal type ref and verify instance check
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        class Animal { name: string = "generic"; }
        let a: Animal = new Animal();
        let typeRef = reflect.getClass(a);
        if (typeRef == null) {
            return false;
        }
        return reflect.isInstanceOf(a, typeRef);
    "#,
        true,
    );
}

#[test]
fn test_reflect_nominal_type_ref_hierarchy_workflow() {
    expect_bool_with_builtins(
        r#"
        import reflect from "std:reflect";
        class Animal {}
        class Dog extends Animal {}
        let dog = new Dog();
        let dogType = reflect.getClass(dog);
        if (dogType == null) {
            return false;
        }
        let animalType = reflect.getSuperclass(dogType);
        if (animalType == null) {
            return false;
        }
        return reflect.isSubclassOf(dogType, animalType)
            && reflect.isInstanceOf(dog, dogType)
            && animalType.name == "Animal";
    "#,
        true,
    );
}

#[test]
fn test_reflect_clone_structural_object() {
    // REFLECT_CLONE = 0x0D43
    expect_bool_with_builtins(
        r#"
        let original = { a: 1, b: 2 };
        let cloned = __NATIVE_CALL(0x0D43, original) as { a: number, b: number };
        return cloned.a == 1 && cloned.b == 2;
    "#,
        true,
    );
}

#[test]
fn test_reflect_get_enumerable_keys_structural_object() {
    // REFLECT_GET_ENUMERABLE_KEYS = 0x0DA1
    expect_bool_with_builtins(
        r#"
        let original = { a: 1, b: 2 };
        let keys = __NATIVE_CALL(0x0DA1, original) as string[];
        return keys.length == 2 && keys.includes("a") && keys.includes("b");
    "#,
        true,
    );
}
