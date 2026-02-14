//! Codec module implementation (std:codec)
//!
//! Native implementation using the ABI for UTF-8, MessagePack,
//! CBOR, and Protobuf encoding/decoding.

use raya_engine::vm::{
    buffer_allocate, buffer_read_bytes, object_allocate, object_get_field, object_set_field,
    string_allocate, string_read, NativeCallResult, NativeContext, NativeValue,
};

// ============================================================================
// Public API
// ============================================================================

/// Handle codec method calls
pub fn call_codec_method(
    ctx: &NativeContext,
    method_id: u16,
    args: &[NativeValue],
) -> NativeCallResult {
    match method_id {
        // UTF-8
        0x7000 => utf8_encode(ctx, args),
        0x7001 => utf8_decode(ctx, args),
        0x7002 => utf8_is_valid(args),
        0x7003 => utf8_byte_length(args),

        // MessagePack
        0x7010 => msgpack_encode_object(ctx, args),
        0x7011 => msgpack_decode_object(ctx, args),
        0x7012 => msgpack_encoded_size(args),

        // CBOR
        0x7020 => cbor_encode_object(ctx, args),
        0x7021 => cbor_decode_object(ctx, args),
        0x7022 => cbor_diagnostic(ctx, args),

        // Protobuf
        0x7030 => proto_encode_object(ctx, args),
        0x7031 => proto_decode_object(ctx, args),

        _ => NativeCallResult::Unhandled,
    }
}

// ============================================================================
// UTF-8 Methods
// ============================================================================

/// Utf8.encode(text): Buffer
fn utf8_encode(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("Utf8.encode requires 1 argument".to_string());
    }

    let text = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid string: {}", e)),
    };

    let bytes = text.as_bytes();
    NativeCallResult::Value(buffer_allocate(ctx, bytes))
}

/// Utf8.decode(bytes): string
fn utf8_decode(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("Utf8.decode requires 1 argument".to_string());
    }

    let bytes = match buffer_read_bytes(args[0]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid buffer: {}", e)),
    };

    match String::from_utf8(bytes) {
        Ok(text) => NativeCallResult::Value(string_allocate(ctx, text)),
        Err(e) => NativeCallResult::Error(format!("Invalid UTF-8: {}", e)),
    }
}

/// Utf8.isValid(bytes): boolean
fn utf8_is_valid(args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("Utf8.isValid requires 1 argument".to_string());
    }

    let bytes = match buffer_read_bytes(args[0]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid buffer: {}", e)),
    };

    NativeCallResult::bool(std::str::from_utf8(&bytes).is_ok())
}

/// Utf8.byteLength(text): number
fn utf8_byte_length(args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("Utf8.byteLength requires 1 argument".to_string());
    }

    let text = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid string: {}", e)),
    };

    NativeCallResult::f64(text.len() as f64)
}

// ============================================================================
// MessagePack Methods
// ============================================================================

/// Msgpack.encode<T>(obj): Buffer — compiler-lowered with field metadata
/// args = [obj, field_count, key_1, key_2, ...]
fn msgpack_encode_object(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error(
            "Msgpack.encode requires object and field metadata".to_string(),
        );
    }

    let field_count = match args[1].as_i32() {
        Some(n) => n as usize,
        None => return NativeCallResult::Error("Expected field count".to_string()),
    };

    // Build JSON map from object fields + keys
    let mut map = serde_json::Map::new();
    for i in 0..field_count {
        let key = match string_read(args[2 + i]) {
            Ok(s) => s,
            Err(e) => return NativeCallResult::Error(format!("Invalid key: {}", e)),
        };

        let field_val = match object_get_field(args[0], i) {
            Ok(v) => v,
            Err(e) => return NativeCallResult::Error(format!("Field {} error: {}", i, e)),
        };

        let json_val = match native_value_to_json(&field_val) {
            Ok(j) => j,
            Err(e) => return NativeCallResult::Error(format!("Conversion error: {}", e)),
        };

        map.insert(key, json_val);
    }

    let json_value = serde_json::Value::Object(map);
    match rmp_serde::to_vec(&json_value) {
        Ok(bytes) => NativeCallResult::Value(buffer_allocate(ctx, &bytes)),
        Err(e) => NativeCallResult::Error(format!("msgpack encode failed: {}", e)),
    }
}

