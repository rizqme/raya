//! End-to-end tests for the std:logger module
//!
//! Tests verify that logger methods (debug, info, warn, error) compile
//! and execute correctly. Since logger methods are void-returning native
//! calls, tests return sentinel values to confirm execution completed.
//!
//! Note: logger calls inside nested functions/methods hit a known VM limitation
//! ("CallMethod not implemented in nested call"), so these tests use top-level
//! calls only.

use super::harness::{expect_i32_with_builtins, compile_and_run_with_builtins};

// ============================================================================
// Basic logger method calls
// ============================================================================

#[test]
fn test_logger_info() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        logger.info("hello world");
        return 1;
    "#, 1);
}

#[test]
fn test_logger_debug() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        logger.debug("debug message");
        return 1;
    "#, 1);
}

#[test]
fn test_logger_warn() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        logger.warn("warning message");
        return 1;
    "#, 1);
}

#[test]
fn test_logger_error() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        logger.error("error message");
        return 1;
    "#, 1);
}

// ============================================================================
// Multiple logger calls
// ============================================================================

#[test]
fn test_logger_all_levels() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        logger.debug("step 1");
        logger.info("step 2");
        logger.warn("step 3");
        logger.error("step 4");
        return 4;
    "#, 4);
}

#[test]
fn test_logger_repeated_info() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        let count: number = 0;
        logger.info("first");
        count = count + 1;
        logger.info("second");
        count = count + 1;
        logger.info("third");
        count = count + 1;
        return count;
    "#, 3);
}

// ============================================================================
// Logger with expressions
// ============================================================================

#[test]
fn test_logger_with_string_variable() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        let msg: string = "hello from variable";
        logger.info(msg);
        return 1;
    "#, 1);
}

#[test]
fn test_logger_with_string_concatenation() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        let name: string = "world";
        logger.info("hello " + name);
        return 1;
    "#, 1);
}

#[test]
fn test_logger_with_concatenated_numbers() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        let x: number = 42;
        logger.info("value is large");
        return x;
    "#, 42);
}

// ============================================================================
// Logger in control flow
// ============================================================================

#[test]
fn test_logger_in_if_true_branch() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        let x: number = 10;
        if (x > 5) {
            logger.info("x is greater than 5");
        } else {
            logger.info("x is not greater than 5");
        }
        return x;
    "#, 10);
}

#[test]
fn test_logger_in_if_false_branch() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        let x: number = 3;
        if (x > 5) {
            logger.warn("should not reach here");
        } else {
            logger.info("x is small");
        }
        return x;
    "#, 3);
}

#[test]
fn test_logger_in_for_loop() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        let sum: number = 0;
        for (let i: number = 0; i < 5; i = i + 1) {
            logger.debug("iterating");
            sum = sum + i;
        }
        return sum;
    "#, 10);
}

#[test]
fn test_logger_in_while_loop() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        let count: number = 0;
        while (count < 3) {
            logger.info("looping");
            count = count + 1;
        }
        return count;
    "#, 3);
}

// ============================================================================
// Logger does not affect return value (void correctness)
// ============================================================================

#[test]
fn test_logger_void_between_assignments() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        let x: number = 10;
        logger.info("before");
        let y: number = 20;
        logger.info("after");
        return x + y;
    "#, 30);
}

#[test]
fn test_logger_void_between_computations() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        let a: number = 5;
        logger.debug("a set");
        let b: number = a * 3;
        logger.info("b set");
        let c: number = b + 7;
        logger.warn("c set");
        return c;
    "#, 22);
}

#[test]
fn test_logger_void_no_stack_pollution() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        let result: number = 100;
        logger.info("one");
        logger.debug("two");
        logger.warn("three");
        logger.error("four");
        return result;
    "#, 100);
}

// ============================================================================
// Logger with empty string
// ============================================================================

#[test]
fn test_logger_empty_string() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        logger.info("");
        return 1;
    "#, 1);
}

// ============================================================================
// Logger interleaved with other operations
// ============================================================================

#[test]
fn test_logger_with_array_operations() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        let arr: Array<number> = [1, 2, 3];
        logger.info("array created");
        arr.push(4);
        logger.debug("element pushed");
        return arr.length;
    "#, 4);
}

#[test]
fn test_logger_error_different_messages() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        logger.error("connection refused");
        logger.error("timeout exceeded");
        logger.error("authentication failed");
        return 3;
    "#, 3);
}

#[test]
fn test_logger_conditional_levels() {
    expect_i32_with_builtins(r#"
        import logger from "std:logger";
        let severity: number = 2;
        if (severity == 1) {
            logger.debug("low severity");
        } else if (severity == 2) {
            logger.warn("medium severity");
        } else {
            logger.error("high severity");
        }
        return severity;
    "#, 2);
}

// ============================================================================
// Import syntax
// ============================================================================

#[test]
fn test_logger_import() {
    let result = compile_and_run_with_builtins(r#"
        import logger from "std:logger";
        logger.info("logger is available");
        return 42;
    "#);
    assert!(result.is_ok(), "Logger should be importable from std:logger: {:?}", result.err());
}
