//! Phase 4: Variable Declarations and Assignment tests
//!
//! Tests for variable declarations, scoping, and assignment.

use super::harness::*;

// ============================================================================
// Let Declarations
// ============================================================================

#[test]
fn test_let_declaration_integer() {
    expect_i32("let x = 42; return x;", 42);
}

#[test]
fn test_let_declaration_float() {
    expect_f64("let x = 3.14; return x;", 3.14);
}

#[test]
fn test_let_declaration_boolean() {
    expect_bool("let x = true; return x;", true);
}

#[test]
fn test_let_declaration_null() {
    expect_null("let x = null; return x;");
}

// ============================================================================
// Variable Assignment
// ============================================================================

#[test]
fn test_variable_assignment() {
    expect_i32("let x = 10; x = 20; return x;", 20);
}

#[test]
fn test_variable_reassignment_multiple() {
    expect_i32("let x = 1; x = 2; x = 3; return x;", 3);
}

#[test]
fn test_variable_use_in_expression() {
    expect_i32("let x = 10; let y = x + 5; return y;", 15);
}

#[test]
fn test_variable_self_reference() {
    expect_i32("let x = 10; x = x + 1; return x;", 11);
}

// ============================================================================
// Multiple Variables
// ============================================================================

#[test]
fn test_multiple_variables() {
    expect_i32("let a = 1; let b = 2; let c = 3; return a + b + c;", 6);
}

#[test]
fn test_variable_swap() {
    expect_i32(
        "let x = 10; let y = 20; let temp = x; x = y; y = temp; return x;",
        20,
    );
}

#[test]
fn test_variable_chain_assignment() {
    expect_i32("let a = 1; let b = a; let c = b; return c;", 1);
}

// ============================================================================
// Variable Shadowing - Same Scope (DISALLOWED)
// ============================================================================

// Note: Raya does NOT allow variable shadowing in the same scope.
// Attempting to redeclare a variable in the same scope should produce an error.

#[test]
fn test_same_scope_shadowing_rejected() {
    // Attempting to declare the same variable twice in the same scope should fail
    expect_compile_error("let x = 10; let x = 20; return x;", "Duplicate");
}

#[test]
fn test_same_scope_shadowing_rejected_different_values() {
    // Even with different values, redeclaration in same scope is an error
    expect_compile_error("let x = 42; let x = 100; return x;", "Duplicate");
}

// ============================================================================
// Variable Shadowing - Nested Scope (ALLOWED)
// ============================================================================

// Note: Raya DOES allow shadowing in nested scopes (inner scope shadows outer).

#[test]
fn test_nested_scope_shadowing_in_block() {
    // Variable in inner block can shadow outer variable
    expect_i32(
        "let x = 10;
         if (true) {
             let x = 20;
             return x;
         }
         return x;",
        20,
    );
}

#[test]
fn test_nested_scope_shadowing_preserves_outer() {
    // After inner block exits, outer variable is accessible again
    expect_i32(
        "let x = 10;
         if (true) {
             let x = 20;
         }
         return x;",
        10,
    );
}

#[test]
fn test_nested_scope_shadowing_in_loop() {
    // Variable in loop body shadows outer variable
    expect_i32(
        "let x = 100;
         let sum = 0;
         let i = 0;
         while (i < 3) {
             let x = i;
             sum = sum + x;
             i = i + 1;
         }
         return sum;",
        3, // 0 + 1 + 2
    );
}

#[test]
fn test_nested_scope_shadowing_multiple_levels() {
    // Shadowing works across multiple nesting levels
    expect_i32(
        "let x = 1;
         if (true) {
             let x = 2;
             if (true) {
                 let x = 3;
                 return x;
             }
         }
         return x;",
        3,
    );
}

// ============================================================================
// Complex Expressions with Variables
// ============================================================================

#[test]
fn test_variable_in_arithmetic() {
    expect_i32("let x = 5; let y = 3; return (x + y) * (x - y);", 16);
}

#[test]
fn test_variable_in_comparison() {
    expect_bool("let x = 10; let y = 5; return x > y;", true);
}

#[test]
fn test_variable_in_logical() {
    expect_bool("let a = true; let b = false; return a && !b;", true);
}

#[test]
fn test_variable_accumulator() {
    expect_i32(
        "let sum = 0; sum = sum + 1; sum = sum + 2; sum = sum + 3; return sum;",
        6,
    );
}
