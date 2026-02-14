//! Standard library native call handler
//!
//! Routes native call IDs in the stdlib range to the corresponding
//! Rust implementations.

use raya_engine::vm::{NativeCallResult, NativeContext, NativeHandler, NativeValue, string_read};

/// Standard library native call handler
///
/// Routes native calls in the stdlib ID range to the corresponding
/// Rust implementations.
pub struct StdNativeHandler;

impl StdNativeHandler {
    /// Extract f64 from a NativeValue, handling both i32 and f64
    fn get_f64(val: &NativeValue) -> f64 {
        if let Some(f) = val.as_f64() {
            f
        } else if let Some(i) = val.as_i32() {
            i as f64
        } else {
            0.0
        }
    }
}

impl NativeHandler for StdNativeHandler {
    fn call(&self, ctx: &NativeContext, id: u16, args: &[NativeValue]) -> NativeCallResult {
        // Delegate to specialized modules
        if (0x4000..=0x40FF).contains(&id) {
            // Crypto methods (0x4000-0x400B)
            return crate::crypto::call_crypto_method(ctx, id, args);
        }
        if (0x6000..=0x60FF).contains(&id) {
            // Path methods (0x6000-0x600C)
            return crate::path::call_path_method(ctx, id, args);
        }
        if (0x7000..=0x71FF).contains(&id) {
            // Codec methods (UTF-8: 0x7000-0x7003, Msgpack: 0x7010-0x7012, CBOR: 0x7020-0x7022, Proto: 0x7030-0x7031)
            return crate::codec::call_codec_method(ctx, id, args);
        }

        match id {
            // Logger methods (0x1000-0x1003)
            0x1000 => {
                // LOGGER_DEBUG
                let parts: Vec<String> = args.iter()
                    .filter_map(|v| string_read(*v).ok())
                    .collect();
                let msg = parts.join(" ");
                crate::logger::debug(&msg);
                NativeCallResult::null()
            }
            0x1001 => {
                // LOGGER_INFO
                let parts: Vec<String> = args.iter()
                    .filter_map(|v| string_read(*v).ok())
                    .collect();
                let msg = parts.join(" ");
                crate::logger::info(&msg);
                NativeCallResult::null()
            }
            0x1002 => {
                // LOGGER_WARN
                let parts: Vec<String> = args.iter()
                    .filter_map(|v| string_read(*v).ok())
                    .collect();
                let msg = parts.join(" ");
                crate::logger::warn(&msg);
                NativeCallResult::null()
            }
            0x1003 => {
                // LOGGER_ERROR
                let parts: Vec<String> = args.iter()
                    .filter_map(|v| string_read(*v).ok())
                    .collect();
                let msg = parts.join(" ");
                crate::logger::error(&msg);
                NativeCallResult::null()
            }

            // Math methods (0x2000-0x2016)
            0x2000 => {
                let x = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::abs(x))
            }
            0x2001 => {
                let x = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::sign(x))
            }
            0x2002 => {
                let x = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::floor(x))
            }
            0x2003 => {
                let x = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::ceil(x))
            }
            0x2004 => {
                let x = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::round(x))
            }
            0x2005 => {
                let x = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::trunc(x))
            }
            0x2006 => {
                let a = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                let b = args.get(1).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::min(a, b))
            }
            0x2007 => {
                let a = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                let b = args.get(1).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::max(a, b))
            }
            0x2008 => {
                let base = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                let exp = args.get(1).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::pow(base, exp))
            }
            0x2009 => {
                let x = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::sqrt(x))
            }
            0x200A => {
                let x = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::sin(x))
            }
            0x200B => {
                let x = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::cos(x))
            }
            0x200C => {
                let x = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::tan(x))
            }
            0x200D => {
                let x = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::asin(x))
            }
            0x200E => {
                let x = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::acos(x))
            }
            0x200F => {
                let x = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::atan(x))
            }
            0x2010 => {
                let y = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                let x = args.get(1).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::atan2(y, x))
            }
            0x2011 => {
                let x = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::exp(x))
            }
            0x2012 => {
                let x = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::log(x))
            }
            0x2013 => {
                let x = args.get(0).map(Self::get_f64).unwrap_or(0.0);
                NativeCallResult::f64(crate::math::log10(x))
            }
            0x2014 => {
                NativeCallResult::f64(crate::math::random())
            }
            0x2015 => {
                NativeCallResult::f64(crate::math::pi())
            }
            0x2016 => {
                NativeCallResult::f64(crate::math::e())
            }

            _ => NativeCallResult::Unhandled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raya_engine::vm::gc::GarbageCollector as Gc;
    use raya_engine::vm::scheduler::{Scheduler, TaskId};
    use raya_engine::vm::types::TypeRegistry;
    use raya_engine::vm::interpreter::{ClassRegistry, VmContextId};
    use parking_lot::{Mutex, RwLock};
    use std::sync::Arc;

    fn create_test_context() -> (Mutex<Gc>, RwLock<ClassRegistry>, Arc<Scheduler>) {
        let context_id = VmContextId::new();
        let type_registry = Arc::new(TypeRegistry::new());
        let gc = Mutex::new(Gc::new(context_id, type_registry));
        let classes = RwLock::new(ClassRegistry::new());
        let scheduler = Arc::new(Scheduler::new(1));
        (gc, classes, scheduler)
    }

    #[test]
    fn test_logger_info() {
        let (gc, classes, scheduler) = create_test_context();
        let ctx = NativeContext::new(&gc, &classes, &scheduler, TaskId::from_u64(1));

        // Allocate string arguments using the ABI
        let hello = raya_engine::vm::string_allocate(&ctx, "hello".to_string());
        let world = raya_engine::vm::string_allocate(&ctx, "world".to_string());

        let handler = StdNativeHandler;
        let result = handler.call(&ctx, 0x1001, &[hello, world]);
        assert!(matches!(result, NativeCallResult::Value(v) if v.is_null()));
    }

    #[test]
    fn test_math_abs() {
        let (gc, classes, scheduler) = create_test_context();
        let ctx = NativeContext::new(&gc, &classes, &scheduler, TaskId::from_u64(1));

        let handler = StdNativeHandler;
        let result = handler.call(&ctx, 0x2000, &[NativeValue::f64(-5.0)]);
        match result {
            NativeCallResult::Value(v) => {
                assert_eq!(v.as_f64().unwrap(), 5.0);
            }
            _ => panic!("Expected Value result"),
        }
    }

    #[test]
    fn test_math_pi() {
        let (gc, classes, scheduler) = create_test_context();
        let ctx = NativeContext::new(&gc, &classes, &scheduler, TaskId::from_u64(1));

        let handler = StdNativeHandler;
        let result = handler.call(&ctx, 0x2015, &[]);
        match result {
            NativeCallResult::Value(v) => {
                let n = v.as_f64().unwrap();
                assert!((n - std::f64::consts::PI).abs() < 1e-15);
            }
            _ => panic!("Expected Value result"),
        }
    }

    #[test]
    fn test_unhandled() {
        let (gc, classes, scheduler) = create_test_context();
        let ctx = NativeContext::new(&gc, &classes, &scheduler, TaskId::from_u64(1));

        let handler = StdNativeHandler;
        let result = handler.call(&ctx, 0xFFFF, &[]);
        assert!(matches!(result, NativeCallResult::Unhandled));
    }
}
