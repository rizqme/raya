//! Targeted hardening regressions for strict dispatch/callability behavior.

use super::harness::*;

#[test]
fn test_strict_unresolved_member_call_on_number_compile_error() {
    expect_compile_error(
        "let n: number = 1;
         n.missing();",
        "UnsupportedExpressionTypingPath",
    );
}

#[test]
fn test_strict_unresolved_member_property_on_number_compile_error() {
    expect_compile_error(
        "let n: number = 1;
         return n.missing;",
        "UnsupportedExpressionTypingPath",
    );
}

#[test]
fn test_strict_unresolved_member_assignment_on_number_compile_error() {
    expect_compile_error(
        "let n: number = 1;
         n.missing = 2;
         return 0;",
        "UnsupportedExpressionTypingPath",
    );
}

#[test]
fn test_strict_non_callable_direct_call_compile_error() {
    expect_compile_error(
        "let n: number = 1;
         n();
         return 0;",
        "NotCallable",
    );
}

#[test]
fn test_strict_non_callable_async_call_compile_error() {
    expect_compile_error(
        "let n: number = 1;
         async n();
         return 0;",
        "NotCallable",
    );
}

#[test]
fn test_strict_unresolved_async_member_call_on_number_compile_error() {
    expect_compile_error(
        "let n: number = 1;
         async n.missing();
         return 0;",
        "UnsupportedExpressionTypingPath",
    );
}

#[test]
fn test_class_alias_value_identity_distinguishes_different_classes() {
    expect_bool_with_builtins(
        "class A {}
         class B {}
         const X = A;
         const Y = B;
         return X == Y;",
        false,
    );
}

#[test]
fn test_strict_non_callable_tagged_template_compile_error() {
    expect_compile_error(
        "let n: number = 1;
         n`x`;
         return 0;",
        "NotCallable",
    );
}

#[test]
fn test_strict_unknown_tagged_template_reports_unknown_actionable() {
    expect_compile_error(
        "function run(tag) {
             tag`x`;
             return 0;
         }
         return 0;",
        "UnknownNotActionable",
    );
}

#[test]
fn test_strict_non_callable_structural_member_call_compile_error() {
    expect_compile_error(
        "type C = { f: number };
         let c: C = { f: 1 };
         c.f();
         return 0;",
        "NotCallable",
    );
}

#[test]
fn test_strict_structural_function_member_call_succeeds() {
    expect_i32(
        "type C = { f: (x: number) => number };
         let c: C = { f: (x: number): number => x + 1 };
         return c.f(41);",
        42,
    );
}

#[test]
fn test_strict_await_non_task_compile_error() {
    expect_compile_error(
        "async function f() {
             let x: number = 1;
             await x;
         }
         return 0;",
        "Promise<T> or Promise<T>[]",
    );
}

#[test]
fn test_strict_await_non_task_array_compile_error() {
    expect_compile_error(
        "async function f() {
             let xs: number[] = [1, 2];
             await xs;
         }
         return 0;",
        "Promise<T> or Promise<T>[]",
    );
}
