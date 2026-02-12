# Milestone 4.6: std:crypto Module

**Status:** Complete
**Depends on:** Milestone 4.2 (stdlib pattern), Milestone 4.3 (std:math — established default export pattern)
**Goal:** Implement cryptographic primitives as `std:crypto` with hashing, HMAC, secure random, encoding, and constant-time comparison

---

## Overview

Cryptographic functions are provided as a standard library module `std:crypto`. Uses engine-side handler architecture (like `std:runtime`) for direct Buffer/heap access:

```
Crypto.raya (raya-stdlib)  →  __NATIVE_CALL(ID, args)
                                      ↓
                           NativeCall opcode (VM)
                                      ↓
                           is_crypto_method() check (builtin.rs)
                                      ↓
                           call_crypto_method() (handlers/crypto.rs)
                                      ↓
                           Direct GC heap access for Buffer I/O
```

**Architecture decision:** Engine-side handler instead of NativeHandler trait because `NativeCallResult` only supports `String`/`Number`/`Bool`/`Void` — no `Buffer` variant. Crypto needs direct Buffer allocation/reading via GC heap access.

### Usage

```typescript
import crypto from "std:crypto";

// ── Hashing ──
let digest: string = crypto.hash("sha256", "hello world");
let md5: string = crypto.hash("md5", "password");

// ── HMAC ──
let mac: string = crypto.hmac("sha256", "secret-key", "message");

// ── Random ──
let bytes: Buffer = crypto.randomBytes(32);
let n: number = crypto.randomInt(1, 100);
let id: string = crypto.randomUUID();

// ── Encoding ──
let hex: string = crypto.toHex(bytes);
let raw: Buffer = crypto.fromHex("deadbeef");
let b64: string = crypto.toBase64(bytes);
let decoded: Buffer = crypto.fromBase64(b64);

// ── Constant-time comparison ──
let equal: boolean = crypto.timingSafeEqual(raw, raw);
```

---

## API Design

### Module: `std:crypto`

```typescript
class Crypto {
    // ── Hashing ──
    /** One-shot hash. Returns hex-encoded digest string. */
    hash(algorithm: string, data: string): string;

    /** Hash binary data. Returns raw digest bytes. */
    hashBytes(algorithm: string, data: Buffer): Buffer;

    // ── HMAC ──
    /** Keyed HMAC. Returns hex-encoded MAC string. */
    hmac(algorithm: string, key: string, data: string): string;

    /** HMAC on binary data. Returns raw MAC bytes. */
    hmacBytes(algorithm: string, key: Buffer, data: Buffer): Buffer;

    // ── Random ──
    /** Cryptographically secure random bytes. */
    randomBytes(size: number): Buffer;

    /** Random integer in [min, max). */
    randomInt(min: number, max: number): number;

    /** Generate UUID v4 string. */
    randomUUID(): string;

    // ── Encoding ──
    /** Binary to hex string. */
    toHex(data: Buffer): string;

    /** Hex string to binary. Errors on invalid hex. */
    fromHex(hex: string): Buffer;

    /** Binary to base64 string (standard encoding). */
    toBase64(data: Buffer): string;

    /** Base64 string to binary. Errors on invalid base64. */
    fromBase64(b64: string): Buffer;

    // ── Comparison ──
    /** Constant-time equality check (prevents timing attacks). */
    timingSafeEqual(a: Buffer, b: Buffer): boolean;
}

const crypto = new Crypto();
export default crypto;
```

### Supported Hash Algorithms

| Algorithm | Output Size | Notes |
|-----------|------------|-------|
| `"sha256"` | 32 bytes | Default / recommended |
| `"sha384"` | 48 bytes | |
| `"sha512"` | 64 bytes | |
| `"sha1"` | 20 bytes | Legacy, not recommended for security |
| `"md5"` | 16 bytes | Legacy, not recommended for security |

---

## Native IDs

Range: `0x4000-0x40FF`

| ID | Constant | Method | Args | Return |
|----|----------|--------|------|--------|
| 0x4000 | `CRYPTO_HASH` | `hash(algorithm, data)` | string, string | string |
| 0x4001 | `CRYPTO_HASH_BYTES` | `hashBytes(algorithm, data)` | string, Buffer | Buffer |
| 0x4002 | `CRYPTO_HMAC` | `hmac(algorithm, key, data)` | string, string, string | string |
| 0x4003 | `CRYPTO_HMAC_BYTES` | `hmacBytes(algorithm, key, data)` | string, Buffer, Buffer | Buffer |
| 0x4004 | `CRYPTO_RANDOM_BYTES` | `randomBytes(size)` | number | Buffer |
| 0x4005 | `CRYPTO_RANDOM_INT` | `randomInt(min, max)` | number, number | number |
| 0x4006 | `CRYPTO_RANDOM_UUID` | `randomUUID()` | — | string |
| 0x4007 | `CRYPTO_TO_HEX` | `toHex(data)` | Buffer | string |
| 0x4008 | `CRYPTO_FROM_HEX` | `fromHex(hex)` | string | Buffer |
| 0x4009 | `CRYPTO_TO_BASE64` | `toBase64(data)` | Buffer | string |
| 0x400A | `CRYPTO_FROM_BASE64` | `fromBase64(b64)` | string | Buffer |
| 0x400B | `CRYPTO_TIMING_SAFE_EQUAL` | `timingSafeEqual(a, b)` | Buffer, Buffer | boolean |

---

## Phases

### Phase 1: Native IDs & Engine Infrastructure

**Status:** Complete

