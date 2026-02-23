//! End-to-end tests for std:compress module

use super::harness::*;

#[test]
fn test_compress_gzip_roundtrip() {
    expect_string_with_builtins(
        r#"
        import compress from "std:compress";
        import crypto from "std:crypto";
        const src: Buffer = crypto.fromHex("68656c6c6f2072617961");
        const gz: Buffer = compress.gzip(src);
        const out: Buffer = compress.gunzip(gz);
        return crypto.toHex(out);
    "#,
        "68656c6c6f2072617961",
    );
}

#[test]
fn test_compress_deflate_roundtrip() {
    expect_bool_with_builtins(
        r#"
        import compress from "std:compress";
        import crypto from "std:crypto";
        const src: Buffer = crypto.fromHex("00010203040506070809");
        const c: Buffer = compress.deflate(src);
        const d: Buffer = compress.inflate(c);
        return crypto.toHex(d) == "00010203040506070809";
    "#,
        true,
    );
}

#[test]
fn test_compress_zlib_roundtrip() {
    expect_bool_with_builtins(
        r#"
        import compress from "std:compress";
        import crypto from "std:crypto";
        const src: Buffer = crypto.fromHex("ffffffff00000000");
        const c: Buffer = compress.zlibCompress(src);
        const d: Buffer = compress.zlibDecompress(c);
        return crypto.toHex(d) == "ffffffff00000000";
    "#,
        true,
    );
}
