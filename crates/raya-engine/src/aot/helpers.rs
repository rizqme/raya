#![allow(missing_docs)]
//! AOT runtime helper implementations
//!
//! These are the `unsafe extern "C"` functions that AOT-compiled code calls
//! through the `AotHelperTable`. They bridge between generated native code
//! and the Raya runtime.
//!
//! Helper categories:
//! - **Frame management**: alloc/free AotFrames (fully implemented)
//! - **NaN-boxing constants**: box i32/f64 values (fully implemented)
//! - **Value operations**: comparison, string/array ops (stubs for now)
//! - **GC/Heap**: allocation (requires runtime integration)
//! - **Concurrency**: spawn, preemption (requires scheduler integration)
//! - **Native calls**: dispatch (requires native function table)

use std::alloc::{self, Layout};
use std::ptr;

use super::abi;
use super::frame::{AotEntryFn, AotFrame, AotHelperTable, AotTaskContext};

// =============================================================================
// Frame management
// =============================================================================

/// Allocate a new AotFrame with inline locals storage.
///
/// Layout: [AotFrame struct][u64 * local_count]
/// The `locals` pointer points to the inline storage right after the struct.
///
/// # Safety
///
/// Caller must ensure `local_count` produces a valid allocation size and
/// that the returned pointer is freed with the matching layout.
unsafe extern "C" fn helper_alloc_frame(
    func_id: u32,
    local_count: u32,
    func_ptr: AotEntryFn,
) -> *mut AotFrame {
    let frame_size = std::mem::size_of::<AotFrame>();
    let locals_size = (local_count as usize) * std::mem::size_of::<u64>();
    let total_size = frame_size + locals_size;
    let align = std::mem::align_of::<AotFrame>();

    let layout = Layout::from_size_align(total_size, align)
        .expect("Invalid frame layout");

    let ptr = alloc::alloc_zeroed(layout) as *mut AotFrame;
    if ptr.is_null() {
        alloc::handle_alloc_error(layout);
    }

    // Initialize the frame
    let locals_ptr = (ptr as *mut u8).add(frame_size) as *mut u64;

    (*ptr).function_id = func_id;
    (*ptr).resume_point = 0;
    (*ptr).locals = locals_ptr;
    (*ptr).local_count = local_count;
    (*ptr).param_count = 0; // Set by caller
    (*ptr).child_frame = ptr::null_mut();
    (*ptr).function_ptr = func_ptr;
    (*ptr).suspend_payload = 0;

    // Zero-initialize locals (already done by alloc_zeroed, but explicit for clarity)
    for i in 0..local_count as usize {
        *locals_ptr.add(i) = abi::NULL_VALUE; // Initialize locals to null
    }

    ptr
}

/// Free an AotFrame allocated by `helper_alloc_frame`.
unsafe extern "C" fn helper_free_frame(frame: *mut AotFrame) {
    if frame.is_null() {
        return;
    }

    let frame_size = std::mem::size_of::<AotFrame>();
    let locals_size = ((*frame).local_count as usize) * std::mem::size_of::<u64>();
    let total_size = frame_size + locals_size;
    let align = std::mem::align_of::<AotFrame>();

    let layout = Layout::from_size_align(total_size, align)
        .expect("Invalid frame layout for dealloc");

    alloc::dealloc(frame as *mut u8, layout);
}

// =============================================================================
// GC / Heap (stubs — require runtime GC integration)
// =============================================================================

unsafe extern "C" fn helper_safepoint_poll(_ctx: *mut AotTaskContext) {
    // TODO: Check GC safepoint, trigger collection if needed
}

unsafe extern "C" fn helper_alloc_object(_ctx: *mut AotTaskContext, _class_id: u32) -> u64 {
    // TODO: Allocate object via GC
    abi::NULL_VALUE
}

unsafe extern "C" fn helper_alloc_array(
    _ctx: *mut AotTaskContext,
    _type_id: u32,
    _capacity: u32,
) -> u64 {
    // TODO: Allocate array via GC
    abi::NULL_VALUE
}

unsafe extern "C" fn helper_alloc_string(
    _ctx: *mut AotTaskContext,
    _data_ptr: *const u8,
    _len: u32,
) -> u64 {
    // TODO: Allocate string via GC
    abi::NULL_VALUE
}

// =============================================================================
// Value operations (stubs — require value system integration)
// =============================================================================

unsafe extern "C" fn helper_string_concat(
    _ctx: *mut AotTaskContext,
    _a: u64,
    _b: u64,
) -> u64 {
    // TODO: Concatenate two NaN-boxed strings
    abi::NULL_VALUE
}

