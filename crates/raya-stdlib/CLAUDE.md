# raya-stdlib

Native implementations for Raya's standard library.

## Overview

This crate contains native (Rust) implementations of standard library functions that can't be efficiently implemented in pure Raya, such as:
- Console I/O
- JSON serialization/deserialization
- File system operations (planned)
- Network operations (planned)

## Module Structure

```
src/
├── lib.rs          # Crate entry point
├── console.rs      # Console I/O (print, log, etc.)
├── json.rs         # JSON types and utilities
└── json_native.rs  # Native JSON functions
```

## Current Implementations

### Console (`console.rs`)
- `console.log()` - Print to stdout with newline
- `console.error()` - Print to stderr
- `console.warn()` - Warning messages

### JSON (`json_native.rs`)
- `JSON.parse()` - Parse JSON string to value
- `JSON.stringify()` - Convert value to JSON string
- Type-safe `JSON.decode<T>()` (compile-time specialized)
- Type-safe `JSON.encode<T>()` (compile-time specialized)

## Integration with VM

Native functions are registered using the FFI system:

```rust
use raya_sdk::NativeModule;
use raya_native::{function, module};

#[function]
fn json_parse(input: String) -> NativeValue {
    // Parse JSON and return Raya value
}

#[module]
pub fn init() -> NativeModule {
    let mut module = NativeModule::new("json", "1.0.0");
    module.register_function("parse", json_parse_ffi);
    module
}
```

## Adding New Native Functions

1. Create the function with `#[function]` attribute
2. Register in the module initializer
3. Add corresponding native ID in `raya-engine/src/compiler/native_id.rs`
4. Add dispatch in `raya-engine/src/vm/vm/interpreter.rs`

## Implementation Status

| Module | Status |
|--------|--------|
| console | Complete |
| JSON | Partial |
| fs | Not started |
| net | Not started |
| crypto | Not started |
| os | Not started |

## For AI Assistants

- Use `raya-sdk` and `raya-native` for FFI
- Native IDs must match `compiler/native_id.rs`
- Console output goes through Rust's stdout/stderr
- JSON uses `serde_json` internally
- Keep implementations simple - complex logic should be in Raya
