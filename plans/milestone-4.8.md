# Milestone 4.8: std:path & std:codec Modules

**Status:** Complete (All phases done, 31 codec + 21 path e2e tests)
**Depends on:** Milestone 4.2 (stdlib pattern), Milestone 4.6 (engine-side handler with Buffer/GC)
**Goal:** Implement path manipulation as `std:path` and binary codec support as `std:codec` (UTF-8, MessagePack, CBOR, Protobuf)

---

## Overview

Two standard library modules sharing a milestone because both deal with data transformation and have similar engine-side handler requirements (GC access for Buffer/string allocation).

```
path.raya / codec.raya (raya-stdlib)  →  __NATIVE_CALL(ID, args)
                                                ↓
                                     NativeCall opcode (VM)
                                                ↓
                                     is_path_method() / is_codec_method() (builtin.rs)
                                                ↓
                                     call_path_method() / call_codec_method() (handlers/)
                                                ↓
                                     std::path / serde / proto wire format
```

**Architecture:** Engine-side handlers (like crypto) — both modules need direct GC heap access for Buffer allocation and string returns.

**Crate dependencies (new):**
- `rmp-serde` — MessagePack encode/decode via serde
- `ciborium` — CBOR encode/decode via serde
- (No protobuf crate — wire format implemented directly from compiler-derived type metadata)

---

## Module: std:path

### Usage

```typescript
import path from "std:path";

// ── Join & Normalize ──
let p: string = path.join("users", "alice");       // "users/alice"
path.join("/home", "docs/file.txt");                // "/home/docs/file.txt"
path.normalize("/foo/bar/../baz");                  // "/foo/baz"

// ── Extract Components ──
path.dirname("/home/alice/file.txt");               // "/home/alice"
path.basename("/home/alice/file.txt");              // "file.txt"
path.extname("/home/alice/file.txt");               // ".txt"

// ── Absolute / Relative ──
path.isAbsolute("/foo/bar");                        // true
path.isAbsolute("foo/bar");                         // false
path.resolve("foo", "bar");                         // "/cwd/foo/bar"
path.relative("/data/a", "/data/a/b/c");            // "b/c"
path.cwd();                                         // "/current/working/directory"

// ── OS Constants ──
path.sep();                                         // "/" on Unix, "\" on Windows
path.delimiter();                                   // ":" on Unix, ";" on Windows

// ── Pure Raya Utilities ──
path.stripExt("file.txt");                          // "file"
path.withExt("file.txt", ".md");                    // "file.md"
path.isRelative("foo/bar");                         // true
```

### API

```typescript
class Path {
    // ── Join & Normalize (native) ──
    join(a: string, b: string): string;
    normalize(p: string): string;

    // ── Components (native) ──
    dirname(p: string): string;
    basename(p: string): string;
    extname(p: string): string;

    // ── Resolution (native) ──
    isAbsolute(p: string): boolean;
    resolve(from: string, to: string): string;
    relative(from: string, to: string): string;
    cwd(): string;

    // ── OS Constants (native) ──
    sep(): string;
    delimiter(): string;

    // ── Utilities (pure Raya) ──
    stripExt(p: string): string;     // basename without extension
    withExt(p: string, ext: string): string;  // replace extension
    isRelative(p: string): boolean;  // !isAbsolute(p)
}

const path = new Path();
export default path;
```

**11 native calls + 3 pure Raya methods = 14 total.**

### Native IDs (0x6000–0x60FF)

| ID | Constant | Method | Args | Return |
|----|----------|--------|------|--------|
| 0x6000 | `PATH_JOIN` | `join(a, b)` | 2 strings | string |
| 0x6001 | `PATH_NORMALIZE` | `normalize(p)` | 1 string | string |
| 0x6002 | `PATH_DIRNAME` | `dirname(p)` | 1 string | string |
| 0x6003 | `PATH_BASENAME` | `basename(p)` | 1 string | string |
| 0x6004 | `PATH_EXTNAME` | `extname(p)` | 1 string | string |
| 0x6005 | `PATH_IS_ABSOLUTE` | `isAbsolute(p)` | 1 string | boolean |
| 0x6006 | `PATH_RESOLVE` | `resolve(from, to)` | 2 strings | string |
| 0x6007 | `PATH_RELATIVE` | `relative(from, to)` | 2 strings | string |
| 0x6008 | `PATH_CWD` | `cwd()` | — | string |
| 0x6009 | `PATH_SEP` | `sep()` | — | string |
| 0x600A | `PATH_DELIMITER` | `delimiter()` | — | string |

### Rust Implementation

