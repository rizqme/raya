---
title: "Native Bindings"
---

# Native Bindings Design

> **Status:** Partially Implemented
> **Note:** Internal native ABI superseded by [ABI.md](./abi.md) (M4.9). This document covers the external/third-party native module system.
> **Related:** [ABI](./abi.md), [Stdlib](../stdlib/stdlib.md), [Modules](../runtime/modules.md)

**Version:** 2.0 (Rust-only)

---

## Overview

This document specifies the **Native Module System** for Raya, enabling Raya programs to call functions implemented in Rust. C/C++ libraries can be wrapped using Rust's standard FFI.

**Key Design Principle: Transparency**
- Native modules are **invisible** to Raya users
- All imports use the same syntax regardless of implementation
- No `native:` prefix or special markers in user code
- Module resolver automatically detects bytecode vs native

**Architecture:**
```
Raya Program (.raya)
    ↓ import { parse } from "std:json"
Module Resolver (checks native first, then bytecode)
    ↓ Native? Load .so/.dylib/.dll
    ↓ Bytecode? Load .ryb
Native Module (Rust)
    ↓ implements raya-ffi API
VM Function Registry
```

---

## Design Goals

1. **Transparent**: Users can't tell native from bytecode modules
2. **Rust-only**: Simplify by supporting only Rust (C/C++ wraps in Rust)
3. **Safe**: Thread safety enforced by Rust's type system
4. **Fast**: Zero-copy where possible, ~25-50ns FFI overhead
5. **Stable ABI**: Semantic versioning with compatibility checks
6. **Ergonomic**: Proc-macros make native modules easy to write

---

## User Experience

### Raya Code (Completely Transparent)

```typescript
// All imports look identical - user has no idea which are native
import { parse, stringify } from "std:json";     // Native (pre-configured)
import { readFile, writeFile } from "std:fs";    // Native (pre-configured)
import { hash } from "std:crypto";               // Native (pre-configured)
import { helper } from "mylib";                  // Bytecode (regular dependency)
import { custom } from "custom:mymodule";        // Native (user-configured)

// Usage is identical
const data = parse('{"key": "value"}');
const content = await readFile("./file.txt");
const result = helper(42);
```

**No special syntax required!**

---

## Configuration System

### 1. Standard Library (Pre-configured)

The Raya runtime **automatically** registers standard library native modules during VM initialization:

```rust
// raya-core/src/vm/stdlib.rs (or similar)
pub fn register_stdlib_native_modules(vm: &mut VmContext) {
    // Pre-configured standard library native modules
    register_native_module(vm, raya_stdlib_json::create_module());
    register_native_module(vm, raya_stdlib_fs::create_module());
    register_native_module(vm, raya_stdlib_crypto::create_module());
    register_native_module(vm, raya_stdlib_buffer::create_module());
    register_native_module(vm, raya_stdlib_http::create_module());
    register_native_module(vm, raya_stdlib_net::create_module());
    register_native_module(vm, raya_stdlib_events::create_module());
    register_native_module(vm, raya_stdlib_stream::create_module());
}
```

Users just import `std:*` - no configuration needed.

### 2. Custom Native Bindings (User-configured)

For user-provided native modules, configure in `raya.toml`:

```toml
[package]
name = "my-app"
version = "1.0.0"

[dependencies]
mylib = "1.2.3"  # Regular bytecode dependency

[native-bindings]
# Map package name to native library and type definitions
# Format: "package:name" = { lib = "library_name", types = "path/to/types.d.raya" }

"custom:crypto" = {
    lib = "my_crypto_wrapper",        # Loads libmy_crypto_wrapper.so/dylib/dll
    types = "./types/crypto.d.raya"   # Type definitions for type checker
}

"custom:database" = {
    lib = "postgres_driver",
    types = "./types/database.d.raya"
}
```

**Library Search Path (in order):**
1. `./native_modules/` (current directory)
2. `$RAYA_MODULE_PATH` (environment variable, colon-separated)
3. `~/.raya/modules/` (user modules)
4. `/usr/local/lib/raya/modules/` (system modules - Linux/macOS)
5. `%PROGRAMFILES%\Raya\modules\` (system modules - Windows)

**Platform-specific naming:**
- Linux: `libmy_crypto_wrapper.so`
- macOS: `libmy_crypto_wrapper.dylib`
- Windows: `my_crypto_wrapper.dll`

---

## Module Resolution Flow

```
1. Raya Import Statement
   import { parse } from "std:json"

2. Import Resolver (raya-core/module/import.rs)
   Parse specifier → ImportSpec::Package { name: "std:json" }

