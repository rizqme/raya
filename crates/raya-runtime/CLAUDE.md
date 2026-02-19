# raya-runtime

High-level runtime API for compiling and executing Raya programs. Hosts all e2e tests.

## Overview

This crate provides the `Runtime` struct — the main entry point for compiling `.raya` source files, loading `.ryb` bytecode, resolving dependencies, and executing programs. It integrates the engine, stdlib, and package manager into a clean API used by `raya-cli` and embedders.

## Architecture

```
raya-engine (parser, compiler, VM, bytecode format)
    ↓
raya-stdlib (StdNativeHandler + all stdlib modules)
raya-stdlib-posix (POSIX natives: fs, net, http, process, os, env, io)
rpkg (PackageManifest, DependencyResolver, UrlCache)
    ↓
raya-runtime (Runtime API: compile, load, execute, eval, dependency resolution)
    ↓
raya-cli (CLI commands use Runtime)
```

## Public API

### `Runtime`
- `Runtime::new()` — default options
- `Runtime::with_options(RuntimeOptions)` — custom threads, heap, timeout, JIT settings
- `compile(source: &str) -> Result<CompiledModule>` — compile source code
- `compile_file(path: &Path) -> Result<CompiledModule>` — compile a .raya file
- `load_bytecode(path: &Path) -> Result<CompiledModule>` — load a .ryb file
- `load_bytecode_bytes(bytes: &[u8]) -> Result<CompiledModule>` — load .ryb from memory
- `execute(module: &CompiledModule) -> Result<i32>` — run, returns exit code
- `execute_with_deps(module: &CompiledModule, deps: &[CompiledModule]) -> Result<i32>` — run with linked dependencies
- `eval(source: &str) -> Result<NativeValue>` — evaluate expression, returns value
- `run_file(path: &Path) -> Result<i32>` — auto-detect .raya/.ryb, resolve deps from manifest
- `run_file_with_deps(path: &Path, deps: Vec<CompiledModule>) -> Result<i32>` — run with explicit deps

### `CompiledModule`
- Wraps `Module` + optional `Interner`
- `encode(&self) -> Vec<u8>` — serialize to .ryb bytes

### `RuntimeOptions`
- `threads: usize` — worker threads (0 = auto-detect via num_cpus)
- `heap_limit: usize` — bytes (0 = unlimited)
- `timeout: u64` — milliseconds (0 = unlimited)
- `no_jit: bool` — disable JIT
- `jit_threshold: u32` — invocations before JIT kicks in

### `RuntimeError`
- `Io`, `Lex`, `Parse`, `TypeCheck`, `Compile`, `Bytecode`, `Vm`, `Dependency`

## Module Structure

```
src/
├── lib.rs              # Runtime struct, CompiledModule, RuntimeOptions, public API
├── error.rs            # RuntimeError enum (thiserror)
├── builtins.rs         # builtin_sources() + std_sources() via include_str!
├── compile.rs          # compile_source(): parser → binder → checker → compiler
├── vm_setup.rs         # create_vm(): VM with StdNativeHandler + stdlib + posix
├── loader.rs           # load_bytecode_file(), resolve_ryb_deps(), find_library()
└── deps.rs             # load_dependencies() from raya.toml manifest

tests/
├── e2e_tests.rs        # E2E test entry point
└── e2e/                # 27+ test modules (1,297+ tests)
    ├── harness.rs       # Test harness (compile + execute)
    └── *.rs             # Feature modules (arrays, classes, closures, concurrency, etc.)
```

## Compilation Pipeline

```
builtin_sources() + std_sources() + user source
    → Parser::new() → parse()
    → Binder (empty native sigs — builtins are in source)
    → TypeChecker
    → Compiler::compile_via_ir()
    → Module (bytecode)
```

## Dependency Resolution (`deps.rs`)

Resolves `[dependencies]` from `raya.toml`:
- **Local path** (`path = "../lib"`) — canonicalize + load entry point
- **URL/git** (`git = "https://..."`) — check rpkg UrlCache, fallback to `~/.raya/cache/urls/`
- **Registry** (`version = "1.0"`) — check `raya_packages/`, fallback to `~/.raya/packages/`

Entry point discovery for package dirs: `raya.toml → [package].main` → fallback to `src/lib.raya`, `src/main.raya`, `lib.raya`, `index.raya`, `main.raya`.

## Native ID Routing

Routing is handled by `StdNativeHandler` in `raya-stdlib/src/handler.rs`:

| Range | Module | Methods |
|-------|--------|---------|
| 0x1000-0x1003 | Logger | debug, info, warn, error |
| 0x2000-0x2016 | Math | abs, sign, floor, ceil, round, trunc, min, max, pow, sqrt, sin, cos, tan, asin, acos, atan, atan2, exp, log, log10, random, PI, E |
| 0x4000-0x400B | Crypto | hash, hashBytes, hmac, hmacBytes, randomBytes, randomInt, randomUUID, toHex, fromHex, toBase64, fromBase64, timingSafeEqual |
| 0x5000-0x5004 | Time | now, monotonic, hrtime, elapsed, sleep |
| 0x6000-0x600C | Path | join, normalize, dirname, basename, extname, isAbsolute, resolve, relative, cwd, sep, delimiter, stripExt, withExt |

## Tests

- **E2E tests** (1,297+): Full compilation + execution tests using `StdNativeHandler`
- **0 ignored**: All tests passing
- Test modules include: syntax_edge_cases, concurrency_edge_cases, edge_cases, and 30+ feature modules

## For AI Assistants

- `Runtime` is the primary API — use it instead of manually wiring engine components
- E2E tests live here, NOT in raya-engine
- `StdNativeHandler` implementation lives in `raya-stdlib/src/handler.rs`, re-exported here for backward compat
- When adding new stdlib modules, implement in `raya-stdlib`, route in `handler.rs`
- The `builtins.rs` file uses `include_str!` to embed builtin + std `.raya` source at compile time
- Run runtime tests with: `cargo test -p raya-runtime`