```rust
use std::path::{Path, PathBuf, MAIN_SEPARATOR, MAIN_SEPARATOR_STR};

fn path_join(a: &str, b: &str) -> String {
    PathBuf::from(a).join(b).to_string_lossy().to_string()
}

fn path_normalize(p: &str) -> String {
    // Manual normalization: resolve `.` and `..` components
    // std::path doesn't have a normalize that resolves `..` without I/O
    let path = Path::new(p);
    let mut components = Vec::new();
    for comp in path.components() {
        match comp {
            std::path::Component::ParentDir => { components.pop(); }
            std::path::Component::CurDir => {}
            other => components.push(other),
        }
    }
    components.iter().collect::<PathBuf>().to_string_lossy().to_string()
}

fn path_dirname(p: &str) -> String {
    Path::new(p).parent().map_or(".", |p| p.to_str().unwrap_or(".")).to_string()
}

fn path_basename(p: &str) -> String {
    Path::new(p).file_name().map_or("", |n| n.to_str().unwrap_or("")).to_string()
}

fn path_extname(p: &str) -> String {
    Path::new(p).extension().map_or(String::new(), |e| format!(".{}", e.to_string_lossy()))
}

fn path_is_absolute(p: &str) -> bool {
    Path::new(p).is_absolute()
}

fn path_resolve(from: &str, to: &str) -> String {
    let base = if Path::new(from).is_absolute() {
        PathBuf::from(from)
    } else {
        std::env::current_dir().unwrap_or_default().join(from)
    };
    base.join(to).to_string_lossy().to_string()
}

fn path_relative(from: &str, to: &str) -> String {
    // Compute relative path from `from` to `to`
    pathdiff::diff_paths(to, from)
        .map_or(to.to_string(), |p| p.to_string_lossy().to_string())
}

fn path_cwd() -> String {
    std::env::current_dir().unwrap_or_default().to_string_lossy().to_string()
}

fn path_sep() -> String { MAIN_SEPARATOR_STR.to_string() }
fn path_delimiter() -> String { if cfg!(windows) { ";".to_string() } else { ":".to_string() } }
```

**Note:** `path_relative` uses the `pathdiff` crate (lightweight, 0 deps) for correctness. Alternative: implement manually using path components.

---

## Module: std:codec

### Usage

```typescript
import { Utf8, Msgpack, Cbor, Protobuf } from "std:codec";

// ── UTF-8 ──
let bytes: Buffer = Utf8.encode("hello world");
let text: string = Utf8.decode(bytes);
let valid: boolean = Utf8.isValid(bytes);
let len: number = Utf8.byteLength("日本語");  // 9 (3 bytes per char)

// ── All binary codecs are type-safe with <T>, like JSON.decode<T>() ──
// Msgpack/CBOR reuse //@@json annotations for field name mapping
// Protobuf uses //@@proto annotations for field numbers

class Person {
    //@@json user_name
    //@@proto 1
    name: string;
    //@@json user_age
    //@@proto 2
    age: number;
    //@@proto 3
    active: boolean;
}

let person: Person = new Person();
person.name = "alice";
person.age = 30;
person.active = true;

// ── MessagePack (type-safe) ──
let packed: Buffer = Msgpack.encode<Person>(person);
let p1: Person = Msgpack.decode<Person>(packed);
// p1.name == "alice" — uses "user_name" as msgpack key (from //@@json)
let size: number = Msgpack.encodedSize(packed);

// ── CBOR (type-safe) ──
let cbor: Buffer = Cbor.encode<Person>(person);
let p2: Person = Cbor.decode<Person>(cbor);
let diag: string = Cbor.diagnostic(cbor);

// ── Protobuf (type-safe) ──
let proto: Buffer = Protobuf.encode<Person>(person);
let p3: Person = Protobuf.decode<Person>(proto);
// p3.name == "alice", p3.age == 30, p3.active == true

// Inline object types also work (like JSON.decode<{...}>):
let bytes2: Buffer = Msgpack.encode<{
    //@@json x_pos
    x: number;
    //@@json y_pos
    y: number;
}>({ x: 10, y: 20 });
```

**Design:** All binary codecs use the `<T>` + `//@@` annotation pattern from `JSON.decode<T>()`. The compiler extracts field metadata from the type parameter at compile time and passes it to the VM handler. **Msgpack/CBOR** reuse `//@@json` annotations for field name mapping (string keys in the binary format). **Protobuf** uses `//@@proto` annotations for field numbers (integer-keyed binary format). Named exports follow the `std:runtime` pattern (`import { Compiler, Bytecode } from "std:runtime"`).

### API