3. Module Resolver (VmContext)
   ├─→ Check native module registry
   │   ├─→ Pre-configured stdlib? (std:*)
   │   │   └─→ Return NativeModule
   │   └─→ User-configured binding? (raya.toml)
   │       └─→ Load dynamic library
   │           └─→ Return NativeModule
   │
   └─→ Not native?
       └─→ Resolve as bytecode (.ryb)
           ├─→ Check cache (~/.raya/cache/)
           └─→ Or compile .raya → .ryb

4. Function Call
   Native? → Direct call via NativeFn pointer
   Bytecode? → Interpreter executes opcodes
```

**Key: Native modules checked FIRST, then bytecode fallback**

---

## Rust Ergonomic API

### Simple Example

```rust
// my_crypto_wrapper/src/lib.rs
use raya_native::{function, module};

#[function]
fn sha256(data: String) -> String {
    use sha2::{Sha256, Digest};
    let hash = Sha256::digest(data.as_bytes());
    hex::encode(hash)
}

#[function]
fn random_bytes(length: i32) -> Vec<u8> {
    use rand::RngCore;
    let mut bytes = vec![0u8; length as usize];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes
}

#[module]
mod crypto {
    fn sha256(data: String) -> String;
    fn random_bytes(length: i32) -> Vec<u8>;
}
```

**Proc-macro expansion:**
- `#[function]` generates FFI wrapper with marshalling
- `#[module]` generates registration function
- Automatic panic catching and error conversion
- Type-safe argument extraction

### Type Conversions (FromRaya/ToRaya)

**Supported types (Phase 1):**
- Primitives: `i32`, `f64`, `bool`, `()`
- Results: `Result<T, E>` where `T: ToRaya, E: ToString`

**Future types:**
- Strings: `String`, `&str` (requires heap allocation design)
- Collections: `Vec<T>`, `HashMap<K, V>`
- Binary data: `Vec<u8>`, `&[u8]`
- Custom types: User-defined structs/enums

---

## ABI Design

### ABI Version

**Format:** Semantic versioning `MAJOR.MINOR.PATCH`

```rust
// raya-ffi/src/lib.rs
pub const RAYA_ABI_VERSION: (u32, u32, u32) = (1, 0, 0);

// Native module declares compatible version
#[no_mangle]
pub extern "C" fn raya_module_abi_version() -> (u32, u32, u32) {
    (1, 0, 0)
}
```

**Compatibility rules:**
- `MAJOR` change: Breaking changes (incompatible)
- `MINOR` change: New features (backward compatible)
- `PATCH` change: Bug fixes (fully compatible)

**VM checks on load:**
```rust
let module_version = module.abi_version();
let vm_version = RAYA_ABI_VERSION;

if module_version.0 != vm_version.0 {
    return Err("Incompatible ABI major version");
}
if module_version.1 > vm_version.1 {
    return Err("Module requires newer ABI minor version");
}
// PATCH differences are always compatible
```

### Function Signature

```rust
pub type NativeFn = extern "C" fn(
    args: *const NativeValue,
    arg_count: usize
) -> NativeValue;
```

**Properties:**
- `extern "C"`: C calling convention (stable ABI)
- Raw pointers: FFI-safe
- Simple types: No complex Rust types in signature
- Returns `NativeValue`: Can represent any Raya value or error

### Module Registration

```rust
// Module init function (called on load)
#[no_mangle]
pub extern "C" fn raya_module_init() -> *mut NativeModule {
    let mut module = NativeModule::new("crypto", "1.0.0");
    module.register_function("sha256", sha256_ffi);
    module.register_function("random_bytes", random_bytes_ffi);
    Box::into_raw(Box::new(module))
}

// Module cleanup function (called on unload)
#[no_mangle]
pub extern "C" fn raya_module_cleanup(module: *mut NativeModule) {
    unsafe { drop(Box::from_raw(module)) }
}
```

**Symbol naming convention:**
- Init: `raya_module_init`
- Cleanup: `raya_module_cleanup`
- ABI version: `raya_module_abi_version`
- Functions: `<name>_ffi` (internal, not exported)

### Value Representation

```rust
#[repr(C)]
pub struct NativeValue {
    inner: usize,  // Pointer to heap-allocated Value
}
```

**Zero-copy marshalling:**
- Primitives (`i32`, `f64`, `bool`): Copied (~1-5ns)
- Heap values (strings, objects): Opaque handles (~1ns)
- No deep copying unless necessary

**GC Safety (TODO):**
- Values pinned during native call
- `pin_count` atomic increment
- Automatic unpinning on return

---

## Type Definitions

Native modules provide `.d.raya` type definition files for the type checker.

### Standard Library Type Definitions

**Location:** `$RAYA_ROOT/stdlib/types/`

**Example:** `stdlib/types/json.d.raya`
```typescript
// Type definitions for std:json
export function parse(json: string): any;
export function stringify(value: any, space?: number): string;
```

