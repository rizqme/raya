# Native Bindings Design

**Status:** Draft
**Version:** 0.1
**Last Updated:** 2026-01-06

---

## Overview

This document specifies the **Native Module System** for Raya, enabling Raya programs to call functions implemented in C, C++, or Rust. This is similar to:
- Node.js N-API (native addons)
- Python C extensions
- Ruby C extensions
- Lua C API

**Architecture:**
```
Raya Program (.raya)
    ↓ imports
Native Module Declaration (.raya)
    ↓ implemented by
Native Library (.so/.dylib/.dll)
    ↓ written in
C / C++ / Rust
```

**Example:**
```typescript
// Raya program
import { hash } from "native:crypto";
const digest = hash("sha256", "hello world");
```

```rust
// Native module (Rust)
#[raya_export]
fn hash(algorithm: String, data: String) -> Result<String, Error> {
    // Native implementation using OS crypto libraries
}
```

---

## Design Goals

1. **Easy to use**: Simple API for both Raya and native sides
2. **Safe**: Protect VM from unsafe native code
3. **Fast**: Zero-copy where possible, minimal marshalling overhead
4. **Stable**: ABI-stable interface for native modules
5. **Cross-platform**: Works on Linux, macOS, Windows
6. **Multi-language**: Support C, C++, and Rust equally

---

## Architecture

### 1. Native Module Declaration (Raya Side)

Native modules are declared in `.raya` files with `native` declarations:

```typescript
// stdlib/crypto.raya
declare module "native:crypto" {
  export function hash(algorithm: string, data: string): string;
  export function randomBytes(length: number): Uint8Array;
}
```

**Syntax:**
- `declare module "native:NAME"` - declares a native module
- `native` prefix in import path indicates native module
- Type signatures define the interface (for type checking)
- No implementation in Raya code

**Module Resolution:**
- `native:crypto` → looks for `libcrypto.{so|dylib|dll}` in module path
- Module path: `$RAYA_MODULE_PATH`, `~/.raya/modules`, `/usr/lib/raya/modules`
- Platform-specific extensions automatically appended

### 2. Native Module Implementation (Native Side)

Native modules export functions via a stable C ABI:

#### C API

```c
// crypto.c
#include <raya/module.h>

// Native function implementation
RayaValue* raya_crypto_hash(RayaContext* ctx, RayaValue** args, size_t argc) {
    // Extract arguments
    const char* algorithm = raya_value_to_string(args[0]);
    const char* data = raya_value_to_string(args[1]);

    // Perform computation
    char* result = compute_hash(algorithm, data);

    // Return value
    return raya_value_from_string(ctx, result);
}

// Module initialization
RAYA_MODULE_INIT(crypto) {
    RayaModuleBuilder* builder = raya_module_builder_new("crypto", "1.0.0");

    // Register functions
    raya_module_add_function(builder, "hash", raya_crypto_hash, 2);
    raya_module_add_function(builder, "randomBytes", raya_crypto_random, 1);

    return raya_module_builder_finish(builder);
}
```

#### Rust API (Higher-level)

```rust
// crypto.rs
use raya_native::{module, function, Value, Context, Error};

#[function]
fn hash(ctx: &Context, algorithm: String, data: String) -> Result<String, Error> {
    // Type conversion automatic via #[function] macro
    let result = compute_hash(&algorithm, &data)?;
    Ok(result)
}

#[function]
fn random_bytes(ctx: &Context, length: u32) -> Result<Vec<u8>, Error> {
    let mut bytes = vec![0u8; length as usize];
    ctx.random_fill(&mut bytes)?;
    Ok(bytes)
}

// Module registration
#[module(name = "crypto", version = "1.0.0")]
mod crypto_module {
    exports! {
        hash,
        random_bytes,
    }
}
```

### 3. Value Marshalling

**Raya → Native:**
- `string` → `const char*` (UTF-8, null-terminated)
- `number` → `double` or `int32_t` (depending on function signature)
- `boolean` → `int` (0 = false, non-zero = true)
- `null` → special marker value
- `Array<T>` → `RayaArray*` (opaque handle with accessor functions)
- `object` → `RayaObject*` (opaque handle with property accessors)

