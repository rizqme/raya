#![allow(missing_docs)]
//! AOT runtime helper implementations
//!
//! These are the `unsafe extern "C"` functions that AOT-compiled code calls
//! through the `AotHelperTable`. They bridge between generated native code
//! and the Raya runtime.
//!
//! Helper categories:
//! - **Frame management**: alloc/free AotFrames
//! - **NaN-boxing constants**: box i32/f64 values
//! - **Value operations**: comparison, string/array ops
//! - **GC/Heap**: allocation and safepoint polling
//! - **Concurrency**: spawn, preemption, sync AOT calls
//! - **Native calls**: dispatch through the compiled runtime path

use std::alloc::{self, Layout};
use std::ptr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::OnceLock;

use super::abi;
use super::frame::{AotEntryFn, AotFrame, AotHelperTable, AotTaskContext};
use crate::compiler::compiled_support::CompiledNumericIntrinsicOp;
use crate::compiler::ir::{decode_kernel_op_id, KernelOp};
use crate::compiler::Opcode;
use crate::vm::abi::{
    dispatch_compiled_numeric_intrinsic, native_to_value, value_to_native, EngineContext,
};
use crate::vm::interpreter::Interpreter;
use crate::vm::interpreter::SharedVmState;
use crate::vm::json::view::{js_classify, JSView};
use crate::vm::object::{Array, DynProp, Object, RayaString};
use crate::vm::scheduler::{IoSubmission, ResumePolicy, SuspendReason, Task};
use crate::vm::suspend::{BackendCallResult, SuspendRecord, SuspendTag};
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use parking_lot::RwLock;
use raya_sdk::NativeCallResult;
use rustc_hash::FxHashMap;

#[derive(Clone, Copy)]
pub struct RegisteredAotClone {
    pub ptr: AotEntryFn,
    pub guard_bytecode_offset: Option<u32>,
    pub guard_layout_id: Option<u32>,
    pub guard_arg_index: Option<u32>,
}

pub struct RegisteredAotFunctionEntry {
    pub func_id: u32,
    pub baseline: AotEntryFn,
    pub clones: Vec<RegisteredAotClone>,
}

#[derive(Clone, Copy, Default)]
struct RegisteredAotFunction {
    baseline: usize,
    clones_start: usize,
    clones_len: usize,
}

#[derive(Clone, Copy)]
struct RegisteredAotCloneInternal {
    ptr: usize,
    guard_bytecode_offset: Option<u32>,
    guard_layout_id: Option<u32>,
    guard_arg_index: Option<u32>,
}

#[derive(Default)]
struct RegisteredAotRegistry {
    functions: FxHashMap<u32, RegisteredAotFunction>,
    clones: Vec<RegisteredAotCloneInternal>,
}

static AOT_FUNCTION_REGISTRY: OnceLock<RwLock<RegisteredAotRegistry>> = OnceLock::new();

fn aot_function_registry() -> &'static RwLock<RegisteredAotRegistry> {
    AOT_FUNCTION_REGISTRY.get_or_init(|| RwLock::new(RegisteredAotRegistry::default()))
}

pub struct InstalledAotFunctionRegistry;

impl Drop for InstalledAotFunctionRegistry {
    fn drop(&mut self) {
        clear_registered_aot_functions();
    }
}

pub fn install_registered_aot_functions<I>(entries: I) -> InstalledAotFunctionRegistry
where
    I: IntoIterator<Item = RegisteredAotFunctionEntry>,
{
    let mut registry = aot_function_registry().write();
    registry.functions.clear();
    registry.clones.clear();
    for entry in entries {
        let clones_start = registry.clones.len();
        for clone in entry.clones {
            registry.clones.push(RegisteredAotCloneInternal {
                ptr: clone.ptr as usize,
                guard_bytecode_offset: clone.guard_bytecode_offset,
                guard_layout_id: clone.guard_layout_id,
                guard_arg_index: clone.guard_arg_index,
            });
        }
        let clones_len = registry.clones.len() - clones_start;
        registry.functions.insert(
            entry.func_id,
            RegisteredAotFunction {
                baseline: entry.baseline as usize,
                clones_start,
                clones_len,
            },
        );
    }
    InstalledAotFunctionRegistry
}

pub fn clear_registered_aot_functions() {
    let mut registry = aot_function_registry().write();
    registry.functions.clear();
    registry.clones.clear();
}

const CAST_KIND_MASK_FLAG: u16 = 0x8000;
const CAST_TUPLE_LEN_FLAG: u16 = 0x4000;
const CAST_OBJECT_MIN_FIELDS_FLAG: u16 = 0x2000;
const CAST_ARRAY_ELEM_KIND_FLAG: u16 = 0x1000;
const CAST_KIND_NULL: u16 = 0x0001;
const CAST_KIND_BOOL: u16 = 0x0002;
const CAST_KIND_INT: u16 = 0x0004;
const CAST_KIND_NUMBER: u16 = 0x0008;
const CAST_KIND_STRING: u16 = 0x0010;
const CAST_KIND_ARRAY: u16 = 0x0020;
const CAST_KIND_OBJECT: u16 = 0x0040;
const CAST_KIND_FUNCTION: u16 = 0x0080;

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

    // Zero-initialize locals (already done by alloc_zeroed, but explicit for clarity)
    for i in 0..local_count as usize {
        *locals_ptr.add(i) = abi::NULL_VALUE; // Initialize locals to null
    }

    ptr
}

unsafe fn set_ctx_suspend_reason(ctx: *mut AotTaskContext, reason: &SuspendReason) {
    (*ctx).suspend_record.set_reason(reason);
}

unsafe fn set_ctx_suspend_tag(ctx: *mut AotTaskContext, tag: SuspendTag) {
    (*ctx).suspend_record.set_tag(tag);
}

fn aot_build_interpreter<'a>(shared: &'a SharedVmState) -> Interpreter<'a> {
    Interpreter::new(
        &shared.gc,
        &shared.classes,
        &shared.layouts,
        &shared.mutex_registry,
        &shared.semaphore_registry,
        shared.safepoint.as_ref(),
        &shared.globals_by_index,
        &shared.builtin_global_slots,
        &shared.js_global_bindings,
        &shared.js_global_binding_slots,
        &shared.constant_string_cache,
        &shared.ephemeral_gc_roots,
        &shared.pinned_handles,
        &shared.tasks,
        &shared.injector,
        &shared.promise_microtasks,
        &shared.test262_async_state,
        &shared.test262_async_failure,
        &shared.metadata,
        &shared.class_metadata,
        &shared.native_handler,
        &shared.module_layouts,
        &shared.module_registry,
        &shared.structural_shape_adapters,
        &shared.structural_shape_names,
        &shared.structural_layout_shapes,
        &shared.type_handles,
        &shared.class_value_slots,
        &shared.prop_keys,
        &shared.aot_profile,
        None,
        shared.max_preemptions,
        &shared.stack_pool,
    )
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
// GC / Heap helpers
// =============================================================================

unsafe extern "C" fn helper_safepoint_poll(_ctx: *mut AotTaskContext) {
    // AOT helper entrypoint reserved for explicit safepoint polling.
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

unsafe extern "C" fn helper_alloc_structural_object(
    ctx: *mut AotTaskContext,
    layout_id: u32,
    field_count: u32,
) -> u64 {
    if ctx.is_null() || (*ctx).shared_state.is_null() || layout_id == 0 {
        return abi::NULL_VALUE;
    }
    let shared = &*((*ctx).shared_state as *const SharedVmState);
    let mut gc = shared.gc.lock();
    let obj_ptr = gc.allocate(Object::new_structural(layout_id, field_count as usize));
    let value = Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap());
    value.raw()
}

unsafe extern "C" fn helper_alloc_array(
    ctx: *mut AotTaskContext,
    type_id: u32,
    capacity: u32,
) -> u64 {
    if ctx.is_null() || (*ctx).shared_state.is_null() {
        return abi::NULL_VALUE;
    }
    let shared = &*((*ctx).shared_state as *const SharedVmState);
    let arr = Array::new(type_id as usize, capacity as usize);
    let mut gc = shared.gc.lock();
    let arr_ptr = gc.allocate(arr);
    Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()).raw()
}

unsafe extern "C" fn helper_alloc_string(
    ctx: *mut AotTaskContext,
    data_ptr: *const u8,
    len: u32,
) -> u64 {
    if ctx.is_null() || (*ctx).shared_state.is_null() || data_ptr.is_null() {
        return abi::NULL_VALUE;
    }
    let shared = &*((*ctx).shared_state as *const SharedVmState);
    let bytes = std::slice::from_raw_parts(data_ptr, len as usize);
    let string = match std::str::from_utf8(bytes) {
        Ok(value) => value.to_owned(),
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    };
    let mut gc = shared.gc.lock();
    let ptr = gc.allocate(RayaString::new(string));
    Value::from_ptr(std::ptr::NonNull::new(ptr.as_ptr()).unwrap()).raw()
}

// =============================================================================
// Value operations
// =============================================================================

