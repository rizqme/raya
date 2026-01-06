//! C FFI bindings for the Raya VM
//!
//! This module provides a C-compatible API for embedding the Raya VM in other languages.
//! The API follows these principles:
//! - ABI-stable (uses only C-compatible types)
//! - Thread-safe (VM instances can be used from multiple threads)
//! - Error handling via out-parameters
//! - Opaque pointers for VM objects
//! - Manual memory management

use raya_core::value::Value;
use raya_core::vm::{InnerVm, VmError, VmOptions, VmSnapshot};
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

/// Opaque handle to a VM snapshot
#[repr(C)]
pub struct RayaSnapshot {
    _private: [u8; 0],
}

/// Error information
#[repr(C)]
pub struct RayaError {
    message: *mut c_char,
}

// Internal representation of VM (not exposed to C)
struct VmHandle {
    vm: InnerVm,
}

// Internal representation of Value (not exposed to C)
struct ValueHandle {
    value: Value,
}

// Internal representation of Snapshot (not exposed to C)
struct SnapshotHandle {
    snapshot: VmSnapshot,
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

/// Create error from VmError
unsafe fn create_error(error: VmError) -> *mut RayaError {
    let message = rust_to_c_string(&error.to_string());
    let err = Box::new(RayaError { message });
    Box::into_raw(err)
}

/// Set error out-parameter
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
/// # Arguments
/// * `error` - Optional pointer to receive error information
///
/// # Returns
/// * Non-null pointer to RayaVM on success
/// * NULL on failure (check error parameter)
///
/// # Safety
/// The returned VM must be freed with `raya_vm_destroy()`
///
/// # Example (C)
/// ```c
/// RayaError* error = NULL;
/// RayaVM* vm = raya_vm_new(&error);
/// if (vm == NULL) {
///     fprintf(stderr, "Failed to create VM: %s\n", raya_error_message(error));
///     raya_error_free(error);
///     return 1;
/// }
/// // Use VM...
/// raya_vm_destroy(vm);
/// ```
#[no_mangle]
pub unsafe extern "C" fn raya_vm_new(error: *mut *mut RayaError) -> *mut RayaVM {
    match InnerVm::new(VmOptions::default()) {
        Ok(vm) => {
            let handle = Box::new(VmHandle { vm });
            Box::into_raw(handle) as *mut RayaVM
        }
        Err(e) => {
            set_error(error, e);
            ptr::null_mut()
        }
    }
}

/// Destroy a Raya VM instance and free all resources
///
/// # Arguments
/// * `vm` - Pointer to RayaVM (must not be NULL)
///
/// # Safety
/// - VM pointer must be valid (created by `raya_vm_new()`)
/// - VM must not be used after this call
/// - This function is idempotent (safe to call multiple times)
#[no_mangle]
pub unsafe extern "C" fn raya_vm_destroy(vm: *mut RayaVM) {
    if vm.is_null() {
        return;
    }

    let handle = Box::from_raw(vm as *mut VmHandle);
    drop(handle);
}

/// Load a .rbin bytecode file into the VM
///
/// # Arguments
/// * `vm` - Pointer to RayaVM (must not be NULL)
/// * `path` - Null-terminated path to .rbin file
/// * `error` - Optional pointer to receive error information
///
/// # Returns
/// * 0 on success
/// * -1 on failure (check error parameter)
///
/// # Safety
/// - VM pointer must be valid
/// - Path must be a valid null-terminated string
///
/// # Example (C)
/// ```c
/// RayaError* error = NULL;
/// if (raya_vm_load_file(vm, "./program.rbin", &error) != 0) {
///     fprintf(stderr, "Failed to load file: %s\n", raya_error_message(error));
///     raya_error_free(error);
///     return 1;
/// }
/// ```
#[no_mangle]
pub unsafe extern "C" fn raya_vm_load_file(
    vm: *mut RayaVM,
    path: *const c_char,
    error: *mut *mut RayaError,
) -> c_int {
    if vm.is_null() || path.is_null() {
        if !error.is_null() {
            set_error(
                error,
                VmError::ExecutionError("Invalid arguments (null pointer)".to_string()),
            );
        }
        return -1;
    }

    let handle = &mut *(vm as *mut VmHandle);

    // Convert C string to Rust string
    let path_str = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => {
            set_error(
                error,
                VmError::ExecutionError("Invalid UTF-8 in path".to_string()),
            );
            return -1;
        }
    };

