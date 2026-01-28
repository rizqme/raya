//! FFI support for native modules and C bindings
//!
//! This module provides:
//! - Native module support (NativeModule, NativeFn, etc.) - re-exported from raya-sdk
//! - Dynamic library loading (Library)
//! - C FFI bindings (raya_vm_new, raya_vm_execute, etc.)
//! - Value conversion between raya-sdk types and VM types

pub mod c_api;
pub mod loader;
mod native;

// Re-export SDK types for backward compatibility and ease of use
pub use raya_sdk::{FromRaya, NativeError, NativeFn, NativeModule, NativeValue, ToRaya};

// Re-export VM-specific functions (GC pinning, module registration)
pub use native::{pin_value, register_native_module, unpin_value};

// Re-export loader types
pub use loader::{Library, LoadError};

// Re-export C API types
pub use c_api::{
    raya_error_free, raya_error_message, raya_module_free, raya_module_load_bytes,
    raya_module_load_file, raya_value_bool, raya_value_free, raya_value_i32, raya_value_null,
    raya_version, raya_vm_destroy, raya_vm_execute, raya_vm_new, RayaError, RayaModule, RayaValue,
    RayaVM,
};

// ============================================================================
// Value Conversion Utilities
// ============================================================================

use crate::vm::value::Value;

/// Convert VM Value to SDK NativeValue
pub fn value_to_native(value: Value) -> NativeValue {
    match value {
        v if v.is_null() => NativeValue::null(),
        v if v.as_bool().is_some() => NativeValue::bool(v.as_bool().unwrap()),
        v if v.as_i32().is_some() => NativeValue::i32(v.as_i32().unwrap()),
        v if v.as_i64().is_some() => NativeValue::i64(v.as_i64().unwrap()),
        v if v.as_f64().is_some() => NativeValue::f64(v.as_f64().unwrap()),
        _ => {
            // For complex types (strings, objects), use pointer storage
            let boxed = Box::new(value);
            unsafe { NativeValue::from_ptr(Box::into_raw(boxed) as *mut ()) }
        }
    }
}

/// Convert SDK NativeValue to VM Value
///
/// # Safety
/// For pointer values, the caller must ensure the pointer is valid and
/// was created by `value_to_native`.
pub unsafe fn native_to_value(native: NativeValue) -> Value {
    if native.is_null() {
        return Value::null();
    }
    if let Some(b) = native.as_bool() {
        return Value::bool(b);
    }
    if let Some(i) = native.as_i32() {
        return Value::i32(i);
    }
    if let Some(i) = native.as_i64() {
        return Value::i64(i);
    }
    if let Some(f) = native.as_f64() {
        return Value::f64(f);
    }
    // Pointer type - retrieve the boxed Value
    if let Some(ptr) = native.as_ptr() {
        return *Box::from_raw(ptr as *mut Value);
    }
    // Fallback
    Value::null()
}
