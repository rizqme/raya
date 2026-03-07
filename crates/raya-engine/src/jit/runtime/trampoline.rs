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
    Deoptimized = 2,
    Failed = 3,
}

/// Suspension reasons written into `JitExitInfo.suspend_reason`.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JitSuspendReason {
    None = 0,
    Preemption = 1,
    NativeCallBoundary = 2,
    InterpreterCallBoundary = 3,
}

/// Minimal native-frame snapshot to support resume/deopt plumbing.
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
    /// Suspension reason discriminator (VM-specific; 0 = none).
    pub suspend_reason: u32,
    /// Bytecode offset for deopt/resume (if relevant).
    pub bytecode_offset: u32,
    /// Reserved for alignment/extension.
    pub _reserved: u32,
    /// Captured native frame metadata.
    pub frame: JitMachineFrameSnapshot,
    /// Materialized operand count for interpreter-boundary resume handoff.
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
            suspend_reason: JitSuspendReason::None as u32,
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
    /// Structural/nominal field load: (obj_val, expected_slot, func_id, module_ptr, shared_state) -> result_val
    pub object_get_field: unsafe extern "C" fn(u64, u32, u32, *const (), *mut ()) -> u64,
    /// Structural/nominal field store: (obj_val, expected_slot, value, func_id, module_ptr, shared_state) -> success
    pub object_set_field: unsafe extern "C" fn(u64, u32, u64, u32, *const (), *mut ()) -> bool,
    /// Structural shape check: (obj_val, shape_id, shared_state) -> implements
    pub object_implements_shape: unsafe extern "C" fn(u64, u64, *mut ()) -> bool,
}