**Native → Raya:**
- `const char*` → `string` (copied, UTF-8 validated)
- `double` / `int32_t` → `number`
- `int` → `boolean`
- `NULL` → `null`
- `RayaArray*` → `Array<any>`
- `RayaObject*` → `object`

**Ownership:**
- **Arguments**: Borrowed (read-only), valid only during function call
- **Return values**: Transferred to Raya (VM takes ownership)
- **Strings**: Automatically copied (native can free after return)
- **Objects/Arrays**: Reference counted, VM handles cleanup

---

## API Specification

### Core Types

```c
// raya/module.h

/** Opaque handle to Raya execution context */
typedef struct RayaContext RayaContext;

/** Opaque handle to Raya value */
typedef struct RayaValue RayaValue;

/** Native function signature */
typedef RayaValue* (*RayaNativeFunction)(
    RayaContext* ctx,
    RayaValue** args,
    size_t argc
);

/** Module initialization function signature */
typedef RayaModule* (*RayaModuleInitFn)(void);

/** Module entry point macro */
#define RAYA_MODULE_INIT(name) \
    __attribute__((visibility("default"))) \
    RayaModule* raya_module_init_##name(void)
```

### Value Conversion Functions

```c
// Extract values from RayaValue
const char* raya_value_to_string(RayaValue* value);
double raya_value_to_number(RayaValue* value);
int32_t raya_value_to_i32(RayaValue* value);
int raya_value_to_bool(RayaValue* value);
int raya_value_is_null(RayaValue* value);

// Create RayaValue (transfers ownership to VM)
RayaValue* raya_value_from_string(RayaContext* ctx, const char* str);
RayaValue* raya_value_from_number(RayaContext* ctx, double num);
RayaValue* raya_value_from_i32(RayaContext* ctx, int32_t num);
RayaValue* raya_value_from_bool(RayaContext* ctx, int boolean);
RayaValue* raya_value_null(RayaContext* ctx);

// Error creation
RayaValue* raya_error_new(RayaContext* ctx, const char* message);
```

### Module Builder API

```c
// Create module builder
RayaModuleBuilder* raya_module_builder_new(const char* name, const char* version);

// Register function
void raya_module_add_function(
    RayaModuleBuilder* builder,
    const char* name,
    RayaNativeFunction func,
    size_t arity  // number of parameters
);

// Finish building module
RayaModule* raya_module_builder_finish(RayaModuleBuilder* builder);
```

### Array and Object Access

```c
// Array operations
size_t raya_array_length(RayaValue* array);
RayaValue* raya_array_get(RayaValue* array, size_t index);
RayaValue* raya_array_new(RayaContext* ctx, size_t length);
void raya_array_set(RayaValue* array, size_t index, RayaValue* value);

// Object operations
RayaValue* raya_object_get(RayaValue* object, const char* key);
RayaValue* raya_object_new(RayaContext* ctx);
void raya_object_set(RayaValue* object, const char* key, RayaValue* value);
int raya_object_has(RayaValue* object, const char* key);
```

---

## Module Loading

### Dynamic Library Loading

1. **Import resolution:**
   ```typescript
   import { hash } from "native:crypto";
   ```

2. **Library search:**
   - Search paths: `$RAYA_MODULE_PATH`, `~/.raya/modules`, `/usr/lib/raya/modules`
   - Filenames tried: `libcrypto.so`, `libcrypto.dylib`, `crypto.dll` (platform-specific)

3. **Symbol resolution:**
   - Look for: `raya_module_init_crypto` (mangled from module name)
   - Call init function to get module descriptor

4. **Version checking:**
   - Compare module version with declared version
   - Fail if major version mismatch

5. **Function registration:**
   - Register all exported functions in VM's native function table
   - Link Raya imports to native implementations

### Module Descriptor

```c
typedef struct RayaModule {
    const char* name;           // "crypto"
    const char* version;        // "1.0.0" (semver)
    uint32_t abi_version;       // ABI version (e.g., 1)

    size_t function_count;
    RayaFunctionDescriptor* functions;
} RayaModule;

typedef struct RayaFunctionDescriptor {
    const char* name;           // "hash"
    RayaNativeFunction func;    // Function pointer
    size_t arity;               // Number of parameters
} RayaFunctionDescriptor;
```

