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

impl NativeHandler for StdNativeHandler {
    fn call(&self, id: u16, args: &[String]) -> NativeCallResult {
        // Logger methods (0x1000-0x1003)
        match id {
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
    fn test_unhandled() {
        let handler = StdNativeHandler;
        let result = handler.call(0xFFFF, &[]);
        assert!(matches!(result, NativeCallResult::Unhandled));
    }
}
