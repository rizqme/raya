//! C FFI bindings for the Raya VM
//!
//! This module provides a C-compatible API for embedding the Raya VM in other languages.
//! The API follows these principles:
//! - ABI-stable (uses only C-compatible types)
//! - Thread-safe (VM instances can be used from multiple threads)
//! - Error handling via out-parameters
//! - Opaque pointers for VM objects
//! - Manual memory management

use crate::compiler::Module;
use crate::vm::value::Value;
use crate::vm::vm::Vm;
use crate::vm::VmError;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::Path;
use std::ptr;

// ============================================================================
// Opaque Types
// ============================================================================

/// Opaque handle to a Raya VM instance
#[repr(C)]
pub struct RayaVM {
    _private: [u8; 0],
}

/// Opaque handle to a Raya value
#[repr(C)]
pub struct RayaValue {
    _private: [u8; 0],
}

/// Opaque handle to a compiled module
#[repr(C)]
pub struct RayaModule {
    _private: [u8; 0],
}

/// Error information
#[repr(C)]
pub struct RayaError {
    message: *mut c_char,
}

// Internal representation of VM (not exposed to C)
struct VmHandle {
    vm: Vm,
}

// Internal representation of Value (not exposed to C)
#[allow(dead_code)]
struct ValueHandle {
    value: Value,
}

