//! Phase 3: Arithmetic and Comparison Operators tests
//!
//! Tests for all operators with proper type handling.

use super::harness::*;

// ============================================================================
// Integer Arithmetic
// ============================================================================

#[test]
fn test_integer_add() {
    expect_i32("return 10 + 5;", 15);
}

#[test]
fn test_integer_subtract() {
    expect_i32("return 10 - 5;", 5);
}

#[test]
fn test_integer_multiply() {
    expect_i32("return 10 * 5;", 50);
}

#[test]
fn test_integer_divide() {
    expect_i32("return 10 / 3;", 3); // Integer division
}

#[test]
fn test_integer_modulo() {
    expect_i32("return 10 % 3;", 1);
}

#[test]
fn test_integer_complex_expression() {
    expect_i32("return 2 + 3 * 4;", 14); // Precedence: 2 + 12 = 14
}

#[test]
fn test_integer_parentheses_precedence() {
    expect_i32("return (2 + 3) * 4;", 20);
}

// ============================================================================
// Float Arithmetic
// ============================================================================

#[test]
fn test_float_add() {
    expect_f64("return 10.0 + 5.5;", 15.5);
}

#[test]
fn test_float_subtract() {
    expect_f64("return 10.0 - 5.5;", 4.5);
}

#[test]
fn test_float_multiply() {
    expect_f64("return 10.0 * 2.0;", 20.0);
}

#[test]
fn test_float_divide() {
    expect_f64("return 10.0 / 4.0;", 2.5);
}

// ============================================================================
// Comparison Operators (Integer)
// ============================================================================

#[test]
fn test_integer_equal() {
    expect_bool("return 5 == 5;", true);
}

#[test]
fn test_integer_equal_false() {
    expect_bool("return 5 == 3;", false);
}

#[test]
fn test_integer_not_equal() {
    expect_bool("return 5 != 3;", true);
}

#[test]
fn test_integer_not_equal_false() {
    expect_bool("return 5 != 5;", false);
}

#[test]
fn test_integer_less_than() {
    expect_bool("return 3 < 5;", true);
}

#[test]
fn test_integer_less_than_false() {
    expect_bool("return 5 < 3;", false);
}

#[test]
fn test_integer_less_equal() {
    expect_bool("return 5 <= 5;", true);
}

#[test]
fn test_integer_less_equal_less() {
    expect_bool("return 4 <= 5;", true);
}

#[test]
fn test_integer_greater_than() {
    expect_bool("return 7 > 3;", true);
}

#[test]
fn test_integer_greater_than_false() {
    expect_bool("return 3 > 7;", false);
}

#[test]
fn test_integer_greater_equal() {
    expect_bool("return 5 >= 5;", true);
}

#[test]
fn test_integer_greater_equal_greater() {
    expect_bool("return 6 >= 5;", true);
}

// ============================================================================
// Logical Operators
// ============================================================================

#[test]
fn test_logical_and_true() {
    expect_bool("return true && true;", true);
}

#[test]
fn test_logical_and_false() {
    expect_bool("return true && false;", false);
}

#[test]
fn test_logical_or_true() {
    expect_bool("return true || false;", true);
}

#[test]
fn test_logical_or_false() {
    expect_bool("return false || false;", false);
}

#[test]
fn test_logical_not_true() {
    expect_bool("return !true;", false);
}

#[test]
fn test_logical_not_false() {
    expect_bool("return !false;", true);
}

// ============================================================================
// Unary Operators
// ============================================================================

#[test]
fn test_unary_negate_integer() {
    expect_i32("return -42;", -42);
}

#[test]
fn test_unary_negate_float() {
    expect_f64("return -3.14;", -3.14);
}

#[test]
fn test_unary_negate_expression() {
    expect_i32("return -(10 + 5);", -15);
}

#[test]
fn test_unary_double_negate() {
    expect_i32("return -(-42);", 42);
}

// ============================================================================
// Bitwise Operators
// ============================================================================

#[test]
fn test_bitwise_and() {
    expect_i32("return 0b1100 & 0b1010;", 0b1000); // 12 & 10 = 8
}

#[test]
fn test_bitwise_or() {
    expect_i32("return 0b1100 | 0b1010;", 0b1110); // 12 | 10 = 14
}

#[test]
fn test_bitwise_xor() {
    expect_i32("return 0b1100 ^ 0b1010;", 0b0110); // 12 ^ 10 = 6
}

#[test]
fn test_bitwise_not() {
    expect_i32("return ~0;", -1);
}

#[test]
fn test_left_shift() {
    expect_i32("return 1 << 4;", 16);
}

#[test]
fn test_right_shift() {
    expect_i32("return 16 >> 2;", 4);
}

#[test]
fn test_unsigned_right_shift() {
    expect_i32("return -1 >>> 28;", 15);
}

// ============================================================================
// Exponentiation Operator
// ============================================================================

#[test]
fn test_exponentiation() {
    expect_i32("return 2 ** 10;", 1024);
}

#[test]
fn test_exponentiation_float() {
    expect_f64("return 2.0 ** 0.5;", std::f64::consts::SQRT_2);
}

// ============================================================================
// Nullish Coalescing Operator
// ============================================================================

#[test]
fn test_nullish_coalescing_null() {
    expect_i32("let x: number | null = null; return x ?? 42;", 42);
}

#[test]
fn test_nullish_coalescing_non_null() {
    expect_i32("let x: number | null = 10; return x ?? 42;", 10);
}

// ============================================================================
// Compound Assignment Operators
// ============================================================================

#[test]
fn test_compound_add_assign() {
    expect_i32("let x = 10; x += 5; return x;", 15);
}

#[test]
fn test_compound_sub_assign() {
    expect_i32("let x = 10; x -= 3; return x;", 7);
}

#[test]
fn test_compound_mul_assign() {
    expect_i32("let x = 10; x *= 4; return x;", 40);
}

#[test]
fn test_compound_div_assign() {
    expect_i32("let x = 20; x /= 4; return x;", 5);
}

#[test]
fn test_compound_mod_assign() {
    expect_i32("let x = 17; x %= 5; return x;", 2);
}