```typescript
class Utf8 {
    /** Encode string to UTF-8 bytes. */
    encode(text: string): Buffer;
    /** Decode UTF-8 bytes to string. Throws on invalid UTF-8. */
    decode(bytes: Buffer): string;
    /** Check if bytes are valid UTF-8. */
    isValid(bytes: Buffer): boolean;
    /** Get byte length of string when encoded as UTF-8. */
    byteLength(text: string): number;
}

class Msgpack {
    /** Encode typed object as MessagePack bytes.
     *  Compiler derives field names from <T> using //@@json annotations. */
    encode<T>(obj: T): Buffer;
    /** Decode MessagePack bytes to typed object.
     *  Compiler derives field names from <T> using //@@json annotations. */
    decode<T>(bytes: Buffer): T;
    /** Get byte size of a MessagePack buffer. */
    encodedSize(bytes: Buffer): number;
}

class Cbor {
    /** Encode typed object as CBOR bytes.
     *  Compiler derives field names from <T> using //@@json annotations. */
    encode<T>(obj: T): Buffer;
    /** Decode CBOR bytes to typed object.
     *  Compiler derives field names from <T> using //@@json annotations. */
    decode<T>(bytes: Buffer): T;
    /** Get CBOR diagnostic notation string (human-readable debug format). */
    diagnostic(bytes: Buffer): string;
}

class Protobuf {
    /** Encode typed object as Protobuf bytes.
     *  Compiler derives proto schema from <T> using //@@proto annotations.
     *  @param obj - Typed object to encode */
    encode<T>(obj: T): Buffer;
    /** Decode Protobuf bytes to typed object.
     *  Compiler derives proto schema from <T> using //@@proto annotations. */
    decode<T>(bytes: Buffer): T;
}

export { Utf8, Msgpack, Cbor, Protobuf };
```

**12 native calls across 4 classes, all native (binary format processing must happen in Rust).**

### Annotation: `//@@proto`

Follows the same pattern as `//@@json` — field-level metadata extracted at compile time.

```
//@@proto <field_number>[,<wire_hint>]
```

| Annotation | Meaning |
|-----------|---------|
| `//@@proto 1` | Field number 1, wire type inferred from Raya type |
| `//@@proto 2,int32` | Field number 2, use int32 instead of double |
| `//@@proto 3,float` | Field number 3, use float instead of double |
| `//@@proto 4,int64` | Field number 4, use int64 |
| `//@@proto -` | Skip field (not serialized) |

**Default wire type mapping (when no hint given):**

| Raya Type | Proto Type | Wire Type |
|-----------|-----------|-----------|
| `number` | `double` | Fixed64 (1) |
| `string` | `string` | Length-delimited (2) |
| `boolean` | `bool` | Varint (0) |
| `Buffer` | `bytes` | Length-delimited (2) |

### Compiler Integration

All three codecs follow the exact same pattern as `JSON.decode<T>()`:

1. **Parser** — `//@@json key_name` and `//@@proto 1` parsed as `Annotation` structs
2. **Lowerer** — `get_json_field_info(T)` for Msgpack/CBOR, `get_proto_field_info(T)` for Protobuf
3. **Code generation** — emits `NATIVE_CALL` with field metadata baked in as arguments

**Msgpack/CBOR** reuse `get_json_field_info()` (same string-key format as JSON):
```
NATIVE_CALL MSGPACK_ENCODE_OBJECT [obj, field_count, key_1, key_2, ...]
NATIVE_CALL MSGPACK_DECODE_OBJECT [buffer, field_count, key_1, key_2, ...]
NATIVE_CALL CBOR_ENCODE_OBJECT   [obj, field_count, key_1, key_2, ...]
NATIVE_CALL CBOR_DECODE_OBJECT   [buffer, field_count, key_1, key_2, ...]
```

Structurally identical to JSON's existing:
```
NATIVE_CALL JSON_DECODE_OBJECT [json, field_count, json_key_1, json_key_2, ...]
```

**Protobuf** uses `get_proto_field_info()` (integer field numbers + wire types):

```rust
struct ProtoFieldInfo {
    proto_field_number: u32,     // From //@@proto annotation
    field_name: String,          // Raya field name (for object field access)
    proto_type: ProtoType,       // Inferred from Raya type + optional hint
    optional: bool,
}

enum ProtoType {
    Double,   // number (default)
    Float,    // number with ,float hint
    Int32,    // number with ,int32 hint
    Int64,    // number with ,int64 hint
    Bool,     // boolean
    String,   // string
    Bytes,    // Buffer
}
```

```
NATIVE_CALL PROTO_ENCODE_OBJECT [obj, field_count, field_num_1, proto_type_1, field_num_2, proto_type_2, ...]
NATIVE_CALL PROTO_DECODE_OBJECT [buffer, field_count, field_num_1, proto_type_1, field_num_2, proto_type_2, ...]
```

### Native IDs (0x7000–0x70FF)