**Example:** `stdlib/types/fs.d.raya`
```typescript
// Type definitions for std:fs
export function readFile(path: string): Promise<string>;
export function writeFile(path: string, content: string): Promise<void>;
export function exists(path: string): Promise<boolean>;
export function mkdir(path: string): Promise<void>;
export function readdir(path: string): Promise<string[]>;
```

### User Type Definitions

**Specified in `raya.toml`:**
```toml
[native-bindings]
"custom:crypto" = {
    lib = "my_crypto_wrapper",
    types = "./types/crypto.d.raya"  # ← User provides this
}
```

**Example:** `types/crypto.d.raya`
```typescript
export function sha256(data: string): string;
export function sha512(data: string): string;
export function randomBytes(length: number): Uint8Array;
export function pbkdf2(
    password: string,
    salt: string,
    iterations: number
): string;
```

**Type checker integration:**
1. Compiler reads `raya.toml`
2. Parses `.d.raya` files for native bindings
3. Type checks imports against these signatures
4. Reports errors if types don't match

---

## Standard Library Modules

### std:json
```rust
#[function]
fn parse(json: String) -> Result<Value, String>;

#[function]
fn stringify(value: Value, space: Option<i32>) -> String;
```

**Migration from opcodes:**
- Currently: `JsonParse` (0xE0), `JsonStringify` (0xE1)
- Future: Native module `std:json`
- Transition: Keep opcodes for 1-2 releases, then deprecate

### std:fs
```rust
#[function]
async fn read_file(path: String) -> Result<String, Error>;

#[function]
async fn write_file(path: String, content: String) -> Result<(), Error>;

#[function]
async fn exists(path: String) -> Result<bool, Error>;

#[function]
async fn mkdir(path: String) -> Result<(), Error>;
```

### std:crypto
```rust
#[function]
fn hash(algorithm: String, data: String) -> Result<String, Error>;

#[function]
fn random_bytes(length: i32) -> Vec<u8>;

#[function]
fn uuid() -> String;

#[function]
fn pbkdf2(password: String, salt: String, iterations: i32) -> String;
```

### std:buffer
```rust
// Binary data operations (Node.js Buffer equivalent)
#[function]
fn from_string(s: String, encoding: String) -> Buffer;

#[function]
fn to_string(buf: Buffer, encoding: String) -> String;

#[function]
fn concat(buffers: Vec<Buffer>) -> Buffer;
```

### std:http
```rust
#[function]
async fn fetch(url: String, options: RequestOptions) -> Result<Response, Error>;

#[function]
fn create_server(handler: Function) -> Server;
```

### std:net
```rust
#[function]
async fn connect(host: String, port: i32) -> Result<TcpStream, Error>;

#[function]
fn create_server(port: i32, handler: Function) -> TcpServer;
```

### std:events
```rust
// EventEmitter pattern
#[function]
fn create_emitter() -> EventEmitter;

#[function]
fn on(emitter: EventEmitter, event: String, handler: Function);

#[function]
fn emit(emitter: EventEmitter, event: String, data: Value);
```

### std:stream
```rust
// Readable/Writable/Transform streams
#[function]
fn create_readable(source: Function) -> ReadableStream;

#[function]
fn create_writable(sink: Function) -> WritableStream;

#[function]
fn pipe(source: ReadableStream, dest: WritableStream);
```

---

## Performance

### FFI Overhead Target

**Target:** ~25-50ns total FFI overhead per call

**Breakdown:**
- Primitives (`i32`, `f64`, `bool`): ~1-5ns (direct copy)
- Heap values (strings, objects): ~1ns (pointer copy)
- Function call overhead: ~10-20ns
- Error handling: ~5-10ns

**Comparison:**
- Node.js N-API: ~50-100ns
- Python C API: ~100-200ns
- Raya target: **2-4x faster** than Node.js

### Zero-Copy Strategy

**Primitives:** Always copied (cheap)
```rust
impl ToRaya for i32 {
    fn to_raya(self) -> NativeValue {
        NativeValue::from_value(Value::i32(self))  // ~1-5ns
    }
}
```

**Strings:** Opaque handles with accessors (zero-copy)
```rust
// TODO: Not yet implemented
impl ToRaya for &str {
    fn to_raya(self) -> NativeValue {
        // Pin string, return opaque handle
        NativeValue::from_string_ptr(self.as_ptr(), self.len())  // ~1ns
    }
}

// Access via unsafe API
let ptr = native_value.as_string_ptr();
let len = native_value.string_len();
let s = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) };
```

**Objects:** Opaque handles with accessors (zero-copy)
```rust
// Access properties without copying object
let name = object.get_property("name")?;
let age = object.get_property("age")?.as_i32()?;
```

---

## Thread Safety

### Enforced by Rust Type System

