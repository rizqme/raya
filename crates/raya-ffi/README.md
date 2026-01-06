# raya-ffi

C FFI bindings for the Raya Virtual Machine.

This crate provides a C-compatible API for embedding the Raya VM in other languages (C, C++, Python, etc.).

## Features

- **ABI-stable**: Uses only C-compatible types
- **Thread-safe**: VM instances can be used from multiple threads
- **Error handling**: All functions report errors via out-parameters
- **Opaque pointers**: Internal VM details are hidden
- **Manual memory management**: Explicit resource lifecycle

## API Overview

### Core Types

- `RayaVM`: Opaque handle to a VM instance
- `RayaValue`: Opaque handle to a runtime value
- `RayaSnapshot`: Opaque handle to a VM snapshot
- `RayaError`: Error information with message

### VM Lifecycle

```c
// Create VM
RayaError* error = NULL;
RayaVM* vm = raya_vm_new(&error);

// Load bytecode
raya_vm_load_file(vm, "./program.rbin", &error);

// Run entry point
raya_vm_run_entry(vm, "main", &error);

// Cleanup
raya_vm_destroy(vm);
```

### Error Handling

```c
RayaError* error = NULL;
if (raya_vm_load_file(vm, "./program.rbin", &error) != 0) {
    fprintf(stderr, "Error: %s\n", raya_error_message(error));
    raya_error_free(error);
    return 1;
}
```

### Snapshotting

```c
// Create snapshot
RayaSnapshot* snapshot = raya_vm_snapshot(vm, &error);

// Restore from snapshot
raya_vm_restore(vm, snapshot, &error);

// Free snapshot (if not consumed by restore)
raya_snapshot_free(snapshot);
```

## Building

### As a Shared Library (.so / .dylib / .dll)

```bash
cargo build --release -p raya-ffi
```

The shared library will be at:
- Linux: `target/release/libraya_ffi.so`
- macOS: `target/release/libraya_ffi.dylib`
- Windows: `target/release/raya_ffi.dll`

### As a Static Library (.a / .lib)

```bash
cargo build --release -p raya-ffi
```

The static library will be at:
- Unix: `target/release/libraya_ffi.a`
- Windows: `target/release/raya_ffi.lib`

## Usage in C

### With GCC/Clang

```bash
gcc -o myapp myapp.c -L./target/release -lraya_ffi -I./crates/raya-ffi/include
```

### With CMake

```cmake
add_library(raya_ffi SHARED IMPORTED)
set_target_properties(raya_ffi PROPERTIES
    IMPORTED_LOCATION ${CMAKE_SOURCE_DIR}/target/release/libraya_ffi.so
    INTERFACE_INCLUDE_DIRECTORIES ${CMAKE_SOURCE_DIR}/crates/raya-ffi/include
)

target_link_libraries(myapp PRIVATE raya_ffi)
```

### With pkg-config

```bash
export PKG_CONFIG_PATH=/path/to/raya/target/release
gcc -o myapp myapp.c $(pkg-config --cflags --libs raya)
```

## Examples

See [examples/hello.c](examples/hello.c) for a complete example:

```bash
# Build the example
gcc -o hello crates/raya-ffi/examples/hello.c \
    -L./target/release \
    -lraya_ffi \
    -I./crates/raya-ffi/include

# Run it
LD_LIBRARY_PATH=./target/release ./hello
```

## API Documentation

Complete API documentation is available in [include/raya.h](include/raya.h).

## ABI Stability

The C API follows semantic versioning:

- **MAJOR version**: Breaking ABI changes (incompatible)
- **MINOR version**: New functions (backward compatible)
- **PATCH version**: Bug fixes (no API/ABI changes)

Current version: **0.1.0**

## Thread Safety

All VM instances are fully thread-safe and can be used from multiple threads simultaneously. Internal synchronization ensures safe concurrent access.

## Memory Management

- All functions that return pointers transfer ownership to the caller
- Caller must free resources using the appropriate `*_free()` functions
- NULL pointers are safe to pass to `*_free()` functions (no-op)
- Double-free is safe (idempotent)

## Error Handling

- Most functions accept an optional `RayaError**` parameter
- If an error occurs, the function returns an error code (typically -1 or NULL)
- Retrieve error message with `raya_error_message()`
- Always free errors with `raya_error_free()`

## License

MIT OR Apache-2.0

## See Also

- [design/NATIVE_BINDINGS.md](../../design/NATIVE_BINDINGS.md) - Complete design specification
- [PLAN.md](../../plans/PLAN.md) - Implementation roadmap (Milestone 1.14)