| ID | Constant | Method | Args | Return |
|----|----------|--------|------|--------|
| 0x7000 | `UTF8_ENCODE` | `Utf8.encode(text)` | string | Buffer |
| 0x7001 | `UTF8_DECODE` | `Utf8.decode(bytes)` | Buffer | string |
| 0x7002 | `UTF8_IS_VALID` | `Utf8.isValid(bytes)` | Buffer | boolean |
| 0x7003 | `UTF8_BYTE_LENGTH` | `Utf8.byteLength(text)` | string | number |
| 0x7010 | `MSGPACK_ENCODE_OBJECT` | `Msgpack.encode<T>(obj)` | obj, field_count, key_1, key_2, ... | Buffer |
| 0x7011 | `MSGPACK_DECODE_OBJECT` | `Msgpack.decode<T>(bytes)` | Buffer, field_count, key_1, key_2, ... | T (object) |
| 0x7012 | `MSGPACK_ENCODED_SIZE` | `Msgpack.encodedSize(bytes)` | Buffer | number |
| 0x7020 | `CBOR_ENCODE_OBJECT` | `Cbor.encode<T>(obj)` | obj, field_count, key_1, key_2, ... | Buffer |
| 0x7021 | `CBOR_DECODE_OBJECT` | `Cbor.decode<T>(bytes)` | Buffer, field_count, key_1, key_2, ... | T (object) |
| 0x7022 | `CBOR_DIAGNOSTIC` | `Cbor.diagnostic(bytes)` | Buffer | string |
| 0x7030 | `PROTO_ENCODE_OBJECT` | `Protobuf.encode<T>(obj)` | obj, field_count, field_nums..., proto_types... | Buffer |
| 0x7031 | `PROTO_DECODE_OBJECT` | `Protobuf.decode<T>(bytes)` | Buffer, field_count, field_nums..., proto_types... | T (object) |

### Rust Implementation

```rust
// ── Utf8 (no external crates) ──

fn utf8_encode(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

fn utf8_decode(bytes: &[u8]) -> Result<String, VmError> {
    String::from_utf8(bytes.to_vec())
        .map_err(|e| VmError::RuntimeError(format!("Invalid UTF-8: {}", e)))
}

fn utf8_is_valid(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok()
}

fn utf8_byte_length(text: &str) -> f64 {
    text.len() as f64
}

// ── Msgpack (rmp-serde) ──
// Type-safe: compiler passes field keys from <T> + //@@json annotations.
// VM handler builds serde_json::Value from object fields + keys, then serializes.

// MSGPACK_ENCODE_OBJECT handler:
//   args = [obj, field_count, key_1, key_2, ...]
//   1. Read field_count keys (from //@@json annotations, like JSON_DECODE_OBJECT)
//   2. For each field, read object field by index, build serde_json::Map { key: value }
//   3. Serialize map to msgpack via rmp_serde::to_vec
//   4. Return Buffer

// MSGPACK_DECODE_OBJECT handler:
//   args = [buffer, field_count, key_1, key_2, ...]
//   1. Deserialize msgpack to serde_json::Value via rmp_serde::from_slice
//   2. Extract fields by key name (same as JSON_DECODE_OBJECT)
//   3. Create typed object with field values populated
//   4. Return object

fn msgpack_encoded_size(bytes: &[u8]) -> f64 {
    bytes.len() as f64
}

// ── Cbor (ciborium) ──
// Type-safe: same pattern as Msgpack — compiler passes field keys from <T>.

// CBOR_ENCODE_OBJECT handler:
//   args = [obj, field_count, key_1, key_2, ...]
//   1. Build serde_json::Map from object fields + keys
//   2. Serialize via ciborium::into_writer
//   3. Return Buffer

// CBOR_DECODE_OBJECT handler:
//   args = [buffer, field_count, key_1, key_2, ...]
//   1. Deserialize CBOR to serde_json::Value
//   2. Extract fields by key name
//   3. Create typed object
//   4. Return object

fn cbor_diagnostic(bytes: &[u8]) -> Result<String, VmError> {
    let value: ciborium::Value = ciborium::from_reader(bytes)
        .map_err(|e| VmError::RuntimeError(format!("CBOR decode failed: {}", e)))?;
    Ok(format!("{:?}", value))  // Debug representation as diagnostic
}

// ── Protobuf (hand-rolled wire format — no external crate needed) ──
// Schema is compiler-derived from <T> + //@@proto annotations.
// VM handler receives field metadata as arguments (like JSON_DECODE_OBJECT).

/// Proto type codes passed from compiler (matches ProtoType enum)
const PROTO_TYPE_DOUBLE: i32 = 0;
const PROTO_TYPE_FLOAT: i32 = 1;
const PROTO_TYPE_INT32: i32 = 2;
const PROTO_TYPE_INT64: i32 = 3;
const PROTO_TYPE_BOOL: i32 = 4;
const PROTO_TYPE_STRING: i32 = 5;
const PROTO_TYPE_BYTES: i32 = 6;

/// Wire types
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

/// Encode a field tag (field_number << 3 | wire_type) as varint
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

/// Decode a varint from bytes, returns (value, bytes_consumed)
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

// PROTO_ENCODE_OBJECT handler:
//   args = [obj, field_count, field_num_1, proto_type_1, field_num_2, proto_type_2, ...]
//   1. Read field_count pairs of (field_number, proto_type)
//   2. For each field, read the object field by index, encode to protobuf wire format
//   3. Return Buffer

// PROTO_DECODE_OBJECT handler:
//   args = [buffer, field_count, field_num_1, proto_type_1, field_num_2, proto_type_2, ...]
//   1. Parse protobuf wire format
//   2. Match field numbers to expected fields
//   3. Create typed object with field values populated
//   4. Return object
```