unsafe extern "C" fn helper_string_concat(ctx: *mut AotTaskContext, a: u64, b: u64) -> u64 {
    if ctx.is_null() || (*ctx).shared_state.is_null() {
        return abi::NULL_VALUE;
    }
    let shared = &*((*ctx).shared_state as *const SharedVmState);
    let a = Value::from_raw(a);
    let b = Value::from_raw(b);
    let stringify = |value: Value| -> String {
        if value.is_null() {
            "null".to_string()
        } else if let Some(boolean) = value.as_bool() {
            boolean.to_string()
        } else if let Some(int) = value.as_i32() {
            int.to_string()
        } else if let Some(float) = value.as_f64() {
            if float.fract() == 0.0 && float.abs() < 1e15 {
                (float as i64).to_string()
            } else {
                float.to_string()
            }
        } else if let Some(ptr) = unsafe { value.as_ptr::<RayaString>() } {
            unsafe { &*ptr.as_ptr() }.data.clone()
        } else {
            "[object]".to_string()
        }
    };
    let result = RayaString::new(format!("{}{}", stringify(a), stringify(b)));
    let mut gc = shared.gc.lock();
    let ptr = gc.allocate(result);
    Value::from_ptr(std::ptr::NonNull::new(ptr.as_ptr()).unwrap()).raw()
}

unsafe extern "C" fn helper_string_len(val: u64) -> u64 {
    let value = Value::from_raw(val);
    let Some(ptr) = (unsafe { value.as_ptr::<RayaString>() }) else {
        return Value::i32(0).raw();
    };
    Value::i32(unsafe { &*ptr.as_ptr() }.len() as i32).raw()
}

unsafe extern "C" fn helper_array_len(val: u64) -> u64 {
    let value = Value::from_raw(val);
    let Some(ptr) = (unsafe { value.as_ptr::<Array>() }) else {
        return Value::i32(0).raw();
    };
    Value::i32(unsafe { &*ptr.as_ptr() }.len() as i32).raw()
}

unsafe extern "C" fn helper_array_get(array: u64, index: u64) -> u64 {
    let array = Value::from_raw(array);
    let index = Value::from_raw(index);
    let Some(ptr) = (unsafe { array.as_ptr::<Array>() }) else {
        return abi::NULL_VALUE;
    };
    let index = index
        .as_i32()
        .map(|v| v as usize)
        .or_else(|| index.as_f64().map(|v| v as usize))
        .unwrap_or(0);
    unsafe { &*ptr.as_ptr() }
        .get(index)
        .unwrap_or(Value::null())
        .raw()
}

unsafe extern "C" fn helper_array_set(array: u64, index: u64, value: u64) {
    let array = Value::from_raw(array);
    let index = Value::from_raw(index);
    let Some(ptr) = (unsafe { array.as_ptr::<Array>() }) else {
        return;
    };
    let index = index
        .as_i32()
        .map(|v| v as usize)
        .or_else(|| index.as_f64().map(|v| v as usize))
        .unwrap_or(0);
    let _ = unsafe { &mut *ptr.as_ptr() }.set(index, Value::from_raw(value));
}

unsafe extern "C" fn helper_array_push(_ctx: *mut AotTaskContext, array: u64, value: u64) {
    let array = Value::from_raw(array);
    let Some(ptr) = (unsafe { array.as_ptr::<Array>() }) else {
        return;
    };
    unsafe { &mut *ptr.as_ptr() }.push(Value::from_raw(value));
}

unsafe extern "C" fn helper_generic_equals(ctx: *mut AotTaskContext, a: u64, b: u64) -> u8 {
    if ctx.is_null() {
        return u8::from(a == b);
    }
    match aot_exec_value_opcode_binary(ctx, Opcode::Eq, a, b)
        .and_then(|value| value.as_bool().map(u8::from))
    {
        Some(result) => result,
        None => 0,
    }
}

unsafe extern "C" fn helper_generic_less_than(
    ctx: *mut AotTaskContext,
    a: u64,
    b: u64,
) -> u8 {
    if ctx.is_null() {
        let base = abi::NAN_BOX_BASE;
        if a < base && b < base {
            return u8::from(f64::from_bits(a) < f64::from_bits(b));
        }
        return 0;
    }
    match aot_exec_value_opcode_binary(ctx, Opcode::Flt, a, b)
        .and_then(|value| value.as_bool().map(u8::from))
    {
        Some(result) => result,
        None => 0,
    }
}

unsafe extern "C" fn helper_generic_add(ctx: *mut AotTaskContext, a: u64, b: u64) -> u64 {
    aot_exec_value_opcode_binary(ctx, Opcode::Fadd, a, b)
        .unwrap_or_else(Value::null)
        .raw()
}

unsafe extern "C" fn helper_generic_sub(ctx: *mut AotTaskContext, a: u64, b: u64) -> u64 {
    aot_exec_value_opcode_binary(ctx, Opcode::Fsub, a, b)
        .unwrap_or_else(Value::null)
        .raw()
}

unsafe extern "C" fn helper_generic_mul(ctx: *mut AotTaskContext, a: u64, b: u64) -> u64 {
    aot_exec_value_opcode_binary(ctx, Opcode::Fmul, a, b)
        .unwrap_or_else(Value::null)
        .raw()
}

unsafe extern "C" fn helper_generic_div(ctx: *mut AotTaskContext, a: u64, b: u64) -> u64 {
    aot_exec_value_opcode_binary(ctx, Opcode::Fdiv, a, b)
        .unwrap_or_else(Value::null)
        .raw()
}

unsafe extern "C" fn helper_generic_mod(ctx: *mut AotTaskContext, a: u64, b: u64) -> u64 {
    aot_exec_value_opcode_binary(ctx, Opcode::Fmod, a, b)
        .unwrap_or_else(Value::null)
        .raw()
}

unsafe extern "C" fn helper_generic_neg(ctx: *mut AotTaskContext, value: u64) -> u64 {
    aot_exec_value_opcode_unary(ctx, Opcode::Fneg, value)
        .unwrap_or_else(Value::null)
        .raw()
}

unsafe extern "C" fn helper_generic_not(ctx: *mut AotTaskContext, value: u64) -> u8 {
    match aot_exec_value_opcode_unary(ctx, Opcode::Not, value)
        .and_then(|value| value.as_bool().map(u8::from))
    {
        Some(result) => result,
        None => 0,
    }
}

unsafe extern "C" fn helper_generic_not_equal(
    ctx: *mut AotTaskContext,
    a: u64,
    b: u64,
) -> u8 {
    match aot_exec_value_opcode_binary(ctx, Opcode::Ne, a, b)
        .and_then(|value| value.as_bool().map(u8::from))
    {
        Some(result) => result,
        None => 0,
    }
}

unsafe extern "C" fn helper_generic_strict_equals(
    ctx: *mut AotTaskContext,
    a: u64,
    b: u64,
) -> u8 {
    match aot_exec_value_opcode_binary(ctx, Opcode::StrictEq, a, b)
        .and_then(|value| value.as_bool().map(u8::from))
    {
        Some(result) => result,
        None => 0,
    }
}

unsafe extern "C" fn helper_generic_strict_not_equal(
    ctx: *mut AotTaskContext,
    a: u64,
    b: u64,
) -> u8 {
    match aot_exec_value_opcode_binary(ctx, Opcode::StrictNe, a, b)
        .and_then(|value| value.as_bool().map(u8::from))
    {
        Some(result) => result,
        None => 0,
    }
}

unsafe extern "C" fn helper_generic_less_equal(
    ctx: *mut AotTaskContext,
    a: u64,
    b: u64,
) -> u8 {
    match aot_exec_value_opcode_binary(ctx, Opcode::Fle, a, b)
        .and_then(|value| value.as_bool().map(u8::from))
    {
        Some(result) => result,
        None => 0,
    }
}

unsafe extern "C" fn helper_generic_greater(
    ctx: *mut AotTaskContext,
    a: u64,
    b: u64,
) -> u8 {
    match aot_exec_value_opcode_binary(ctx, Opcode::Fgt, a, b)
        .and_then(|value| value.as_bool().map(u8::from))
    {
        Some(result) => result,
        None => 0,
    }
}

unsafe extern "C" fn helper_generic_greater_equal(
    ctx: *mut AotTaskContext,
    a: u64,
    b: u64,
) -> u8 {
    match aot_exec_value_opcode_binary(ctx, Opcode::Fge, a, b)
        .and_then(|value| value.as_bool().map(u8::from))
    {
        Some(result) => result,
        None => 0,
    }
}

unsafe extern "C" fn helper_to_string(ctx: *mut AotTaskContext, value: u64) -> u64 {
    aot_exec_value_opcode_unary(ctx, Opcode::ToString, value)
        .unwrap_or_else(Value::null)
        .raw()
}

unsafe extern "C" fn helper_typeof_value(ctx: *mut AotTaskContext, value: u64) -> u64 {
    aot_exec_value_opcode_unary(ctx, Opcode::Typeof, value)
        .unwrap_or_else(Value::null)
        .raw()
}

unsafe extern "C" fn helper_string_compare(
    ctx: *mut AotTaskContext,
    left: u64,
    right: u64,
    opcode_raw: u8,
) -> u8 {
    let Some(opcode) = Opcode::from_u8(opcode_raw) else {
        aot_raise_type_error(
            ctx,
            format!("invalid string compare opcode {}", opcode_raw),
        );
        return 0;
    };
    match aot_exec_value_opcode_binary(ctx, opcode, left, right)
        .and_then(|value| value.as_bool().map(u8::from))
    {
        Some(result) => result,
        None => 0,
    }
}

