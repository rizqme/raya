# Crates Overview

This directory contains all Rust crates that make up the Raya language implementation.

## Crate Hierarchy

```
crates/
├── raya-engine/     # Core language engine (parser, compiler, VM)
├── raya-runtime/    # High-level Runtime API (compile, load, execute, eval, deps)
├── raya-stdlib/     # Standard library native implementations
├── raya-cli/        # Command-line interface
├── raya-lsp/        # Language Server Protocol (LSP) server
├── raya-pm/         # Package manager
├── raya-sdk/        # Lightweight SDK for native modules
└── raya-native/     # Proc-macros for native module development
```

## Crate Dependencies

```
raya-cli ──────────────────┬─> raya-runtime (engine + stdlib)
                           ├─> raya-pm (package management)
                           └─> raya-lsp (LSP server)

raya-runtime ─────────────┬─> raya-engine (core engine)
                           ├─> raya-stdlib (native implementations)
                           ├─> raya-stdlib-posix (POSIX natives)
                           └─> raya_pm (manifest parsing, URL cache)

raya-engine ───────────────┬─> (defines NativeHandler trait)
                           ├─> (no dependency on stdlib)
                           ├─> cranelift-* (optional, behind "jit" feature)
                           └─> cranelift-* (optional, behind "aot" feature)

raya-stdlib ───────────────┬─> raya-sdk (FFI types)
                           └─> raya-native (proc-macros)

Third-party native modules ┬─> raya-sdk (FFI types only)
                           └─> raya-native (proc-macros)
```

## Crate Purposes

| Crate | Purpose | Status |
|-------|---------|--------|
| `raya-engine` | Full language engine: parser, compiler, VM, JIT + AOT (feature-gated) | Active development |
| `raya-runtime` | High-level Runtime API + bundle format: compile, load, execute, eval, dependency resolution; hosts e2e tests | ✅ Complete (Runtime struct, 1,297 e2e + 15 bundle tests) |
| `raya-stdlib` | Native stdlib implementations + type defs | ✅ Logger, Math, Crypto, Time, Path complete |
| `raya-cli` | `raya` CLI tool (run, build, eval, pkg, etc.) | ✅ run/build/eval implemented (19 integration tests) |
| `raya-lsp` | Language server for IDE support | Placeholder |
| `raya-pm` | Package manager (cache, resolution, manifests, URL imports) | ✅ Complete |
| `raya-sdk` | Minimal types for native module FFI | ✅ Complete |
| `raya-native` | Proc-macros: `#[function]`, `#[module]` | ✅ Complete |

## Key Design Decisions

1. **Consolidated Engine**: Parser, compiler, and VM are in one crate (`raya-engine`) for easier development and internal API changes.

2. **Engine/Stdlib Decoupling** (M4.2): `raya-engine` defines a `NativeHandler` trait but does NOT depend on `raya-stdlib`. The `raya-runtime` crate binds them together via `StdNativeHandler` and exposes a high-level `Runtime` API for compile/load/execute/eval.

3. **Minimal SDK**: `raya-sdk` contains only FFI types (`NativeValue`, `NativeModule`, traits). Third-party native modules only depend on this crate.

4. **Proc-Macro Separation**: `raya-native` is a separate proc-macro crate due to Rust's compilation model.

5. **Package Manager Independence**: `raya-pm` is separate so it can be used without the full engine.

## For AI Assistants

When working on Raya:
- **Most changes** go in `raya-engine` - it's the core of everything
- **Stdlib implementations** go in `raya-stdlib` (contains `StdNativeHandler`), re-exported by `raya-runtime`
- **E2E tests** live in `raya-runtime` (moved from engine in M4.2)
- **CLI commands** go in `raya-cli/src/commands/` — run/build/eval are wired through `raya-runtime::Runtime`, pkg/clean/info also functional
- **Native module development** uses `raya-sdk` + `raya-native`
- **Package resolution** logic is in `raya-pm`

See each crate's `CLAUDE.md` for detailed guidance.