unsafe extern "C" fn helper_string_len(_val: u64) -> u64 {
    // TODO: Get string length, return NaN-boxed i32
    // For now, return 0
    abi::I32_TAG_BASE // NaN-boxed 0
}

unsafe extern "C" fn helper_array_len(_val: u64) -> u64 {
    // TODO: Get array length, return NaN-boxed i32
    abi::I32_TAG_BASE // NaN-boxed 0
}

unsafe extern "C" fn helper_array_get(_array: u64, _index: u64) -> u64 {
    // TODO: Array element access
    abi::NULL_VALUE
}

unsafe extern "C" fn helper_array_set(_array: u64, _index: u64, _value: u64) {
    // TODO: Array element store
}

unsafe extern "C" fn helper_array_push(
    _ctx: *mut AotTaskContext,
    _array: u64,
    _value: u64,
) {
    // TODO: Array push
}

unsafe extern "C" fn helper_generic_equals(a: u64, b: u64) -> u8 {
    // Simple equality: raw bit comparison
    // TODO: Proper deep equality with type-aware comparison
    if a == b { 1 } else { 0 }
}

unsafe extern "C" fn helper_generic_less_than(a: u64, b: u64) -> u8 {
    // Simple comparison: treat as f64 if both are plain f64 (below NaN-box base)
    // TODO: Proper type-aware comparison
    let base = abi::NAN_BOX_BASE;
    if a < base && b < base {
        // Both are f64 — compare as f64
        let fa = f64::from_bits(a);
        let fb = f64::from_bits(b);
        if fa < fb { 1 } else { 0 }
    } else {
        0
    }
}

// =============================================================================
// Object field access (stubs)
// =============================================================================

unsafe extern "C" fn helper_object_get_field(_obj: u64, _field_index: u32) -> u64 {
    // TODO: Object field access via GC heap
    abi::NULL_VALUE
}

unsafe extern "C" fn helper_object_set_field(_obj: u64, _field_index: u32, _value: u64) {
    // TODO: Object field store via GC heap
}

// =============================================================================
// Native call dispatch (stub)
// =============================================================================

unsafe extern "C" fn helper_native_call(
    _ctx: *mut AotTaskContext,
    _native_id: u16,
    _args_ptr: *const u64,
    _argc: u8,
) -> u64 {
    // TODO: Dispatch to native function implementation
    abi::NULL_VALUE
}

unsafe extern "C" fn helper_is_native_suspend(_result: u64) -> u8 {
    // TODO: Check if a native call result is a suspend token
    0
}

// =============================================================================
// Concurrency (stubs)
// =============================================================================

unsafe extern "C" fn helper_spawn(
    _ctx: *mut AotTaskContext,
    _func_id: u32,
    _args_ptr: *const u64,
    _argc: u32,
) -> u64 {
    // TODO: Spawn a new task on the scheduler
    abi::NULL_VALUE
}

unsafe extern "C" fn helper_check_preemption(_ctx: *mut AotTaskContext) -> u8 {
    // TODO: Check the preempt_requested atomic flag
    0
}

// =============================================================================
// Exceptions (stub)
// =============================================================================

unsafe extern "C" fn helper_throw_exception(_ctx: *mut AotTaskContext, _value: u64) {
    // TODO: Throw exception through the runtime
    panic!("AOT throw_exception not yet implemented");
}

// =============================================================================
// AOT function dispatch (stub)
// =============================================================================

unsafe extern "C" fn helper_get_aot_func_ptr(_func_id: u32) -> AotEntryFn {
    // TODO: Look up function pointer from the loaded code region
    // For now, return a trap function
    helper_trap_fn
}

/// Placeholder function for unresolved AOT calls.
unsafe extern "C" fn helper_trap_fn(
    _frame: *mut AotFrame,
    _ctx: *mut AotTaskContext,
) -> u64 {
    panic!("AOT function call to unresolved function");
}

// =============================================================================
// Constant pool access
// =============================================================================

unsafe extern "C" fn helper_load_string_constant(
    _ctx: *mut AotTaskContext,
    _const_index: u32,
) -> u64 {
    // TODO: Load from module constant pool
    abi::NULL_VALUE
}

/// Box an i32 value into a NaN-boxed u64.
unsafe extern "C" fn helper_load_i32_constant(value: i32) -> u64 {
    let payload = (value as u32) as u64;
    abi::I32_TAG_BASE | (payload & abi::PAYLOAD_MASK_32)
}

