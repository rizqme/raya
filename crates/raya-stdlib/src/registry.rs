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
    register_compress(registry);
    register_path(registry);
    register_time(registry);
    register_stream(registry);
    register_url(registry);
    register_encoding(registry);
    register_template(registry);
    register_semver(registry);
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

    registry.register("logger.setLevel", |ctx, args| {
        let level = ctx.read_string(args[0]).unwrap_or_default();
        crate::logger::set_level(&level);
        NativeCallResult::null()
    });

    registry.register("logger.getLevel", |ctx, _args| {
        NativeCallResult::Value(ctx.create_string(crate::logger::get_level()))
    });

    registry.register("logger.debugData", |ctx, args| {
        let msg = ctx.read_string(args[0]).unwrap_or_default();
        let data = ctx.read_string(args[1]).unwrap_or_default();
        crate::logger::debug_data(&msg, &data);
        NativeCallResult::null()
    });

    registry.register("logger.infoData", |ctx, args| {
        let msg = ctx.read_string(args[0]).unwrap_or_default();
        let data = ctx.read_string(args[1]).unwrap_or_default();
        crate::logger::info_data(&msg, &data);
        NativeCallResult::null()
    });

    registry.register("logger.warnData", |ctx, args| {
        let msg = ctx.read_string(args[0]).unwrap_or_default();
        let data = ctx.read_string(args[1]).unwrap_or_default();
        crate::logger::warn_data(&msg, &data);
        NativeCallResult::null()
    });

    registry.register("logger.errorData", |ctx, args| {
        let msg = ctx.read_string(args[0]).unwrap_or_default();
        let data = ctx.read_string(args[1]).unwrap_or_default();
        crate::logger::error_data(&msg, &data);
        NativeCallResult::null()
    });

    registry.register("logger.setFormat", |ctx, args| {
        let fmt = ctx.read_string(args[0]).unwrap_or_default();
        crate::logger::set_format(&fmt);
        NativeCallResult::null()
    });

    registry.register("logger.setTimestamp", |_ctx, args| {
        let enabled = args.first().and_then(|v| v.as_bool()).unwrap_or(false);
        crate::logger::set_timestamp(enabled);
        NativeCallResult::null()
    });

    registry.register("logger.setPrefix", |ctx, args| {
        let prefix = ctx.read_string(args[0]).unwrap_or_default();
        crate::logger::set_prefix(&prefix);
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
    registry.register("crypto.encrypt", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x400C, args));
    registry.register("crypto.decrypt", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x400D, args));
    registry.register("crypto.generateKey", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x400E, args));
    registry.register("crypto.sign", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x400F, args));
    registry.register("crypto.verify", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x4010, args));
    registry.register("crypto.generateKeyPair", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x4011, args));
    registry.register("crypto.hkdf", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x4012, args));
    registry.register("crypto.pbkdf2", |ctx, args| crate::crypto::call_crypto_method(ctx, 0x4013, args));
}