    // Load the file
    match handle.vm.load_rbin(Path::new(path_str)) {
        Ok(_) => 0,
        Err(e) => {
            set_error(error, e);
            -1
        }
    }
}

/// Load bytecode from memory
///
/// # Arguments
/// * `vm` - Pointer to RayaVM (must not be NULL)
/// * `bytes` - Pointer to bytecode data
/// * `length` - Length of bytecode data in bytes
/// * `error` - Optional pointer to receive error information
///
/// # Returns
/// * 0 on success
/// * -1 on failure (check error parameter)
///
/// # Safety
/// - VM pointer must be valid
/// - Bytes pointer must be valid for `length` bytes
#[no_mangle]
pub unsafe extern "C" fn raya_vm_load_bytes(
    vm: *mut RayaVM,
    bytes: *const u8,
    length: usize,
    error: *mut *mut RayaError,
) -> c_int {
    if vm.is_null() || bytes.is_null() {
        if !error.is_null() {
            set_error(
                error,
                VmError::ExecutionError("Invalid arguments (null pointer)".to_string()),
            );
        }
        return -1;
    }

    let handle = &mut *(vm as *mut VmHandle);
    let bytecode = std::slice::from_raw_parts(bytes, length);

    match handle.vm.load_rbin_bytes(bytecode) {
        Ok(_) => 0,
        Err(e) => {
            set_error(error, e);
            -1
        }
    }
}

/// Run an entry point function
///
/// # Arguments
/// * `vm` - Pointer to RayaVM (must not be NULL)
/// * `name` - Null-terminated function name (e.g., "main")
/// * `error` - Optional pointer to receive error information
///
/// # Returns
/// * 0 on success
/// * -1 on failure (check error parameter)
///
/// # Safety
/// - VM pointer must be valid
/// - Name must be a valid null-terminated string
///
/// # Example (C)
/// ```c
/// RayaError* error = NULL;
/// if (raya_vm_run_entry(vm, "main", &error) != 0) {
///     fprintf(stderr, "Execution failed: %s\n", raya_error_message(error));
///     raya_error_free(error);
///     return 1;
/// }
/// ```
#[no_mangle]
pub unsafe extern "C" fn raya_vm_run_entry(
    vm: *mut RayaVM,
    name: *const c_char,
    error: *mut *mut RayaError,
) -> c_int {
    if vm.is_null() || name.is_null() {
        if !error.is_null() {
            set_error(
                error,
                VmError::ExecutionError("Invalid arguments (null pointer)".to_string()),
            );
        }
        return -1;
    }

    let handle = &mut *(vm as *mut VmHandle);

    // Convert C string to Rust string
    let name_str = match CStr::from_ptr(name).to_str() {
        Ok(s) => s,
        Err(_) => {
            set_error(
                error,
                VmError::ExecutionError("Invalid UTF-8 in function name".to_string()),
            );
            return -1;
        }
    };

    // Run the entry point
    match handle.vm.run_entry(name_str, vec![]) {
        Ok(_task_id) => 0, // TODO: Return task ID when async execution is supported
        Err(e) => {
            set_error(error, e);
            -1
        }
    }
}

