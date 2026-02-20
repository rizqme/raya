//! AOT frame and context types
//!
//! Defines the C-ABI compatible structures used by AOT-compiled code:
//! - `AotFrame`: heap-allocated per-function state for suspend/resume
//! - `AotTaskContext`: shared context between the scheduler and AOT code
//! - `AotHelperTable`: function pointer table for runtime services
//! - `AotEntryFn`: the standard function signature for all AOT functions

use std::sync::atomic::AtomicBool;

/// Sentinel value returned by AOT functions when they suspend.
/// This is an invalid NaN-box value (not a valid float, i32, bool, null, or ptr).
pub const AOT_SUSPEND: u64 = 0xFFFF_DEAD_0000_0000;

/// AOT function entry point signature.
///
/// Every AOT-compiled function has this exact C-ABI signature:
/// - `frame`: heap-allocated, persists across suspends. Contains locals and resume state.
/// - `ctx`: points into Task and SharedVmState. Contains helpers and preemption flag.
///
/// Returns:
/// - A NaN-boxed u64 value on completion
/// - `AOT_SUSPEND` sentinel when the function suspends
///
/// On suspend, the reason is written to `ctx.suspend_reason`.
pub type AotEntryFn = unsafe extern "C" fn(
    frame: *mut AotFrame,
    ctx: *mut AotTaskContext,
) -> u64;

/// Reason why an AOT function suspended execution.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuspendReason {
    /// Not suspended (initial state)
    None = 0,
    /// Awaiting a spawned task to complete. Payload: task handle (NaN-boxed).
    AwaitTask = 1,
    /// Waiting for I/O completion. Payload: IO request handle.
    IoWait = 2,
    /// Preempted by the scheduler (time slice expired).
    Preempted = 3,
    /// Yielded voluntarily.
    Yielded = 4,
    /// Sleeping for a duration. Payload: duration in milliseconds.
    Sleep = 5,
    /// Waiting on a channel receive.
    ChannelRecv = 6,
    /// Waiting on a channel send (backpressure).
    ChannelSend = 7,
    /// Waiting on a mutex lock.
    MutexLock = 8,
}

/// Heap-allocated frame for each active function call.
///
/// Lives in the task's virtual call stack. When a function suspends,
/// its locals and resume point are preserved here. On resume, the
/// function reloads from this frame and jumps to the saved resume point.
#[repr(C)]
pub struct AotFrame {
    /// Which function this frame belongs to (global function ID).
    pub function_id: u32,

    /// Resume point: 0 = entry, 1+ = continuation after a suspension point.
    pub resume_point: u32,

    /// Pointer to NaN-boxed local variable storage.
    /// Allocated inline after the AotFrame struct for cache locality.
    pub locals: *mut u64,

    /// Number of locals in this frame.
    pub local_count: u32,

    /// Number of parameters (subset of locals).
    pub param_count: u32,

    /// Pointer to the callee frame that suspended (null if no active callee).
    /// When this frame resumes, it first re-enters the child if present.
    pub child_frame: *mut AotFrame,

    /// Pointer to this frame's compiled function for re-entry.
    pub function_ptr: AotEntryFn,

    /// Payload value associated with the suspend reason (e.g., task handle, IO handle).
    pub suspend_payload: u64,
}

/// Task-level context passed to every AOT function.
///
/// Points into the Task struct and SharedVmState. Populated by the
/// scheduler before each call into AOT code.
#[repr(C)]
pub struct AotTaskContext {
    /// Atomic flag: set by reactor when this task should yield.
    pub preempt_requested: *const AtomicBool,

    /// Value provided when resuming from await/IO/channel.
    /// Read by AOT code after re-entry at a suspension point.
    pub resume_value: u64,

    /// Reason this function suspended. Written by AOT code before returning AOT_SUSPEND.
    pub suspend_reason: SuspendReason,

    /// Payload value associated with the suspend reason.
    pub suspend_payload: u64,

    /// Function pointer table for runtime services.
    /// AOT code calls these indirectly — no relocations needed in machine code.
    pub helpers: AotHelperTable,

    /// Opaque pointer to SharedVmState.
    pub shared_state: *mut (),

    /// Opaque pointer to current Task.
    pub current_task: *mut (),

    /// Opaque pointer to the module (for constant pool access, etc.).
    pub module: *const (),
}

