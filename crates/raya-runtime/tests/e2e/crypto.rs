//! End-to-end tests for the std:crypto module
//!
//! Tests verify that crypto methods compile and execute correctly,
//! including hashing, HMAC, random generation, encoding, and comparison.

use super::harness::{
    compile_and_run_with_builtins, expect_bool_with_builtins, expect_i32_with_builtins,
    expect_string_with_builtins,
};

// ============================================================================
// Hashing — crypto.hash(algorithm, data)
// ============================================================================

#[test]
fn test_crypto_hash_sha256() {
    expect_string_with_builtins(
        r#"
        import crypto from "std:crypto";
        return crypto.hash("sha256", "hello");
    "#,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
    );
}

#[test]
fn test_crypto_hash_sha384() {
    expect_string_with_builtins(
        r#"
        import crypto from "std:crypto";
        return crypto.hash("sha384", "hello");
    "#,
        "59e1748777448c69de6b800d7a33bbfb9ff1b463e44354c3553bcdb9c666fa90125a3c79f90397bdf5f6a13de828684f",
    );
}

#[test]
fn test_crypto_hash_sha512() {
    expect_string_with_builtins(
        r#"
        import crypto from "std:crypto";
        return crypto.hash("sha512", "hello");
    "#,
        "9b71d224bd62f3785d96d46ad3ea3d73319bfbc2890caadae2dff72519673ca72323c3d99ba5c11d7c7acc6e14b8c5da0c4663475c2e5c3adef46f73bcdec043",
    );
}

#[test]
fn test_crypto_hash_sha1() {
    expect_string_with_builtins(
        r#"
        import crypto from "std:crypto";
        return crypto.hash("sha1", "hello");
    "#,
        "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d",
    );
}

#[test]
fn test_crypto_hash_md5() {
    expect_string_with_builtins(
        r#"
        import crypto from "std:crypto";
        return crypto.hash("md5", "hello");
    "#,
        "5d41402abc4b2a76b9719d911017c592",
    );
}