/// Terminate the VM and stop all running tasks
///
/// # Arguments
/// * `vm` - Pointer to RayaVM (must not be NULL)
/// * `error` - Optional pointer to receive error information
///
/// # Returns
/// * 0 on success
/// * -1 on failure (check error parameter)
///
/// # Safety
/// - VM pointer must be valid
/// - VM can still be used after termination (to load new code)
#[no_mangle]
pub unsafe extern "C" fn raya_vm_terminate(
    vm: *mut RayaVM,
    error: *mut *mut RayaError,
) -> c_int {
    if vm.is_null() {
        if !error.is_null() {
            set_error(
                error,
                VmError::ExecutionError("Invalid arguments (null pointer)".to_string()),
            );
        }
        return -1;
    }

    let handle = &mut *(vm as *mut VmHandle);

    match handle.vm.terminate() {
        Ok(_) => 0,
        Err(e) => {
            set_error(error, e);
            -1
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
/// - Do not free the returned string directly
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
// Snapshot Functions
// ============================================================================

/// Create a snapshot of the VM state
///
/// # Arguments
/// * `vm` - Pointer to RayaVM (must not be NULL)
/// * `error` - Optional pointer to receive error information
///
/// # Returns
/// * Pointer to RayaSnapshot on success
/// * NULL on failure (check error parameter)
///
/// # Safety
/// - VM pointer must be valid
/// - The returned snapshot must be freed with `raya_snapshot_free()`
#[no_mangle]
pub unsafe extern "C" fn raya_vm_snapshot(
    vm: *mut RayaVM,
    error: *mut *mut RayaError,
) -> *mut RayaSnapshot {
    if vm.is_null() {
        if !error.is_null() {
            set_error(
                error,
                VmError::ExecutionError("Invalid arguments (null pointer)".to_string()),
            );
        }
        return ptr::null_mut();
    }

    let handle = &mut *(vm as *mut VmHandle);

    match handle.vm.snapshot() {
        Ok(snapshot) => {
            let snap_handle = Box::new(SnapshotHandle { snapshot });
            Box::into_raw(snap_handle) as *mut RayaSnapshot
        }
        Err(e) => {
            set_error(error, e);
            ptr::null_mut()
        }
    }
}

/// Restore VM state from a snapshot
///
/// # Arguments
/// * `vm` - Pointer to RayaVM (must not be NULL)
/// * `snapshot` - Pointer to RayaSnapshot (must not be NULL)
/// * `error` - Optional pointer to receive error information
///
/// # Returns
/// * 0 on success
/// * -1 on failure (check error parameter)
///
/// # Safety
/// - VM and snapshot pointers must be valid
/// - The snapshot is consumed and must not be used after this call
#[no_mangle]
pub unsafe extern "C" fn raya_vm_restore(
    vm: *mut RayaVM,
    snapshot: *mut RayaSnapshot,
    error: *mut *mut RayaError,
) -> c_int {
    if vm.is_null() || snapshot.is_null() {
        if !error.is_null() {
            set_error(
                error,
                VmError::ExecutionError("Invalid arguments (null pointer)".to_string()),
            );
        }
        return -1;
    }

    let handle = &mut *(vm as *mut VmHandle);
    let snap_handle = Box::from_raw(snapshot as *mut SnapshotHandle);

    match handle.vm.restore(snap_handle.snapshot) {
        Ok(_) => 0,
        Err(e) => {
            set_error(error, e);
            -1
        }
    }
}

/// Free a snapshot
///
/// # Arguments
/// * `snapshot` - Pointer to RayaSnapshot (may be NULL)
///
/// # Safety
/// - Snapshot pointer must be valid (created by `raya_vm_snapshot()`)
/// - Snapshot must not be used after this call
#[no_mangle]
pub unsafe extern "C" fn raya_snapshot_free(snapshot: *mut RayaSnapshot) {
    if snapshot.is_null() {
        return;
    }

    let handle = Box::from_raw(snapshot as *mut SnapshotHandle);
    drop(handle);
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
    fn test_error_handling() {
        unsafe {
            let mut error: *mut RayaError = ptr::null_mut();

            // Try to load with null VM (should fail)
            let result = raya_vm_load_file(
                ptr::null_mut(),
                b"test.rbin\0".as_ptr() as *const c_char,
                &mut error as *mut *mut RayaError,
            );

            assert_eq!(result, -1);
            assert!(!error.is_null());

            // Get error message
            let message = raya_error_message(error);
            assert!(!message.is_null());

            // Free error
            raya_error_free(error);
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
