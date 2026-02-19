# raya-stdlib

Native implementations for Raya's standard library.

## Overview

This crate contains native (Rust) implementations of standard library functions that can't be efficiently implemented in pure Raya. Contains `StdNativeHandler` which implements the `NativeHandler` trait, plus a `register_stdlib()` function for name-based dispatch via `NativeFunctionRegistry`.

## Architecture

```
raya-sdk (defines NativeHandler trait, NativeContext, NativeValue, IoRequest/IoCompletion)
    ↓
raya-stdlib (implements StdNativeHandler + register_stdlib, depends on raya-sdk)
    ↓
raya-runtime (Runtime API: compile/load/execute/eval, uses StdNativeHandler, hosts e2e tests)
    ↓
raya-cli (CLI commands wired through Runtime)
```

## Module Structure

```
src/
├── lib.rs           # Crate entry, re-exports StdNativeHandler + register_stdlib
├── handler.rs       # StdNativeHandler (NativeHandler trait impl, ID-based dispatch)
├── registry.rs      # register_stdlib() (name-based handler registration)
├── logger.rs        # Logger: debug, info, warn, error
├── math.rs          # Math: abs, floor, ceil, sin, cos, sqrt, random, etc.
├── crypto.rs        # Crypto: hash, hmac, randomBytes, toHex, toBase64, etc.
├── path.rs          # Path: join, normalize, dirname, basename, resolve, etc.
└── stream.rs        # Stream: reactive stream implementation

raya/                # .raya source files and type definitions
├── logger.raya      # std:logger source (default export)
├── logger.d.raya    # Logger type definitions
├── math.raya        # std:math source
├── math.d.raya      # Math type definitions
├── crypto.raya      # std:crypto source
├── crypto.d.raya    # Crypto type definitions
├── time.raya        # std:time source (pure Raya + native calls)
├── time.d.raya      # Time type definitions
├── path.raya        # std:path source
├── path.d.raya      # Path type definitions
├── stream.raya      # std:stream source
├── stream.d.raya    # Stream type definitions
├── runtime.raya     # std:runtime source
├── runtime.d.raya   # Runtime type definitions
├── reflect.raya     # std:reflect source
└── reflect.d.raya   # Reflect type definitions
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
            _ => NativeCallResult::Unhandled,
        }
    }
}
```

## Implementation Status

| Module | Status | Native IDs | Notes |
|--------|--------|------------|-------|
| logger | Complete | 0x1000-0x1003 | Via NativeHandler (M4.2) |
| math | Complete | 0x2000-0x2016 | 22 functions + PI, E (M4.3) |
| crypto | Complete | 0x4000-0x400B | 12 methods (M4.6) |
| time | Complete | 0x5000-0x5004 | 5 native + 7 pure Raya; sleep via Suspend(BlockingWork) |
| path | Complete | 0x6000-0x600C | 14 methods (M4.8) |
| stream | In Progress | — | Reactive streams |
| runtime | Complete | 0x3000-0x30FF | Handlers in raya-engine (M4.5) |
| reflect | Type defs | 0x0D00-0x0E2F | Handlers in raya-engine |

## Dual Dispatch

Two dispatch mechanisms exist:
1. **ID-based** (`handler.rs`): `StdNativeHandler::call(id)` — used by `NativeCall` opcode
2. **Name-based** (`registry.rs`): `register_stdlib()` → `NativeFunctionRegistry` — used by `ModuleNativeCall` opcode

Both route to the same Rust implementations.

## Adding New Stdlib Modules

1. **Create `.raya` + `.d.raya`** in `crates/raya-stdlib/raya/`
2. **Define native IDs** in `raya-engine/src/vm/builtin.rs`
3. **Add to std registry** in `raya-engine/src/compiler/module/std_modules.rs`
4. **Implement Rust functions** in `crates/raya-stdlib/src/` (e.g., `mymodule.rs`)
5. **Route in `handler.rs`** — add match arm in `StdNativeHandler::call()`
6. **Register in `registry.rs`** — add name-based registration in `register_stdlib()`

## For AI Assistants

- **Architecture**: `raya-sdk` defines `NativeHandler` trait + IO types, stdlib implements it, runtime re-exports
- **No direct coupling**: `raya-engine` does NOT depend on `raya-stdlib`
- `time.sleep`/`time.sleepMicros` use `Suspend(BlockingWork)` — sleep runs on IO pool, not VM worker
- **Native IDs** must match across `builtin.rs`, `.raya` sources, and `StdNativeHandler`
- **std: prefix**: Standard library modules use `std:` namespace with default exports (e.g., `import math from "std:math"`)
- `NativeContext` provides GC allocation, class registry, and scheduler access
- `NativeValue` is type-safe (not string-based) — use `.as_f64()`, `.as_i32()`, `.as_string()`, etc.
- Keep native implementations simple — complex logic should be in Raya