---

## Safety and Error Handling

### Error Propagation

Native functions can return errors that become Raya exceptions:

```c
RayaValue* raya_crypto_hash(RayaContext* ctx, RayaValue** args, size_t argc) {
    if (argc != 2) {
        return raya_error_new(ctx, "hash() requires 2 arguments");
    }

    const char* algorithm = raya_value_to_string(args[0]);
    if (strcmp(algorithm, "sha256") != 0 && strcmp(algorithm, "sha512") != 0) {
        return raya_error_new(ctx, "Unsupported hash algorithm");
    }

    // ... normal execution
}
```

In Raya:
```typescript
try {
    const digest = hash("md5", "data"); // unsupported
} catch (e) {
    console.log(e.message); // "Unsupported hash algorithm"
}
```

### Memory Safety

**Rules:**
1. **No dangling pointers**: All `RayaValue*` are GC-managed
2. **Lifetime guarantees**: Arguments valid only during function call
3. **String ownership**: VM copies all returned strings
4. **Reference counting**: Arrays/objects automatically managed

**Unsafe operations banned:**
- Direct pointer manipulation of `RayaValue` internals
- Storing `RayaValue*` in static variables
- Returning stack-allocated memory

### Sandboxing

Native modules run in the same process but with limited capabilities:

1. **No direct VM access**: Can't bypass VM security
2. **Resource limits**: Inherit context's resource limits
3. **Capability-based**: Module can only access capabilities passed by Raya
4. **Crash isolation**: Native crashes terminate only the current VM context

---

## Build System Integration

### Building Native Modules with Rust

```toml
# Cargo.toml
[package]
name = "raya-crypto"
version = "1.0.0"

[lib]
crate-type = ["cdylib"]

[dependencies]
raya-native = "0.1"
```

```bash
cargo build --release
# Produces: target/release/libcrypto.so (Linux)
#           target/release/libcrypto.dylib (macOS)
#           target/release/crypto.dll (Windows)
```

### Building with C/C++

```makefile
# Makefile
CC = gcc
CFLAGS = -fPIC -O2 -I/usr/include/raya

crypto.so: crypto.c
	$(CC) $(CFLAGS) -shared -o libcrypto.so crypto.c

install:
	cp libcrypto.so ~/.raya/modules/
```

### CMake Integration

```cmake
# CMakeLists.txt
add_library(crypto SHARED crypto.c)
target_include_directories(crypto PRIVATE /usr/include/raya)
set_target_properties(crypto PROPERTIES PREFIX "lib")
```

---

## Example: Complete Native Module

### Raya Declaration

```typescript
// stdlib/fs.raya
declare module "native:fs" {
    export function readFile(path: string): Result<string, Error>;
    export function writeFile(path: string, content: string): Result<void, Error>;
    export function exists(path: string): boolean;
}
```

### Rust Implementation

```rust
// fs.rs
use raya_native::{module, function, Value, Context, Error};
use std::fs;

#[function]
fn read_file(ctx: &Context, path: String) -> Result<String, Error> {
    fs::read_to_string(&path)
        .map_err(|e| Error::new(&format!("Failed to read file: {}", e)))
}

#[function]
fn write_file(ctx: &Context, path: String, content: String) -> Result<(), Error> {
    fs::write(&path, content)
        .map_err(|e| Error::new(&format!("Failed to write file: {}", e)))
}

#[function]
fn exists(ctx: &Context, path: String) -> Result<bool, Error> {
    Ok(fs::metadata(&path).is_ok())
}

#[module(name = "fs", version = "1.0.0")]
mod fs_module {
    exports! {
        read_file,
        write_file,
        exists,
    }
}
```

### Raya Usage

```typescript
import { readFile, writeFile, exists } from "native:fs";

if (exists("/tmp/input.txt")) {
    const content = readFile("/tmp/input.txt");
    match(content, {
        ok: (text) => {
            console.log("File contents:", text);
            writeFile("/tmp/output.txt", text.toUpperCase());
        },
        error: (e) => {
            console.error("Error:", e.message);
        }
    });
}
```

---

## ABI Versioning

