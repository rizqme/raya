# VM FFI

This folder is the runtime boundary between the engine and external native code. It covers both dynamically loaded native modules and the exported C ABI for embedding.

## What This Folder Owns

- Loading shared libraries that provide Raya native modules.
- Exposing `raya_vm_*` and related C ABI functions.
- Converting between engine `Value` and SDK `NativeValue`.
- Pinning/unpinning values when crossing the native boundary.

## File Guide

- `loader.rs`: shared-library loading and loader error handling.
- `c_api.rs`: exported C ABI symbols and wrapper structs.
- `native.rs`: engine-side registration and pin/unpin helpers.
- `mod.rs`: public re-exports and zero-cost conversion helpers.

## Start Here When

- Dynamic native libraries fail to load.
- A C embedding surface changes.
- Native value conversion, pinning, or ownership is wrong.

## Read Next

- ABI-facing types: [`../../../../raya-sdk/CLAUDE.md`](../../../../raya-sdk/CLAUDE.md)
- Proc-macro side of native modules: [`../../../../raya-native/CLAUDE.md`](../../../../raya-native/CLAUDE.md)
- VM runtime callers: [`../interpreter/CLAUDE.md`](../interpreter/CLAUDE.md)