---

## Phases

### Phase 1: Native IDs & Engine Infrastructure

**Status:** Complete

**Tasks:**
- [x] Define native IDs in `builtin.rs`
  - [x] Add `pub mod path { ... }` with IDs 0x6000-0x600A + `is_path_method()`
  - [x] Add `pub mod codec { ... }` with IDs 0x7000-0x7032 + `is_codec_method()`
- [x] Add corresponding constants in `native_id.rs`
  - [x] 11 `PATH_*` constants (0x6000-0x600A)
  - [x] 4 `UTF8_*` constants (0x7000-0x7003)
  - [x] 3 `MSGPACK_*` constants (0x7010-0x7012): `MSGPACK_ENCODE_OBJECT`, `MSGPACK_DECODE_OBJECT`, `MSGPACK_ENCODED_SIZE`
  - [x] 3 `CBOR_*` constants (0x7020-0x7022): `CBOR_ENCODE_OBJECT`, `CBOR_DECODE_OBJECT`, `CBOR_DIAGNOSTIC`
  - [x] 2 `PROTO_*` constants (0x7030-0x7031)
- [x] Add `native_name()` entries for all 23 IDs
- [x] Add crate dependencies to `crates/raya-engine/Cargo.toml`
  - [x] `rmp-serde` — MessagePack
  - [x] `ciborium` — CBOR
  - [x] `pathdiff` — relative path computation
  - [x] (No protobuf crate needed — wire format implemented directly from compiler-derived schema)

**Files:**
- `crates/raya-engine/src/vm/builtin.rs`
- `crates/raya-engine/src/compiler/native_id.rs`
- `crates/raya-engine/Cargo.toml`

---

### Phase 2: std:path — Raya Source & Engine Handler

**Status:** Complete

**Tasks:**
- [x] Create `crates/raya-stdlib/raya/path.raya`
  - [x] 11 native methods: join, normalize, dirname, basename, extname, isAbsolute, resolve, relative, cwd, sep, delimiter
  - [x] 3 pure Raya methods: stripExt, withExt, isRelative
  - [x] `export default path;`
  - Note: `from` is a reserved keyword — parameters renamed to `base`/`target`
- [x] Create `crates/raya-stdlib/raya/path.d.raya` — type declarations
- [x] Register `path.raya` in std module registry (`std_modules.rs`)
- [x] Create `crates/raya-engine/src/vm/vm/handlers/path.rs`
  - [x] `PathHandlerContext` with GC reference (for string allocation)
  - [x] `call_path_method()` with 11 match arms
  - [x] Helper functions: `get_string()`, `allocate_string()`
- [x] Register in `handlers/mod.rs`
- [x] Add dispatch in `task_interpreter.rs` at both native call sites
- [x] Add `call_path_method` bridge to TaskInterpreter impl

**Files:**
- `crates/raya-stdlib/raya/path.raya` (new)
- `crates/raya-stdlib/raya/path.d.raya` (new)
- `crates/raya-engine/src/compiler/module/std_modules.rs`
- `crates/raya-engine/src/vm/vm/handlers/path.rs` (new)
- `crates/raya-engine/src/vm/vm/handlers/mod.rs`
- `crates/raya-engine/src/vm/vm/task_interpreter.rs`

---

### Phase 3: std:codec — Raya Source & Engine Handler (Utf8, Msgpack, Cbor)

**Status:** Complete

**Tasks:**
- [x] Create `crates/raya-stdlib/raya/codec.raya`
  - [x] `Utf8Codec` class: 4 native methods (encode, decode, isValid, byteLength)
  - [x] `MsgpackCodec` class with `encode<T>`/`decode<T>` generic methods + encodedSize
  - [x] `CborCodec` class with `encode<T>`/`decode<T>` generic methods + diagnostic
  - [x] `TypeSchema` class (opaque handle for compile-time type metadata)
  - [x] Named exports: `export { Utf8, Msgpack, Cbor, Protobuf, TypeSchema };`
