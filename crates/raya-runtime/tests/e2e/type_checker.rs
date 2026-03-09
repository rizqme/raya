//! Type checker tests adapted from typescript-go conformance/compiler tests.
//!
//! These tests verify that the Raya type checker correctly detects errors
//! at compile time. Adapted from:
//!   - reachabilityChecks9.ts, reachabilityChecks10.ts, reachabilityChecks11.ts
//!   - duplicateIdentifierChecks.ts
//!   - circularDestructuring.ts
//!   - protectedAccessibilityCheck.ts
//!   - exhaustiveSwitchStatementsGeneric1.ts
//!   - switchExhaustiveNarrowing.ts

use super::harness::*;

// ============================================================================
// 1. Duplicate Identifier Detection
//    Adapted from: typescript-go/testdata/tests/cases/compiler/duplicateIdentifierChecks.ts
// ============================================================================

#[test]
fn test_duplicate_class_field_names() {
    expect_compile_error(
        "class Foo {
             x: number = 1;
             x: number = 2;
         }
         return 0;",
        "duplicate",
    );
}

#[test]
fn test_duplicate_method_names() {
    expect_compile_error(
        "class Foo {
             doWork(): number { return 1; }
             doWork(): number { return 2; }
         }
         return 0;",
        "duplicate",
    );
}

#[test]
fn test_duplicate_let_in_same_scope() {
    expect_compile_error(
        "let x: number = 1;
         let x: number = 2;
         return x;",
        "duplicate",
    );
}

#[test]
fn test_duplicate_const_in_same_scope() {
    expect_compile_error(
        "const x: number = 1;
         const x: number = 2;
         return x;",
        "duplicate",
    );
}

#[test]
fn test_duplicate_function_names() {
    expect_compile_error(
        "function foo(): number { return 1; }
         function foo(): number { return 2; }
         return foo();",
        "duplicate",
    );
}

#[test]
fn test_duplicate_class_names() {
    expect_compile_error(
        "class Foo { x: number = 1; }
         class Foo { x: number = 2; }
         return 0;",
        "duplicate",
    );
}

// ============================================================================
// 2. Type Mismatch Detection
//    Adapted from: typescript-go/testdata/tests/cases/compiler/simpleTestSingleFile.ts
// ============================================================================

#[test]
fn test_type_mismatch_assignment() {
    // Assigning a string to a number variable should fail
    expect_compile_error(
        "let x: number = \"hello\";
         return 0;",
        "TypeMismatch",
    );
}

#[test]
fn test_type_mismatch_return() {
    // Returning wrong type from function
    expect_compile_error(
        "function foo(): number {
             return \"not a number\";
         }
         return foo();",
        "TypeMismatch",
    );
}

#[test]
fn test_type_mismatch_function_argument() {
    // Passing wrong type to function parameter
    expect_compile_error(
        "function add(a: number, b: number): number {
             return a + b;
         }
         return add(1, \"two\");",
        "TypeMismatch",
    );
}

#[test]
fn test_type_mismatch_bool_to_number() {
    // Assigning boolean to number should fail
    expect_compile_error(
        "let x: number = true;
         return 0;",
        "TypeMismatch",
    );
}

// ============================================================================
// 3. Access Control
//    Adapted from: typescript-go/testdata/tests/cases/compiler/protectedAccessibilityCheck.ts
// ============================================================================

#[test]
fn test_private_field_external_access() {
    expect_compile_error(
        "class Secret {
             private code: number = 42;
         }
         let s = new Secret();
         return s.code;",
        "private",
    );
}

#[test]
fn test_private_method_external_access() {
    expect_compile_error(
        "class Secret {
             private getCode(): number { return 42; }
         }
         let s = new Secret();
         return s.getCode();",
        "private",
    );
}

#[test]
fn test_private_field_accessible_within_class() {
    // Private fields should be accessible from within the class
    expect_i32(
        "class Secret {
             private code: number = 42;
             getCode(): number { return this.code; }
         }
         let s = new Secret();
         return s.getCode();",
        42,
    );
}

#[test]
fn test_private_field_not_accessible_from_subclass() {
    expect_compile_error(
        "class Base {
             private secret: number = 42;
         }
         class Child extends Base {
             read(): number { return this.secret; }
         }
         return 0;",
        "private",
    );
}

#[test]
fn test_protected_field_external_access() {
    expect_compile_error(
        "class Base {
             protected code: number = 42;
         }
         let b = new Base();
         return b.code;",
        "protected",
    );
}

#[test]
fn test_protected_field_unrelated_class_access() {
    expect_compile_error(
        "class Base {
             protected code: number = 42;
         }
         class Other {
             read(b: Base): number { return b.code; }
         }
         return 0;",
        "protected",
    );
}

#[test]
fn test_protected_field_accessible_from_subclass() {
    expect_i32(
        "class Base {
             protected code: number = 42;
         }
         class Child extends Base {
             read(): number { return this.code; }
         }
         return new Child().read();",
        42,
    );
}

