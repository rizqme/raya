//! Compress module implementation (std:compress)
//!
//! Native implementation using flate2 for gzip, deflate, and zlib
//! compression and decompression.

use raya_sdk::{NativeCallResult, NativeContext, NativeValue};

use flate2::read::{DeflateDecoder, GzDecoder, ZlibDecoder};
use flate2::write::{DeflateEncoder, GzEncoder, ZlibEncoder};
use flate2::Compression;
use std::io::{Read, Write};

// ============================================================================
// Public API
// ============================================================================

/// Handle compress method calls
pub fn call_compress_method(
    ctx: &dyn NativeContext,
    method_id: u16,
    args: &[NativeValue],
) -> NativeCallResult {
    match method_id {
        0x8000 => gzip(ctx, args),
        0x8001 => gunzip(ctx, args),
        0x8002 => deflate(ctx, args),
        0x8003 => inflate(ctx, args),
        0x8004 => zlib_compress(ctx, args),
        0x8005 => zlib_decompress(ctx, args),
        _ => NativeCallResult::Unhandled,
    }
}

// ============================================================================
// Helper
// ============================================================================

/// Extract compression level from args (default 6, clamped to 0-9)
fn get_level(args: &[NativeValue], index: usize) -> u32 {
    let level = args
        .get(index)
        .and_then(|v| v.as_i32().or_else(|| v.as_f64().map(|f| f as i32)))
        .unwrap_or(6);
    level.clamp(0, 9) as u32
}

// ============================================================================
// Method Implementations
// ============================================================================

/// compress.gzip(data: Buffer, level?: number): Buffer
fn gzip(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("compress.gzip requires at least 1 argument".to_string());
    }

    let data = match ctx.read_buffer(args[0]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("compress.gzip: invalid buffer: {}", e)),
    };

    let level = get_level(args, 1);

    let mut encoder = GzEncoder::new(Vec::new(), Compression::new(level));
    if let Err(e) = encoder.write_all(&data) {
        return NativeCallResult::Error(format!("compress.gzip: write error: {}", e));
    }
    match encoder.finish() {
        Ok(compressed) => NativeCallResult::Value(ctx.create_buffer(&compressed)),
        Err(e) => NativeCallResult::Error(format!("compress.gzip: finish error: {}", e)),
    }
}

/// compress.gunzip(data: Buffer): Buffer
fn gunzip(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error(
            "compress.gunzip requires 1 argument".to_string(),
        );
    }

    let data = match ctx.read_buffer(args[0]) {
        Ok(b) => b,
        Err(e) => {
            return NativeCallResult::Error(format!("compress.gunzip: invalid buffer: {}", e))
        }
    };

    let mut decoder = GzDecoder::new(&data[..]);
    let mut decompressed = Vec::new();
    match decoder.read_to_end(&mut decompressed) {
        Ok(_) => NativeCallResult::Value(ctx.create_buffer(&decompressed)),
        Err(e) => NativeCallResult::Error(format!("compress.gunzip: decompression error: {}", e)),
    }
}

/// compress.deflate(data: Buffer, level?: number): Buffer
fn deflate(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error(
            "compress.deflate requires at least 1 argument".to_string(),
        );
    }

    let data = match ctx.read_buffer(args[0]) {
        Ok(b) => b,
        Err(e) => {
            return NativeCallResult::Error(format!("compress.deflate: invalid buffer: {}", e))
        }
    };

    let level = get_level(args, 1);

    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(level));
    if let Err(e) = encoder.write_all(&data) {
        return NativeCallResult::Error(format!("compress.deflate: write error: {}", e));
    }
    match encoder.finish() {
        Ok(compressed) => NativeCallResult::Value(ctx.create_buffer(&compressed)),
        Err(e) => NativeCallResult::Error(format!("compress.deflate: finish error: {}", e)),
    }
}

/// compress.inflate(data: Buffer): Buffer
fn inflate(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error(
            "compress.inflate requires 1 argument".to_string(),
        );
    }

    let data = match ctx.read_buffer(args[0]) {
        Ok(b) => b,
        Err(e) => {
            return NativeCallResult::Error(format!("compress.inflate: invalid buffer: {}", e))
        }
    };

    let mut decoder = DeflateDecoder::new(&data[..]);
    let mut decompressed = Vec::new();
    match decoder.read_to_end(&mut decompressed) {
        Ok(_) => NativeCallResult::Value(ctx.create_buffer(&decompressed)),
        Err(e) => {
            NativeCallResult::Error(format!("compress.inflate: decompression error: {}", e))
        }
    }
}

/// compress.zlibCompress(data: Buffer, level?: number): Buffer
fn zlib_compress(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error(
            "compress.zlibCompress requires at least 1 argument".to_string(),
        );
    }

    let data = match ctx.read_buffer(args[0]) {
        Ok(b) => b,
        Err(e) => {
            return NativeCallResult::Error(format!(
                "compress.zlibCompress: invalid buffer: {}",
                e
            ))
        }
    };

    let level = get_level(args, 1);

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(level));
    if let Err(e) = encoder.write_all(&data) {
        return NativeCallResult::Error(format!("compress.zlibCompress: write error: {}", e));
    }
    match encoder.finish() {
        Ok(compressed) => NativeCallResult::Value(ctx.create_buffer(&compressed)),
        Err(e) => {
            NativeCallResult::Error(format!("compress.zlibCompress: finish error: {}", e))
        }
    }
}

/// compress.zlibDecompress(data: Buffer): Buffer
fn zlib_decompress(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error(
            "compress.zlibDecompress requires 1 argument".to_string(),
        );
    }

    let data = match ctx.read_buffer(args[0]) {
        Ok(b) => b,
        Err(e) => {
            return NativeCallResult::Error(format!(
                "compress.zlibDecompress: invalid buffer: {}",
                e
            ))
        }
    };

    let mut decoder = ZlibDecoder::new(&data[..]);
    let mut decompressed = Vec::new();
    match decoder.read_to_end(&mut decompressed) {
        Ok(_) => NativeCallResult::Value(ctx.create_buffer(&decompressed)),
        Err(e) => NativeCallResult::Error(format!(
            "compress.zlibDecompress: decompression error: {}",
            e
        )),
    }
}
