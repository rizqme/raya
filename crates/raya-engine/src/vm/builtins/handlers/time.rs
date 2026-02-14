//! Time method handlers (std:time)
//!
//! Native implementation of std:time module for wall clock, monotonic clock,
//! high-resolution timing, and sleep.

use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::vm::builtin::time;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;

/// Monotonic reference epoch — created once at first use
static MONOTONIC_EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

// ============================================================================
// Handler
// ============================================================================

/// Handle built-in time methods (std:time)
pub fn call_time_method(
    stack: &mut Stack,
    method_id: u16,
    arg_count: usize,
) -> Result<(), VmError> {
    // Pop arguments
    let mut args = Vec::with_capacity(arg_count);
    for _ in 0..arg_count {
        args.push(stack.pop()?);
    }
    args.reverse();

    let result = match method_id {
        time::NOW => {
            // time.now(): number — ms since Unix epoch
            let ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_millis() as f64;
            Value::f64(ms)
        }

        time::MONOTONIC => {
            // time.monotonic(): number — monotonic ms since process start
            let ms = Instant::now()
                .duration_since(*MONOTONIC_EPOCH)
                .as_millis() as f64;
            Value::f64(ms)
        }

        time::HRTIME => {
            // time.hrtime(): number — monotonic nanoseconds
            let ns = Instant::now()
                .duration_since(*MONOTONIC_EPOCH)
                .as_nanos() as f64;
            Value::f64(ns)
        }

        time::SLEEP => {
            // time.sleep(ms): void — synchronous sleep
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "time.sleep requires 1 argument".to_string(),
                ));
            }
            let ms = get_number(&args[0], "ms")?;
            if ms < 0.0 {
                return Err(VmError::RuntimeError(
                    "time.sleep: ms must be non-negative".to_string(),
                ));
            }
            std::thread::sleep(Duration::from_millis(ms as u64));
            Value::null()
        }

        time::SLEEP_MICROS => {
            // time.sleepMicros(us): void — microsecond sleep
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "time.sleepMicros requires 1 argument".to_string(),
                ));
            }
            let us = get_number(&args[0], "us")?;
            if us < 0.0 {
                return Err(VmError::RuntimeError(
                    "time.sleepMicros: us must be non-negative".to_string(),
                ));
            }
            std::thread::sleep(Duration::from_micros(us as u64));
            Value::null()
        }

        _ => {
            return Err(VmError::RuntimeError(format!(
                "Unknown time method: {:#06x}",
                method_id
            )));
        }
    };

    stack.push(result)?;
    Ok(())
}

/// Extract a number (f64 or i32) from a Value
fn get_number(v: &Value, name: &str) -> Result<f64, VmError> {
    if let Some(f) = v.as_f64() {
        Ok(f)
    } else if let Some(i) = v.as_i32() {
        Ok(i as f64)
    } else {
        Err(VmError::TypeError(format!(
            "Expected number for {}",
            name
        )))
    }
}