/// Msgpack.decode<T>(bytes): T — compiler-lowered with field metadata
/// args = [buffer, field_count, key_1, key_2, ...]
fn msgpack_decode_object(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error(
            "Msgpack.decode requires buffer and field metadata".to_string(),
        );
    }

    let bytes = match buffer_read_bytes(args[0]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid buffer: {}", e)),
    };

    let field_count = match args[1].as_i32() {
        Some(n) => n as usize,
        None => return NativeCallResult::Error("Expected field count".to_string()),
    };

    let value: serde_json::Value = match rmp_serde::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => return NativeCallResult::Error(format!("msgpack decode failed: {}", e)),
    };

    decode_object_from_json(ctx, &value, &args[2..2 + field_count])
}

/// Msgpack.encodedSize(bytes): number
fn msgpack_encoded_size(args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("Msgpack.encodedSize requires 1 argument".to_string());
    }

    let bytes = match buffer_read_bytes(args[0]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid buffer: {}", e)),
    };

    NativeCallResult::f64(bytes.len() as f64)
}

// ============================================================================
// CBOR Methods
// ============================================================================

/// Cbor.encode<T>(obj): Buffer — compiler-lowered with field metadata
/// args = [obj, field_count, key_1, key_2, ...]
fn cbor_encode_object(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("Cbor.encode requires object and field metadata".to_string());
    }

    let field_count = match args[1].as_i32() {
        Some(n) => n as usize,
        None => return NativeCallResult::Error("Expected field count".to_string()),
    };

    // Build JSON map from object fields + keys
    let mut map = serde_json::Map::new();
    for i in 0..field_count {
        let key = match string_read(args[2 + i]) {
            Ok(s) => s,
            Err(e) => return NativeCallResult::Error(format!("Invalid key: {}", e)),
        };

        let field_val = match object_get_field(args[0], i) {
            Ok(v) => v,
            Err(e) => return NativeCallResult::Error(format!("Field {} error: {}", i, e)),
        };

        let json_val = match native_value_to_json(&field_val) {
            Ok(j) => j,
            Err(e) => return NativeCallResult::Error(format!("Conversion error: {}", e)),
        };

        map.insert(key, json_val);
    }

    let json_value = serde_json::Value::Object(map);
    let mut bytes = Vec::new();
    match ciborium::ser::into_writer(&json_value, &mut bytes) {
        Ok(_) => NativeCallResult::Value(buffer_allocate(ctx, &bytes)),
        Err(e) => NativeCallResult::Error(format!("cbor encode failed: {}", e)),
    }
}

/// Cbor.decode<T>(bytes): T — compiler-lowered with field metadata
/// args = [buffer, field_count, key_1, key_2, ...]
fn cbor_decode_object(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("Cbor.decode requires buffer and field metadata".to_string());
    }

    let bytes = match buffer_read_bytes(args[0]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid buffer: {}", e)),
    };

    let field_count = match args[1].as_i32() {
        Some(n) => n as usize,
        None => return NativeCallResult::Error("Expected field count".to_string()),
    };

    let value: serde_json::Value = match ciborium::de::from_reader(&bytes[..]) {
        Ok(v) => v,
        Err(e) => return NativeCallResult::Error(format!("cbor decode failed: {}", e)),
    };

    decode_object_from_json(ctx, &value, &args[2..2 + field_count])
}

/// Cbor.diagnostic(bytes): string
fn cbor_diagnostic(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("Cbor.diagnostic requires 1 argument".to_string());
    }

    let bytes = match buffer_read_bytes(args[0]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid buffer: {}", e)),
    };

    let value: serde_json::Value = match ciborium::de::from_reader(&bytes[..]) {
        Ok(v) => v,
        Err(e) => return NativeCallResult::Error(format!("cbor decode failed: {}", e)),
    };

    let diagnostic = format!("{:#}", value);
    NativeCallResult::Value(string_allocate(ctx, diagnostic))
}

// ============================================================================
// Protobuf Methods
// ============================================================================

/// Proto type codes (must match compiler constants)
const PROTO_TYPE_DOUBLE: i32 = 0;
const PROTO_TYPE_FLOAT: i32 = 1;
const PROTO_TYPE_INT32: i32 = 2;
const PROTO_TYPE_INT64: i32 = 3;
const PROTO_TYPE_BOOL: i32 = 4;
const PROTO_TYPE_STRING: i32 = 5;
const PROTO_TYPE_BYTES: i32 = 6;

/// Wire type constants
const WIRE_VARINT: u32 = 0;
const WIRE_FIXED64: u32 = 1;
const WIRE_LENGTH_DELIMITED: u32 = 2;
const WIRE_FIXED32: u32 = 5;

