//! End-to-end tests for the std:time module
//!
//! Tests verify that time methods compile and execute correctly,
//! including wall clock, monotonic clock, sleep, and duration conversion.

use super::harness::{
    compile_and_run_with_builtins, expect_f64_with_builtins, expect_i32_with_builtins,
};

// ============================================================================
// Import
// ============================================================================

#[test]
fn test_time_import() {
    let result = compile_and_run_with_builtins(
        r#"
        import time from "std:time";
        let ts: number = time.now();
        return 1;
    "#,
    );
    assert!(
        result.is_ok(),
        "Time should be importable from std:time: {:?}",
        result.err()
    );
}

// ============================================================================
// Wall Clock — time.now()
// ============================================================================

#[test]
fn test_time_now_returns_positive() {
    // now() should return a large positive number (ms since epoch)
    let result = compile_and_run_with_builtins(
        r#"
        import time from "std:time";
        let ts: number = time.now();
        if (ts > 0) {
            return 1;
        }
        return 0;
    "#,
    );
    assert!(result.is_ok(), "time.now() should work: {:?}", result.err());
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

#[test]
fn test_time_now_is_recent() {
    // now() should be after Jan 1 2024 (1704067200000 ms)
    let result = compile_and_run_with_builtins(
        r#"
        import time from "std:time";
        let ts: number = time.now();
        if (ts > 1704067200000) {
            return 1;
        }
        return 0;
    "#,
    );
    assert!(result.is_ok(), "time.now() should work: {:?}", result.err());
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

// ============================================================================
// Monotonic Clock — time.monotonic()
// ============================================================================

#[test]
fn test_time_monotonic_non_negative() {
    let result = compile_and_run_with_builtins(
        r#"
        import time from "std:time";
        let ms: number = time.monotonic();
        if (ms >= 0) {
            return 1;
        }
        return 0;
    "#,
    );
    assert!(
        result.is_ok(),
        "time.monotonic() should work: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

#[test]
fn test_time_monotonic_increases() {
    // Two monotonic() calls should return non-decreasing values
    let result = compile_and_run_with_builtins(
        r#"
        import time from "std:time";
        let a: number = time.monotonic();
        let b: number = time.monotonic();
        if (b >= a) {
            return 1;
        }
        return 0;
    "#,
    );
    assert!(result.is_ok(), "monotonic should increase: {:?}", result.err());
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

// ============================================================================
// High-Resolution — time.hrtime()
// ============================================================================

#[test]
fn test_time_hrtime_non_negative() {
    let result = compile_and_run_with_builtins(
        r#"
        import time from "std:time";
        let ns: number = time.hrtime();
        if (ns >= 0) {
            return 1;
        }
        return 0;
    "#,
    );
    assert!(
        result.is_ok(),
        "time.hrtime() should work: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

// ============================================================================
// Elapsed — time.elapsed(start)
// ============================================================================

#[test]
fn test_time_elapsed_non_negative() {
    let result = compile_and_run_with_builtins(
        r#"
        import time from "std:time";
        let start: number = time.monotonic();
        let el: number = time.elapsed(start);
        if (el >= 0) {
            return 1;
        }
        return 0;
    "#,
    );
    assert!(
        result.is_ok(),
        "time.elapsed() should work: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

// ============================================================================
// Sleep — time.sleep(ms)
// ============================================================================

#[test]
fn test_time_sleep_basic() {
    // sleep(10) should complete without error
    let result = compile_and_run_with_builtins(
        r#"
        import time from "std:time";
        time.sleep(10);
        return 1;
    "#,
    );
    assert!(result.is_ok(), "time.sleep() should work: {:?}", result.err());
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

#[test]
fn test_time_sleep_measurable() {
    // Verify that monotonic time advances after sleep.
    // Use Rust-side timing to avoid VM preemption interference.
    let start = std::time::Instant::now();
    let result = compile_and_run_with_builtins(
        r#"
        import time from "std:time";
        time.sleep(30);
        return 1;
    "#,
    );
    let elapsed = start.elapsed().as_millis();
    assert!(result.is_ok(), "sleep should work: {:?}", result.err());
    assert_eq!(result.unwrap().as_i32(), Some(1));
    assert!(
        elapsed >= 25,
        "Expected at least 25ms elapsed, got {}ms",
        elapsed
    );
}

// ============================================================================
// Sleep — time.sleepMicros(us)
// ============================================================================

#[test]
fn test_time_sleep_micros_basic() {
    let result = compile_and_run_with_builtins(
        r#"
        import time from "std:time";
        time.sleepMicros(100);
        return 1;
    "#,
    );
    assert!(
        result.is_ok(),
        "time.sleepMicros() should work: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

// ============================================================================
// Duration Conversion — pure Raya methods
// ============================================================================

#[test]
fn test_time_seconds() {
    expect_i32_with_builtins(
        r#"
        import time from "std:time";
        return time.seconds(3);
    "#,
        3000,
    );
}

#[test]
fn test_time_minutes() {
    expect_i32_with_builtins(
        r#"
        import time from "std:time";
        return time.minutes(2);
    "#,
        120000,
    );
}

#[test]
fn test_time_hours() {
    expect_i32_with_builtins(
        r#"
        import time from "std:time";
        return time.hours(1);
    "#,
        3600000,
    );
}

#[test]
fn test_time_to_seconds() {
    expect_i32_with_builtins(
        r#"
        import time from "std:time";
        return time.toSeconds(5000);
    "#,
        5,
    );
}

#[test]
fn test_time_to_minutes() {
    expect_i32_with_builtins(
        r#"
        import time from "std:time";
        return time.toMinutes(120000);
    "#,
        2,
    );
}

#[test]
fn test_time_to_hours() {
    expect_i32_with_builtins(
        r#"
        import time from "std:time";
        return time.toHours(7200000);
    "#,
        2,
    );
}

#[test]
fn test_time_roundtrip() {
    expect_i32_with_builtins(
        r#"
        import time from "std:time";
        return time.toSeconds(time.seconds(42));
    "#,
        42,
    );
}

// ============================================================================
// Combined operations
// ============================================================================

#[test]
fn test_time_sleep_with_duration() {
    // Use duration helpers with sleep
    let result = compile_and_run_with_builtins(
        r#"
        import time from "std:time";
        let start: number = time.monotonic();
        time.sleep(10);
        let el: number = time.elapsed(start);
        let secs: number = time.toSeconds(el);
        if (secs < 1) {
            return 1;
        }
        return 0;
    "#,
    );
    assert!(result.is_ok(), "combined time ops should work: {:?}", result.err());
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

#[test]
fn test_time_zero_sleep() {
    // sleep(0) should be a no-op
    let result = compile_and_run_with_builtins(
        r#"
        import time from "std:time";
        time.sleep(0);
        return 1;
    "#,
    );
    assert!(result.is_ok(), "sleep(0) should work: {:?}", result.err());
    assert_eq!(result.unwrap().as_i32(), Some(1));
}