- [x] Create `crates/raya-stdlib/raya/codec.d.raya` — type declarations
- [x] Register `codec.raya` in std module registry (`std_modules.rs`)
- [x] Detect `Msgpack.encode<T>()` / `Msgpack.decode<T>()` in `compiler/lower/expr.rs`
  - [x] Reuse `get_json_field_info()` — same string-key pattern
  - [x] Emit `NATIVE_CALL MSGPACK_ENCODE_OBJECT / MSGPACK_DECODE_OBJECT`
- [x] Detect `Cbor.encode<T>()` / `Cbor.decode<T>()` in `compiler/lower/expr.rs`
  - [x] Reuse `get_json_field_info()` — same string-key pattern
  - [x] Emit `NATIVE_CALL CBOR_ENCODE_OBJECT / CBOR_DECODE_OBJECT`
- [x] Create `crates/raya-engine/src/vm/vm/handlers/codec.rs`
  - [x] `CodecHandlerContext` with GC reference (for Buffer/string allocation)
  - [x] `call_codec_method()` with 12 match arms (Utf8 4 + Msgpack encode/decode/encodedSize + CBOR encode/decode/diagnostic + Proto encode/decode)
  - [x] Msgpack encode: build serde_json::Map from object fields + keys → `rmp_serde::to_vec`
  - [x] Msgpack decode: `rmp_serde::from_slice` → extract fields by key → build typed object
  - [x] CBOR encode: build serde_json::Map → `ciborium::into_writer`
  - [x] CBOR decode: `ciborium::from_reader` → extract fields → build typed object
  - [x] Reuse `get_buffer_bytes()`, `allocate_buffer()`, `allocate_string()` patterns from crypto handler
- [x] Register in `handlers/mod.rs`
- [x] Add dispatch in `task_interpreter.rs` at both native call sites
- [x] Add `call_codec_method` bridge to TaskInterpreter impl
- [x] Fix `get_field_value()` to convert whole-number f64 to JSON integers (preserves int semantics through codec roundtrips)
- [x] Add field layout tracking for decoded object property access (register_object_fields → variable_object_fields → lower_member lookup)

**Files:**
- `crates/raya-stdlib/raya/codec.raya` (new)
- `crates/raya-stdlib/raya/codec.d.raya` (new)
- `crates/raya-engine/src/compiler/module/std_modules.rs`
- `crates/raya-engine/src/compiler/lower/expr.rs` (detect Msgpack/Cbor.encode/decode<T>)
- `crates/raya-engine/src/vm/vm/handlers/codec.rs` (new)
- `crates/raya-engine/src/vm/vm/handlers/mod.rs`
- `crates/raya-engine/src/vm/vm/task_interpreter.rs`

---

### Phase 4: std:codec — Protobuf Support (Type-Safe)

**Status:** Complete

**Tasks:**
- [x] Add `//@@proto` annotation support
  - [x] `Annotation::proto_field_number()` helper in `parser/ast/statement.rs`
  - [x] `Annotation::proto_wire_hint()` helper (parses optional `,int32`/`,float`/`,int64`)
- [x] Add `get_proto_field_info()` to `compiler/lower/mod.rs`
  - [x] Parallel to `get_json_field_info()` — extracts `ProtoFieldInfo` from type
  - [x] Reads `//@@proto` annotations for field numbers
  - [x] Infers proto wire type from Raya type + optional hint
  - [x] Supports inline object types (class reference support deferred, same as JSON)
- [x] Add `emit_proto_encode_with_fields()` to `compiler/lower/mod.rs`
  - [x] Generates `NATIVE_CALL PROTO_ENCODE_OBJECT [obj, count, field_num_1, type_1, ...]`
- [x] Add `emit_proto_decode_with_fields()` to `compiler/lower/mod.rs`
  - [x] Generates `NATIVE_CALL PROTO_DECODE_OBJECT [buf, count, field_num_1, type_1, ...]`
- [x] Detect `Protobuf.encode<T>()` / `Protobuf.decode<T>()` calls in `compiler/lower/expr.rs`
  - [x] Extract type arg, call `get_proto_field_info()`, emit specialized native call
- [x] Add `ProtobufCodec` class to `codec.raya` with `encode<T>`/`decode<T>` generic methods
- [x] Add 2 Protobuf match arms to `handlers/codec.rs`
  - [x] `PROTO_ENCODE_OBJECT`: read object fields by index, encode to protobuf wire format
  - [x] `PROTO_DECODE_OBJECT`: parse wire format, create typed object with fields
  - [x] Implement varint, fixed32/64, length-delimited encoding/decoding
