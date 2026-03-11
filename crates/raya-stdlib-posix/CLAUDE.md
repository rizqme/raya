# raya-stdlib-posix

This crate implements the parts of the standard library that need OS access: files, processes, sockets, HTTP, terminal IO, watchers, and similar platform services.

## What This Crate Owns

- OS-bound native std modules.
- Handle registries for long-lived native resources.
- Registration of name-based native functions for those modules.
- Raya wrapper source and type definitions that expose those native features to user code.

## Layout

- `src/registry.rs`: registration of exported native functions.
- `src/handles.rs`: handle registries for sockets, servers, watchers, subprocesses, and similar resources.
- `src/*.rs`: module implementations such as `fs`, `net`, `http`, `fetch`, `dns`, `process`, `terminal`, `watch`, `ws`, `readline`, and more.
- `raya/`: `.raya` and `.d.raya` wrapper/type files for std modules.

## Start Here When

- A std module needs OS or network access.
- Blocking work must be suspended onto the IO pool correctly.
- Resource-handle lifecycle is wrong.
- A POSIX-specific std wrapper needs to expose a new native capability.

## Read Next

- Shared ABI types: [`../raya-sdk/CLAUDE.md`](../raya-sdk/CLAUDE.md)
- Runtime wiring: [`../raya-runtime/CLAUDE.md`](../raya-runtime/CLAUDE.md)
- VM scheduler if blocking behavior is suspect: [`../raya-engine/src/vm/scheduler/CLAUDE.md`](../raya-engine/src/vm/scheduler/CLAUDE.md)
