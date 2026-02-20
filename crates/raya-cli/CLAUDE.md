# raya-cli

Unified command-line interface for the Raya toolchain.

## Overview

Single `raya` binary combining all toolchain operations. Built with clap derive.

**Key features:**
- Implicit file execution: `raya ./file.raya` (no `run` subcommand needed)
- Dual-mode `run`: named scripts from `[scripts]` in raya.toml OR direct file execution
- JIT enabled by default, `--no-jit` to disable
- `pkg` subcommand group is the canonical home for all package management
- Common PM commands aliased at top-level: `init`, `install`, `add`, `remove`, `update`, `publish`, `upgrade`

## Commands

### Toolchain Commands

| Command | Alias | Description | Status |
|---------|-------|-------------|--------|
| `raya run <target>` | `r` | Run script or file (dual-mode) | **Implemented** — compiles+executes .raya, loads .ryb, resolves deps |
| `raya build` | `b` | Compile to .ryb binary | **Implemented** — compiles .raya files to .ryb bytecode |
| `raya check` | `c` | Type-check without building | Stub |
| `raya eval <code>` | — | Evaluate inline expression | **Implemented** — evaluates expressions, wraps bare exprs automatically |
| `raya test` | `t` | Run tests | Stub |
| `raya bench` | — | Run benchmarks | Stub |
| `raya fmt` | — | Format source files | Stub |
| `raya lint` | — | Lint source files | Stub |
| `raya repl` | — | Interactive REPL | **Implemented** — persistent session, multi-line, history, dot-commands |
| `raya bundle` | — | AOT compile to native bundle | **Implemented** — requires `--features aot` |
| `raya doc` | — | Generate documentation | Stub |
| `raya lsp` | — | Start Language Server | Stub |
| `raya completions` | — | Generate shell completions | Stub |
| `raya clean` | — | Clear caches/artifacts | Implemented |
| `raya info` | — | Display environment info | Implemented |

### Package Management (`raya pkg` — canonical, with top-level aliases)

| Command | Alias | Description | Status |
|---------|-------|-------------|--------|
| `raya pkg init` | `raya init` | Initialize project | **Implemented** |
| `raya pkg install` | `raya install`, `raya i` | Install all dependencies | **Implemented** |
| `raya pkg add <pkg>` | `raya add`, `raya a` | Add dependency | **Implemented** |
| `raya pkg remove <pkg>` | `raya remove`, `raya rm` | Remove dependency | **Implemented** |
| `raya pkg update` | `raya update` | Update dependencies | Partial (full update works) |
| `raya pkg publish` | `raya publish` | Publish to registry | Stub |
| `raya pkg upgrade` | `raya upgrade` | Upgrade Raya installation | Stub |
| `raya pkg login` | — | Authenticate with registry | **Implemented** |
| `raya pkg logout` | — | Remove credentials | **Implemented** |
| `raya pkg set-url` | — | Set registry URL | **Implemented** |
| `raya pkg whoami` | — | Show current user | **Implemented** |
| `raya pkg info` | — | Show package info | Stub |

## Key Files

```
src/
├── main.rs              # CLI definition, implicit run detection, dispatch
└── commands/
    ├── mod.rs            # Module declarations
    ├── run.rs            # Dual-mode run (scripts + files)
    ├── pkg/
    │   ├── mod.rs        # PkgCommands enum (all PM commands) + dispatch
    │   ├── login.rs      # Registry authentication (~/.raya/credentials.toml)
    │   ├── logout.rs     # Remove credentials
    │   ├── set_url.rs    # Registry URL management (project + global)
    │   ├── whoami.rs     # Current user info
    │   └── info.rs       # Package info (stub)
    ├── repl.rs           # Interactive REPL (rustyline, Session, multi-line, dot-commands)
    ├── init.rs           # Project initialization (called by pkg dispatch)
    ├── install.rs        # Dependency installation (called by pkg dispatch)
    ├── add.rs            # Add dependency (called by pkg dispatch)
    ├── remove.rs         # Remove dependency (called by pkg dispatch)
    ├── update.rs         # Update dependencies (called by pkg dispatch)
    ├── publish.rs        # Publish to registry (stub)
    ├── upgrade.rs        # Self-update (stub)
    ├── bundle.rs         # AOT compilation to native bundle (feature-gated: "aot")
    ├── clean.rs          # Functional: deletes dist/, .raya-cache/
    ├── info.rs           # Functional: displays env/project info
    └── *.rs              # Other stubs (check, test, etc.)
```