- [x] Update `codec.d.raya`

**Files:**
- `crates/raya-engine/src/parser/ast/statement.rs` (add proto annotation helpers)
- `crates/raya-engine/src/compiler/lower/mod.rs` (add `get_proto_field_info`, emit helpers)
- `crates/raya-engine/src/compiler/lower/expr.rs` (detect Protobuf.encode/decode<T>)
- `crates/raya-engine/src/vm/vm/handlers/codec.rs`
- `crates/raya-stdlib/raya/codec.raya`
- `crates/raya-stdlib/raya/codec.d.raya`

---

### Phase 5: E2E Tests

**Status:** Complete (31 codec tests + 21 path tests = 52 total)

**Tasks:**
- [x] Update test harness `get_std_sources()` to include `path.raya` and `codec.raya`
- [x] Create `crates/raya-runtime/tests/e2e/path.rs` (21 tests: 19 passing, 2 ignored)
- [x] Create `crates/raya-runtime/tests/e2e/codec.rs` (31 tests passing)
- [x] Register `mod path;` and `mod codec;` in `crates/raya-runtime/tests/e2e/mod.rs`
- [x] Add Msgpack encode/decode tests (8 tests: roundtrip, string/number/bool fields, json annotation, encodedSize)
- [x] Add Cbor encode/decode tests (7 tests: roundtrip, string/number fields, json annotation, diagnostic)
- [x] Add Protobuf encode/decode tests (7 tests: roundtrip int32/string/bool/double, multiple fields, skip field, field order)

**Test Plan — std:path:**

| Test | What it verifies |
|------|-----------------|
| `test_path_import` | `import path from "std:path"` compiles and runs |
| `test_path_join_basic` | `path.join("a", "b")` == `"a/b"` |
| `test_path_join_absolute` | `path.join("/home", "docs")` == `"/home/docs"` |
| `test_path_dirname` | `path.dirname("/home/alice/file.txt")` == `"/home/alice"` |
| `test_path_dirname_no_parent` | `path.dirname("file.txt")` returns `.` or empty |
| `test_path_basename` | `path.basename("/home/alice/file.txt")` == `"file.txt"` |
| `test_path_extname` | `path.extname("file.txt")` == `".txt"` |
| `test_path_extname_none` | `path.extname("Makefile")` == `""` |
| `test_path_normalize` | `path.normalize("/foo/bar/../baz")` == `"/foo/baz"` |
| `test_path_normalize_dot` | `path.normalize("./foo/./bar")` == `"foo/bar"` |
| `test_path_is_absolute_true` | `path.isAbsolute("/foo")` == true |
| `test_path_is_absolute_false` | `path.isAbsolute("foo")` == false |
| `test_path_resolve` | `path.resolve("/base", "rel")` contains `"rel"` |
| `test_path_relative` | `path.relative("/a/b", "/a/b/c/d")` == `"c/d"` |
| `test_path_cwd` | `path.cwd()` returns a non-empty absolute path |
| `test_path_sep` | `path.sep()` returns `"/"` (on Unix) |
| `test_path_delimiter` | `path.delimiter()` returns `":"` (on Unix) |
| `test_path_strip_ext` | `path.stripExt("file.txt")` == `"file"` |
| `test_path_with_ext` | `path.withExt("file.txt", ".md")` == `"file.md"` |
| `test_path_is_relative` | `path.isRelative("foo/bar")` == true |
| `test_path_join_chain` | `path.join(path.join("a","b"),"c")` == `"a/b/c"` |

**Test Plan — std:codec:**

