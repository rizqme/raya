# Crate Structure

Raya is organized into 8 crates with clear separation of concerns.

## Dependency Graph

```
raya-sdk (types, traits)
    ↓
raya-engine (parser, compiler, VM)
    ↓
raya-stdlib + raya-stdlib-posix (native implementations)
    ↓
raya-runtime (high-level API, e2e tests)
    ↓
raya-cli (command-line interface)

raya-native (proc-macros)
raya-pm (package manager)
raya-lsp (language server)
```

## Core Crates

### raya-engine

**Purpose:** Parser, compiler, VM, JIT, AOT

**Location:** `crates/raya-engine/`

**Key Modules:**
- `parser/` - Lexer, parser, AST, type checker
- `compiler/` - IR, optimizations, bytecode generation
- `vm/` - Interpreter, scheduler, GC, runtime
- `jit/` - JIT compilation (feature-gated: `jit`)
- `aot/` - AOT compilation (feature-gated: `aot`)
- `builtins/` - Precompiled builtin type signatures

**Dependencies:**
- Internal only (no external crates except standard libs)
- `logos` for lexer
- `serde` for serialization
- `cranelift-*` for JIT/AOT (feature-gated)

**Tests:** 1,136 lib tests + 147 JIT tests + 55 AOT tests

**Key Types:**
- `Parser`, `Lexer`, `TypeChecker`
- `Compiler`, `Module`, `Opcode`
- `Vm`, `Interpreter`, `Scheduler`, `Gc`

---

### raya-runtime

**Purpose:** High-level API + e2e tests

**Location:** `crates/raya-runtime/`

**Key Modules:**
- `lib.rs` - `Runtime`, `Session`, `CompiledModule`
- `compile.rs` - Compilation pipeline
- `vm_setup.rs` - VM initialization with stdlib
- `loader.rs` - Bytecode loading and linking
- `deps.rs` - Dependency resolution
- `bundle/` - AOT bundle format

**Dependencies:**
- `raya-engine`
- `raya-stdlib`
- `raya-stdlib-posix`
- `raya_pm` for manifest parsing

**Tests:** 2,450 e2e tests + 30 runtime lib tests + 15 bundle tests

**Public API:**
```rust
pub struct Runtime { /* ... */ }
pub struct Session { /* ... */ }
pub struct CompiledModule { /* ... */ }
pub struct RuntimeOptions { /* ... */ }
```

**Usage:**
```rust
use raya_runtime::Runtime;

let rt = Runtime::new();
let module = rt.compile("function main() { }")?;
rt.execute(&module)?;
```

---

### raya-stdlib

**Purpose:** Cross-platform native stdlib

**Location:** `crates/raya-stdlib/`

**Modules:**
- `logger.rs` - Structured logging
- `math.rs` - Math functions
- `crypto.rs` - Hashing, HMAC, random
- `path.rs` - Path manipulation
- `stream.rs` - Reactive streams
- `url.rs` - URL parsing
- `compress.rs` - Compression (gzip, deflate, zlib)
- `encoding.rs` - Hex, base32, base64url
- `semver_mod.rs` - Semantic versioning
- `template.rs` - Template engine

**Raya Sources:** `raya/` directory with `.raya` and `.d.raya` files

**Dependencies:**
- `raya-sdk` (NativeHandler trait)
- Platform-independent crates only

**Tests:** 41 stdlib tests

**Key Type:**
```rust
pub struct StdNativeHandler;

impl NativeHandler for StdNativeHandler {
    fn call(&self, ctx: &NativeContext, id: u16, args: &[NativeValue]) 
        -> NativeCallResult;
}
```

---

### raya-stdlib-posix

**Purpose:** POSIX system APIs

**Location:** `crates/raya-stdlib-posix/`

**Modules:**
- `fs.rs` - File system operations
- `net.rs` - TCP/UDP networking
- `http.rs` - HTTP server
- `fetch.rs` - HTTP client
- `env.rs` - Environment variables
- `process.rs` - Process management
- `os.rs` - Platform info
- `io.rs` - stdin/stdout/stderr
- `dns.rs` - DNS resolution
- `terminal.rs` - Terminal control
- `ws.rs` - WebSocket client
- `readline.rs` - Line editing
- `glob.rs` - File globbing
- `archive.rs` - tar, zip
- `watch.rs` - File watching

**Dependencies:**
- `raya-sdk`
- POSIX-specific crates (`libc`, `rustls`, `tungstenite`, etc.)

**Platform Support:** Linux, macOS, BSD (POSIX-compliant)

---

### raya-cli

**Purpose:** Command-line interface

