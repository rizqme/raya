//! End-to-end tests for the std:math module
//!
//! Tests verify that math methods compile and execute correctly,
//! returning proper numeric results through the NativeHandler pipeline.

use super::harness::{expect_f64_with_builtins, expect_i32_with_builtins, compile_and_run_with_builtins};

// ============================================================================
// Constants
// ============================================================================

#[test]
fn test_math_pi() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.PI();
    "#, std::f64::consts::PI);
}

#[test]
fn test_math_e() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.E();
    "#, std::f64::consts::E);
}

// ============================================================================
// Basic: abs, sign
// ============================================================================

#[test]
fn test_math_abs_positive() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.abs(5);
    "#, 5.0);
}

#[test]
fn test_math_abs_negative() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.abs(-5);
    "#, 5.0);
}

#[test]
fn test_math_abs_zero() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.abs(0);
    "#, 0.0);
}

#[test]
fn test_math_sign_positive() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.sign(42);
    "#, 1.0);
}

#[test]
fn test_math_sign_negative() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.sign(-3);
    "#, -1.0);
}

// ============================================================================
// Rounding: floor, ceil, round, trunc
// ============================================================================

#[test]
fn test_math_floor() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.floor(3.7);
    "#, 3.0);
}

#[test]
fn test_math_floor_negative() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.floor(-3.2);
    "#, -4.0);
}

#[test]
fn test_math_ceil() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.ceil(3.2);
    "#, 4.0);
}

#[test]
fn test_math_ceil_negative() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.ceil(-3.7);
    "#, -3.0);
}

#[test]
fn test_math_round() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.round(3.5);
    "#, 4.0);
}

#[test]
fn test_math_round_down() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.round(3.4);
    "#, 3.0);
}

#[test]
fn test_math_trunc() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.trunc(3.7);
    "#, 3.0);
}

#[test]
fn test_math_trunc_negative() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.trunc(-3.7);
    "#, -3.0);
}

// ============================================================================
// Min/Max
// ============================================================================

#[test]
fn test_math_min() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.min(3, 7);
    "#, 3.0);
}

#[test]
fn test_math_max() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.max(3, 7);
    "#, 7.0);
}

#[test]
fn test_math_min_equal() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.min(5, 5);
    "#, 5.0);
}

#[test]
fn test_math_max_negative() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.max(-10, -3);
    "#, -3.0);
}

// ============================================================================
// Power: pow, sqrt
// ============================================================================

#[test]
fn test_math_pow() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.pow(2, 10);
    "#, 1024.0);
}

#[test]
fn test_math_pow_fractional() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.pow(4, 0.5);
    "#, 2.0);
}

#[test]
fn test_math_sqrt() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.sqrt(16);
    "#, 4.0);
}

#[test]
fn test_math_sqrt_two() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.sqrt(2);
    "#, std::f64::consts::SQRT_2);
}

// ============================================================================
// Trigonometry
// ============================================================================

#[test]
fn test_math_sin_zero() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.sin(0);
    "#, 0.0);
}

#[test]
fn test_math_cos_zero() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.cos(0);
    "#, 1.0);
}

#[test]
fn test_math_tan_zero() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.tan(0);
    "#, 0.0);
}

#[test]
fn test_math_asin() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.asin(1);
    "#, std::f64::consts::FRAC_PI_2);
}

#[test]
fn test_math_acos() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.acos(1);
    "#, 0.0);
}

#[test]
fn test_math_atan() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.atan(1);
    "#, std::f64::consts::FRAC_PI_4);
}

#[test]
fn test_math_atan2() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.atan2(1, 1);
    "#, std::f64::consts::FRAC_PI_4);
}

// ============================================================================
// Exponential/Logarithmic
// ============================================================================

#[test]
fn test_math_exp_zero() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.exp(0);
    "#, 1.0);
}

#[test]
fn test_math_exp_one() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.exp(1);
    "#, std::f64::consts::E);
}

#[test]
fn test_math_log_one() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.log(1);
    "#, 0.0);
}

#[test]
fn test_math_log10_hundred() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.log10(100);
    "#, 2.0);
}

#[test]
fn test_math_log10_thousand() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        return math.log10(1000);
    "#, 3.0);
}

// ============================================================================
// Random
// ============================================================================

#[test]
fn test_math_random_in_range() {
    let result = compile_and_run_with_builtins(r#"
        import math from "std:math";
        let r: number = math.random();
        if (r >= 0) {
            if (r < 1) {
                return 1;
            }
        }
        return 0;
    "#);
    assert!(result.is_ok(), "math.random() should work: {:?}", result.err());
}

// ============================================================================
// Combined operations
// ============================================================================

#[test]
fn test_math_pow_and_sqrt() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        let x: number = math.pow(5, 2);
        return math.sqrt(x);
    "#, 5.0);
}

#[test]
fn test_math_pi_value() {
    expect_i32_with_builtins(r#"
        import math from "std:math";
        let pi: number = math.PI();
        if (pi > 3) {
            return 1;
        }
        return 0;
    "#, 1);
}

#[test]
fn test_math_clamp() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        let value: number = 15;
        let lo: number = 0;
        let hi: number = 10;
        let clamped_lo: number = math.max(value, lo);
        return math.min(clamped_lo, hi);
    "#, 10.0);
}

#[test]
fn test_math_multiple_operations() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        let a: number = math.abs(-5);
        let b: number = math.floor(3.7);
        let c: number = math.max(a, b);
        return c;
    "#, 5.0);
}

// ============================================================================
// Math in control flow
// ============================================================================

#[test]
fn test_math_in_if() {
    expect_i32_with_builtins(r#"
        import math from "std:math";
        let x: number = -7;
        let ax: number = math.abs(x);
        if (ax > 5) {
            return 1;
        }
        return 0;
    "#, 1);
}

#[test]
fn test_math_in_for_loop() {
    expect_i32_with_builtins(r#"
        import math from "std:math";
        let count: number = 0;
        for (let i: number = 0; i < 5; i = i + 1) {
            let val: number = math.abs(i);
            count = count + 1;
        }
        return count;
    "#, 5);
}

#[test]
fn test_math_in_while_loop() {
    expect_i32_with_builtins(r#"
        import math from "std:math";
        let count: number = 0;
        let x: number = 10;
        while (x > 0) {
            let f: number = math.floor(x);
            x = x - 3;
            count = count + 1;
        }
        return count;
    "#, 4);
}

// ============================================================================
// Import syntax
// ============================================================================

#[test]
fn test_math_import() {
    let result = compile_and_run_with_builtins(r#"
        import math from "std:math";
        let x: number = math.sqrt(9);
        return x;
    "#);
    assert!(result.is_ok(), "Math should be importable from std:math: {:?}", result.err());
}

// ============================================================================
// Nested calls (stdlib method calls inside function bodies)
// ============================================================================

#[test]
fn test_math_call_inside_function() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        function compute(): number {
            return math.abs(-42);
        }
        return compute();
    "#, 42.0);
}

#[test]
fn test_math_multiple_nested_calls() {
    expect_f64_with_builtins(r#"
        import math from "std:math";
        function compute(x: number): number {
            let a: number = math.abs(x);
            let b: number = math.sqrt(a);
            return math.floor(b);
        }
        return compute(-16);
    "#, 4.0);
}
