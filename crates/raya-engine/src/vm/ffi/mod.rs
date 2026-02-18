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

#[allow(unused_imports)]
use crate::vm::value::Value;

/// Convert VM Value to SDK NativeValue (zero-cost — same NaN-boxing)
#[inline(always)]
pub fn value_to_native(value: Value) -> NativeValue {
    crate::vm::abi::value_to_native(value)
}

/// Convert SDK NativeValue to VM Value (zero-cost — same NaN-boxing)
#[inline(always)]
pub fn native_to_value(native: NativeValue) -> Value {
    crate::vm::abi::native_to_value(native)
}
