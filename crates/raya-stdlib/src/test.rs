//! Test module native implementations.
//!
//! Provides native functions for the `std:test` module:
//! - Result reporting (pass/fail/skip)
//! - Deep equality comparison
//! - String matching
//! - Contains check

use parking_lot::Mutex;
use raya_sdk::{NativeCallResult, NativeContext, NativeFunctionRegistry, NativeValue};
use std::sync::Arc;

// ============================================================================
// Test Result Types
// ============================================================================

/// Result of a single test case.
#[derive(Debug, Clone)]
pub struct TestResult {
    /// Full test name (e.g., "suite > nested > test name")
    pub name: String,
    /// Whether the test passed
    pub passed: bool,
    /// Error message if the test failed
    pub error_message: Option<String>,
    /// Duration in milliseconds
    pub duration_ms: f64,
}

/// Collected results from a test file execution.
#[derive(Debug, Clone, Default)]
pub struct TestResults {
    /// Individual test results in execution order
    pub results: Vec<TestResult>,
    /// Total number of tests registered
    pub total_registered: usize,
}

impl TestResults {
    /// Number of passed tests.
    pub fn passed(&self) -> usize {
        self.results.iter().filter(|r| r.passed).count()
    }

    /// Number of failed tests.
    pub fn failed(&self) -> usize {
        self.results.iter().filter(|r| !r.passed).count()
    }

    /// Total duration in milliseconds.
    pub fn total_duration_ms(&self) -> f64 {
        self.results.iter().map(|r| r.duration_ms).sum()
    }
}

/// Shared handle for collecting test results across native calls.
pub type SharedTestResults = Arc<Mutex<TestResults>>;

/// Create a new shared test results collector.
pub fn new_results() -> SharedTestResults {
    Arc::new(Mutex::new(TestResults::default()))
}

// ============================================================================
// Registry
// ============================================================================

