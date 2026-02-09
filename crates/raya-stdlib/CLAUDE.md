# raya-stdlib

Native implementations for Raya's standard library.

## Overview

This crate contains native (Rust) implementations of standard library functions that can't be efficiently implemented in pure Raya. **Decoupled from raya-engine via the `NativeHandler` trait** — the runtime layer (`raya-runtime`) binds them together.

## Architecture (Post-M4.2)

```
raya-engine (defines NativeHandler trait)
    ↓
raya-stdlib (implements logger, future std modules)
    ↓
raya-runtime (StdNativeHandler routes calls)
```

## Module Structure

```
src/
├── lib.rs          # Crate entry point
└── logger.rs       # Logger implementations (debug, info, warn, error)

# Type definition & source files (.d.raya, .raya)
├── Logger.raya     # std:logger source (default export)
├── json.d.raya     # Type definitions for std:json
└── reflect.d.raya  # Type definitions for std:reflect
```

## Current Implementations

### Logger (`logger.rs`, `Logger.raya`)
- `logger.debug(msg)` - Print debug to stdout with `[DEBUG]` prefix
- `logger.info(msg)` - Print to stdout (no prefix)
- `logger.warn(msg)` - Print to stderr with `[WARN]` prefix
- `logger.error(msg)` - Print to stderr with `[ERROR]` prefix

**Usage:**
```typescript
import logger from "std:logger";

logger.info("Server started");
logger.error("Connection failed");
```

**Native IDs:** 0x1000-0x1003 (defined in `raya-engine/src/vm/builtin.rs`)

### JSON (Type defs only)
Type definitions in `json.d.raya` — implementation pending migration to new architecture.

### Reflect (`reflect.d.raya`)
Type definitions for the Reflection API (implementation in raya-engine):
- **Metadata**: `defineMetadata`, `getMetadata`, `hasMetadata`, `deleteMetadata`
- **Introspection**: `getClass`, `getFields`, `getMethods`, `getTypeInfo`
- **Dynamic access**: `get`, `set`, `invoke`, `construct`
- **Proxies**: `createProxy`, `isProxy`, `getProxyTarget`
- **Dynamic classes**: `createSubclass`, `defineClass`, `newClassBuilder`
- **Bytecode generation**: `newBytecodeBuilder`, `bcEmit*`, `bcBuild`
- **Permissions**: `setPermissions`, `getPermissions`, `sealPermissions`
- **Dynamic modules**: `createModule`, `moduleAddFunction`, `moduleSeal`
- **Bootstrap**: `bootstrap`, `isBootstrapped`, `getObjectClass`

Note: Reflect implementations use native call IDs (0x0Dxx-0x0Exx) handled in `raya-engine/src/vm/vm/handlers/reflect.rs`

## Integration with VM

**Post-M4.2:** Native functions are routed via the `NativeHandler` trait in `raya-runtime`:

```rust
// raya-runtime/src/lib.rs
impl NativeHandler for StdNativeHandler {
    fn call(&self, id: u16, args: &[String]) -> NativeCallResult {
        match id {
            0x1001 => {
                let msg = args.join(" ");
                raya_stdlib::logger::info(&msg);
                NativeCallResult::Void
            }
            // ...
        }
    }
}
```

The VM (`raya-engine`) remains decoupled from specific stdlib implementations.

## Adding New Stdlib Modules

1. **Create `.raya` source** in `crates/raya-stdlib/` (e.g., `Math.raya`)
2. **Define native IDs** in `raya-engine/src/vm/builtin.rs`
3. **Add to std registry** in `raya-engine/src/compiler/module/std_modules.rs`
4. **Implement Rust functions** in `crates/raya-stdlib/src/` (e.g., `math.rs`)
5. **Route in StdNativeHandler** in `raya-runtime/src/lib.rs`

## Implementation Status

| Module | Status | Notes |
|--------|--------|-------|
| logger | ✅ Complete | Via NativeHandler (M4.2) |
| JSON | Type defs only | Migration pending |
| reflect | ✅ Type defs complete | Handlers in raya-engine |
| math | Planned (M4.3) | abs, floor, ceil, PI, E, etc. |
| fs | Not started | |
| net | Not started | |
| crypto | Not started | |
| os | Not started | |

## For AI Assistants

- **Architecture**: Engine defines `NativeHandler` trait, stdlib implements functions, runtime binds them
- **No direct coupling**: `raya-engine` does NOT depend on `raya-stdlib`
- **Native IDs** must match across `builtin.rs`, `.raya` sources, and `StdNativeHandler`
- **std: prefix**: Standard library modules use `std:` namespace (e.g., `import logger from "std:logger"`)
- Keep implementations simple - complex logic should be in Raya
