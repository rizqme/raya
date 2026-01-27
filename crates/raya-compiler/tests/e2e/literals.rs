//! Phase 2: Literals and Basic Expressions tests
//!
//! Tests for all literal types and basic expressions.

use super::harness::*;

// ============================================================================
// Integer Literals
// ============================================================================

#[test]
fn test_integer_literal_positive() {
    expect_i32("return 42;", 42);
}

#[test]
fn test_integer_literal_negative() {
    expect_i32("return -17;", -17);
}

#[test]
fn test_integer_literal_zero() {
    expect_i32("return 0;", 0);
}

#[test]
fn test_integer_literal_large() {
    expect_i32("return 1000000;", 1_000_000);
}

// ============================================================================
// Boolean Literals
// ============================================================================

#[test]
fn test_boolean_true() {
    expect_bool("return true;", true);
}

#[test]
fn test_boolean_false() {
    expect_bool("return false;", false);
}

// ============================================================================
// Null Literal
// ============================================================================

#[test]
fn test_null_literal() {
    expect_null("return null;");
}

// ============================================================================
// Parenthesized Expressions
// ============================================================================

#[test]
fn test_parenthesized_simple() {
    expect_i32("return (42);", 42);
}

#[test]
fn test_parenthesized_arithmetic() {
    expect_i32("return (1 + 2) * 3;", 9);
}

#[test]
fn test_nested_parentheses() {
    expect_i32("return ((1 + 2) * (3 + 4));", 21);
}

// ============================================================================
// Float Literals
// ============================================================================

#[test]
fn test_float_literal_positive() {
    expect_f64("return 3.14;", 3.14);
}

#[test]
fn test_float_literal_negative() {
    expect_f64("return -0.5;", -0.5);
}

#[test]
fn test_float_literal_zero() {
    expect_f64("return 0.0;", 0.0);
}

#[test]
fn test_float_literal_scientific() {
    expect_f64("return 1e10;", 1e10);
}

#[test]
fn test_float_literal_scientific_negative() {
    expect_f64("return 1e-5;", 1e-5);
}

// ============================================================================
// Hexadecimal Literals
// ============================================================================

#[test]
fn test_hex_literal() {
    expect_i32("return 0x1A;", 26);
}

#[test]
fn test_hex_literal_uppercase() {
    expect_i32("return 0xFF;", 255);
}

#[test]
fn test_hex_literal_lowercase() {
    expect_i32("return 0xff;", 255);
}

// ============================================================================
// Octal Literals
// ============================================================================

#[test]
fn test_octal_literal() {
    expect_i32("return 0o755;", 493);
}

#[test]
fn test_octal_literal_simple() {
    expect_i32("return 0o10;", 8);
}

// ============================================================================
// Binary Literals
// ============================================================================

#[test]
fn test_binary_literal() {
    expect_i32("return 0b1010;", 10);
}

#[test]
fn test_binary_literal_byte() {
    expect_i32("return 0b11111111;", 255);
}

// ============================================================================
// Numeric Separators
// ============================================================================

#[test]
fn test_numeric_separator_millions() {
    expect_i32("return 1_000_000;", 1_000_000);
}

#[test]
fn test_numeric_separator_hex() {
    expect_i32("return 0xFF_FF;", 0xFFFF);
}

#[test]
fn test_numeric_separator_binary() {
    expect_i32("return 0b1111_0000;", 0b11110000);
}