fn proto_wire_type(proto_type: i32) -> u32 {
    match proto_type {
        PROTO_TYPE_DOUBLE => WIRE_FIXED64,
        PROTO_TYPE_FLOAT => WIRE_FIXED32,
        PROTO_TYPE_INT32 | PROTO_TYPE_INT64 | PROTO_TYPE_BOOL => WIRE_VARINT,
        PROTO_TYPE_STRING | PROTO_TYPE_BYTES => WIRE_LENGTH_DELIMITED,
        _ => WIRE_VARINT,
    }
}

/// Protobuf.encode<T>(obj): Buffer — compiler-lowered with field metadata + proto annotations
/// args = [obj, field_count, field_num_1, proto_type_1, field_num_2, proto_type_2, ...]
fn proto_encode_object(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("Protobuf.encode requires object and field metadata".to_string());
    }

    let field_count = match args[1].as_i32() {
        Some(n) => n as usize,
        None => return NativeCallResult::Error("Expected field count".to_string()),
    };

    if args.len() < 2 + field_count * 2 {
        return NativeCallResult::Error("Missing proto field metadata".to_string());
    }

    let mut buffer = Vec::new();

    for i in 0..field_count {
        let field_num = match args[2 + i * 2].as_i32() {
            Some(n) => n as u32,
            None => return NativeCallResult::Error(format!("Invalid field number for field {}", i)),
        };

        let proto_type = match args[2 + i * 2 + 1].as_i32() {
            Some(t) => t,
            None => return NativeCallResult::Error(format!("Invalid proto type for field {}", i)),
        };

        let field_val = match object_get_field(args[0], i) {
            Ok(v) => v,
            Err(e) => return NativeCallResult::Error(format!("Field {} error: {}", i, e)),
        };

        // Encode field tag
        let wire_type = proto_wire_type(proto_type);
        let tag = (field_num << 3) | wire_type;
        encode_varint(&mut buffer, tag as u64);

        // Encode field value based on proto type
        match proto_type {
            PROTO_TYPE_BOOL => {
                // Try bool first, then check numeric values
                let val = if let Some(b) = field_val.as_bool() {
                    b
                } else if let Some(i) = field_val.as_i32() {
                    i != 0
                } else if let Some(f) = field_val.as_f64() {
                    f != 0.0
                } else {
                    false
                };
                encode_varint(&mut buffer, if val { 1 } else { 0 });
            }
            PROTO_TYPE_INT32 => {
                // Try i32 first, then f64 (JSON numbers are f64)
                let val = if let Some(i) = field_val.as_i32() {
                    i
                } else if let Some(f) = field_val.as_f64() {
                    f as i32
                } else {
                    0
                };
                encode_varint(&mut buffer, val as u64);
            }
            PROTO_TYPE_INT64 => {
                // Try f64 first (might be i64 stored as f64)
                let val = if let Some(f) = field_val.as_f64() {
                    f as i64
                } else if let Some(i) = field_val.as_i32() {
                    i as i64
                } else {
                    0
                };
                encode_varint(&mut buffer, val as u64);
            }
            PROTO_TYPE_FLOAT => {
                let val = field_val.as_f64().unwrap_or(0.0) as f32;
                buffer.extend_from_slice(&val.to_le_bytes());
            }
            PROTO_TYPE_DOUBLE => {
                let val = field_val.as_f64().unwrap_or(0.0);
                buffer.extend_from_slice(&val.to_le_bytes());
            }
            PROTO_TYPE_STRING => {
                let s = match string_read(field_val) {
                    Ok(s) => s,
                    Err(_) => String::new(),
                };
                let bytes = s.as_bytes();
                encode_varint(&mut buffer, bytes.len() as u64);
                buffer.extend_from_slice(bytes);
            }
            PROTO_TYPE_BYTES => {
                let bytes = match buffer_read_bytes(field_val) {
                    Ok(b) => b,
                    Err(_) => Vec::new(),
                };
                encode_varint(&mut buffer, bytes.len() as u64);
                buffer.extend_from_slice(&bytes);
            }
            _ => {
                return NativeCallResult::Error(format!("Unsupported proto type: {}", proto_type));
            }
        }
    }

    NativeCallResult::Value(buffer_allocate(ctx, &buffer))
}