// Internal representation of Module (not exposed to C)
struct ModuleHandle {
    module: Module,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert Rust string to C string (caller must free)
unsafe fn rust_to_c_string(s: &str) -> *mut c_char {
    match CString::new(s) {
        Ok(c_str) => c_str.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

/// Create error from string
unsafe fn create_error_str(msg: &str) -> *mut RayaError {
    let message = rust_to_c_string(msg);
    let err = Box::new(RayaError { message });
    Box::into_raw(err)
}

/// Create error from VmError
unsafe fn create_error(error: VmError) -> *mut RayaError {
    create_error_str(&error.to_string())
}

/// Set error out-parameter from string
unsafe fn set_error_str(error_out: *mut *mut RayaError, msg: &str) {
    if !error_out.is_null() {
        *error_out = create_error_str(msg);
    }
}

/// Set error out-parameter from VmError
#[allow(dead_code)]
unsafe fn set_error(error_out: *mut *mut RayaError, error: VmError) {
    if !error_out.is_null() {
        *error_out = create_error(error);
    }
}

// ============================================================================
// VM Lifecycle Functions
// ============================================================================

/// Create a new Raya VM instance
///
/// # Returns
/// * Non-null pointer to RayaVM on success
///
/// # Safety
/// The returned VM must be freed with `raya_vm_destroy()`
#[no_mangle]
pub unsafe extern "C" fn raya_vm_new(_error: *mut *mut RayaError) -> *mut RayaVM {
    let vm = Vm::new();
    let handle = Box::new(VmHandle { vm });
    Box::into_raw(handle) as *mut RayaVM
}

/// Destroy a Raya VM instance and free all resources
///
/// # Arguments
/// * `vm` - Pointer to RayaVM (must not be NULL)
///
/// # Safety
/// - VM pointer must be valid (created by `raya_vm_new()`)
/// - VM must not be used after this call
#[no_mangle]
pub unsafe extern "C" fn raya_vm_destroy(vm: *mut RayaVM) {
    if vm.is_null() {
        return;
    }

    let handle = Box::from_raw(vm as *mut VmHandle);
    drop(handle);
}

/// Load a compiled module from a file
///
/// # Arguments
/// * `path` - Null-terminated path to .rbin file
/// * `error` - Optional pointer to receive error information
///
/// # Returns
/// * Non-null pointer to RayaModule on success
/// * NULL on failure (check error parameter)
///
/// # Safety
/// - Path must be a valid null-terminated string
/// - The returned module must be freed with `raya_module_free()`
#[no_mangle]
pub unsafe extern "C" fn raya_module_load_file(
    path: *const c_char,
    error: *mut *mut RayaError,
) -> *mut RayaModule {
    if path.is_null() {
        set_error_str(error, "Invalid arguments (null path)");
        return ptr::null_mut();
    }

    // Convert C string to Rust string
    let path_str = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => {
            set_error_str(error, "Invalid UTF-8 in path");
            return ptr::null_mut();
        }
    };

    // Read file contents
    let bytes = match std::fs::read(Path::new(path_str)) {
        Ok(b) => b,
        Err(e) => {
            set_error_str(error, &format!("Failed to read file: {}", e));
            return ptr::null_mut();
        }
    };

    // Decode module
    match Module::decode(&bytes) {
        Ok(module) => {
            let handle = Box::new(ModuleHandle { module });
            Box::into_raw(handle) as *mut RayaModule
        }
        Err(e) => {
            set_error_str(error, &format!("Failed to decode module: {}", e));
            ptr::null_mut()
        }
    }
}

/// Load a compiled module from memory
///
/// # Arguments
/// * `bytes` - Pointer to bytecode data
/// * `length` - Length of bytecode data in bytes
/// * `error` - Optional pointer to receive error information
///
/// # Returns
/// * Non-null pointer to RayaModule on success
/// * NULL on failure (check error parameter)
///
/// # Safety
/// - Bytes pointer must be valid for `length` bytes
/// - The returned module must be freed with `raya_module_free()`
#[no_mangle]
pub unsafe extern "C" fn raya_module_load_bytes(
    bytes: *const u8,
    length: usize,
    error: *mut *mut RayaError,
) -> *mut RayaModule {
    if bytes.is_null() {
        set_error_str(error, "Invalid arguments (null bytes)");
        return ptr::null_mut();
    }

    let bytecode = std::slice::from_raw_parts(bytes, length);

    match Module::decode(bytecode) {
        Ok(module) => {
            let handle = Box::new(ModuleHandle { module });
            Box::into_raw(handle) as *mut RayaModule
        }
        Err(e) => {
            set_error_str(error, &format!("Failed to decode module: {}", e));
            ptr::null_mut()
        }
    }
}

/// Free a module
///
/// # Arguments
/// * `module` - Pointer to RayaModule (may be NULL)
///
/// # Safety
/// - Module pointer must be valid (created by raya_module_load_* function)
/// - Module must not be used after this call
#[no_mangle]
pub unsafe extern "C" fn raya_module_free(module: *mut RayaModule) {
    if module.is_null() {
        return;
    }

    let handle = Box::from_raw(module as *mut ModuleHandle);
    drop(handle);
}

/// Execute a module and return the result
///
/// # Arguments
/// * `vm` - Pointer to RayaVM (must not be NULL)
/// * `module` - Pointer to RayaModule (must not be NULL)
/// * `error` - Optional pointer to receive error information
///
/// # Returns
/// * Pointer to RayaValue result on success
/// * NULL on failure (check error parameter)
///
/// # Safety
/// - VM and module pointers must be valid
/// - The returned value must be freed with `raya_value_free()`
#[no_mangle]
pub unsafe extern "C" fn raya_vm_execute(
    vm: *mut RayaVM,
    module: *const RayaModule,
    error: *mut *mut RayaError,
) -> *mut RayaValue {
    if vm.is_null() || module.is_null() {
        set_error_str(error, "Invalid arguments (null pointer)");
        return ptr::null_mut();
    }

    let vm_handle = &mut *(vm as *mut VmHandle);
    let module_handle = &*(module as *const ModuleHandle);

    match vm_handle.vm.execute(&module_handle.module) {
        Ok(value) => {
            let handle = Box::new(ValueHandle { value });
            Box::into_raw(handle) as *mut RayaValue
        }
        Err(e) => {
            set_error(error, e);
            ptr::null_mut()
        }
    }
}

// ============================================================================
// Value Creation Functions
// ============================================================================

/// Create a null value
///
/// # Returns
/// * Pointer to RayaValue representing null
///
/// # Safety
/// The returned value must be freed with `raya_value_free()`
#[no_mangle]
pub unsafe extern "C" fn raya_value_null() -> *mut RayaValue {
    let handle = Box::new(ValueHandle {
        value: Value::null(),
    });
    Box::into_raw(handle) as *mut RayaValue
}

/// Create a boolean value
///
/// # Arguments
/// * `value` - Boolean value (0 = false, non-zero = true)
///
/// # Returns
/// * Pointer to RayaValue representing the boolean
///
/// # Safety
/// The returned value must be freed with `raya_value_free()`
#[no_mangle]
pub unsafe extern "C" fn raya_value_bool(value: c_int) -> *mut RayaValue {
    let handle = Box::new(ValueHandle {
        value: Value::bool(value != 0),
    });
    Box::into_raw(handle) as *mut RayaValue
}

/// Create a 32-bit integer value
///
/// # Arguments
/// * `value` - Integer value
///
/// # Returns
/// * Pointer to RayaValue representing the integer
///
/// # Safety
/// The returned value must be freed with `raya_value_free()`
#[no_mangle]
pub unsafe extern "C" fn raya_value_i32(value: i32) -> *mut RayaValue {
    let handle = Box::new(ValueHandle {
        value: Value::i32(value),
    });
    Box::into_raw(handle) as *mut RayaValue
}

/// Free a value
///
/// # Arguments
/// * `value` - Pointer to RayaValue (may be NULL)
///
/// # Safety
/// - Value pointer must be valid (created by raya_value_* function)
/// - Value must not be used after this call
#[no_mangle]
pub unsafe extern "C" fn raya_value_free(value: *mut RayaValue) {
    if value.is_null() {
        return;
    }

    let handle = Box::from_raw(value as *mut ValueHandle);
    drop(handle);
}

// ============================================================================
// Error Handling Functions
// ============================================================================

/// Get the error message
///
/// # Arguments
/// * `error` - Pointer to RayaError (must not be NULL)
///
/// # Returns
/// * Null-terminated error message string
/// * NULL if error is NULL
///
/// # Safety
/// - Error pointer must be valid
/// - Returned string is valid until `raya_error_free()` is called
#[no_mangle]
pub unsafe extern "C" fn raya_error_message(error: *const RayaError) -> *const c_char {
    if error.is_null() {
        return ptr::null();
    }

    (*error).message
}

/// Free an error
///
/// # Arguments
/// * `error` - Pointer to RayaError (may be NULL)
///
/// # Safety
/// - Error pointer must be valid (created by Raya API)
/// - Error must not be used after this call
#[no_mangle]
pub unsafe extern "C" fn raya_error_free(error: *mut RayaError) {
    if error.is_null() {
        return;
    }

    // Free the message string
    if !(*error).message.is_null() {
        let _ = CString::from_raw((*error).message);
    }

    // Free the error struct
    let _ = Box::from_raw(error);
}

// ============================================================================
// Version Information
// ============================================================================

/// Get the Raya VM version string
///
/// # Returns
/// * Null-terminated version string (e.g., "0.1.0")
///
/// # Safety
/// - The returned string is a static string and must not be freed
#[no_mangle]
pub unsafe extern "C" fn raya_version() -> *const c_char {
    static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
    VERSION.as_ptr() as *const c_char
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_lifecycle() {
        unsafe {
            let mut error: *mut RayaError = ptr::null_mut();

            // Create VM
            let vm = raya_vm_new(&mut error as *mut *mut RayaError);
            assert!(!vm.is_null());
            assert!(error.is_null());

            // Destroy VM
            raya_vm_destroy(vm);
        }
    }

    #[test]
    fn test_value_creation() {
        unsafe {
            // Create null
            let null = raya_value_null();
            assert!(!null.is_null());
            raya_value_free(null);

            // Create bool
            let bool_val = raya_value_bool(1);
            assert!(!bool_val.is_null());
            raya_value_free(bool_val);

            // Create i32
            let int_val = raya_value_i32(42);
            assert!(!int_val.is_null());
            raya_value_free(int_val);
        }
    }

    #[test]
    fn test_version() {
        unsafe {
            let version = raya_version();
            assert!(!version.is_null());

            let version_str = CStr::from_ptr(version).to_str().unwrap();
            assert!(!version_str.is_empty());
        }
    }
}
