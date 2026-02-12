//! Codec method handlers (std:codec)
//!
//! Native implementation of std:codec module for UTF-8, MessagePack,
//! CBOR, and Protobuf encoding/decoding.

use parking_lot::Mutex;

use crate::vm::builtin::codec;
use crate::vm::gc::GarbageCollector as Gc;
use crate::vm::object::{Buffer, RayaString};
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;

// ============================================================================
// Handler Context
// ============================================================================

/// Context needed for codec method execution
pub struct CodecHandlerContext<'a> {
    /// GC for allocating strings and buffers
    pub gc: &'a Mutex<Gc>,
}

// ============================================================================
// Handler
// ============================================================================

/// Handle built-in codec methods (std:codec)
pub fn call_codec_method(
    ctx: &CodecHandlerContext,
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
        // ====================================================================
        // Utf8
        // ====================================================================
        codec::UTF8_ENCODE => {
            // Utf8.encode(text): Buffer
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Utf8.encode requires 1 argument".to_string(),
                ));
            }
            let text = get_string(args[0])?;
            let bytes = text.as_bytes();
            allocate_buffer(ctx, bytes)
        }

        codec::UTF8_DECODE => {
            // Utf8.decode(bytes): string
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Utf8.decode requires 1 argument".to_string(),
                ));
            }
            let bytes = get_buffer_bytes(args[0])?;
            let text = String::from_utf8(bytes)
                .map_err(|e| VmError::RuntimeError(format!("Invalid UTF-8: {}", e)))?;
            allocate_string(ctx, text)
        }

        codec::UTF8_IS_VALID => {
            // Utf8.isValid(bytes): boolean
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Utf8.isValid requires 1 argument".to_string(),
                ));
            }
            let bytes = get_buffer_bytes(args[0])?;
            Value::bool(std::str::from_utf8(&bytes).is_ok())
        }

        codec::UTF8_BYTE_LENGTH => {
            // Utf8.byteLength(text): number
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Utf8.byteLength requires 1 argument".to_string(),
                ));
            }
            let text = get_string(args[0])?;
            Value::f64(text.len() as f64)
        }

        // ====================================================================
        // Msgpack
        // ====================================================================
        codec::MSGPACK_ENCODE_OBJECT => {
            // Msgpack.encode<T>(obj): Buffer — compiler-lowered with field metadata
            // args = [obj, field_count, key_1, key_2, ...]
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "Msgpack.encode requires object and field metadata".to_string(),
                ));
            }
            let field_count = args[1]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected field count".to_string()))?
                as usize;

            // Build JSON map from object fields + keys
            let mut map = serde_json::Map::new();
            for i in 0..field_count {
                let key = get_string(args[2 + i])?;
                let field_val = get_field_value(&args[0], i)?;
                map.insert(key, field_val);
            }

            let json_value = serde_json::Value::Object(map);
            let bytes = rmp_serde::to_vec(&json_value)
                .map_err(|e| VmError::RuntimeError(format!("msgpack encode failed: {}", e)))?;
            allocate_buffer(ctx, &bytes)
        }

        codec::MSGPACK_DECODE_OBJECT => {
            // Msgpack.decode<T>(bytes): T — compiler-lowered with field metadata
            // args = [buffer, field_count, key_1, key_2, ...]
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "Msgpack.decode requires buffer and field metadata".to_string(),
                ));
            }
            let bytes = get_buffer_bytes(args[0])?;
            let field_count = args[1]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected field count".to_string()))?
                as usize;

            let value: serde_json::Value = rmp_serde::from_slice(&bytes)
                .map_err(|e| VmError::RuntimeError(format!("msgpack decode failed: {}", e)))?;

            // Extract fields by key and build typed object
            decode_object_from_json(ctx, &value, &args[2..2 + field_count], &get_string)?
        }

        codec::MSGPACK_ENCODED_SIZE => {
            // Msgpack.encodedSize(bytes): number
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Msgpack.encodedSize requires 1 argument".to_string(),
                ));
            }
            let bytes = get_buffer_bytes(args[0])?;
            Value::f64(bytes.len() as f64)
        }

        // ====================================================================
        // CBOR
        // ====================================================================
        codec::CBOR_ENCODE_OBJECT => {
            // Cbor.encode<T>(obj): Buffer — compiler-lowered with field metadata
            // args = [obj, field_count, key_1, key_2, ...]
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "Cbor.encode requires object and field metadata".to_string(),
                ));
            }
            let field_count = args[1]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected field count".to_string()))?
                as usize;

            // Build JSON map from object fields + keys
            let mut map = serde_json::Map::new();
            for i in 0..field_count {
                let key = get_string(args[2 + i])?;
                let field_val = get_field_value(&args[0], i)?;
                map.insert(key, field_val);
            }

            let json_value = serde_json::Value::Object(map);
            let mut buf = Vec::new();
            ciborium::into_writer(&json_value, &mut buf)
                .map_err(|e| VmError::RuntimeError(format!("CBOR encode failed: {}", e)))?;
            allocate_buffer(ctx, &buf)
        }

        codec::CBOR_DECODE_OBJECT => {
            // Cbor.decode<T>(bytes): T — compiler-lowered with field metadata
            // args = [buffer, field_count, key_1, key_2, ...]
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "Cbor.decode requires buffer and field metadata".to_string(),
                ));
            }
            let bytes = get_buffer_bytes(args[0])?;
            let field_count = args[1]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected field count".to_string()))?
                as usize;

            // Decode CBOR to serde_json::Value
            let cbor_value: ciborium::Value = ciborium::from_reader(&bytes[..])
                .map_err(|e| VmError::RuntimeError(format!("CBOR decode failed: {}", e)))?;

            // Convert ciborium::Value to serde_json::Value for uniform field extraction
            let json_str = serde_json::to_string(&cbor_value)
                .map_err(|e| VmError::RuntimeError(format!("CBOR→JSON failed: {}", e)))?;
            let json_value: serde_json::Value = serde_json::from_str(&json_str)
                .map_err(|e| VmError::RuntimeError(format!("JSON parse failed: {}", e)))?;

            decode_object_from_json(ctx, &json_value, &args[2..2 + field_count], &get_string)?
        }

        codec::CBOR_DIAGNOSTIC => {
            // Cbor.diagnostic(bytes): string
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Cbor.diagnostic requires 1 argument".to_string(),
                ));
            }
            let bytes = get_buffer_bytes(args[0])?;
            let value: ciborium::Value = ciborium::from_reader(&bytes[..])
                .map_err(|e| VmError::RuntimeError(format!("CBOR decode failed: {}", e)))?;
            allocate_string(ctx, format!("{:?}", value))
        }

        // ====================================================================
        // Protobuf
        // ====================================================================
        codec::PROTO_ENCODE_OBJECT => {
            // Protobuf.encode<T>(obj): Buffer — compiler-lowered with proto field metadata
            // args = [obj, field_count, field_num_1, proto_type_1, field_num_2, proto_type_2, ...]
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "Protobuf.encode requires object and field metadata".to_string(),
                ));
            }
            let field_count = args[1]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected field count".to_string()))?
                as usize;

            let mut buf = Vec::new();
            for i in 0..field_count {
                let field_num = args[2 + i * 2]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("Expected field number".to_string()))?
                    as u32;
                let proto_type = args[3 + i * 2]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("Expected proto type".to_string()))?;

                let field_val = get_field_value(&args[0], i)?;
                proto_encode_field(&mut buf, field_num, proto_type, &field_val)?;
            }
            allocate_buffer(ctx, &buf)
        }

        codec::PROTO_DECODE_OBJECT => {
            // Protobuf.decode<T>(bytes): T — compiler-lowered with proto field metadata
            // args = [buffer, field_count, field_num_1, proto_type_1, field_num_2, proto_type_2, ...]
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "Protobuf.decode requires buffer and field metadata".to_string(),
                ));
            }
            let bytes = get_buffer_bytes(args[0])?;
            let field_count = args[1]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected field count".to_string()))?
                as usize;

            // Parse proto wire format into a map of field_number -> raw bytes
            let wire_fields = proto_parse_wire(&bytes)?;

            // Build object from field metadata
            use crate::vm::object::Object;
            let mut obj = Object::new(0, field_count);

            for i in 0..field_count {
                let field_num = args[2 + i * 2]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("Expected field number".to_string()))?
                    as u32;
                let proto_type = args[3 + i * 2]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("Expected proto type".to_string()))?;

                let val = if let Some(field_bytes) = wire_fields.get(&field_num) {
                    proto_decode_field(ctx, proto_type, field_bytes)?
                } else {
                    Value::null()
                };
                obj.set_field(i, val);
            }

            let gc_ptr = ctx.gc.lock().allocate(obj);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
        }

        _ => {
            return Err(VmError::RuntimeError(format!(
                "Unknown codec method: {:#06x}",
                method_id
            )));
        }
    };

    stack.push(result)?;
    Ok(())
}