/// Function pointer table for all runtime services callable from AOT code.
///
/// AOT code calls through this table (indirect calls) instead of referencing
/// runtime symbols directly. This means the compiled machine code has zero
/// relocations — it works at any load address.
///
/// Same design pattern as the JIT's `RuntimeHelperTable`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AotHelperTable {
    // ---- Frame management ----

    /// Allocate a new AotFrame: (func_id, local_count, func_ptr) -> frame_ptr
    pub alloc_frame: unsafe extern "C" fn(u32, u32, AotEntryFn) -> *mut AotFrame,

    /// Free an AotFrame: (frame_ptr)
    pub free_frame: unsafe extern "C" fn(*mut AotFrame),

    // ---- GC / Heap ----

    /// GC safepoint poll: (ctx)
    pub safepoint_poll: unsafe extern "C" fn(*mut AotTaskContext),

    /// Allocate a new object: (ctx, class_id) -> NaN-boxed ptr
    pub alloc_object: unsafe extern "C" fn(*mut AotTaskContext, u32) -> u64,

    /// Allocate a new array: (ctx, type_id, capacity) -> NaN-boxed ptr
    pub alloc_array: unsafe extern "C" fn(*mut AotTaskContext, u32, u32) -> u64,

    /// Allocate a new string from UTF-8 bytes: (ctx, data_ptr, len) -> NaN-boxed ptr
    pub alloc_string: unsafe extern "C" fn(*mut AotTaskContext, *const u8, u32) -> u64,

    // ---- Value operations ----

    /// String concatenation: (ctx, a, b) -> NaN-boxed string ptr
    pub string_concat: unsafe extern "C" fn(*mut AotTaskContext, u64, u64) -> u64,

    /// String length: (string_val) -> NaN-boxed i32
    pub string_len: unsafe extern "C" fn(u64) -> u64,

    /// Array length: (array_val) -> NaN-boxed i32
    pub array_len: unsafe extern "C" fn(u64) -> u64,

    /// Array get element: (array_val, index_val) -> NaN-boxed element
    pub array_get: unsafe extern "C" fn(u64, u64) -> u64,

    /// Array set element: (array_val, index_val, value)
    pub array_set: unsafe extern "C" fn(u64, u64, u64),

    /// Array push element: (ctx, array_val, value)
    pub array_push: unsafe extern "C" fn(*mut AotTaskContext, u64, u64),

    /// Generic equality comparison: (a, b) -> bool (as i8)
    pub generic_equals: unsafe extern "C" fn(u64, u64) -> u8,

    /// Generic less-than comparison: (a, b) -> bool (as i8)
    pub generic_less_than: unsafe extern "C" fn(u64, u64) -> u8,

    // ---- Object field access ----

    /// Get object field by index: (obj_val, field_index) -> NaN-boxed value
    pub object_get_field: unsafe extern "C" fn(u64, u32) -> u64,

    /// Set object field by index: (obj_val, field_index, value)
    pub object_set_field: unsafe extern "C" fn(u64, u32, u64),

    // ---- Native call dispatch ----

    /// Dispatch a native function call: (ctx, native_id, args_ptr, argc) -> result
    ///
    /// Returns a NaN-boxed value on immediate completion, or a special
    /// suspend token if the native needs I/O.
    pub native_call: unsafe extern "C" fn(*mut AotTaskContext, u16, *const u64, u8) -> u64,

    /// Check if a native call result is a suspend token: (result) -> bool
    pub is_native_suspend: unsafe extern "C" fn(u64) -> u8,

    // ---- Concurrency ----

    /// Spawn a new task: (ctx, func_id, args_ptr, argc) -> NaN-boxed task handle
    pub spawn: unsafe extern "C" fn(*mut AotTaskContext, u32, *const u64, u32) -> u64,

    /// Check if preemption is requested: (ctx) -> should_yield (bool as u8)
    pub check_preemption: unsafe extern "C" fn(*mut AotTaskContext) -> u8,

    // ---- Exceptions ----

    /// Throw an exception: (ctx, exception_value)
    pub throw_exception: unsafe extern "C" fn(*mut AotTaskContext, u64),

    // ---- AOT function dispatch ----

    /// Look up an AOT function pointer by global ID: (func_id) -> AotEntryFn
    ///
    /// Used for indirect calls (closures, virtual dispatch, cross-module calls).
    pub get_aot_func_ptr: unsafe extern "C" fn(u32) -> AotEntryFn,

    // ---- Constant pool access ----

    /// Load a string constant from the module's constant pool: (ctx, const_index) -> NaN-boxed string
    pub load_string_constant: unsafe extern "C" fn(*mut AotTaskContext, u32) -> u64,

    /// Load an i32 constant: (value) -> NaN-boxed i32
    pub load_i32_constant: unsafe extern "C" fn(i32) -> u64,

    /// Load an f64 constant: (value) -> NaN-boxed f64
    pub load_f64_constant: unsafe extern "C" fn(f64) -> u64,
}

// Safety: AotFrame is allocated per-task and only accessed from a single worker thread at a time.
unsafe impl Send for AotFrame {}
// AotFrame is not Sync — it's mutably accessed by AOT code.

// Safety: AotTaskContext is populated from Task+SharedVmState which are Send.
unsafe impl Send for AotTaskContext {}

impl AotFrame {
    /// Check if this frame has an active child (callee that suspended).
    #[inline]
    pub fn has_child(&self) -> bool {
        !self.child_frame.is_null()
    }
}

impl Default for SuspendReason {
    fn default() -> Self {
        SuspendReason::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    #[test]
    fn test_aot_suspend_is_invalid_nan_box() {
        // AOT_SUSPEND must not collide with any valid NaN-boxed value
        let nan_box_base: u64 = 0xFFF8_0000_0000_0000;
        // Valid NaN-box tags use bits 48-50 (0-6), so 0xFFF8..0xFFFE range
        // AOT_SUSPEND (0xFFFF_DEAD_...) has tag bits = 0x7 which is unused
        assert!(AOT_SUSPEND & nan_box_base == nan_box_base);
        let tag = (AOT_SUSPEND >> 48) & 0x7;
        assert_eq!(tag, 0x7); // Tag 7 is not used by any value type
    }

    #[test]
    fn test_frame_layout_is_repr_c() {
        // Ensure the struct is C-compatible (no padding surprises)
        assert!(mem::size_of::<AotFrame>() > 0);
        assert!(mem::align_of::<AotFrame>() <= 8);
    }

    #[test]
    fn test_suspend_reason_values() {
        assert_eq!(SuspendReason::None as u32, 0);
        assert_eq!(SuspendReason::AwaitTask as u32, 1);
        assert_eq!(SuspendReason::Preempted as u32, 3);
    }
}
