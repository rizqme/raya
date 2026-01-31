# Crates Overview

This directory contains all Rust crates that make up the Raya language implementation.

## Crate Hierarchy

```
crates/
├── raya-engine/     # Core language engine (parser, compiler, VM)
├── raya-cli/        # Command-line interface
├── raya-lsp/        # Language Server Protocol (LSP) server
├── raya-pm/         # Package manager
├── raya-sdk/        # Lightweight SDK for native modules
├── raya-native/     # Proc-macros for native module development
└── raya-stdlib/     # Standard library native implementations
```

## Crate Dependencies

```
raya-cli ──────────────────┬─> raya-engine (full runtime)
                           ├─> raya-pm (package management)
                           └─> raya-lsp (LSP server)

raya-engine ───────────────┬─> raya-sdk (FFI types)
                           └─> raya-stdlib (native stdlib)

raya-stdlib ───────────────┬─> raya-sdk (FFI types)
                           └─> raya-native (proc-macros)

Third-party native modules ┬─> raya-sdk (FFI types only)
                           └─> raya-native (proc-macros)
```

## Crate Purposes

| Crate | Purpose | Status |
|-------|---------|--------|
| `raya-engine` | Full language engine: parser, compiler, VM | Active development |
| `raya-cli` | `raya` CLI tool (run, build, test, etc.) | Scaffolded |
| `raya-lsp` | Language server for IDE support | Placeholder |
| `raya-pm` | Package manager (cache, resolution, manifests) | Partial |
| `raya-sdk` | Minimal types for native module FFI | Complete |
| `raya-native` | Proc-macros: `#[function]`, `#[module]` | Complete |
| `raya-stdlib` | Native implementations (console, JSON) | Partial |

## Key Design Decisions

1. **Consolidated Engine**: Parser, compiler, and VM are in one crate (`raya-engine`) for easier development and internal API changes.

2. **Minimal SDK**: `raya-sdk` contains only FFI types (`NativeValue`, `NativeModule`, traits). Third-party native modules only depend on this crate.

3. **Proc-Macro Separation**: `raya-native` is a separate proc-macro crate due to Rust's compilation model.

4. **Package Manager Independence**: `raya-pm` is separate so it can be used without the full engine.

## For AI Assistants

When working on Raya:
- **Most changes** go in `raya-engine` - it's the core of everything
- **CLI commands** go in `raya-cli` (currently stubs)
- **Native module development** uses `raya-sdk` + `raya-native`
- **Package resolution** logic is in `raya-pm`

See each crate's `CLAUDE.md` for detailed guidance.
