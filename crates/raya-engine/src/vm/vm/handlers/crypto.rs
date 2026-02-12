//! Crypto method handlers (std:crypto)
//!
//! Native implementation of std:crypto module for hashing, HMAC,
//! secure random, encoding, and constant-time comparison.

use parking_lot::Mutex;

use crate::vm::builtin::crypto;
use crate::vm::gc::GarbageCollector as Gc;
use crate::vm::object::{Buffer, RayaString};
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;

use sha2::{Digest, Sha256, Sha384, Sha512};

// ============================================================================
// Handler Context
// ============================================================================

/// Context needed for crypto method execution
pub struct CryptoHandlerContext<'a> {
    /// GC for allocating strings and buffers
    pub gc: &'a Mutex<Gc>,
}

// ============================================================================
// Handler
// ============================================================================

/// Handle built-in crypto methods (std:crypto)
pub fn call_crypto_method(
    ctx: &CryptoHandlerContext,
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

    // Helper to get string from Value
    let get_string = |v: Value| -> Result<String, VmError> {
        if !v.is_ptr() {
            return Err(VmError::TypeError("Expected string".to_string()));
        }
        let s_ptr = unsafe { v.as_ptr::<RayaString>() };
        let s = unsafe { &*s_ptr.unwrap().as_ptr() };
        Ok(s.data.clone())
    };

    // Helper to get Buffer bytes from Value
    let get_buffer_bytes = |v: Value| -> Result<Vec<u8>, VmError> {
        if !v.is_ptr() {
            return Err(VmError::TypeError("Expected Buffer".to_string()));
        }
        let buf_ptr = unsafe { v.as_ptr::<Buffer>() }
            .ok_or_else(|| VmError::TypeError("Expected Buffer".to_string()))?;
        let buffer = unsafe { &*buf_ptr.as_ptr() };
        Ok((0..buffer.length())
            .filter_map(|i| buffer.get_byte(i))
            .collect())
    };

    let result = match method_id {
        crypto::HASH => {
            // crypto.hash(algorithm, data): string — hex-encoded digest
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "crypto.hash requires 2 arguments".to_string(),
                ));
            }
            let algorithm = get_string(args[0])?;
            let data = get_string(args[1])?;
            let digest = hash_string(&algorithm, data.as_bytes())?;
            let hex = hex::encode(digest);
            allocate_string(ctx, hex)
        }

        crypto::HASH_BYTES => {
            // crypto.hashBytes(algorithm, data: Buffer): Buffer
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "crypto.hashBytes requires 2 arguments".to_string(),
                ));
            }
            let algorithm = get_string(args[0])?;
            let data = get_buffer_bytes(args[1])?;
            let digest = hash_string(&algorithm, &data)?;
            allocate_buffer(ctx, &digest)
        }

        crypto::HMAC => {
            // crypto.hmac(algorithm, key, data): string — hex-encoded MAC
            if args.len() < 3 {
                return Err(VmError::RuntimeError(
                    "crypto.hmac requires 3 arguments".to_string(),
                ));
            }
            let algorithm = get_string(args[0])?;
            let key = get_string(args[1])?;
            let data = get_string(args[2])?;
            let mac = hmac_compute(&algorithm, key.as_bytes(), data.as_bytes())?;
            let hex = hex::encode(mac);
            allocate_string(ctx, hex)
        }

        crypto::HMAC_BYTES => {
            // crypto.hmacBytes(algorithm, key: Buffer, data: Buffer): Buffer
            if args.len() < 3 {
                return Err(VmError::RuntimeError(
                    "crypto.hmacBytes requires 3 arguments".to_string(),
                ));
            }
            let algorithm = get_string(args[0])?;
            let key = get_buffer_bytes(args[1])?;
            let data = get_buffer_bytes(args[2])?;
            let mac = hmac_compute(&algorithm, &key, &data)?;
            allocate_buffer(ctx, &mac)
        }

        crypto::RANDOM_BYTES => {
            // crypto.randomBytes(size): Buffer
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "crypto.randomBytes requires 1 argument".to_string(),
                ));
            }
            let size = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for size".to_string()))?
                as usize;
            if size > 65536 {
                return Err(VmError::RuntimeError(
                    "crypto.randomBytes: size too large (max 65536)".to_string(),
                ));
            }
            let mut bytes = vec![0u8; size];
            use rand::RngCore;
            rand::thread_rng().fill_bytes(&mut bytes);
            allocate_buffer(ctx, &bytes)
        }

        crypto::RANDOM_INT => {
            // crypto.randomInt(min, max): number — random integer in [min, max)
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "crypto.randomInt requires 2 arguments".to_string(),
                ));
            }
            let min = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for min".to_string()))?;
            let max = args[1]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for max".to_string()))?;
            if min >= max {
                return Err(VmError::RuntimeError(
                    "crypto.randomInt: min must be less than max".to_string(),
                ));
            }
            use rand::Rng;
            let n = rand::thread_rng().gen_range(min..max);
            Value::i32(n)
        }

        crypto::RANDOM_UUID => {
            // crypto.randomUUID(): string — UUID v4
            let id = uuid::Uuid::new_v4().to_string();
            allocate_string(ctx, id)
        }

        crypto::TO_HEX => {
            // crypto.toHex(data: Buffer): string
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "crypto.toHex requires 1 argument".to_string(),
                ));
            }
            let data = get_buffer_bytes(args[0])?;
            let hex = hex::encode(data);
            allocate_string(ctx, hex)
        }

        crypto::FROM_HEX => {
            // crypto.fromHex(hex: string): Buffer
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "crypto.fromHex requires 1 argument".to_string(),
                ));
            }
            let hex_str = get_string(args[0])?;
            let bytes = hex::decode(&hex_str).map_err(|e| {
                VmError::RuntimeError(format!("crypto.fromHex: invalid hex: {}", e))
            })?;
            allocate_buffer(ctx, &bytes)
        }

        crypto::TO_BASE64 => {
            // crypto.toBase64(data: Buffer): string
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "crypto.toBase64 requires 1 argument".to_string(),
                ));
            }
            let data = get_buffer_bytes(args[0])?;
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
            allocate_string(ctx, b64)
        }

        crypto::FROM_BASE64 => {
            // crypto.fromBase64(b64: string): Buffer
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "crypto.fromBase64 requires 1 argument".to_string(),
                ));
            }
            let b64_str = get_string(args[0])?;
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&b64_str)
                .map_err(|e| {
                    VmError::RuntimeError(format!("crypto.fromBase64: invalid base64: {}", e))
                })?;
            allocate_buffer(ctx, &bytes)
        }

        crypto::TIMING_SAFE_EQUAL => {
            // crypto.timingSafeEqual(a: Buffer, b: Buffer): boolean
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "crypto.timingSafeEqual requires 2 arguments".to_string(),
                ));
            }
            let a = get_buffer_bytes(args[0])?;
            let b = get_buffer_bytes(args[1])?;
            if a.len() != b.len() {
                Value::bool(false)
            } else {
                use subtle::ConstantTimeEq;
                let equal: bool = a.ct_eq(&b).into();
                Value::bool(equal)
            }
        }

        _ => {
            return Err(VmError::RuntimeError(format!(
                "Unknown crypto method: {:#06x}",
                method_id
            )));
        }
    };

    stack.push(result)?;
    Ok(())
}

