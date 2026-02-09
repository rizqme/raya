# raya-runtime

Binds the Raya engine with the standard library via the `NativeHandler` trait.

## Overview

This crate is the integration layer between `raya-engine` and `raya-stdlib`. It provides `StdNativeHandler`, which routes native call IDs to their Rust implementations. All end-to-end tests live here since they require stdlib integration.

## Architecture

```
raya-engine (defines NativeHandler trait)
    ↓
raya-runtime (StdNativeHandler routes native calls)
    ↓
raya-stdlib (logger, future modules)
```

## Module Structure

```
src/
└── lib.rs          # StdNativeHandler implementation

tests/
├── e2e_tests.rs    # E2E test harness (594 tests)
└── e2e/            # Individual .raya test files
```

## Key Types

### StdNativeHandler
```rust
pub struct StdNativeHandler;

impl NativeHandler for StdNativeHandler {
    fn call(&self, id: u16, args: &[String]) -> NativeCallResult {
        match id {
            0x1000 => { raya_stdlib::logger::debug(&msg); NativeCallResult::Void }
            0x1001 => { raya_stdlib::logger::info(&msg); NativeCallResult::Void }
            0x1002 => { raya_stdlib::logger::warn(&msg); NativeCallResult::Void }
            0x1003 => { raya_stdlib::logger::error(&msg); NativeCallResult::Void }
            _ => NativeCallResult::Unhandled,
        }
    }
}
```

## Native ID Routing

| Range | Module | Methods |
|-------|--------|---------|
| 0x1000-0x1003 | Logger | debug, info, warn, error |
| (future) | Math | abs, floor, ceil, PI, E, etc. |

## Tests

- **E2E tests** (594): Full compilation + execution tests using `StdNativeHandler`
- **Unit tests** (2): Handler routing verification
- Tests moved from `raya-engine` in M4.2 to ensure stdlib integration

## Adding a New Stdlib Module

1. Implement Rust functions in `raya-stdlib/src/`
2. Add native ID routing in `StdNativeHandler::call()`
3. Add e2e tests in `tests/e2e/`

## For AI Assistants

- This is the primary crate for running Raya programs (engine + stdlib)
- E2E tests live here, NOT in raya-engine
- When adding new stdlib modules, route their native IDs here
- `NativeCallResult::Unhandled` means the ID wasn't recognized (engine handles it)