/// Box an f64 value into a NaN-boxed u64.
unsafe extern "C" fn helper_load_f64_constant(value: f64) -> u64 {
    let bits = value.to_bits();
    // Check for NaN-box collision
    if (bits & abi::NAN_BOX_BASE) == abi::NAN_BOX_BASE {
        // Canonical positive quiet NaN
        0x7FF8_0000_0000_0000
    } else {
        bits
    }
}

// =============================================================================
// Helper table construction
// =============================================================================

/// Create a fully populated `AotHelperTable` with all helper function pointers.
///
/// This is the default table used when no runtime is connected. Frame management
/// and NaN-boxing helpers work correctly; other helpers are stubs.
pub fn create_default_helper_table() -> AotHelperTable {
    AotHelperTable {
        alloc_frame: helper_alloc_frame,
        free_frame: helper_free_frame,
        safepoint_poll: helper_safepoint_poll,
        alloc_object: helper_alloc_object,
        alloc_array: helper_alloc_array,
        alloc_string: helper_alloc_string,
        string_concat: helper_string_concat,
        string_len: helper_string_len,
        array_len: helper_array_len,
        array_get: helper_array_get,
        array_set: helper_array_set,
        array_push: helper_array_push,
        generic_equals: helper_generic_equals,
        generic_less_than: helper_generic_less_than,
        object_get_field: helper_object_get_field,
        object_set_field: helper_object_set_field,
        native_call: helper_native_call,
        is_native_suspend: helper_is_native_suspend,
        spawn: helper_spawn,
        check_preemption: helper_check_preemption,
        throw_exception: helper_throw_exception,
        get_aot_func_ptr: helper_get_aot_func_ptr,
        load_string_constant: helper_load_string_constant,
        load_i32_constant: helper_load_i32_constant,
        load_f64_constant: helper_load_f64_constant,
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_and_free_frame() {
        unsafe {
            let frame = helper_alloc_frame(42, 4, helper_trap_fn);
            assert!(!frame.is_null());

            assert_eq!((*frame).function_id, 42);
            assert_eq!((*frame).resume_point, 0);
            assert_eq!((*frame).local_count, 4);
            assert!((*frame).child_frame.is_null());

            // Check locals are initialized to null
            for i in 0..4 {
                let local = *(*frame).locals.add(i);
                assert_eq!(local, abi::NULL_VALUE, "Local {} should be null", i);
            }

            // Write and read locals
            *(*frame).locals.add(0) = abi::I32_TAG_BASE | 100;
            assert_eq!(*(*frame).locals.add(0), abi::I32_TAG_BASE | 100);

            helper_free_frame(frame);
        }
    }

    #[test]
    fn test_free_null_frame() {
        unsafe {
            // Should not panic
            helper_free_frame(ptr::null_mut());
        }
    }

    #[test]
    fn test_load_i32_constant() {
        unsafe {
            let boxed = helper_load_i32_constant(42);
            assert_eq!(boxed, abi::I32_TAG_BASE | 42);

            let boxed_zero = helper_load_i32_constant(0);
            assert_eq!(boxed_zero, abi::I32_TAG_BASE);

            let boxed_neg = helper_load_i32_constant(-1);
            // -1 as u32 is 0xFFFFFFFF
            assert_eq!(boxed_neg, abi::I32_TAG_BASE | 0xFFFF_FFFF);
        }
    }

    #[test]
    fn test_load_f64_constant() {
        unsafe {
            let boxed = helper_load_f64_constant(3.14);
            let unboxed = f64::from_bits(boxed);
            assert!((unboxed - 3.14).abs() < f64::EPSILON);

            let boxed_zero = helper_load_f64_constant(0.0);
            assert_eq!(f64::from_bits(boxed_zero), 0.0);
        }
    }

    #[test]
    fn test_generic_equals() {
        unsafe {
            assert_eq!(helper_generic_equals(42, 42), 1);
            assert_eq!(helper_generic_equals(42, 43), 0);
            assert_eq!(helper_generic_equals(abi::NULL_VALUE, abi::NULL_VALUE), 1);
        }
    }

    #[test]
    fn test_create_helper_table() {
        let table = create_default_helper_table();

        // Verify the table is populated (all function pointers should be non-null)
        // We can test by calling through the table
        unsafe {
            let frame = (table.alloc_frame)(1, 2, helper_trap_fn);
            assert!(!frame.is_null());
            assert_eq!((*frame).function_id, 1);
            assert_eq!((*frame).local_count, 2);
            (table.free_frame)(frame);

            let i32_val = (table.load_i32_constant)(42);
            assert_eq!(i32_val, abi::I32_TAG_BASE | 42);

            let eq = (table.generic_equals)(100, 100);
            assert_eq!(eq, 1);
        }
    }
}
