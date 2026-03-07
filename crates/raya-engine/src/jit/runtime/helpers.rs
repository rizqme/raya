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
    ClassRegistry, ModuleRuntimeLayout, RuntimeLayoutRegistry, SafepointCoordinator, ShapeAdapter,
    StructuralAdapterKey, StructuralSlotBinding,
};
use crate::vm::native_registry::ResolvedNatives;
use crate::vm::object::{global_layout_names, BoundMethod, Object};
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
    pub layouts: *const parking_lot::RwLock<RuntimeLayoutRegistry>,
    pub module_layouts:
        *const parking_lot::RwLock<FxHashMap<[u8; 32], ModuleRuntimeLayout>>,
    pub class_metadata: *const parking_lot::RwLock<ClassMetadataRegistry>,
    pub resolved_natives: *const parking_lot::RwLock<ResolvedNatives>,
    pub structural_shape_names:
        *const parking_lot::RwLock<FxHashMap<u64, Vec<String>>>,
    pub structural_shape_adapters: *const parking_lot::RwLock<
        FxHashMap<StructuralAdapterKey, Arc<ShapeAdapter>>,
    >,
    pub prop_keys: *const parking_lot::RwLock<crate::vm::interpreter::PropertyKeyRegistry>,
    pub io_submit_tx: *const crossbeam::channel::Sender<IoSubmission>,
}

/// Build a runtime context for a JIT invocation running inside interpreter thread loop.
#[inline]
pub fn build_runtime_bridge_context(
    safepoint: &SafepointCoordinator,
    task: &Task,
    gc: &parking_lot::Mutex<GarbageCollector>,
    classes: &parking_lot::RwLock<ClassRegistry>,
    layouts: &parking_lot::RwLock<RuntimeLayoutRegistry>,
    module_layouts: &parking_lot::RwLock<FxHashMap<[u8; 32], ModuleRuntimeLayout>>,
    class_metadata: &parking_lot::RwLock<ClassMetadataRegistry>,
    resolved_natives: &parking_lot::RwLock<ResolvedNatives>,
    structural_shape_names: &parking_lot::RwLock<FxHashMap<u64, Vec<String>>>,
    structural_shape_adapters: &parking_lot::RwLock<
        FxHashMap<StructuralAdapterKey, Arc<ShapeAdapter>>,
    >,
    prop_keys: &parking_lot::RwLock<crate::vm::interpreter::PropertyKeyRegistry>,
    io_submit_tx: Option<&crossbeam::channel::Sender<IoSubmission>>,
) -> JitRuntimeBridgeContext {
    JitRuntimeBridgeContext {
        safepoint: safepoint as *const SafepointCoordinator,
        task: task as *const Task,
        gc: gc as *const _,
        classes: classes as *const _,
        layouts: layouts as *const _,
        module_layouts: module_layouts as *const _,
        class_metadata: class_metadata as *const _,
        resolved_natives: resolved_natives as *const _,
        structural_shape_names: structural_shape_names as *const _,
        structural_shape_adapters: structural_shape_adapters as *const _,
        prop_keys: prop_keys as *const _,
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
        object_implements_shape: helper_object_implements_shape,
        object_is_nominal: helper_object_is_nominal,
    }
}

#[inline]
unsafe fn jit_object_ptr_checked(value: Value) -> Option<NonNull<Object>> {
    if !value.is_ptr() {
        return None;
    }
    let ptr = value.as_ptr::<u8>()?;
    let header = {
        let hp = ptr.as_ptr().sub(std::mem::size_of::<crate::vm::gc::GcHeader>());
        &*(hp as *const crate::vm::gc::GcHeader)
    };
    if header.type_id() == std::any::TypeId::of::<Object>() {
        value.as_ptr::<Object>()
    } else {
        None
    }
}

fn jit_layout_field_names(
    bridge: &JitRuntimeBridgeContext,
    object: &Object,
) -> Option<Vec<String>> {
    if !bridge.layouts.is_null() {
        let layouts = unsafe { &*bridge.layouts }.read();
        if let Some(names) = layouts.layout_field_names(object.layout_id()) {
            return Some(names.to_vec());
        }
    }
    global_layout_names(object.layout_id())
}

