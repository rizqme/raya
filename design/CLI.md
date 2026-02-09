# Raya CLI Design (v0.1)

**Single Unified CLI Tool**

Inspired by Bun, Raya uses a single `raya` command for all operations instead of separate tools like `rayac` (compiler) and `rpkg` (package manager).

---

## Table of Contents

1. [Philosophy](#philosophy)
2. [File Extensions](#file-extensions)
3. [Command Structure](#command-structure)
4. [Core Commands](#core-commands)
5. [Package Management](#package-management)
6. [Development Commands](#development-commands)
7. [Project Management](#project-management)
8. [Configuration](#configuration)
9. [Examples](#examples)

---

## Philosophy

### Single Entry Point

Instead of:
```bash
# ❌ Multiple separate tools
rayac run main.raya
rpkg install
rayafmt format src/
```

Use:
```bash
# ✅ Single unified CLI
raya run main.raya
raya install
raya fmt src/
```

### Benefits

1. **Simplicity** - One command to learn
2. **Consistency** - Uniform flag conventions
3. **Speed** - Shared initialization and caching
4. **UX** - Better autocomplete and help
5. **Distribution** - Single binary to install

---

## File Extensions

Raya uses specific file extensions for different artifact types:

### `.raya` - Source Files

Raya source code files. These are human-readable TypeScript-syntax files.

```typescript
// main.raya
function main(): void {
  logger.info("Hello, Raya!");
}
```

**Characteristics:**
- UTF-8 encoded text
- TypeScript syntax
- Can also use `.ts` extension for compatibility
- Compiled to bytecode before execution

### `.ryb` - Binary Files

Compiled Raya binary files. Unified format for both executables and libraries.

**Format:**
- Magic number: `RAYA` (0x52 0x41 0x59 0x41)
- Version number (u32)
- Constant pool
- Function definitions
- Class definitions
- Type metadata (mandatory)
- Export table

**Usage:**
```bash
# Compile to binary
raya build main.raya           # Creates dist/main.ryb

# Run binary directly (if it has main())
raya run dist/main.ryb

# Import as library (if it has exports)
import { function } from "./dist/mylib.ryb";
```

**Characteristics:**
- Binary format (not human-readable)
- Optimized for fast loading
- **Always includes reflection metadata** (not optional)
- Can be cached for faster execution
- Dual-purpose: executable or library depending on contents
- Binary with `main()` → can be executed
- Binary with exports → can be imported
- Can have both `main()` and exports (hybrid)

### Executable Bundles

Standalone executables with embedded runtime.

**Extensions:**
- **Linux/macOS:** No extension or `.elf`
- **Windows:** `.exe`
- **WebAssembly:** `.wasm`

**Creation:**
```bash
# Bundle to standalone executable
raya bundle main.raya -o myapp

# Cross-compile
raya bundle main.raya --target windows -o myapp.exe
```

**Characteristics:**
- Self-contained (includes Raya runtime)
- No dependencies required
- Single-file distribution
- Optimized and stripped

---

## Command Structure

```
raya [OPTIONS] <COMMAND> [ARGS]
```

### Global Options

```
--version, -v       Show version information
--help, -h         Show help message
--quiet, -q        Suppress output
--verbose          Show detailed output
--color <when>     Colorize output (auto/always/never)
--config <path>    Use custom config file
```

---

## Core Commands

### `raya run`

Execute a Raya file.

```bash
raya run <file> [args...]
```

**Examples:**
```bash
raya run main.raya
raya run src/server.raya --port 3000
raya run example.raya arg1 arg2
```

**Options:**
```
--watch, -w        Watch for changes and reload
--inspect          Enable debugger
--inspect-brk      Enable debugger and break at start
--no-cache         Disable bytecode cache
```

**Behavior:**
- Compiles to bytecode (cached)
- Executes immediately
- Passes remaining args to program

### `raya build`

Compile Raya source to bytecode.

```bash
raya build [files...] [OPTIONS]
```

**Examples:**
```bash
raya build                    # Build entire project
raya build src/              # Build directory
raya build main.raya         # Build single file
raya build --release         # Optimized build
```

**Options:**
```
--out-dir <dir>      Output directory (default: dist/)
--release            Optimized build with full optimization
--target <name>      Build target (native/wasm/bytecode)
--watch, -w          Watch for changes
```

**Output:**
- `.ryb` binary files (with mandatory reflection metadata)
- Dependency graph
- Debug source mapping (in debug builds)

### `raya check`

Type-check without building.

```bash
raya check [files...]
```

**Examples:**
```bash
raya check                    # Check entire project
raya check src/              # Check directory
raya check main.raya         # Check single file
```

**Options:**
```
--watch, -w          Watch for changes
--strict             Enable strict mode
```

**Output:**
- Type errors
- Warnings
- Suggestions

### `raya test`

Run tests.

```bash
raya test [pattern] [OPTIONS]
```

**Examples:**
```bash
raya test                     # Run all tests
raya test user                # Run tests matching "user"
raya test src/auth/          # Run tests in directory
raya test --watch            # Watch mode
```

**Options:**
```
--watch, -w          Watch for changes
--coverage           Generate coverage report
--bail               Stop after first failure
--timeout <ms>       Test timeout (default: 5000)
--concurrency <n>    Max parallel tests (default: CPU cores)
```

**Test Discovery:**
- Files matching `*.test.raya`
- Files matching `*.spec.raya`
- `__tests__/` directories

---

## Package Management

### `raya install`

Install dependencies.

```bash
raya install [package] [OPTIONS]
```

**Examples:**
```bash
raya install                  # Install all dependencies
raya install lodash          # Install specific package
raya install lodash@4.17.0   # Install specific version
```

**Options:**
```
--save, -S           Save to dependencies (default)
--save-dev, -D       Save to devDependencies
--global, -g         Install globally
--frozen             Don't update lock file
```

### `raya add`

Add a dependency (alias for `raya install <package>`).

```bash
raya add <package> [OPTIONS]
```

**Examples:**
```bash
raya add lodash
raya add -D prettier
raya add @types/node@18
```

### `raya remove`

Remove a dependency.

```bash
raya remove <package>
```

**Examples:**
```bash
raya remove lodash
raya remove -D prettier
```

### `raya update`

Update dependencies.

```bash
raya update [package]
```

**Examples:**
```bash
raya update                   # Update all
raya update lodash           # Update specific package
```

### `raya publish`

Publish package to registry.

```bash
raya publish [OPTIONS]
```

**Options:**
```
--tag <tag>          Publish with tag (default: latest)
--access <level>     public or restricted
--dry-run            Show what would be published
```

---

## Development Commands

### `raya fmt`

Format code.

```bash
raya fmt [files...] [OPTIONS]
```

**Examples:**
```bash
raya fmt                      # Format all files
raya fmt src/                # Format directory
raya fmt main.raya           # Format single file
raya fmt --check             # Check formatting
```

**Options:**
```
--check              Check if files are formatted
--write, -w          Write changes (default)
--config <path>      Custom config file
```

### `raya lint`

Lint code.

```bash
raya lint [files...] [OPTIONS]
```

**Examples:**
```bash
raya lint                     # Lint all files
raya lint src/               # Lint directory
raya lint --fix              # Auto-fix issues
```

**Options:**
```
--fix                Auto-fix issues
--watch, -w          Watch mode
```

### `raya doc`

Generate documentation.

```bash
raya doc [OPTIONS]
```

**Examples:**
```bash
raya doc                      # Generate docs
raya doc --serve             # Generate and serve
```

**Options:**
```
--out-dir <dir>      Output directory (default: docs/)
--serve              Start documentation server
--open               Open in browser
```

### `raya repl`

Start interactive REPL.

```bash
raya repl
```

**Features:**
- Multi-line input
- Tab completion
- History
- Type information on hover
- Import modules

### `raya bench`

Run benchmarks.

```bash
raya bench [pattern] [OPTIONS]
```

**Examples:**
```bash
raya bench                    # Run all benchmarks
raya bench sort              # Run matching benchmarks
```

**Options:**
```
--save <file>        Save results
--compare <file>     Compare with saved results
```

### `raya bundle`

Create standalone executable with embedded runtime.

```bash
raya bundle <file> [OPTIONS]
```

**Examples:**
```bash
# Create native executable
raya bundle main.raya -o myapp

# Create for specific platform
raya bundle main.raya --target windows -o myapp.exe
raya bundle main.raya --target linux -o myapp
raya bundle main.raya --target macos -o myapp

# Create WebAssembly bundle
raya bundle main.raya --target wasm -o myapp.wasm

# Optimized bundle
raya bundle main.raya --release -o myapp
```

**Options:**
```
-o, --output <file>     Output file path
--target <platform>     Target platform (native/windows/linux/macos/wasm)
--release               Optimized release build
--strip                 Strip debug symbols
--compress              Compress executable (UPX-style)
--icon <file>           Application icon (Windows/macOS)
--no-runtime            Don't embed runtime (requires Raya installed)
```

**Targets:**
- `native` - Current platform (default)
- `windows` - Windows x64 (.exe)
- `linux` - Linux x64 (ELF)
- `macos` - macOS universal binary
- `wasm` - WebAssembly (.wasm)

**Bundle Structure:**
```
Executable:
├── Raya VM Runtime (embedded)
├── Compiled binary (.ryb with main())
├── Dependencies (.ryb libraries)
└── Metadata
```

**Characteristics:**
- **Self-contained** - No Raya installation required
- **Single file** - Easy distribution
- **Optimized** - Dead code elimination
- **Cross-platform** - Build for any target from any host
- **Reflection included** - All type metadata embedded

**Size:**
- Minimal bundle: ~2.5-5.5 MB (stripped, includes reflection)
- With full stdlib: ~5.5-10.5 MB
- Compressed: ~1.5-3.5 MB

---

## Project Management

### `raya init`

Initialize a new project.

```bash
raya init [name] [OPTIONS]
```

**Examples:**
```bash
raya init                     # Init in current directory
raya init my-app             # Create new directory
raya init --template web     # Use template
```

**Options:**
```
--template <name>    Use project template
--yes, -y            Skip prompts
```

**Templates:**
- `basic` - Basic project
- `web` - Web application
- `api` - API server
- `lib` - Library package
- `cli` - CLI application

### `raya create`

Create from template (alias for `raya init`).

```bash
raya create <name>
```

### `raya upgrade`

Upgrade Raya installation.

```bash
raya upgrade [version]
```

**Examples:**
```bash
raya upgrade                  # Upgrade to latest
raya upgrade 1.2.0           # Upgrade to specific version
```

---

## Configuration

### `raya.toml`

Project configuration file.

```toml
[package]
name = "my-app"
version = "1.0.0"
authors = ["Your Name <you@example.com>"]
license = "MIT"

[dependencies]
lodash = "4.17.21"

[dev-dependencies]
prettier = "^3.0.0"

[build]
target = "native"

[test]
timeout = 5000
coverage = true

[fmt]
line-width = 100
tab-width = 2
```

### Environment Variables

```bash
RAYA_LOG=debug           # Logging level
RAYA_CACHE_DIR=/tmp      # Cache directory
RAYA_NUM_THREADS=8       # VM worker threads
RAYA_REGISTRY=...        # Custom registry URL
```

---

## Examples

### Complete Workflow

```bash
# Create new project
raya init my-app
cd my-app

# Add dependencies
raya add lodash
raya add -D prettier

# Run in dev mode
raya run src/main.raya

# Run tests
raya test --watch

# Format code
raya fmt

# Type check
raya check

# Build for production
raya build --release

# Run benchmarks
raya bench

# Publish package
raya publish
```

### Development Server

```bash
# Run with auto-reload
raya run --watch src/server.raya

# Run with debugger
raya run --inspect src/server.raya
```

### CI/CD

```bash
# Install dependencies
raya install --frozen

# Type check
raya check

# Run tests with coverage
raya test --coverage --bail

# Build release
raya build --release
```

---

## Command Aliases

Some commands have shorter aliases for convenience:

| Full Command | Alias | Example |
|-------------|-------|---------|
| `raya install` | `raya i` | `raya i lodash` |
| `raya add` | `raya a` | `raya a lodash` |
| `raya remove` | `raya rm` | `raya rm lodash` |
| `raya test` | `raya t` | `raya t --watch` |
| `raya build` | `raya b` | `raya b --release` |

---

## Help System

### Get Help

```bash
raya --help              # General help
raya run --help         # Command-specific help
raya help run           # Alternative syntax
```

### Auto-completion

Install shell completions:

```bash
raya completions bash > /etc/bash_completion.d/raya
raya completions zsh > ~/.zsh/completion/_raya
raya completions fish > ~/.config/fish/completions/raya.fish
```

---

## Comparison with Other Tools

### vs. Separate Tools

| Traditional | Raya |
|------------|------|
| `rayac run` | `raya run` |
| `rayac build` | `raya build` |
| `rpkg install` | `raya install` |
| `rpkg add` | `raya add` |
| `rayafmt format` | `raya fmt` |
| `rayalint check` | `raya lint` |

### vs. Node.js Ecosystem

| Node.js | Raya |
|---------|------|
| `node file.js` | `raya run file.raya` |
| `npm install` | `raya install` |
| `npm run build` | `raya build` |
| `npm test` | `raya test` |
| `npx prettier` | `raya fmt` |

### vs. Bun

| Bun | Raya | Notes |
|-----|------|-------|
| `bun run` | `raya run` | Execute files |
| `bun install` | `raya install` | Install deps |
| `bun test` | `raya test` | Run tests |
| `bun build` | `raya build` | Bundle/compile |
| `bun fmt` | `raya fmt` | Format code |

---

## Implementation Notes

### Binary Structure

```rust
// crates/raya-cli/src/main.rs
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "raya")]
#[command(about = "Raya programming language toolchain")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run(RunCommand),
    Build(BuildCommand),
    Check(CheckCommand),
    Test(TestCommand),
    Install(InstallCommand),
    Add(AddCommand),
    Remove(RemoveCommand),
    // ... etc
}
```

### Shared Context

The unified CLI allows sharing:
- **Cache** - Compiled bytecode, type information
- **Configuration** - Single config file
- **State** - Lock files, dependency graph
- **Performance** - JIT compilation of CLI itself

### Fast Startup

- Lazy loading of subcommands
- Incremental compilation
- Persistent cache
- Optimized binary (release profile)

---

## Migration from Separate Tools

### Deprecation Path

1. **Phase 1** - Both `raya` and old tools work
2. **Phase 2** - Old tools show deprecation warning
3. **Phase 3** - Old tools redirect to `raya`
4. **Phase 4** - Old tools removed

### Compatibility

Old commands can be supported via symlinks:

```bash
ln -s raya /usr/local/bin/rayac
ln -s raya /usr/local/bin/rpkg
```

Then detect invocation name:
```rust
match std::env::args().next().unwrap().as_str() {
    "rayac" => handle_compiler_mode(),
    "rpkg" => handle_package_manager_mode(),
    "raya" => handle_unified_mode(),
}
```

---

## Future Extensions

### Potential Commands

- `raya debug` - Start debugger
- `raya profile` - Profile performance
- `raya outdated` - Check for outdated dependencies
- `raya audit` - Security audit
- `raya exec` - Execute binary without installation
- `raya cache` - Manage cache
- `raya config` - Manage configuration

### Plugin System

```bash
raya plugin install raya-deploy
raya deploy --target production
```

---

## Design Rationale

### Why Single CLI?

1. **Developer Experience**
   - One tool to install
   - One command to remember
   - Consistent interface

2. **Performance**
   - Shared initialization
   - Unified caching
   - Faster startup

3. **Maintainability**
   - Single codebase
   - Consistent patterns
   - Easier testing

4. **Distribution**
   - Single binary
   - Simpler installation
   - Reduced confusion

### Inspiration from Bun

Bun demonstrated that a unified CLI:
- Reduces cognitive load
- Improves discoverability
- Creates cohesive ecosystem
- Feels more "batteries included"

---

**Status:** Partially Implemented
**Version:** v0.3
**Last Updated:** 2026-02-01

**Key Changes in v0.3:**
- Package management commands now implemented (init, install, add, remove, update, new)
- Uses rpkg library integrated into raya-cli
- Registry client with HTTP API support

**Key Changes in v0.2:**
- Updated all `.rbc` references to `.ryb`
- Removed `.rlib` format (now uses `.ryb` for libraries)
- Removed `--emit-reflection` flag (reflection is always included)
- Removed `--lib` flag (binary type determined by contents)
- Updated size estimates to account for mandatory reflection
