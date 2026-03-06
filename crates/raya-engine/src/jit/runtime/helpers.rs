//! Runtime helper implementations for JIT RuntimeContext.
//!
//! Phase 3 focus:
//! - wire safepoint + preemption helpers used by lowered machine-code branches
//! - provide conservative stubs for not-yet-lowered runtime helpers

use crate::compiler::Module;
use crate::jit::runtime::trampoline::{RuntimeContext, RuntimeHelperTable};
use crate::vm::abi::{native_to_value, value_to_native, EngineContext};
use crate::vm::gc::GarbageCollector;
use crate::vm::interpreter::{
    ClassRegistry, SafepointCoordinator, ShapeAdapter, StructuralAdapterKey,
    StructuralSlotBinding, StructuralViewHandle,
};
use crate::vm::native_registry::ResolvedNatives;
use crate::vm::object::{BoundMethod, Object};
use crate::vm::reflect::ClassMetadataRegistry;
use crate::vm::scheduler::IoSubmission;
use crate::vm::scheduler::Task;
use crate::vm::value::Value;
use raya_sdk::NativeCallResult;
use rustc_hash::FxHashMap;
use std::ptr::NonNull;
use std::sync::Arc;

/// Sentinel returned by JIT native helper dispatch when the native call suspended.
/// Distinct from valid NaN-boxed Values.
pub const JIT_NATIVE_SUSPEND_SENTINEL: u64 = 0xFFFF_DEAD_0000_0001;

#[repr(C)]
pub struct JitRuntimeBridgeContext {
    pub safepoint: *const SafepointCoordinator,
    pub task: *const Task,
    pub gc: *const parking_lot::Mutex<GarbageCollector>,
    pub classes: *const parking_lot::RwLock<ClassRegistry>,
    pub class_metadata: *const parking_lot::RwLock<ClassMetadataRegistry>,
    pub resolved_natives: *const parking_lot::RwLock<ResolvedNatives>,
    pub structural_slot_views:
        *const parking_lot::RwLock<FxHashMap<([u8; 32], usize, u64), StructuralViewHandle>>,
    pub structural_shape_adapters: *const parking_lot::RwLock<
        FxHashMap<StructuralAdapterKey, Arc<ShapeAdapter>>,
    >,
    pub io_submit_tx: *const crossbeam::channel::Sender<IoSubmission>,
}

/// Build a runtime context for a JIT invocation running inside interpreter thread loop.
#[inline]
pub fn build_runtime_bridge_context(
    safepoint: &SafepointCoordinator,
    task: &Task,
    gc: &parking_lot::Mutex<GarbageCollector>,
    classes: &parking_lot::RwLock<ClassRegistry>,
    class_metadata: &parking_lot::RwLock<ClassMetadataRegistry>,
    resolved_natives: &parking_lot::RwLock<ResolvedNatives>,
    structural_slot_views: &parking_lot::RwLock<
        FxHashMap<([u8; 32], usize, u64), StructuralViewHandle>,
    >,
    structural_shape_adapters: &parking_lot::RwLock<
        FxHashMap<StructuralAdapterKey, Arc<ShapeAdapter>>,
    >,
    io_submit_tx: Option<&crossbeam::channel::Sender<IoSubmission>>,
) -> JitRuntimeBridgeContext {
    JitRuntimeBridgeContext {
        safepoint: safepoint as *const SafepointCoordinator,
        task: task as *const Task,
        gc: gc as *const _,
        classes: classes as *const _,
        class_metadata: class_metadata as *const _,
        resolved_natives: resolved_natives as *const _,
        structural_slot_views: structural_slot_views as *const _,
        structural_shape_adapters: structural_shape_adapters as *const _,
        io_submit_tx: io_submit_tx.map_or(std::ptr::null(), |tx| tx as *const _),
    }
}

#[inline]
pub fn build_runtime_context(bridge: &JitRuntimeBridgeContext, module: &Module) -> RuntimeContext {
    RuntimeContext {
        shared_state: (bridge as *const JitRuntimeBridgeContext).cast::<()>(),
        current_task: bridge.task.cast::<()>(),
        module: (module as *const Module).cast::<()>(),
        helpers: runtime_helpers(),
    }
}