fn jit_build_shape_slot_map_for_object(
    bridge: &JitRuntimeBridgeContext,
    object: &Object,
    required_names: &[String],
) -> Option<Vec<StructuralSlotBinding>> {
    let layout_names = jit_layout_field_names(bridge, object);
    let dynamic_binding_for = |name: &str| -> Option<StructuralSlotBinding> {
        if bridge.prop_keys.is_null() {
            return None;
        }
        let key = unsafe { &*bridge.prop_keys }.write().intern(name);
        object
            .dyn_map()
            .and_then(|dyn_map| dyn_map.contains_key(&key).then_some(StructuralSlotBinding::Dynamic(key)))
    };

    if let Some(nominal_type_id) = object.nominal_type_id_usize() {
        let class_meta = if bridge.class_metadata.is_null() {
            None
        } else {
            unsafe { &*bridge.class_metadata }.read().get(nominal_type_id).cloned()
        };
        return Some(
            required_names
                .iter()
                .map(|name| {
                    class_meta
                        .as_ref()
                        .and_then(|meta| meta.get_field_index(name))
                        .and_then(|index| {
                            (index < object.field_count()).then_some(StructuralSlotBinding::Field(index))
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

fn jit_ensure_shape_adapter_for_object(
    bridge: &JitRuntimeBridgeContext,
    object: &Object,
    required_shape: u64,
) -> Option<Arc<ShapeAdapter>> {
    if bridge.structural_shape_adapters.is_null() {
        return None;
    }

    let adapter_key = StructuralAdapterKey {
        provider_layout: object.layout_id(),
        required_shape,
    };
    if let Some(adapter) = unsafe { &*bridge.structural_shape_adapters }
        .read()
        .get(&adapter_key)
        .cloned()
    {
        return Some(adapter);
    }

    if bridge.structural_shape_names.is_null() {
        return None;
    }
    let required_names = unsafe { &*bridge.structural_shape_names }
        .read()
        .get(&required_shape)
        .cloned()?;
    let slot_map = jit_build_shape_slot_map_for_object(bridge, object, &required_names)?;
    let adapter = Arc::new(ShapeAdapter::from_slot_map(
        object.layout_id(),
        required_shape,
        &slot_map,
    ));
    let mut adapters = unsafe { &*bridge.structural_shape_adapters }.write();
    Some(
        adapters
            .entry(adapter_key)
            .or_insert_with(|| adapter.clone())
            .clone(),
    )
}

fn jit_resolve_nominal_type_id(
    bridge: &JitRuntimeBridgeContext,
    module: &Module,
    local_nominal_type_index: u32,
) -> Option<usize> {
    if bridge.module_layouts.is_null() {
        return None;
    }
    let module_layouts = unsafe { &*bridge.module_layouts }.read();
    let module_layout = module_layouts.get(&module.checksum)?;
    if local_nominal_type_index as usize >= module_layout.nominal_type_len {
        return None;
    }
    Some(module_layout.nominal_type_base + local_nominal_type_index as usize)
}

fn jit_object_matches_nominal_type(
    bridge: &JitRuntimeBridgeContext,
    object: &Object,
    target_nominal_type_id: usize,
) -> bool {
    let Some(mut current_nominal_type_id) = object.nominal_type_id_usize() else {
        return false;
    };
    if bridge.classes.is_null() {
        return false;
    }
    let classes = unsafe { &*bridge.classes }.read();
    loop {
        if current_nominal_type_id == target_nominal_type_id {
            return true;
        }
        let Some(class) = classes.get_class(current_nominal_type_id) else {
            return false;
        };
        let Some(parent_id) = class.parent_id else {
            return false;
        };
        current_nominal_type_id = parent_id;
    }
}

unsafe extern "C" fn helper_alloc_object(
    local_nominal_type_index: u32,
    module_ptr: *const (),
    shared_state: *mut (),
) -> *mut () {
    if shared_state.is_null() || module_ptr.is_null() {
        return std::ptr::null_mut();
    }
    let bridge = &*(shared_state.cast::<JitRuntimeBridgeContext>());
    if bridge.gc.is_null() || bridge.layouts.is_null() || bridge.module_layouts.is_null() {
        return std::ptr::null_mut();
    }

    let module = &*(module_ptr.cast::<Module>());
    let Some(nominal_type_id) = jit_resolve_nominal_type_id(bridge, module, local_nominal_type_index)
    else {
        return std::ptr::null_mut();
    };
    let (field_count, layout_id) = {
        let layouts = (&*bridge.layouts).read();
        match layouts.nominal_allocation(nominal_type_id) {
            Some((layout_id, field_count)) => (field_count, layout_id),
            None => return std::ptr::null_mut(),
        }
    };

    let mut gc = (&*bridge.gc).lock();
    let obj_ptr = gc.allocate(Object::new_nominal(
        layout_id,
        nominal_type_id as u32,
        field_count,
    ));
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
            && !bridge.layouts.is_null()
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
                &*bridge.layouts,
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
    let Some(object_ptr) = jit_object_ptr_checked(object_val) else {
        return Value::null().raw();
    };
    let object = &*object_ptr.as_ptr();
    let _ = bridge;
    let _ = module_ptr;
    let _ = func_id;
    let binding = StructuralSlotBinding::Field(expected_slot as usize);

    match binding {
        StructuralSlotBinding::Field(slot) => object.get_field(slot).unwrap_or(Value::null()).raw(),
        StructuralSlotBinding::Method(method_slot) => {
            let Some(nominal_type_id) = object.nominal_type_id_usize() else {
                return Value::null().raw();
            };
            let (func_id, method_module) = {
                let classes = (&*bridge.classes).read();
                let Some(class) = classes.get_class(nominal_type_id) else {
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
        StructuralSlotBinding::Dynamic(key) => object
            .dyn_map()
            .and_then(|dyn_map| dyn_map.get(&key).copied())
            .unwrap_or(Value::null())
            .raw(),
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
    let Some(object_ptr) = jit_object_ptr_checked(object_val) else {
        return false;
    };
    let object = &mut *object_ptr.as_ptr();
    let _ = bridge;
    let _ = module_ptr;
    let _ = func_id;
    let binding = StructuralSlotBinding::Field(expected_slot as usize);
    match binding {
        StructuralSlotBinding::Field(slot) => {
            object.set_field(slot, Value::from_raw(value_raw)).is_ok()
        }
        StructuralSlotBinding::Dynamic(key) => {
            object.ensure_dyn_map().insert(key, Value::from_raw(value_raw));
            true
        }
        StructuralSlotBinding::Method(_) | StructuralSlotBinding::Missing => false,
    }
}

unsafe extern "C" fn helper_object_implements_shape(
    object_raw: u64,
    required_shape: u64,
    shared_state: *mut (),
) -> bool {
    if shared_state.is_null() {
        return false;
    }
    let bridge = &*(shared_state.cast::<JitRuntimeBridgeContext>());
    let object_val = Value::from_raw(object_raw);
    let Some(object_ptr) = jit_object_ptr_checked(object_val) else {
        return false;
    };
    let object = &*object_ptr.as_ptr();
    let Some(adapter) = jit_ensure_shape_adapter_for_object(bridge, object, required_shape) else {
        return false;
    };
    (0..adapter.len()).all(|slot| {
        !matches!(adapter.binding_for_slot(slot), StructuralSlotBinding::Missing)
    })
}

unsafe extern "C" fn helper_object_is_nominal(
    object_raw: u64,
    local_nominal_type_index: u32,
    module_ptr: *const (),
    shared_state: *mut (),
) -> bool {
    if shared_state.is_null() || module_ptr.is_null() {
        return false;
    }
    let bridge = &*(shared_state.cast::<JitRuntimeBridgeContext>());
    let module = &*(module_ptr.cast::<Module>());
    let Some(target_nominal_type_id) =
        jit_resolve_nominal_type_id(bridge, module, local_nominal_type_index)
    else {
        return false;
    };
    let object_val = Value::from_raw(object_raw);
    let Some(object_ptr) = jit_object_ptr_checked(object_val) else {
        return false;
    };
    let object = &*object_ptr.as_ptr();
    jit_object_matches_nominal_type(bridge, object, target_nominal_type_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::bytecode::ClassDef;
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
            &shared.layouts,
            &shared.module_layouts,
            &shared.class_metadata,
            &shared.resolved_natives,
            &shared.structural_shape_names,
            &shared.structural_shape_adapters,
            &shared.prop_keys,
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
            &shared.layouts,
            &shared.module_layouts,
            &shared.class_metadata,
            &shared.resolved_natives,
            &shared.structural_shape_names,
            &shared.structural_shape_adapters,
            &shared.prop_keys,
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

    #[test]
    fn jit_helper_alloc_object_resolves_module_local_nominal_type_index() {
        let safepoint = Arc::new(SafepointCoordinator::new(1));
        let tasks = Arc::new(RwLock::new(FxHashMap::default()));
        let injector = Arc::new(Injector::new());
        let shared = Arc::new(crate::vm::interpreter::SharedVmState::new(
            safepoint.clone(),
            tasks,
            injector,
        ));

        let mut seed_module = Module::new("jit-seed".to_string());
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

        let mut target_module = Module::new("jit-target".to_string());
        target_module.classes.push(ClassDef {
            name: "Target".to_string(),
            field_count: 2,
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

        let task = Task::new(0, target_module.clone(), None);
        let bridge = build_runtime_bridge_context(
            safepoint.as_ref(),
            &task,
            &shared.gc,
            &shared.classes,
            &shared.layouts,
            &shared.module_layouts,
            &shared.class_metadata,
            &shared.resolved_natives,
            &shared.structural_shape_names,
            &shared.structural_shape_adapters,
            &shared.prop_keys,
            None,
        );

        let object_ptr = unsafe {
            helper_alloc_object(
                0,
                Arc::as_ptr(&target_module) as *const (),
                (&bridge as *const JitRuntimeBridgeContext) as *mut (),
            )
        };
        assert!(!object_ptr.is_null());

        let obj = unsafe { &*(object_ptr.cast::<Object>()) };
        assert_eq!(obj.nominal_type_id_usize(), Some(expected_nominal_type_id));
        assert_eq!(obj.field_count(), 2);
    }
}