/// Register compress native functions (delegate to existing call_compress_method)
fn register_compress(registry: &mut NativeFunctionRegistry) {
    registry.register("compress.gzip", |ctx, args| crate::compress::call_compress_method(ctx, 0x8000, args));
    registry.register("compress.gunzip", |ctx, args| crate::compress::call_compress_method(ctx, 0x8001, args));
    registry.register("compress.deflate", |ctx, args| crate::compress::call_compress_method(ctx, 0x8002, args));
    registry.register("compress.inflate", |ctx, args| crate::compress::call_compress_method(ctx, 0x8003, args));
    registry.register("compress.zlibCompress", |ctx, args| crate::compress::call_compress_method(ctx, 0x8004, args));
    registry.register("compress.zlibDecompress", |ctx, args| crate::compress::call_compress_method(ctx, 0x8005, args));
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
        if ms == 0 {
            return NativeCallResult::null();
        }
        NativeCallResult::Suspend(raya_sdk::IoRequest::BlockingWork {
            work: Box::new(move || {
                std::thread::sleep(std::time::Duration::from_millis(ms));
                raya_sdk::IoCompletion::Primitive(raya_sdk::NativeValue::null())
            }),
        })
    });

    registry.register("time.sleepMicros", |_ctx, args| {
        let us = args.first()
            .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
            .unwrap_or(0.0) as u64;
        if us == 0 {
            return NativeCallResult::null();
        }
        NativeCallResult::Suspend(raya_sdk::IoRequest::BlockingWork {
            work: Box::new(move || {
                std::thread::sleep(std::time::Duration::from_micros(us));
                raya_sdk::IoCompletion::Primitive(raya_sdk::NativeValue::null())
            }),
        })
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

/// Register URL native functions (delegate to existing call_url_method)
fn register_url(registry: &mut NativeFunctionRegistry) {
    // URL parsing
    registry.register("url.parse", |ctx, args| crate::url::call_url_method(ctx, 0x9000, args));
    registry.register("url.parseWithBase", |ctx, args| crate::url::call_url_method(ctx, 0x9001, args));

    // URL components
    registry.register("url.protocol", |ctx, args| crate::url::call_url_method(ctx, 0x9010, args));
    registry.register("url.hostname", |ctx, args| crate::url::call_url_method(ctx, 0x9011, args));
    registry.register("url.port", |ctx, args| crate::url::call_url_method(ctx, 0x9012, args));
    registry.register("url.host", |ctx, args| crate::url::call_url_method(ctx, 0x9013, args));
    registry.register("url.pathname", |ctx, args| crate::url::call_url_method(ctx, 0x9014, args));
    registry.register("url.search", |ctx, args| crate::url::call_url_method(ctx, 0x9015, args));
    registry.register("url.hash", |ctx, args| crate::url::call_url_method(ctx, 0x9016, args));
    registry.register("url.origin", |ctx, args| crate::url::call_url_method(ctx, 0x9017, args));
    registry.register("url.href", |ctx, args| crate::url::call_url_method(ctx, 0x9018, args));
    registry.register("url.username", |ctx, args| crate::url::call_url_method(ctx, 0x9019, args));
    registry.register("url.password", |ctx, args| crate::url::call_url_method(ctx, 0x901A, args));
    registry.register("url.searchParams", |ctx, args| crate::url::call_url_method(ctx, 0x901B, args));
    registry.register("url.toString", |ctx, args| crate::url::call_url_method(ctx, 0x901C, args));

    // Mutators
    registry.register("url.withProtocol", |ctx, args| crate::url::call_url_method(ctx, 0x901D, args));
    registry.register("url.withHostname", |ctx, args| crate::url::call_url_method(ctx, 0x901E, args));
    registry.register("url.withPort", |ctx, args| crate::url::call_url_method(ctx, 0x901F, args));
    registry.register("url.withPathname", |ctx, args| crate::url::call_url_method(ctx, 0x9022, args));
    registry.register("url.withSearch", |ctx, args| crate::url::call_url_method(ctx, 0x9023, args));
    registry.register("url.withHash", |ctx, args| crate::url::call_url_method(ctx, 0x9024, args));

    // Encoding
    registry.register("url.encode", |ctx, args| crate::url::call_url_method(ctx, 0x9020, args));
    registry.register("url.decode", |ctx, args| crate::url::call_url_method(ctx, 0x9021, args));
    registry.register("url.encodePath", |ctx, args| crate::url::call_url_method(ctx, 0x9025, args));
    registry.register("url.decodePath", |ctx, args| crate::url::call_url_method(ctx, 0x9026, args));

    // URLSearchParams
    registry.register("url.paramsNew", |ctx, args| crate::url::call_url_method(ctx, 0x9030, args));
    registry.register("url.paramsFromString", |ctx, args| crate::url::call_url_method(ctx, 0x9031, args));
    registry.register("url.paramsGet", |ctx, args| crate::url::call_url_method(ctx, 0x9032, args));
    registry.register("url.paramsGetAll", |ctx, args| crate::url::call_url_method(ctx, 0x9033, args));
    registry.register("url.paramsHas", |ctx, args| crate::url::call_url_method(ctx, 0x9034, args));
    registry.register("url.paramsSet", |ctx, args| crate::url::call_url_method(ctx, 0x9035, args));
    registry.register("url.paramsAppend", |ctx, args| crate::url::call_url_method(ctx, 0x9036, args));
    registry.register("url.paramsDelete", |ctx, args| crate::url::call_url_method(ctx, 0x9037, args));
    registry.register("url.paramsKeys", |ctx, args| crate::url::call_url_method(ctx, 0x9038, args));
    registry.register("url.paramsValues", |ctx, args| crate::url::call_url_method(ctx, 0x9039, args));
    registry.register("url.paramsEntries", |ctx, args| crate::url::call_url_method(ctx, 0x903A, args));
    registry.register("url.paramsSort", |ctx, args| crate::url::call_url_method(ctx, 0x903B, args));
    registry.register("url.paramsToString", |ctx, args| crate::url::call_url_method(ctx, 0x903C, args));
    registry.register("url.paramsSize", |ctx, args| crate::url::call_url_method(ctx, 0x903D, args));
}

/// Register encoding native functions (delegate to existing call_encoding_method)
fn register_encoding(registry: &mut NativeFunctionRegistry) {
    // CSV operations
    registry.register("encoding.csvParse", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA000, args));
    registry.register("encoding.csvParseHeaders", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA001, args));
    registry.register("encoding.csvStringify", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA002, args));
    registry.register("encoding.csvStringifyHeaders", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA003, args));
    registry.register("encoding.csvTableHeaders", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA004, args));
    registry.register("encoding.csvTableRows", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA005, args));
    registry.register("encoding.csvTableRow", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA006, args));
    registry.register("encoding.csvTableColumn", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA007, args));
    registry.register("encoding.csvTableRowCount", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA008, args));
    registry.register("encoding.csvTableRelease", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA009, args));

    // XML operations
    registry.register("encoding.xmlParse", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA010, args));
    registry.register("encoding.xmlStringify", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA011, args));
    registry.register("encoding.xmlTag", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA012, args));
    registry.register("encoding.xmlText", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA013, args));
    registry.register("encoding.xmlAttr", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA014, args));
    registry.register("encoding.xmlAttrs", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA015, args));
    registry.register("encoding.xmlChildren", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA016, args));
    registry.register("encoding.xmlChild", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA017, args));
    registry.register("encoding.xmlChildrenByTag", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA018, args));
    registry.register("encoding.xmlRelease", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA019, args));

    // Base32 operations
    registry.register("encoding.base32Encode", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA020, args));
    registry.register("encoding.base32Decode", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA021, args));
    registry.register("encoding.base32HexEncode", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA022, args));
    registry.register("encoding.base32HexDecode", |ctx, args| crate::encoding::call_encoding_method(ctx, 0xA023, args));
}