#[inline]
pub fn runtime_helpers() -> RuntimeHelperTable {
    RuntimeHelperTable {
        alloc_object: helper_alloc_object,
        alloc_array: helper_alloc_array,
        alloc_string: helper_alloc_string,
        safepoint_poll: helper_safepoint_poll,
        check_preemption: helper_check_preemption,
        native_call_dispatch: helper_native_call_dispatch,
        interpreter_call: helper_interpreter_call,
        throw_exception: helper_throw_exception,
        deoptimize: helper_deoptimize,
        string_concat: helper_string_concat,
        generic_equals: helper_generic_equals,
        object_get_field: helper_object_get_field,
        object_set_field: helper_object_set_field,
    }
}

unsafe extern "C" fn helper_alloc_object(class_id: u32, shared_state: *mut ()) -> *mut () {
    if shared_state.is_null() {
        return std::ptr::null_mut();
    }
    let bridge = &*(shared_state.cast::<JitRuntimeBridgeContext>());
    if bridge.gc.is_null() || bridge.classes.is_null() {
        return std::ptr::null_mut();
    }

    let class_id = class_id as usize;
    let (field_count, layout_id) = {
        let classes = (&*bridge.classes).read();
        match classes.get_class(class_id) {
            Some(class) => (class.field_count, class.layout_id),
            None => return std::ptr::null_mut(),
        }
    };

    let mut gc = (&*bridge.gc).lock();
    let obj_ptr = gc.allocate(Object::new_nominal(layout_id, class_id as u32, field_count));
    obj_ptr.as_ptr().cast::<()>()
}

unsafe extern "C" fn helper_alloc_array(
    _type_id: u32,
    _capacity: usize,
    _shared_state: *mut (),
) -> *mut () {
    std::ptr::null_mut()
}

unsafe extern "C" fn helper_alloc_string(
    _data_ptr: *const u8,
    _len: usize,
    _shared_state: *mut (),
) -> *mut () {
    std::ptr::null_mut()
}

unsafe extern "C" fn helper_safepoint_poll(shared_state: *const ()) {
    if shared_state.is_null() {
        return;
    }
    let bridge = &*(shared_state.cast::<JitRuntimeBridgeContext>());
    if bridge.safepoint.is_null() {
        return;
    }
    let safepoint = &*bridge.safepoint;
    safepoint.poll();
}

unsafe extern "C" fn helper_check_preemption(current_task: *const ()) -> bool {
    if current_task.is_null() {
        return false;
    }
    let task = &*(current_task.cast::<Task>());
    task.is_preempt_requested()
}

unsafe extern "C" fn helper_native_call_dispatch(
    native_id: u16,
    args_ptr: *const u64,
    arg_count: u8,
    shared_state: *mut (),
) -> u64 {
    if !shared_state.is_null() {
        let bridge = &*(shared_state.cast::<JitRuntimeBridgeContext>());
        if !bridge.gc.is_null()
            && !bridge.classes.is_null()
            && !bridge.class_metadata.is_null()
            && !bridge.resolved_natives.is_null()
        {
            let task_id = if !bridge.task.is_null() {
                (*bridge.task).id()
            } else {
                crate::vm::scheduler::TaskId::from_u64(0)
            };

            let ctx = EngineContext::new(
                &*bridge.gc,
                &*bridge.classes,
                task_id,
                &*bridge.class_metadata,
            );

            let value_args: Vec<Value> = if arg_count == 0 || args_ptr.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(args_ptr, arg_count as usize)
                    .iter()
                    .copied()
                    .map(|raw| Value::from_raw(raw))
                    .collect()
            };
            let native_args: Vec<raya_sdk::NativeValue> =
                value_args.iter().map(|v| value_to_native(*v)).collect();

            let resolved = (&*bridge.resolved_natives).read();
            match resolved.call(native_id, &ctx, &native_args) {
                NativeCallResult::Value(v) => return native_to_value(v).raw(),
                NativeCallResult::Suspend(io_request) => {
                    if !bridge.io_submit_tx.is_null() {
                        let tx = &*bridge.io_submit_tx;
                        let _ = tx.send(IoSubmission {
                            task_id,
                            request: io_request,
                        });
                    }
                    return JIT_NATIVE_SUSPEND_SENTINEL;
                }
                NativeCallResult::Unhandled | NativeCallResult::Error(_) => {}
            }
        }
    }
    Value::null().raw()
}

