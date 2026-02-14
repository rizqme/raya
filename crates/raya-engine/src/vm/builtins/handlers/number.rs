//! Number method handlers
//!
//! Implements built-in methods for the number type:
//! - toFixed(digits): Format with fixed decimal places
//! - toPrecision(precision): Format with significant digits
//! - toString(radix): Convert to string with optional radix

use crate::vm::gc::GarbageCollector;
use crate::vm::object::RayaString;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use parking_lot::Mutex;

/// Context for number method handlers
pub struct NumberHandlerContext<'a> {
    pub gc: &'a Mutex<GarbageCollector>,
    pub stack: &'a mut Stack,
}

/// Call a number method
pub fn call_number_method(
    ctx: &mut NumberHandlerContext,
    method_id: u16,
    receiver: Value,
    args: &[Value],
) -> Result<(), VmError> {
    // Extract number value from receiver
    let value = receiver
        .as_f64()
        .or_else(|| receiver.as_i32().map(|v| v as f64))
        .unwrap_or(0.0);

    let result_str = match method_id {
        0x0F00 => to_fixed(value, args),
        0x0F01 => to_precision(value, args),
        0x0F02 => to_string(value, args),
        _ => {
            return Err(VmError::RuntimeError(format!(
                "Number method {:#06x} not implemented",
                method_id
            )));
        }
    };

    // Allocate string and push to stack
    let s = RayaString::new(result_str);
    let gc_ptr = ctx.gc.lock().allocate(s);
    let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
    ctx.stack.push(val)?;
    Ok(())
}

// ============================================================================
// Method Implementations
// ============================================================================

/// number.toFixed(digits): Format with fixed decimal places
fn to_fixed(value: f64, args: &[Value]) -> String {
    let digits = args
        .get(0)
        .and_then(|v| v.as_i32())
        .unwrap_or(0)
        .max(0) as usize;
    format!("{:.prec$}", value, prec = digits)
}

/// number.toPrecision(precision): Format with significant digits
fn to_precision(value: f64, args: &[Value]) -> String {
    let prec = args
        .get(0)
        .and_then(|v| v.as_i32())
        .unwrap_or(1)
        .max(1) as usize;

    if value == 0.0 {
        return format!("{:.prec$}", 0.0, prec = prec - 1);
    }

    let magnitude = value.abs().log10().floor() as i32;
    if prec as i32 <= magnitude + 1 {
        // Need to round to integer with fewer digits
        let shift = 10f64.powi(magnitude + 1 - prec as i32);
        let rounded = (value / shift).round() * shift;
        format!("{}", rounded as i64)
    } else {
        // Show decimal places
        let decimal_places = (prec as i32 - magnitude - 1) as usize;
        format!("{:.prec$}", value, prec = decimal_places)
    }
}

/// number.toString(radix?): Convert to string with optional radix (2-36)
fn to_string(value: f64, args: &[Value]) -> String {
    let radix = args.get(0).and_then(|v| v.as_i32()).unwrap_or(10);

    // Default to decimal if radix is invalid or 10
    if radix == 10 || radix < 2 || radix > 36 {
        return if value.fract() == 0.0 && value.abs() < i64::MAX as f64 {
            format!("{}", value as i64)
        } else {
            format!("{}", value)
        };
    }

    // Convert integer part to specified radix
    let int_val = value as i64;
    match radix {
        2 => format!("{:b}", int_val),
        8 => format!("{:o}", int_val),
        16 => format!("{:x}", int_val),
        _ => {
            if int_val == 0 {
                return "0".to_string();
            }

            let negative = int_val < 0;
            let mut n = int_val.unsigned_abs();
            let mut digits = Vec::new();
            let r = radix as u64;

            while n > 0 {
                let d = (n % r) as u8;
                digits.push(if d < 10 { b'0' + d } else { b'a' + d - 10 });
                n /= r;
            }

            digits.reverse();
            let s = String::from_utf8(digits).unwrap_or_default();
            if negative {
                format!("-{}", s)
            } else {
                s
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_fixed() {
        assert_eq!(to_fixed(3.14159, &[Value::i32(2)]), "3.14");
        assert_eq!(to_fixed(10.0, &[Value::i32(0)]), "10");
        assert_eq!(to_fixed(1.5, &[Value::i32(3)]), "1.500");
    }

    #[test]
    fn test_to_precision() {
        assert_eq!(to_precision(123.456, &[Value::i32(5)]), "123.46");
        assert_eq!(to_precision(0.0012345, &[Value::i32(3)]), "0.00123");
    }

    #[test]
    fn test_to_string() {
        assert_eq!(to_string(42.0, &[Value::i32(10)]), "42");
        assert_eq!(to_string(255.0, &[Value::i32(16)]), "ff");
        assert_eq!(to_string(8.0, &[Value::i32(2)]), "1000");
    }
}
