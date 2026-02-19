# raya-cli

Unified command-line interface for the Raya toolchain.

## Overview

Single `raya` binary combining all toolchain operations. Built with clap derive.

**Key features:**
- Implicit file execution: `raya ./file.raya` (no `run` subcommand needed)
- Dual-mode `run`: named scripts from `[scripts]` in raya.toml OR direct file execution
- JIT enabled by default, `--no-jit` to disable
- `pkg` subcommand group for registry/auth management

## Commands

| Command | Alias | Description | Status |
|---------|-------|-------------|--------|
| `raya run <target>` | `r` | Run script or file (dual-mode) | Scaffolded (script resolution works, execution pipeline TODO) |
| `raya build` | `b` | Compile to .ryb binary | Stub |
| `raya check` | `c` | Type-check without building | Stub |
| `raya eval <code>` | — | Evaluate inline expression | Stub |
| `raya test` | `t` | Run tests | Stub |
| `raya bench` | — | Run benchmarks | Stub |
| `raya fmt` | — | Format source files | Stub |
| `raya lint` | — | Lint source files | Stub |
| `raya repl` | — | Interactive REPL | Stub |
| `raya init` | — | Initialize project | Stub |
| `raya new <name>` | — | Create new project | Stub |
| `raya add <pkg>` | `a` | Add dependency | Stub |
| `raya remove <pkg>` | `rm` | Remove dependency | Stub |
| `raya install` | `i` | Install all dependencies | Stub |
| `raya update` | — | Update dependencies | Stub |
| `raya publish` | — | Publish to registry | Stub |
| `raya pkg login` | — | Authenticate with registry | Implemented |
| `raya pkg logout` | — | Remove credentials | Implemented |
| `raya pkg set-url` | — | Set registry URL | Implemented |
| `raya pkg whoami` | — | Show current user | Implemented |
| `raya pkg info` | — | Show package info | Stub |
| `raya bundle` | — | Create standalone executable | Stub |
| `raya doc` | — | Generate documentation | Stub |
| `raya lsp` | — | Start Language Server | Stub |
| `raya completions` | — | Generate shell completions | Stub |
| `raya clean` | — | Clear caches/artifacts | Implemented |
| `raya info` | — | Display environment info | Implemented |
| `raya upgrade` | — | Upgrade Raya installation | Stub |

## Key Files

```
src/
├── main.rs              # CLI definition, implicit run detection, dispatch
└── commands/
    ├── mod.rs            # Module declarations
    ├── run.rs            # Dual-mode run (scripts + files)
    ├── pkg/
    │   ├── mod.rs        # PkgCommands enum + dispatch
    │   ├── login.rs      # Registry authentication (~/.raya/credentials.toml)
    │   ├── logout.rs     # Remove credentials
    │   ├── set_url.rs    # Registry URL management (project + global)
    │   ├── whoami.rs     # Current user info
    │   └── info.rs       # Package info (stub)
    ├── clean.rs          # Functional: deletes dist/, .raya-cache/
    ├── info.rs           # Functional: displays env/project info
    └── *.rs              # Other stubs (build, check, test, etc.)
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

- `raya-engine`: Compilation and execution
- `rpkg` (raya-pm): Package management
- `clap`: CLI argument parsing (derive)
- `anyhow`: Error handling
- `toml`: Config file parsing
- `dirs`: Platform-specific directories (~/.raya)

## For AI Assistants

- Most commands are stubs — execution pipeline in `run.rs` needs wiring (Parse → Check → Compile → `Vm::execute`)
- `pkg` subcommands (login/logout/set-url/whoami) are fully implemented
- `clean` and `info` are functional
- Use `StdNativeHandler` from raya-stdlib when wiring execution
- JIT is default-on at the CLI level; `--no-jit` flag disables it