// ============================================================================
// Helpers
// ============================================================================

/// Extract a field value from an object as serde_json::Value
fn get_field_value(obj: &Value, field_index: usize) -> Result<serde_json::Value, VmError> {
    use crate::vm::object::Object;

    if !obj.is_ptr() {
        return Err(VmError::TypeError("Expected object".to_string()));
    }

    let obj_ptr = unsafe { obj.as_ptr::<Object>() }
        .ok_or_else(|| VmError::TypeError("Expected object".to_string()))?;
    let object = unsafe { &*obj_ptr.as_ptr() };

    let field_val = object.get_field(field_index).unwrap_or_else(Value::null);

    // Convert VM Value to serde_json::Value
    if field_val.is_null() {
        Ok(serde_json::Value::Null)
    } else if let Some(b) = field_val.as_bool() {
        Ok(serde_json::Value::Bool(b))
    } else if let Some(i) = field_val.as_i32() {
        Ok(serde_json::Value::Number(serde_json::Number::from(i)))
    } else if let Some(f) = field_val.as_f64() {
        // Whole-number f64 → JSON integer (preserves int semantics through codec roundtrips)
        if f.fract() == 0.0 && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
            Ok(serde_json::Value::Number(serde_json::Number::from(f as i64)))
        } else {
            Ok(serde_json::json!(f))
        }
    } else if field_val.is_ptr() {
        // Try string
        if let Some(s_ptr) = unsafe { field_val.as_ptr::<RayaString>() } {
            let s = unsafe { &*s_ptr.as_ptr() };
            Ok(serde_json::Value::String(s.data.clone()))
        } else {
            Ok(serde_json::Value::Null)
        }
    } else {
        Ok(serde_json::Value::Null)
    }
}

