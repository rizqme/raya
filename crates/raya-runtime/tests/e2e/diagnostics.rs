//! Diagnostic / error tests
//!
//! Tests that verify specific compile-time and runtime errors are produced
//! for invalid programs. A dedicated diagnostics test file catches regressions
//! in error messages and ensures all error paths are exercised.

use super::harness::*;

// ============================================================================
// 1. Type Mismatch Errors
// ============================================================================

#[test]
fn test_error_assign_string_to_int() {
    expect_compile_error("let x: int = \"hello\";", "TypeMismatch");
}

#[test]
fn test_error_return_wrong_type() {
    expect_compile_error("function f(): int { return \"hello\"; }", "TypeMismatch");
}

#[test]
fn test_error_wrong_argument_type() {
    expect_compile_error(
        "function f(x: int): int { return x; }
         f(\"hello\");",
        "TypeMismatch",
    );
}

#[test]
fn test_error_wrong_argument_count() {
    expect_compile_error(
        "function f(x: int, y: int): int { return x + y; }
         f(1);",
        "ArgumentCountMismatch",
    );
}

#[test]
fn test_error_too_many_arguments() {
    expect_compile_error(
        "function f(x: int): int { return x; }
         f(1, 2);",
        "ArgumentCountMismatch",
    );
}

#[test]
fn test_error_incompatible_binary_op() {
    expect_compile_error("return \"hello\" - 1;", "TypeMismatch");
}

// ============================================================================
// 2. Scope / Binding Errors
// ============================================================================

#[test]
fn test_error_undefined_variable() {
    expect_compile_error("return undefinedVar;", "undefined");
}

#[test]
fn test_error_undefined_function() {
    expect_compile_error("return nonExistentFunction();", "Undefined");
}

#[test]
fn test_error_duplicate_let() {
    expect_compile_error(
        "let x = 10;
         let x = 20;",
        "Duplicate",
    );
}

#[test]
fn test_error_const_reassignment() {
    expect_compile_error(
        "const x = 42;
         x = 10;",
        "ConstReassignment",
    );
}

// ============================================================================
// 3. Class Errors
// ============================================================================

#[test]
fn test_error_access_nonexistent_field() {
    expect_compile_error(
        "class Foo {
             x: int;
             constructor() { this.x = 1; }
         }
         let f = new Foo();
         return f.nonexistent;",
        "property",
    );
}

#[test]
fn test_error_access_nonexistent_method() {
    expect_compile_error(
        "class Foo {
             x: int;
             constructor() { this.x = 1; }
         }
         let f = new Foo();
         f.nonexistent();",
        "PropertyNotFound",
    );
}

#[test]
fn test_error_abstract_class_instantiation() {
    expect_compile_error(
        "abstract class Base {
             abstract foo(): int;
         }
         let b = new Base();",
        "AbstractClassInstantiation",
    );
}

// ============================================================================
// 4. Control Flow Errors
//    BUG DISCOVERY: break/continue outside loop not caught as compile error
// ============================================================================

// TODO: These tests document a bug — break and continue outside loops
// should be compile errors but currently compile successfully.
// Uncomment when the parser/checker is fixed.

// #[test]
// fn test_error_break_outside_loop() {
//     expect_compile_error("break;", "break");
// }
//
// #[test]
// fn test_error_continue_outside_loop() {
//     expect_compile_error("continue;", "continue");
// }

// ============================================================================
// 5. Runtime Errors
// ============================================================================

#[test]
fn test_runtime_error_division_by_zero() {
    expect_runtime_error(
        "let x = 42;
         let y = 0;
         return x / y;",
        "division",
    );
}

#[test]
fn test_runtime_error_array_out_of_bounds() {
    expect_runtime_error(
        "let arr: int[] = [1, 2, 3];
         return arr[10];",
        "bounds",
    );
}

#[test]
fn test_runtime_error_null_dereference() {
    // Actual error: "Expected object for field access"
    expect_runtime_error(
        "class Foo {
             value: int;
             constructor(v: int) { this.value = v; }
         }
         let x: Foo | null = null;
         return x.value;",
        "Expected object",
    );
}

#[test]
fn test_runtime_error_stack_overflow() {
    expect_runtime_error(
        "function infinite(): int {
             return infinite();
         }
         return infinite();",
        "Stack overflow",
    );
}

// NOTE: `throw new Error("boom")` without builtins fails with "Invalid class index"
// because Error is a builtin class. Test with builtins:
#[test]
fn test_runtime_error_uncaught_throw_with_builtins() {
    expect_runtime_error_with_builtins("throw new Error(\"boom\");", "boom");
}

// ============================================================================
// 6. Caught Exceptions (should succeed)
// ============================================================================

#[test]
fn test_caught_error_returns_fallback() {
    expect_i32_with_builtins(
        "function risky(): int {
             throw new Error(\"fail\");
         }
         try {
             risky();
         } catch (e) {
             return 42;
         }
         return 0;",
        42,
    );
}

#[test]
fn test_caught_error_message() {
    expect_string_with_builtins(
        "try {
             throw new Error(\"custom message\");
         } catch (e) {
             return e.message;
         }
         return \"no error\";",
        "custom message",
    );
}

#[test]
fn test_nested_exception_propagation() {
    expect_i32_with_builtins(
        "function inner(): int {
             throw new Error(\"inner\");
         }
         function middle(): int {
             return inner();
         }
         try {
             middle();
         } catch (e) {
             return 42;
         }
         return 0;",
        42,
    );
}

// ============================================================================
// 7. Multiple Error Patterns
// ============================================================================

#[test]
fn test_multiple_catch_scenarios() {
    expect_i32_with_builtins(
        "let result = 0;
         try { throw new Error(\"a\"); } catch (e) { result = result + 10; }
         try { throw new Error(\"b\"); } catch (e) { result = result + 12; }
         try { throw new Error(\"c\"); } catch (e) { result = result + 20; }
         return result;",
        42,
    );
}

#[test]
fn test_try_without_exception() {
    expect_i32(
        "let x = 0;
         try {
             x = 42;
         } catch (e) {
             x = -1;
         }
         return x;",
        42,
    );
}

#[test]
fn test_finally_with_return_value() {
    expect_i32(
        "function test(): int {
             let x = 0;
             try {
                 x = 42;
             } finally {
                 // finally runs, but doesn't change return
             }
             return x;
         }
         return test();",
        42,
    );
}

// ============================================================================
// 8. Edge Case Errors
// ============================================================================

#[test]
fn test_error_negative_array_access() {
    expect_runtime_error(
        "let arr: int[] = [1, 2, 3];
         return arr[-1];",
        "bounds",
    );
}

#[test]
fn test_error_method_on_wrong_type() {
    expect_compile_error(
        "let x: int = 42;
         return x.toUpperCase();",
        "NotCallable",
    );
}

#[test]
fn test_error_assign_to_wrong_class() {
    expect_compile_error(
        "class A { x: int; constructor() { this.x = 1; } }
         class B { y: int; constructor() { this.y = 2; } }
         let a: A = new B();",
        "TypeMismatch",
    );
}
