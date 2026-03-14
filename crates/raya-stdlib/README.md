# Raya Standard Library (raya-stdlib)

Native implementations of Raya's standard library functions, built as both static and dynamic libraries.

## Overview

The `raya-stdlib` crate provides high-performance native implementations of core standard library functionality for the Raya VM. It is built using the native module FFI system and can be loaded either statically (linked at compile time) or dynamically (loaded at runtime).

## Features

### JSON Module (`std:json`)

High-performance JSON parsing and stringification using Raya's custom JSON parser that directly integrates with the VM's garbage collector.

**Functions:**
- `parse(jsonString: string): any` - Parse JSON string into Raya value
- `stringify(value: any): string` - Convert Raya value to JSON string
- `isValid(jsonString: string): boolean` - Validate JSON without parsing

**Implementation Details:**
- Uses custom JSON parser from `raya-core` ([json.rs](src/json.rs))
- Direct GC integration for efficient memory management
- Zero-copy parsing where possible
- Native FFI wrapper in ([json_native.rs](src/json_native.rs))

**Type Definitions:**
The standard library surface is defined by the shipped `.raya` wrapper modules.

## Building

### As a Static Library
```bash
cargo build -p raya-stdlib
```

### As a Dynamic Library
The crate is configured to build both static (`.rlib`) and dynamic (`.so`/`.dylib`/`.dll`) libraries automatically:
```bash
cargo build -p raya-stdlib --release
```

Output locations:
- **macOS**: `target/release/libraya_stdlib.dylib`
- **Linux**: `target/release/libraya_stdlib.so`
- **Windows**: `target/release/raya_stdlib.dll`

## Usage

### Static Linking

```rust
use raya_stdlib::json_module_init;

// Initialize the JSON module
let module = json_module_init();

// Register with VM
vm.register_native_module(module);
```

### Dynamic Loading

```rust
use raya_ffi::Library;

// Load the dynamic library
let lib = Library::open("./libraya_stdlib.dylib")?;

// Load the module
let module = lib.load_module()?;

// Register with VM
vm.register_native_module(module);
```

## Testing

Run all tests:
```bash
cargo test -p raya-stdlib
```

### Test Coverage

**Module Initialization Tests** ([json_native.rs](tests/json_native.rs)):
- ✅ Module metadata verification
- ✅ Function registration verification
- ✅ Function count validation
- ✅ Multiple initialization support

**Dynamic Loading Tests** ([json_dynamic_loading.rs](tests/json_dynamic_loading.rs)):
- ✅ Dynamic library loading
- ✅ Symbol export verification
- ✅ Cross-platform compatibility

**VM Integration Tests** ([json_vm_integration.rs](tests/json_vm_integration.rs)):
- ✅ Module registration with VM context
- ✅ Function invocation with Value manipulation
- ✅ Type checking and error handling
- ✅ Concurrent module access
- ✅ Multiple function calls with different values

**Total:** 25 tests passing (7 unit + 2 dynamic loading + 6 initialization + 10 VM integration)

## Architecture

```
raya-stdlib/
├── src/
│   ├── lib.rs              # Main entry point
│   ├── json.rs             # Custom JSON implementation
│   ├── json_native.rs      # Native FFI wrapper
│   └── logger.rs           # Logger module implementation
├── tests/
│   ├── json_native.rs      # Module initialization tests
│   └── json_dynamic_loading.rs  # Dynamic loading tests
├── reflect.raya            # Reflection API wrapper
└── Cargo.toml
```

## Dependencies

- `raya-core` - Core VM types and JSON implementation
- `raya-ffi` - FFI bindings, type definitions, library loader, and proc-macros (`#[function]`, `#[module]`)

## Implementation Status

### ✅ Complete
- JSON module native FFI wrapper
- Module initialization and registration
- Dynamic library build configuration
- Comprehensive test suite

### 🚧 Pending (String Marshalling)
The current implementation uses placeholder functions because String marshalling (`FromRaya`/`ToRaya` for `String`) is not yet implemented. Once String support is added:

1. Replace placeholder functions in [json_native.rs](src/json_native.rs)
2. Wire to actual `json::parse()` and `json::stringify()` implementations
3. Add integration tests with real JSON data

## Module Registration

The JSON module is automatically exported with the module name `"std:json"` and can be imported in Raya code as:

```typescript
import * as JSON from "std:json";

const data = JSON.parse('{"name": "Alice"}');
logger.info(data.name);
```

## Future Modules

Additional standard library modules planned:
- `std:fs` - File system operations
- `std:crypto` - Cryptographic functions
- `std:buffer` - Binary data manipulation
- `std:http` - HTTP client/server
- `std:net` - Network primitives
- `std:events` - Event emitter
- `std:stream` - Streaming data

## License

Licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT License ([LICENSE-MIT](../../LICENSE-MIT))

at your option.
