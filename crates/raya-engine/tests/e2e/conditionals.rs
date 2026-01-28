//! Phase 5: Control Flow - Conditionals tests
//!
//! Tests for if/else statements and conditional expressions.

use super::harness::*;

// ============================================================================
// Simple If Statements
// ============================================================================

#[test]
fn test_if_true_branch() {
    expect_i32("if (true) { return 1; } return 0;", 1);
}

#[test]
fn test_if_false_branch() {
    expect_i32("if (false) { return 1; } return 0;", 0);
}

#[test]
fn test_if_with_variable() {
    expect_i32("let x = 10; if (x > 5) { return 1; } return 0;", 1);
}

// ============================================================================
// If-Else Statements
// ============================================================================

#[test]
fn test_if_else_true() {
    expect_i32("if (true) { return 1; } else { return 2; }", 1);
}

#[test]
fn test_if_else_false() {
    expect_i32("if (false) { return 1; } else { return 2; }", 2);
}

#[test]
fn test_if_else_with_comparison() {
    expect_i32("let n = 3; if (n % 2 == 0) { return 1; } else { return 2; }", 2);
}

#[test]
fn test_if_else_even_odd() {
    expect_i32("let n = 4; if (n % 2 == 0) { return 1; } else { return 0; }", 1);
}

// ============================================================================
// If-Else-If Chains
// ============================================================================

#[test]
fn test_if_elseif_first() {
    expect_i32(
        "let x = 1; if (x == 1) { return 10; } else if (x == 2) { return 20; } else { return 30; }",
        10,
    );
}

#[test]
fn test_if_elseif_second() {
    expect_i32(
        "let x = 2; if (x == 1) { return 10; } else if (x == 2) { return 20; } else { return 30; }",
        20,
    );
}

#[test]
fn test_if_elseif_else() {
    expect_i32(
        "let x = 3; if (x == 1) { return 10; } else if (x == 2) { return 20; } else { return 30; }",
        30,
    );
}

#[test]
fn test_grade_calculation() {
    expect_i32(
        "let grade = 85;
         if (grade >= 90) { return 4; }
         else if (grade >= 80) { return 3; }
         else if (grade >= 70) { return 2; }
         else { return 1; }",
        3,
    );
}

// ============================================================================
// Nested Conditionals
// ============================================================================

#[test]
fn test_nested_if() {
    expect_i32(
        "let x = 5; let y = 10;
         if (x > 0) {
             if (y > 5) { return 1; }
             else { return 2; }
         }
         return 0;",
        1,
    );
}

#[test]
fn test_nested_if_outer_false() {
    expect_i32(
        "let x = -1; let y = 10;
         if (x > 0) {
             if (y > 5) { return 1; }
             else { return 2; }
         }
         return 0;",
        0,
    );
}

// ============================================================================
// Ternary Operator
// ============================================================================

#[test]
fn test_ternary_true() {
    expect_i32("return true ? 1 : 2;", 1);
}

#[test]
fn test_ternary_false() {
    expect_i32("return false ? 1 : 2;", 2);
}

#[test]
fn test_ternary_with_comparison() {
    expect_i32("let x = 10; return x > 5 ? 100 : 200;", 100);
}

#[test]
fn test_ternary_nested() {
    expect_i32("let x = 2; return x == 1 ? 10 : (x == 2 ? 20 : 30);", 20);
}

#[test]
fn test_ternary_in_expression() {
    expect_i32("let x = 5; let y = (x > 3 ? 10 : 20) + 5; return y;", 15);
}

// ============================================================================
// Conditionals with Side Effects
// ============================================================================

#[test]
fn test_if_with_assignment() {
    expect_i32(
        "let result = 0;
         if (true) { result = 42; }
         return result;",
        42,
    );
}

#[test]
fn test_if_else_with_assignment() {
    expect_i32(
        "let result = 0;
         let condition = false;
         if (condition) { result = 10; }
         else { result = 20; }
         return result;",
        20,
    );
}

// ============================================================================
// Complex Conditional Logic
// ============================================================================

#[test]
fn test_logical_and_short_circuit() {
    // If first operand is false, second is not evaluated
    expect_bool("return false && true;", false);
}

#[test]
fn test_logical_or_short_circuit() {
    // If first operand is true, second is not evaluated
    expect_bool("return true || false;", true);
}

#[test]
fn test_complex_condition() {
    expect_bool(
        "let x = 5; let y = 10;
         return (x > 0 && y > 0) || (x < 0 && y < 0);",
        true,
    );
}
