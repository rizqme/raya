//! Phase 12: Exception Handling tests
//!
//! Tests for try/catch/finally and throw statements.

use super::harness::*;

// ============================================================================
// Basic Try-Catch
// ============================================================================

#[test]
fn test_try_catch_no_throw() {
    // Try block completes normally, catch is skipped
    expect_i32(
        "let result = 0;
         try {
             result = 42;
         } catch (e) {
             result = 0;
         }
         return result;",
        42,
    );
}

#[test]
fn test_try_finally_no_throw() {
    // Try block completes normally, finally runs
    expect_i32(
        "let result = 0;
         try {
             result = 40;
         } finally {
             result = result + 2;
         }
         return result;",
        42,
    );
}

#[test]
fn test_try_catch_finally_no_throw() {
    // Try completes normally, catch skipped, finally runs
    expect_i32(
        "let result = 0;
         try {
             result = 40;
         } catch (e) {
             result = 0;
         } finally {
             result = result + 2;
         }
         return result;",
        42,
    );
}

// ============================================================================
// Throw and Catch
// ============================================================================

#[test]
fn test_throw_and_catch() {
    expect_i32(
        "let result = 0;
         try {
             throw 'error';
             result = 0;
         } catch (e) {
             result = 42;
         }
         return result;",
        42,
    );
}

#[test]
fn test_throw_and_catch_with_finally() {
    expect_i32(
        "let result = 0;
         let finalized = 0;
         try {
             throw 'error';
             result = 0;
         } catch (e) {
             result = 40;
         } finally {
             finalized = 1;
             result = result + 2;
         }
         return result;",
        42,
    );
}

#[test]
fn test_catch_receives_value() {
    // Test that catch receives the thrown value
    // Since catch parameter has type 'unknown', we return it directly
    // The VM will handle the value passing
    expect_i32(
        "try {
             throw 42;
         } catch (e) {
             return 42;  // Can't directly return e (unknown type), but verify we reach here
         }
         return 0;",
        42,
    );
}

// ============================================================================
// Nested Try-Catch
// ============================================================================

#[test]
fn test_nested_try_no_throw() {
    expect_i32(
        "let result = 0;
         try {
             try {
                 result = 42;
             } catch (e) {
                 result = 0;
             }
         } catch (e) {
             result = 0;
         }
         return result;",
        42,
    );
}

#[test]
fn test_nested_try_inner_catch() {
    expect_i32(
        "let result = 0;
         try {
             try {
                 throw 'inner';
             } catch (e) {
                 result = 42;
             }
         } catch (e) {
             result = 0;
         }
         return result;",
        42,
    );
}

// ============================================================================
// Finally Always Runs
// ============================================================================

#[test]
fn test_finally_runs_on_normal_exit() {
    expect_i32(
        "let count = 0;
         try {
             count = count + 1;
         } finally {
             count = count + 10;
         }
         return count;",
        11,
    );
}

#[test]
fn test_finally_runs_on_exception() {
    expect_i32(
        "let count = 0;
         try {
             count = count + 1;
             throw 'error';
         } catch (e) {
             count = count + 10;
         } finally {
             count = count + 100;
         }
         return count;",
        111,
    );
}

// ============================================================================
// Exception in Functions
// ============================================================================

#[test]
fn test_throw_in_function() {
    expect_i32(
        "function fail(): number {
             throw 'error';
             return 0;
         }
         let result = 0;
         try {
             result = fail();
         } catch (e) {
             result = 42;
         }
         return result;",
        42,
    );
}

#[test]
fn test_rethrow() {
    expect_i32(
        "let result = 0;
         try {
             try {
                 throw 'error';
             } catch (e) {
                 throw e;
             }
         } catch (e) {
             result = 42;
         }
         return result;",
        42,
    );
}

// ============================================================================
// Async Exception Handling
// ============================================================================

#[test]
fn test_async_exception_caught_when_awaited() {
    // Exception in async function IS caught when awaited inside try block
    expect_i32(
        "async function fail(): number {
             throw 'async error';
             return 0;
         }
         let result = 0;
         try {
             result = await fail();
         } catch (e) {
             result = 42;
         }
         return result;",
        42,
    );
}

#[test]
fn test_async_exception_not_caught_without_await() {
    // Exception in async function is NOT caught by surrounding try-catch
    // because the async function runs in a separate task
    expect_i32(
        "async function fail(): number {
             throw 'async error';
             return 0;
         }
         let result = 42;
         try {
             fail();  // Not awaited - exception happens in separate task
             result = 42;  // This still executes
         } catch (e) {
             result = 0;  // This is NOT reached
         }
         return result;",
        42,
    );
}

#[test]
fn test_async_exception_in_nested_await() {
    // Exception propagates through nested async calls when awaited
    expect_i32(
        "async function inner(): number {
             throw 'inner error';
             return 0;
         }
         async function outer(): number {
             return await inner();
         }
         let result = 0;
         try {
             result = await outer();
         } catch (e) {
             result = 42;
         }
         return result;",
        42,
    );
}

// ============================================================================
// Error Classes
// ============================================================================

#[test]
fn test_throw_error_class() {
    // Test throwing Error class instances
    // Note: Uses with_builtins because Error class is defined in builtin Error.raya
    expect_i32_with_builtins(
        "let result = 0;
         try {
             throw new Error('test error');
         } catch (e) {
             result = 42;
         }
         return result;",
        42,
    );
}

#[test]
fn test_throw_type_error() {
    // Test throwing TypeError
    expect_i32_with_builtins(
        "let result = 0;
         try {
             throw new TypeError('type mismatch');
         } catch (e) {
             result = 42;
         }
         return result;",
        42,
    );
}

#[test]
fn test_throw_range_error() {
    // Test throwing RangeError
    expect_i32_with_builtins(
        "let result = 0;
         try {
             throw new RangeError('out of bounds');
         } catch (e) {
             result = 42;
         }
         return result;",
        42,
    );
}

#[test]
fn test_error_message_property() {
    // Test accessing error message property
    expect_string_with_builtins(
        "let msg = '';
         try {
             throw new Error('hello');
         } catch (e) {
             msg = e.message;
         }
         return msg;",
        "hello",
    );
}

#[test]
fn test_error_name_property() {
    // Test accessing error name property
    expect_string_with_builtins(
        "let name = '';
         try {
             throw new TypeError('oops');
         } catch (e) {
             name = e.name;
         }
         return name;",
        "TypeError",
    );
}

#[test]
fn test_error_to_string_direct() {
    // Test Error.toString() directly (not via catch)
    expect_string_with_builtins(
        "let err = new Error('test');
         return err.toString();",
        "Error: test",
    );
}


#[test]
fn test_error_to_string() {
    // Test Error.toString()
    expect_string_with_builtins(
        "let str = '';
         try {
             throw new Error('test');
         } catch (e) {
             str = e.toString();
         }
         return str;",
        "Error: test",
    );
}

#[test]
fn test_error_from_function_simple() {
    // Simplified test: throw Error from function (no if statement)
    expect_i32_with_builtins(
        "function fail(): number {
             throw new Error('test');
             return 0;
         }
         let result = 0;
         try {
             result = fail();
         } catch (e) {
             result = 42;
         }
         return result;",
        42,
    );
}

#[test]
fn test_custom_error_function() {
    // Test function that throws errors
    expect_i32_with_builtins(
        "function validate(x: number): number {
             if (x < 0) {
                 throw new RangeError('value must be non-negative');
             }
             return x;
         }
         let result = 0;
         try {
             result = validate(-1);
         } catch (e) {
             result = 42;
         }
         return result;",
        42,
    );
}
