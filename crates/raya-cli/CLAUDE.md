# raya-cli

Unified command-line interface for the Raya toolchain.

## Overview

This crate provides the `raya` CLI tool that combines all toolchain operations:
- Compilation and execution
- Testing and benchmarking
- Package management
- Code formatting and linting
- Documentation generation

## Commands

| Command | Description | Status |
|---------|-------------|--------|
| `raya run` | Run a Raya file | Stub |
| `raya build` | Compile to .ryb binary | Stub |
| `raya check` | Type-check without building | Stub |
| `raya test` | Run tests | Stub |
| `raya install` | Install dependencies | Stub |
| `raya add` | Add a dependency | Stub |
| `raya remove` | Remove a dependency | Stub |
| `raya update` | Update dependencies | Stub |
| `raya publish` | Publish to registry | Stub |
| `raya fmt` | Format code | Stub |
| `raya lint` | Lint code | Stub |
| `raya doc` | Generate documentation | Stub |
| `raya repl` | Interactive REPL | Stub |
| `raya bench` | Run benchmarks | Stub |
| `raya bundle` | Create standalone executable | Stub |
| `raya init` | Initialize new project | Stub |
| `raya create` | Create from template | Stub |
| `raya upgrade` | Upgrade Raya installation | Stub |

## Implementation Status

Currently all commands are stubs that print placeholder messages. The CLI structure is complete using `clap` for argument parsing.

## Key Files

- `src/main.rs`: CLI definition and command dispatch

## Implementation Priority

When implementing commands:
1. **`run`** - Most important, needed for basic development
2. **`build`** - Compile to .ryb binaries
3. **`check`** - Fast type-checking feedback
4. **`test`** - Testing infrastructure
5. **`init`/`create`** - Project scaffolding
6. **Package commands** - `install`, `add`, etc.

## Dependencies

- `raya-engine`: For compilation and execution
- `raya-pm`: For package management commands
- `clap`: CLI argument parsing
- `anyhow`: Error handling

## Example Usage

```bash
# Run a file
raya run main.raya

# Build to binary
raya build main.raya -o dist/

# Type-check
raya check src/

# Run tests
raya test --watch

# Add a package
raya add logging
```

## For AI Assistants

- All commands are currently stubs - implementation needed
- Reference `design/CLI.md` for complete specification
- Use `raya-engine` for actual compilation/execution
- Use `raya-pm` for package management operations
