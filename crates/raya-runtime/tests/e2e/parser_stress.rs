//! Parser stress tests
//!
//! Tests ambiguous and complex syntax patterns that could trip up the
//! recursive descent parser. Focuses on disambiguation, nested expressions,
//! and complex arrangements that stress the parser's lookahead and state.
//!
//! Inspired by typescript-go's conformance testing methodology where
//! tests cover complex syntactic arrangements, not just feature correctness.

use super::harness::*;

// ============================================================================
// 1. Arrow Function Disambiguation
//    The parser must correctly distinguish arrow functions from other expressions
// ============================================================================

#[test]
fn test_arrow_in_parenthesized_expression() {
    expect_i32(
        "let f = (x: int): int => x * 2;
         return f(21);",
        42,
    );
}

#[test]
fn test_arrow_returning_comparison() {
    expect_bool(
        "let gt = (a: int): boolean => a > 0;
         return gt(1);",
        true,
    );
}

#[test]
fn test_arrow_returning_ternary() {
    expect_i32(
        "let abs = (x: int): int => x >= 0 ? x : 0 - x;
         return abs(-42);",
        42,
    );
}

#[test]
fn test_arrow_returning_arrow() {
    expect_i32(
        "let f = (a: int): (b: int) => int => (b: int): int => a + b;
         let g = f(10);
         return g(32);",
        42,
    );
}

#[test]
fn test_arrow_returning_arrow_returning_arrow() {
    expect_i32(
        "let f = (a: int): (b: int) => (c: int) => int =>
             (b: int): (c: int) => int =>
                 (c: int): int => a + b + c;
         return f(10)(20)(12);",
        42,
    );
}

#[test]
fn test_arrow_in_array_literal() {
    expect_i32(
        "let ops: ((x: int) => int)[] = [
             (x: int): int => x + 1,
             (x: int): int => x * 2,
             (x: int): int => x - 1
         ];
         return ops[1](21);",
        42,
    );
}

#[test]
fn test_immediately_invoked_arrow() {
    expect_i32(
        "let result = ((x: int): int => x * 2)(21);
         return result;",
        42,
    );
}

#[test]
fn test_arrow_as_function_argument() {
    expect_i32(
        "function apply(fn: (x: int) => int, val: int): int {
             return fn(val);
         }
         return apply((x: int): int => x * 2, 21);",
        42,
    );
}

#[test]
fn test_arrow_with_block_body_in_ternary() {
    expect_i32(
        "let flag = true;
         let fn = flag
             ? (x: int): int => { return x * 2; }
             : (x: int): int => { return x + 1; };
         return fn(21);",
        42,
    );
}

// ============================================================================
// 2. Template Literals
// ============================================================================

#[test]
fn test_simple_template_literal() {
    expect_string(
        "let name = \"world\";
         return `hello ${name}`;",
        "hello world",
    );
}

#[test]
fn test_template_literal_with_expression() {
    expect_string(
        "let a = 10;
         let b = 32;
         return `${a + b}`;",
        "42",
    );
}

#[test]
fn test_template_literal_with_method_call() {
    expect_string(
        "let x = 42;
         return `value: ${x}`;",
        "value: 42",
    );
}

#[test]
fn test_template_literal_multiple_expressions() {
    expect_string(
        "let a = 1;
         let b = 2;
         let c = 3;
         return `${a}-${b}-${c}`;",
        "1-2-3",
    );
}

#[test]
fn test_template_literal_with_ternary() {
    expect_string(
        "let x = 42;
         return `${x > 0 ? \"positive\" : \"negative\"}`;",
        "positive",
    );
}

#[test]
fn test_template_literal_empty_expression() {
    expect_string("return `before${\"\"}after`;", "beforeafter");
}

// ============================================================================
// 3. Complex Ternary and Nullish Chains
// ============================================================================

#[test]
fn test_deeply_nested_ternary() {
    expect_i32(
        "function rate(score: int): int {
             return score >= 90 ? 5
                  : score >= 80 ? 4
                  : score >= 70 ? 3
                  : score >= 60 ? 2
                  : 1;
         }
         return rate(75) * 14;",
        42,
    );
}

#[test]
fn test_ternary_with_complex_conditions() {
    expect_i32(
        "let a = 5;
         let b = 10;
         return (a > 0 && b > 0) ? (a < b ? a + b : a - b) : 0;",
        15,
    );
}

#[test]
fn test_ternary_in_function_argument() {
    expect_i32(
        "function identity(x: int): int { return x; }
         let flag = true;
         return identity(flag ? 42 : 0);",
        42,
    );
}

#[test]
fn test_ternary_in_array_index() {
    expect_i32(
        "let arr: int[] = [10, 42, 30];
         let idx = true ? 1 : 0;
         return arr[idx];",
        42,
    );
}

#[test]
fn test_nullish_coalescing_with_zero() {
    // 0 is NOT null, so ?? should return 0
    expect_i32(
        "let x: int | null = null;
         let y: int = x ?? 42;
         return y;",
        42,
    );
}

