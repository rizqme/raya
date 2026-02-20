//! Discriminated union tests
//!
//! Adapted from TypeScript conformance tests:
//!   - controlFlow/controlFlowGenericTypes.ts (discriminated patterns)
//!   - types/conditional/ (discriminant narrowing patterns)
//!
//! Raya supports discriminated unions with the
//! `type X = | { kind: "a" } | { kind: "b" }` syntax.

use super::harness::*;

// ============================================================================
// 1. Basic Discriminated Union with Switch
// ============================================================================

#[test]
fn test_discriminated_union_switch_string() {
    expect_string(
        "type Shape =
             | { kind: \"circle\"; radius: number }
             | { kind: \"rect\"; width: number; height: number };
         function describe(s: Shape): string {
             switch (s.kind) {
                 case \"circle\":
                     return \"circle r=\" + s.radius.toString();
                 case \"rect\":
                     return \"rect \" + s.width.toString() + \"x\" + s.height.toString();
             }
         }
         let c: Shape = { kind: \"circle\", radius: 5 };
         return describe(c);",
        "circle r=5",
    );
}

#[test]
fn test_discriminated_union_switch_rect() {
    expect_string(
        "type Shape =
             | { kind: \"circle\"; radius: number }
             | { kind: \"rect\"; width: number; height: number };
         function describe(s: Shape): string {
             switch (s.kind) {
                 case \"circle\":
                     return \"circle r=\" + s.radius.toString();
                 case \"rect\":
                     return \"rect \" + s.width.toString() + \"x\" + s.height.toString();
             }
         }
         let r: Shape = { kind: \"rect\", width: 3, height: 4 };
         return describe(r);",
        "rect 3x4",
    );
}

// ============================================================================
// 2. Discriminated Union with If-Else Narrowing
// ============================================================================

#[test]
fn test_discriminated_union_if_narrowing() {
    expect_i32(
        "type Result =
             | { status: \"ok\"; value: number }
             | { status: \"err\"; code: number };
         function extract(r: Result): number {
             if (r.status == \"ok\") {
                 return r.value;
             } else {
                 return r.code * -1;
             }
         }
         let ok: Result = { status: \"ok\", value: 42 };
         return extract(ok);",
        42,
    );
}

#[test]
fn test_discriminated_union_if_error_branch() {
    expect_i32(
        "type Result =
             | { status: \"ok\"; value: number }
             | { status: \"err\"; code: number };
         function extract(r: Result): number {
             if (r.status == \"ok\") {
                 return r.value;
             } else {
                 return r.code * -1;
             }
         }
         let err: Result = { status: \"err\", code: 42 };
         return extract(err);",
        -42,
    );
}

// ============================================================================
// 3. Three-Way Discriminated Union
// ============================================================================

#[test]
fn test_discriminated_union_three_variants() {
    expect_i32(
        "type Action =
             | { type: \"add\"; amount: number }
             | { type: \"sub\"; amount: number }
             | { type: \"reset\" };
         function apply(state: number, action: Action): number {
             switch (action.type) {
                 case \"add\":
                     return state + action.amount;
                 case \"sub\":
                     return state - action.amount;
                 case \"reset\":
                     return 0;
             }
         }
         let s = 0;
         s = apply(s, { type: \"add\", amount: 50 });
         s = apply(s, { type: \"sub\", amount: 8 });
         return s;",
        42,
    );
}

#[test]
fn test_discriminated_union_reset_variant() {
    expect_i32(
        "type Action =
             | { type: \"add\"; amount: number }
             | { type: \"reset\" };
         function apply(state: number, action: Action): number {
             switch (action.type) {
                 case \"add\":
                     return state + action.amount;
                 case \"reset\":
                     return 0;
             }
         }
         let s = apply(100, { type: \"reset\" });
         return s;",
        0,
    );
}

// ============================================================================
// 4. Discriminated Union with Generic Result Pattern
// ============================================================================

#[test]
#[ignore = "generic type parameters in union type alias body not yet supported"]
fn test_discriminated_union_generic_result() {
    expect_i32(
        "type Result<T, E> =
             | { status: \"ok\"; value: T }
             | { status: \"err\"; error: E };
         function unwrapOr<T, E>(r: Result<T, E>, fallback: T): T {
             if (r.status == \"ok\") {
                 return r.value;
             }
             return fallback;
         }
         let ok: Result<number, string> = { status: \"ok\", value: 42 };
         return unwrapOr<number, string>(ok, 0);",
        42,
    );
}

#[test]
#[ignore = "generic type parameters in union type alias body not yet supported"]
fn test_discriminated_union_generic_result_fallback() {
    expect_i32(
        "type Result<T, E> =
             | { status: \"ok\"; value: T }
             | { status: \"err\"; error: E };
         function unwrapOr<T, E>(r: Result<T, E>, fallback: T): T {
             if (r.status == \"ok\") {
                 return r.value;
             }
             return fallback;
         }
         let err: Result<number, string> = { status: \"err\", error: \"fail\" };
         return unwrapOr<number, string>(err, 42);",
        42,
    );
}

// ============================================================================
// 5. Nested Discriminated Unions
// ============================================================================

#[test]
fn test_discriminated_union_nested() {
    expect_i32(
        "type Inner =
             | { tag: \"num\"; n: number }
             | { tag: \"str\"; s: string };
         type Outer =
             | { kind: \"wrapped\"; inner: Inner }
             | { kind: \"raw\"; value: number };
         function extract(o: Outer): number {
             if (o.kind == \"raw\") {
                 return o.value;
             }
             if (o.inner.tag == \"num\") {
                 return o.inner.n;
             }
             return 0;
         }
         let o: Outer = { kind: \"wrapped\", inner: { tag: \"num\", n: 42 } };
         return extract(o);",
        42,
    );
}
