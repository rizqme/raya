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
    expect_compile_error(
        "return undeclaredVar;",
        "undeclared",
    );
}

#[test]
fn test_undeclared_function() {
    expect_compile_error(
        "return undeclaredFunc();",
        "undeclared",
    );
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
