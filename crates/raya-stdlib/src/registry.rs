//! Symbolic native function registry for stdlib
//!
//! Registers all stdlib native functions by symbolic name (e.g., "math.abs",
//! "logger.info") into a `NativeFunctionRegistry`. At module load time,
//! the VM resolves these names to handler functions for zero-cost dispatch.

use raya_sdk::{NativeCallResult, NativeFunctionRegistry, NativeValue};

/// Helper to extract f64 from a NativeValue, handling both i32 and f64
fn get_f64(val: &NativeValue) -> f64 {
    if let Some(f) = val.as_f64() {
        f
    } else if let Some(i) = val.as_i32() {
        i as f64
    } else {
        0.0
    }
}

/// Register all stdlib native functions into the given registry.
///
/// After calling this, the registry contains all symbolic names
/// (e.g., "math.abs", "logger.info", "crypto.hash") mapped to their handlers.
pub fn register_stdlib(registry: &mut NativeFunctionRegistry) {
    register_logger(registry);
    register_math(registry);
    register_crypto(registry);
    register_path(registry);
    register_time(registry);
    register_stream(registry);
}

/// Register logger native functions
fn register_logger(registry: &mut NativeFunctionRegistry) {
    registry.register("logger.debug", |ctx, args| {
        let parts: Vec<String> = args.iter()
            .filter_map(|v| ctx.read_string(*v).ok())
            .collect();
        crate::logger::debug(&parts.join(" "));
        NativeCallResult::null()
    });

    registry.register("logger.info", |ctx, args| {
        let parts: Vec<String> = args.iter()
            .filter_map(|v| ctx.read_string(*v).ok())
            .collect();
        crate::logger::info(&parts.join(" "));
        NativeCallResult::null()
    });

    registry.register("logger.warn", |ctx, args| {
        let parts: Vec<String> = args.iter()
            .filter_map(|v| ctx.read_string(*v).ok())
            .collect();
        crate::logger::warn(&parts.join(" "));
        NativeCallResult::null()
    });

    registry.register("logger.error", |ctx, args| {
        let parts: Vec<String> = args.iter()
            .filter_map(|v| ctx.read_string(*v).ok())
            .collect();
        crate::logger::error(&parts.join(" "));
        NativeCallResult::null()
    });
}

/// Register math native functions
fn register_math(registry: &mut NativeFunctionRegistry) {
    registry.register("math.abs", |_ctx, args| {
        NativeCallResult::f64(crate::math::abs(args.first().map(get_f64).unwrap_or(0.0)))
    });
    registry.register("math.sign", |_ctx, args| {
        NativeCallResult::f64(crate::math::sign(args.first().map(get_f64).unwrap_or(0.0)))
    });
    registry.register("math.floor", |_ctx, args| {
        NativeCallResult::f64(crate::math::floor(args.first().map(get_f64).unwrap_or(0.0)))
    });
    registry.register("math.ceil", |_ctx, args| {
        NativeCallResult::f64(crate::math::ceil(args.first().map(get_f64).unwrap_or(0.0)))
    });
    registry.register("math.round", |_ctx, args| {
        NativeCallResult::f64(crate::math::round(args.first().map(get_f64).unwrap_or(0.0)))
    });
    registry.register("math.trunc", |_ctx, args| {
        NativeCallResult::f64(crate::math::trunc(args.first().map(get_f64).unwrap_or(0.0)))
    });
    registry.register("math.min", |_ctx, args| {
        let a = args.first().map(get_f64).unwrap_or(0.0);
        let b = args.get(1).map(get_f64).unwrap_or(0.0);
        NativeCallResult::f64(crate::math::min(a, b))
    });
    registry.register("math.max", |_ctx, args| {
        let a = args.first().map(get_f64).unwrap_or(0.0);
        let b = args.get(1).map(get_f64).unwrap_or(0.0);
        NativeCallResult::f64(crate::math::max(a, b))
    });
    registry.register("math.pow", |_ctx, args| {
        let base = args.first().map(get_f64).unwrap_or(0.0);
        let exp = args.get(1).map(get_f64).unwrap_or(0.0);
        NativeCallResult::f64(crate::math::pow(base, exp))
    });
    registry.register("math.sqrt", |_ctx, args| {
        NativeCallResult::f64(crate::math::sqrt(args.first().map(get_f64).unwrap_or(0.0)))
    });
    registry.register("math.sin", |_ctx, args| {
        NativeCallResult::f64(crate::math::sin(args.first().map(get_f64).unwrap_or(0.0)))
    });
    registry.register("math.cos", |_ctx, args| {
        NativeCallResult::f64(crate::math::cos(args.first().map(get_f64).unwrap_or(0.0)))
    });
    registry.register("math.tan", |_ctx, args| {
        NativeCallResult::f64(crate::math::tan(args.first().map(get_f64).unwrap_or(0.0)))
    });
    registry.register("math.asin", |_ctx, args| {
        NativeCallResult::f64(crate::math::asin(args.first().map(get_f64).unwrap_or(0.0)))
    });
    registry.register("math.acos", |_ctx, args| {
        NativeCallResult::f64(crate::math::acos(args.first().map(get_f64).unwrap_or(0.0)))
    });
    registry.register("math.atan", |_ctx, args| {
        NativeCallResult::f64(crate::math::atan(args.first().map(get_f64).unwrap_or(0.0)))
    });
    registry.register("math.atan2", |_ctx, args| {
        let y = args.first().map(get_f64).unwrap_or(0.0);
        let x = args.get(1).map(get_f64).unwrap_or(0.0);
        NativeCallResult::f64(crate::math::atan2(y, x))
    });
    registry.register("math.exp", |_ctx, args| {
        NativeCallResult::f64(crate::math::exp(args.first().map(get_f64).unwrap_or(0.0)))
    });
    registry.register("math.log", |_ctx, args| {
        NativeCallResult::f64(crate::math::log(args.first().map(get_f64).unwrap_or(0.0)))
    });
    registry.register("math.log10", |_ctx, args| {
        NativeCallResult::f64(crate::math::log10(args.first().map(get_f64).unwrap_or(0.0)))
    });
    registry.register("math.random", |_ctx, _args| {
        NativeCallResult::f64(crate::math::random())
    });
    registry.register("math.PI", |_ctx, _args| {
        NativeCallResult::f64(crate::math::pi())
    });
    registry.register("math.E", |_ctx, _args| {
        NativeCallResult::f64(crate::math::e())
    });
}