**Tasks:**
- [x] Define native IDs in `builtin.rs`
  - [x] Add `pub mod crypto { ... }` with IDs 0x4000-0x400B
  - [x] Add `is_crypto_method()` helper
- [x] Add corresponding constants in `native_id.rs`
  - [x] `CRYPTO_HASH` through `CRYPTO_TIMING_SAFE_EQUAL` (12 constants)
- [x] Add `native_name()` entries for all crypto IDs
- [x] Add crypto crate dependencies to `raya-engine/Cargo.toml` (sha1, md-5, hmac, uuid, base64, subtle)

**Files:**
- `crates/raya-engine/src/vm/builtin.rs`
- `crates/raya-engine/src/compiler/native_id.rs`
- `crates/raya-engine/Cargo.toml`

---

### Phase 2: Raya Source & Stdlib Implementation

**Status:** Complete

**Implementation note:** Crypto handlers live engine-side in `handlers/crypto.rs` (not through NativeHandler/raya-stdlib) because Buffer I/O requires direct GC heap access.

**Tasks:**
- [x] Create `crates/raya-stdlib/raya/Crypto.raya` — class with `__NATIVE_CALL` methods + `export default`
- [x] Create `crates/raya-stdlib/raya/crypto.d.raya` — type declarations
- [x] Register `Crypto.raya` in std module registry (`std_modules.rs`)

**Files:**
- `crates/raya-stdlib/raya/Crypto.raya` (new)
- `crates/raya-stdlib/raya/crypto.d.raya` (new)
- `crates/raya-engine/src/compiler/module/std_modules.rs`

---

### Phase 3: VM Dispatch & Engine Handler

**Status:** Complete

**Tasks:**
- [x] Create `crates/raya-engine/src/vm/vm/handlers/crypto.rs` — engine-side handler
  - [x] `CryptoHandlerContext` with GC reference
  - [x] `call_crypto_method()` with all 12 match arms
  - [x] Helper functions: `hash_string`, `hmac_compute`, `allocate_string`, `allocate_buffer`
- [x] Register in `handlers/mod.rs`
- [x] Add dispatch in `task_interpreter.rs` at both native call sites
- [x] Add `call_crypto_method` bridge function to TaskInterpreter impl

**Files:**
- `crates/raya-engine/src/vm/vm/handlers/crypto.rs` (new)
- `crates/raya-engine/src/vm/vm/handlers/mod.rs`
- `crates/raya-engine/src/vm/vm/task_interpreter.rs`

---

### Phase 4: E2E Tests

**Status:** Complete

**Tasks:**
- [x] Update test harness `get_std_sources()` to include `Crypto.raya`
- [x] Create `crates/raya-runtime/tests/e2e/crypto.rs` — 27 tests
- [x] Register `mod crypto;` in `crates/raya-runtime/tests/e2e/mod.rs`

**Test coverage:** 27 tests covering all 12 crypto methods:
- Hashing: SHA-256, SHA-384, SHA-512, SHA-1, MD5, empty string, hashBytes
- HMAC: SHA-256 (string + bytes), SHA-512, verification
- Random: randomBytes (buffer, uniqueness), randomInt (range, bounds), randomUUID (format, uniqueness)
- Encoding: hex roundtrip, empty hex, base64 roundtrip, known values
- Comparison: equal, different, different-length buffers
- Integration: hash-then-verify, HMAC verify, import syntax

**Files:**
- `crates/raya-runtime/tests/e2e/harness.rs`
- `crates/raya-runtime/tests/e2e/crypto.rs` (new)
- `crates/raya-runtime/tests/e2e/mod.rs`

---

## Dependencies

Added to `crates/raya-engine/Cargo.toml` (engine-side, not raya-stdlib):

```toml
sha2 = "0.10"    # already existed
sha1 = "0.10"    # new
md-5 = "0.10"    # new
hmac = "0.12"    # new
hex = "0.4"      # already existed
base64 = "0.22"  # new
uuid = { version = "1", features = ["v4"] }  # new
subtle = "2.5"   # new
rand = { workspace = true }  # already existed
```

---

## Key Files Reference

| File | Purpose |
|------|---------|
| `crates/raya-stdlib/raya/Crypto.raya` | Raya source: Crypto class + `__NATIVE_CALL` + `export default` |
| `crates/raya-stdlib/raya/crypto.d.raya` | Type declarations for IDE/tooling |
| `crates/raya-engine/src/vm/builtin.rs` | `pub mod crypto` IDs + `is_crypto_method()` |
| `crates/raya-engine/src/compiler/native_id.rs` | `CRYPTO_*` constants + `native_name()` entries |
| `crates/raya-engine/src/compiler/module/std_modules.rs` | Register `Crypto.raya` in std module registry |
| `crates/raya-engine/src/vm/vm/handlers/crypto.rs` | Engine-side handler with all 12 methods |
| `crates/raya-engine/src/vm/vm/handlers/mod.rs` | Register + re-export crypto handler |
| `crates/raya-engine/src/vm/vm/task_interpreter.rs` | VM dispatch at both native call sites |
| `crates/raya-runtime/tests/e2e/harness.rs` | `get_std_sources()` includes `Crypto.raya` |
| `crates/raya-runtime/tests/e2e/crypto.rs` | 27 E2E tests |

## Known Limitations

- **String comparison between two native call results:** The type checker doesn't fully resolve `__NATIVE_CALL<T>` return types, so comparing two native call string results with `==`/`!=` in Raya code uses the wrong comparison opcode. Workaround: compare with a string literal, or compare in Rust test code.
