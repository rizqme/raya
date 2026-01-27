//! Phase 8: Array tests
//!
//! Tests for array literals, access, and operations.

use super::harness::*;

// ============================================================================
// Array Literals
// ============================================================================

#[test]
fn test_array_literal_integers() {
    expect_i32("let arr = [1, 2, 3]; return arr[0];", 1);
}

#[test]
fn test_array_literal_empty() {
    expect_i32("let arr: number[] = []; return arr.length;", 0);
}

#[test]
fn test_array_literal_single() {
    expect_i32("let arr = [42]; return arr[0];", 42);
}

// ============================================================================
// Array Access
// ============================================================================

#[test]
fn test_array_access_first() {
    expect_i32("let arr = [10, 20, 30]; return arr[0];", 10);
}

#[test]
fn test_array_access_last() {
    expect_i32("let arr = [10, 20, 30]; return arr[2];", 30);
}

#[test]
fn test_array_access_middle() {
    expect_i32("let arr = [10, 20, 30, 40, 50]; return arr[2];", 30);
}

// ============================================================================
// Array Length
// ============================================================================

#[test]
fn test_array_length() {
    expect_i32("let arr = [1, 2, 3, 4, 5]; return arr.length;", 5);
}

#[test]
fn test_array_length_empty() {
    expect_i32("let arr: number[] = []; return arr.length;", 0);
}

// ============================================================================
// Array Assignment
// ============================================================================

#[test]
fn test_array_assignment() {
    expect_i32("let arr = [1, 2, 3]; arr[1] = 100; return arr[1];", 100);
}

#[test]
fn test_array_assignment_first() {
    expect_i32("let arr = [10, 20, 30]; arr[0] = 5; return arr[0];", 5);
}

// ============================================================================
// Nested Arrays
// ============================================================================

#[test]
fn test_nested_array_access() {
    expect_i32("let matrix = [[1, 2], [3, 4]]; return matrix[1][0];", 3);
}

#[test]
fn test_nested_array_3d() {
    expect_i32(
        "let cube = [[[1, 2], [3, 4]], [[5, 6], [7, 8]]]; return cube[1][0][1];",
        6,
    );
}

// ============================================================================
// Array with Expressions
// ============================================================================

#[test]
fn test_array_computed_index() {
    expect_i32("let arr = [10, 20, 30]; let i = 1; return arr[i];", 20);
}

#[test]
fn test_array_expression_elements() {
    expect_i32("let x = 5; let arr = [x, x + 1, x + 2]; return arr[2];", 7);
}

// ============================================================================
// Array Methods (if supported)
// ============================================================================

#[test]
fn test_array_push() {
    expect_i32("let arr = [1, 2]; arr.push(3); return arr.length;", 3);
}

#[test]
fn test_array_pop() {
    expect_i32("let arr = [1, 2, 3]; return arr.pop();", 3);
}

// ============================================================================
// For-Of Loop with Arrays
// ============================================================================

#[test]
fn test_for_of_array() {
    expect_i32(
        "let arr = [1, 2, 3, 4, 5];
         let sum = 0;
         for (let x of arr) {
             sum = sum + x;
         }
         return sum;",
        15,
    );
}

#[test]
fn test_for_of_array_break() {
    expect_i32(
        "let arr = [1, 2, 3, 4, 5];
         let sum = 0;
         for (let x of arr) {
             if (x > 3) { break; }
             sum = sum + x;
         }
         return sum;",
        6, // 1 + 2 + 3
    );
}