/// Register template native functions (delegate to existing call_template_method)
fn register_template(registry: &mut NativeFunctionRegistry) {
    registry.register("template.compile", |ctx, args| crate::template::call_template_method(ctx, 0xB000, args));
    registry.register("template.render", |ctx, args| crate::template::call_template_method(ctx, 0xB001, args));
    registry.register("template.compiledRender", |ctx, args| crate::template::call_template_method(ctx, 0xB002, args));
    registry.register("template.compiledRelease", |ctx, args| crate::template::call_template_method(ctx, 0xB003, args));
}

/// Register semver native functions (delegate to existing call_semver_method)
fn register_semver(registry: &mut NativeFunctionRegistry) {
    // Parsing
    registry.register("semver.parse", |ctx, args| crate::semver_mod::call_semver_method(ctx, 0xC000, args));
    registry.register("semver.valid", |ctx, args| crate::semver_mod::call_semver_method(ctx, 0xC001, args));

    // Comparison (string-based)
    registry.register("semver.compare", |ctx, args| crate::semver_mod::call_semver_method(ctx, 0xC002, args));
    registry.register("semver.satisfies", |ctx, args| crate::semver_mod::call_semver_method(ctx, 0xC003, args));
    registry.register("semver.gt", |ctx, args| crate::semver_mod::call_semver_method(ctx, 0xC004, args));
    registry.register("semver.gte", |ctx, args| crate::semver_mod::call_semver_method(ctx, 0xC005, args));
    registry.register("semver.lt", |ctx, args| crate::semver_mod::call_semver_method(ctx, 0xC006, args));
    registry.register("semver.lte", |ctx, args| crate::semver_mod::call_semver_method(ctx, 0xC007, args));
    registry.register("semver.eq", |ctx, args| crate::semver_mod::call_semver_method(ctx, 0xC008, args));

    // Version component accessors (handle-based)
    registry.register("semver.versionMajor", |ctx, args| crate::semver_mod::call_semver_method(ctx, 0xC010, args));
    registry.register("semver.versionMinor", |ctx, args| crate::semver_mod::call_semver_method(ctx, 0xC011, args));
    registry.register("semver.versionPatch", |ctx, args| crate::semver_mod::call_semver_method(ctx, 0xC012, args));
    registry.register("semver.versionPrerelease", |ctx, args| crate::semver_mod::call_semver_method(ctx, 0xC013, args));
    registry.register("semver.versionBuild", |ctx, args| crate::semver_mod::call_semver_method(ctx, 0xC014, args));
    registry.register("semver.versionToString", |ctx, args| crate::semver_mod::call_semver_method(ctx, 0xC015, args));
    registry.register("semver.versionRelease", |ctx, args| crate::semver_mod::call_semver_method(ctx, 0xC016, args));
}