**Location:** `crates/raya-cli/`

**Commands:**
- `run` - Execute .raya or .ryb files
- `build` - Compile to bytecode
- `check` - Type-check without building
- `eval` - Evaluate expressions
- `repl` - Interactive shell
- `bundle` - AOT compilation (feature-gated)
- `pkg` - Package management namespace
- `clean`, `info` - Utilities

**Dependencies:**
- `raya-runtime`
- `raya_pm`
- `clap` for argument parsing
- `rustyline` for REPL

**Tests:** 26 integration tests + 13 REPL unit tests

**Entry Point:**
```rust
// src/main.rs
fn main() {
    let cli = Cli::parse();
    // Dispatch to commands
}
```

---

### raya-pm

**Purpose:** Package manager

**Location:** `crates/raya-pm/`

**Features:**
- Manifest parsing (`raya.toml`)
- Dependency resolution
- Registry communication
- URL/git caching
- Credential management

**Key Types:**
```rust
pub struct PackageManifest { /* ... */ }
pub struct DependencyResolver { /* ... */ }
pub struct UrlCache { /* ... */ }
```

**Tests:** 204 package manager tests

---

### raya-sdk

**Purpose:** Native module FFI types

**Location:** `crates/raya-sdk/`

**Key Types:**
```rust
pub trait NativeHandler: Send + Sync {
    fn call(&self, ctx: &NativeContext, id: u16, args: &[NativeValue]) 
        -> NativeCallResult;
}

pub struct NativeContext { /* GC, classes, scheduler */ }
pub enum NativeValue { /* Type-safe value wrapper */ }
pub enum NativeCallResult { /* Return or Suspend */ }
pub enum IoRequest { /* I/O operations */ }
pub enum IoCompletion { /* I/O results */ }
```

**Purpose:**
- Defines FFI boundary
- Used by `raya-stdlib`, `raya-stdlib-posix`, and custom native modules
- No dependencies on engine internals

---

### raya-native

**Purpose:** Proc-macros for native modules

**Location:** `crates/raya-native/`

**Macros:**
```rust
#[native_module]
pub mod mymodule {
    #[native_function]
    pub fn add(a: i32, b: i32) -> i32 {
        a + b
    }
}
```

**Usage:** Simplifies native module authoring

---

### raya-lsp

**Purpose:** Language Server Protocol implementation

**Location:** `crates/raya-lsp/`

**Status:** Stub (planned)

**Features (Planned):**
- Go to definition
- Auto-completion
- Hover information
- Diagnostics
- Rename refactoring

**Dependencies:**
- `tower-lsp`
- `tokio`
- `raya-engine` (for parsing/type checking)

---

## Build Configuration

### Workspace Cargo.toml

```toml
[workspace]
resolver = "2"
members = [
    "crates/raya-engine",
    "crates/raya-sdk",
    "crates/raya-stdlib",
    "crates/raya-stdlib-posix",
    "crates/raya-runtime",
    "crates/raya-cli",
    "crates/raya-lsp",
    "crates/raya-pm",
    "crates/raya-native",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
rust-version = "1.90"
```

### Feature Flags

**raya-engine:**
- `jit` - Enable JIT compilation
- `aot` - Enable AOT compilation

**raya-cli:**
- `aot` - Enable `bundle` command

**Usage:**
```bash
cargo build --features jit
cargo build --features aot
cargo build -p raya-cli --features aot
```

## Test Organization

| Crate | Tests | Type |
|-------|-------|------|
| raya-engine | 1,136 | Unit + integration |
| raya-engine (jit) | +147 | JIT unit + integration |
| raya-engine (aot) | +55 | AOT unit + integration |
| raya-runtime | 2,450 | E2E tests |
| raya-runtime | 30 | Runtime API unit tests |
| raya-runtime (bundle) | 15 | Bundle format tests |
| raya-cli | 26 | Integration tests |
| raya-cli (repl) | 13 | REPL unit tests |
| raya-stdlib | 41 | Stdlib unit tests |
| raya-pm | 204 | Package manager tests |
| **Total** | **4,121** | **0 ignored** |

## Build Commands

```bash
# Build all crates
cargo build --workspace

# Build with JIT
cargo build --workspace --features jit

# Build with AOT
cargo build --workspace --features aot

# Test all
cargo test --workspace

# Test specific crate
cargo test -p raya-engine
cargo test -p raya-runtime
cargo test -p raya-cli

# Release build
cargo build --release -p raya-cli
```

## Related

- [Overview](overview.md) - Architecture overview
- [Compiler](compiler.md) - Compilation pipeline
- [VM](vm.md) - Virtual machine internals