#[test]
fn test_private_constructor_not_instantiable() {
    expect_compile_error(
        "class Secret {
             private constructor() {}
         }
         let s = new Secret();
         return 0;",
        "NewNonClass",
    );
}

#[test]
fn test_private_constructor_accessible_inside_declaring_class() {
    expect_i32(
        "class Secret {
             private constructor() {}
             static make(): Secret { return new Secret(); }
         }
         return Secret.make() != null ? 1 : 0;",
        1,
    );
}

#[test]
fn test_protected_constructor_not_directly_instantiable() {
    expect_compile_error(
        "class Base {
             protected constructor() {}
         }
         let b = new Base();
         return 0;",
        "NewNonClass",
    );
}

#[test]
fn test_public_constructor_still_instantiable() {
    expect_i32(
        "class Box {
             constructor() {}
         }
         let b = new Box();
         return b != null ? 1 : 0;",
        1,
    );
}

#[test]
fn test_union_member_requires_all_variants() {
    expect_compile_error(
        "type U = { kind: \"a\", a: number } | { kind: \"b\", b: number };
         function read(u: U): number {
             return u.b;
         }
         return 0;",
        "property",
    );
}

#[test]
fn test_nullable_union_array_literal_assignable_to_array_annotation() {
    expect_i32(
        "let arr: (number | null)[] = [1, null, 3];
         let v = arr[2];
         if (v === null) { return 0; }
         return v;",
        3,
    );
}

#[test]
fn test_in_operator_reports_compile_error_not_parser_hang() {
    expect_compile_error("return \"a\" in { a: 1 };", "unsupported operator 'in'");
}

// ============================================================================
// 4. Class Inheritance Type Checks
//    Adapted from: typescript-go/testdata/tests/cases/compiler/checkInheritedProperty.ts
// ============================================================================

#[test]
fn test_inherited_field_access() {
    // Fields from base class should be accessible in derived class
    expect_i32(
        "class Base {
             value: number = 10;
         }
         class Derived extends Base {
             getDouble(): number { return this.value * 2; }
         }
         let d = new Derived();
         return d.getDouble();",
        20,
    );
}

#[test]
fn test_inherited_field_override() {
    // Derived class field initializer should override base class field
    expect_i32(
        "class Base {
             value: number = 10;
         }
         class Derived extends Base {
             value: number = 42;
         }
         let d = new Derived();
         return d.value;",
        42,
    );
}

#[test]
fn test_multi_level_inheritance() {
    // Fields and methods should be accessible through multi-level inheritance
    expect_i32(
        "class A {
             x: number = 1;
         }
         class B extends A {
             y: number = 2;
         }
         class C extends B {
             z: number = 3;
             sum(): number { return this.x + this.y + this.z; }
         }
         let c = new C();
         return c.sum();",
        6,
    );
}

#[test]
fn test_constructor_inheritance_chain() {
    // Constructor with super call chain
    expect_i32(
        "class Base {
             value: number;
             constructor(v: number) {
                 this.value = v;
             }
         }
         class Mid extends Base {
             extra: number;
             constructor(v: number, e: number) {
                 super(v);
                 this.extra = e;
             }
         }
         class Leaf extends Mid {
             constructor() {
                 super(10, 20);
             }
             total(): number { return this.value + this.extra; }
         }
         let l = new Leaf();
         return l.total();",
        30,
    );
}

// ============================================================================
// 5. Undeclared Variable / Unknown Identifier
// ============================================================================

#[test]
fn test_undeclared_variable() {
    expect_compile_error("return undeclaredVar;", "undeclared");
}

#[test]
fn test_undeclared_function() {
    expect_compile_error("return undeclaredFunc();", "undeclared");
}

#[test]
fn test_undeclared_field_on_class() {
    expect_compile_error(
        "class Foo {
             x: number = 1;
         }
         let f = new Foo();
         return f.y;",
        "PropertyNotFound",
    );
}

// ============================================================================
// 6. Const Reassignment
// ============================================================================

#[test]
fn test_const_reassignment() {
    expect_compile_error(
        "const x: number = 42;
         x = 100;
         return x;",
        "ConstReassignment",
    );
}

#[test]
fn test_const_compound_assignment() {
    expect_compile_error(
        "const x: number = 42;
         x += 1;
         return x;",
        "ConstReassignment",
    );
}

// ============================================================================
// 7. Function Arity Errors
// ============================================================================

#[test]
fn test_too_few_arguments() {
    expect_compile_error(
        "function add(a: number, b: number): number {
             return a + b;
         }
         return add(1);",
        "ArgumentCountMismatch",
    );
}

#[test]
fn test_too_many_arguments() {
    // Calling function with more arguments than declared
    expect_compile_error(
        "function double(x: number): number {
             return x * 2;
         }
         return double(1, 2);",
        "ArgumentCountMismatch",
    );
}

// ============================================================================
// 8. Invalid Operations
// ============================================================================