/// Decode an object from a serde_json::Value using field keys
fn decode_object_from_json(
    ctx: &CodecHandlerContext,
    json: &serde_json::Value,
    key_args: &[Value],
    get_string: &dyn Fn(Value) -> Result<String, VmError>,
) -> Result<Value, VmError> {
    use crate::vm::object::Object;

    let map = json
        .as_object()
        .ok_or_else(|| VmError::RuntimeError("Expected JSON object".to_string()))?;

    let field_count = key_args.len();
    let mut obj = Object::new(0, field_count);

    for (i, key_val) in key_args.iter().enumerate() {
        let key = get_string(*key_val)?;
        let json_val = map.get(&key).unwrap_or(&serde_json::Value::Null);

        let vm_val = json_to_value(ctx, json_val);
        obj.set_field(i, vm_val);
    }

    let gc_ptr = ctx.gc.lock().allocate(obj);
    Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) })
}

/// Convert a serde_json::Value to a VM Value
fn json_to_value(ctx: &CodecHandlerContext, json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::null(),
        serde_json::Value::Bool(b) => Value::bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i >= i32::MIN as i64 && i <= i32::MAX as i64 {
                    Value::i32(i as i32)
                } else {
                    Value::f64(i as f64)
                }
            } else if let Some(f) = n.as_f64() {
                Value::f64(f)
            } else {
                Value::null()
            }
        }
        serde_json::Value::String(s) => allocate_string(ctx, s.clone()),
        _ => Value::null(), // Arrays/objects not supported in MVP
    }
}

/// Allocate a string on the GC heap and return a Value
fn allocate_string(ctx: &CodecHandlerContext, s: String) -> Value {
    let raya_str = RayaString::new(s);
    let gc_ptr = ctx.gc.lock().allocate(raya_str);
    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
}

/// Allocate a Buffer on the GC heap and return a Value
fn allocate_buffer(ctx: &CodecHandlerContext, data: &[u8]) -> Value {
    let mut buffer = Buffer::new(data.len());
    for (i, &byte) in data.iter().enumerate() {
        let _ = buffer.set_byte(i, byte);
    }
    let gc_ptr = ctx.gc.lock().allocate(buffer);
    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
}

// ============================================================================
// Protobuf Wire Format Helpers
// ============================================================================

/// Proto type codes (must match compiler constants in lower/mod.rs)
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

fn decode_varint(bytes: &[u8]) -> Result<(u64, usize), VmError> {
    let mut value: u64 = 0;
    let mut shift = 0;
    for (i, &byte) in bytes.iter().enumerate() {
        value |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok((value, i + 1));
        }
        shift += 7;
        if shift >= 64 {
            return Err(VmError::RuntimeError("varint too long".into()));
        }
    }
    Err(VmError::RuntimeError("unexpected end of varint".into()))
}

