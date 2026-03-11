# raya-runtime

This crate is the high-level entrypoint for compiling, loading, and executing Raya programs. It is the layer most other crates should use instead of wiring parser/compiler/VM pieces together manually.

## What This Crate Owns

- The `Runtime` API used by CLI commands and tests.
- Builtin mode and type mode selection.
- Graph-first file compilation and diagnostics.
- Dependency loading from manifests, caches, and bytecode files.
- VM setup with stdlib/native bindings.
- Persistent sessions for REPL-style evaluation.
- AOT bundle loading helpers.

## Layout

- `src/lib.rs`: `Runtime`, `RuntimeOptions`, and most public API entrypoints.
- `src/compile.rs`: compile helpers and type-mode handling.
- `src/module_system/`: program graph resolution and merged-program compilation.
- `src/deps.rs`: dependency loading from manifests/caches.
- `src/loader.rs`: bytecode loading and dependency resolution.
- `src/session.rs`: persistent session state for REPL/eval flows.
- `src/vm_setup.rs`: VM creation and stdlib/native handler wiring.
- `src/bundle/`: AOT bundle support.
- `src/test_runner.rs`: runtime-backed test execution helpers.

## Start Here When

- The CLI or embedding layer needs a new runtime capability.
- Project/file execution behavior changes.
- Dependency resolution behavior is wrong at the runtime API layer.
- REPL/session behavior changes.

## Read Next

- Engine internals: [`../raya-engine/CLAUDE.md`](../raya-engine/CLAUDE.md)
- Stdlib bindings: [`../raya-stdlib/CLAUDE.md`](../raya-stdlib/CLAUDE.md) and [`../raya-stdlib-posix/CLAUDE.md`](../raya-stdlib-posix/CLAUDE.md)
- Package/dependency primitives: [`../raya-pm/CLAUDE.md`](../raya-pm/CLAUDE.md)