/// Protobuf.decode<T>(bytes): T — compiler-lowered with field metadata + proto annotations
/// args = [buffer, field_count, field_num_1, proto_type_1, field_num_2, proto_type_2, ...]
fn proto_decode_object(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("Protobuf.decode requires buffer and field metadata".to_string());
    }

    let bytes = match buffer_read_bytes(args[0]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid buffer: {}", e)),
    };

    let field_count = match args[1].as_i32() {
        Some(n) => n as usize,
        None => return NativeCallResult::Error("Expected field count".to_string()),
    };

    if args.len() < 2 + field_count * 2 {
        return NativeCallResult::Error("Missing proto field metadata".to_string());
    }

    // Build field_num -> (index, proto_type) mapping
    let mut field_map = std::collections::HashMap::new();
    for i in 0..field_count {
        let field_num = match args[2 + i * 2].as_i32() {
            Some(n) => n as u32,
            None => continue,
        };
        let proto_type = match args[2 + i * 2 + 1].as_i32() {
            Some(t) => t,
            None => continue,
        };
        field_map.insert(field_num, (i, proto_type));
    }

    // Allocate object with null fields
    let obj_val = object_allocate(ctx, 0, field_count);
    for i in 0..field_count {
        let _ = object_set_field(obj_val, i, NativeValue::null());
    }

    // Parse protobuf wire format
    let mut pos = 0;
    while pos < bytes.len() {
        // Decode tag
        let (tag, tag_len) = match decode_varint(&bytes[pos..]) {
            Ok((v, len)) => (v as u32, len),
            Err(e) => return NativeCallResult::Error(e),
        };
        pos += tag_len;

        let field_num = tag >> 3;
        let wire_type = tag & 0x7;

        if let Some(&(field_index, proto_type)) = field_map.get(&field_num) {
            // Decode value based on proto type
            let value = match proto_type {
                PROTO_TYPE_BOOL => {
                    let (v, len) = match decode_varint(&bytes[pos..]) {
                        Ok((v, l)) => (v, l),
                        Err(e) => return NativeCallResult::Error(e),
                    };
                    pos += len;
                    NativeValue::bool(v != 0)
                }
                PROTO_TYPE_INT32 => {
                    let (v, len) = match decode_varint(&bytes[pos..]) {
                        Ok((v, l)) => (v, l),
                        Err(e) => return NativeCallResult::Error(e),
                    };
                    pos += len;
                    NativeValue::i32(v as i32)
                }
                PROTO_TYPE_INT64 => {
                    let (v, len) = match decode_varint(&bytes[pos..]) {
                        Ok((v, l)) => (v, l),
                        Err(e) => return NativeCallResult::Error(e),
                    };
                    pos += len;
                    NativeValue::f64(v as i64 as f64)
                }
                PROTO_TYPE_FLOAT => {
                    if pos + 4 > bytes.len() {
                        return NativeCallResult::Error("Truncated float".to_string());
                    }
                    let val = f32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]);
                    pos += 4;
                    NativeValue::f64(val as f64)
                }
                PROTO_TYPE_DOUBLE => {
                    if pos + 8 > bytes.len() {
                        return NativeCallResult::Error("Truncated double".to_string());
                    }
                    let mut arr = [0u8; 8];
                    arr.copy_from_slice(&bytes[pos..pos + 8]);
                    let val = f64::from_le_bytes(arr);
                    pos += 8;
                    NativeValue::f64(val)
                }
                PROTO_TYPE_STRING => {
                    let (len, len_bytes) = match decode_varint(&bytes[pos..]) {
                        Ok((v, l)) => (v as usize, l),
                        Err(e) => return NativeCallResult::Error(e),
                    };
                    pos += len_bytes;
                    if pos + len > bytes.len() {
                        return NativeCallResult::Error("Truncated string".to_string());
                    }
                    let s = match String::from_utf8(bytes[pos..pos + len].to_vec()) {
                        Ok(s) => s,
                        Err(e) => return NativeCallResult::Error(format!("Invalid UTF-8: {}", e)),
                    };
                    pos += len;
                    string_allocate(ctx, s)
                }
                PROTO_TYPE_BYTES => {
                    let (len, len_bytes) = match decode_varint(&bytes[pos..]) {
                        Ok((v, l)) => (v as usize, l),
                        Err(e) => return NativeCallResult::Error(e),
                    };
                    pos += len_bytes;
                    if pos + len > bytes.len() {
                        return NativeCallResult::Error("Truncated bytes".to_string());
                    }
                    let b = bytes[pos..pos + len].to_vec();
                    pos += len;
                    buffer_allocate(ctx, &b)
                }
                _ => {
                    // Skip unknown field type
                    match skip_field(wire_type, &bytes[pos..]) {
                        Ok(skip_len) => {
                            pos += skip_len;
                            continue;
                        }
                        Err(e) => return NativeCallResult::Error(e),
                    }
                }
            };

            let _ = object_set_field(obj_val, field_index, value);
        } else {
            // Skip unknown field
            match skip_field(wire_type, &bytes[pos..]) {
                Ok(skip_len) => pos += skip_len,
                Err(e) => return NativeCallResult::Error(e),
            }
        }
    }

    NativeCallResult::Value(obj_val)
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert a NativeValue to serde_json::Value for encoding
fn native_value_to_json(val: &NativeValue) -> Result<serde_json::Value, String> {
    if val.is_null() {
        Ok(serde_json::Value::Null)
    } else if let Some(b) = val.as_bool() {
        Ok(serde_json::Value::Bool(b))
    } else if let Some(i) = val.as_i32() {
        Ok(serde_json::Value::Number(serde_json::Number::from(i)))
    } else if let Some(f) = val.as_f64() {
        // Whole-number f64 → JSON integer (preserves int semantics)
        if f.fract() == 0.0 && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
            Ok(serde_json::Value::Number(serde_json::Number::from(f as i64)))
        } else {
            Ok(serde_json::json!(f))
        }
    } else if val.is_ptr() {
        // Try string
        if let Ok(s) = string_read(*val) {
            Ok(serde_json::Value::String(s))
        } else {
            Ok(serde_json::Value::Null)
        }
    } else {
        Ok(serde_json::Value::Null)
    }
}

