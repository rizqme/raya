//! Phase 7: Functions tests
//!
//! Tests for function declarations, calls, and closures.

use super::harness::*;

// ============================================================================
// Simple Function Declarations
// ============================================================================

#[test]
fn test_function_return_constant() {
    expect_i32(
        "function answer(): number { return 42; }
         return answer();",
        42,
    );
}

#[test]
fn test_function_with_parameter() {
    expect_i32(
        "function double(x: number): number { return x * 2; }
         return double(21);",
        42,
    );
}

#[test]
fn test_function_two_parameters() {
    expect_i32(
        "function add(a: number, b: number): number { return a + b; }
         return add(10, 32);",
        42,
    );
}

#[test]
fn test_function_three_parameters() {
    expect_i32(
        "function sum3(a: number, b: number, c: number): number { return a + b + c; }
         return sum3(10, 20, 12);",
        42,
    );
}

// ============================================================================
// Function Calls in Expressions
// ============================================================================

#[test]
fn test_function_call_in_expression() {
    expect_i32(
        "function square(x: number): number { return x * x; }
         return square(3) + square(4);",
        25,
    );
}

#[test]
fn test_function_call_as_argument() {
    expect_i32(
        "function double(x: number): number { return x * 2; }
         function addOne(x: number): number { return x + 1; }
         return double(addOne(20));",
        42,
    );
}

// ============================================================================
// Recursive Functions
// ============================================================================

#[test]
fn test_factorial_recursive() {
    expect_i32(
        "function factorial(n: number): number {
             if (n <= 1) { return 1; }
             return n * factorial(n - 1);
         }
         return factorial(5);",
        120,
    );
}

#[test]
fn test_fibonacci_recursive() {
    expect_i32(
        "function fib(n: number): number {
             if (n <= 1) { return n; }
             return fib(n - 1) + fib(n - 2);
         }
         return fib(10);",
        55,
    );
}

#[test]
fn test_sum_recursive() {
    expect_i32(
        "function sumTo(n: number): number {
             if (n <= 0) { return 0; }
             return n + sumTo(n - 1);
         }
         return sumTo(10);",
        55,
    );
}

// ============================================================================
// Functions with Local Variables
// ============================================================================

#[test]
fn test_function_with_local() {
    expect_i32(
        "function compute(x: number): number {
             let y = x * 2;
             let z = y + 10;
             return z;
         }
         return compute(5);",
        20,
    );
}

#[test]
fn test_function_multiple_locals() {
    expect_i32(
        "function average(a: number, b: number, c: number): number {
             let sum = a + b + c;
             let count = 3;
             return sum / count;
         }
         return average(10, 20, 30);",
        20,
    );
}

// ============================================================================
// Multiple Functions
// ============================================================================

#[test]
fn test_multiple_functions() {
    expect_i32(
        "function square(x: number): number { return x * x; }
         function cube(x: number): number { return x * x * x; }
         return square(3) + cube(2);",
        17,
    );
}

#[test]
fn test_function_calls_function() {
    expect_i32(
        "function add(a: number, b: number): number { return a + b; }
         function sumThree(a: number, b: number, c: number): number {
             return add(add(a, b), c);
         }
         return sumThree(10, 20, 12);",
        42,
    );
}

// ============================================================================
// Functions Returning Boolean
// ============================================================================

#[test]
fn test_function_return_bool() {
    expect_bool(
        "function isPositive(x: number): boolean { return x > 0; }
         return isPositive(5);",
        true,
    );
}

#[test]
fn test_function_return_bool_false() {
    expect_bool(
        "function isEven(x: number): boolean { return x % 2 == 0; }
         return isEven(7);",
        false,
    );
}

// ============================================================================
// Functions with Conditionals
// ============================================================================

#[test]
fn test_function_with_if() {
    expect_i32(
        "function abs(x: number): number {
             if (x < 0) { return -x; }
             return x;
         }
         return abs(-42);",
        42,
    );
}

#[test]
fn test_function_max() {
    expect_i32(
        "function max(a: number, b: number): number {
             if (a > b) { return a; }
             return b;
         }
         return max(10, 42);",
        42,
    );
}

#[test]
fn test_function_min() {
    expect_i32(
        "function min(a: number, b: number): number {
             if (a < b) { return a; }
             return b;
         }
         return min(10, 42);",
        10,
    );
}

// ============================================================================
// Functions with Loops
// ============================================================================

#[test]
fn test_function_with_loop() {
    expect_i32(
        "function sumTo(n: number): number {
             let sum = 0;
             let i = 1;
             while (i <= n) {
                 sum = sum + i;
                 i = i + 1;
             }
             return sum;
         }
         return sumTo(10);",
        55,
    );
}

#[test]
fn test_function_count_digits() {
    expect_i32(
        "function countDigits(n: number): number {
             let count = 0;
             while (n > 0) {
                 count = count + 1;
                 n = n / 10;
             }
             return count;
         }
         return countDigits(12345);",
        5,
    );
}