/// Register test native functions into the given registry.
///
/// The `results` handle is shared between the native handlers and the caller
/// (typically the test runner CLI), allowing the caller to read results after
/// module execution completes.
pub fn register_test(registry: &mut NativeFunctionRegistry, results: SharedTestResults) {
    // test.reportStart(count: number)
    let r = results.clone();
    registry.register("test.reportStart", move |_ctx, args| {
        let count = args
            .first()
            .and_then(|v| v.as_i32())
            .unwrap_or(0) as usize;
        r.lock().total_registered = count;
        NativeCallResult::null()
    });

    // test.reportPass(name: string, duration: number)
    let r = results.clone();
    registry.register("test.reportPass", move |ctx, args| {
        let name = args
            .first()
            .and_then(|v| ctx.read_string(*v).ok())
            .unwrap_or_default();
        let duration = args
            .get(1)
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        r.lock().results.push(TestResult {
            name,
            passed: true,
            error_message: None,
            duration_ms: duration,
        });
        NativeCallResult::null()
    });

    // test.reportFail(name: string, message: string, duration: number)
    let r = results.clone();
    registry.register("test.reportFail", move |ctx, args| {
        let name = args
            .first()
            .and_then(|v| ctx.read_string(*v).ok())
            .unwrap_or_default();
        let message = args
            .get(1)
            .and_then(|v| ctx.read_string(*v).ok())
            .unwrap_or_default();
        let duration = args
            .get(2)
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        r.lock().results.push(TestResult {
            name,
            passed: false,
            error_message: Some(message),
            duration_ms: duration,
        });
        NativeCallResult::null()
    });

    // test.reportEnd(passed: number, failed: number)
    registry.register("test.reportEnd", |_ctx, _args| {
        NativeCallResult::null()
    });

    // test.deepEqual(a: any, b: any) -> boolean
    registry.register("test.deepEqual", |ctx, args| {
        let a = args.first().copied().unwrap_or_else(NativeValue::null);
        let b = args.get(1).copied().unwrap_or_else(NativeValue::null);
        let equal = deep_equal(ctx, a, b);
        NativeCallResult::bool(equal)
    });

    // test.contains(haystack: any, needle: any) -> boolean
    registry.register("test.contains", |ctx, args| {
        let haystack = args.first().copied().unwrap_or_else(NativeValue::null);
        let needle = args.get(1).copied().unwrap_or_else(NativeValue::null);
        let result = contains_check(ctx, haystack, needle);
        NativeCallResult::bool(result)
    });

    // test.stringMatch(str: string, pattern: string) -> boolean
    registry.register("test.stringMatch", |ctx, args| {
        let s = args
            .first()
            .and_then(|v| ctx.read_string(*v).ok())
            .unwrap_or_default();
        let pattern = args
            .get(1)
            .and_then(|v| ctx.read_string(*v).ok())
            .unwrap_or_default();
        // Simple substring match
        NativeCallResult::bool(s.contains(&pattern))
    });

    // test.stringify(value: any) -> string
    // Converts any value to its string representation for error messages.
    registry.register("test.stringify", |ctx, args| {
        let val = args.first().copied().unwrap_or_else(NativeValue::null);
        let s = stringify_value(ctx, val);
        NativeCallResult::Value(ctx.create_string(&s))
    });

    // test.isTruthy(value: any) -> boolean
    // Returns whether a value is truthy (non-null, non-false, non-zero, non-empty-string).
    registry.register("test.isTruthy", |ctx, args| {
        let val = args.first().copied().unwrap_or_else(NativeValue::null);
        NativeCallResult::bool(is_truthy(ctx, val))
    });

    // test.getErrorMessage(error: any) -> string
    // Reads the `message` field (index 0) from an Error object.
    registry.register("test.getErrorMessage", |ctx, args| {
        let err = args.first().copied().unwrap_or_else(NativeValue::null);
        if err.is_null() {
            return NativeCallResult::Value(ctx.create_string("unknown error"));
        }
        // Error.message is field 0
        match ctx.object_get_field(err, 0) {
            Ok(msg_val) => match ctx.read_string(msg_val) {
                Ok(s) => NativeCallResult::Value(ctx.create_string(&s)),
                Err(_) => NativeCallResult::Value(ctx.create_string("unknown error")),
            },
            Err(_) => NativeCallResult::Value(ctx.create_string("unknown error")),
        }
    });

    // test.isNull(value) -> boolean
    registry.register("test.isNull", |_ctx, args| {
        let val = args.first().copied().unwrap_or_else(NativeValue::null);
        NativeCallResult::bool(val.is_null())
    });

    // test.compare(a, b, op: string) -> boolean
    // Numeric comparison: supports ">", ">=", "<", "<="
    registry.register("test.compare", |ctx, args| {
        let a = args.first().copied().unwrap_or_else(NativeValue::null);
        let b = args.get(1).copied().unwrap_or_else(NativeValue::null);
        let op = args
            .get(2)
            .and_then(|v| ctx.read_string(*v).ok())
            .unwrap_or_default();

        let af = a
            .as_f64()
            .or_else(|| a.as_i32().map(|i| i as f64))
            .unwrap_or(f64::NAN);
        let bf = b
            .as_f64()
            .or_else(|| b.as_i32().map(|i| i as f64))
            .unwrap_or(f64::NAN);

        let result = match op.as_str() {
            ">" => af > bf,
            ">=" => af >= bf,
            "<" => af < bf,
            "<=" => af <= bf,
            _ => false,
        };
        NativeCallResult::bool(result)
    });
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert a NativeValue to a human-readable string for assertion messages.
fn stringify_value(ctx: &dyn NativeContext, val: NativeValue) -> String {
    if val.is_null() {
        return "null".to_string();
    }
    if let Some(b) = val.as_bool() {
        return if b { "true".to_string() } else { "false".to_string() };
    }
    if let Some(i) = val.as_i32() {
        return i.to_string();
    }
    if let Some(f) = val.as_f64() {
        return format!("{}", f);
    }
    if let Ok(s) = ctx.read_string(val) {
        return format!("\"{}\"", s);
    }
    if let Ok(len) = ctx.array_len(val) {
        let mut parts = Vec::new();
        for i in 0..len.min(10) {
            if let Ok(elem) = ctx.array_get(val, i) {
                parts.push(stringify_value(ctx, elem));
            }
        }
        if len > 10 {
            parts.push(format!("...({} more)", len - 10));
        }
        return format!("[{}]", parts.join(", "));
    }
    "[object]".to_string()
}

/// Deep equality check for two NativeValues.
fn deep_equal(ctx: &dyn NativeContext, a: NativeValue, b: NativeValue) -> bool {
    // Both null
    if a.is_null() && b.is_null() {
        return true;
    }
    if a.is_null() || b.is_null() {
        return false;
    }

    // Booleans
    if let (Some(ab), Some(bb)) = (a.as_bool(), b.as_bool()) {
        return ab == bb;
    }

    // Numeric: coerce both to f64 for comparison (handles int/float mix)
    let a_num = a.as_f64().or_else(|| a.as_i32().map(|i| i as f64));
    let b_num = b.as_f64().or_else(|| b.as_i32().map(|i| i as f64));
    if let (Some(af), Some(bf)) = (a_num, b_num) {
        return (af - bf).abs() < f64::EPSILON;
    }

    // Strings
    if let (Ok(sa), Ok(sb)) = (ctx.read_string(a), ctx.read_string(b)) {
        return sa == sb;
    }

    // Arrays
    if let (Ok(la), Ok(lb)) = (ctx.array_len(a), ctx.array_len(b)) {
        if la != lb {
            return false;
        }
        for i in 0..la {
            let ea = ctx.array_get(a, i).unwrap_or_else(|_| NativeValue::null());
            let eb = ctx.array_get(b, i).unwrap_or_else(|_| NativeValue::null());
            if !deep_equal(ctx, ea, eb) {
                return false;
            }
        }
        return true;
    }

    // Fallback: bit-level equality (same reference)
    a == b
}

/// Check if a value is truthy.
fn is_truthy(ctx: &dyn NativeContext, val: NativeValue) -> bool {
    if val.is_null() {
        return false;
    }
    if let Some(b) = val.as_bool() {
        return b;
    }
    if let Some(i) = val.as_i32() {
        return i != 0;
    }
    if let Some(f) = val.as_f64() {
        return f != 0.0 && !f.is_nan();
    }
    if let Ok(s) = ctx.read_string(val) {
        return !s.is_empty();
    }
    // Objects, arrays, functions are truthy
    true
}

/// Check if haystack contains needle (arrays or strings).
fn contains_check(ctx: &dyn NativeContext, haystack: NativeValue, needle: NativeValue) -> bool {
    // String contains
    if let (Ok(hs), Ok(ns)) = (ctx.read_string(haystack), ctx.read_string(needle)) {
        return hs.contains(&ns);
    }

    // Array contains
    if let Ok(len) = ctx.array_len(haystack) {
        for i in 0..len {
            if let Ok(elem) = ctx.array_get(haystack, i) {
                if deep_equal(ctx, elem, needle) {
                    return true;
                }
            }
        }
        return false;
    }

    false
}
