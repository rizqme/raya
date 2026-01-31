# raya-native

Proc-macro crate for ergonomic native module development.

## Overview

This crate provides procedural macros that generate FFI boilerplate for native modules:
- `#[function]` - Wraps a Rust function for native module use
- `#[module]` - Defines a module initialization function

## Macros

### `#[function]`

Transforms a regular Rust function into a native module function.

**Input:**
```rust
#[function]
fn add(a: i32, b: i32) -> i32 {
    a + b
}
```

**Generated:**
```rust
fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub extern "C" fn add_ffi(
    args: *const NativeValue,
    arg_count: usize
) -> NativeValue {
    // Argument count validation
    // Type conversion using FromRaya
    // Panic catching
    // Result conversion using ToRaya
}
```

### `#[module]`

Marks a function as the module initializer, generating FFI entry points.

**Input:**
```rust
#[module]
fn init() -> NativeModule {
    let mut module = NativeModule::new("math", "1.0.0");
    module.register_function("add", add_ffi);
    module
}
```

**Generated:**
```rust
fn init() -> NativeModule { /* ... */ }

#[no_mangle]
pub extern "C" fn raya_module_init() -> *mut NativeModule {
    Box::into_raw(Box::new(init()))
}

#[no_mangle]
pub extern "C" fn raya_module_cleanup(module: *mut NativeModule) {
    unsafe { drop(Box::from_raw(module)); }
}
```

## Module Files

```
src/
├── lib.rs       # Proc-macro entry points
├── function.rs  # #[function] macro implementation
├── module.rs    # #[module] macro implementation
└── traits.rs    # Helper code generation
```

## Full Example

```rust
use raya_sdk::NativeModule;
use raya_native::{function, module};

#[function]
fn greet(name: String) -> String {
    format!("Hello, {}!", name)
}

#[function]
fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[module]
fn init() -> NativeModule {
    let mut module = NativeModule::new("example", "1.0.0");
    module.register_function("greet", greet_ffi);
    module.register_function("add", add_ffi);
    module
}
```

## Building a Native Module

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
raya-sdk = { path = "../raya-sdk" }
raya-native = { path = "../raya-native" }
```

```bash
cargo build --release
# Creates: target/release/libexample.so (or .dylib/.dll)
```

## For AI Assistants

- This is a proc-macro crate - uses `syn` and `quote`
- `#[function]` generates `{name}_ffi` wrapper function
- `#[module]` generates `raya_module_init` and `raya_module_cleanup`
- Panic handling is automatic in generated wrappers
- Type conversions use `FromRaya`/`ToRaya` traits from `raya-sdk`
