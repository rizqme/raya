//! Int and Number type expression tests
//!
//! Raya has two numeric types unlike TypeScript:
//!   - `int` (32-bit signed integer) — integer literals without decimal point
//!   - `number` (64-bit float, IEEE 754) — literals with decimal point or exponent
//!
//! These tests verify correct type promotion, arithmetic, and operator behavior
//! with Raya's typed opcode system (IADD vs FADD).
//!
//! Note: Integer literals are inferred as `number` type by default.
//! Explicit `int` type annotation is not yet fully supported.

use super::harness::*;

// ============================================================================
// 1. Int Literals and Arithmetic
// ============================================================================

#[test]
fn test_int_literal() {
    expect_i32("return 42;", 42);
}

#[test]
fn test_int_addition() {
    expect_i32("return 20 + 22;", 42);
}

#[test]
fn test_int_subtraction() {
    expect_i32("return 50 - 8;", 42);
}

#[test]
fn test_int_multiplication() {
    expect_i32("return 6 * 7;", 42);
}

#[test]
fn test_int_division() {
    expect_i32("return 84 / 2;", 42);
}

#[test]
fn test_int_modulo() {
    expect_i32("return 142 % 100;", 42);
}

#[test]
fn test_int_negation() {
    expect_i32("return -(-42);", 42);
}

// ============================================================================
// 2. Number (Float) Literals and Arithmetic
// ============================================================================

#[test]
fn test_number_literal() {
    expect_f64("return 3.14;", 3.14);
}

#[test]
fn test_number_addition() {
    expect_f64("return 1.5 + 2.5;", 4.0);
}

#[test]
fn test_number_subtraction() {
    expect_f64("return 10.0 - 3.5;", 6.5);
}

#[test]
fn test_number_multiplication() {
    expect_f64("return 2.5 * 4.0;", 10.0);
}

#[test]
fn test_number_division() {
    expect_f64("return 7.0 / 2.0;", 3.5);
}

// ============================================================================
// 3. Int to Number Promotion
//    When int and number are mixed, int promotes to number
// ============================================================================

#[test]

fn test_int_number_addition_promotion() {
    expect_f64(
        "let x: int = 10;
         let y: number = 32.0;
         return x + y;",
        42.0,
    );
}

#[test]

fn test_int_number_in_function() {
    expect_f64(
        "function compute(x: int, y: number): number {
             return x + y;
         }
         return compute(20, 22.0);",
        42.0,
    );
}

// ============================================================================
// 4. Int Variable Declarations
// ============================================================================

#[test]

fn test_int_variable_explicit_type() {
    expect_i32(
        "let x: int = 42;
         return x;",
        42,
    );
}

#[test]

fn test_int_variable_computed() {
    expect_i32(
        "let a: int = 20;
         let b: int = 22;
         let c: int = a + b;
         return c;",
        42,
    );
}

// ============================================================================
// 5. Comparison Operators with Int and Number
// ============================================================================

#[test]
fn test_int_equality() {
    expect_bool("return 42 == 42;", true);
}

#[test]
fn test_int_inequality() {
    expect_bool("return 42 != 43;", true);
}

#[test]
fn test_int_less_than() {
    expect_bool("return 41 < 42;", true);
}

#[test]
fn test_int_greater_than() {
    expect_bool("return 43 > 42;", true);
}

#[test]
fn test_int_less_equal() {
    expect_bool("return 42 <= 42;", true);
}

#[test]
fn test_int_greater_equal() {
    expect_bool("return 42 >= 42;", true);
}

#[test]
fn test_number_equality() {
    expect_bool("return 3.14 == 3.14;", true);
}

// ============================================================================
// 6. Bitwise Operators (Int Only)
// ============================================================================

#[test]
fn test_bitwise_and() {
    expect_i32("return 0xFF & 0x2A;", 42);
}

#[test]
fn test_bitwise_or() {
    expect_i32("return 0x20 | 0x0A;", 42);
}

#[test]
fn test_bitwise_xor() {
    expect_i32("return 0x3F ^ 0x15;", 42);
}

#[test]
fn test_bitwise_shift_left() {
    expect_i32("return 21 << 1;", 42);
}

#[test]
fn test_bitwise_shift_right() {
    expect_i32("return 84 >> 1;", 42);
}

// ============================================================================
// 7. Int/Number in Collections
// ============================================================================

#[test]

fn test_int_array() {
    expect_i32(
        "let nums: int[] = [10, 20, 12];
         return nums[0] + nums[1] + nums[2];",
        42,
    );
}

#[test]
fn test_number_array() {
    expect_f64(
        "let nums: number[] = [1.5, 2.5, 3.0];
         return nums[0] + nums[1] + nums[2];",
        7.0,
    );
}

// ============================================================================
// 8. Numeric Type in Class Fields
// ============================================================================

#[test]
fn test_int_class_field() {
    expect_i32(
        "class Counter {
             count: number = 0;
             increment(): void { this.count = this.count + 1; }
             get(): number { return this.count; }
         }
         let c = new Counter();
         c.increment();
         c.increment();
         return c.get();",
        2,
    );
}

#[test]
fn test_number_class_field() {
    expect_f64(
        "class Accumulator {
             total: number = 0.0;
             add(x: number): void { this.total = this.total + x; }
         }
         let a = new Accumulator();
         a.add(1.5);
         a.add(2.5);
         return a.total;",
        4.0,
    );
}