unsafe extern "C" fn helper_bind_method(
    ctx: *mut AotTaskContext,
    object: u64,
    method_slot: u16,
) -> u64 {
    aot_exec_bind_method(ctx, object, method_slot)
        .unwrap_or_else(Value::null)
        .raw()
}

unsafe extern "C" fn helper_get_arg_count(frame: *const AotFrame) -> u64 {
    if frame.is_null() {
        return Value::i32(0).raw();
    }
    Value::i32((*frame).param_count as i32).raw()
}

unsafe extern "C" fn helper_load_arg_local(frame: *const AotFrame, index_raw: u64) -> u64 {
    if frame.is_null() {
        return Value::undefined().raw();
    }
    let Some(index) = Value::from_raw(index_raw).as_i32() else {
        return Value::undefined().raw();
    };
    if index < 0 {
        return Value::undefined().raw();
    }
    let index = index as usize;
    if index >= (*frame).param_count as usize {
        return Value::undefined().raw();
    }
    *(*frame).locals.add(index)
}

// =============================================================================
// Shape / dynamic / cast helpers
// =============================================================================

fn aot_shared<'a>(ctx: *mut AotTaskContext) -> Option<&'a SharedVmState> {
    if ctx.is_null() {
        return None;
    }
    let ptr = unsafe { (*ctx).shared_state as *const SharedVmState };
    (!ptr.is_null()).then(|| unsafe { &*ptr })
}

fn aot_module<'a>(ctx: *mut AotTaskContext) -> Option<&'a crate::compiler::Module> {
    if ctx.is_null() {
        return None;
    }
    let ptr = unsafe { (*ctx).module as *const crate::compiler::Module };
    (!ptr.is_null()).then(|| unsafe { &*ptr })
}

fn aot_task<'a>(ctx: *mut AotTaskContext) -> Option<&'a Task> {
    if ctx.is_null() {
        return None;
    }
    let ptr = unsafe { (*ctx).current_task as *const Task };
    (!ptr.is_null()).then(|| unsafe { &*ptr })
}

fn aot_task_arc(ctx: *mut AotTaskContext) -> Option<Arc<Task>> {
    let shared = aot_shared(ctx)?;
    let task = aot_task(ctx)?;
    shared.tasks.read().get(&task.id()).cloned()
}

fn aot_raise_type_error(ctx: *mut AotTaskContext, message: String) {
    let (Some(shared), Some(task)) = (aot_shared(ctx), aot_task(ctx)) else {
        return;
    };
    if task.has_exception() {
        return;
    }
    let mut gc = shared.gc.lock();
    let ptr = gc.allocate(RayaString::new(message));
    let exc = unsafe { Value::from_ptr(std::ptr::NonNull::new(ptr.as_ptr()).unwrap()) };
    task.set_exception(exc);
}

fn aot_raise_vm_error(ctx: *mut AotTaskContext, error: VmError) {
    aot_raise_type_error(ctx, error.to_string());
}

unsafe fn aot_exec_value_opcode_unary(
    ctx: *mut AotTaskContext,
    opcode: Opcode,
    operand_raw: u64,
) -> Option<Value> {
    let (Some(shared), Some(module), Some(task)) =
        (aot_shared(ctx), aot_module(ctx), aot_task_arc(ctx))
    else {
        return None;
    };
    let mut interpreter = aot_build_interpreter(shared);
    let mut stack = Stack::new();
    if let Err(error) = stack.push(Value::from_raw(operand_raw)) {
        aot_raise_vm_error(ctx, error);
        return None;
    }
    let result = match opcode {
        Opcode::Fneg => interpreter.exec_arithmetic_ops(&mut stack, module, &task, opcode),
        Opcode::Typeof => {
            let code = [opcode as u8];
            let mut ip = 1usize;
            interpreter.exec_type_ops(&mut stack, &mut ip, &code, module, &task, opcode)
        }
        Opcode::ToString => interpreter.exec_string_ops(&mut stack, opcode),
        Opcode::Not => interpreter.exec_comparison_ops(&mut stack, module, &task, opcode),
        _ => {
            aot_raise_type_error(
                ctx,
                format!("unsupported unary AOT helper opcode {:?}", opcode),
            );
            return None;
        }
    };
    match result {
        crate::vm::interpreter::OpcodeResult::Continue => stack.pop().ok(),
        crate::vm::interpreter::OpcodeResult::Return(value) => Some(value),
        crate::vm::interpreter::OpcodeResult::Error(error) => {
            aot_raise_vm_error(ctx, error);
            None
        }
        crate::vm::interpreter::OpcodeResult::Suspend(reason) => {
            aot_raise_type_error(
                ctx,
                format!(
                    "unexpected suspension {:?} in exact unary AOT helper {:?}",
                    reason, opcode
                ),
            );
            None
        }
        crate::vm::interpreter::OpcodeResult::PushFrame { .. } => {
            aot_raise_type_error(
                ctx,
                format!(
                    "unexpected nested frame in exact unary AOT helper {:?}",
                    opcode
                ),
            );
            None
        }
    }
}

unsafe fn aot_exec_value_opcode_binary(
    ctx: *mut AotTaskContext,
    opcode: Opcode,
    left_raw: u64,
    right_raw: u64,
) -> Option<Value> {
    let (Some(shared), Some(module), Some(task)) =
        (aot_shared(ctx), aot_module(ctx), aot_task_arc(ctx))
    else {
        return None;
    };
    let mut interpreter = aot_build_interpreter(shared);
    let mut stack = Stack::new();
    if let Err(error) = stack.push(Value::from_raw(left_raw)) {
        aot_raise_vm_error(ctx, error);
        return None;
    }
    if let Err(error) = stack.push(Value::from_raw(right_raw)) {
        aot_raise_vm_error(ctx, error);
        return None;
    }
    let result = match opcode {
        Opcode::Fadd
        | Opcode::Fsub
        | Opcode::Fmul
        | Opcode::Fdiv
        | Opcode::Fmod => interpreter.exec_arithmetic_ops(&mut stack, module, &task, opcode),
        Opcode::Eq
        | Opcode::Ne
        | Opcode::StrictEq
        | Opcode::StrictNe
        | Opcode::Flt
        | Opcode::Fle
        | Opcode::Fgt
        | Opcode::Fge => interpreter.exec_comparison_ops(&mut stack, module, &task, opcode),
        Opcode::Sconcat
        | Opcode::Seq
        | Opcode::Sne
        | Opcode::Slt
        | Opcode::Sle
        | Opcode::Sgt
        | Opcode::Sge => interpreter.exec_string_ops(&mut stack, opcode),
        _ => {
            aot_raise_type_error(
                ctx,
                format!("unsupported binary AOT helper opcode {:?}", opcode),
            );
            return None;
        }
    };
    match result {
        crate::vm::interpreter::OpcodeResult::Continue => stack.pop().ok(),
        crate::vm::interpreter::OpcodeResult::Return(value) => Some(value),
        crate::vm::interpreter::OpcodeResult::Error(error) => {
            aot_raise_vm_error(ctx, error);
            None
        }
        crate::vm::interpreter::OpcodeResult::Suspend(reason) => {
            aot_raise_type_error(
                ctx,
                format!(
                    "unexpected suspension {:?} in exact binary AOT helper {:?}",
                    reason, opcode
                ),
            );
            None
        }
        crate::vm::interpreter::OpcodeResult::PushFrame { .. } => {
            aot_raise_type_error(
                ctx,
                format!(
                    "unexpected nested frame in exact binary AOT helper {:?}",
                    opcode
                ),
            );
            None
        }
    }
}

unsafe fn aot_exec_bind_method(
    ctx: *mut AotTaskContext,
    object_raw: u64,
    method_slot: u16,
) -> Option<Value> {
    let (Some(shared), Some(module), Some(task)) =
        (aot_shared(ctx), aot_module(ctx), aot_task_arc(ctx))
    else {
        return None;
    };
    let mut interpreter = aot_build_interpreter(shared);
    let mut stack = Stack::new();
    if let Err(error) = stack.push(Value::from_raw(object_raw)) {
        aot_raise_vm_error(ctx, error);
        return None;
    }
    let code = [Opcode::BindMethod as u8, (method_slot & 0xFF) as u8, (method_slot >> 8) as u8];
    let mut ip = 1usize;
    let result = interpreter.exec_object_ops(&mut stack, &mut ip, &code, module, &task, Opcode::BindMethod);
    match result {
        crate::vm::interpreter::OpcodeResult::Continue => stack.pop().ok(),
        crate::vm::interpreter::OpcodeResult::Return(value) => Some(value),
        crate::vm::interpreter::OpcodeResult::Error(error) => {
            aot_raise_vm_error(ctx, error);
            None
        }
        crate::vm::interpreter::OpcodeResult::Suspend(reason) => {
            aot_raise_type_error(
                ctx,
                format!("unexpected suspension {:?} in bind method helper", reason),
            );
            None
        }
        crate::vm::interpreter::OpcodeResult::PushFrame { .. } => {
            aot_raise_type_error(
                ctx,
                "unexpected nested frame in bind method helper".to_string(),
            );
            None
        }
    }
}

