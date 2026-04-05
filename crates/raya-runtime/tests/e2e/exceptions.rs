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
// Error Classes
// ============================================================================

#[test]
fn test_throw_error_class() {
    // Test throwing Error class instances
    // Note: Uses with_builtins because Error class is defined in builtin error.raya
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
             const err = e as Error;
             msg = err.message;
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
             const err = e as TypeError;
             name = err.name;
         }
         return name;",
        "TypeError",
    );
}

#[test]
fn test_error_constructor_options_cause() {
    expect_bool_with_builtins(
        "let inner = new Error('inner');
         let err = new Error('outer', { cause: inner });
         let cause = err.cause as Error;
         return cause.message == 'inner';",
        true,
    );
}

#[test]
fn test_aggregate_error_constructor_options_cause() {
    expect_bool_with_builtins(
        "let root = new Error('root');
         let agg = new AggregateError([new Error('leaf')], 'boom', { cause: root });
         let cause = agg.cause as Error;
         return agg.name == 'AggregateError' && cause.message == 'root' && agg.errors.length == 1;",
        true,
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
             const err = e as Error;
             str = err.toString();
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

// ============================================================================
// Stack Trace Tests
// ============================================================================

#[test]
fn test_error_stack_property_exists() {
    // Test that Error has a stack property that is initially an empty string
    expect_string_with_builtins(
        "let err = new Error('test');
         return err.stack;",
        "",
    );
}

#[test]
fn test_error_stack_trace_simple() {
    // Test that stack trace contains the error header
    expect_string_contains_with_builtins(
        "let stack = '';
         try {
             throw new Error('test error');
         } catch (e) {
             const err = e as Error;
             stack = err.stack;
         }
         return stack;",
        "Error: test error",
    );
}

#[test]
fn test_error_stack_trace_from_function() {
    // Test that stack trace contains function name
    expect_string_contains_with_builtins(
        "function fail(): void {
             throw new Error('oops');
         }
         let stack = '';
         try {
             fail();
         } catch (e) {
             const err = e as Error;
             stack = err.stack;
         }
         return stack;",
        "at fail",
    );
}

#[test]
fn test_error_stack_trace_nested_functions() {
    // Test that stack trace contains nested function names
    expect_string_contains_with_builtins(
        "function inner(): void {
             throw new Error('deep');
         }
         function outer(): void {
             inner();
         }
         let stack = '';
         try {
             outer();
         } catch (e) {
             const err = e as Error;
             stack = err.stack;
         }
         return stack;",
        "at inner",
    );
}

#[test]
fn test_type_error_stack_trace() {
    // Test that TypeError also gets stack trace
    expect_string_contains_with_builtins(
        "let stack = '';
         try {
             throw new TypeError('bad type');
         } catch (e) {
             const err = e as TypeError;
             stack = err.stack;
         }
         return stack;",
        "TypeError: bad type",
    );
}

#[test]
fn test_structural_error_like_object_gets_stack_trace() {
    expect_bool(
        "let stack = '';
         try {
             throw { message: 'boom', name: 'CustomError', stack: '' };
         } catch (e) {
             const err = e as { message: string, name: string, stack: string };
             stack = err.stack;
         }
         return stack != '' && stack.includes('CustomError: boom');",
        true,
    );
}

#[test]
fn test_custom_error_class_stack_trace_uses_structural_surface() {
    expect_bool(
        "class MyError {
             message: string;
             name: string = 'MyError';
             stack: string = '';

             constructor(message: string) {
                 this.message = message;
             }
         }
         let stack = '';
         try {
             throw new MyError('boom');
         } catch (e) {
             const err = e as { stack: string };
             stack = err.stack;
         }
         return stack != '' && stack.includes('MyError: boom');",
        true,
    );
}