/// Decode an object from a serde_json::Value using field keys
fn decode_object_from_json(
    ctx: &NativeContext,
    json: &serde_json::Value,
    key_args: &[NativeValue],
) -> NativeCallResult {
    let map = match json.as_object() {
        Some(m) => m,
        None => return NativeCallResult::Error("Expected JSON object".to_string()),
    };

    let field_count = key_args.len();
    let obj_val = object_allocate(ctx, 0, field_count);

    for (i, key_val) in key_args.iter().enumerate() {
        let key = match string_read(*key_val) {
            Ok(s) => s,
            Err(e) => return NativeCallResult::Error(format!("Key error: {}", e)),
        };

        let json_val = map.get(&key).unwrap_or(&serde_json::Value::Null);
        let vm_val = json_to_native_value(ctx, json_val);

        if let Err(e) = object_set_field(obj_val, i, vm_val) {
            return NativeCallResult::Error(format!("Set field error: {}", e));
        }
    }

    NativeCallResult::Value(obj_val)
}

/// Convert a serde_json::Value to a NativeValue
fn json_to_native_value(ctx: &NativeContext, json: &serde_json::Value) -> NativeValue {
    match json {
        serde_json::Value::Null => NativeValue::null(),
        serde_json::Value::Bool(b) => NativeValue::bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i >= i32::MIN as i64 && i <= i32::MAX as i64 {
                    NativeValue::i32(i as i32)
                } else {
                    NativeValue::f64(i as f64)
                }
            } else if let Some(f) = n.as_f64() {
                NativeValue::f64(f)
            } else {
                NativeValue::null()
            }
        }
        serde_json::Value::String(s) => string_allocate(ctx, s.clone()),
        _ => NativeValue::null(), // Arrays/objects not supported in MVP
    }
}

// ============================================================================
// Protobuf Wire Format Helpers
// ============================================================================

fn encode_varint(buf: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            buf.push(byte);
            break;
        }
        buf.push(byte | 0x80);
    }
}

fn decode_varint(bytes: &[u8]) -> Result<(u64, usize), String> {
    let mut value: u64 = 0;
    let mut shift = 0;
    for (i, &byte) in bytes.iter().enumerate() {
        value |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok((value, i + 1));
        }
        shift += 7;
        if shift >= 64 {
            return Err("Varint overflow".to_string());
        }
    }
    Err("Truncated varint".to_string())
}

fn skip_field(wire_type: u32, bytes: &[u8]) -> Result<usize, String> {
    match wire_type {
        WIRE_VARINT => {
            let (_, len) = decode_varint(bytes)?;
            Ok(len)
        }
        WIRE_FIXED64 => Ok(8),
        WIRE_LENGTH_DELIMITED => {
            let (len, len_bytes) = decode_varint(bytes)?;
            Ok(len_bytes + len as usize)
        }
        WIRE_FIXED32 => Ok(4),
        _ => Err(format!("Unknown wire type: {}", wire_type)),
    }
}