fn aot_value_kind_mask(value: Value) -> u16 {
    if value.is_null() {
        return CAST_KIND_NULL;
    }
    if value.as_bool().is_some() {
        return CAST_KIND_BOOL;
    }
    if value.as_i32().is_some() {
        return CAST_KIND_INT;
    }
    if value.as_f64().is_some() {
        return CAST_KIND_NUMBER;
    }
    if unsafe { value.as_ptr::<RayaString>() }.is_some() {
        return CAST_KIND_STRING;
    }
    if unsafe { value.as_ptr::<Array>() }.is_some() {
        return CAST_KIND_ARRAY;
    }
    if unsafe { value.as_ptr::<Object>() }.is_some() {
        let obj = unsafe { &*value.as_ptr::<Object>().unwrap().as_ptr() };
        if obj.is_callable() {
            return CAST_KIND_FUNCTION;
        }
        return CAST_KIND_OBJECT;
    }
    0
}

fn aot_object_ptr_checked(value: Value) -> Option<std::ptr::NonNull<Object>> {
    match js_classify(value) {
        JSView::Struct { ptr, .. } => std::ptr::NonNull::new(ptr as *mut Object),
        _ => None,
    }
}

fn aot_dyn_key_parts(key_val: Value) -> Result<(Option<String>, Option<usize>), String> {
    match js_classify(key_val) {
        JSView::Str(ptr) => {
            let key = unsafe { &*ptr }.data.clone();
            let index = key.parse::<usize>().ok();
            Ok((Some(key), index))
        }
        JSView::Int(index) if index >= 0 => {
            let index = index as usize;
            Ok((Some(index.to_string()), Some(index)))
        }
        JSView::Number(number) if number.is_finite() && number.fract() == 0.0 && number >= 0.0 => {
            let index = number as usize;
            Ok((Some(index.to_string()), Some(index)))
        }
        _ => Err("Dynamic key must be a string or non-negative integer".to_string()),
    }
}

fn aot_shape_slot_map_for_object(
    shared: &SharedVmState,
    object: &Object,
    required_names: &[String],
) -> Option<Vec<crate::vm::interpreter::StructuralSlotBinding>> {
    use crate::vm::interpreter::StructuralSlotBinding;

    let layout_names = shared
        .layouts
        .read()
        .layout_field_names(object.layout_id())
        .map(|names| names.to_vec())
        .or_else(|| {
            shared
                .structural_layout_shapes
                .read()
                .get(&object.layout_id())
                .cloned()
        });
    let class_meta = object
        .nominal_type_id_usize()
        .and_then(|nominal_type_id| shared.class_metadata.read().get(nominal_type_id).cloned());
    let dynamic_binding_for = |name: &str| {
        object.dyn_props().and_then(|dp| {
            dp.keys_in_order().find_map(|key| {
                shared
                    .prop_key_name(key)
                    .filter(|actual| actual == name)
                    .map(|_| StructuralSlotBinding::Dynamic(key))
            })
        })
    };

    if let Some(nominal_type_id) = object.nominal_type_id_usize() {
        let class_meta = class_meta;
        let classes = shared.classes.read();
        let class = classes.get_class(nominal_type_id)?;
        return Some(
            required_names
                .iter()
                .map(|name| {
                    class_meta
                        .as_ref()
                        .and_then(|meta| meta.get_field_index(name))
                        .and_then(|index| {
                            (index < object.field_count())
                                .then_some(StructuralSlotBinding::Field(index))
                        })
                        .or_else(|| {
                            layout_names
                                .as_ref()
                                .and_then(|names| names.iter().position(|actual| actual == name))
                                .map(StructuralSlotBinding::Field)
                        })
                        .or_else(|| {
                            class_meta
                                .as_ref()
                                .and_then(|meta| meta.get_method_index(name))
                                .map(StructuralSlotBinding::Method)
                        })
                        .or_else(|| dynamic_binding_for(name))
                        .unwrap_or(StructuralSlotBinding::Missing)
                })
                .collect(),
        );
    }

    Some(
        required_names
            .iter()
            .map(|name| {
                layout_names
                    .as_ref()
                    .and_then(|names| names.iter().position(|actual| actual == name))
                    .map(StructuralSlotBinding::Field)
                    .or_else(|| dynamic_binding_for(name))
                    .unwrap_or(StructuralSlotBinding::Missing)
            })
            .collect(),
    )
}

fn aot_ensure_shape_adapter_for_object(
    shared: &SharedVmState,
    object: &Object,
    required_shape: u64,
) -> Option<Arc<crate::vm::interpreter::ShapeAdapter>> {
    use crate::vm::interpreter::ShapeAdapter;
    use crate::vm::interpreter::StructuralAdapterKey;

    let adapter_key = StructuralAdapterKey {
        provider_layout: object.layout_id(),
        required_shape,
    };
    let current_epoch = shared
        .layouts
        .read()
        .layout_epoch(object.layout_id())
        .unwrap_or(0);
    if let Some(adapter) = shared
        .structural_shape_adapters
        .read()
        .get(&adapter_key)
        .cloned()
    {
        if adapter.epoch == current_epoch {
            return Some(adapter);
        }
    }
    let required_names = shared
        .structural_shape_names
        .read()
        .get(&required_shape)
        .cloned()?;
    let slot_map = aot_shape_slot_map_for_object(shared, object, &required_names)?;
    let adapter = Arc::new(ShapeAdapter::from_slot_map(
        object.layout_id(),
        required_shape,
        &slot_map,
        current_epoch,
    ));
    let mut adapters = shared.structural_shape_adapters.write();
    let entry = adapters
        .entry(adapter_key)
        .or_insert_with(|| adapter.clone())
        .clone();
    Some(entry)
}

unsafe extern "C" fn helper_object_is_nominal(
    ctx: *mut AotTaskContext,
    object_raw: u64,
    local_nominal_type_index: u32,
) -> u8 {
    let (Some(shared), Some(module)) = (aot_shared(ctx), aot_module(ctx)) else {
        return 0;
    };
    let Some(target_nominal_type_id) =
        shared.resolve_nominal_type_id(module, local_nominal_type_index as usize)
    else {
        return 0;
    };
    let object = Value::from_raw(object_raw);
    let Some(object_ptr) = aot_object_ptr_checked(object) else {
        return 0;
    };
    let object = &*object_ptr.as_ptr();
    let classes = shared.classes.read();
    let mut current = object.nominal_type_id_usize();
    while let Some(nominal_type_id) = current {
        if nominal_type_id == target_nominal_type_id {
            return 1;
        }
        current = classes
            .get_class(nominal_type_id)
            .and_then(|class| class.parent_id);
    }
    0
}

unsafe extern "C" fn helper_object_implements_shape(
    ctx: *mut AotTaskContext,
    object_raw: u64,
    required_shape: u64,
) -> u8 {
    let Some(shared) = aot_shared(ctx) else {
        return 0;
    };
    let object = Value::from_raw(object_raw);
    let Some(object_ptr) = aot_object_ptr_checked(object) else {
        return 0;
    };
    let object = &*object_ptr.as_ptr();
    let required_names = shared
        .structural_shape_names
        .read()
        .get(&required_shape)
        .cloned()
        .unwrap_or_default();
    let Some(adapter) = aot_ensure_shape_adapter_for_object(shared, object, required_shape) else {
        return 0;
    };
    if (0..adapter.len()).all(|slot| {
        !matches!(
            adapter.binding_for_slot(slot),
            crate::vm::interpreter::StructuralSlotBinding::Missing
        ) || required_names.get(slot).is_some_and(|name| {
            matches!(
                name.as_str(),
                "constructor"
                    | "equals"
                    | "hashCode"
                    | "hasOwnProperty"
                    | "isPrototypeOf"
                    | "propertyIsEnumerable"
                    | "toLocaleString"
                    | "toString"
                    | "valueOf"
            )
        })
    }) {
        1
    } else {
        0
    }
}

unsafe extern "C" fn helper_object_get_shape_field(
    ctx: *mut AotTaskContext,
    object_raw: u64,
    required_shape: u64,
    expected_slot: u32,
    optional: u8,
) -> u64 {
    use crate::vm::interpreter::StructuralSlotBinding;

    let Some(shared) = aot_shared(ctx) else {
        return abi::NULL_VALUE;
    };
    let object = Value::from_raw(object_raw);
    if optional != 0 && object.is_null() {
        return Value::null().raw();
    }
    let Some(object_ptr) = aot_object_ptr_checked(object) else {
        return abi::NULL_VALUE;
    };
    let object_ref = &*object_ptr.as_ptr();
    let Some(adapter) = aot_ensure_shape_adapter_for_object(shared, object_ref, required_shape)
    else {
        return abi::NULL_VALUE;
    };
    match adapter.binding_for_slot(expected_slot as usize) {
        StructuralSlotBinding::Field(slot) => {
            object_ref.get_field(slot).unwrap_or(Value::null()).raw()
        }
        StructuralSlotBinding::Dynamic(key) => object_ref
            .dyn_props()
            .and_then(|dp| dp.get(key).map(|p| p.value))
            .unwrap_or(Value::null())
            .raw(),
        StructuralSlotBinding::Method(method_slot) => {
            let Some(nominal_type_id) = object_ref.nominal_type_id_usize() else {
                return abi::NULL_VALUE;
            };
            let (func_id, method_module) = {
                let classes = shared.classes.read();
                let Some(class) = classes.get_class(nominal_type_id) else {
                    return abi::NULL_VALUE;
                };
                let Some(fid) = class.vtable.get_method(method_slot) else {
                    return abi::NULL_VALUE;
                };
                (fid, class.module.clone())
            };
            let mut gc = shared.gc.lock();
            let ptr = gc.allocate(Object::new_bound_method(object, func_id, method_module));
            Value::from_ptr(std::ptr::NonNull::new(ptr.as_ptr()).unwrap()).raw()
        }
        StructuralSlotBinding::Missing => Value::null().raw(),
    }
}