## Run Command (Dual-Mode)

```bash
raya run dev           # Run script "dev" from [scripts] in raya.toml
raya run ./app.raya    # Run a file directly
raya ./app.raya        # Implicit run (no subcommand)
raya run --list        # List available scripts
```

Script vs file disambiguation: if target has `.raya`/`.ryb` extension or contains `/`/`\`/`.`, it's treated as a file path. Otherwise, it's looked up in `[scripts]`.

## Registry/Auth

- Credentials stored at `~/.raya/credentials.toml`
- Global config at `~/.raya/config.toml`
- Registry URL resolution: `RAYA_REGISTRY` env → project `[registry].url` → global config → default

## Dependencies

- `raya-runtime`: High-level Runtime API (compile, execute, eval, dependency resolution)
- `raya-engine`: Compilation and execution (used transitively via runtime)
- `raya_pm` (raya-pm): Package management, manifest parsing
- `clap`: CLI argument parsing (derive)
- `anyhow`: Error handling
- `toml`: Config file parsing
- `dirs`: Platform-specific directories (~/.raya)
- `rustyline`: Line editing, history, Ctrl-C/Ctrl-D for REPL

## Integration Tests

26 tests in `tests/cli_integration.rs` covering:
- Run .raya file, compile .raya file, run .ryb file, bytecode roundtrip
- Run with manifest (raya.toml), local path dependencies, URL deps (cached + error)
- Package dep resolution, mixed deps (ryb + source), .ryb with separate library
- Eval expressions, eval functions, eval complex expressions
- Script manifest parsing, script file targets
- Build to .ryb, runtime with options, runtime defaults
- Session: eval, variable persistence, function persistence, reset, format_value, multiple evals

13 unit tests in `src/commands/repl.rs` covering:
- `is_incomplete()`: braces, strings, comments, nesting, escapes
- `needs_wrapping()`: bare expressions vs declarations

## For AI Assistants

- `run`, `build`, `eval`, `repl` are fully wired through `raya-runtime::Runtime`/`Session`
- `repl.rs` uses `raya_runtime::Session` which accumulates declarations and re-compiles each eval
- REPL supports: dot-commands (.help, .clear, .load, .type, .exit), multi-line input, colored output, history (~/.raya/repl_history)
- `bundle` compiles to native via AOT pipeline (requires `--features aot`): compile → lift → Cranelift → bundle format
- `pkg` is the canonical PM namespace — all PM commands live in `PkgCommands` enum
- Top-level `init`, `install`, `add`, `remove`, `update`, `publish`, `upgrade` are aliases that delegate to the same implementations
- `pkg` registry subcommands (login/logout/set-url/whoami) are fully implemented
- `clean` and `info` are functional
- `run.rs` uses `Runtime::run_file()` which auto-detects .raya/.ryb and resolves deps from raya.toml
- `eval.rs` auto-wraps bare expressions in `return ...;`
- `build.rs` uses `Runtime::compile_file()` + `CompiledModule::encode()`
- `bundle.rs` uses `raya-engine::aot` pipeline + `raya-runtime::bundle::format` for output
- JIT is default-on at the CLI level; `--no-jit` flag disables it
- AOT feature: `[features] aot = ["raya-engine/aot"]` — forwards to engine
- Run CLI tests with: `cargo test -p raya-cli`