// ============================================================================
// Crypto Helpers
// ============================================================================

/// Hash data with the given algorithm, returning raw digest bytes
fn hash_string(algorithm: &str, data: &[u8]) -> Result<Vec<u8>, VmError> {
    match algorithm {
        "sha256" => Ok(Sha256::digest(data).to_vec()),
        "sha384" => Ok(Sha384::digest(data).to_vec()),
        "sha512" => Ok(Sha512::digest(data).to_vec()),
        "sha1" => {
            use sha1::Sha1;
            Ok(Sha1::digest(data).to_vec())
        }
        "md5" => {
            use md5::Md5;
            Ok(Md5::digest(data).to_vec())
        }
        _ => Err(VmError::RuntimeError(format!(
            "Unsupported hash algorithm: {}. Supported: sha256, sha384, sha512, sha1, md5",
            algorithm
        ))),
    }
}

/// Compute HMAC with the given algorithm
fn hmac_compute(algorithm: &str, key: &[u8], data: &[u8]) -> Result<Vec<u8>, VmError> {
    use hmac::{Hmac, Mac};

    match algorithm {
        "sha256" => {
            type HmacSha256 = Hmac<Sha256>;
            let mut mac = HmacSha256::new_from_slice(key)
                .map_err(|e| VmError::RuntimeError(format!("HMAC error: {}", e)))?;
            mac.update(data);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        "sha384" => {
            type HmacSha384 = Hmac<Sha384>;
            let mut mac = HmacSha384::new_from_slice(key)
                .map_err(|e| VmError::RuntimeError(format!("HMAC error: {}", e)))?;
            mac.update(data);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        "sha512" => {
            type HmacSha512 = Hmac<Sha512>;
            let mut mac = HmacSha512::new_from_slice(key)
                .map_err(|e| VmError::RuntimeError(format!("HMAC error: {}", e)))?;
            mac.update(data);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        _ => Err(VmError::RuntimeError(format!(
            "Unsupported HMAC algorithm: {}. Supported: sha256, sha384, sha512",
            algorithm
        ))),
    }
}

/// Allocate a string on the GC heap and return a Value
fn allocate_string(ctx: &CryptoHandlerContext, s: String) -> Value {
    let raya_str = RayaString::new(s);
    let gc_ptr = ctx.gc.lock().allocate(raya_str);
    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
}

/// Allocate a Buffer on the GC heap and return a Value
fn allocate_buffer(ctx: &CryptoHandlerContext, data: &[u8]) -> Value {
    let mut buffer = Buffer::new(data.len());
    for (i, &byte) in data.iter().enumerate() {
        let _ = buffer.set_byte(i, byte);
    }
    let gc_ptr = ctx.gc.lock().allocate(buffer);
    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
}
