# Raya

This repository implements the Raya language, its runtime, standard library, CLI, package tooling, and native extension surface.

## Start Here

If you do not yet know where a change belongs, use this rule of thumb:

- Source syntax, type checking, bytecode, interpreter semantics: go to [`crates/raya-engine/CLAUDE.md`](crates/raya-engine/CLAUDE.md).
- Running files, compiling projects, loading dependencies, REPL behavior: go to [`crates/raya-runtime/CLAUDE.md`](crates/raya-runtime/CLAUDE.md).
- Command-line flags and user-facing commands: go to [`crates/raya-cli/CLAUDE.md`](crates/raya-cli/CLAUDE.md).
- Standard library native behavior: go to [`crates/raya-stdlib/CLAUDE.md`](crates/raya-stdlib/CLAUDE.md) or [`crates/raya-stdlib-posix/CLAUDE.md`](crates/raya-stdlib-posix/CLAUDE.md).
- Manifest, lockfile, package cache, URL/package resolution: go to [`crates/raya-pm/CLAUDE.md`](crates/raya-pm/CLAUDE.md).
- Native module ABI or proc-macros: go to [`crates/raya-sdk/CLAUDE.md`](crates/raya-sdk/CLAUDE.md) and [`crates/raya-native/CLAUDE.md`](crates/raya-native/CLAUDE.md).

## Repository Shape

- `crates/raya-engine`: the language core. Parser, checker, compiler, VM, JIT/AOT live here.
- `crates/raya-runtime`: the high-level embedding API that glues engine + stdlib + dependency loading together.
- `crates/raya-cli`: the `raya` executable and command dispatch.
- `crates/raya-stdlib`: platform-independent native stdlib modules.
- `crates/raya-stdlib-posix`: OS and network dependent native stdlib modules.
- `crates/raya-pm`: manifests, lockfiles, semver, caches, and package-resolution primitives.
- `crates/raya-sdk`: ABI types and traits for native modules.
- `crates/raya-native`: proc-macros that generate native module wrappers.
- `crates/raya-examples`: sample apps and fixture projects used by integration tests.
- `crates/raya-lsp`: placeholder language-server crate.

## Common Navigation Paths

- New language feature: parser -> checker -> compiler -> VM.
- Import or package loading bug: CLI/runtime -> runtime module/deps loading -> engine module compiler/linker -> `raya-pm`.
- Native stdlib bug: runtime wiring -> stdlib crate -> engine native dispatch if ids or registry changed.
- Runtime crash or wrong execution result: runtime -> engine VM -> bytecode/compiler only if emitted code is suspect.

## What These Docs Are For

- Every leaf `CLAUDE.md` should explain what a folder owns and what nearby folders it depends on.
- Every parent `CLAUDE.md` should help an agent choose the correct child folder quickly.
- Keep these files structural and navigational. Do not turn them into milestone trackers or test-count logs.