unsafe extern "C" fn helper_interpreter_call(
    _func_index: u32,
    _args_ptr: *const u64,
    _arg_count: u16,
    _shared_state: *mut (),
) -> u64 {
    Value::null().raw()
}

unsafe extern "C" fn helper_throw_exception(_exception_value: u64, _shared_state: *mut ()) {
    panic!("helper_throw_exception is not wired yet")
}

unsafe extern "C" fn helper_deoptimize(_bytecode_offset: u32, _shared_state: *mut ()) {
    panic!("helper_deoptimize is not wired yet")
}

unsafe extern "C" fn helper_string_concat(_left: u64, _right: u64, _shared_state: *mut ()) -> u64 {
    Value::null().raw()
}

unsafe extern "C" fn helper_generic_equals(
    _left: u64,
    _right: u64,
    _shared_state: *mut (),
) -> bool {
    false
}

unsafe fn remap_structural_slot_binding(
    bridge: &JitRuntimeBridgeContext,
    module: &Module,
    func_id: usize,
    object: &Object,
    expected_slot: usize,
) -> StructuralSlotBinding {
    if bridge.structural_slot_views.is_null() || bridge.structural_shape_adapters.is_null() {
        return StructuralSlotBinding::Field(expected_slot);
    }

    let views = (&*bridge.structural_slot_views).read();
    let handle = views
        .get(&(module.checksum, func_id, object.object_id()))
        .or_else(|| views.get(&(module.checksum, usize::MAX, object.object_id())))
        .copied();
    drop(views);

    let Some(handle) = handle else {
        return StructuralSlotBinding::Field(expected_slot);
    };

    let adapters = (&*bridge.structural_shape_adapters).read();
    adapters
        .get(&handle.adapter_key)
        .map(|adapter: &Arc<ShapeAdapter>| adapter.binding_for_slot(expected_slot))
        .unwrap_or(StructuralSlotBinding::Missing)
}

unsafe extern "C" fn helper_object_get_field(
    object_raw: u64,
    expected_slot: u32,
    func_id: u32,
    module_ptr: *const (),
    shared_state: *mut (),
) -> u64 {
    if shared_state.is_null() || module_ptr.is_null() {
        return Value::null().raw();
    }
    let bridge = &*(shared_state.cast::<JitRuntimeBridgeContext>());
    if bridge.classes.is_null() || bridge.gc.is_null() {
        return Value::null().raw();
    }

    let object_val = Value::from_raw(object_raw);
    let Some(object_ptr) = object_val.as_ptr::<Object>() else {
        return Value::null().raw();
    };
    let object = &*object_ptr.as_ptr();
    let module = &*(module_ptr.cast::<Module>());
    let binding = remap_structural_slot_binding(
        bridge,
        module,
        func_id as usize,
        object,
        expected_slot as usize,
    );

    match binding {
        StructuralSlotBinding::Field(slot) => object.get_field(slot).unwrap_or(Value::null()).raw(),
        StructuralSlotBinding::Method(method_slot) => {
            let Some(class_id) = object.nominal_class_id() else {
                return Value::null().raw();
            };
            let (func_id, method_module) = {
                let classes = (&*bridge.classes).read();
                let Some(class) = classes.get_class(class_id) else {
                    return Value::null().raw();
                };
                let Some(fid) = class.vtable.get_method(method_slot) else {
                    return Value::null().raw();
                };
                (fid, class.module.clone())
            };

            let bound = BoundMethod {
                receiver: object_val,
                func_id,
                module: method_module,
            };
            let mut gc = (&*bridge.gc).lock();
            let bm_ptr = gc.allocate(bound);
            Value::from_ptr(NonNull::new(bm_ptr.as_ptr()).unwrap()).raw()
        }
        StructuralSlotBinding::Missing => Value::null().raw(),
    }
}

