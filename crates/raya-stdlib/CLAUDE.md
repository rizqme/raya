# raya-stdlib

Cross-platform native implementations for Raya's standard library.

## Overview

This crate contains native (Rust) implementations of standard library functions that can't be efficiently implemented in pure Raya. Contains `StdNativeHandler` which implements the `NativeHandler` trait, plus a `register_stdlib()` function for name-based dispatch via `NativeFunctionRegistry`.

Platform-independent modules live here. OS-dependent modules (fs, net, http, process, etc.) live in `raya-stdlib-posix`.

## Architecture

```
raya-sdk (defines NativeHandler trait, NativeContext, NativeValue, IoRequest/IoCompletion)
    ↓
raya-stdlib (implements StdNativeHandler + register_stdlib, depends on raya-sdk)
raya-stdlib-posix (POSIX-specific natives, depends on raya-sdk)
    ↓
raya-runtime (Runtime API: compile/load/execute/eval, binds both stdlib crates)
    ↓
raya-cli (CLI commands wired through Runtime)
```

## Module Structure

```
src/
├── lib.rs           # Crate entry, re-exports StdNativeHandler + register_stdlib
├── handler.rs       # StdNativeHandler (NativeHandler trait impl, ID-based dispatch)
├── registry.rs      # register_stdlib() (name-based handler registration)
├── logger.rs        # Logger: debug/info/warn/error + level filtering, structured data, JSON format
├── math.rs          # Math: abs, floor, ceil, sin, cos, sqrt, random, etc.
├── crypto.rs        # Crypto: hash, hmac, randomBytes, toHex, toBase64, etc.
├── path.rs          # Path: join, normalize, dirname, basename, resolve, etc.
├── stream.rs        # Stream: reactive stream implementation
├── url.rs           # URL: WHATWG URL parsing, components, encoding, URLSearchParams, withX mutators
├── compress.rs      # Compress: gzip, deflate, zlib (flate2)
├── encoding.rs      # Encoding: hex, base32, base64url encode/decode
├── semver_mod.rs    # Semver: parse, compare, satisfies, increment, ranges
└── template.rs      # Template: simple string template engine with loops/conditionals

raya/                # .raya source files and type definitions (paired .raya + .d.raya)
├── logger.raya/d    # std:logger — Logger class (debug/info/warn/error + setLevel/getLevel/data variants/setFormat/setTimestamp/setPrefix)
├── math.raya/d      # std:math — Math functions + PI, E constants
├── crypto.raya/d    # std:crypto — Hashing, HMAC, random, hex/base64 encoding
├── time.raya/d      # std:time — Clocks, sleep, duration utilities (pure Raya + native)
├── path.raya/d      # std:path — Path manipulation (join, resolve, dirname, etc.)
├── stream.raya/d    # std:stream — Reactive streams
├── runtime.raya/d   # std:runtime — Compiler, Bytecode, Vm, Parser, TypeChecker, etc.
├── reflect.raya/d   # std:reflect — Reflection API (handlers in raya-engine)
├── url.raya/d       # std:url — Url, UrlSearchParams, UrlUtils (withX mutators, encodePath/decodePath)
├── compress.raya/d  # std:compress — Gzip, Deflate, Zlib compression
├── encoding.raya/d  # std:encoding — Hex, Base32, Base64url encoding
├── semver.raya/d    # std:semver — Semantic versioning (parse, compare, satisfies)
├── template.raya/d  # std:template — String template engine
└── args.raya/d      # std:args — Command-line argument parser (pure Raya)
```

## Key Types

### StdNativeHandler

```rust
// raya-stdlib/src/handler.rs
pub struct StdNativeHandler;

impl NativeHandler for StdNativeHandler {
    fn call(&self, ctx: &NativeContext, id: u16, args: &[NativeValue]) -> NativeCallResult {
        match id {
            0x1000..=0x1003 => /* Logger dispatch */,
            0x2000..=0x2016 => /* Math dispatch */,
            0x4000..=0x40FF => crate::crypto::call_crypto_method(ctx, id, args),
            0x5000..=0x5004 => /* Time dispatch */,
            0x6000..=0x60FF => crate::path::call_path_method(ctx, id, args),
            0x8000..=0x80FF => crate::compress::call_compress_method(ctx, id, args),
            0x9000..=0x90FF => crate::url::call_url_method(ctx, id, args),
            _ => NativeCallResult::Unhandled,
        }
    }
}
```

## Implementation Status

| Module | Status | Native IDs | Notes |
|--------|--------|------------|-------|
| logger | Complete | 0x1000-0x1003 | Level filtering, structured data, JSON format, timestamps, prefixes |
| math | Complete | 0x2000-0x2016 | 22 functions + PI, E |
| crypto | Complete | 0x4000-0x400B | 12 methods (hash, hmac, random, encoding) |
| time | Complete | 0x5000-0x5004 | 5 native + 7 pure Raya; sleep via Suspend(BlockingWork) |
| path | Complete | 0x6000-0x600C | 14 methods |
| stream | In Progress | — | Reactive streams |
| compress | Complete | 0x8000-0x8005 | gzip, deflate, zlib (flate2) |
| url | Complete | 0x9000-0x9026 | WHATWG URL parser, URLSearchParams, withX mutators, encodePath/decodePath |
| encoding | Complete | name-based | hex, base32, base64url encode/decode |
| semver | Complete | name-based | parse, compare, satisfies, increment, range checking |
| template | Complete | name-based | String template engine with loops/conditionals |
| args | Complete | pure Raya | CLI argument parser (no native code) |
| runtime | Complete | 0x3000-0x30FF | Handlers in raya-engine |
| reflect | Type defs | 0x0D00-0x0E2F | Handlers in raya-engine |

## Dual Dispatch

Two dispatch mechanisms exist:
1. **ID-based** (`handler.rs`): `StdNativeHandler::call(id)` — used by `NativeCall` opcode
2. **Name-based** (`registry.rs`): `register_stdlib()` → `NativeFunctionRegistry` — used by `ModuleNativeCall` opcode

Both route to the same Rust implementations. Newer modules (encoding, semver, template) use name-based dispatch only.

## Adding New Stdlib Modules

1. **Create `.raya` + `.d.raya`** in `crates/raya-stdlib/raya/`
2. **Define native IDs** in `raya-engine/src/vm/builtin.rs` (or use name-based only)
3. **Add to std registry** in `raya-engine/src/compiler/module/std_modules.rs`
4. **Implement Rust functions** in `crates/raya-stdlib/src/` (e.g., `mymodule.rs`)
5. **Route in `handler.rs`** — add match arm in `StdNativeHandler::call()` (if ID-based)
6. **Register in `registry.rs`** — add name-based registration in `register_stdlib()`

## For AI Assistants

- **Architecture**: `raya-sdk` defines `NativeHandler` trait + IO types, stdlib implements it, runtime re-exports
- **No direct coupling**: `raya-engine` does NOT depend on `raya-stdlib`
- **Cross-platform only**: OS-dependent modules go in `raya-stdlib-posix`, not here
- `time.sleep`/`time.sleepMicros` use `Suspend(BlockingWork)` — sleep runs on IO pool, not VM worker
- **Native IDs** must match across `builtin.rs`, `.raya` sources, and `StdNativeHandler`
- **std: prefix**: Standard library modules use `std:` namespace with default exports (e.g., `import math from "std:math"`)
- `NativeContext` provides GC allocation, class registry, and scheduler access
- `NativeValue` is type-safe (not string-based) — use `.as_f64()`, `.as_i32()`, `.as_string()`, etc.
- Keep native implementations simple — complex logic should be in Raya