/// Encode a single protobuf field
fn proto_encode_field(
    buf: &mut Vec<u8>,
    field_num: u32,
    proto_type: i32,
    json_val: &serde_json::Value,
) -> Result<(), VmError> {
    let wire_type = proto_wire_type(proto_type);
    let tag = (field_num << 3) | wire_type;
    encode_varint(buf, tag as u64);

    match proto_type {
        PROTO_TYPE_DOUBLE => {
            let f = json_val.as_f64().unwrap_or(0.0);
            buf.extend_from_slice(&f.to_le_bytes());
        }
        PROTO_TYPE_FLOAT => {
            let f = json_val.as_f64().unwrap_or(0.0) as f32;
            buf.extend_from_slice(&f.to_le_bytes());
        }
        PROTO_TYPE_INT32 => {
            let i = json_val.as_i64().unwrap_or(0) as i32;
            encode_varint(buf, i as u32 as u64);
        }
        PROTO_TYPE_INT64 => {
            let i = json_val.as_i64().unwrap_or(0);
            encode_varint(buf, i as u64);
        }
        PROTO_TYPE_BOOL => {
            let b = json_val.as_bool().unwrap_or(false);
            encode_varint(buf, if b { 1 } else { 0 });
        }
        PROTO_TYPE_STRING => {
            let s = json_val.as_str().unwrap_or("");
            let bytes = s.as_bytes();
            encode_varint(buf, bytes.len() as u64);
            buf.extend_from_slice(bytes);
        }
        PROTO_TYPE_BYTES => {
            // Bytes stored as JSON null for now (Buffer field extraction not implemented)
            encode_varint(buf, 0);
        }
        _ => {
            return Err(VmError::RuntimeError(format!(
                "Unknown proto type: {}",
                proto_type
            )));
        }
    }
    Ok(())
}

/// Parse protobuf wire format into a map of field_number -> raw bytes
fn proto_parse_wire(
    bytes: &[u8],
) -> Result<std::collections::HashMap<u32, Vec<u8>>, VmError> {
    let mut fields = std::collections::HashMap::new();
    let mut pos = 0;

    while pos < bytes.len() {
        let (tag, consumed) = decode_varint(&bytes[pos..])?;
        pos += consumed;

        let field_num = (tag >> 3) as u32;
        let wire_type = (tag & 0x07) as u32;

        let field_bytes = match wire_type {
            WIRE_VARINT => {
                let start = pos;
                let (_, consumed) = decode_varint(&bytes[pos..])?;
                pos += consumed;
                bytes[start..pos].to_vec()
            }
            WIRE_FIXED64 => {
                if pos + 8 > bytes.len() {
                    return Err(VmError::RuntimeError("unexpected end of fixed64".into()));
                }
                let data = bytes[pos..pos + 8].to_vec();
                pos += 8;
                data
            }
            WIRE_LENGTH_DELIMITED => {
                let (len, consumed) = decode_varint(&bytes[pos..])?;
                pos += consumed;
                let len = len as usize;
                if pos + len > bytes.len() {
                    return Err(VmError::RuntimeError(
                        "unexpected end of length-delimited field".into(),
                    ));
                }
                let data = bytes[pos..pos + len].to_vec();
                pos += len;
                data
            }
            WIRE_FIXED32 => {
                if pos + 4 > bytes.len() {
                    return Err(VmError::RuntimeError("unexpected end of fixed32".into()));
                }
                let data = bytes[pos..pos + 4].to_vec();
                pos += 4;
                data
            }
            _ => {
                return Err(VmError::RuntimeError(format!(
                    "Unknown wire type: {}",
                    wire_type
                )));
            }
        };

        fields.insert(field_num, field_bytes);
    }

    Ok(fields)
}

/// Decode a single protobuf field from raw bytes
fn proto_decode_field(
    ctx: &CodecHandlerContext,
    proto_type: i32,
    field_bytes: &[u8],
) -> Result<Value, VmError> {
    match proto_type {
        PROTO_TYPE_DOUBLE => {
            if field_bytes.len() != 8 {
                return Err(VmError::RuntimeError("invalid double field".into()));
            }
            let f = f64::from_le_bytes(field_bytes[..8].try_into().unwrap());
            Ok(Value::f64(f))
        }
        PROTO_TYPE_FLOAT => {
            if field_bytes.len() != 4 {
                return Err(VmError::RuntimeError("invalid float field".into()));
            }
            let f = f32::from_le_bytes(field_bytes[..4].try_into().unwrap());
            Ok(Value::f64(f as f64))
        }
        PROTO_TYPE_INT32 => {
            let (val, _) = decode_varint(field_bytes)?;
            Ok(Value::i32(val as i32))
        }
        PROTO_TYPE_INT64 => {
            let (val, _) = decode_varint(field_bytes)?;
            Ok(Value::f64(val as i64 as f64))
        }
        PROTO_TYPE_BOOL => {
            let (val, _) = decode_varint(field_bytes)?;
            Ok(Value::bool(val != 0))
        }
        PROTO_TYPE_STRING => {
            let s = String::from_utf8(field_bytes.to_vec())
                .map_err(|e| VmError::RuntimeError(format!("Invalid UTF-8 in proto: {}", e)))?;
            Ok(allocate_string(ctx, s))
        }
        PROTO_TYPE_BYTES => {
            Ok(allocate_buffer(ctx, field_bytes))
        }
        _ => {
            Err(VmError::RuntimeError(format!(
                "Unknown proto type: {}",
                proto_type
            )))
        }
    }
}