unsafe extern "C" fn helper_object_set_shape_field(
    ctx: *mut AotTaskContext,
    object_raw: u64,
    required_shape: u64,
    expected_slot: u32,
    value_raw: u64,
) -> u8 {
    use crate::vm::interpreter::StructuralSlotBinding;

    let Some(shared) = aot_shared(ctx) else {
        return 0;
    };
    let object = Value::from_raw(object_raw);
    let Some(object_ptr) = aot_object_ptr_checked(object) else {
        return 0;
    };
    let object_ref = &mut *object_ptr.as_ptr();
    let Some(adapter) = aot_ensure_shape_adapter_for_object(shared, object_ref, required_shape)
    else {
        return 0;
    };
    match adapter.binding_for_slot(expected_slot as usize) {
        StructuralSlotBinding::Field(slot) => object_ref
            .set_field(slot, Value::from_raw(value_raw))
            .map(|_| 1)
            .unwrap_or(0),
        StructuralSlotBinding::Dynamic(key) => {
            object_ref
                .ensure_dyn_props()
                .insert(key, DynProp::data(Value::from_raw(value_raw)));
            1
        }
        StructuralSlotBinding::Method(_) | StructuralSlotBinding::Missing => 0,
    }
}

unsafe extern "C" fn helper_cast_value(
    ctx: *mut AotTaskContext,
    value_raw: u64,
    target: u32,
) -> u64 {
    let value = Value::from_raw(value_raw);
    let target = target as u16;
    let (Some(shared), Some(module)) = (aot_shared(ctx), aot_module(ctx)) else {
        return abi::NULL_VALUE;
    };

    if (target & CAST_KIND_MASK_FLAG) != 0 {
        if (target & CAST_TUPLE_LEN_FLAG) != 0 {
            let expected_len = (target & 0x3FFF) as usize;
            let Some(ptr) = (unsafe { value.as_ptr::<Array>() }) else {
                aot_raise_type_error(
                    ctx,
                    format!(
                        "Cannot cast non-array value to tuple length {}",
                        expected_len
                    ),
                );
                return abi::NULL_VALUE;
            };
            if unsafe { &*ptr.as_ptr() }.len() != expected_len {
                aot_raise_type_error(
                    ctx,
                    format!("Cannot cast array to tuple length {}", expected_len),
                );
                return abi::NULL_VALUE;
            }
            return value.raw();
        }
        if (target & CAST_OBJECT_MIN_FIELDS_FLAG) != 0 {
            let required_fields = (target & 0x1FFF) as usize;
            let Some(ptr) = aot_object_ptr_checked(value) else {
                aot_raise_type_error(
                    ctx,
                    format!(
                        "Cannot cast non-object value to object with {} required fields",
                        required_fields
                    ),
                );
                return abi::NULL_VALUE;
            };
            let object = unsafe { &*ptr.as_ptr() };
            let field_count = object
                .field_count()
                .max(object.dyn_props().map(|dp| dp.len()).unwrap_or(0));
            if field_count < required_fields {
                aot_raise_type_error(
                    ctx,
                    format!(
                        "Cannot cast object(field_count={}) to required field count {}",
                        field_count, required_fields
                    ),
                );
                return abi::NULL_VALUE;
            }
            return value.raw();
        }
        if (target & CAST_ARRAY_ELEM_KIND_FLAG) != 0 {
            let expected = target & 0x00FF;
            let Some(ptr) = (unsafe { value.as_ptr::<Array>() }) else {
                aot_raise_type_error(
                    ctx,
                    format!(
                        "Cannot cast non-array value to array element mask 0x{:02X}",
                        expected
                    ),
                );
                return abi::NULL_VALUE;
            };
            let array = unsafe { &*ptr.as_ptr() };
            for element in &array.elements {
                let mut actual = aot_value_kind_mask(*element);
                if (actual & CAST_KIND_INT) != 0 {
                    actual |= CAST_KIND_NUMBER;
                }
                if (actual & expected) == 0 {
                    aot_raise_type_error(
                        ctx,
                        format!(
                            "Cannot cast array element to required kind mask 0x{:02X}",
                            expected
                        ),
                    );
                    return abi::NULL_VALUE;
                }
            }
            return value.raw();
        }
        let expected = target & !CAST_KIND_MASK_FLAG;
        let mut actual = aot_value_kind_mask(value);
        if (actual & CAST_KIND_INT) != 0 {
            actual |= CAST_KIND_NUMBER;
        }
        if (actual & expected) != 0 {
            return value.raw();
        }
        if expected == CAST_KIND_FUNCTION {
            let func_id = value.as_i32().map(|v| v as usize).or_else(|| {
                value.as_f64().and_then(|v| {
                    if v.is_finite() && v.fract() == 0.0 && v >= 0.0 && v <= usize::MAX as f64 {
                        Some(v as usize)
                    } else {
                        None
                    }
                })
            });
            if let Some(func_id) = func_id {
                if module.functions.get(func_id).is_some() {
                    return value.raw();
                }
            }
        }
        if expected == CAST_KIND_INT {
            if let Some(num) = value.as_f64() {
                if num.is_finite()
                    && num.fract() == 0.0
                    && num >= i32::MIN as f64
                    && num <= i32::MAX as f64
                {
                    return Value::i32(num as i32).raw();
                }
            }
        }
        aot_raise_type_error(
            ctx,
            format!("Cannot cast value to runtime kind mask 0x{:04X}", expected),
        );
        return abi::NULL_VALUE;
    }

    let Some(target_nominal_type_id) = shared.resolve_nominal_type_id(module, target as usize)
    else {
        aot_raise_type_error(ctx, format!("Unknown nominal target {}", target));
        return abi::NULL_VALUE;
    };
    let Some(object_ptr) = aot_object_ptr_checked(value) else {
        aot_raise_type_error(
            ctx,
            "Cannot cast non-object value to nominal type".to_string(),
        );
        return abi::NULL_VALUE;
    };
    let object = unsafe { &*object_ptr.as_ptr() };
    let classes = shared.classes.read();
    let mut current = object.nominal_type_id_usize();
    while let Some(nominal_type_id) = current {
        if nominal_type_id == target_nominal_type_id {
            return value.raw();
        }
        current = classes
            .get_class(nominal_type_id)
            .and_then(|class| class.parent_id);
    }
    aot_raise_type_error(ctx, "Cannot cast object to target nominal type".to_string());
    abi::NULL_VALUE
}

unsafe extern "C" fn helper_cast_shape(
    ctx: *mut AotTaskContext,
    value_raw: u64,
    required_shape: u64,
) -> u64 {
    if helper_object_implements_shape(ctx, value_raw, required_shape) != 0 {
        return value_raw;
    }
    let Some(shared) = aot_shared(ctx) else {
        return abi::NULL_VALUE;
    };
    let value = Value::from_raw(value_raw);
    let JSView::Struct { ptr, .. } = js_classify(value) else {
        aot_raise_type_error(
            ctx,
            format!("Cannot cast non-object value to structural shape @{required_shape:016x}"),
        );
        return abi::NULL_VALUE;
    };
    let object = unsafe { &*ptr };
    let Some(adapter) = aot_ensure_shape_adapter_for_object(shared, object, required_shape) else {
        aot_raise_type_error(
            ctx,
            format!(
                "Cannot cast object(layout_id={}) to structural shape @{required_shape:016x}",
                object.layout_id()
            ),
        );
        return abi::NULL_VALUE;
    };
    let required_names = shared
        .structural_shape_names
        .read()
        .get(&required_shape)
        .cloned()
        .unwrap_or_default();
    for slot in 0..adapter.len() {
        if matches!(
            adapter.binding_for_slot(slot),
            crate::vm::interpreter::StructuralSlotBinding::Missing
        ) && !required_names.get(slot).is_some_and(|name| {
            matches!(
                name.as_str(),
                "constructor"
                    | "equals"
                    | "hashCode"
                    | "hasOwnProperty"
                    | "isPrototypeOf"
                    | "propertyIsEnumerable"
                    | "toLocaleString"
                    | "toString"
                    | "valueOf"
            )
        }) {
            aot_raise_type_error(
                ctx,
                format!(
                    "Cannot cast object(layout_id={}) to structural shape @{required_shape:016x}: missing required slot {}",
                    object.layout_id(),
                    slot
                ),
            );
            return abi::NULL_VALUE;
        }
    }
    value.raw()
}

