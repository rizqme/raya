# Native Bindings - Milestone 1.14

**Status:** Design Complete
**Last Updated:** 2026-01-06
**Implementation:** Pending

---

## Overview

This document specifies the native binding layer for the Raya VM, enabling C, C++, and Rust programs to embed and interact with the Raya runtime. The binding layer provides:

- **C API:** Low-level, ABI-stable interface (foundational layer)
- **C++ API:** High-level, RAII-based wrapper over C API
- **Rust API:** Safe, idiomatic Rust wrapper with zero-cost abstractions

---

## Table of Contents

1. [Design Principles](#design-principles)
2. [Architecture Overview](#architecture-overview)
3. [C API (Foundation Layer)](#c-api-foundation-layer)
4. [C++ API](#c-api-1)
5. [Rust API](#rust-api)
6. [Type Marshalling](#type-marshalling)
7. [Memory Management](#memory-management)
8. [Error Handling](#error-handling)
9. [Thread Safety](#thread-safety)
10. [ABI Stability](#abi-stability)
11. [Build System Integration](#build-system-integration)
12. [Examples](#examples)

---

## Design Principles

### 1. **Layered Architecture**
```
┌─────────────────────────────────┐
│   High-Level APIs (C++/Rust)   │  ← Safe, ergonomic wrappers
├─────────────────────────────────┤
│         C API (FFI)             │  ← ABI-stable, low-level
├─────────────────────────────────┤
│      Raya Core (Rust)           │  ← VM implementation
└─────────────────────────────────┘
```

### 2. **ABI Stability**
- C API uses only C-compatible types
- No Rust-specific types exposed in C headers
- Opaque pointers for all VM objects
- Stable versioning with semantic versioning

### 3. **Safety Boundaries**
- C API: Unsafe, requires careful usage
- C++ API: RAII-based, exception-safe
- Rust API: Compile-time safety guarantees

### 4. **Zero-Cost Abstractions**
- Rust bindings have no runtime overhead
- C++ bindings add minimal overhead (RAII destructors)
- No unnecessary allocations or copies

### 5. **Memory Ownership Clarity**
- Clear ownership semantics at API boundaries
- Explicit transfer vs. borrow in function signatures
- Automatic cleanup via RAII/Drop

---

## Architecture Overview

### Directory Structure

```
crates/
├── raya-core/          # Core VM implementation (Rust)
├── raya-ffi/           # C API bindings (Rust → C)
│   ├── src/
│   │   ├── lib.rs      # C API exports
│   │   ├── vm.rs       # VM lifecycle functions
│   │   ├── value.rs    # Value marshalling
│   │   ├── error.rs    # Error handling
│   │   └── context.rs  # Execution context
│   └── include/
│       └── raya.h      # C header
├── raya-cpp/           # C++ wrapper (C++ over C API)
│   ├── include/
│   │   └── raya.hpp    # C++ header
│   └── src/
│       └── raya.cpp    # C++ implementation
└── raya-rs/            # Rust safe wrapper (Rust over C API)
    └── src/
        └── lib.rs      # Safe Rust API
```

### API Layers

1. **raya-ffi (C API)**
   - Direct FFI exports from Rust
   - `extern "C"` functions
   - Opaque pointer types
   - Manual memory management

2. **raya-cpp (C++ API)**
   - RAII wrappers around C API
   - Exception-based error handling
   - Move semantics
   - STL integration

3. **raya-rs (Rust Safe API)**
   - Safe wrappers around unsafe C API
   - Ownership tracking
   - Result-based error handling
   - Zero-cost abstractions

---

## C API (Foundation Layer)

### Core Types

```c
// raya.h

#ifndef RAYA_H
#define RAYA_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

// Opaque types (defined in Rust)
typedef struct RayaVM RayaVM;
typedef struct RayaValue RayaValue;
typedef struct RayaSnapshot RayaSnapshot;
typedef struct RayaError RayaError;

// Error codes
typedef enum {
    RAYA_OK = 0,
    RAYA_ERR_NULL_POINTER = 1,
    RAYA_ERR_INVALID_ARGUMENT = 2,
    RAYA_ERR_OUT_OF_MEMORY = 3,
    RAYA_ERR_IO = 4,
    RAYA_ERR_EXECUTION = 5,
    RAYA_ERR_TYPE_MISMATCH = 6,
    RAYA_ERR_RESOURCE_LIMIT = 7,
} RayaErrorCode;

// Resource limits
typedef struct {
    uint64_t max_heap_bytes;      // 0 = unlimited
    uint64_t max_tasks;            // 0 = unlimited
    uint64_t max_step_budget;      // 0 = unlimited
} RayaResourceLimits;

// VM options
typedef struct {
    RayaResourceLimits limits;
    uint64_t gc_threshold;
} RayaVMOptions;

// VM statistics
typedef struct {
    uint64_t heap_bytes_used;
    uint64_t max_heap_bytes;
    uint64_t tasks;
    uint64_t max_tasks;
    uint64_t steps_executed;
} RayaVMStats;

// Value types
typedef enum {
    RAYA_TYPE_NULL = 0,
    RAYA_TYPE_BOOL = 1,
    RAYA_TYPE_I32 = 2,
    RAYA_TYPE_U32 = 3,
    RAYA_TYPE_I64 = 4,
    RAYA_TYPE_U64 = 5,
    RAYA_TYPE_F64 = 6,
    RAYA_TYPE_STRING = 7,
    RAYA_TYPE_OBJECT = 8,
} RayaValueType;

// ===== VM Lifecycle =====

// Create a new VM with default options
RayaVM* raya_vm_new(RayaError** error);

// Create a new VM with custom options
RayaVM* raya_vm_new_with_options(const RayaVMOptions* options, RayaError** error);

// Destroy a VM and free all resources
void raya_vm_destroy(RayaVM* vm);

// Load bytecode from file
bool raya_vm_load_file(RayaVM* vm, const char* path, RayaError** error);

// Load bytecode from memory
bool raya_vm_load_bytes(RayaVM* vm, const uint8_t* bytes, size_t len, RayaError** error);

// Execute entry point
bool raya_vm_run_entry(RayaVM* vm, const char* name, RayaError** error);

// Get VM statistics
bool raya_vm_get_stats(const RayaVM* vm, RayaVMStats* stats, RayaError** error);

// Terminate VM
bool raya_vm_terminate(RayaVM* vm, RayaError** error);

// ===== Snapshotting =====

// Create a snapshot of the VM
RayaSnapshot* raya_vm_snapshot(RayaVM* vm, RayaError** error);

// Restore VM from snapshot
bool raya_vm_restore(RayaVM* vm, const RayaSnapshot* snapshot, RayaError** error);

// Create VM from snapshot
RayaVM* raya_vm_from_snapshot(const RayaSnapshot* snapshot,
                               const RayaVMOptions* options,
                               RayaError** error);

// Save snapshot to file
bool raya_snapshot_save(const RayaSnapshot* snapshot, const char* path, RayaError** error);

// Load snapshot from file
RayaSnapshot* raya_snapshot_load(const char* path, RayaError** error);

// Destroy snapshot
void raya_snapshot_destroy(RayaSnapshot* snapshot);

// ===== Value Manipulation =====

// Create values
RayaValue* raya_value_null(void);
RayaValue* raya_value_bool(bool value);
RayaValue* raya_value_i32(int32_t value);
RayaValue* raya_value_u32(uint32_t value);
RayaValue* raya_value_i64(int64_t value);
RayaValue* raya_value_u64(uint64_t value);
RayaValue* raya_value_f64(double value);
RayaValue* raya_value_string(const char* str, size_t len);

// Get value type
RayaValueType raya_value_type(const RayaValue* value);

// Extract values (returns false if type mismatch)
bool raya_value_as_bool(const RayaValue* value, bool* out);
bool raya_value_as_i32(const RayaValue* value, int32_t* out);
bool raya_value_as_u32(const RayaValue* value, uint32_t* out);
bool raya_value_as_i64(const RayaValue* value, int64_t* out);
bool raya_value_as_u64(const RayaValue* value, uint64_t* out);
bool raya_value_as_f64(const RayaValue* value, double* out);

// Get string (returns NULL-terminated string, must not be freed)
const char* raya_value_as_string(const RayaValue* value, size_t* len_out);

// Clone value
RayaValue* raya_value_clone(const RayaValue* value);

// Destroy value
void raya_value_destroy(RayaValue* value);

// ===== Error Handling =====

// Get error code
RayaErrorCode raya_error_code(const RayaError* error);

// Get error message (NULL-terminated, must not be freed)
const char* raya_error_message(const RayaError* error);

// Destroy error
void raya_error_destroy(RayaError* error);

// ===== Versioning =====

// Get library version
const char* raya_version(void);

// Check ABI compatibility
bool raya_check_abi_version(uint32_t major, uint32_t minor);

#ifdef __cplusplus
}
#endif

#endif // RAYA_H
```

### Implementation in Rust (raya-ffi/src/lib.rs)

```rust
//! C FFI bindings for Raya VM

use raya_core::vm::{InnerVm as Vm, VmOptions, VmError, ResourceLimits, VmStats};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::ptr;

// Re-export C types
pub use ffi_types::*;

mod ffi_types {
    use super::*;

    #[repr(C)]
    pub struct RayaResourceLimits {
        pub max_heap_bytes: u64,
        pub max_tasks: u64,
        pub max_step_budget: u64,
    }

    #[repr(C)]
    pub struct RayaVMOptions {
        pub limits: RayaResourceLimits,
        pub gc_threshold: u64,
    }

    #[repr(C)]
    pub struct RayaVMStats {
        pub heap_bytes_used: u64,
        pub max_heap_bytes: u64,
        pub tasks: u64,
        pub max_tasks: u64,
        pub steps_executed: u64,
    }

    #[repr(C)]
    pub enum RayaErrorCode {
        Ok = 0,
        NullPointer = 1,
        InvalidArgument = 2,
        OutOfMemory = 3,
        Io = 4,
        Execution = 5,
        TypeMismatch = 6,
        ResourceLimit = 7,
    }
}

// Opaque types
pub struct RayaVM {
    inner: Vm,
}

pub struct RayaError {
    code: RayaErrorCode,
    message: CString,
}

// ===== VM Lifecycle =====

#[no_mangle]
pub extern "C" fn raya_vm_new(error_out: *mut *mut RayaError) -> *mut RayaVM {
    raya_vm_new_with_options(ptr::null(), error_out)
}

#[no_mangle]
pub extern "C" fn raya_vm_new_with_options(
    options: *const RayaVMOptions,
    error_out: *mut *mut RayaError,
) -> *mut RayaVM {
    let opts = if options.is_null() {
        VmOptions::default()
    } else {
        let opts = unsafe { &*options };
        VmOptions {
            limits: ResourceLimits {
                max_heap_bytes: if opts.limits.max_heap_bytes == 0 {
                    None
                } else {
                    Some(opts.limits.max_heap_bytes as usize)
                },
                max_tasks: if opts.limits.max_tasks == 0 {
                    None
                } else {
                    Some(opts.limits.max_tasks as usize)
                },
                max_step_budget: if opts.limits.max_step_budget == 0 {
                    None
                } else {
                    Some(opts.limits.max_step_budget)
                },
            },
            gc_threshold: opts.gc_threshold as usize,
            ..Default::default()
        }
    };

    match Vm::new(opts) {
        Ok(vm) => Box::into_raw(Box::new(RayaVM { inner: vm })),
        Err(e) => {
            set_error(error_out, RayaErrorCode::Execution, &e.to_string());
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn raya_vm_destroy(vm: *mut RayaVM) {
    if !vm.is_null() {
        unsafe {
            let _ = Box::from_raw(vm);
        }
    }
}

#[no_mangle]
pub extern "C" fn raya_vm_get_stats(
    vm: *const RayaVM,
    stats_out: *mut RayaVMStats,
    error_out: *mut *mut RayaError,
) -> bool {
    if vm.is_null() || stats_out.is_null() {
        set_error(error_out, RayaErrorCode::NullPointer, "NULL pointer");
        return false;
    }

    let vm = unsafe { &*vm };
    match vm.inner.get_stats() {
        Ok(stats) => {
            unsafe {
                (*stats_out) = RayaVMStats {
                    heap_bytes_used: stats.heap_bytes_used as u64,
                    max_heap_bytes: stats.max_heap_bytes as u64,
                    tasks: stats.tasks as u64,
                    max_tasks: stats.max_tasks as u64,
                    steps_executed: stats.steps_executed,
                };
            }
            true
        }
        Err(e) => {
            set_error(error_out, RayaErrorCode::Execution, &e.to_string());
            false
        }
    }
}

// ===== Error Handling =====

#[no_mangle]
pub extern "C" fn raya_error_code(error: *const RayaError) -> RayaErrorCode {
    if error.is_null() {
        return RayaErrorCode::Ok;
    }
    unsafe { (*error).code }
}

#[no_mangle]
pub extern "C" fn raya_error_message(error: *const RayaError) -> *const c_char {
    if error.is_null() {
        return ptr::null();
    }
    unsafe { (*error).message.as_ptr() }
}

#[no_mangle]
pub extern "C" fn raya_error_destroy(error: *mut RayaError) {
    if !error.is_null() {
        unsafe {
            let _ = Box::from_raw(error);
        }
    }
}

// ===== Versioning =====

#[no_mangle]
pub extern "C" fn raya_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}

#[no_mangle]
pub extern "C" fn raya_check_abi_version(major: u32, minor: u32) -> bool {
    // Current version
    const MAJOR: u32 = 1;
    const MINOR: u32 = 0;

    major == MAJOR && minor <= MINOR
}

// Helper functions

fn set_error(error_out: *mut *mut RayaError, code: RayaErrorCode, msg: &str) {
    if error_out.is_null() {
        return;
    }

    let message = CString::new(msg).unwrap_or_else(|_| CString::new("Invalid UTF-8").unwrap());
    let error = Box::new(RayaError { code, message });
    unsafe {
        *error_out = Box::into_raw(error);
    }
}
```

---

## C++ API

### Header (raya-cpp/include/raya.hpp)

```cpp
// raya.hpp

#ifndef RAYA_HPP
#define RAYA_HPP

#include <raya.h>
#include <memory>
#include <string>
#include <vector>
#include <optional>
#include <stdexcept>
#include <cstdint>

namespace raya {

// Forward declarations
class VM;
class Value;
class Snapshot;

// ===== Exception Types =====

class Exception : public std::runtime_error {
public:
    Exception(RayaErrorCode code, const std::string& message)
        : std::runtime_error(message), code_(code) {}

    RayaErrorCode code() const noexcept { return code_; }

private:
    RayaErrorCode code_;
};

class NullPointerException : public Exception {
public:
    explicit NullPointerException(const std::string& msg = "Null pointer")
        : Exception(RAYA_ERR_NULL_POINTER, msg) {}
};

class ExecutionException : public Exception {
public:
    explicit ExecutionException(const std::string& msg)
        : Exception(RAYA_ERR_EXECUTION, msg) {}
};

class TypeMismatchException : public Exception {
public:
    explicit TypeMismatchException(const std::string& msg)
        : Exception(RAYA_ERR_TYPE_MISMATCH, msg) {}
};

// ===== Resource Limits =====

struct ResourceLimits {
    std::optional<uint64_t> max_heap_bytes;
    std::optional<uint64_t> max_tasks;
    std::optional<uint64_t> max_step_budget;

    ResourceLimits() = default;

    RayaResourceLimits to_c() const {
        return RayaResourceLimits{
            max_heap_bytes.value_or(0),
            max_tasks.value_or(0),
            max_step_budget.value_or(0)
        };
    }
};

struct VMOptions {
    ResourceLimits limits;
    uint64_t gc_threshold = 0;

    VMOptions() = default;

    RayaVMOptions to_c() const {
        return RayaVMOptions{
            limits.to_c(),
            gc_threshold
        };
    }
};

struct VMStats {
    uint64_t heap_bytes_used;
    uint64_t max_heap_bytes;
    uint64_t tasks;
    uint64_t max_tasks;
    uint64_t steps_executed;

    static VMStats from_c(const RayaVMStats& stats) {
        return VMStats{
            stats.heap_bytes_used,
            stats.max_heap_bytes,
            stats.tasks,
            stats.max_tasks,
            stats.steps_executed
        };
    }
};

// ===== Value =====

class Value {
public:
    // Constructors
    Value() : ptr_(raya_value_null(), &raya_value_destroy) {}
    explicit Value(bool value) : ptr_(raya_value_bool(value), &raya_value_destroy) {}
    explicit Value(int32_t value) : ptr_(raya_value_i32(value), &raya_value_destroy) {}
    explicit Value(uint32_t value) : ptr_(raya_value_u32(value), &raya_value_destroy) {}
    explicit Value(int64_t value) : ptr_(raya_value_i64(value), &raya_value_destroy) {}
    explicit Value(uint64_t value) : ptr_(raya_value_u64(value), &raya_value_destroy) {}
    explicit Value(double value) : ptr_(raya_value_f64(value), &raya_value_destroy) {}
    explicit Value(const std::string& str)
        : ptr_(raya_value_string(str.data(), str.size()), &raya_value_destroy) {}

    // Copy/move
    Value(const Value& other)
        : ptr_(raya_value_clone(other.ptr_.get()), &raya_value_destroy) {}
    Value(Value&&) = default;
    Value& operator=(const Value& other) {
        if (this != &other) {
            ptr_.reset(raya_value_clone(other.ptr_.get()));
        }
        return *this;
    }
    Value& operator=(Value&&) = default;

    // Type checking
    RayaValueType type() const {
        return raya_value_type(ptr_.get());
    }

    bool is_null() const { return type() == RAYA_TYPE_NULL; }
    bool is_bool() const { return type() == RAYA_TYPE_BOOL; }
    bool is_i32() const { return type() == RAYA_TYPE_I32; }
    bool is_u32() const { return type() == RAYA_TYPE_U32; }
    bool is_i64() const { return type() == RAYA_TYPE_I64; }
    bool is_u64() const { return type() == RAYA_TYPE_U64; }
    bool is_f64() const { return type() == RAYA_TYPE_F64; }
    bool is_string() const { return type() == RAYA_TYPE_STRING; }

    // Conversion
    bool as_bool() const {
        bool out;
        if (!raya_value_as_bool(ptr_.get(), &out)) {
            throw TypeMismatchException("Cannot convert to bool");
        }
        return out;
    }

    int32_t as_i32() const {
        int32_t out;
        if (!raya_value_as_i32(ptr_.get(), &out)) {
            throw TypeMismatchException("Cannot convert to i32");
        }
        return out;
    }

    uint32_t as_u32() const {
        uint32_t out;
        if (!raya_value_as_u32(ptr_.get(), &out)) {
            throw TypeMismatchException("Cannot convert to u32");
        }
        return out;
    }

    int64_t as_i64() const {
        int64_t out;
        if (!raya_value_as_i64(ptr_.get(), &out)) {
            throw TypeMismatchException("Cannot convert to i64");
        }
        return out;
    }

    uint64_t as_u64() const {
        uint64_t out;
        if (!raya_value_as_u64(ptr_.get(), &out)) {
            throw TypeMismatchException("Cannot convert to u64");
        }
        return out;
    }

    double as_f64() const {
        double out;
        if (!raya_value_as_f64(ptr_.get(), &out)) {
            throw TypeMismatchException("Cannot convert to f64");
        }
        return out;
    }

    std::string as_string() const {
        size_t len;
        const char* str = raya_value_as_string(ptr_.get(), &len);
        if (!str) {
            throw TypeMismatchException("Cannot convert to string");
        }
        return std::string(str, len);
    }

    RayaValue* c_ptr() const { return ptr_.get(); }

private:
    std::unique_ptr<RayaValue, decltype(&raya_value_destroy)> ptr_;
};

// ===== Snapshot =====

class Snapshot {
public:
    // Load from file
    static Snapshot load(const std::string& path) {
        RayaError* error = nullptr;
        RayaSnapshot* snapshot = raya_snapshot_load(path.c_str(), &error);
        if (!snapshot) {
            throw_error(error);
        }
        return Snapshot(snapshot);
    }

    // Save to file
    void save(const std::string& path) const {
        RayaError* error = nullptr;
        if (!raya_snapshot_save(ptr_.get(), path.c_str(), &error)) {
            throw_error(error);
        }
    }

    RayaSnapshot* c_ptr() const { return ptr_.get(); }

private:
    friend class VM;

    explicit Snapshot(RayaSnapshot* ptr)
        : ptr_(ptr, &raya_snapshot_destroy) {}

    std::unique_ptr<RayaSnapshot, decltype(&raya_snapshot_destroy)> ptr_;
};

// ===== VM =====

class VM {
public:
    // Constructors
    VM() {
        RayaError* error = nullptr;
        RayaVM* vm = raya_vm_new(&error);
        if (!vm) {
            throw_error(error);
        }
        ptr_.reset(vm);
    }

    explicit VM(const VMOptions& options) {
        RayaError* error = nullptr;
        auto c_opts = options.to_c();
        RayaVM* vm = raya_vm_new_with_options(&c_opts, &error);
        if (!vm) {
            throw_error(error);
        }
        ptr_.reset(vm);
    }

    // From snapshot
    static VM from_snapshot(const Snapshot& snapshot,
                           const std::optional<VMOptions>& options = std::nullopt) {
        RayaError* error = nullptr;
        RayaVMOptions c_opts;
        const RayaVMOptions* opts_ptr = nullptr;
        if (options) {
            c_opts = options->to_c();
            opts_ptr = &c_opts;
        }

        RayaVM* vm = raya_vm_from_snapshot(snapshot.c_ptr(), opts_ptr, &error);
        if (!vm) {
            throw_error(error);
        }
        return VM(vm);
    }

    // No copy, move only
    VM(const VM&) = delete;
    VM& operator=(const VM&) = delete;
    VM(VM&&) = default;
    VM& operator=(VM&&) = default;

    // Load bytecode
    void load_file(const std::string& path) {
        RayaError* error = nullptr;
        if (!raya_vm_load_file(ptr_.get(), path.c_str(), &error)) {
            throw_error(error);
        }
    }

    void load_bytes(const std::vector<uint8_t>& bytes) {
        RayaError* error = nullptr;
        if (!raya_vm_load_bytes(ptr_.get(), bytes.data(), bytes.size(), &error)) {
            throw_error(error);
        }
    }

    // Execute
    void run_entry(const std::string& name) {
        RayaError* error = nullptr;
        if (!raya_vm_run_entry(ptr_.get(), name.c_str(), &error)) {
            throw_error(error);
        }
    }

    // Statistics
    VMStats get_stats() const {
        RayaError* error = nullptr;
        RayaVMStats stats;
        if (!raya_vm_get_stats(ptr_.get(), &stats, &error)) {
            throw_error(error);
        }
        return VMStats::from_c(stats);
    }

    // Snapshotting
    Snapshot snapshot() const {
        RayaError* error = nullptr;
        RayaSnapshot* snap = raya_vm_snapshot(ptr_.get(), &error);
        if (!snap) {
            throw_error(error);
        }
        return Snapshot(snap);
    }

    void restore(const Snapshot& snapshot) {
        RayaError* error = nullptr;
        if (!raya_vm_restore(ptr_.get(), snapshot.c_ptr(), &error)) {
            throw_error(error);
        }
    }

    // Terminate
    void terminate() {
        RayaError* error = nullptr;
        if (!raya_vm_terminate(ptr_.get(), &error)) {
            throw_error(error);
        }
    }

private:
    explicit VM(RayaVM* ptr) : ptr_(ptr, &raya_vm_destroy) {}

    static void throw_error(RayaError* error) {
        if (!error) {
            throw Exception(RAYA_ERR_EXECUTION, "Unknown error");
        }

        auto code = raya_error_code(error);
        std::string message = raya_error_message(error);
        raya_error_destroy(error);

        throw Exception(code, message);
    }

    std::unique_ptr<RayaVM, decltype(&raya_vm_destroy)> ptr_{nullptr, &raya_vm_destroy};
};

// ===== Version Info =====

inline std::string version() {
    return raya_version();
}

inline bool check_abi_version(uint32_t major, uint32_t minor) {
    return raya_check_abi_version(major, minor);
}

} // namespace raya

#endif // RAYA_HPP
```

---

## Rust API

### Safe Wrapper (raya-rs/src/lib.rs)

```rust
//! Safe Rust bindings for Raya VM

use raya_ffi::*;
use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr;

pub use raya_ffi::{RayaErrorCode, RayaValueType};

// ===== Error Types =====

#[derive(Debug)]
pub struct Error {
    code: RayaErrorCode,
    message: String,
}

impl Error {
    fn from_c(error: *mut RayaError) -> Self {
        let code = unsafe { raya_error_code(error) };
        let msg_ptr = unsafe { raya_error_message(error) };
        let message = if msg_ptr.is_null() {
            "Unknown error".to_string()
        } else {
            unsafe { CStr::from_ptr(msg_ptr).to_string_lossy().into_owned() }
        };
        unsafe { raya_error_destroy(error) };

        Self { code, message }
    }

    pub fn code(&self) -> RayaErrorCode {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

// ===== Resource Limits =====

#[derive(Debug, Clone, Default)]
pub struct ResourceLimits {
    pub max_heap_bytes: Option<u64>,
    pub max_tasks: Option<u64>,
    pub max_step_budget: Option<u64>,
}

impl ResourceLimits {
    fn to_c(&self) -> RayaResourceLimits {
        RayaResourceLimits {
            max_heap_bytes: self.max_heap_bytes.unwrap_or(0),
            max_tasks: self.max_tasks.unwrap_or(0),
            max_step_budget: self.max_step_budget.unwrap_or(0),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct VmOptions {
    pub limits: ResourceLimits,
    pub gc_threshold: u64,
}

impl VmOptions {
    fn to_c(&self) -> RayaVMOptions {
        RayaVMOptions {
            limits: self.limits.to_c(),
            gc_threshold: self.gc_threshold,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VmStats {
    pub heap_bytes_used: u64,
    pub max_heap_bytes: u64,
    pub tasks: u64,
    pub max_tasks: u64,
    pub steps_executed: u64,
}

// ===== Value =====

pub struct Value {
    ptr: *mut RayaValue,
}

impl Value {
    pub fn null() -> Self {
        Self { ptr: unsafe { raya_value_null() } }
    }

    pub fn bool(value: bool) -> Self {
        Self { ptr: unsafe { raya_value_bool(value) } }
    }

    pub fn i32(value: i32) -> Self {
        Self { ptr: unsafe { raya_value_i32(value) } }
    }

    pub fn u32(value: u32) -> Self {
        Self { ptr: unsafe { raya_value_u32(value) } }
    }

    pub fn i64(value: i64) -> Self {
        Self { ptr: unsafe { raya_value_i64(value) } }
    }

    pub fn u64(value: u64) -> Self {
        Self { ptr: unsafe { raya_value_u64(value) } }
    }

    pub fn f64(value: f64) -> Self {
        Self { ptr: unsafe { raya_value_f64(value) } }
    }

    pub fn string(value: &str) -> Self {
        Self {
            ptr: unsafe { raya_value_string(value.as_ptr() as *const i8, value.len()) }
        }
    }

    pub fn value_type(&self) -> RayaValueType {
        unsafe { raya_value_type(self.ptr) }
    }

    pub fn as_bool(&self) -> Option<bool> {
        let mut out = false;
        if unsafe { raya_value_as_bool(self.ptr, &mut out) } {
            Some(out)
        } else {
            None
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        let mut out = 0;
        if unsafe { raya_value_as_i32(self.ptr, &mut out) } {
            Some(out)
        } else {
            None
        }
    }

    // ... similar for other types ...
}

impl Clone for Value {
    fn clone(&self) -> Self {
        Self { ptr: unsafe { raya_value_clone(self.ptr) } }
    }
}

impl Drop for Value {
    fn drop(&mut self) {
        unsafe { raya_value_destroy(self.ptr) };
    }
}

// Safety: Value is Send because the underlying C API is thread-safe
unsafe impl Send for Value {}
unsafe impl Sync for Value {}

// ===== VM =====

pub struct Vm {
    ptr: *mut RayaVM,
}

impl Vm {
    pub fn new() -> Result<Self> {
        Self::with_options(VmOptions::default())
    }

    pub fn with_options(options: VmOptions) -> Result<Self> {
        let mut error = ptr::null_mut();
        let c_opts = options.to_c();
        let vm = unsafe { raya_vm_new_with_options(&c_opts, &mut error) };

        if vm.is_null() {
            return Err(Error::from_c(error));
        }

        Ok(Self { ptr: vm })
    }

    pub fn load_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let path_str = CString::new(path.as_ref().to_string_lossy().as_bytes())
            .map_err(|_| Error {
                code: RayaErrorCode::InvalidArgument,
                message: "Invalid path".to_string(),
            })?;

        let mut error = ptr::null_mut();
        if unsafe { raya_vm_load_file(self.ptr, path_str.as_ptr(), &mut error) } {
            Ok(())
        } else {
            Err(Error::from_c(error))
        }
    }

    pub fn load_bytes(&self, bytes: &[u8]) -> Result<()> {
        let mut error = ptr::null_mut();
        if unsafe { raya_vm_load_bytes(self.ptr, bytes.as_ptr(), bytes.len(), &mut error) } {
            Ok(())
        } else {
            Err(Error::from_c(error))
        }
    }

    pub fn run_entry(&self, name: &str) -> Result<()> {
        let name_cstr = CString::new(name).map_err(|_| Error {
            code: RayaErrorCode::InvalidArgument,
            message: "Invalid function name".to_string(),
        })?;

        let mut error = ptr::null_mut();
        if unsafe { raya_vm_run_entry(self.ptr, name_cstr.as_ptr(), &mut error) } {
            Ok(())
        } else {
            Err(Error::from_c(error))
        }
    }

    pub fn get_stats(&self) -> Result<VmStats> {
        let mut stats = RayaVMStats {
            heap_bytes_used: 0,
            max_heap_bytes: 0,
            tasks: 0,
            max_tasks: 0,
            steps_executed: 0,
        };

        let mut error = ptr::null_mut();
        if unsafe { raya_vm_get_stats(self.ptr, &mut stats, &mut error) } {
            Ok(VmStats {
                heap_bytes_used: stats.heap_bytes_used,
                max_heap_bytes: stats.max_heap_bytes,
                tasks: stats.tasks,
                max_tasks: stats.max_tasks,
                steps_executed: stats.steps_executed,
            })
        } else {
            Err(Error::from_c(error))
        }
    }

    pub fn terminate(&self) -> Result<()> {
        let mut error = ptr::null_mut();
        if unsafe { raya_vm_terminate(self.ptr, &mut error) } {
            Ok(())
        } else {
            Err(Error::from_c(error))
        }
    }
}

impl Drop for Vm {
    fn drop(&mut self) {
        unsafe { raya_vm_destroy(self.ptr) };
    }
}

// Safety: Vm is Send/Sync because the underlying C API is thread-safe
unsafe impl Send for Vm {}
unsafe impl Sync for Vm {}

// ===== Version Info =====

pub fn version() -> &'static str {
    unsafe {
        let ptr = raya_version();
        CStr::from_ptr(ptr).to_str().unwrap_or("unknown")
    }
}

pub fn check_abi_version(major: u32, minor: u32) -> bool {
    unsafe { raya_check_abi_version(major, minor) }
}
```

---

## Type Marshalling

### Primitive Types

| Raya Type | C Type | C++ Type | Rust Type |
|-----------|--------|----------|-----------|
| null | - | - | () |
| bool | bool | bool | bool |
| i32 | int32_t | std::int32_t | i32 |
| u32 | uint32_t | std::uint32_t | u32 |
| i64 | int64_t | std::int64_t | i64 |
| u64 | uint64_t | std::uint64_t | u64 |
| f64 | double | double | f64 |
| string | const char* | std::string | &str / String |

### Complex Types

**Objects/Arrays:**
- Pass as opaque `RayaValue*` pointers
- Use accessor functions for field access

**Callbacks:**
- C: Function pointers
- C++: std::function wrappers
- Rust: Fn trait objects

---

## Memory Management

### Ownership Rules

1. **C API:**
   - Functions returning `*` transfer ownership
   - Functions taking `const *` borrow (don't free)
   - Caller must call corresponding `_destroy` function

2. **C++ API:**
   - RAII: Destructors call `_destroy` automatically
   - Move semantics for transferring ownership
   - Copy uses `clone` functions

3. **Rust API:**
   - Drop trait calls `_destroy` automatically
   - Move semantics by default
   - Clone trait for explicit copies

### Example Memory Flow

```
┌─────────────────┐
│  Rust Core VM   │
└────────┬────────┘
         │ Box::into_raw()
         ↓
┌─────────────────┐
│   C API (*ptr)  │  ← Opaque pointer
└────────┬────────┘
         │ unique_ptr<>(ptr, deleter)
         ↓
┌─────────────────┐
│  C++ RAII Obj   │  ← Auto-cleanup
└─────────────────┘
```

---

## Error Handling

### C API
```c
RayaError* error = NULL;
RayaVM* vm = raya_vm_new(&error);
if (!vm) {
    printf("Error: %s\n", raya_error_message(error));
    raya_error_destroy(error);
    return 1;
}
```

### C++ API
```cpp
try {
    raya::VM vm;
    vm.load_file("program.rbin");
    vm.run_entry("main");
} catch (const raya::Exception& e) {
    std::cerr << "Error: " << e.what() << std::endl;
}
```

### Rust API
```rust
let vm = Vm::new()?;
vm.load_file("program.rbin")?;
vm.run_entry("main")?;
```

---

## Thread Safety

### Guarantees

1. **VM instances are thread-safe:**
   - Can be accessed from multiple threads
   - Internal synchronization with locks

2. **Value types are thread-safe:**
   - Immutable once created
   - Safe to share across threads

3. **Snapshots are thread-safe:**
   - Read-only after creation
   - Can be shared across threads

### Synchronization

```cpp
// C++: VM is thread-safe
std::thread t1([&vm]() {
    auto stats = vm.get_stats();
});

std::thread t2([&vm]() {
    vm.run_entry("main");
});
```

---

## ABI Stability

### Versioning Scheme

**Format:** `MAJOR.MINOR.PATCH`

- **MAJOR:** Breaking ABI changes
- **MINOR:** Backward-compatible additions
- **PATCH:** Bug fixes, no API/ABI changes

### Stability Guarantees

1. **Stable across MINOR versions:**
   - v1.0.0 binaries work with v1.1.0 library
   - v1.1.0 binaries work with v1.0.0 library (missing new functions)

2. **Breaking changes require MAJOR bump:**
   - v1.x.x incompatible with v2.x.x
   - Recompilation required

### Checking Compatibility

```c
// C
if (!raya_check_abi_version(1, 0)) {
    fprintf(stderr, "Incompatible ABI version\n");
    exit(1);
}
```

```cpp
// C++
if (!raya::check_abi_version(1, 0)) {
    throw std::runtime_error("Incompatible ABI version");
}
```

---

## Build System Integration

### CMake (C/C++)

```cmake
# CMakeLists.txt

cmake_minimum_required(VERSION 3.15)
project(MyRayaApp)

# Find Raya library
find_package(Raya REQUIRED)

# Add executable
add_executable(myapp main.cpp)

# Link Raya
target_link_libraries(myapp PRIVATE Raya::Raya)

# C++ standard
set_target_properties(myapp PROPERTIES CXX_STANDARD 17)
```

### Cargo (Rust)

```toml
# Cargo.toml

[dependencies]
raya-rs = "1.0"
```

### pkg-config

```bash
# Compile with pkg-config
gcc myapp.c $(pkg-config --cflags --libs raya) -o myapp
```

---

## Examples

### C Example

```c
#include <raya.h>
#include <stdio.h>
#include <stdlib.h>

int main(void) {
    // Create VM
    RayaError* error = NULL;
    RayaVM* vm = raya_vm_new(&error);
    if (!vm) {
        fprintf(stderr, "Failed to create VM: %s\n",
                raya_error_message(error));
        raya_error_destroy(error);
        return 1;
    }

    // Load bytecode
    if (!raya_vm_load_file(vm, "program.rbin", &error)) {
        fprintf(stderr, "Failed to load: %s\n",
                raya_error_message(error));
        raya_error_destroy(error);
        raya_vm_destroy(vm);
        return 1;
    }

    // Execute
    if (!raya_vm_run_entry(vm, "main", &error)) {
        fprintf(stderr, "Execution failed: %s\n",
                raya_error_message(error));
        raya_error_destroy(error);
        raya_vm_destroy(vm);
        return 1;
    }

    // Get stats
    RayaVMStats stats;
    if (raya_vm_get_stats(vm, &stats, &error)) {
        printf("Heap used: %lu bytes\n", stats.heap_bytes_used);
        printf("Tasks: %lu\n", stats.tasks);
    }

    // Cleanup
    raya_vm_destroy(vm);
    return 0;
}
```

### C++ Example

```cpp
#include <raya.hpp>
#include <iostream>

int main() {
    try {
        // Create VM with options
        raya::VMOptions options;
        options.limits.max_heap_bytes = 16 * 1024 * 1024;

        raya::VM vm(options);

        // Load and execute
        vm.load_file("program.rbin");
        vm.run_entry("main");

        // Get stats
        auto stats = vm.get_stats();
        std::cout << "Heap used: " << stats.heap_bytes_used << " bytes\n";
        std::cout << "Tasks: " << stats.tasks << "\n";

        // Snapshot
        auto snapshot = vm.snapshot();
        snapshot.save("vm_state.snap");

        // Restore later
        auto loaded = raya::Snapshot::load("vm_state.snap");
        auto vm2 = raya::VM::from_snapshot(loaded);

    } catch (const raya::Exception& e) {
        std::cerr << "Error: " << e.what() << "\n";
        return 1;
    }

    return 0;
}
```

### Rust Example

```rust
use raya_rs::{Vm, VmOptions, ResourceLimits};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create VM with options
    let options = VmOptions {
        limits: ResourceLimits {
            max_heap_bytes: Some(16 * 1024 * 1024),
            ..Default::default()
        },
        ..Default::default()
    };

    let vm = Vm::with_options(options)?;

    // Load and execute
    vm.load_file("program.rbin")?;
    vm.run_entry("main")?;

    // Get stats
    let stats = vm.get_stats()?;
    println!("Heap used: {} bytes", stats.heap_bytes_used);
    println!("Tasks: {}", stats.tasks);

    Ok(())
}
```

---

## Implementation Checklist

### Phase 1: C API (raya-ffi)
- [ ] Core types and error handling
- [ ] VM lifecycle functions
- [ ] Value marshalling
- [ ] Snapshot functions
- [ ] Build system (cbindgen for header generation)
- [ ] Unit tests
- [ ] Documentation

### Phase 2: C++ API (raya-cpp)
- [ ] RAII wrappers
- [ ] Exception-based error handling
- [ ] STL integration
- [ ] Move semantics
- [ ] Build system (CMake)
- [ ] Unit tests
- [ ] Documentation

### Phase 3: Rust API (raya-rs)
- [ ] Safe wrappers
- [ ] Result-based error handling
- [ ] Ownership tracking
- [ ] Build system (Cargo)
- [ ] Unit tests
- [ ] Documentation

### Phase 4: Integration & Testing
- [ ] Cross-language tests
- [ ] Memory leak tests (valgrind)
- [ ] Thread safety tests
- [ ] ABI compatibility tests
- [ ] Example programs
- [ ] Benchmarks

---

## Future Extensions

1. **More Language Bindings:**
   - Python (via ctypes/CFFI)
   - Go (via cgo)
   - JavaScript/Node.js (via N-API)

2. **Advanced Features:**
   - Custom callbacks from Raya to host
   - Foreign object wrappers
   - JIT compilation hooks

3. **Performance:**
   - Zero-copy data sharing
   - Shared memory snapshots

---

**End of Design Document**