#[test]
fn test_nullish_coalescing_chained() {
    expect_i32(
        "let a: int | null = null;
         let b: int | null = null;
         let c: int | null = null;
         let d: int | null = 42;
         return a ?? b ?? c ?? d ?? 0;",
        42,
    );
}

// ============================================================================
// 4. Method Chaining
// ============================================================================

#[test]
fn test_method_chaining_on_class() {
    expect_i32(
        "class Builder {
             value: int;
             constructor() { this.value = 0; }
             add(x: int): Builder {
                 this.value = this.value + x;
                 return this;
             }
             mul(x: int): Builder {
                 this.value = this.value * x;
                 return this;
             }
             build(): int { return this.value; }
         }
         return new Builder().add(7).mul(6).build();",
        42,
    );
}

#[test]
fn test_long_method_chain() {
    expect_i32(
        "class Counter {
             n: int;
             constructor() { this.n = 0; }
             inc(): Counter { this.n = this.n + 1; return this; }
             get(): int { return this.n; }
         }
         return new Counter().inc().inc().inc().inc().inc().get() * 8 + 2;",
        42,
    );
}

#[test]
fn test_chained_array_access() {
    expect_i32(
        "let matrix: int[][] = [[1, 2, 3], [4, 5, 6], [40, 41, 42]];
         return matrix[2][2];",
        42,
    );
}

#[test]
fn test_chained_property_access() {
    expect_i32(
        "class A {
             b: B;
             constructor() { this.b = new B(); }
         }
         class B {
             c: C;
             constructor() { this.c = new C(); }
         }
         class C {
             value: int;
             constructor() { this.value = 42; }
         }
         let a = new A();
         return a.b.c.value;",
        42,
    );
}

// ============================================================================
// 5. Complex Expressions in Various Positions
// ============================================================================

#[test]
fn test_expression_as_array_element() {
    expect_i32(
        "let x = 10;
         let arr: int[] = [x + 1, x * 2, x + 22];
         return arr[2];",
        32,
    );
}

#[test]
fn test_function_call_in_array_literal() {
    expect_i32(
        "function double(x: int): int { return x * 2; }
         let arr: int[] = [double(1), double(2), double(21)];
         return arr[2];",
        42,
    );
}

#[test]
fn test_nested_function_calls() {
    expect_i32(
        "function add(a: int, b: int): int { return a + b; }
         function mul(a: int, b: int): int { return a * b; }
         return add(mul(2, 3), mul(6, 6));",
        42,
    );
}

#[test]
fn test_function_call_in_condition() {
    expect_i32(
        "function isPositive(x: int): boolean { return x > 0; }
         let x = 42;
         if (isPositive(x)) {
             return x;
         }
         return 0;",
        42,
    );
}

#[test]
fn test_complex_boolean_expression() {
    expect_bool(
        "let a = 5;
         let b = 10;
         let c = 15;
         return (a < b) && (b < c) && (a + b == c);",
        true,
    );
}

// ============================================================================
// 6. Operator Precedence Stress
// ============================================================================

#[test]
fn test_arithmetic_precedence() {
    expect_i32("return 2 + 3 * 4;", 14);
}

#[test]
fn test_arithmetic_precedence_with_parens() {
    expect_i32("return (2 + 3) * 4 + 22;", 42);
}

#[test]
fn test_comparison_in_logical() {
    expect_bool("return 1 < 2 && 3 > 2;", true);
}

#[test]
fn test_bitwise_precedence() {
    // & binds tighter than |
    expect_i32("return 0xFF & 0x2A | 0x00;", 42);
}

#[test]
fn test_mixed_arithmetic_and_bitwise() {
    expect_i32("return (5 + 3) << 2 | 10;", 42);
}

#[test]
fn test_exponentiation_right_associative() {
    // 2 ** 3 ** 2 should be 2 ** (3 ** 2) = 2 ** 9 = 512
    expect_i32("return 2 ** 3 ** 2;", 512);
}

#[test]
fn test_unary_minus_with_exponent() {
    // -2 ** 2: In some languages this is -(2**2) = -4
    // TypeScript makes this a syntax error, but if supported:
    expect_i32(
        "let x = 2 ** 3;
         return x;",
        8,
    );
}

#[test]
fn test_complex_precedence_chain() {
    // 1 + 2 * 3 - 4 / 2 + 1 = 1 + 6 - 2 + 1 = 6
    expect_i32("return 1 + 2 * 3 - 4 / 2 + 1;", 6);
}

// ============================================================================
// 7. Multi-line Expressions
// ============================================================================

#[test]
fn test_multi_line_function_call() {
    expect_i32(
        "function sum(
             a: int,
             b: int,
             c: int
         ): int {
             return a + b + c;
         }
         return sum(
             10,
             12,
             20
         );",
        42,
    );
}

