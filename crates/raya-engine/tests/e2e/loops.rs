//! Phase 6: Control Flow - Loops tests
//!
//! Tests for while, for, and do-while loops.

use super::harness::*;

// ============================================================================
// While Loops
// ============================================================================

#[test]
fn test_while_simple() {
    expect_i32(
        "let i = 0;
         while (i < 5) { i = i + 1; }
         return i;",
        5,
    );
}

#[test]
fn test_while_sum() {
    expect_i32(
        "let sum = 0;
         let i = 1;
         while (i <= 10) {
             sum = sum + i;
             i = i + 1;
         }
         return sum;",
        55,
    );
}

#[test]
fn test_while_never_runs() {
    expect_i32(
        "let x = 0;
         while (false) { x = 100; }
         return x;",
        0,
    );
}

#[test]
fn test_while_countdown() {
    expect_i32(
        "let n = 10;
         while (n > 0) { n = n - 1; }
         return n;",
        0,
    );
}

// ============================================================================
// For Loops
// ============================================================================

#[test]
fn test_for_simple() {
    expect_i32(
        "let sum = 0;
         for (let i = 1; i <= 5; i = i + 1) {
             sum = sum + i;
         }
         return sum;",
        15,
    );
}

#[test]
fn test_for_factorial() {
    expect_i32(
        "let product = 1;
         for (let i = 1; i <= 5; i = i + 1) {
             product = product * i;
         }
         return product;",
        120,
    );
}

#[test]
fn test_for_countdown() {
    expect_i32(
        "let result = 0;
         for (let i = 10; i > 0; i = i - 1) {
             result = i;
         }
         return result;",
        1,
    );
}

// ============================================================================
// Break Statement
// ============================================================================

#[test]
fn test_while_break() {
    expect_i32(
        "let i = 0;
         while (true) {
             if (i == 5) { break; }
             i = i + 1;
         }
         return i;",
        5,
    );
}

#[test]
fn test_for_break() {
    expect_i32(
        "let result = 0;
         for (let i = 0; i < 100; i = i + 1) {
             if (i == 10) { break; }
             result = i;
         }
         return result;",
        9,
    );
}

// ============================================================================
// Continue Statement
// ============================================================================

#[test]
fn test_while_continue() {
    expect_i32(
        "let sum = 0;
         let i = 0;
         while (i < 10) {
             i = i + 1;
             if (i % 2 == 0) { continue; }
             sum = sum + i;
         }
         return sum;",
        25, // 1 + 3 + 5 + 7 + 9
    );
}

#[test]
fn test_for_continue() {
    expect_i32(
        "let sum = 0;
         for (let i = 0; i < 10; i = i + 1) {
             if (i % 2 == 0) { continue; }
             sum = sum + i;
         }
         return sum;",
        25, // 1 + 3 + 5 + 7 + 9
    );
}

// ============================================================================
// Nested Loops
// ============================================================================

#[test]
fn test_nested_while() {
    expect_i32(
        "let count = 0;
         let i = 0;
         while (i < 3) {
             let j = 0;
             while (j < 3) {
                 count = count + 1;
                 j = j + 1;
             }
             i = i + 1;
         }
         return count;",
        9,
    );
}

#[test]
fn test_nested_for() {
    expect_i32(
        "let count = 0;
         for (let i = 0; i < 3; i = i + 1) {
             for (let j = 0; j < 4; j = j + 1) {
                 count = count + 1;
             }
         }
         return count;",
        12,
    );
}

// ============================================================================
// Complex Loop Patterns
// ============================================================================

#[test]
fn test_find_first_divisible() {
    expect_i32(
        "let result = 0;
         for (let i = 1; i <= 20; i = i + 1) {
             if (i % 7 == 0) {
                 result = i;
                 break;
             }
         }
         return result;",
        7,
    );
}

#[test]
fn test_sum_until_threshold() {
    expect_i32(
        "let sum = 0;
         let i = 1;
         while (sum < 50) {
             sum = sum + i;
             i = i + 1;
         }
         return sum;",
        55, // 1+2+3+4+5+6+7+8+9+10 = 55
    );
}