```rust
// NativeFn must be Send + Sync (enforced at compile time)
pub type NativeFn = extern "C" fn(...) -> NativeValue;

// Module registration enforces safety
pub fn register_function(&mut self, name: impl Into<String>, func: NativeFn) {
    // func: NativeFn automatically requires Send + Sync
    self.functions.insert(name.into(), func);
}
```

### Atomic Operations

```rust
// GC pinning (atomic reference counting)
struct GcPtr {
    ptr: NonNull<HeapObject>,
    pin_count: AtomicU32,  // Thread-safe
}

pub fn pin_value(value: NativeValue) {
    // Atomic increment
    value.ptr().pin_count.fetch_add(1, Ordering::AcqRel);
}

pub fn unpin_value(value: NativeValue) {
    // Atomic decrement
    value.ptr().pin_count.fetch_sub(1, Ordering::AcqRel);
}
```

### Task-Local Storage (Future)

```rust
// For storing per-task state
#[function]
fn set_context(key: String, value: Value) {
    task_local::set(key, value);
}

#[function]
fn get_context(key: String) -> Option<Value> {
    task_local::get(&key)
}
```

---

## Error Handling

### Panic Catching

```rust
// Generated by #[function] macro
extern "C" fn sha256_ffi(args: *const NativeValue, arg_count: usize) -> NativeValue {
    // Validate arguments
    if arg_count != 1 {
        return NativeValue::error("sha256 expects 1 argument");
    }

    // Catch panics
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let data = unsafe { String::from_raya(*args.offset(0))? };
        sha256(data)
    }));

    match result {
        Ok(Ok(value)) => value.to_raya(),
        Ok(Err(e)) => NativeValue::error(e.to_string()),
        Err(panic) => {
            let msg = panic_message(&panic);
            NativeValue::error(format!("Function panicked: {}", msg))
        }
    }
}
```

### Error Propagation

```rust
// Automatic conversion from Result<T, E>
impl<T: ToRaya, E: ToString> ToRaya for Result<T, E> {
    fn to_raya(self) -> NativeValue {
        match self {
            Ok(value) => value.to_raya(),
            Err(error) => NativeValue::error(error.to_string()),
        }
    }
}

// Usage in native functions
#[function]
fn read_file(path: String) -> Result<String, std::io::Error> {
    std::fs::read_to_string(path)  // Errors automatically propagated
}
```

---

## Security Considerations

### Sandboxing (Future)

Native modules run with full OS access by default. Future sandboxing options:

1. **Capability-based security** (like Inner VM)
   - Native modules declare required capabilities
   - VM grants/denies based on policy

2. **System call filtering** (seccomp on Linux)
   - Restrict native modules to allowed syscalls
   - Prevent access to filesystem, network, etc.

3. **Process isolation** (separate process)
   - Run native modules in isolated process
   - IPC for communication (higher overhead)

### Code Signing (Future)

- Require native libraries to be signed
- Verify signature before loading
- Prevent loading of untrusted code

---

## Implementation Status

**Phase 1: Rust Ergonomic API** ✅ COMPLETE
- [x] `raya-native` crate (proc-macros)
- [x] `raya-ffi` native module support
- [x] `FromRaya`/`ToRaya` traits (primitives)
- [x] `NativeModule` registration
- [x] Working example

**Phase 2: Dynamic Library Loading** (Next)
- [ ] Cross-platform loading (.so/.dylib/.dll)
- [ ] Symbol resolution
- [ ] ABI version checking
- [ ] Module search paths
- [ ] Module registry in VmContext

**Phase 3: Type Definitions**
- [ ] Parse `.d.raya` files
- [ ] Integrate with type checker
- [ ] Standard library type definitions

**Phase 4: Standard Library Modules**
- [ ] std:json (migrate from opcodes)
- [ ] std:fs
- [ ] std:crypto
- [ ] std:buffer
- [ ] std:http
- [ ] std:net
- [ ] std:events
- [ ] std:stream

---

## Summary

**Key Innovation: Transparency**
- Users write `import { parse } from "std:json"` without knowing it's native
- No special syntax or configuration for standard library
- Custom native bindings via `raya.toml` for advanced users

**Benefits:**
- **Performance:** Hot path operations (JSON, I/O, crypto) use native code
- **Flexibility:** Modules can be swapped (bytecode ↔ native) without changing user code
- **Safety:** Rust's type system enforces thread safety
- **Simplicity:** One import syntax for all modules
- **Distribution:** Native libraries packaged with type definitions

**See Also:**
- [plans/milestone-1.15.md](https://github.com/rizqme/raya/blob/main/plans/milestone-1.15.md) - Implementation plan
- [Modules](../runtime/modules.md) - Module system design
- [Stdlib](../stdlib/stdlib.md) - Standard library design