unsafe extern "C" fn helper_dyn_get_prop(
    ctx: *mut AotTaskContext,
    object_raw: u64,
    key_raw: u64,
) -> u64 {
    let Some(shared) = aot_shared(ctx) else {
        return abi::NULL_VALUE;
    };
    let object = Value::from_raw(object_raw);
    let key = Value::from_raw(key_raw);
    let Ok((key_str, array_index)) = aot_dyn_key_parts(key) else {
        return abi::NULL_VALUE;
    };
    match js_classify(object) {
        JSView::Arr(ptr) => {
            let arr = unsafe { &*ptr };
            if let Some(index) = array_index {
                arr.get(index).unwrap_or(Value::null()).raw()
            } else if key_str.as_deref() == Some("length") {
                Value::i32(arr.len() as i32).raw()
            } else {
                abi::NULL_VALUE
            }
        }
        JSView::Struct { ptr, .. } => {
            let obj = unsafe { &*ptr };
            let key_str = key_str.unwrap_or_default();
            if let Some(index) = {
                let layout_names = shared
                    .layouts
                    .read()
                    .layout_field_names(obj.layout_id())
                    .map(|names| names.to_vec())
                    .or_else(|| {
                        shared
                            .structural_layout_shapes
                            .read()
                            .get(&obj.layout_id())
                            .cloned()
                    });
                layout_names.and_then(|names| names.iter().position(|name| name == &key_str))
            } {
                obj.get_field(index).unwrap_or(Value::null()).raw()
            } else {
                let key_id = shared.prop_keys.write().intern(&key_str);
                obj.dyn_props()
                    .and_then(|dp| dp.get(key_id).map(|p| p.value))
                    .unwrap_or(Value::null())
                    .raw()
            }
        }
        _ => abi::NULL_VALUE,
    }
}

unsafe extern "C" fn helper_dyn_set_prop(
    ctx: *mut AotTaskContext,
    object_raw: u64,
    key_raw: u64,
    value_raw: u64,
) {
    let Some(shared) = aot_shared(ctx) else {
        return;
    };
    let object = Value::from_raw(object_raw);
    let key = Value::from_raw(key_raw);
    let value = Value::from_raw(value_raw);
    let Ok((key_str, array_index)) = aot_dyn_key_parts(key) else {
        return;
    };
    match js_classify(object) {
        JSView::Arr(ptr) => {
            let Some(index) = array_index else {
                return;
            };
            let arr = unsafe { &mut *(ptr as *mut Array) };
            if index >= arr.elements.len() {
                arr.elements.resize(index + 1, Value::null());
            }
            arr.elements[index] = value;
        }
        JSView::Struct { ptr, .. } => {
            let obj = unsafe { &mut *(ptr as *mut Object) };
            let key_str = key_str.unwrap_or_default();
            if let Some(index) = {
                let layout_names = shared
                    .layouts
                    .read()
                    .layout_field_names(obj.layout_id())
                    .map(|names| names.to_vec())
                    .or_else(|| {
                        shared
                            .structural_layout_shapes
                            .read()
                            .get(&obj.layout_id())
                            .cloned()
                    });
                layout_names.and_then(|names| names.iter().position(|name| name == &key_str))
            } {
                let _ = obj.set_field(index, value);
            } else {
                let key_id = shared.prop_keys.write().intern(&key_str);
                obj.ensure_dyn_props().insert(key_id, DynProp::data(value));
            }
        }
        _ => {}
    }
}

unsafe extern "C" fn helper_load_global_value(ctx: *mut AotTaskContext, local_slot: u32) -> u64 {
    let (Some(shared), Some(module)) = (aot_shared(ctx), aot_module(ctx)) else {
        return abi::NULL_VALUE;
    };
    let absolute = shared.resolve_global_slot(module, local_slot as usize);
    shared
        .globals_by_index
        .read()
        .get(absolute)
        .copied()
        .unwrap_or(Value::null())
        .raw()
}

unsafe extern "C" fn helper_store_global_value(
    ctx: *mut AotTaskContext,
    local_slot: u32,
    value_raw: u64,
) {
    let (Some(shared), Some(module)) = (aot_shared(ctx), aot_module(ctx)) else {
        return;
    };
    let absolute = shared.resolve_global_slot(module, local_slot as usize);
    let mut globals = shared.globals_by_index.write();
    if absolute >= globals.len() {
        globals.resize(absolute + 1, Value::null());
    }
    globals[absolute] = Value::from_raw(value_raw);
}

// =============================================================================
// Object field access helpers
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
// Native call dispatch
// =============================================================================

unsafe extern "C" fn helper_native_call(
    ctx: *mut AotTaskContext,
    kernel_op_id: u16,
    args_ptr: *const u64,
    argc: u8,
) -> BackendCallResult {
    let Some(kernel_op) = decode_kernel_op_id(kernel_op_id) else {
        aot_raise_type_error(ctx, format!("unknown compiled kernel op {}", kernel_op_id));
        return BackendCallResult::threw();
    };

    if !ctx.is_null() && !(*ctx).shared_state.is_null() {
        let shared = &*((*ctx).shared_state as *const SharedVmState);

        // Build engine context for native handler dispatch
        let task_id = if !(*ctx).current_task.is_null() {
            (*((*ctx).current_task as *const Task)).id()
        } else {
            // Fallback for tests/partial contexts without a task pointer.
            crate::vm::scheduler::TaskId::from_u64(0)
        };
        let mut engine_ctx = EngineContext::new(
            &shared.gc,
            &shared.classes,
            &shared.layouts,
            task_id,
            &shared.class_metadata,
        );
        let current_task_arc = if task_id.as_u64() == 0 {
            None
        } else {
            shared.tasks.read().get(&task_id).cloned()
        };
        if let Some(task_arc) = current_task_arc.as_ref() {
            engine_ctx = engine_ctx.with_shared_runtime(shared, task_arc);
        }

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

        if let KernelOp::RegisteredNative(local_idx) = kernel_op {
            let resolved = if !(*ctx).module.is_null() {
                let module = &*((*ctx).module as *const crate::compiler::Module);
                shared
                    .module_layouts
                    .read()
                    .get(&module.checksum)
                    .map(|layout| layout.resolved_natives.clone())
                    .unwrap_or_else(crate::vm::native_registry::ResolvedNatives::empty)
            } else {
                shared.resolved_natives.read().clone()
            };
            match resolved.call(local_idx, &engine_ctx, &native_args) {
                NativeCallResult::Value(val) => {
                    return BackendCallResult::completed_raw(native_to_value(val).raw())
                }
                NativeCallResult::Suspend(io_request) => {
                    if let Some(tx) = shared.io_submit_tx.lock().as_ref() {
                        let _ = tx.send(IoSubmission {
                            task_id,
                            request: io_request,
                        });
                    }
                    set_ctx_suspend_reason(ctx, &SuspendReason::IoWait);
                    return BackendCallResult::suspended_with_tag(SuspendTag::IoWait);
                }
                NativeCallResult::Unhandled | NativeCallResult::Error(_) => {
                    aot_raise_type_error(
                        ctx,
                        format!(
                            "registered native {} could not handle compiled kernel call",
                            local_idx
                        ),
                    );
                    return BackendCallResult::threw();
                }
            }
        }

        if !matches!(kernel_op, KernelOp::VmNative(_)) {
            let Some(module) =
                (!(*ctx).module.is_null()).then(|| &*((*ctx).module as *const crate::compiler::Module))
            else {
                aot_raise_type_error(ctx, "compiled kernel dispatch missing module context".to_string());
                return BackendCallResult::threw();
            };
            let Some(current_task) =
                (!(*ctx).current_task.is_null()).then(|| &*((*ctx).current_task as *const Task))
            else {
                aot_raise_type_error(ctx, "compiled kernel dispatch missing task context".to_string());
                return BackendCallResult::threw();
            };
            let task_arc = shared.tasks.read().get(&current_task.id()).cloned();
            let Some(task) = task_arc.as_ref() else {
                aot_raise_type_error(ctx, "compiled kernel dispatch missing task arc".to_string());
                return BackendCallResult::threw();
            };
            let mut interpreter = aot_build_interpreter(shared);
            let mut stack = Stack::new();
            for arg in &value_args {
                if stack.push(*arg).is_err() {
                    aot_raise_type_error(
                        ctx,
                        "compiled kernel dispatch could not materialize operand stack".to_string(),
                    );
                    return BackendCallResult::threw();
                }
            }
            let code = [
                (kernel_op_id & 0x00FF) as u8,
                ((kernel_op_id >> 8) & 0x00FF) as u8,
                argc,
            ];
            let mut ip = 0usize;
            return match interpreter.exec_native_ops(
                &mut stack,
                &mut ip,
                &code,
                module,
                task,
                Opcode::KernelCall,
            ) {
                crate::vm::interpreter::OpcodeResult::Continue => {
                    BackendCallResult::completed(stack.pop().unwrap_or_else(|_| Value::null()))
                }
                crate::vm::interpreter::OpcodeResult::Return(value) => {
                    BackendCallResult::completed(value)
                }
                crate::vm::interpreter::OpcodeResult::Suspend(reason) => {
                    set_ctx_suspend_reason(ctx, &reason);
                    BackendCallResult::suspended_with_tag(SuspendTag::from_reason(&reason))
                }
                crate::vm::interpreter::OpcodeResult::Error(_) => {
                    BackendCallResult::threw()
                }
                crate::vm::interpreter::OpcodeResult::PushFrame { .. } => {
                    aot_raise_type_error(
                        ctx,
                        "compiled kernel dispatch encountered nested frame and cannot bounce to the interpreter"
                            .to_string(),
                    );
                    BackendCallResult::threw()
                }
            };
        }

        let native_id = match kernel_op {
            KernelOp::VmNative(native_id) => native_id,
            _ => unreachable!(),
        };

        match native_id {
            crate::compiler::native_id::JSON_PARSE => {
                use crate::vm::json;

                if value_args.is_empty() {
                    return BackendCallResult::completed_raw(abi::NULL_VALUE);
                }
                let Some(s) = (unsafe { value_args[0].as_ptr::<RayaString>() }) else {
                    return BackendCallResult::completed_raw(abi::NULL_VALUE);
                };
                let json_str = unsafe { &*s.as_ptr() }.data.clone();
                let mut gc = shared.gc.lock();
                let mut prop_keys = shared.prop_keys.write();
                return match json::parser::parse_with_prop_key_interner(
                    &json_str,
                    &mut gc,
                    &mut |name| prop_keys.intern(name),
                ) {
                    Ok(v) => BackendCallResult::completed(v),
                    Err(_) => BackendCallResult::completed_raw(abi::NULL_VALUE),
                };
            }
            crate::compiler::native_id::JSON_STRINGIFY => {
                use crate::vm::json;

                if value_args.is_empty() {
                    return BackendCallResult::completed_raw(abi::NULL_VALUE);
                }
                return match json::stringify::stringify_with_runtime_metadata(
                    value_args[0],
                    |key| shared.prop_key_name(key),
                    |layout_id| shared.structural_layout_names(layout_id),
                ) {
                    Ok(json_str) => {
                        let gc_ptr = shared.gc.lock().allocate(RayaString::new(json_str));
                        BackendCallResult::completed(Value::from_ptr(
                            std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                        ))
                    }
                    Err(_) => BackendCallResult::completed_raw(abi::NULL_VALUE),
                };
            }
            _ => {}
        }

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
            NativeCallResult::Value(val) => {
                return BackendCallResult::completed_raw(native_to_value(val).raw())
            }
            NativeCallResult::Suspend(io_request) => {
                if let Some(tx) = shared.io_submit_tx.lock().as_ref() {
                    let _ = tx.send(IoSubmission {
                        task_id,
                        request: io_request,
                    });
                }
                set_ctx_suspend_reason(ctx, &SuspendReason::IoWait);
                return BackendCallResult::suspended_with_tag(SuspendTag::IoWait);
            }
            NativeCallResult::Unhandled => {}
            NativeCallResult::Error(_) => {}
        }

        let native_name = crate::compiler::native_id::native_name(native_id);
        if let Some(handler) = shared.native_registry.read().get(native_name) {
            match handler(&engine_ctx, &native_args) {
                NativeCallResult::Value(val) => {
                    return BackendCallResult::completed_raw(native_to_value(val).raw())
                }
                NativeCallResult::Suspend(io_request) => {
                    if let Some(tx) = shared.io_submit_tx.lock().as_ref() {
                        let _ = tx.send(IoSubmission {
                            task_id,
                            request: io_request,
                        });
                    }
                    set_ctx_suspend_reason(ctx, &SuspendReason::IoWait);
                    return BackendCallResult::suspended_with_tag(SuspendTag::IoWait);
                }
                NativeCallResult::Unhandled => {}
                NativeCallResult::Error(_) => {}
            }
        }
    }

    aot_raise_type_error(
        ctx,
        format!("compiled kernel op {:?} has no exact AOT runtime implementation", kernel_op),
    );
    BackendCallResult::threw()
}