| Test | What it verifies |
|------|-----------------|
| `test_codec_import` | `import { Utf8, Msgpack, Cbor } from "std:codec"` compiles and runs |
| `test_utf8_encode` | `Utf8.encode("hi")` returns a Buffer |
| `test_utf8_roundtrip` | `Utf8.decode(Utf8.encode("hello"))` == `"hello"` |
| `test_utf8_is_valid` | `Utf8.isValid(Utf8.encode("hello"))` == true |
| `test_utf8_byte_length` | `Utf8.byteLength("abc")` == 3 |
| `test_utf8_byte_length_multibyte` | `Utf8.byteLength("日本")` == 6 |
| `test_msgpack_encode_decode` | `Msgpack.encode<T>(obj)` roundtrips via `Msgpack.decode<T>(bytes)` |
| `test_msgpack_string_field` | String field encodes/decodes correctly |
| `test_msgpack_number_field` | Number field encodes/decodes correctly |
| `test_msgpack_bool_field` | Boolean field encodes/decodes correctly |
| `test_msgpack_json_annotation` | `//@@json` annotation maps field name in msgpack |
| `test_msgpack_encoded_size` | `Msgpack.encodedSize(buf)` returns positive number |
| `test_cbor_encode_decode` | `Cbor.encode<T>(obj)` roundtrips via `Cbor.decode<T>(bytes)` |
| `test_cbor_string_field` | String field encodes/decodes correctly |
| `test_cbor_number_field` | Number field encodes/decodes correctly |
| `test_cbor_json_annotation` | `//@@json` annotation maps field name in CBOR |
| `test_cbor_diagnostic` | `Cbor.diagnostic(buf)` returns non-empty string |
| `test_proto_encode_decode` | `Protobuf.encode<T>(obj)` roundtrips via `Protobuf.decode<T>(bytes)` |
| `test_proto_string_field` | String field encodes/decodes correctly |
| `test_proto_number_field` | Number field (double) encodes/decodes correctly |
| `test_proto_bool_field` | Boolean field encodes/decodes correctly |
| `test_proto_multiple_fields` | Object with 3+ fields roundtrips correctly |
| `test_proto_skip_field` | `//@@proto -` field is not serialized |
| `test_proto_field_order` | Fields encode in field_number order regardless of declaration order |

**Files:**
- `crates/raya-runtime/tests/e2e/harness.rs`
- `crates/raya-runtime/tests/e2e/path.rs` (new)
- `crates/raya-runtime/tests/e2e/codec.rs` (new)
- `crates/raya-runtime/tests/e2e/mod.rs`

---

## Key Files Reference

| File | Purpose |
|------|---------|
| `crates/raya-stdlib/raya/path.raya` | Path class (11 native + 3 pure Raya) + `export default` |
| `crates/raya-stdlib/raya/path.d.raya` | Path type declarations |
| `crates/raya-stdlib/raya/codec.raya` | Utf8, Msgpack, Cbor, Protobuf classes (12 native) + named exports |
| `crates/raya-stdlib/raya/codec.d.raya` | Codec type declarations |
| `crates/raya-engine/src/vm/builtin.rs` | `pub mod path` + `pub mod codec` IDs |
| `crates/raya-engine/src/compiler/native_id.rs` | `PATH_*` + `CODEC_*` constants |
| `crates/raya-engine/src/vm/vm/handlers/path.rs` | Path handler (11 methods, GC context) |
| `crates/raya-engine/src/vm/vm/handlers/codec.rs` | Codec handler (12 methods, GC context) |
| `crates/raya-engine/src/parser/ast/statement.rs` | `//@@proto` annotation helpers |
| `crates/raya-engine/src/compiler/lower/mod.rs` | `get_proto_field_info()`, emit helpers |
| `crates/raya-engine/src/compiler/lower/expr.rs` | Msgpack/Cbor/Protobuf encode/decode<T> detection |
| `crates/raya-engine/src/vm/vm/handlers/mod.rs` | Register + re-export path and codec handlers |
| `crates/raya-engine/src/vm/vm/task_interpreter.rs` | VM dispatch at both native call sites |
| `crates/raya-engine/src/compiler/module/std_modules.rs` | Register both modules |
| `crates/raya-runtime/tests/e2e/path.rs` | Path E2E tests |
| `crates/raya-runtime/tests/e2e/codec.rs` | Codec E2E tests |

---

## Future Work (Deferred)

- **Class type references for all codecs** — `Msgpack.encode<Person>(obj)`, `Cbor.encode<Person>(obj)`, `Protobuf.encode<Person>(obj)` where `Person` is a class. Requires compiler support for resolving class field info from type references (same limitation as `JSON.decode<T>()` today — only inline object types supported in MVP).
- **Protobuf nested messages** — Encoding/decoding nested objects (field of type `T` where `T` is another proto-annotated class). Requires recursive field info extraction.
- **Protobuf repeated fields** — `number[]`, `string[]` → packed/unpacked repeated fields. Needs array wire type support.
- **Protobuf map fields** — `Map<string, number>` → proto map entries.
- **Protobuf oneof** — Discriminated union → proto oneof. Needs compiler mapping.
- **Protobuf well-known types** — Built-in support for `google.protobuf.Timestamp`, `Duration`, `Struct`, etc.
- **Protobuf external descriptors** — `Protobuf.encodeRaw(json, typeName, descriptorBytes)` for interop with external `.proto` schemas. Would use `prost-reflect`.
- **Streaming codecs** — Incremental encode/decode for large data (CBOR indefinite-length, msgpack streaming).
- **path.glob(pattern)** — Glob matching on paths. Requires `glob` crate.
- **path.exists(p) / path.isFile(p) / path.isDir(p)** — File system queries. Better suited for a future `std:fs` module.
