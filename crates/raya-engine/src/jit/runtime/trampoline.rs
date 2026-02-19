//! JIT trampolines and calling convention
//!
//! Defines the C-ABI interface between JIT-compiled code and the VM runtime.
//! JIT code calls back into the runtime through function pointers in
//! `RuntimeHelperTable` for GC, allocation, native calls, etc.

/// Entry point signature for JIT-compiled functions
///
/// JIT code receives arguments as NaN-boxed Value array, a locals buffer,
/// and a context pointer containing runtime helpers.
pub type JitEntryFn = unsafe extern "C" fn(
    args: *const u64,        // NaN-boxed Value array
    arg_count: u32,
    locals: *mut u64,        // pre-allocated locals
    local_count: u32,
    ctx: *mut RuntimeContext,
) -> u64; // returns NaN-boxed Value

/// Runtime context passed to JIT-compiled code
///
/// Contains opaque pointers to VM state and a table of helper function pointers
/// that JIT code can call for runtime services.
#[repr(C)]
pub struct RuntimeContext {
    /// Pointer to SharedVmState
    pub shared_state: *const (),
    /// Pointer to current Task
    pub current_task: *const (),
    /// Pointer to Module
    pub module: *const (),
    /// Table of runtime helper function pointers
    pub helpers: RuntimeHelperTable,
}

/// C-ABI function pointer table for runtime helpers
///
/// JIT code calls these through the RuntimeContext to interact with the VM.
/// All functions take raw pointers and NaN-boxed u64 values.
#[repr(C)]
pub struct RuntimeHelperTable {
    /// Allocate a new object: (class_id, shared_state) -> obj_ptr
    pub alloc_object: unsafe extern "C" fn(u32, *mut ()) -> *mut (),
    /// Allocate a new array: (type_id, capacity, shared_state) -> array_ptr
    pub alloc_array: unsafe extern "C" fn(u32, usize, *mut ()) -> *mut (),
    /// Allocate a new string: (data_ptr, len, shared_state) -> string_ptr
    pub alloc_string: unsafe extern "C" fn(*const u8, usize, *mut ()) -> *mut (),
    /// GC safepoint poll: (shared_state)
    pub safepoint_poll: unsafe extern "C" fn(*const ()),
    /// Check if current task should be preempted: (current_task) -> should_yield
    pub check_preemption: unsafe extern "C" fn(*const ()) -> bool,
    /// Dispatch a native call: (native_id, args_ptr, arg_count, shared_state) -> result
    pub native_call_dispatch: unsafe extern "C" fn(u16, *const u64, u8, *mut ()) -> u64,
    /// Call an interpreted function: (func_index, args_ptr, arg_count, shared_state) -> result
    pub interpreter_call: unsafe extern "C" fn(u32, *const u64, u16, *mut ()) -> u64,
    /// Throw an exception: (exception_value, shared_state) -> !
    pub throw_exception: unsafe extern "C" fn(u64, *mut ()),
    /// Deoptimize: (bytecode_offset, shared_state) -> !
    pub deoptimize: unsafe extern "C" fn(u32, *mut ()),
    /// String concatenation: (left_val, right_val, shared_state) -> result_val
    pub string_concat: unsafe extern "C" fn(u64, u64, *mut ()) -> u64,
    /// Generic equality: (left_val, right_val, shared_state) -> bool
    pub generic_equals: unsafe extern "C" fn(u64, u64, *mut ()) -> bool,
}
