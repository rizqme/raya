//! Crypto module implementation (std:crypto)
//!
//! Native implementation using the SDK for hashing, HMAC,
//! secure random, encoding, and constant-time comparison.

use raya_sdk::{NativeCallResult, NativeContext, NativeValue};

use sha2::{Digest, Sha256, Sha384, Sha512};

// ============================================================================
// Public API
// ============================================================================

/// Handle crypto method calls
pub fn call_crypto_method(
    ctx: &dyn NativeContext,
    method_id: u16,
    args: &[NativeValue],
) -> NativeCallResult {
    match method_id {
        0x4000 => hash(ctx, args),           // HASH
        0x4001 => hash_bytes(ctx, args),     // HASH_BYTES
        0x4002 => hmac(ctx, args),           // HMAC
        0x4003 => hmac_bytes(ctx, args),     // HMAC_BYTES
        0x4004 => random_bytes(ctx, args),   // RANDOM_BYTES
        0x4005 => random_int(args),          // RANDOM_INT
        0x4006 => random_uuid(ctx, args),    // RANDOM_UUID
        0x4007 => to_hex(ctx, args),         // TO_HEX
        0x4008 => from_hex(ctx, args),       // FROM_HEX
        0x4009 => to_base64(ctx, args),      // TO_BASE64
        0x400A => from_base64(ctx, args),    // FROM_BASE64
        0x400B => timing_safe_equal(ctx, args),   // TIMING_SAFE_EQUAL
        _ => NativeCallResult::Unhandled,
    }
}

// ============================================================================
// Method Implementations
// ============================================================================

/// crypto.hash(algorithm, data): string — hex-encoded digest
fn hash(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("crypto.hash requires 2 arguments".to_string());
    }

    let algorithm = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid algorithm: {}", e)),
    };

    let data = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid data: {}", e)),
    };

    match hash_bytes_internal(&algorithm, data.as_bytes()) {
        Ok(digest) => {
            let hex = hex::encode(digest);
            NativeCallResult::Value(ctx.create_string(&hex))
        }
        Err(e) => NativeCallResult::Error(e),
    }
}

/// crypto.hashBytes(algorithm, data: Buffer): Buffer
fn hash_bytes(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("crypto.hashBytes requires 2 arguments".to_string());
    }

    let algorithm = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid algorithm: {}", e)),
    };

    let data = match ctx.read_buffer(args[1]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid buffer: {}", e)),
    };

    match hash_bytes_internal(&algorithm, &data) {
        Ok(digest) => NativeCallResult::Value(ctx.create_buffer(&digest)),
        Err(e) => NativeCallResult::Error(e),
    }
}

/// crypto.hmac(algorithm, key, data): string — hex-encoded MAC
fn hmac(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 3 {
        return NativeCallResult::Error("crypto.hmac requires 3 arguments".to_string());
    }

    let algorithm = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid algorithm: {}", e)),
    };

    let key = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid key: {}", e)),
    };

    let data = match ctx.read_string(args[2]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid data: {}", e)),
    };

    match hmac_compute_internal(&algorithm, key.as_bytes(), data.as_bytes()) {
        Ok(mac) => {
            let hex = hex::encode(mac);
            NativeCallResult::Value(ctx.create_string(&hex))
        }
        Err(e) => NativeCallResult::Error(e),
    }
}

/// crypto.hmacBytes(algorithm, key: Buffer, data: Buffer): Buffer
fn hmac_bytes(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 3 {
        return NativeCallResult::Error("crypto.hmacBytes requires 3 arguments".to_string());
    }

    let algorithm = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid algorithm: {}", e)),
    };

    let key = match ctx.read_buffer(args[1]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid key: {}", e)),
    };

    let data = match ctx.read_buffer(args[2]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid data: {}", e)),
    };

    match hmac_compute_internal(&algorithm, &key, &data) {
        Ok(mac) => NativeCallResult::Value(ctx.create_buffer(&mac)),
        Err(e) => NativeCallResult::Error(e),
    }
}

/// crypto.randomBytes(size): Buffer
fn random_bytes(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("crypto.randomBytes requires 1 argument".to_string());
    }

    let size = match args[0].as_i32() {
        Some(n) => n as usize,
        None => return NativeCallResult::Error("Expected number for size".to_string()),
    };

    if size > 65536 {
        return NativeCallResult::Error(
            "crypto.randomBytes: size too large (max 65536)".to_string(),
        );
    }

    let mut bytes = vec![0u8; size];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut bytes);

    NativeCallResult::Value(ctx.create_buffer(&bytes))
}