// =============================================================================
// Concurrency helpers
// =============================================================================

unsafe extern "C" fn helper_spawn(
    ctx: *mut AotTaskContext,
    func_id: u32,
    _args_ptr: *const u64,
    _argc: u32,
) -> u64 {
    aot_raise_type_error(
        ctx,
        format!("compiled AOT spawn for function {} is not implemented", func_id),
    );
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

unsafe extern "C" fn helper_run_sync_aot_call(
    ctx: *mut AotTaskContext,
    func_id: u32,
    local_count: u32,
    args_ptr: *const u64,
    argc: u32,
) -> u64 {
    let helpers = if ctx.is_null() {
        return abi::NULL_VALUE;
    } else {
        &(*ctx).helpers
    };
    let mut args = Vec::with_capacity(argc as usize);
    for i in 0..argc as usize {
        let raw = if args_ptr.is_null() {
            abi::NULL_VALUE
        } else {
            *args_ptr.add(i)
        };
        args.push(Value::from_raw(raw));
    }
    let frame = crate::aot::executor::allocate_initial_frame(
        func_id,
        local_count,
        dispatch_registered_aot_entry,
        &args,
        helpers,
    );
    if frame.is_null() {
        return abi::NULL_VALUE;
    }
    let result = crate::aot::executor::run_aot_function(frame, ctx, 100);
    match result.result {
        crate::vm::interpreter::ExecutionResult::Completed(value) => value.raw(),
        crate::vm::interpreter::ExecutionResult::Failed(error) => {
            aot_raise_type_error(ctx, format!("sync AOT call failed: {}", error));
            abi::NULL_VALUE
        }
        crate::vm::interpreter::ExecutionResult::Suspended(_) => {
            aot_raise_type_error(ctx, "sync AOT call suspended unexpectedly".to_string());
            abi::NULL_VALUE
        }
    }
}

unsafe extern "C" fn helper_prepare_aot_call_frame(
    ctx: *mut AotTaskContext,
    func_id: u32,
    local_count: u32,
    args_ptr: *const u64,
    argc: u32,
) -> *mut AotFrame {
    let helpers = if ctx.is_null() {
        return ptr::null_mut();
    } else {
        &(*ctx).helpers
    };
    let mut args = Vec::with_capacity(argc as usize);
    for i in 0..argc as usize {
        let raw = if args_ptr.is_null() {
            abi::NULL_VALUE
        } else {
            *args_ptr.add(i)
        };
        args.push(Value::from_raw(raw));
    }
    crate::aot::executor::allocate_initial_frame(
        func_id,
        local_count,
        dispatch_registered_aot_entry,
        &args,
        helpers,
    )
}

// =============================================================================
// Exceptions
// =============================================================================

unsafe extern "C" fn helper_throw_exception(ctx: *mut AotTaskContext, value: u64) {
    if let Some(task) = aot_task(ctx) {
        task.set_exception(Value::from_raw(value));
    }
}

// =============================================================================
// AOT function dispatch
// =============================================================================

fn raw_value_layout_id(raw: u64) -> Option<u32> {
    let value = unsafe { Value::from_raw(raw) };
    match js_classify(value) {
        JSView::Struct { layout_id, .. } => Some(layout_id),
        _ => None,
    }
}

fn select_registered_aot_func_ptr(func_id: u32, callee_frame: *mut AotFrame) -> AotEntryFn {
    let registry = aot_function_registry().read();
    let Some(entry) = registry.functions.get(&func_id).copied() else {
        return helper_trap_fn;
    };
    let clones = &registry.clones[entry.clones_start..entry.clones_start + entry.clones_len];
    if !callee_frame.is_null() {
        let param_count = unsafe { (*callee_frame).param_count as usize };
        let locals = unsafe { (*callee_frame).locals };
        for clone in clones {
            let (Some(guard_layout_id), Some(guard_arg_index)) =
                (clone.guard_layout_id, clone.guard_arg_index)
            else {
                continue;
            };
            let guard_arg_index = guard_arg_index as usize;
            if guard_arg_index >= param_count || locals.is_null() {
                continue;
            }
            let raw = unsafe { *locals.add(guard_arg_index) };
            if raw_value_layout_id(raw) == Some(guard_layout_id) {
                return unsafe { std::mem::transmute::<usize, AotEntryFn>(clone.ptr) };
            }
        }
    }
    unsafe { std::mem::transmute::<usize, AotEntryFn>(entry.baseline) }
}

pub unsafe extern "C" fn dispatch_registered_aot_entry(
    frame: *mut AotFrame,
    ctx: *mut AotTaskContext,
) -> u64 {
    if frame.is_null() {
        return helper_trap_fn(frame, ctx);
    }
    let func_id = (*frame).function_id;
    let target = select_registered_aot_func_ptr(func_id, frame);
    target(frame, ctx)
}

unsafe extern "C" fn helper_get_aot_func_ptr(
    func_id: u32,
    callee_frame: *mut AotFrame,
) -> AotEntryFn {
    let registry = aot_function_registry().read();
    if let Some(entry) = registry.functions.get(&func_id) {
        if entry.clones_len > 0 {
            return dispatch_registered_aot_entry;
        }
    }
    drop(registry);
    select_registered_aot_func_ptr(func_id, callee_frame)
}

/// Placeholder function for unresolved AOT calls.
unsafe extern "C" fn helper_trap_fn(_frame: *mut AotFrame, _ctx: *mut AotTaskContext) -> u64 {
    panic!("AOT function call to unresolved function");
}

// =============================================================================
// Constant pool access
// =============================================================================

unsafe extern "C" fn helper_load_string_constant(
    ctx: *mut AotTaskContext,
    const_index: u32,
) -> u64 {
    if ctx.is_null() || (*ctx).shared_state.is_null() || (*ctx).module.is_null() {
        return abi::NULL_VALUE;
    }
    let shared = &*((*ctx).shared_state as *const SharedVmState);
    let module = &*((*ctx).module as *const crate::compiler::Module);
    let Some(string) = module.constants.get_string(const_index) else {
        return abi::NULL_VALUE;
    };
    let mut gc = shared.gc.lock();
    let ptr = gc.allocate(RayaString::new(string.to_string()));
    Value::from_ptr(std::ptr::NonNull::new(ptr.as_ptr()).unwrap()).raw()
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

unsafe extern "C" fn helper_numeric_intrinsic(op_raw: u16, lhs_raw: u64, rhs_raw: u64) -> u64 {
    let Some(op) = CompiledNumericIntrinsicOp::from_u16(op_raw) else {
        return 0;
    };
    dispatch_compiled_numeric_intrinsic(op, lhs_raw, rhs_raw)
}

// =============================================================================
// Helper table construction
// =============================================================================

/// Create a fully populated `AotHelperTable` with all helper function pointers.
///
/// This is the default table used when no runtime is connected. Frame management
/// and NaN-boxing helpers work correctly across the active compiled path.
pub fn create_default_helper_table() -> AotHelperTable {
    AotHelperTable {
        alloc_frame: helper_alloc_frame,
        free_frame: helper_free_frame,
        safepoint_poll: helper_safepoint_poll,
        alloc_object: helper_alloc_object,
        alloc_structural_object: helper_alloc_structural_object,
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
        generic_add: helper_generic_add,
        generic_sub: helper_generic_sub,
        generic_mul: helper_generic_mul,
        generic_div: helper_generic_div,
        generic_mod: helper_generic_mod,
        generic_neg: helper_generic_neg,
        generic_not: helper_generic_not,
        generic_not_equal: helper_generic_not_equal,
        generic_strict_equals: helper_generic_strict_equals,
        generic_strict_not_equal: helper_generic_strict_not_equal,
        generic_less_equal: helper_generic_less_equal,
        generic_greater: helper_generic_greater,
        generic_greater_equal: helper_generic_greater_equal,
        to_string: helper_to_string,
        typeof_value: helper_typeof_value,
        string_compare: helper_string_compare,
        bind_method: helper_bind_method,
        get_arg_count: helper_get_arg_count,
        load_arg_local: helper_load_arg_local,
        object_get_field: helper_object_get_field,
        object_set_field: helper_object_set_field,
        object_is_nominal: helper_object_is_nominal,
        object_implements_shape: helper_object_implements_shape,
        object_get_shape_field: helper_object_get_shape_field,
        object_set_shape_field: helper_object_set_shape_field,
        cast_value: helper_cast_value,
        cast_shape: helper_cast_shape,
        dyn_get_prop: helper_dyn_get_prop,
        dyn_set_prop: helper_dyn_set_prop,
        load_global_value: helper_load_global_value,
        store_global_value: helper_store_global_value,
        native_call: helper_native_call,
        spawn: helper_spawn,
        check_preemption: helper_check_preemption,
        run_sync_aot_call: helper_run_sync_aot_call,
        prepare_aot_call_frame: helper_prepare_aot_call_frame,
        throw_exception: helper_throw_exception,
        get_aot_func_ptr: helper_get_aot_func_ptr,
        load_string_constant: helper_load_string_constant,
        load_i32_constant: helper_load_i32_constant,
        load_f64_constant: helper_load_f64_constant,
        numeric_intrinsic: helper_numeric_intrinsic,
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compiled_support::CompiledNumericIntrinsicOp;
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
            assert_eq!(helper_generic_equals(ptr::null_mut(), 42, 42), 1);
            assert_eq!(helper_generic_equals(ptr::null_mut(), 42, 43), 0);
            assert_eq!(
                helper_generic_equals(ptr::null_mut(), abi::NULL_VALUE, abi::NULL_VALUE),
                1
            );
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

            let eq = (table.generic_equals)(ptr::null_mut(), 100, 100);
            assert_eq!(eq, 1);

            let pow = (table.numeric_intrinsic)(CompiledNumericIntrinsicOp::I32Pow as u16, 2, 5);
            assert_eq!(pow as i32, 32);
        }
    }

    #[test]
    fn test_numeric_intrinsic_helper_matches_runtime_semantics() {
        unsafe {
            let i32_pow =
                helper_numeric_intrinsic(CompiledNumericIntrinsicOp::I32Pow as u16, 7, -1i64 as u64);
            assert_eq!(i32_pow as i32, 0);

            let f64_pow = helper_numeric_intrinsic(
                CompiledNumericIntrinsicOp::F64Pow as u16,
                9.0f64.to_bits(),
                0.5f64.to_bits(),
            );
            assert_eq!(f64::from_bits(f64_pow), 3.0);

            let f64_mod = helper_numeric_intrinsic(
                CompiledNumericIntrinsicOp::F64Mod as u16,
                7.5f64.to_bits(),
                2.0f64.to_bits(),
            );
            assert_eq!(f64::from_bits(f64_mod), 1.5);
        }
    }

    #[test]
    fn test_native_call_without_runtime_context_fails() {
        let preempt = AtomicBool::new(false);
        let mut ctx = AotTaskContext {
            preempt_requested: &preempt,
            resume_record: crate::vm::suspend::ResumeRecord::none(),
            suspend_record: SuspendRecord::none(),
            helpers: create_default_helper_table(),
            shared_state: ptr::null_mut(),
            current_task: ptr::null_mut(),
            module: ptr::null(),
        };
        let result = unsafe { helper_native_call(&mut ctx, 42, ptr::null(), 0) };
        assert_eq!(result.status, crate::vm::suspend::BackendCallStatus::Threw);
        assert_eq!(ctx.suspend_record, SuspendRecord::none());
    }

    #[test]
    fn test_native_call_unknown_kernel_op_fails() {
        let preempt = AtomicBool::new(false);
        let mut ctx = AotTaskContext {
            preempt_requested: &preempt,
            resume_record: crate::vm::suspend::ResumeRecord::none(),
            suspend_record: SuspendRecord::none(),
            helpers: create_default_helper_table(),
            shared_state: ptr::null_mut(),
            current_task: ptr::null_mut(),
            module: ptr::null(),
        };
        let result = unsafe { helper_native_call(&mut ctx, u16::MAX, ptr::null(), 0) };
        assert_eq!(result.status, crate::vm::suspend::BackendCallStatus::Threw);
        assert_eq!(ctx.suspend_record, SuspendRecord::none());
    }

    #[test]
    fn test_check_preemption_reads_flag() {
        let preempt = AtomicBool::new(false);
        let mut ctx = AotTaskContext {
            preempt_requested: &preempt,
            resume_record: crate::vm::suspend::ResumeRecord::none(),
            suspend_record: SuspendRecord::none(),
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
            resume_record: crate::vm::suspend::ResumeRecord::none(),
            suspend_record: SuspendRecord::none(),
            helpers: create_default_helper_table(),
            shared_state: Arc::as_ptr(&shared) as *mut (),
            current_task: ptr::null_mut(),
            module: ptr::null(),
        };

        let result = unsafe { helper_native_call(&mut ctx, 0, ptr::null(), 0) };
        assert_eq!(result.status, crate::vm::suspend::BackendCallStatus::Completed);
        assert_eq!(result.payload, Value::i32(77).raw());
        assert_eq!(ctx.suspend_record, SuspendRecord::none());
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
            resume_record: crate::vm::suspend::ResumeRecord::none(),
            suspend_record: SuspendRecord::none(),
            helpers: create_default_helper_table(),
            shared_state: Arc::as_ptr(&shared) as *mut (),
            current_task: ptr::null_mut(),
            module: ptr::null(),
        };

        let args = [Value::i32(7).raw(), Value::i32(11).raw()];
        let result = unsafe { helper_native_call(&mut ctx, 0, args.as_ptr(), args.len() as u8) };
        assert_eq!(result.status, crate::vm::suspend::BackendCallStatus::Completed);
        assert_eq!(result.payload, Value::i32(18).raw());
        assert_eq!(ctx.suspend_record, SuspendRecord::none());
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
            resume_record: crate::vm::suspend::ResumeRecord::none(),
            suspend_record: SuspendRecord::none(),
            helpers: create_default_helper_table(),
            shared_state: Arc::as_ptr(&shared) as *mut (),
            current_task: ptr::null_mut(),
            module: ptr::null(),
        };

        let result = unsafe { helper_native_call(&mut ctx, 0, ptr::null(), 0) };
        assert_eq!(result.status, crate::vm::suspend::BackendCallStatus::Suspended);
        assert_eq!(result.payload, SuspendTag::IoWait as u64);
        assert_eq!(ctx.suspend_record.tag, SuspendTag::IoWait);
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
            parent_name: None,
            methods: Vec::new(),
            static_methods: Vec::new(),
            runtime_instance_publication: false,
            runtime_static_publication: false,
        });
        let seed_module =
            Arc::new(Module::decode(&seed_module.encode()).expect("finalize seed module checksum"));
        shared
            .register_module(seed_module)
            .expect("register seed module");

        let mut target_module = Module::new("aot-target".to_string());
        target_module.classes.push(ClassDef {
            name: "Target".to_string(),
            field_count: 3,
            parent_id: None,
            parent_name: None,
            methods: Vec::new(),
            static_methods: Vec::new(),
            runtime_instance_publication: false,
            runtime_static_publication: false,
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
            resume_record: crate::vm::suspend::ResumeRecord::none(),
            suspend_record: SuspendRecord::none(),
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