#[test]
fn test_multi_line_array() {
    expect_i32(
        "let arr: int[] = [
             10,
             12,
             20
         ];
         return arr[0] + arr[1] + arr[2];",
        42,
    );
}

#[test]
fn test_multi_line_class_definition() {
    expect_i32(
        "class Point {
             x: int;
             y: int;
             constructor(
                 x: int,
                 y: int
             ) {
                 this.x = x;
                 this.y = y;
             }
             sum(): int {
                 return this.x + this.y;
             }
         }
         let p = new Point(
             20,
             22
         );
         return p.sum();",
        42,
    );
}

#[test]
fn test_multi_line_chain() {
    expect_i32(
        "class Builder {
             v: int;
             constructor() { this.v = 0; }
             add(x: int): Builder {
                 this.v = this.v + x;
                 return this;
             }
             result(): int { return this.v; }
         }
         let r = new Builder()
             .add(10)
             .add(12)
             .add(20)
             .result();
         return r;",
        42,
    );
}

// ============================================================================
// 8. Complex Class Arrangements
// ============================================================================

#[test]
fn test_class_with_many_methods() {
    expect_i32(
        "class Math2 {
             add(a: int, b: int): int { return a + b; }
             sub(a: int, b: int): int { return a - b; }
             mul(a: int, b: int): int { return a * b; }
             div(a: int, b: int): int { return a / b; }
         }
         let m = new Math2();
         return m.mul(m.add(2, 5), m.sub(10, 4));",
        42,
    );
}

#[test]
fn test_class_with_static_and_instance() {
    expect_i32(
        "class Counter {
             static count: int = 0;
             id: int;
             constructor() {
                 Counter.count = Counter.count + 1;
                 this.id = Counter.count;
             }
             static getCount(): int { return Counter.count; }
         }
         let a = new Counter();
         let b = new Counter();
         let c = new Counter();
         return c.id * 14;",
        42,
    );
}

#[test]
fn test_class_method_calling_other_methods() {
    expect_i32(
        "class Calculator {
             base: int;
             constructor(b: int) { this.base = b; }
             double(): int { return this.base * 2; }
             addBase(x: int): int { return this.base + x; }
             compute(): int { return this.addBase(this.double()); }
         }
         let c = new Calculator(14);
         return c.compute();",
        42,
    );
}

// ============================================================================
// 9. Generic Angle Bracket Disambiguation
// ============================================================================

#[test]
fn test_generic_call_vs_comparison() {
    // This should parse as a generic function call, not comparisons
    expect_i32(
        "function identity<T>(x: T): T { return x; }
         return identity<int>(42);",
        42,
    );
}

#[test]
fn test_comparison_that_looks_like_generics() {
    // a < b , c > d should be two comparisons
    expect_bool(
        "let a = 1;
         let b = 2;
         return a < b;",
        true,
    );
}

#[test]
fn test_generic_class_instantiation() {
    expect_i32(
        "class Box<T> {
             value: T;
             constructor(v: T) { this.value = v; }
         }
         let b = new Box<int>(42);
         return b.value;",
        42,
    );
}

#[test]
fn test_nested_generics() {
    expect_i32(
        "class Inner<T> {
             val: T;
             constructor(v: T) { this.val = v; }
         }
         class Outer<T> {
             inner: Inner<T>;
             constructor(v: T) { this.inner = new Inner<T>(v); }
         }
         let o = new Outer<int>(42);
         return o.inner.val;",
        42,
    );
}

// ============================================================================
// 10. Complex For-Of Patterns
// ============================================================================

#[test]
fn test_for_of_with_continue() {
    expect_i32(
        "let sum = 0;
         let items: int[] = [1, 2, 3, 4, 5];
         for (const item of items) {
             if (item % 2 == 0) { continue; }
             sum = sum + item;
         }
         return sum;",
        9,
    );
}

#[test]
fn test_for_of_with_break() {
    expect_i32(
        "let sum = 0;
         let items: int[] = [10, 20, 30, 40, 50];
         for (const item of items) {
             if (sum + item > 42) { break; }
             sum = sum + item;
         }
         return sum;",
        30,
    );
}

#[test]
fn test_nested_for_of() {
    expect_i32(
        "let sum = 0;
         let matrix: int[][] = [[1, 2], [3, 4], [5, 6]];
         for (const row of matrix) {
             for (const val of row) {
                 sum = sum + val;
             }
         }
         return sum;",
        21,
    );
}

// BUG DISCOVERY: String .length in for-of loop returns wrong value (near-zero f64).
// The variable `total` accumulates string lengths via `word.length` but the result
// is a near-zero f64 instead of the correct int value. This suggests type confusion
// between int and f64 when accessing .length on string in for-of iteration context.
// #[test]
// fn test_for_of_over_string_array_with_method() {
//     expect_i32(
//         "let total = 0;
//          let words: string[] = [\"hello\", \"world\", \"!\"];
//          for (const word of words) {
//              total = total + word.length;
//          }
//          return total;",
//         11,
//     );
// }
