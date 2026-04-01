//! JIT trampolines and calling convention
//!
//! Defines the C-ABI interface between JIT-compiled code and the VM runtime.
//! JIT code calls back into the runtime through function pointers in
//! `RuntimeHelperTable` for GC, allocation, kernel calls, etc.

use crate::vm::suspend::{BackendCallResult, SuspendTag};

/// Entry point signature for JIT-compiled functions
///
/// JIT code receives arguments as NaN-boxed Value array, a locals buffer,
/// and a context pointer containing runtime helpers.
pub type JitEntryFn = unsafe extern "C" fn(
    args: *const u64, // NaN-boxed Value array
    arg_count: u32,
    locals: *mut u64, // pre-allocated locals
    local_count: u32,
    ctx: *mut RuntimeContext,
    exit_info: *mut JitExitInfo,
) -> u64; // returns NaN-boxed Value

/// Exit state for a JIT function invocation.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JitExitKind {
    Completed = 0,
    Suspended = 1,
    Failed = 2,
}

/// Minimal native-frame snapshot to support resume plumbing.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct JitMachineFrameSnapshot {
    /// Native instruction pointer / continuation marker.
    pub resume_ip: u64,
    /// Native stack pointer captured at exit (if available).
    pub stack_ptr: u64,
    /// Native frame/base pointer captured at exit (if available).
    pub frame_ptr: u64,
}

/// Maximum operand materialization supported when handing control back to the interpreter.
pub const JIT_EXIT_MAX_NATIVE_ARGS: usize = 32;

/// Out-parameter written by JIT entry to describe exit behavior.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct JitExitInfo {
    /// How execution exited.
    pub kind: u32,
    /// Suspension tag discriminator (VM-specific; 0 = none).
    pub suspend_tag: u32,
    /// Bytecode offset for resume (if relevant).
    pub bytecode_offset: u32,
    /// Reserved for alignment/extension.
    pub _reserved: u32,
    /// Captured native frame metadata.
    pub frame: JitMachineFrameSnapshot,
    /// Materialized operand count for compiled suspend/resume handoff.
    pub native_arg_count: u32,
    /// Reserved for alignment/extension.
    pub _native_reserved: u32,
    /// Materialized operands (NaN-boxed values) for interpreter resume.
    pub native_args: [u64; JIT_EXIT_MAX_NATIVE_ARGS],
}

impl Default for JitExitInfo {
    fn default() -> Self {
        Self {
            kind: JitExitKind::Completed as u32,
            suspend_tag: SuspendTag::None as u32,
            bytecode_offset: 0,
            _reserved: 0,
            frame: JitMachineFrameSnapshot::default(),
            native_arg_count: 0,
            _native_reserved: 0,
            native_args: [0; JIT_EXIT_MAX_NATIVE_ARGS],
        }
    }
}

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
    /// Allocate a new nominal object: (local_nominal_type_index, module_ptr, shared_state) -> obj_ptr
    pub alloc_object: unsafe extern "C" fn(u32, *const (), *mut ()) -> *mut (),
    /// Allocate a new array: (type_id, capacity, shared_state) -> array_ptr
    pub alloc_array: unsafe extern "C" fn(u32, usize, *mut ()) -> *mut (),
    /// Allocate a new string: (data_ptr, len, shared_state) -> string_ptr
    pub alloc_string: unsafe extern "C" fn(*const u8, usize, *mut ()) -> *mut (),
    /// GC safepoint poll: (shared_state)
    pub safepoint_poll: unsafe extern "C" fn(*const ()),
    /// Check if current task should be preempted: (current_task) -> should_yield
    pub check_preemption: unsafe extern "C" fn(*const ()) -> bool,
    /// Dispatch a kernel call through the shared compiled-backend ABI.
    pub kernel_call_dispatch:
        unsafe extern "C" fn(u16, *const u64, u8, *const (), *mut ()) -> BackendCallResult,
    /// Execute a call-family opcode through the interpreter runtime using the
    /// shared compiled-backend ABI.
    pub interpreter_call:
        unsafe extern "C" fn(
            u8,
            u64,
            u32,
            u64,
            *const u64,
            u16,
            *const (),
            *mut (),
        ) -> BackendCallResult,
    /// Throw an exception through the shared runtime ABI.
    pub throw_exception: unsafe extern "C" fn(u64, *mut ()),
    /// Reserved helper slot kept only for table layout stability.
    pub reserved0: usize,
    /// String concatenation: (left_val, right_val, shared_state) -> result_val
    pub string_concat: unsafe extern "C" fn(u64, u64, *mut ()) -> u64,
    /// Generic equality: (left_val, right_val, shared_state) -> bool
    pub generic_equals: unsafe extern "C" fn(u64, u64, *mut ()) -> bool,
    /// Structural/nominal field load: (obj_val, expected_slot, func_id, module_ptr, shared_state) -> result_val
    pub object_get_field: unsafe extern "C" fn(u64, u32, u32, *const (), *mut ()) -> u64,
    /// Structural/nominal field store: (obj_val, expected_slot, value, func_id, module_ptr, shared_state) -> success
    pub object_set_field: unsafe extern "C" fn(u64, u32, u64, u32, *const (), *mut ()) -> bool,
    /// Structural shape check: (obj_val, shape_id, shared_state) -> implements
    pub object_implements_shape: unsafe extern "C" fn(u64, u64, *mut ()) -> bool,
    /// Nominal type check: (obj_val, local_nominal_type_index, module_ptr, shared_state) -> matches
    pub object_is_nominal: unsafe extern "C" fn(u64, u32, *const (), *mut ()) -> bool,
    /// Shape-aware field load: (obj_val, shape_id, expected_slot, optional, func_id, module_ptr, shared_state) -> result/sentinel
    pub object_get_shape_field:
        unsafe extern "C" fn(u64, u64, u32, u8, u32, *const (), *mut ()) -> u64,
    /// Shape-aware field store: (obj_val, shape_id, expected_slot, value, func_id, module_ptr, shared_state) -> status
    pub object_set_shape_field:
        unsafe extern "C" fn(u64, u64, u32, u64, u32, *const (), *mut ()) -> i8,
    /// String length: (string_val, shared_state) -> len or i32::MIN fallback sentinel
    pub string_len: unsafe extern "C" fn(u64, *mut ()) -> i32,
    /// Execute an exact numeric intrinsic for compiled backends.
    pub numeric_intrinsic: unsafe extern "C" fn(u16, u64, u64) -> u64,
}
