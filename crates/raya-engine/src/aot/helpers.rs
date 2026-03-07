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
use std::sync::atomic::Ordering;

use super::abi;
use super::frame::{
    AotEntryFn, AotFrame, AotHelperTable, AotTaskContext, SuspendReason, AOT_SUSPEND,
};
use crate::vm::abi::{native_to_value, value_to_native, EngineContext};
use crate::vm::interpreter::SharedVmState;
use crate::vm::object::Object;
use crate::vm::scheduler::{IoSubmission, Task};
use crate::vm::value::Value;
use raya_sdk::NativeCallResult;

/// Temporary marker native ID for exercising suspend handoff in default AOT helpers.
/// Real runtime dispatch will replace this stub behavior.
const STUB_NATIVE_SUSPEND_ID: u16 = u16::MAX;

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

    let layout = Layout::from_size_align(total_size, align).expect("Invalid frame layout");

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

    let layout =
        Layout::from_size_align(total_size, align).expect("Invalid frame layout for dealloc");

    alloc::dealloc(frame as *mut u8, layout);
}

// =============================================================================
// GC / Heap (stubs — require runtime GC integration)
// =============================================================================

unsafe extern "C" fn helper_safepoint_poll(_ctx: *mut AotTaskContext) {
    // TODO: Check GC safepoint, trigger collection if needed
}

unsafe extern "C" fn helper_alloc_object(
    ctx: *mut AotTaskContext,
    local_nominal_type_index: u32,
) -> u64 {
    if ctx.is_null() || (*ctx).shared_state.is_null() || (*ctx).module.is_null() {
        return abi::NULL_VALUE;
    }
    let shared = &*((*ctx).shared_state as *const SharedVmState);
    let module = &*((*ctx).module as *const crate::compiler::Module);
    let Some(nominal_type_id) =
        shared.resolve_nominal_type_id(module, local_nominal_type_index as usize)
    else {
        return abi::NULL_VALUE;
    };
    let (field_count, layout_id) = {
        let layouts = shared.layouts.read();
        let Some((layout_id, field_count)) = layouts.nominal_allocation(nominal_type_id) else {
            return abi::NULL_VALUE;
        };
        (field_count, layout_id)
    };
    let mut gc = shared.gc.lock();
    let obj_ptr = gc.allocate(Object::new_nominal(
        layout_id,
        nominal_type_id as u32,
        field_count,
    ));
    let value = Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap());
    value.raw()
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

