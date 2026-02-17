# raya-runtime

Binds the Raya engine with the standard library via the `NativeHandler` trait. Hosts all e2e tests.

## Overview

This crate is a thin integration layer. `StdNativeHandler` is implemented in `raya-stdlib` and re-exported here for backward compatibility. All end-to-end tests live here since they require stdlib integration.

## Architecture

```
raya-engine (defines NativeHandler trait, NativeContext, NativeValue)
    ↓
raya-stdlib (implements StdNativeHandler + all stdlib modules)
    ↓
raya-runtime (re-exports StdNativeHandler, hosts e2e tests)
```

## Module Structure

```
src/
└── lib.rs              # Re-exports StdNativeHandler from raya-stdlib (7 lines)

tests/
├── e2e_tests.rs        # E2E test entry point
└── e2e/                # 27 test modules (883+ tests)
    ├── mod.rs           # Module declarations
    ├── harness.rs       # Test harness (compile + execute)
    ├── arrays.rs        # Array operations
    ├── async_await.rs   # Async/await concurrency
    ├── builtins.rs      # Built-in type methods
    ├── classes.rs       # Class features
    ├── closures.rs      # Closure semantics
    ├── closure_captures.rs # Capture-by-reference
    ├── concurrency.rs   # Task scheduling
    ├── conditionals.rs  # If/switch
    ├── crypto.rs        # std:crypto
    ├── decorators.rs    # Decorator system
    ├── exceptions.rs    # Try/catch/throw
    ├── fundamentals.rs  # Basic operations
    ├── functions.rs     # Function features
    ├── json.rs          # JSON operations
    ├── literals.rs      # Literal types
    ├── logger.rs        # std:logger
    ├── loops.rs         # Loop constructs
    ├── math.rs          # std:math
    ├── operators.rs     # Operator semantics
    ├── path.rs          # std:path
    ├── reflect.rs       # Reflect API
    ├── runtime.rs       # std:runtime (VM instances, compiler)
    ├── stream.rs        # std:stream
    ├── strings.rs       # String operations
    ├── time.rs          # std:time
    └── variables.rs     # Variable semantics
```

## Native ID Routing

Routing is handled by `StdNativeHandler` in `raya-stdlib/src/handler.rs`:

| Range | Module | Methods |
|-------|--------|---------|
| 0x1000-0x1003 | Logger | debug, info, warn, error |
| 0x2000-0x2016 | Math | abs, sign, floor, ceil, round, trunc, min, max, pow, sqrt, sin, cos, tan, asin, acos, atan, atan2, exp, log, log10, random, PI, E |
| 0x4000-0x400B | Crypto | hash, hashBytes, hmac, hmacBytes, randomBytes, randomInt, randomUUID, toHex, fromHex, toBase64, fromBase64, timingSafeEqual |
| 0x5000-0x5004 | Time | now, monotonic, hrtime, elapsed, sleep |
| 0x6000-0x600C | Path | join, normalize, dirname, basename, extname, isAbsolute, resolve, relative, cwd, sep, delimiter, stripExt, withExt |

## Tests

- **E2E tests** (883+): Full compilation + execution tests using `StdNativeHandler`
- **2 ignored**: Path tests using CallMethod in nested call context
- Tests moved from `raya-engine` in M4.2 to ensure stdlib integration
- Runtime tests (VM instance/spawn) need `--test-threads=2` due to thread contention

## For AI Assistants

- This is the primary crate for running Raya programs (engine + stdlib)
- E2E tests live here, NOT in raya-engine
- `StdNativeHandler` implementation lives in `raya-stdlib/src/handler.rs`, re-exported here
- When adding new stdlib modules, implement in `raya-stdlib`, route in `handler.rs`
- Run runtime tests with: `cargo test -p raya-runtime runtime -- --test-threads=2`