/// crypto.randomInt(min, max): number — random integer in [min, max)
fn random_int(args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("crypto.randomInt requires 2 arguments".to_string());
    }

    let min = match args[0].as_i32() {
        Some(n) => n,
        None => return NativeCallResult::Error("Expected number for min".to_string()),
    };

    let max = match args[1].as_i32() {
        Some(n) => n,
        None => return NativeCallResult::Error("Expected number for max".to_string()),
    };

    if min >= max {
        return NativeCallResult::Error("crypto.randomInt: min must be less than max".to_string());
    }

    use rand::Rng;
    let n = rand::thread_rng().gen_range(min..max);
    NativeCallResult::i32(n)
}

/// crypto.randomUUID(): string — UUID v4
fn random_uuid(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let id = uuid::Uuid::new_v4().to_string();
    NativeCallResult::Value(ctx.create_string(&id))
}

/// crypto.toHex(data: Buffer): string
fn to_hex(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("crypto.toHex requires 1 argument".to_string());
    }

    let data = match ctx.read_buffer(args[0]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid buffer: {}", e)),
    };

    let hex = hex::encode(data);
    NativeCallResult::Value(ctx.create_string(&hex))
}

/// crypto.fromHex(hex: string): Buffer
fn from_hex(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("crypto.fromHex requires 1 argument".to_string());
    }

    let hex_str = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid hex string: {}", e)),
    };

    match hex::decode(&hex_str) {
        Ok(bytes) => NativeCallResult::Value(ctx.create_buffer(&bytes)),
        Err(e) => NativeCallResult::Error(format!("crypto.fromHex: invalid hex: {}", e)),
    }
}

/// crypto.toBase64(data: Buffer): string
fn to_base64(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("crypto.toBase64 requires 1 argument".to_string());
    }

    let data = match ctx.read_buffer(args[0]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid buffer: {}", e)),
    };

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
    NativeCallResult::Value(ctx.create_string(&b64))
}

/// crypto.fromBase64(b64: string): Buffer
fn from_base64(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("crypto.fromBase64 requires 1 argument".to_string());
    }

    let b64_str = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid base64 string: {}", e)),
    };

    use base64::Engine;
    match base64::engine::general_purpose::STANDARD.decode(&b64_str) {
        Ok(bytes) => NativeCallResult::Value(ctx.create_buffer(&bytes)),
        Err(e) => NativeCallResult::Error(format!("crypto.fromBase64: invalid base64: {}", e)),
    }
}

/// crypto.timingSafeEqual(a: Buffer, b: Buffer): boolean
fn timing_safe_equal(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error(
            "crypto.timingSafeEqual requires 2 arguments".to_string(),
        );
    }

    let a = match ctx.read_buffer(args[0]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid buffer a: {}", e)),
    };

    let b = match ctx.read_buffer(args[1]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid buffer b: {}", e)),
    };

    if a.len() != b.len() {
        return NativeCallResult::bool(false);
    }

    use subtle::ConstantTimeEq;
    let equal: bool = a.ct_eq(&b).into();
    NativeCallResult::bool(equal)
}

// ============================================================================
// Internal Helpers
// ============================================================================

/// Hash data with the given algorithm, returning raw digest bytes
fn hash_bytes_internal(algorithm: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    match algorithm {
        "sha256" => Ok(Sha256::digest(data).to_vec()),
        "sha384" => Ok(Sha384::digest(data).to_vec()),
        "sha512" => Ok(Sha512::digest(data).to_vec()),
        "sha1" => {
            use sha1::Sha1;
            Ok(Sha1::digest(data).to_vec())
        }
        "md5" => {
            Ok(md5::compute(data).to_vec())
        }
        _ => Err(format!(
            "Unsupported hash algorithm: {}. Supported: sha256, sha384, sha512, sha1, md5",
            algorithm
        )),
    }
}

/// Compute HMAC with the given algorithm
fn hmac_compute_internal(algorithm: &str, key: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    use hmac::{Hmac, Mac};

    match algorithm {
        "sha256" => {
            type HmacSha256 = Hmac<Sha256>;
            let mut mac = HmacSha256::new_from_slice(key)
                .map_err(|e| format!("HMAC error: {}", e))?;
            mac.update(data);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        "sha384" => {
            type HmacSha384 = Hmac<Sha384>;
            let mut mac = HmacSha384::new_from_slice(key)
                .map_err(|e| format!("HMAC error: {}", e))?;
            mac.update(data);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        "sha512" => {
            type HmacSha512 = Hmac<Sha512>;
            let mut mac = HmacSha512::new_from_slice(key)
                .map_err(|e| format!("HMAC error: {}", e))?;
            mac.update(data);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        _ => Err(format!(
            "Unsupported HMAC algorithm: {}. Supported: sha256, sha384, sha512",
            algorithm
        )),
    }
}
