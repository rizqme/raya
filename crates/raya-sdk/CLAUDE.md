# raya-sdk

This crate is the ABI-facing contract for native modules. It is intentionally smaller and more stable than the engine crate so stdlib code and third-party modules can depend on it without pulling in the whole VM implementation.

## What This Crate Owns

- `NativeValue` and related conversion traits.
- `NativeContext` abstraction for VM services exposed to native code.
- `NativeHandler` and name-based native function registries.
- Suspend/completion types for blocking or async-style native work.
- Wrapper types for arrays, objects, classes, and tasks.

## Layout

- `src/value.rs`: `NativeValue`.
- `src/context.rs`: `NativeContext` and related traits.
- `src/handler.rs`: handler traits, registries, suspend/completion types.
- `src/types.rs`: wrappers around object-like runtime values.
- `src/convert.rs`: Rust-object conversion helpers.
- `src/error.rs`: native/ABI error types.
- `src/lib.rs`: public re-exports and backward-compatible helpers.

## Start Here When

- Native handlers need new capabilities from the VM.
- Value conversion semantics need to change.
- Stdlib and third-party modules both need a shared ABI feature.

## Read Next

- Engine FFI boundary: [`../raya-engine/src/vm/ffi/CLAUDE.md`](../raya-engine/src/vm/ffi/CLAUDE.md)
- Macro-based authoring: [`../raya-native/CLAUDE.md`](../raya-native/CLAUDE.md)
- Stdlib consumers: [`../raya-stdlib/CLAUDE.md`](../raya-stdlib/CLAUDE.md)
