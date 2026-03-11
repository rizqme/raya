# raya-native

This proc-macro crate helps authors write native modules without hand-writing all ABI glue. It generates the wrapper layer that turns ordinary Rust functions into Raya-compatible native entrypoints.

## What This Crate Owns

- Attribute macros for native functions and modules.
- Generated wrapper code for argument/result conversion.
- Module initialization entrypoint generation.

## Layout

- `src/lib.rs`: macro entrypoints.
- `src/function.rs`: expansion for `#[function]`.
- `src/module.rs`: expansion for `#[module]`.
- `src/traits.rs`: internal support code.

## Start Here When

- Macro-generated wrapper behavior is wrong.
- Native module ergonomics need to improve.
- The generated ABI glue should match new `raya-sdk` capabilities.

## Read Next

- ABI types/macros target: [`../raya-sdk/CLAUDE.md`](../raya-sdk/CLAUDE.md)
- Engine-side loading/execution: [`../raya-engine/src/vm/ffi/CLAUDE.md`](../raya-engine/src/vm/ffi/CLAUDE.md)
