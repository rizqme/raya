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
fn test_array_push_return_value() {
    expect_i32("let arr = [1, 2]; return arr.push(3);", 3);
}

#[test]
fn test_array_push_access_new_element() {
    expect_i32("let arr = [1, 2]; arr.push(99); return arr[2];", 99);
}

#[test]
fn test_array_pop() {
    expect_i32("let arr = [1, 2, 3]; return arr.pop();", 3);
}

#[test]
fn test_array_pop_length() {
    expect_i32("let arr = [1, 2, 3]; arr.pop(); return arr.length;", 2);
}

#[test]
fn test_array_shift() {
    expect_i32("let arr = [1, 2, 3]; return arr.shift();", 1);
}

#[test]
fn test_array_shift_length() {
    expect_i32("let arr = [1, 2, 3]; arr.shift(); return arr.length;", 2);
}

#[test]
fn test_array_shift_remaining() {
    expect_i32("let arr = [1, 2, 3]; arr.shift(); return arr[0];", 2);
}

#[test]
fn test_array_unshift() {
    expect_i32("let arr = [2, 3]; return arr.unshift(1);", 3);
}

#[test]
fn test_array_unshift_access() {
    expect_i32("let arr = [2, 3]; arr.unshift(1); return arr[0];", 1);
}

#[test]
fn test_array_index_of_found() {
    expect_i32("let arr = [10, 20, 30, 40]; return arr.indexOf(30);", 2);
}

#[test]
fn test_array_index_of_first() {
    expect_i32("let arr = [10, 20, 30]; return arr.indexOf(10);", 0);
}

#[test]
fn test_array_index_of_not_found() {
    expect_i32("let arr = [10, 20, 30]; return arr.indexOf(999);", -1);
}

#[test]
fn test_array_includes_true() {
    expect_bool("let arr = [1, 2, 3]; return arr.includes(2);", true);
}

#[test]
fn test_array_includes_false() {
    expect_bool("let arr = [1, 2, 3]; return arr.includes(999);", false);
}

// ============================================================================
// Additional Array Methods
// ============================================================================

#[test]
fn test_array_slice() {
    expect_i32("let arr = [1, 2, 3, 4, 5]; let s = arr.slice(1, 3); return s.length;", 2);
}

#[test]
fn test_array_slice_content() {
    expect_i32("let arr = [10, 20, 30, 40, 50]; let s = arr.slice(1, 3); return s[0];", 20);
}

#[test]
fn test_array_slice_second_element() {
    expect_i32("let arr = [10, 20, 30, 40, 50]; let s = arr.slice(1, 3); return s[1];", 30);
}

#[test]
fn test_array_concat() {
    expect_i32("let a = [1, 2]; let b = [3, 4]; let c = a.concat(b); return c.length;", 4);
}

#[test]
fn test_array_concat_content() {
    expect_i32("let a = [1, 2]; let b = [3, 4]; let c = a.concat(b); return c[2];", 3);
}

#[test]
fn test_array_reverse() {
    expect_i32("let arr = [1, 2, 3]; arr.reverse(); return arr[0];", 3);
}

#[test]
fn test_array_reverse_last() {
    expect_i32("let arr = [1, 2, 3]; arr.reverse(); return arr[2];", 1);
}

#[test]
fn test_array_join() {
    expect_string("let arr = [1, 2, 3]; return arr.join(',');", "1,2,3");
}

#[test]
fn test_array_join_empty_separator() {
    expect_string("let arr = [1, 2, 3]; return arr.join('');", "123");
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

// ============================================================================
// Callback-Based Array Methods
// ============================================================================

#[test]
fn test_array_filter() {
    expect_i32(
        "let arr = [1, 2, 3, 4, 5];
         let evens = arr.filter((x: number): boolean => x % 2 == 0);
         return evens.length;",
        2,
    );
}

#[test]
fn test_array_filter_content() {
    expect_i32(
        "let arr = [1, 2, 3, 4, 5];
         let evens = arr.filter((x: number): boolean => x % 2 == 0);
         return evens[0];",
        2,
    );
}

#[test]
fn test_array_filter_second() {
    expect_i32(
        "let arr = [1, 2, 3, 4, 5];
         let evens = arr.filter((x: number): boolean => x % 2 == 0);
         return evens[1];",
        4,
    );
}

#[test]
fn test_array_find() {
    expect_i32(
        "let arr = [1, 2, 3, 4, 5];
         let found = arr.find((x: number): boolean => x > 3);
         return found;",
        4,
    );
}

#[test]
fn test_array_find_index() {
    expect_i32(
        "let arr = [10, 20, 30, 40, 50];
         return arr.findIndex((x: number): boolean => x > 25);",
        2,
    );
}

#[test]
fn test_array_find_index_not_found() {
    expect_i32(
        "let arr = [10, 20, 30];
         return arr.findIndex((x: number): boolean => x > 100);",
        -1,
    );
}

#[test]
fn test_array_every_true() {
    expect_bool(
        "let arr = [2, 4, 6, 8];
         return arr.every((x: number): boolean => x % 2 == 0);",
        true,
    );
}

#[test]
fn test_array_every_false() {
    expect_bool(
        "let arr = [2, 4, 5, 8];
         return arr.every((x: number): boolean => x % 2 == 0);",
        false,
    );
}

#[test]
fn test_array_some_true() {
    expect_bool(
        "let arr = [1, 3, 5, 6];
         return arr.some((x: number): boolean => x % 2 == 0);",
        true,
    );
}

#[test]
fn test_array_some_false() {
    expect_bool(
        "let arr = [1, 3, 5, 7];
         return arr.some((x: number): boolean => x % 2 == 0);",
        false,
    );
}

#[test]
fn test_array_foreach() {
    expect_i32(
        "let arr = [1, 2, 3];
         let sum = 0;
         arr.forEach((x: number): void => { sum = sum + x; });
         return sum;",
        6,
    );
}
