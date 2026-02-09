//! Raya Runtime
//!
//! Binds the Raya engine with the standard library implementation.
//! Provides the default native call handler that routes stdlib calls
//! to their Rust implementations.

use raya_engine::vm::{NativeCallResult, NativeHandler};

/// Standard library native call handler
///
/// Routes native calls in the stdlib ID range to the corresponding
/// Rust implementations in `raya_stdlib`.
pub struct StdNativeHandler;

impl StdNativeHandler {
    /// Parse an f64 from string args at the given index, defaulting to 0.0
    fn parse_f64(args: &[String], index: usize) -> f64 {
        args.get(index)
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0)
    }
}

impl NativeHandler for StdNativeHandler {
    fn call(&self, id: u16, args: &[String]) -> NativeCallResult {
        match id {
            // Logger methods (0x1000-0x1003)
            0x1000 => {
                // LOGGER_DEBUG
                let msg = args.join(" ");
                raya_stdlib::logger::debug(&msg);
                NativeCallResult::Void
            }
            0x1001 => {
                // LOGGER_INFO
                let msg = args.join(" ");
                raya_stdlib::logger::info(&msg);
                NativeCallResult::Void
            }
            0x1002 => {
                // LOGGER_WARN
                let msg = args.join(" ");
                raya_stdlib::logger::warn(&msg);
                NativeCallResult::Void
            }
            0x1003 => {
                // LOGGER_ERROR
                let msg = args.join(" ");
                raya_stdlib::logger::error(&msg);
                NativeCallResult::Void
            }

            // Math methods (0x2000-0x2016)
            0x2000 => {
                let x = Self::parse_f64(args, 0);
                NativeCallResult::Number(raya_stdlib::math::abs(x))
            }
            0x2001 => {
                let x = Self::parse_f64(args, 0);
                NativeCallResult::Number(raya_stdlib::math::sign(x))
            }
            0x2002 => {
                let x = Self::parse_f64(args, 0);
                NativeCallResult::Number(raya_stdlib::math::floor(x))
            }
            0x2003 => {
                let x = Self::parse_f64(args, 0);
                NativeCallResult::Number(raya_stdlib::math::ceil(x))
            }
            0x2004 => {
                let x = Self::parse_f64(args, 0);
                NativeCallResult::Number(raya_stdlib::math::round(x))
            }
            0x2005 => {
                let x = Self::parse_f64(args, 0);
                NativeCallResult::Number(raya_stdlib::math::trunc(x))
            }
            0x2006 => {
                let a = Self::parse_f64(args, 0);
                let b = Self::parse_f64(args, 1);
                NativeCallResult::Number(raya_stdlib::math::min(a, b))
            }
            0x2007 => {
                let a = Self::parse_f64(args, 0);
                let b = Self::parse_f64(args, 1);
                NativeCallResult::Number(raya_stdlib::math::max(a, b))
            }
            0x2008 => {
                let base = Self::parse_f64(args, 0);
                let exp = Self::parse_f64(args, 1);
                NativeCallResult::Number(raya_stdlib::math::pow(base, exp))
            }
            0x2009 => {
                let x = Self::parse_f64(args, 0);
                NativeCallResult::Number(raya_stdlib::math::sqrt(x))
            }
            0x200A => {
                let x = Self::parse_f64(args, 0);
                NativeCallResult::Number(raya_stdlib::math::sin(x))
            }
            0x200B => {
                let x = Self::parse_f64(args, 0);
                NativeCallResult::Number(raya_stdlib::math::cos(x))
            }
            0x200C => {
                let x = Self::parse_f64(args, 0);
                NativeCallResult::Number(raya_stdlib::math::tan(x))
            }
            0x200D => {
                let x = Self::parse_f64(args, 0);
                NativeCallResult::Number(raya_stdlib::math::asin(x))
            }
            0x200E => {
                let x = Self::parse_f64(args, 0);
                NativeCallResult::Number(raya_stdlib::math::acos(x))
            }
            0x200F => {
                let x = Self::parse_f64(args, 0);
                NativeCallResult::Number(raya_stdlib::math::atan(x))
            }
            0x2010 => {
                let y = Self::parse_f64(args, 0);
                let x = Self::parse_f64(args, 1);
                NativeCallResult::Number(raya_stdlib::math::atan2(y, x))
            }
            0x2011 => {
                let x = Self::parse_f64(args, 0);
                NativeCallResult::Number(raya_stdlib::math::exp(x))
            }
            0x2012 => {
                let x = Self::parse_f64(args, 0);
                NativeCallResult::Number(raya_stdlib::math::log(x))
            }
            0x2013 => {
                let x = Self::parse_f64(args, 0);
                NativeCallResult::Number(raya_stdlib::math::log10(x))
            }
            0x2014 => {
                NativeCallResult::Number(raya_stdlib::math::random())
            }
            0x2015 => {
                NativeCallResult::Number(raya_stdlib::math::pi())
            }
            0x2016 => {
                NativeCallResult::Number(raya_stdlib::math::e())
            }

            _ => NativeCallResult::Unhandled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logger_info() {
        let handler = StdNativeHandler;
        let result = handler.call(0x1001, &["hello".to_string(), "world".to_string()]);
        assert!(matches!(result, NativeCallResult::Void));
    }

    #[test]
    fn test_math_abs() {
        let handler = StdNativeHandler;
        let result = handler.call(0x2000, &["-5".to_string()]);
        match result {
            NativeCallResult::Number(n) => assert_eq!(n, 5.0),
            _ => panic!("Expected Number result"),
        }
    }

    #[test]
    fn test_math_pi() {
        let handler = StdNativeHandler;
        let result = handler.call(0x2015, &[]);
        match result {
            NativeCallResult::Number(n) => assert!((n - std::f64::consts::PI).abs() < 1e-15),
            _ => panic!("Expected Number result"),
        }
    }

    #[test]
    fn test_unhandled() {
        let handler = StdNativeHandler;
        let result = handler.call(0xFFFF, &[]);
        assert!(matches!(result, NativeCallResult::Unhandled));
    }
}
