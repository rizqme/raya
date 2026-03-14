# raya-stdlib

This crate implements platform-independent native standard-library modules. It is where fast or engine-integrated std features live when they do not require direct OS APIs.

## What This Crate Owns

- Cross-platform native std modules such as logger, math, crypto, path, url, compression, encoding, semver, and templates.
- The stdlib dispatch layer used by the runtime.
- Raya wrapper source and type files for standard modules.

## Layout

- `src/handler.rs`: id-based stdlib dispatch for VM native call ids.
- `src/registry.rs`: name-based stdlib registration.
- `src/*.rs`: module implementations such as `logger`, `math`, `crypto`, `path`, `stream`, `url`, `compress`, `encoding`, `semver_mod`, `template`, `json_toml`, and `test`.
- `raya/`: `.raya` wrapper files for exported std modules.

## Start Here When

- A std module is platform-independent.
- Native ids or name-based stdlib registration need to change.
- Wrapper Raya sources or type definitions for std modules need to change.

## Read Next

- Shared ABI types: [`../raya-sdk/CLAUDE.md`](../raya-sdk/CLAUDE.md)
- Runtime wiring: [`../raya-runtime/CLAUDE.md`](../raya-runtime/CLAUDE.md)
- Reflection-backed std modules: [`../raya-engine/src/vm/reflect/CLAUDE.md`](../raya-engine/src/vm/reflect/CLAUDE.md)