#[test]
fn test_calling_non_function() {
    // Trying to call a non-function value
    expect_compile_error(
        "let x: number = 42;
         return x();",
        "NotCallable",
    );
}

#[test]
fn test_new_on_non_class() {
    expect_compile_error(
        "let x: number = 42;
         let y = new x();
         return 0;",
        "NewNonClass",
    );
}

// ============================================================================
// 9. Fallback Retention & Diagnostics
// ============================================================================

#[test]
fn test_parenthesized_expression_type_retained() {
    expect_i32(
        "let x: number = (41 + 1);
         return x;",
        42,
    );
}

#[test]
fn test_intrinsic_invalid_inference_context_reports_error() {
    expect_compile_error(
        "let x: number = __OPCODE_AWAIT(1);
         return x;",
        "InvalidIntrinsicInferenceContext",
    );
}

#[test]
fn test_checker_type_reference_arity_error() {
    expect_compile_error(
        "let x = 1 as Array<number, string>;
         return 0;",
        "InvalidTypeReferenceArity",
    );
}

#[test]
fn test_checker_map_type_reference_arity_error() {
    expect_compile_error(
        "let m = 1 as Map<string>;
         return 0;",
        "InvalidTypeReferenceArity",
    );
}

#[test]
fn test_checker_set_type_reference_arity_error() {
    expect_compile_error(
        "let s = 1 as Set<number, string>;
         return 0;",
        "InvalidTypeReferenceArity",
    );
}

#[test]
fn test_index_missing_property_reports_error() {
    expect_compile_error(
        "let o: { x: number } = { x: 1 };
         return o[\"y\"];",
        "PropertyNotFound",
    );
}

#[test]
fn test_tuple_index_out_of_bounds_reports_error() {
    expect_compile_error(
        "let t: [number, string] = [1, \"a\"];
         return t[2];",
        "PropertyNotFound",
    );
}

#[test]
fn test_interface_member_access_type_retained() {
    expect_i32(
        "interface Box { value: number }
         let b: Box = { value: 7 };
         return b.value;",
        7,
    );
}

#[test]
fn test_interface_index_access_type_retained() {
    expect_i32(
        "interface Box { value: number }
         let b: Box = { value: 9 };
         return b[\"value\"];",
        9,
    );
}

#[test]
fn test_interface_string_index_signature_type_retained() {
    expect_i32(
        "interface Dict { [key: string]: number }
         let d: Dict = { value: 11 };
         return d[\"value\"];",
        11,
    );
}

#[test]
fn test_interface_extends_type_retained() {
    expect_i32(
        "interface Base { x: number }
         interface Derived extends Base { y: number }
         let d: Derived = { x: 1, y: 2 };
         return d.x + d.y;",
        3,
    );
}

#[test]
fn test_interface_extends_undefined_type_reports_error() {
    expect_compile_error(
        "interface Derived extends MissingBase { y: number }
         let d: Derived = { y: 1 };
         return d.y;",
        "Undefined type",
    );
}

#[test]
fn test_interface_missing_required_property_reports_error() {
    expect_compile_error(
        "interface Box { value: number }
         let b: Box = {};
         return 0;",
        "Type mismatch",
    );
}

#[test]
fn test_interface_optional_method_can_be_omitted() {
    expect_i32(
        "interface Handler { run?(): number }
         let h: Handler = {};
         return 1;",
        1,
    );
}

#[test]
fn test_interface_call_signature_assignability_and_call() {
    expect_i32(
        "interface Adder { (a: number, b: number): number }
         function unary(a: number): number { return a; }
         let f: Adder = unary;
         return f(20, 22);",
        20,
    );
}

#[test]
fn test_interface_construct_signature_assignability_and_new() {
    expect_i32(
        "interface BoxCtor { new (value: number): { value: number } }
         class Box {
             value: number;
             constructor(value: number) { this.value = value; }
         }
         let C: BoxCtor = Box;
         let b = new C(42);
         return b.value;",
        42,
    );
}

#[test]
fn test_interface_extends_class_public_surface_type_retained() {
    expect_i32(
        "class Base { x: number = 1; }
         interface Derived extends Base { y: number }
         let d: Derived = { x: 1, y: 2 };
         return d.x + d.y;",
        3,
    );
}

#[test]
fn test_interface_extends_class_requires_base_members() {
    expect_compile_error(
        "class Base { x: number = 1; }
         interface Derived extends Base { y: number }
         let d: Derived = { y: 2 };
         return d.y;",
        "Type mismatch",
    );
}

#[test]
fn test_member_non_object_reports_unsupported_typing_path() {
    expect_compile_error(
        "let x: number = 1;
         return x.foo;",
        "UnsupportedExpressionTypingPath",
    );
}

#[test]
fn test_index_non_object_reports_unsupported_typing_path() {
    expect_compile_error(
        "let x: number = 1;
         return x[0];",
        "UnsupportedExpressionTypingPath",
    );
}