unsafe extern "C" fn helper_object_set_field(
    object_raw: u64,
    expected_slot: u32,
    value_raw: u64,
    func_id: u32,
    module_ptr: *const (),
    shared_state: *mut (),
) -> bool {
    if shared_state.is_null() || module_ptr.is_null() {
        return false;
    }
    let bridge = &*(shared_state.cast::<JitRuntimeBridgeContext>());
    let object_val = Value::from_raw(object_raw);
    let Some(object_ptr) = object_val.as_ptr::<Object>() else {
        return false;
    };
    let object = &mut *object_ptr.as_ptr();
    let module = &*(module_ptr.cast::<Module>());
    let binding = remap_structural_slot_binding(
        bridge,
        module,
        func_id as usize,
        object,
        expected_slot as usize,
    );
    match binding {
        StructuralSlotBinding::Field(slot) => {
            object.set_field(slot, Value::from_raw(value_raw)).is_ok()
        }
        StructuralSlotBinding::Method(_) | StructuralSlotBinding::Missing => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam::channel::unbounded;
    use crossbeam_deque::Injector;
    use parking_lot::RwLock;
    use rustc_hash::FxHashMap;
    use std::sync::Arc;

    #[test]
    fn jit_helper_native_dispatch_returns_resolved_value() {
        let safepoint = Arc::new(SafepointCoordinator::new(1));
        let tasks = Arc::new(RwLock::new(FxHashMap::default()));
        let injector = Arc::new(Injector::new());
        let shared = Arc::new(crate::vm::interpreter::SharedVmState::new(
            safepoint.clone(),
            tasks,
            injector,
        ));
        {
            let mut reg = shared.native_registry.write();
            reg.register("jit.native.value", |_ctx, _args| NativeCallResult::i32(88));
            let resolved =
                ResolvedNatives::link(&["jit.native.value".to_string()], &reg).expect("link");
            *shared.resolved_natives.write() = resolved;
        }

        let module = Arc::new(Module::new("jit-test".to_string()));
        let task = Task::new(0, module, None);
        let bridge = build_runtime_bridge_context(
            safepoint.as_ref(),
            &task,
            &shared.gc,
            &shared.classes,
            &shared.class_metadata,
            &shared.resolved_natives,
            &shared.structural_slot_views,
            &shared.structural_shape_adapters,
            None,
        );

        let raw = unsafe {
            helper_native_call_dispatch(
                0,
                std::ptr::null(),
                0,
                (&bridge as *const JitRuntimeBridgeContext) as *mut (),
            )
        };
        assert_eq!(raw, Value::i32(88).raw());
    }

    #[test]
    fn jit_helper_native_dispatch_submits_io_on_suspend() {
        let safepoint = Arc::new(SafepointCoordinator::new(1));
        let tasks = Arc::new(RwLock::new(FxHashMap::default()));
        let injector = Arc::new(Injector::new());
        let shared = Arc::new(crate::vm::interpreter::SharedVmState::new(
            safepoint.clone(),
            tasks,
            injector,
        ));
        let (tx, rx) = unbounded();
        *shared.io_submit_tx.lock() = Some(tx.clone());
        {
            let mut reg = shared.native_registry.write();
            reg.register("jit.native.suspend", |_ctx, _args| {
                NativeCallResult::Suspend(raya_sdk::IoRequest::Sleep { duration_nanos: 1 })
            });
            let resolved =
                ResolvedNatives::link(&["jit.native.suspend".to_string()], &reg).expect("link");
            *shared.resolved_natives.write() = resolved;
        }

        let module = Arc::new(Module::new("jit-test".to_string()));
        let task = Task::new(0, module, None);
        let bridge = build_runtime_bridge_context(
            safepoint.as_ref(),
            &task,
            &shared.gc,
            &shared.classes,
            &shared.class_metadata,
            &shared.resolved_natives,
            &shared.structural_slot_views,
            &shared.structural_shape_adapters,
            Some(&tx),
        );

        let raw = unsafe {
            helper_native_call_dispatch(
                0,
                std::ptr::null(),
                0,
                (&bridge as *const JitRuntimeBridgeContext) as *mut (),
            )
        };
        assert_eq!(raw, JIT_NATIVE_SUSPEND_SENTINEL);
        let submission = rx.try_recv().expect("expected io submission");
        assert_eq!(submission.task_id.as_u64(), task.id().as_u64());
        assert!(matches!(
            submission.request,
            raya_sdk::IoRequest::Sleep { duration_nanos: 1 }
        ));
    }
}