unsafe extern "C" fn helper_string_concat(_ctx: *mut AotTaskContext, _a: u64, _b: u64) -> u64 {
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

unsafe extern "C" fn helper_array_push(_ctx: *mut AotTaskContext, _array: u64, _value: u64) {
    // TODO: Array push
}

unsafe extern "C" fn helper_generic_equals(a: u64, b: u64) -> u8 {
    // Simple equality: raw bit comparison
    // TODO: Proper deep equality with type-aware comparison
    if a == b {
        1
    } else {
        0
    }
}

unsafe extern "C" fn helper_generic_less_than(a: u64, b: u64) -> u8 {
    // Simple comparison: treat as f64 if both are plain f64 (below NaN-box base)
    // TODO: Proper type-aware comparison
    let base = abi::NAN_BOX_BASE;
    if a < base && b < base {
        // Both are f64 — compare as f64
        let fa = f64::from_bits(a);
        let fb = f64::from_bits(b);
        if fa < fb {
            1
        } else {
            0
        }
    } else {
        0
    }
}

// =============================================================================
// Object field access (stubs)
// =============================================================================

unsafe extern "C" fn helper_object_get_field(obj: u64, field_index: u32) -> u64 {
    let value = Value::from_raw(obj);
    let Some(obj_ptr) = value.as_ptr::<Object>() else {
        return abi::NULL_VALUE;
    };
    let obj = &*obj_ptr.as_ptr();
    obj.get_field(field_index as usize)
        .unwrap_or(Value::null())
        .raw()
}

unsafe extern "C" fn helper_object_set_field(obj: u64, field_index: u32, value: u64) {
    let object = Value::from_raw(obj);
    let Some(obj_ptr) = object.as_ptr::<Object>() else {
        return;
    };
    let obj = &mut *obj_ptr.as_ptr();
    let _ = obj.set_field(field_index as usize, Value::from_raw(value));
}

// =============================================================================
// Native call dispatch (stub)
// =============================================================================

unsafe extern "C" fn helper_native_call(
    ctx: *mut AotTaskContext,
    native_id: u16,
    args_ptr: *const u64,
    argc: u8,
) -> u64 {
    if !ctx.is_null() && !(*ctx).shared_state.is_null() {
        let shared = &*((*ctx).shared_state as *const SharedVmState);

        // Build engine context for native handler dispatch
        let task_id = if !(*ctx).current_task.is_null() {
            (*((*ctx).current_task as *const Task)).id()
        } else {
            // Fallback for tests/partial contexts without a task pointer.
            crate::vm::scheduler::TaskId::from_u64(0)
        };
        let engine_ctx = EngineContext::new(
            &shared.gc,
            &shared.classes,
            &shared.layouts,
            task_id,
            &shared.class_metadata,
        );

        // Convert NaN-boxed args into NativeValue slice.
        let value_args: Vec<Value> = if argc == 0 {
            Vec::new()
        } else if args_ptr.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(args_ptr, argc as usize)
                .iter()
                .copied()
                .map(|raw| Value::from_raw(raw))
                .collect()
        };
        let native_args: Vec<raya_sdk::NativeValue> =
            value_args.iter().map(|v| value_to_native(*v)).collect();

        // Dispatch through the module-scoped resolved native table when available.
        let resolved = if !(*ctx).module.is_null() {
            let module = &*((*ctx).module as *const crate::compiler::Module);
            shared
                .module_layouts
                .read()
                .get(&module.checksum)
                .map(|layout| layout.resolved_natives.clone())
                .unwrap_or_else(crate::vm::native_registry::ResolvedNatives::empty)
        } else {
            // Context-less helper path used in unit tests / partial runtimes.
            shared.resolved_natives.read().clone()
        };
        match resolved.call(native_id, &engine_ctx, &native_args) {
            NativeCallResult::Value(val) => return native_to_value(val).raw(),
            NativeCallResult::Suspend(io_request) => {
                if let Some(tx) = shared.io_submit_tx.lock().as_ref() {
                    let _ = tx.send(IoSubmission {
                        task_id,
                        request: io_request,
                    });
                }
                (*ctx).suspend_reason = SuspendReason::IoWait;
                (*ctx).suspend_payload = 0;
                return AOT_SUSPEND;
            }
            NativeCallResult::Unhandled | NativeCallResult::Error(_) => {
                // Fall through to stub behavior below for now.
            }
        }
    }

    // Stub split behavior:
    // - Most IDs take an immediate "completed" fast path (null result placeholder).
    // - STUB_NATIVE_SUSPEND_ID exercises boundary suspend handoff behavior.
    if native_id == STUB_NATIVE_SUSPEND_ID {
        if !ctx.is_null() {
            (*ctx).suspend_reason = SuspendReason::NativeCallBoundary;
            (*ctx).suspend_payload = 0;
        }
        AOT_SUSPEND
    } else {
        abi::NULL_VALUE
    }
}

