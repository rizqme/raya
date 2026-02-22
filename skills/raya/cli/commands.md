# CLI Commands

The `raya` CLI provides all toolchain operations in a single binary.

## Toolchain Commands

### raya run

Execute Raya programs (dual-mode: scripts or files).

```bash
# Run file
raya run app.raya
raya ./app.raya  # Implicit (no subcommand needed)

# Run bytecode
raya run app.ryb

# Run named script from raya.toml
raya run dev

# List available scripts
raya run --list

# Disable JIT
raya run --no-jit app.raya
```

### raya build

Compile to bytecode (.ryb).

```bash
# Compile single file
raya build app.raya -o app.ryb

# Compile project (from raya.toml)
raya build

# Verbose output
raya build --verbose
```

### raya check

Type-check without building.

```bash
# Check file
raya check app.raya

# Strict mode (all warnings as errors)
raya check --strict app.raya

# Allow specific warnings
raya check --allow unused-variable app.raya

# Deny specific warnings
raya check --deny shadowed-variable app.raya

# JSON output
raya check --format json app.raya

# No warnings
raya check --no-warnings app.raya
```

**Warning Codes:**
- `W1001` - unused-variable
- `W1002` - unused-import
- `W1003` - unused-parameter
- `W1004` - unreachable-code
- `W1005` - shadowed-variable

### raya eval

Evaluate inline expressions.

```bash
# Evaluate expression
raya eval "1 + 2"  # 3

# Evaluate function call
raya eval "math.sqrt(16)"  # 4 (with import)

# Complex expression
raya eval 'const x = 10; x * 2'  # 20
```

### raya repl

Interactive REPL (Read-Eval-Print Loop).

```bash
raya repl
```

**REPL Commands:**
- `help` - Show help
- `clear` - Clear session
- `load <file>` - Load file
- `type <expr>` - Show type
- `exit` - Exit REPL

**Features:**
- Multi-line input
- History (saved to `~/.raya/repl_history`)
- Syntax highlighting
- Declaration accumulation

### raya bundle

Compile to native bundle (requires `--features aot`).

```bash
# Compile to native
raya bundle app.raya -o app.bundle

# Run bundle
raya run app.bundle
```

### raya clean

Clear caches and build artifacts.

```bash
raya clean
```

**Removes:**
- `dist/` directory
- `.raya-cache/` directory

### raya info

Display environment information.

```bash
raya info
```

**Shows:**
- Raya version
- Rust version
- Platform (OS, arch)
- Install location
- Project info (if in project directory)

## Stub Commands (Planned)

- `raya test` - Run tests
- `raya bench` - Run benchmarks
- `raya fmt` - Format code
- `raya lint` - Lint code
- `raya doc` - Generate documentation
- `raya lsp` - Start Language Server
- `raya completions` - Generate shell completions

## Global Flags

```bash
--help, -h     Show help
--version, -V  Show version
--verbose, -v  Verbose output
--quiet, -q    Minimal output
```

## Examples

```bash
# Development workflow
raya check src/main.raya
raya build src/main.raya -o dist/main.ryb
raya run dist/main.ryb

# Quick iteration
raya run --no-jit app.raya  # Faster startup during development

# Production build
raya bundle app.raya -o app.bundle  # Native binary

# Debugging
raya eval 'import logger; logger.info("Debug:", someVar)'
```

## Related

- [Package Manager](package-manager.md) - Dependency management
- [REPL](repl.md) - Interactive shell details
