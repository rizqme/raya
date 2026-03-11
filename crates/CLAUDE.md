# Workspace Crates

This directory contains the Rust crates that make up the Raya toolchain. They are not peers in practice; they form a stack.

## Dependency Shape

Read the workspace roughly like this:

`raya-cli` -> `raya-runtime` -> `raya-engine`

`raya-runtime` also binds:

- `raya-stdlib`
- `raya-stdlib-posix`
- `raya-pm`

Native extension authors work mostly against:

- `raya-sdk`
- `raya-native`

## Crate Guide

- [`raya-engine/CLAUDE.md`](raya-engine/CLAUDE.md): source of truth for parsing, typing, compilation, bytecode, and VM behavior.
- [`raya-runtime/CLAUDE.md`](raya-runtime/CLAUDE.md): the main API for compiling files, resolving dependencies, creating VMs, and running code.
- [`raya-cli/CLAUDE.md`](raya-cli/CLAUDE.md): clap command tree and command implementations for the `raya` binary.
- [`raya-stdlib/CLAUDE.md`](raya-stdlib/CLAUDE.md): cross-platform native stdlib modules such as math, crypto, url, and path.
- [`raya-stdlib-posix/CLAUDE.md`](raya-stdlib-posix/CLAUDE.md): filesystem, process, networking, terminal, watch, and other OS-bound modules.
- [`raya-pm/CLAUDE.md`](raya-pm/CLAUDE.md): manifests, lockfiles, semver, package caches, and URL cache logic.
- [`raya-sdk/CLAUDE.md`](raya-sdk/CLAUDE.md): ABI-facing types and traits used by stdlib and third-party native modules.
- [`raya-native/CLAUDE.md`](raya-native/CLAUDE.md): proc-macros that generate glue code for native modules.
- [`raya-examples/CLAUDE.md`](raya-examples/CLAUDE.md): fixture apps and integration scenarios used by tests.
- [`raya-lsp/CLAUDE.md`](raya-lsp/CLAUDE.md): currently minimal; future LSP implementation lives there.

## How To Pick A Crate

- If a change affects the language semantics, start in `raya-engine`.
- If a change affects how users run, build, or test programs, start in `raya-runtime`.
- If a change is just command UX or flag wiring, start in `raya-cli`.
- If a native std module behaves incorrectly, go to the relevant stdlib crate.
- If a dependency path, lockfile, or cache is wrong, go to `raya-pm`.