unsafe extern "C" fn helper_is_native_suspend(result: u64) -> u8 {
    if result == AOT_SUSPEND {
        1
    } else {
        0
    }
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

unsafe extern "C" fn helper_check_preemption(ctx: *mut AotTaskContext) -> u8 {
    if ctx.is_null() {
        return 0;
    }
    let flag_ptr = (*ctx).preempt_requested;
    if flag_ptr.is_null() {
        return 0;
    }
    if (*flag_ptr).load(Ordering::Relaxed) {
        1
    } else {
        0
    }
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
unsafe extern "C" fn helper_trap_fn(_frame: *mut AotFrame, _ctx: *mut AotTaskContext) -> u64 {
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
    use crate::compiler::bytecode::{ClassDef, Module};
    use crate::vm::interpreter::SafepointCoordinator;
    use crate::vm::interpreter::SharedVmState;
    use crate::vm::native_registry::ResolvedNatives;
    use crossbeam::channel::unbounded;
    use crossbeam_deque::Injector;
    use parking_lot::RwLock;
    use raya_sdk::IoRequest;
    use rustc_hash::FxHashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

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

    #[test]
    fn test_native_call_marks_boundary_suspend() {
        let preempt = AtomicBool::new(false);
        let mut ctx = AotTaskContext {
            preempt_requested: &preempt,
            resume_value: abi::NULL_VALUE,
            suspend_reason: SuspendReason::None,
            suspend_payload: 99,
            helpers: create_default_helper_table(),
            shared_state: ptr::null_mut(),
            current_task: ptr::null_mut(),
            module: ptr::null(),
        };
        let result =
            unsafe { helper_native_call(&mut ctx, STUB_NATIVE_SUSPEND_ID, ptr::null(), 0) };
        assert_eq!(result, AOT_SUSPEND);
        assert_eq!(ctx.suspend_reason, SuspendReason::NativeCallBoundary);
        assert_eq!(ctx.suspend_payload, 0);
        assert_eq!(unsafe { helper_is_native_suspend(result) }, 1);
    }

    #[test]
    fn test_native_call_fast_path_returns_immediate_value() {
        let preempt = AtomicBool::new(false);
        let mut ctx = AotTaskContext {
            preempt_requested: &preempt,
            resume_value: abi::NULL_VALUE,
            suspend_reason: SuspendReason::None,
            suspend_payload: 123,
            helpers: create_default_helper_table(),
            shared_state: ptr::null_mut(),
            current_task: ptr::null_mut(),
            module: ptr::null(),
        };
        let result = unsafe { helper_native_call(&mut ctx, 42, ptr::null(), 0) };
        assert_eq!(result, abi::NULL_VALUE);
        assert_eq!(unsafe { helper_is_native_suspend(result) }, 0);
        assert_eq!(ctx.suspend_reason, SuspendReason::None);
        assert_eq!(ctx.suspend_payload, 123);
    }

    #[test]
    fn test_check_preemption_reads_flag() {
        let preempt = AtomicBool::new(false);
        let mut ctx = AotTaskContext {
            preempt_requested: &preempt,
            resume_value: abi::NULL_VALUE,
            suspend_reason: SuspendReason::None,
            suspend_payload: 0,
            helpers: create_default_helper_table(),
            shared_state: ptr::null_mut(),
            current_task: ptr::null_mut(),
            module: ptr::null(),
        };
        assert_eq!(unsafe { helper_check_preemption(&mut ctx) }, 0);
        preempt.store(true, Ordering::Relaxed);
        assert_eq!(unsafe { helper_check_preemption(&mut ctx) }, 1);
    }

    #[test]
    fn test_native_call_uses_resolved_natives_when_shared_state_available() {
        let safepoint = Arc::new(SafepointCoordinator::new(1));
        let tasks = Arc::new(RwLock::new(FxHashMap::default()));
        let injector = Arc::new(Injector::new());
        let shared = Arc::new(SharedVmState::new(safepoint, tasks, injector));

        {
            let mut reg = shared.native_registry.write();
            reg.register("test.native.value", |_ctx, _args| NativeCallResult::i32(77));
            let resolved = ResolvedNatives::link(&["test.native.value".to_string()], &reg)
                .expect("link resolved natives");
            *shared.resolved_natives.write() = resolved;
        }

        let preempt = AtomicBool::new(false);
        let mut ctx = AotTaskContext {
            preempt_requested: &preempt,
            resume_value: abi::NULL_VALUE,
            suspend_reason: SuspendReason::None,
            suspend_payload: 0,
            helpers: create_default_helper_table(),
            shared_state: Arc::as_ptr(&shared) as *mut (),
            current_task: ptr::null_mut(),
            module: ptr::null(),
        };

        let raw = unsafe { helper_native_call(&mut ctx, 0, ptr::null(), 0) };
        assert_eq!(raw, Value::i32(77).raw());
        assert_eq!(ctx.suspend_reason, SuspendReason::None);
    }

    #[test]
    fn test_native_call_with_args_uses_resolved_natives_when_shared_state_available() {
        let safepoint = Arc::new(SafepointCoordinator::new(1));
        let tasks = Arc::new(RwLock::new(FxHashMap::default()));
        let injector = Arc::new(Injector::new());
        let shared = Arc::new(SharedVmState::new(safepoint, tasks, injector));

        {
            let mut reg = shared.native_registry.write();
            reg.register("test.native.sum", |_ctx, args| {
                let a = native_to_value(args[0]).as_i32().unwrap_or(0);
                let b = native_to_value(args[1]).as_i32().unwrap_or(0);
                NativeCallResult::i32(a + b)
            });
            let resolved = ResolvedNatives::link(&["test.native.sum".to_string()], &reg)
                .expect("link resolved natives");
            *shared.resolved_natives.write() = resolved;
        }

        let preempt = AtomicBool::new(false);
        let mut ctx = AotTaskContext {
            preempt_requested: &preempt,
            resume_value: abi::NULL_VALUE,
            suspend_reason: SuspendReason::None,
            suspend_payload: 0,
            helpers: create_default_helper_table(),
            shared_state: Arc::as_ptr(&shared) as *mut (),
            current_task: ptr::null_mut(),
            module: ptr::null(),
        };

        let args = [Value::i32(7).raw(), Value::i32(11).raw()];
        let raw = unsafe { helper_native_call(&mut ctx, 0, args.as_ptr(), args.len() as u8) };
        assert_eq!(raw, Value::i32(18).raw());
        assert_eq!(ctx.suspend_reason, SuspendReason::None);
    }

    #[test]
    fn test_native_call_suspend_submits_io_when_shared_state_available() {
        let safepoint = Arc::new(SafepointCoordinator::new(1));
        let tasks = Arc::new(RwLock::new(FxHashMap::default()));
        let injector = Arc::new(Injector::new());
        let shared = Arc::new(SharedVmState::new(safepoint, tasks, injector));
        let (tx, rx) = unbounded();
        *shared.io_submit_tx.lock() = Some(tx);

        {
            let mut reg = shared.native_registry.write();
            reg.register("test.native.suspend", |_ctx, _args| {
                NativeCallResult::Suspend(IoRequest::Sleep { duration_nanos: 1 })
            });
            let resolved = ResolvedNatives::link(&["test.native.suspend".to_string()], &reg)
                .expect("link resolved natives");
            *shared.resolved_natives.write() = resolved;
        }

        let preempt = AtomicBool::new(false);
        let mut ctx = AotTaskContext {
            preempt_requested: &preempt,
            resume_value: abi::NULL_VALUE,
            suspend_reason: SuspendReason::None,
            suspend_payload: 0,
            helpers: create_default_helper_table(),
            shared_state: Arc::as_ptr(&shared) as *mut (),
            current_task: ptr::null_mut(),
            module: ptr::null(),
        };

        let raw = unsafe { helper_native_call(&mut ctx, 0, ptr::null(), 0) };
        assert_eq!(raw, AOT_SUSPEND);
        assert_eq!(ctx.suspend_reason, SuspendReason::IoWait);
        let submission = rx.try_recv().expect("io submission should be sent");
        assert_eq!(submission.task_id.as_u64(), 0);
        assert!(matches!(
            submission.request,
            IoRequest::Sleep { duration_nanos: 1 }
        ));
    }

    #[test]
    fn test_alloc_object_resolves_module_local_nominal_type_index() {
        let safepoint = Arc::new(SafepointCoordinator::new(1));
        let tasks = Arc::new(RwLock::new(FxHashMap::default()));
        let injector = Arc::new(Injector::new());
        let shared = Arc::new(SharedVmState::new(safepoint, tasks, injector));

        let mut seed_module = Module::new("aot-seed".to_string());
        seed_module.classes.push(ClassDef {
            name: "Seed".to_string(),
            field_count: 1,
            parent_id: None,
            methods: Vec::new(),
        });
        let seed_module = Arc::new(
            Module::decode(&seed_module.encode()).expect("finalize seed module checksum"),
        );
        shared
            .register_module(seed_module)
            .expect("register seed module");

        let mut target_module = Module::new("aot-target".to_string());
        target_module.classes.push(ClassDef {
            name: "Target".to_string(),
            field_count: 3,
            parent_id: None,
            methods: Vec::new(),
        });
        let target_module = Arc::new(
            Module::decode(&target_module.encode()).expect("finalize target module checksum"),
        );
        shared
            .register_module(target_module.clone())
            .expect("register target module");

        let expected_nominal_type_id = shared
            .resolve_nominal_type_id(&target_module, 0)
            .expect("module-local nominal type id");

        let preempt = AtomicBool::new(false);
        let mut ctx = AotTaskContext {
            preempt_requested: &preempt,
            resume_value: abi::NULL_VALUE,
            suspend_reason: SuspendReason::None,
            suspend_payload: 0,
            helpers: create_default_helper_table(),
            shared_state: Arc::as_ptr(&shared) as *mut (),
            current_task: ptr::null_mut(),
            module: Arc::as_ptr(&target_module) as *const (),
        };

        let raw = unsafe { helper_alloc_object(&mut ctx, 0) };
        let value = unsafe { Value::from_raw(raw) };
        let obj_ptr = unsafe { value.as_ptr::<Object>() }.expect("allocated object");
        let obj = unsafe { &*obj_ptr.as_ptr() };

        assert_eq!(
            obj.nominal_type_id_usize(),
            Some(expected_nominal_type_id),
            "AOT alloc helper must resolve module-local nominal type indices"
        );
        assert_eq!(obj.field_count(), 3);
    }
}