#[test]
fn test_crypto_hash_empty_string() {
    // SHA-256 of empty string
    expect_string_with_builtins(
        r#"
        import crypto from "std:crypto";
        return crypto.hash("sha256", "");
    "#,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );
}

// ============================================================================
// Hashing — crypto.hashBytes(algorithm, data: Buffer)
// ============================================================================

#[test]
fn test_crypto_hash_bytes_sha256() {
    // Hash a buffer containing "hello" (0x68 0x65 0x6c 0x6c 0x6f)
    // Then convert result to hex — should match crypto.hash("sha256", "hello")
    expect_string_with_builtins(
        r#"
        import crypto from "std:crypto";
        let buf: Buffer = crypto.fromHex("68656c6c6f");
        let digest: Buffer = crypto.hashBytes("sha256", buf);
        return crypto.toHex(digest);
    "#,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
    );
}

// ============================================================================
// HMAC — crypto.hmac(algorithm, key, data)
// ============================================================================

#[test]
fn test_crypto_hmac_sha256() {
    // HMAC-SHA256("key", "hello") — known test vector
    expect_string_with_builtins(
        r#"
        import crypto from "std:crypto";
        return crypto.hmac("sha256", "key", "hello");
    "#,
        "9307b3b915efb5171ff14d8cb55fbcc798c6c0ef1456d66ded1a6aa723a58b7b",
    );
}

#[test]
fn test_crypto_hmac_sha512() {
    // Just verify HMAC-SHA512 compiles and produces a string result
    let result = compile_and_run_with_builtins(
        r#"
        import crypto from "std:crypto";
        let mac: string = crypto.hmac("sha512", "secret", "message");
        return mac;
    "#,
    );
    assert!(result.is_ok(), "HMAC-SHA512 should work: {:?}", result.err());
}

// ============================================================================
// HMAC — crypto.hmacBytes(algorithm, key: Buffer, data: Buffer)
// ============================================================================

#[test]
fn test_crypto_hmac_bytes_sha256() {
    // Same as string HMAC but using buffers
    expect_string_with_builtins(
        r#"
        import crypto from "std:crypto";
        let key: Buffer = crypto.fromHex("6b6579");
        let data: Buffer = crypto.fromHex("68656c6c6f");
        let mac: Buffer = crypto.hmacBytes("sha256", key, data);
        return crypto.toHex(mac);
    "#,
        "9307b3b915efb5171ff14d8cb55fbcc798c6c0ef1456d66ded1a6aa723a58b7b",
    );
}

// ============================================================================
// Random — crypto.randomBytes(size)
// ============================================================================

#[test]
fn test_crypto_random_bytes_returns_buffer() {
    // randomBytes(16) should produce a buffer; convert to hex to verify
    let result = compile_and_run_with_builtins(
        r#"
        import crypto from "std:crypto";
        let buf: Buffer = crypto.randomBytes(16);
        let hex: string = crypto.toHex(buf);
        return hex;
    "#,
    );
    assert!(
        result.is_ok(),
        "randomBytes should work: {:?}",
        result.err()
    );
}

#[test]
fn test_crypto_random_bytes_different() {
    // Two calls to randomBytes should (almost certainly) produce different hex results
    use raya_engine::vm::RayaString;
    let r1 = compile_and_run_with_builtins(
        r#"
        import crypto from "std:crypto";
        let buf: Buffer = crypto.randomBytes(32);
        return crypto.toHex(buf);
    "#,
    )
    .expect("first randomBytes should work");
    let r2 = compile_and_run_with_builtins(
        r#"
        import crypto from "std:crypto";
        let buf: Buffer = crypto.randomBytes(32);
        return crypto.toHex(buf);
    "#,
    )
    .expect("second randomBytes should work");
    let s1 = unsafe { &*r1.as_ptr::<RayaString>().unwrap().as_ptr() };
    let s2 = unsafe { &*r2.as_ptr::<RayaString>().unwrap().as_ptr() };
    assert_ne!(s1.data, s2.data, "Two random byte sequences should differ");
}

// ============================================================================
// Random — crypto.randomInt(min, max)
// ============================================================================

#[test]
fn test_crypto_random_int_in_range() {
    // randomInt(10, 20) should return a value in [10, 20)
    expect_i32_with_builtins(
        r#"
        import crypto from "std:crypto";
        let n: number = crypto.randomInt(10, 20);
        if (n >= 10) {
            if (n < 20) {
                return 1;
            }
        }
        return 0;
    "#,
        1,
    );
}

#[test]
fn test_crypto_random_int_small_range() {
    // randomInt(5, 6) should always return 5
    expect_i32_with_builtins(
        r#"
        import crypto from "std:crypto";
        return crypto.randomInt(5, 6);
    "#,
        5,
    );
}

// ============================================================================
// Random — crypto.randomUUID()
// ============================================================================

#[test]
fn test_crypto_random_uuid_returns_string() {
    // UUID v4 should return a valid string
    let result = compile_and_run_with_builtins(
        r#"
        import crypto from "std:crypto";
        let id: string = crypto.randomUUID();
        return id;
    "#,
    );
    assert!(
        result.is_ok(),
        "randomUUID should work: {:?}",
        result.err()
    );
}

#[test]
fn test_crypto_random_uuid_unique() {
    // Two UUIDs should be different (compare in Rust to avoid type inference issues)
    use raya_engine::vm::RayaString;
    let r1 = compile_and_run_with_builtins(
        r#"
        import crypto from "std:crypto";
        return crypto.randomUUID();
    "#,
    )
    .expect("first randomUUID should work");
    let r2 = compile_and_run_with_builtins(
        r#"
        import crypto from "std:crypto";
        return crypto.randomUUID();
    "#,
    )
    .expect("second randomUUID should work");
    let s1 = unsafe { &*r1.as_ptr::<RayaString>().unwrap().as_ptr() };
    let s2 = unsafe { &*r2.as_ptr::<RayaString>().unwrap().as_ptr() };
    assert_ne!(s1.data, s2.data, "Two UUIDs should differ");
}

// ============================================================================
// Encoding — crypto.toHex / crypto.fromHex
// ============================================================================

#[test]
fn test_crypto_hex_roundtrip() {
    expect_string_with_builtins(
        r#"
        import crypto from "std:crypto";
        let buf: Buffer = crypto.fromHex("48656c6c6f");
        return crypto.toHex(buf);
    "#,
        "48656c6c6f",
    );
}

#[test]
fn test_crypto_from_hex_empty() {
    expect_string_with_builtins(
        r#"
        import crypto from "std:crypto";
        let buf: Buffer = crypto.fromHex("");
        return crypto.toHex(buf);
    "#,
        "",
    );
}

// ============================================================================
// Encoding — crypto.toBase64 / crypto.fromBase64
// ============================================================================

#[test]
fn test_crypto_base64_roundtrip() {
    // "Hello" in hex → base64 → back to hex
    expect_string_with_builtins(
        r#"
        import crypto from "std:crypto";
        let buf: Buffer = crypto.fromHex("48656c6c6f");
        let b64: string = crypto.toBase64(buf);
        let buf2: Buffer = crypto.fromBase64(b64);
        return crypto.toHex(buf2);
    "#,
        "48656c6c6f",
    );
}

#[test]
fn test_crypto_to_base64_known() {
    // "Hello" → base64 is "SGVsbG8="
    expect_string_with_builtins(
        r#"
        import crypto from "std:crypto";
        let buf: Buffer = crypto.fromHex("48656c6c6f");
        return crypto.toBase64(buf);
    "#,
        "SGVsbG8=",
    );
}

#[test]
fn test_crypto_from_base64_known() {
    // "SGVsbG8=" → hex is "48656c6c6f" (Hello)
    expect_string_with_builtins(
        r#"
        import crypto from "std:crypto";
        let buf: Buffer = crypto.fromBase64("SGVsbG8=");
        return crypto.toHex(buf);
    "#,
        "48656c6c6f",
    );
}

// ============================================================================
// Comparison — crypto.timingSafeEqual(a, b)
// ============================================================================

#[test]
fn test_crypto_timing_safe_equal_same() {
    expect_bool_with_builtins(
        r#"
        import crypto from "std:crypto";
        let a: Buffer = crypto.fromHex("aabbccdd");
        let b: Buffer = crypto.fromHex("aabbccdd");
        return crypto.timingSafeEqual(a, b);
    "#,
        true,
    );
}

#[test]
fn test_crypto_timing_safe_equal_different() {
    expect_bool_with_builtins(
        r#"
        import crypto from "std:crypto";
        let a: Buffer = crypto.fromHex("aabbccdd");
        let b: Buffer = crypto.fromHex("aabbccee");
        return crypto.timingSafeEqual(a, b);
    "#,
        false,
    );
}

#[test]
fn test_crypto_timing_safe_equal_different_length() {
    expect_bool_with_builtins(
        r#"
        import crypto from "std:crypto";
        let a: Buffer = crypto.fromHex("aabb");
        let b: Buffer = crypto.fromHex("aabbcc");
        return crypto.timingSafeEqual(a, b);
    "#,
        false,
    );
}

// ============================================================================
// Combined operations
// ============================================================================

#[test]
fn test_crypto_hash_then_verify() {
    // Hash a value and verify it matches the expected hash
    expect_bool_with_builtins(
        r#"
        import crypto from "std:crypto";
        let hash: string = crypto.hash("sha256", "password123");
        let expected: string = "ef92b778bafe771e89245b89ecbc08a44a4e166c06659911881f383d4473e94f";
        return hash == expected;
    "#,
        true,
    );
}

#[test]
fn test_crypto_hmac_verify() {
    // Compute HMAC and verify using timing-safe comparison via hex roundtrip
    expect_bool_with_builtins(
        r#"
        import crypto from "std:crypto";
        let mac1: string = crypto.hmac("sha256", "secret", "data");
        let mac2: string = crypto.hmac("sha256", "secret", "data");
        let buf1: Buffer = crypto.fromHex(mac1);
        let buf2: Buffer = crypto.fromHex(mac2);
        return crypto.timingSafeEqual(buf1, buf2);
    "#,
        true,
    );
}

#[test]
fn test_crypto_import() {
    let result = compile_and_run_with_builtins(
        r#"
        import crypto from "std:crypto";
        let hash: string = crypto.hash("sha256", "test");
        return 1;
    "#,
    );
    assert!(
        result.is_ok(),
        "Crypto should be importable from std:crypto: {:?}",
        result.err()
    );
}