The native module ABI follows semantic versioning:

```c
#define RAYA_ABI_VERSION_MAJOR 1
#define RAYA_ABI_VERSION_MINOR 0
#define RAYA_ABI_VERSION_PATCH 0
```

**Version compatibility:**
- **MAJOR**: Breaking changes (incompatible)
  - Function signature changes
  - Struct layout changes
  - Removed functions
- **MINOR**: New features (backward compatible)
  - New functions added
  - New optional struct fields
- **PATCH**: Bug fixes (no API/ABI changes)

**Version checking on load:**
```c
// VM checks module's ABI version
if (module->abi_version_major != RAYA_ABI_VERSION_MAJOR) {
    return error("Incompatible module ABI version");
}
```

---

## Standard Library Native Modules

Planned standard native modules:

1. **`native:fs`** - File system operations
2. **`native:crypto`** - Cryptographic functions (hash, encrypt, sign)
3. **`native:net`** - Low-level networking (sockets, TLS)
4. **`native:http`** - HTTP client/server
5. **`native:compress`** - Compression algorithms (gzip, zstd)
6. **`native:regex`** - Regular expressions (using PCRE or RE2)
7. **`native:json`** - Fast JSON parsing/serialization
8. **`native:sqlite`** - SQLite database binding

---

## Security Considerations

1. **Module verification:**
   - Optional code signing for native modules
   - Whitelist of allowed modules
   - Permissions model (e.g., file access, network access)

2. **Resource limits:**
   - CPU time limits inherited from VM context
   - Memory limits enforced by VM
   - Native code can't bypass VM resource accounting

3. **Crash handling:**
   - Native crash doesn't kill entire VM
   - Context isolation limits blast radius
   - Error reporting back to Raya code

4. **Type safety:**
   - Runtime type checking of arguments
   - Validation of return values
   - No uninitialized memory leaks

---

## Implementation Plan

### Phase 1: Core Infrastructure (Milestone 1.14)
- [ ] Define C API (`raya/module.h`)
- [ ] Implement module loader (dlopen/LoadLibrary)
- [ ] Implement value marshalling
- [ ] Implement native function invocation
- [ ] Add `native:` import resolution to compiler

### Phase 2: Rust API (Milestone 1.14)
- [ ] Create `raya-native` Rust crate
- [ ] Implement `#[function]` macro
- [ ] Implement `#[module]` macro
- [ ] Add automatic type conversion

### Phase 3: Standard Modules (Milestone 1.14)
- [ ] Implement `native:fs` module
- [ ] Implement `native:crypto` module
- [ ] Add documentation and examples

### Phase 4: Tooling (Future)
- [ ] Module signing and verification
- [ ] Module package manager integration
- [ ] Binary distribution of native modules

---

## Comparison with Other Systems

| Feature | Raya Native | Node.js N-API | Python C API | Lua C API |
|---------|-------------|---------------|--------------|-----------|
| **ABI Stability** | ✅ Stable | ✅ Stable | ❌ Unstable | ✅ Stable |
| **Type Safety** | ✅ Checked | ⚠️ Manual | ⚠️ Manual | ⚠️ Manual |
| **Rust Support** | ✅ First-class | ⚠️ Wrapper | ⚠️ Wrapper | ⚠️ Wrapper |
| **Error Handling** | ✅ Result types | ⚠️ Exceptions | ⚠️ Manual | ⚠️ Manual |
| **GC Integration** | ✅ Automatic | ✅ Automatic | ✅ Automatic | ✅ Automatic |
| **Async Support** | 🔄 Planned | ✅ Yes | ⚠️ Limited | ❌ No |

**Key advantages:**
- **Rust-first design**: Native modules in Rust are as easy as Raya code
- **Type-safe marshalling**: Automatic conversion with compile-time validation
- **Modern error handling**: Result types and match expressions
- **ABI stability**: Version 1.0 modules work with future VM versions

---

## References

- Node.js N-API: https://nodejs.org/api/n-api.html
- Python C API: https://docs.python.org/3/c-api/
- Lua C API: https://www.lua.org/manual/5.4/manual.html#4
- Rust FFI: https://doc.rust-lang.org/nomicon/ffi.html
