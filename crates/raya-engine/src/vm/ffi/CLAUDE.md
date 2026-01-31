# ffi module

Foreign Function Interface for native module integration.

## Overview

Provides the bridge between Raya VM and native (Rust/C) code, enabling:
- Native module loading
- Value conversion between Raya and native types
- GC-safe value handling

## Module Structure

```
ffi/
├── mod.rs        # Re-exports, module registration
├── value.rs      # Value conversion
├── module.rs     # Native module loading
├── library.rs    # Dynamic library loading
└── c_api.rs      # C FFI for embedding
```

## Key Types

### NativeModule (from raya-sdk)
```rust
pub struct NativeModule {
    name: String,
    version: String,
    functions: HashMap<String, NativeFn>,
}
```

### NativeFn
```rust
pub type NativeFn = extern "C" fn(
    args: *const NativeValue,
    arg_count: usize
) -> NativeValue;
```

### Library (Dynamic Loading)
```rust
pub struct Library {
    handle: *mut c_void,
    init: extern "C" fn() -> *mut NativeModule,
    cleanup: extern "C" fn(*mut NativeModule),
}

Library::load(path) -> Result<Library, LoadError>
library.init() -> NativeModule
```

## Value Conversion

### Raya Value → NativeValue
```rust
pub fn value_to_native(value: &Value) -> NativeValue {
    match value {
        Value::Null => NativeValue::null(),
        Value::Bool(b) => NativeValue::bool(*b),
        Value::I32(i) => NativeValue::i32(*i),
        Value::F64(f) => NativeValue::f64(*f),
        Value::String(s) => unsafe { NativeValue::from_ptr(s.as_ptr()) },
        Value::Object(o) => unsafe { NativeValue::from_ptr(o.as_ptr()) },
        // ...
    }
}
```

### NativeValue → Raya Value
```rust
pub fn native_to_value(native: NativeValue) -> Value {
    match native.tag() {
        TAG_NULL => Value::Null,
        TAG_BOOL => Value::Bool(native.as_bool().unwrap()),
        TAG_I32 => Value::I32(native.as_i32().unwrap()),
        TAG_F64 => Value::F64(native.as_f64().unwrap()),
        TAG_PTR => {
            // Decode heap pointer type
            let ptr = unsafe { native.as_ptr().unwrap() };
            // ... reconstruct Raya value
        }
    }
}
```

## GC Pinning

Heap values passed to native code must be pinned:

```rust
// Pin value to prevent GC collection
pub fn pin_value(value: &Value) -> PinGuard {
    GC.pin(value.as_ptr())
}

// Unpin when native call returns
pub fn unpin_value(guard: PinGuard) {
    GC.unpin(guard)
}

// Usage in native call dispatch
fn call_native(func: NativeFn, args: &[Value]) -> Value {
    let guards: Vec<_> = args.iter().map(pin_value).collect();
    let native_args: Vec<_> = args.iter().map(value_to_native).collect();

    let result = func(native_args.as_ptr(), native_args.len());

    drop(guards);  // Unpin after call
    native_to_value(result)
}
```

## Module Registration

```rust
static NATIVE_MODULES: Lazy<RwLock<HashMap<String, NativeModule>>> = ...;

pub fn register_native_module(module: NativeModule) {
    NATIVE_MODULES.write().insert(module.name().to_string(), module);
}

pub fn get_native_function(module: &str, name: &str) -> Option<NativeFn> {
    NATIVE_MODULES.read()
        .get(module)?
        .get_function(name)
}
```

## C API (`c_api.rs`)

For embedding Raya in C/C++ applications:

```c
// Create VM
RayaVM* vm = raya_vm_new();

// Load module
RayaModule* module = raya_module_load_file("main.ryb");

// Execute
RayaValue result = raya_vm_execute(vm, module);

// Cleanup
raya_module_free(module);
raya_vm_destroy(vm);
```

## Error Handling

```rust
pub enum NativeError {
    TypeMismatch { expected: String, got: String },
    ArgumentError(String),
    Panic(String),
    ModuleError(String),
}
```

## For AI Assistants

- Native functions use C calling convention (`extern "C"`)
- Values must be pinned during native calls (GC safety)
- NativeValue is a tagged union, NOT a Rust enum
- Dynamic libraries export `raya_module_init` and `raya_module_cleanup`
- Use `raya-sdk` types in native modules (not `raya-engine`)
- C API enables embedding Raya in other languages