/// Register crypto native functions (delegate to existing call_crypto_method)
fn register_crypto(registry: &mut NativeFunctionRegistry) {
    registry.register("crypto.hash", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x4000, args));
    registry.register("crypto.hashBytes", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x4001, args));
    registry.register("crypto.hmac", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x4002, args));
    registry.register("crypto.hmacBytes", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x4003, args));
    registry.register("crypto.randomBytes", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x4004, args));
    registry.register("crypto.randomInt", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x4005, args));
    registry.register("crypto.randomUUID", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x4006, args));
    registry.register("crypto.toHex", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x4007, args));
    registry.register("crypto.fromHex", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x4008, args));
    registry.register("crypto.toBase64", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x4009, args));
    registry.register("crypto.fromBase64", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x400A, args));
    registry.register("crypto.timingSafeEqual", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x400B, args));
}

/// Register path native functions (delegate to existing call_path_method)
fn register_path(registry: &mut NativeFunctionRegistry) {
    registry.register("path.join", |ctx, args| crate::path::call_path_method(ctx, 0x6000, args));
    registry.register("path.normalize", |ctx, args| crate::path::call_path_method(ctx, 0x6001, args));
    registry.register("path.dirname", |ctx, args| crate::path::call_path_method(ctx, 0x6002, args));
    registry.register("path.basename", |ctx, args| crate::path::call_path_method(ctx, 0x6003, args));
    registry.register("path.extname", |ctx, args| crate::path::call_path_method(ctx, 0x6004, args));
    registry.register("path.isAbsolute", |ctx, args| crate::path::call_path_method(ctx, 0x6005, args));
    registry.register("path.resolve", |ctx, args| crate::path::call_path_method(ctx, 0x6006, args));
    registry.register("path.relative", |ctx, args| crate::path::call_path_method(ctx, 0x6007, args));
    registry.register("path.cwd", |ctx, args| crate::path::call_path_method(ctx, 0x6008, args));
    registry.register("path.sep", |ctx, args| crate::path::call_path_method(ctx, 0x6009, args));
    registry.register("path.delimiter", |ctx, args| crate::path::call_path_method(ctx, 0x600A, args));
    registry.register("path.stripExt", |ctx, args| crate::path::call_path_method(ctx, 0x600B, args));
    registry.register("path.withExt", |ctx, args| crate::path::call_path_method(ctx, 0x600C, args));
}

/// Register time native functions
fn register_time(registry: &mut NativeFunctionRegistry) {
    use std::time::{Instant, SystemTime, UNIX_EPOCH};
    use std::sync::LazyLock;

    static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

    registry.register("time.now", |_ctx, _args| {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        NativeCallResult::f64(now.as_millis() as f64)
    });

    registry.register("time.monotonic", |_ctx, _args| {
        let elapsed = EPOCH.elapsed();
        NativeCallResult::f64(elapsed.as_millis() as f64)
    });

    registry.register("time.hrtime", |_ctx, _args| {
        let elapsed = EPOCH.elapsed();
        NativeCallResult::f64(elapsed.as_nanos() as f64)
    });

    registry.register("time.sleep", |_ctx, args| {
        let ms = args.first()
            .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
            .unwrap_or(0.0) as u64;
        if ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
        NativeCallResult::null()
    });

    registry.register("time.sleepMicros", |_ctx, args| {
        let us = args.first()
            .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
            .unwrap_or(0.0) as u64;
        if us > 0 {
            std::thread::sleep(std::time::Duration::from_micros(us));
        }
        NativeCallResult::null()
    });
}

/// Register stream native functions
fn register_stream(registry: &mut NativeFunctionRegistry) {
    registry.register("stream.forward", |ctx, args| crate::stream::forward(ctx, args));
    registry.register("stream.collect", |ctx, args| crate::stream::collect(ctx, args));
    registry.register("stream.count", |ctx, args| crate::stream::count(ctx, args));
    registry.register("stream.receive", |ctx, args| crate::stream::receive(ctx, args));
    registry.register("stream.send", |ctx, args| crate::stream::send(ctx, args));
}
