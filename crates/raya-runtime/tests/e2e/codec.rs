//! End-to-end tests for the std:codec module
//!
//! Tests verify that codec methods compile and execute correctly.
//! Phase 1: Utf8 tests (simple native calls)
//! Phase 2: Msgpack/Cbor/Protobuf tests (compiler-lowered encode/decode<T>)

use super::harness::{
    compile_and_run_with_builtins, expect_bool_with_builtins, expect_f64_with_builtins,
    expect_i32_with_builtins, expect_string_with_builtins,
};

// ============================================================================
// Import
// ============================================================================

#[test]
fn test_codec_import() {
    let result = compile_and_run_with_builtins(
        r#"
        import { Utf8 } from "std:codec";
        return 1;
    "#,
    );
    assert!(
        result.is_ok(),
        "Codec should be importable from std:codec: {:?}",
        result.err()
    );
}

#[test]
fn test_codec_import_all() {
    let result = compile_and_run_with_builtins(
        r#"
        import { Utf8, Msgpack, Cbor, Protobuf } from "std:codec";
        return 1;
    "#,
    );
    assert!(
        result.is_ok(),
        "All codec classes should be importable: {:?}",
        result.err()
    );
}

// ============================================================================
// Utf8.encode
// ============================================================================

#[test]
fn test_utf8_encode() {
    let result = compile_and_run_with_builtins(
        r#"
        import { Utf8 } from "std:codec";
        let buf: Buffer = Utf8.encode("hello");
        if (buf.length > 0) {
            return 1;
        }
        return 0;
    "#,
    );
    assert!(
        result.is_ok(),
        "Utf8.encode should work: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

#[test]
fn test_utf8_encode_length() {
    expect_i32_with_builtins(
        r#"
        import { Utf8 } from "std:codec";
        let buf: Buffer = Utf8.encode("hello");
        return buf.length;
    "#,
        5,
    );
}

// ============================================================================
// Utf8.decode
// ============================================================================

#[test]
fn test_utf8_roundtrip() {
    expect_string_with_builtins(
        r#"
        import { Utf8 } from "std:codec";
        let buf: Buffer = Utf8.encode("hello world");
        return Utf8.decode(buf);
    "#,
        "hello world",
    );
}

#[test]
fn test_utf8_roundtrip_empty() {
    expect_string_with_builtins(
        r#"
        import { Utf8 } from "std:codec";
        let buf: Buffer = Utf8.encode("");
        return Utf8.decode(buf);
    "#,
        "",
    );
}

// ============================================================================
// Utf8.isValid
// ============================================================================

#[test]
fn test_utf8_is_valid_true() {
    expect_bool_with_builtins(
        r#"
        import { Utf8 } from "std:codec";
        let buf: Buffer = Utf8.encode("hello");
        return Utf8.isValid(buf);
    "#,
        true,
    );
}

// ============================================================================
// Utf8.byteLength
// ============================================================================

#[test]
fn test_utf8_byte_length_ascii() {
    expect_f64_with_builtins(
        r#"
        import { Utf8 } from "std:codec";
        return Utf8.byteLength("abc");
    "#,
        3.0,
    );
}

#[test]
fn test_utf8_byte_length_empty() {
    expect_f64_with_builtins(
        r#"
        import { Utf8 } from "std:codec";
        return Utf8.byteLength("");
    "#,
        0.0,
    );
}

// ============================================================================
// Msgpack.encode<T> — Compiler-lowered typed encode
// ============================================================================

#[test]
fn test_msgpack_encode_compiles() {
    let result = compile_and_run_with_builtins(
        r#"
        import { Msgpack } from "std:codec";
        let json: string = '{"name":"Alice","age":30}';
        let obj = JSON.decode<{name: string; age: number}>(json);
        let buf: Buffer = Msgpack.encode<{name: string; age: number}>(obj);
        return 1;
    "#,
    );
    assert!(
        result.is_ok(),
        "Msgpack.encode<T> should compile and run: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

#[test]
fn test_msgpack_encode_produces_buffer() {
    let result = compile_and_run_with_builtins(
        r#"
        import { Msgpack } from "std:codec";
        let json: string = '{"name":"Alice","age":30}';
        let obj = JSON.decode<{name: string; age: number}>(json);
        let buf: Buffer = Msgpack.encode<{name: string; age: number}>(obj);
        if (buf.length > 0) {
            return 1;
        }
        return 0;
    "#,
    );
    assert!(
        result.is_ok(),
        "Msgpack encode should produce non-empty buffer: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

#[test]
fn test_msgpack_encode_with_annotations() {
    // Use //@@json to map Raya field names to wire field names
    let result = compile_and_run_with_builtins(
        r#"
        import { Msgpack } from "std:codec";
        let json: string = '{"user_name":"Charlie","user_age":35}';
        let obj = JSON.decode<{
            //@@json user_name
            name: string;
            //@@json user_age
            age: number;
        }>(json);
        let buf: Buffer = Msgpack.encode<{
            //@@json user_name
            name: string;
            //@@json user_age
            age: number;
        }>(obj);
        if (buf.length > 0) {
            return 1;
        }
        return 0;
    "#,
    );
    assert!(
        result.is_ok(),
        "Msgpack encode with annotations should work: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

// ============================================================================
// Msgpack.decode<T> — Compiler-lowered typed decode
// ============================================================================

#[test]
fn test_msgpack_decode_compiles() {
    let result = compile_and_run_with_builtins(
        r#"
        import { Msgpack } from "std:codec";
        let json: string = '{"name":"Alice","age":30}';
        let obj = JSON.decode<{name: string; age: number}>(json);
        let buf: Buffer = Msgpack.encode<{name: string; age: number}>(obj);
        let decoded = Msgpack.decode<{name: string; age: number}>(buf);
        return 1;
    "#,
    );
    assert!(
        result.is_ok(),
        "Msgpack.decode<T> should compile and run: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

#[test]
fn test_msgpack_roundtrip_string_field() {
    expect_string_with_builtins(
        r#"
        import { Msgpack } from "std:codec";
        let json: string = '{"name":"Alice","age":30}';
        let obj = JSON.decode<{name: string; age: number}>(json);
        let buf: Buffer = Msgpack.encode<{name: string; age: number}>(obj);
        let decoded = Msgpack.decode<{name: string; age: number}>(buf);
        return decoded.name;
    "#,
        "Alice",
    );
}

#[test]
fn test_msgpack_roundtrip_number_field() {
    expect_i32_with_builtins(
        r#"
        import { Msgpack } from "std:codec";
        let json: string = '{"value":42}';
        let obj = JSON.decode<{value: number}>(json);
        let buf: Buffer = Msgpack.encode<{value: number}>(obj);
        let decoded = Msgpack.decode<{value: number}>(buf);
        return decoded.value;
    "#,
        42,
    );
}

#[test]
fn test_msgpack_roundtrip_boolean_field() {
    expect_bool_with_builtins(
        r#"
        import { Msgpack } from "std:codec";
        let json: string = '{"active":true}';
        let obj = JSON.decode<{active: boolean}>(json);
        let buf: Buffer = Msgpack.encode<{active: boolean}>(obj);
        let decoded = Msgpack.decode<{active: boolean}>(buf);
        return decoded.active;
    "#,
        true,
    );
}

#[test]
fn test_msgpack_encoded_size() {
    let result = compile_and_run_with_builtins(
        r#"
        import { Msgpack } from "std:codec";
        let json: string = '{"name":"Alice"}';
        let obj = JSON.decode<{name: string}>(json);
        let buf: Buffer = Msgpack.encode<{name: string}>(obj);
        let size: number = Msgpack.encodedSize(buf);
        if (size > 0) {
            return 1;
        }
        return 0;
    "#,
    );
    assert!(
        result.is_ok(),
        "Msgpack.encodedSize should work: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

// ============================================================================
// Cbor.encode<T> — Compiler-lowered typed encode
// ============================================================================

#[test]
fn test_cbor_encode_compiles() {
    let result = compile_and_run_with_builtins(
        r#"
        import { Cbor } from "std:codec";
        let json: string = '{"name":"Alice","age":30}';
        let obj = JSON.decode<{name: string; age: number}>(json);
        let buf: Buffer = Cbor.encode<{name: string; age: number}>(obj);
        return 1;
    "#,
    );
    assert!(
        result.is_ok(),
        "Cbor.encode<T> should compile and run: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

#[test]
fn test_cbor_encode_produces_buffer() {
    let result = compile_and_run_with_builtins(
        r#"
        import { Cbor } from "std:codec";
        let json: string = '{"name":"Alice","age":30}';
        let obj = JSON.decode<{name: string; age: number}>(json);
        let buf: Buffer = Cbor.encode<{name: string; age: number}>(obj);
        if (buf.length > 0) {
            return 1;
        }
        return 0;
    "#,
    );
    assert!(
        result.is_ok(),
        "Cbor encode should produce non-empty buffer: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

#[test]
fn test_cbor_encode_with_annotations() {
    let result = compile_and_run_with_builtins(
        r#"
        import { Cbor } from "std:codec";
        let json: string = '{"user_name":"Charlie"}';
        let obj = JSON.decode<{
            //@@json user_name
            name: string;
        }>(json);
        let buf: Buffer = Cbor.encode<{
            //@@json user_name
            name: string;
        }>(obj);
        if (buf.length > 0) {
            return 1;
        }
        return 0;
    "#,
    );
    assert!(
        result.is_ok(),
        "Cbor encode with annotations should work: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

// ============================================================================
// Cbor.decode<T> — Compiler-lowered typed decode
// ============================================================================

#[test]
fn test_cbor_decode_compiles() {
    let result = compile_and_run_with_builtins(
        r#"
        import { Cbor } from "std:codec";
        let json: string = '{"name":"Alice","age":30}';
        let obj = JSON.decode<{name: string; age: number}>(json);
        let buf: Buffer = Cbor.encode<{name: string; age: number}>(obj);
        let decoded = Cbor.decode<{name: string; age: number}>(buf);
        return 1;
    "#,
    );
    assert!(
        result.is_ok(),
        "Cbor.decode<T> should compile and run: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

#[test]
fn test_cbor_roundtrip_string_field() {
    expect_string_with_builtins(
        r#"
        import { Cbor } from "std:codec";
        let json: string = '{"name":"Alice","age":30}';
        let obj = JSON.decode<{name: string; age: number}>(json);
        let buf: Buffer = Cbor.encode<{name: string; age: number}>(obj);
        let decoded = Cbor.decode<{name: string; age: number}>(buf);
        return decoded.name;
    "#,
        "Alice",
    );
}

#[test]
fn test_cbor_roundtrip_number_field() {
    expect_i32_with_builtins(
        r#"
        import { Cbor } from "std:codec";
        let json: string = '{"value":42}';
        let obj = JSON.decode<{value: number}>(json);
        let buf: Buffer = Cbor.encode<{value: number}>(obj);
        let decoded = Cbor.decode<{value: number}>(buf);
        return decoded.value;
    "#,
        42,
    );
}

#[test]
fn test_cbor_diagnostic_on_encoded() {
    let result = compile_and_run_with_builtins(
        r#"
        import { Cbor } from "std:codec";
        let json: string = '{"name":"Alice"}';
        let obj = JSON.decode<{name: string}>(json);
        let buf: Buffer = Cbor.encode<{name: string}>(obj);
        let diag: string = Cbor.diagnostic(buf);
        if (diag.length > 0) {
            return 1;
        }
        return 0;
    "#,
    );
    assert!(
        result.is_ok(),
        "Cbor.diagnostic should work on encoded buffer: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

// ============================================================================
// Protobuf.encode<T> — Compiler-lowered typed encode with //@@proto
// ============================================================================

#[test]
fn test_proto_encode_compiles() {
    let result = compile_and_run_with_builtins(
        r#"
        import { Protobuf } from "std:codec";
        let json: string = '{"id":42,"name":"Bob"}';
        let obj = JSON.decode<{id: number; name: string}>(json);
        let buf: Buffer = Protobuf.encode<{
            //@@proto 1,int32
            id: number;
            //@@proto 2
            name: string;
        }>(obj);
        return 1;
    "#,
    );
    assert!(
        result.is_ok(),
        "Protobuf.encode<T> should compile and run: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

#[test]
fn test_proto_encode_produces_buffer() {
    let result = compile_and_run_with_builtins(
        r#"
        import { Protobuf } from "std:codec";
        let json: string = '{"id":42,"name":"Bob"}';
        let obj = JSON.decode<{id: number; name: string}>(json);
        let buf: Buffer = Protobuf.encode<{
            //@@proto 1,int32
            id: number;
            //@@proto 2
            name: string;
        }>(obj);
        if (buf.length > 0) {
            return 1;
        }
        return 0;
    "#,
    );
    assert!(
        result.is_ok(),
        "Protobuf encode should produce non-empty buffer: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

// ============================================================================
// Protobuf.decode<T> — Compiler-lowered typed decode with //@@proto
// ============================================================================

#[test]
fn test_proto_decode_compiles() {
    let result = compile_and_run_with_builtins(
        r#"
        import { Protobuf } from "std:codec";
        let json: string = '{"id":42,"name":"Bob"}';
        let obj = JSON.decode<{id: number; name: string}>(json);
        let buf: Buffer = Protobuf.encode<{
            //@@proto 1,int32
            id: number;
            //@@proto 2
            name: string;
        }>(obj);
        let decoded = Protobuf.decode<{
            //@@proto 1,int32
            id: number;
            //@@proto 2
            name: string;
        }>(buf);
        return 1;
    "#,
    );
    assert!(
        result.is_ok(),
        "Protobuf.decode<T> should compile and run: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

#[test]
fn test_proto_roundtrip_int32() {
    expect_i32_with_builtins(
        r#"
        import { Protobuf } from "std:codec";
        let json: string = '{"id":42,"name":"Bob"}';
        let obj = JSON.decode<{id: number; name: string}>(json);
        let buf: Buffer = Protobuf.encode<{
            //@@proto 1,int32
            id: number;
            //@@proto 2
            name: string;
        }>(obj);
        let decoded = Protobuf.decode<{
            //@@proto 1,int32
            id: number;
            //@@proto 2
            name: string;
        }>(buf);
        return decoded.id;
    "#,
        42,
    );
}

#[test]
fn test_proto_roundtrip_string() {
    expect_string_with_builtins(
        r#"
        import { Protobuf } from "std:codec";
        let json: string = '{"id":1,"name":"Alice"}';
        let obj = JSON.decode<{id: number; name: string}>(json);
        let buf: Buffer = Protobuf.encode<{
            //@@proto 1,int32
            id: number;
            //@@proto 2
            name: string;
        }>(obj);
        let decoded = Protobuf.decode<{
            //@@proto 1,int32
            id: number;
            //@@proto 2
            name: string;
        }>(buf);
        return decoded.name;
    "#,
        "Alice",
    );
}

#[test]
fn test_proto_roundtrip_bool() {
    expect_bool_with_builtins(
        r#"
        import { Protobuf } from "std:codec";
        let json: string = '{"active":true}';
        let obj = JSON.decode<{active: boolean}>(json);
        let buf: Buffer = Protobuf.encode<{
            //@@proto 1
            active: boolean;
        }>(obj);
        let decoded = Protobuf.decode<{
            //@@proto 1
            active: boolean;
        }>(buf);
        return decoded.active;
    "#,
        true,
    );
}

#[test]
fn test_proto_roundtrip_double() {
    expect_f64_with_builtins(
        r#"
        import { Protobuf } from "std:codec";
        let json: string = '{"value":3.14}';
        let obj = JSON.decode<{value: number}>(json);
        let buf: Buffer = Protobuf.encode<{
            //@@proto 1,double
            value: number;
        }>(obj);
        let decoded = Protobuf.decode<{
            //@@proto 1,double
            value: number;
        }>(buf);
        return decoded.value;
    "#,
        3.14,
    );
}
