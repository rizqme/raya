//! Runtime helper implementations for JIT RuntimeContext.
//!
//! Phase 3 focus:
//! - wire safepoint + preemption helpers used by lowered machine-code branches
//! - provide conservative runtime helpers for lowered machine-code branches

use crate::compiler::ir::{decode_kernel_op_id, KernelOp};
use crate::compiler::{Module, Opcode};
use crate::jit::runtime::trampoline::{RuntimeContext, RuntimeHelperTable};
use crate::vm::abi::{native_to_value, value_to_native, EngineContext};
use crate::vm::gc::GarbageCollector;
use crate::vm::interpreter::{
    ClassRegistry, ExecutionFrame, Interpreter, ModuleRuntimeLayout, ReturnAction,
    RuntimeLayoutRegistry, SafepointCoordinator, ShapeAdapter, StructuralAdapterKey,
    StructuralSlotBinding,
};
use crate::vm::native_handler::NativeHandler;
use crate::vm::native_registry::ResolvedNatives;
use crate::vm::object::{global_layout_names, DynProp, Object, RayaString};
use crate::vm::reflect::ClassMetadataRegistry;
use crate::vm::scheduler::IoSubmission;
use crate::vm::scheduler::{Task, TaskId};
use crate::vm::stack::Stack;
use crate::vm::suspend::BackendCallResult;
use crate::vm::suspend::{SuspendReason, SuspendTag};
use crate::vm::sync::{MutexRegistry, SemaphoreRegistry};
use crate::vm::value::Value;
use crate::vm::VmError;
use crossbeam_deque::Injector;
use raya_sdk::NativeCallResult;
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::RefCell;
use std::ptr::NonNull;
use std::sync::Arc;

pub const JIT_SHAPE_FIELD_FALLBACK_SENTINEL: u64 = 0xFFFF_DEAD_0000_0004;
pub const JIT_STRING_LEN_FALLBACK_SENTINEL: i32 = i32::MIN;
const JIT_SHAPE_ADAPTER_PIC_CAPACITY: usize = 4;

thread_local! {
    static JIT_SHAPE_ADAPTER_LAST: RefCell<Option<(StructuralAdapterKey, u32, Arc<ShapeAdapter>)>> =
        const { RefCell::new(None) };
    static JIT_SHAPE_ADAPTER_PIC: RefCell<Vec<(StructuralAdapterKey, u32, Arc<ShapeAdapter>)>> =
        const { RefCell::new(Vec::new()) };
}

const JIT_STORE_SUCCESS: i8 = 1;
const JIT_STORE_FALLBACK: i8 = 0;

#[repr(C)]
pub struct JitRuntimeBridgeContext {
    pub safepoint: *const SafepointCoordinator,
    pub task: *const Task,
    pub task_arc: *const Arc<Task>,
    pub gc: *const parking_lot::Mutex<GarbageCollector>,
    pub classes: *const parking_lot::RwLock<ClassRegistry>,
    pub layouts: *const parking_lot::RwLock<RuntimeLayoutRegistry>,
    pub mutex_registry: *const MutexRegistry,
    pub semaphore_registry: *const SemaphoreRegistry,
    pub globals_by_index: *const parking_lot::RwLock<Vec<Value>>,
    pub builtin_global_slots: *const parking_lot::RwLock<FxHashMap<String, usize>>,
    pub js_global_bindings: *const parking_lot::RwLock<
        FxHashMap<String, crate::vm::interpreter::JsGlobalBindingRecord>,
    >,
    pub js_global_binding_slots: *const parking_lot::RwLock<FxHashMap<usize, String>>,
    pub constant_string_cache: *const parking_lot::RwLock<FxHashMap<(String, usize), Value>>,
    pub ephemeral_gc_roots: *const parking_lot::RwLock<Vec<Value>>,
    pub pinned_handles: *const parking_lot::RwLock<rustc_hash::FxHashSet<u64>>,
    pub tasks: *const Arc<parking_lot::RwLock<FxHashMap<TaskId, Arc<Task>>>>,
    pub injector: *const Arc<Injector<Arc<Task>>>,
    pub promise_microtasks: *const parking_lot::Mutex<
        std::collections::VecDeque<crate::vm::interpreter::PromiseMicrotask>,
    >,
    pub test262_async_state: *const std::sync::atomic::AtomicU8,
    pub test262_async_failure: *const parking_lot::Mutex<Option<String>>,
    pub module_layouts: *const parking_lot::RwLock<FxHashMap<[u8; 32], ModuleRuntimeLayout>>,
    pub module_registry: *const parking_lot::RwLock<crate::vm::interpreter::ModuleRegistry>,
    pub metadata: *const parking_lot::Mutex<crate::vm::reflect::MetadataStore>,
    pub class_metadata: *const parking_lot::RwLock<ClassMetadataRegistry>,
    pub native_handler: *const Arc<dyn NativeHandler>,
    pub resolved_natives: *const parking_lot::RwLock<ResolvedNatives>,
    pub structural_shape_names: *const parking_lot::RwLock<FxHashMap<u64, Vec<String>>>,
    pub structural_layout_shapes:
        *const parking_lot::RwLock<FxHashMap<crate::vm::object::LayoutId, Vec<String>>>,
    pub structural_shape_adapters:
        *const parking_lot::RwLock<FxHashMap<StructuralAdapterKey, Arc<ShapeAdapter>>>,
    pub aot_profile: *const parking_lot::RwLock<crate::aot_profile::AotProfileCollector>,
    pub type_handles: *const parking_lot::RwLock<crate::vm::interpreter::RuntimeTypeHandleRegistry>,
    pub class_value_slots: *const parking_lot::RwLock<FxHashMap<usize, usize>>,
    pub prop_keys: *const parking_lot::RwLock<crate::vm::interpreter::PropertyKeyRegistry>,
    pub stack_pool: *const crate::vm::scheduler::StackPool,
    pub io_submit_tx: *const crossbeam::channel::Sender<IoSubmission>,
    pub max_preemptions: u32,
    pub current_frame_depth: usize,
}

/// Build a runtime context for a JIT invocation running inside interpreter thread loop.
#[inline]
pub fn build_runtime_bridge_context(
    safepoint: &SafepointCoordinator,
    task: &Arc<Task>,
    gc: &parking_lot::Mutex<GarbageCollector>,
    classes: &parking_lot::RwLock<ClassRegistry>,
    layouts: &parking_lot::RwLock<RuntimeLayoutRegistry>,
    mutex_registry: &MutexRegistry,
    semaphore_registry: &SemaphoreRegistry,
    globals_by_index: &parking_lot::RwLock<Vec<Value>>,
    builtin_global_slots: &parking_lot::RwLock<FxHashMap<String, usize>>,
    js_global_bindings: &parking_lot::RwLock<
        FxHashMap<String, crate::vm::interpreter::JsGlobalBindingRecord>,
    >,
    js_global_binding_slots: &parking_lot::RwLock<FxHashMap<usize, String>>,
    constant_string_cache: &parking_lot::RwLock<FxHashMap<(String, usize), Value>>,
    ephemeral_gc_roots: &parking_lot::RwLock<Vec<Value>>,
    pinned_handles: &parking_lot::RwLock<rustc_hash::FxHashSet<u64>>,
    tasks: &Arc<parking_lot::RwLock<FxHashMap<TaskId, Arc<Task>>>>,
    injector: &Arc<Injector<Arc<Task>>>,
    promise_microtasks: &parking_lot::Mutex<
        std::collections::VecDeque<crate::vm::interpreter::PromiseMicrotask>,
    >,
    test262_async_state: &std::sync::atomic::AtomicU8,
    test262_async_failure: &parking_lot::Mutex<Option<String>>,
    module_layouts: &parking_lot::RwLock<FxHashMap<[u8; 32], ModuleRuntimeLayout>>,
    module_registry: &parking_lot::RwLock<crate::vm::interpreter::ModuleRegistry>,
    metadata: &parking_lot::Mutex<crate::vm::reflect::MetadataStore>,
    class_metadata: &parking_lot::RwLock<ClassMetadataRegistry>,
    native_handler: &Arc<dyn NativeHandler>,
    resolved_natives: &parking_lot::RwLock<ResolvedNatives>,
    structural_shape_names: &parking_lot::RwLock<FxHashMap<u64, Vec<String>>>,
    structural_layout_shapes: &parking_lot::RwLock<
        FxHashMap<crate::vm::object::LayoutId, Vec<String>>,
    >,
    structural_shape_adapters: &parking_lot::RwLock<
        FxHashMap<StructuralAdapterKey, Arc<ShapeAdapter>>,
    >,
    aot_profile: &parking_lot::RwLock<crate::aot_profile::AotProfileCollector>,
    type_handles: &parking_lot::RwLock<crate::vm::interpreter::RuntimeTypeHandleRegistry>,
    class_value_slots: &parking_lot::RwLock<FxHashMap<usize, usize>>,
    prop_keys: &parking_lot::RwLock<crate::vm::interpreter::PropertyKeyRegistry>,
    stack_pool: &crate::vm::scheduler::StackPool,
    max_preemptions: u32,
    current_frame_depth: usize,
    io_submit_tx: Option<&crossbeam::channel::Sender<IoSubmission>>,
) -> JitRuntimeBridgeContext {
    JitRuntimeBridgeContext {
        safepoint: safepoint as *const SafepointCoordinator,
        task: task.as_ref() as *const Task,
        task_arc: task as *const Arc<Task>,
        gc: gc as *const _,
        classes: classes as *const _,
        layouts: layouts as *const _,
        mutex_registry: mutex_registry as *const _,
        semaphore_registry: semaphore_registry as *const _,
        globals_by_index: globals_by_index as *const _,
        builtin_global_slots: builtin_global_slots as *const _,
        js_global_bindings: js_global_bindings as *const _,
        js_global_binding_slots: js_global_binding_slots as *const _,
        constant_string_cache: constant_string_cache as *const _,
        ephemeral_gc_roots: ephemeral_gc_roots as *const _,
        pinned_handles: pinned_handles as *const _,
        tasks: tasks as *const _,
        injector: injector as *const _,
        promise_microtasks: promise_microtasks as *const _,
        test262_async_state: test262_async_state as *const _,
        test262_async_failure: test262_async_failure as *const _,
        module_layouts: module_layouts as *const _,
        module_registry: module_registry as *const _,
        metadata: metadata as *const _,
        class_metadata: class_metadata as *const _,
        native_handler: native_handler as *const _,
        resolved_natives: resolved_natives as *const _,
        structural_shape_names: structural_shape_names as *const _,
        structural_layout_shapes: structural_layout_shapes as *const _,
        structural_shape_adapters: structural_shape_adapters as *const _,
        aot_profile: aot_profile as *const _,
        type_handles: type_handles as *const _,
        class_value_slots: class_value_slots as *const _,
        prop_keys: prop_keys as *const _,
        stack_pool: stack_pool as *const _,
        io_submit_tx: io_submit_tx.map_or(std::ptr::null(), |tx| tx as *const _),
        max_preemptions,
        current_frame_depth,
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
        kernel_call_dispatch: helper_kernel_call_dispatch,
        interpreter_call: helper_interpreter_call,
        throw_exception: helper_throw_exception,
        reserved0: 0,
        string_concat: helper_string_concat,
        generic_equals: helper_generic_equals,
        object_get_field: helper_object_get_field,
        object_set_field: helper_object_set_field,
        object_implements_shape: helper_object_implements_shape,
        object_is_nominal: helper_object_is_nominal,
        object_get_shape_field: helper_object_get_shape_field,
        object_set_shape_field: helper_object_set_shape_field,
        string_len: helper_string_len,
    }
}

#[inline]
unsafe fn jit_object_ptr_checked(value: Value) -> Option<NonNull<Object>> {
    if !value.is_ptr() {
        return None;
    }
    let ptr = value.as_ptr::<u8>()?;
    let header = &*crate::vm::gc::header_ptr_from_value_ptr(ptr.as_ptr());
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
        object.dyn_props().and_then(|dp| {
            dp.contains_key(key)
                .then_some(StructuralSlotBinding::Dynamic(key))
        })
    };

    if let Some(nominal_type_id) = object.nominal_type_id_usize() {
        let class_meta = if bridge.class_metadata.is_null() {
            None
        } else {
            unsafe { &*bridge.class_metadata }
                .read()
                .get(nominal_type_id)
                .cloned()
        };
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
    let current_epoch = if bridge.layouts.is_null() {
        0
    } else {
        unsafe { &*bridge.layouts }
            .read()
            .layout_epoch(object.layout_id())
            .unwrap_or(0)
    };
    if let Some(adapter) = JIT_SHAPE_ADAPTER_LAST.with(|cache| {
        let borrowed = cache.borrow();
        let Some((cached_key, cached_epoch, adapter)) = borrowed.as_ref() else {
            return None;
        };
        if *cached_key == adapter_key && *cached_epoch == current_epoch {
            Some(adapter.clone())
        } else {
            None
        }
    }) {
        return Some(adapter);
    }
    if let Some(adapter) = JIT_SHAPE_ADAPTER_PIC.with(|cache| {
        let borrowed = cache.borrow();
        borrowed
            .iter()
            .find(|(cached_key, cached_epoch, _)| {
                *cached_key == adapter_key && *cached_epoch == current_epoch
            })
            .map(|(_, _, adapter)| adapter.clone())
    }) {
        JIT_SHAPE_ADAPTER_LAST.with(|cache| {
            *cache.borrow_mut() = Some((adapter_key, current_epoch, adapter.clone()));
        });
        return Some(adapter);
    }
    if let Some(adapter) = unsafe { &*bridge.structural_shape_adapters }
        .read()
        .get(&adapter_key)
        .cloned()
    {
        if adapter.epoch == current_epoch {
            JIT_SHAPE_ADAPTER_LAST.with(|cache| {
                *cache.borrow_mut() = Some((adapter_key, current_epoch, adapter.clone()));
            });
            JIT_SHAPE_ADAPTER_PIC.with(|cache| {
                let mut cache = cache.borrow_mut();
                if let Some(pos) = cache
                    .iter()
                    .position(|(cached_key, _, _)| *cached_key == adapter_key)
                {
                    cache.remove(pos);
                }
                cache.insert(0, (adapter_key, current_epoch, adapter.clone()));
                cache.truncate(JIT_SHAPE_ADAPTER_PIC_CAPACITY);
            });
            return Some(adapter);
        }
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
        current_epoch,
    ));
    let mut adapters = unsafe { &*bridge.structural_shape_adapters }.write();
    let adapter = adapters
        .entry(adapter_key)
        .or_insert_with(|| adapter.clone())
        .clone();
    JIT_SHAPE_ADAPTER_LAST.with(|cache| {
        *cache.borrow_mut() = Some((adapter_key, current_epoch, adapter.clone()));
    });
    JIT_SHAPE_ADAPTER_PIC.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(pos) = cache
            .iter()
            .position(|(cached_key, _, _)| *cached_key == adapter_key)
        {
            cache.remove(pos);
        }
        cache.insert(0, (adapter_key, current_epoch, adapter.clone()));
        cache.truncate(JIT_SHAPE_ADAPTER_PIC_CAPACITY);
    });
    Some(adapter)
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

fn jit_build_interpreter<'a>(bridge: &'a JitRuntimeBridgeContext) -> Option<Interpreter<'a>> {
    if bridge.gc.is_null()
        || bridge.classes.is_null()
        || bridge.layouts.is_null()
        || bridge.mutex_registry.is_null()
        || bridge.semaphore_registry.is_null()
        || bridge.safepoint.is_null()
        || bridge.globals_by_index.is_null()
        || bridge.builtin_global_slots.is_null()
        || bridge.js_global_bindings.is_null()
        || bridge.js_global_binding_slots.is_null()
        || bridge.constant_string_cache.is_null()
        || bridge.ephemeral_gc_roots.is_null()
        || bridge.pinned_handles.is_null()
        || bridge.tasks.is_null()
        || bridge.injector.is_null()
        || bridge.promise_microtasks.is_null()
        || bridge.test262_async_state.is_null()
        || bridge.test262_async_failure.is_null()
        || bridge.metadata.is_null()
        || bridge.class_metadata.is_null()
        || bridge.native_handler.is_null()
        || bridge.module_layouts.is_null()
        || bridge.module_registry.is_null()
        || bridge.structural_shape_adapters.is_null()
        || bridge.structural_shape_names.is_null()
        || bridge.structural_layout_shapes.is_null()
        || bridge.type_handles.is_null()
        || bridge.class_value_slots.is_null()
        || bridge.prop_keys.is_null()
        || bridge.aot_profile.is_null()
        || bridge.stack_pool.is_null()
    {
        return None;
    }

    Some(Interpreter::new(
        unsafe { &*bridge.gc },
        unsafe { &*bridge.classes },
        unsafe { &*bridge.layouts },
        unsafe { &*bridge.mutex_registry },
        unsafe { &*bridge.semaphore_registry },
        unsafe { &*bridge.safepoint },
        unsafe { &*bridge.globals_by_index },
        unsafe { &*bridge.builtin_global_slots },
        unsafe { &*bridge.js_global_bindings },
        unsafe { &*bridge.js_global_binding_slots },
        unsafe { &*bridge.constant_string_cache },
        unsafe { &*bridge.ephemeral_gc_roots },
        unsafe { &*bridge.pinned_handles },
        unsafe { &*bridge.tasks },
        unsafe { &*bridge.injector },
        unsafe { &*bridge.promise_microtasks },
        unsafe { &*bridge.test262_async_state },
        unsafe { &*bridge.test262_async_failure },
        unsafe { &*bridge.metadata },
        unsafe { &*bridge.class_metadata },
        unsafe { &*bridge.native_handler },
        unsafe { &*bridge.module_layouts },
        unsafe { &*bridge.module_registry },
        unsafe { &*bridge.structural_shape_adapters },
        unsafe { &*bridge.structural_shape_names },
        unsafe { &*bridge.structural_layout_shapes },
        unsafe { &*bridge.type_handles },
        unsafe { &*bridge.class_value_slots },
        unsafe { &*bridge.prop_keys },
        unsafe { &*bridge.aot_profile },
        if bridge.io_submit_tx.is_null() {
            None
        } else {
            Some(unsafe { &*bridge.io_submit_tx })
        },
        bridge.max_preemptions,
        unsafe { &*bridge.stack_pool },
    ))
}

fn jit_raise_vm_error(bridge: &JitRuntimeBridgeContext, error: VmError) {
    if bridge.task.is_null() || bridge.gc.is_null() {
        return;
    }
    let task = unsafe { &*bridge.task };
    if task.has_exception() {
        return;
    }
    let raya_string = crate::vm::object::RayaString::new(error.to_string());
    let gc_ptr = unsafe { &*bridge.gc }.lock().allocate(raya_string);
    let exc_val = unsafe { Value::from_ptr(NonNull::new(gc_ptr.as_ptr()).unwrap()) };
    task.set_exception(exc_val);
}

fn jit_suspend_task(bridge: &JitRuntimeBridgeContext, reason: SuspendReason) -> BackendCallResult {
    if !bridge.task.is_null() {
        let task = unsafe { &*bridge.task };
        task.suspend(reason.clone());
    }
    BackendCallResult::suspended_with_tag(SuspendTag::from_reason(&reason))
}

#[derive(Clone, Copy)]
struct JitTaskStateSnapshot {
    exception_handler_count: usize,
    call_frame_count: usize,
    closure_count: usize,
    held_mutex_count: usize,
    current_func_id: usize,
    current_locals_base: usize,
    current_arg_count: usize,
    current_exception: Option<Value>,
    caught_exception: Option<Value>,
}

fn jit_snapshot_task_state(task: &Task) -> JitTaskStateSnapshot {
    JitTaskStateSnapshot {
        exception_handler_count: task.exception_handler_count(),
        call_frame_count: task.call_frame_count(),
        closure_count: task.closure_count(),
        held_mutex_count: task.held_mutex_count(),
        current_func_id: task.current_func_id(),
        current_locals_base: task.current_locals_base(),
        current_arg_count: task.current_arg_count(),
        current_exception: task.current_exception(),
        caught_exception: task.caught_exception(),
    }
}

fn jit_restore_task_exceptions(
    task: &Task,
    current_exception: Option<Value>,
    caught_exception: Option<Value>,
) {
    if let Some(exception) = current_exception {
        task.set_exception(exception);
    } else {
        task.clear_exception();
    }
    if let Some(exception) = caught_exception {
        task.set_caught_exception(exception);
    } else {
        task.clear_caught_exception();
    }
}

fn jit_rollback_mutexes(
    bridge: &JitRuntimeBridgeContext,
    task: &Task,
    snapshot: &JitTaskStateSnapshot,
) {
    let released = task.take_mutexes_since(snapshot.held_mutex_count);
    if released.is_empty() || bridge.mutex_registry.is_null() {
        return;
    }

    let registry = unsafe { &*bridge.mutex_registry };
    for mutex_id in released.into_iter().rev() {
        let Some(mutex) = registry.get(mutex_id) else {
            continue;
        };
        let Ok(next_waiter) = mutex.unlock(task.id()) else {
            continue;
        };
        if let Some(waiter_id) = next_waiter {
            if bridge.tasks.is_null() || bridge.injector.is_null() {
                continue;
            }
            let tasks = unsafe { &*bridge.tasks }.read();
            if let Some(waiter_task) = tasks.get(&waiter_id) {
                waiter_task.add_held_mutex(mutex_id);
                waiter_task.set_state(crate::vm::scheduler::TaskState::Resumed);
                waiter_task.clear_suspend_reason();
                unsafe { &*bridge.injector }.push(waiter_task.clone());
            }
        }
    }
}

fn jit_restore_task_state(
    bridge: &JitRuntimeBridgeContext,
    task: &Task,
    snapshot: &JitTaskStateSnapshot,
    preserve_current_exception: bool,
) {
    while task.exception_handler_count() > snapshot.exception_handler_count {
        let _ = task.pop_exception_handler();
    }
    while task.call_frame_count() > snapshot.call_frame_count {
        let _ = task.pop_call_frame();
    }
    while task.closure_count() > snapshot.closure_count {
        let _ = task.pop_closure();
    }
    jit_rollback_mutexes(bridge, task, snapshot);
    task.set_current_func_id(snapshot.current_func_id);
    task.set_current_locals_base(snapshot.current_locals_base);
    task.set_current_arg_count(snapshot.current_arg_count);

    let current_exception = if preserve_current_exception {
        task.current_exception().or(snapshot.current_exception)
    } else {
        snapshot.current_exception
    };
    jit_restore_task_exceptions(task, current_exception, snapshot.caught_exception);
}

fn jit_function_is_sync_safe(
    module: &Module,
    func_id: usize,
    visiting: &mut FxHashSet<([u8; 32], usize)>,
) -> bool {
    let key = (module.checksum, func_id);
    if !visiting.insert(key) {
        return true;
    }
    let Some(func) = module.functions.get(func_id) else {
        return false;
    };
    let Ok(instrs) = crate::jit::analysis::decoder::decode_function(&func.code) else {
        return false;
    };
    for instr in instrs {
        use crate::jit::analysis::decoder::Operands;
        match instr.opcode {
            Opcode::Await
            | Opcode::WaitAll
            | Opcode::Sleep
            | Opcode::Yield
            | Opcode::KernelCall
            | Opcode::Spawn
            | Opcode::SpawnClosure => return false,
            Opcode::Call => match instr.operands {
                Operands::Call {
                    func_index: 0xFFFF_FFFF,
                    ..
                } => return false,
                Operands::Call { func_index, .. } => {
                    if !jit_function_is_sync_safe(module, func_index as usize, visiting) {
                        return false;
                    }
                }
                _ => return false,
            },
            Opcode::CallStatic => match instr.operands {
                Operands::Call { func_index, .. } => {
                    if !jit_function_is_sync_safe(module, func_index as usize, visiting) {
                        return false;
                    }
                }
                _ => return false,
            },
            Opcode::CallMethodExact
            | Opcode::OptionalCallMethodExact
            | Opcode::CallMethodShape
            | Opcode::OptionalCallMethodShape
            | Opcode::CallConstructor
            | Opcode::ConstructType
            | Opcode::CallSuper => return false,
            _ => {}
        }
    }
    true
}

enum JitNestedCallResult {
    Value(Value),
    Fallback,
    Exception,
}

fn jit_apply_return_action(
    interpreter: &Interpreter<'_>,
    stack: &mut Stack,
    return_value: Value,
    return_action: ReturnAction,
) -> Result<Option<Value>, VmError> {
    match return_action {
        ReturnAction::PushReturnValue => {
            stack.push(return_value)?;
            Ok(None)
        }
        ReturnAction::PushConstructResult(receiver) => {
            stack.push(interpreter.constructor_result_or_receiver(return_value, receiver))?;
            Ok(None)
        }
        ReturnAction::Discard => Ok(None),
    }
}

fn jit_execute_sync_frame(
    interpreter: &mut Interpreter<'_>,
    bridge: &JitRuntimeBridgeContext,
    stack: &mut Stack,
    initial_module: Arc<Module>,
    initial_func_id: usize,
    initial_arg_count: usize,
    initial_is_closure: bool,
    initial_closure_val: Option<Value>,
    initial_return_action: ReturnAction,
) -> JitNestedCallResult {
    let Some(task) = (!bridge.task_arc.is_null()).then(|| unsafe { &*bridge.task_arc }) else {
        return JitNestedCallResult::Fallback;
    };
    let task_snapshot = jit_snapshot_task_state(task.as_ref());

    let mut frames: Vec<ExecutionFrame> = Vec::new();
    let mut module = initial_module;
    let mut current_func_id = initial_func_id;
    let mut ip = 0usize;
    let mut current_arg_count = initial_arg_count;
    let mut current_arg_values: Vec<Value> = (0..initial_arg_count)
        .map(|i| stack.peek_at(stack.depth().saturating_sub(initial_arg_count) + i).unwrap_or(Value::undefined()))
        .collect();
    let mut current_is_closure = initial_is_closure;
    let mut current_return_action = initial_return_action;

    macro_rules! finish_nested_call {
        ($result:expr, $preserve_exception:expr) => {{
            jit_restore_task_state(bridge, task.as_ref(), &task_snapshot, $preserve_exception);
            return $result;
        }};
    }

    task.push_call_frame(current_func_id);
    if let Some(closure_val) = initial_closure_val {
        task.push_closure(closure_val);
    }

    let mut locals_base = stack.depth().saturating_sub(initial_arg_count);
    let local_count = module
        .functions
        .get(current_func_id)
        .map(|f| f.local_count)
        .unwrap_or(initial_arg_count);
    for _ in 0..local_count.saturating_sub(initial_arg_count) {
        if let Err(error) = stack.push(Value::null()) {
            jit_raise_vm_error(bridge, error);
            finish_nested_call!(JitNestedCallResult::Exception, true);
        }
    }
    task.set_current_func_id(current_func_id);
    task.set_current_locals_base(locals_base);
    task.set_current_arg_count(current_arg_count);

    loop {
        let code = &module.functions[current_func_id].code;
        if ip >= code.len() {
            let return_value =
                if stack.depth() > locals_base + module.functions[current_func_id].local_count {
                    stack.pop().unwrap_or_else(|_| Value::null())
                } else {
                    Value::null()
                };
            while stack.depth() > locals_base {
                let _ = stack.pop();
            }
            task.pop_call_frame();
            if current_is_closure {
                task.pop_closure();
            }
            if let Some(frame) = frames.pop() {
                if let Err(error) =
                    jit_apply_return_action(interpreter, stack, return_value, current_return_action)
                {
                    jit_raise_vm_error(bridge, error);
                    finish_nested_call!(JitNestedCallResult::Exception, true);
                }
                module = frame.module;
                current_func_id = frame.func_id;
                ip = frame.ip;
                locals_base = frame.locals_base;
                current_is_closure = frame.is_closure;
                current_return_action = frame.return_action;
                current_arg_count = frame.arg_count;
                current_arg_values = frame.arg_values;
                task.set_current_func_id(current_func_id);
                task.set_current_locals_base(locals_base);
                task.set_current_arg_count(current_arg_count);
                continue;
            }
            finish_nested_call!(
                match current_return_action {
                    ReturnAction::PushReturnValue => JitNestedCallResult::Value(return_value),
                    ReturnAction::PushConstructResult(receiver) => JitNestedCallResult::Value(
                        interpreter.constructor_result_or_receiver(return_value, receiver),
                    ),
                    ReturnAction::Discard => JitNestedCallResult::Value(Value::null()),
                },
                false
            );
        }

        let opcode = match Opcode::from_u8(code[ip]) {
            Some(op) => op,
            None => {
                jit_raise_vm_error(bridge, VmError::InvalidOpcode(code[ip]));
                finish_nested_call!(JitNestedCallResult::Exception, true);
            }
        };
        ip += 1;

        let frame_depth = bridge.current_frame_depth + 1 + frames.len();
        match interpreter.execute_opcode(
            task,
            stack,
            &mut ip,
            code,
            module.as_ref(),
            opcode,
            locals_base,
            frame_depth,
            current_arg_count,
            &current_arg_values,
        ) {
            crate::vm::interpreter::OpcodeResult::Continue => {}
            crate::vm::interpreter::OpcodeResult::Return(return_value) => {
                while stack.depth() > locals_base {
                    let _ = stack.pop();
                }
                task.pop_call_frame();
                if current_is_closure {
                    task.pop_closure();
                }
                if let Some(frame) = frames.pop() {
                    if let Err(error) = jit_apply_return_action(
                        interpreter,
                        stack,
                        return_value,
                        current_return_action,
                    ) {
                        jit_raise_vm_error(bridge, error);
                        finish_nested_call!(JitNestedCallResult::Exception, true);
                    }
                    module = frame.module;
                    current_func_id = frame.func_id;
                    ip = frame.ip;
                    locals_base = frame.locals_base;
                    current_is_closure = frame.is_closure;
                    current_return_action = frame.return_action;
                    current_arg_count = frame.arg_count;
                    current_arg_values = frame.arg_values;
                    task.set_current_func_id(current_func_id);
                    task.set_current_locals_base(locals_base);
                    task.set_current_arg_count(current_arg_count);
                } else {
                    finish_nested_call!(
                        match current_return_action {
                            ReturnAction::PushReturnValue =>
                                JitNestedCallResult::Value(return_value),
                            ReturnAction::PushConstructResult(receiver) => {
                                JitNestedCallResult::Value(
                                    interpreter
                                        .constructor_result_or_receiver(return_value, receiver),
                                )
                            }
                            ReturnAction::Discard => JitNestedCallResult::Value(Value::null()),
                        },
                        false
                    );
                }
            }
            crate::vm::interpreter::OpcodeResult::Suspend(_) => {
                finish_nested_call!(JitNestedCallResult::Fallback, false);
            }
            crate::vm::interpreter::OpcodeResult::PushFrame {
                func_id,
                arg_count,
                is_closure,
                closure_val,
                module: callee_module,
                return_action,
            } => {
                let callee_module = callee_module.unwrap_or_else(|| module.clone());
                if !jit_function_is_sync_safe(
                    callee_module.as_ref(),
                    func_id,
                    &mut FxHashSet::default(),
                ) {
                    finish_nested_call!(JitNestedCallResult::Fallback, false);
                }

                frames.push(ExecutionFrame {
                    module: module.clone(),
                    func_id: current_func_id,
                    ip,
                    locals_base,
                    is_closure: current_is_closure,
                    return_action: current_return_action,
                    arg_count: current_arg_count,
                    arg_values: current_arg_values.clone(),
                });
                task.push_call_frame(func_id);
                if let Some(cv) = closure_val {
                    task.push_closure(cv);
                }
                current_arg_values = (0..arg_count)
                    .map(|i| stack.peek_at(stack.depth().saturating_sub(arg_count) + i).unwrap_or(Value::undefined()))
                    .collect();

                locals_base = stack.depth().saturating_sub(arg_count);
                let local_count = callee_module
                    .functions
                    .get(func_id)
                    .map(|f| f.local_count)
                    .unwrap_or(arg_count);
                for _ in 0..local_count.saturating_sub(arg_count) {
                    if let Err(error) = stack.push(Value::null()) {
                        jit_raise_vm_error(bridge, error);
                        finish_nested_call!(JitNestedCallResult::Exception, true);
                    }
                }

                module = callee_module;
                current_func_id = func_id;
                ip = 0;
                current_arg_count = arg_count;
                current_is_closure = is_closure;
                current_return_action = return_action;
                task.set_current_func_id(current_func_id);
                task.set_current_locals_base(locals_base);
                task.set_current_arg_count(current_arg_count);
            }
            crate::vm::interpreter::OpcodeResult::Error(error) => {
                if !task.has_exception() {
                    jit_raise_vm_error(bridge, error);
                }

                let exception = task.current_exception().unwrap_or_else(Value::null);
                let mut handled = false;
                'exception_search: loop {
                    let current_frame_depth = bridge.current_frame_depth + 1 + frames.len();
                    while let Some(handler) = task.peek_exception_handler() {
                        if handler.frame_count != current_frame_depth {
                            break;
                        }

                        while stack.depth() > handler.stack_size {
                            let _ = stack.pop();
                        }

                        if handler.catch_offset != -1 {
                            task.pop_exception_handler();
                            task.set_caught_exception(exception);
                            task.clear_exception();
                            if let Err(push_error) = stack.push(exception) {
                                jit_raise_vm_error(bridge, push_error);
                                finish_nested_call!(JitNestedCallResult::Exception, true);
                            }
                            ip = handler.catch_offset as usize;
                            handled = true;
                            break 'exception_search;
                        }

                        if handler.finally_offset != -1 {
                            task.pop_exception_handler();
                            ip = handler.finally_offset as usize;
                            handled = true;
                            break 'exception_search;
                        }

                        task.pop_exception_handler();
                    }

                    if let Some(frame) = frames.pop() {
                        task.pop_call_frame();
                        if current_is_closure {
                            task.pop_closure();
                        }
                        module = frame.module;
                        current_func_id = frame.func_id;
                        ip = frame.ip;
                        locals_base = frame.locals_base;
                        current_is_closure = frame.is_closure;
                        current_return_action = frame.return_action;
                        current_arg_count = frame.arg_count;
                        task.set_current_func_id(current_func_id);
                        task.set_current_locals_base(locals_base);
                        task.set_current_arg_count(current_arg_count);
                    } else {
                        break;
                    }
                }

                if !handled {
                    finish_nested_call!(JitNestedCallResult::Exception, true);
                }
            }
        }
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
    let Some(nominal_type_id) =
        jit_resolve_nominal_type_id(bridge, module, local_nominal_type_index)
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
    data_ptr: *const u8,
    len: usize,
    shared_state: *mut (),
) -> *mut () {
    if shared_state.is_null() || (len != 0 && data_ptr.is_null()) {
        return std::ptr::null_mut();
    }
    let bridge = &*(shared_state.cast::<JitRuntimeBridgeContext>());
    if bridge.gc.is_null() {
        return std::ptr::null_mut();
    }

    let bytes = if len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(data_ptr, len)
    };
    let text = String::from_utf8_lossy(bytes).into_owned();
    let mut gc = (&*bridge.gc).lock();
    let string_ptr = gc.allocate(RayaString::new(text));
    string_ptr.as_ptr().cast::<()>()
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

unsafe extern "C" fn helper_kernel_call_dispatch(
    kernel_op_id: u16,
    args_ptr: *const u64,
    arg_count: u8,
    module_ptr: *const (),
    shared_state: *mut (),
) -> BackendCallResult {
    let Some(kernel_op) = decode_kernel_op_id(kernel_op_id) else {
        return BackendCallResult::completed(Value::null());
    };

    let value_args: Vec<Value> = if arg_count == 0 || args_ptr.is_null() {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, arg_count as usize)
            .iter()
            .copied()
            .map(|raw| Value::from_raw(raw))
            .collect()
    };

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

            let mut ctx = EngineContext::new(
                &*bridge.gc,
                &*bridge.classes,
                &*bridge.layouts,
                task_id,
                &*bridge.class_metadata,
            );
            if !bridge.tasks.is_null() && !bridge.injector.is_null() {
                ctx = ctx.with_scheduler(&*bridge.tasks, &*bridge.injector);
            }

            let native_args: Vec<raya_sdk::NativeValue> =
                value_args.iter().map(|v| value_to_native(*v)).collect();

            match kernel_op {
                KernelOp::VmNative(native_id) => {
                    let resolved = (&*bridge.resolved_natives).read();
                    match resolved.call(native_id, &ctx, &native_args) {
                        NativeCallResult::Value(v) => {
                            return BackendCallResult::completed_raw(native_to_value(v).raw())
                        }
                        NativeCallResult::Suspend(io_request) => {
                            if !bridge.io_submit_tx.is_null() {
                                let tx = &*bridge.io_submit_tx;
                                let _ = tx.send(IoSubmission {
                                    task_id,
                                    request: io_request,
                                });
                            }
                            return jit_suspend_task(bridge, SuspendReason::IoWait);
                        }
                        NativeCallResult::Unhandled | NativeCallResult::Error(_) => {}
                    }
                }
                KernelOp::RegisteredNative(local_idx) => {
                    let resolved = if !module_ptr.is_null() && !bridge.module_layouts.is_null() {
                        let module = &*(module_ptr.cast::<Module>());
                        (&*bridge.module_layouts)
                            .read()
                            .get(&module.checksum)
                            .map(|layout| layout.resolved_natives.clone())
                            .unwrap_or_else(ResolvedNatives::empty)
                    } else {
                        (&*bridge.resolved_natives).read().clone()
                    };
                    match resolved.call(local_idx, &ctx, &native_args) {
                        NativeCallResult::Value(v) => {
                            return BackendCallResult::completed_raw(native_to_value(v).raw())
                        }
                        NativeCallResult::Suspend(io_request) => {
                            if !bridge.io_submit_tx.is_null() {
                                let tx = &*bridge.io_submit_tx;
                                let _ = tx.send(IoSubmission {
                                    task_id,
                                    request: io_request,
                                });
                            }
                            return jit_suspend_task(bridge, SuspendReason::IoWait);
                        }
                        NativeCallResult::Unhandled | NativeCallResult::Error(_) => {}
                    }
                }
                _ => {}
            }
        }

        let Some(module) = (!module_ptr.is_null()).then(|| &*(module_ptr.cast::<Module>())) else {
            jit_raise_vm_error(
                bridge,
                VmError::RuntimeError("JIT kernel dispatch missing module context".to_string()),
            );
            return BackendCallResult::threw();
        };
        let Some(task) = (!bridge.task_arc.is_null()).then(|| &*bridge.task_arc) else {
            jit_raise_vm_error(
                bridge,
                VmError::RuntimeError("JIT kernel dispatch missing task context".to_string()),
            );
            return BackendCallResult::threw();
        };
        let Some(mut interpreter) = jit_build_interpreter(bridge) else {
            jit_raise_vm_error(
                bridge,
                VmError::RuntimeError("JIT kernel dispatch could not build interpreter".to_string()),
            );
            return BackendCallResult::threw();
        };

        let mut stack = Stack::new();
        for arg in &value_args {
            if stack.push(*arg).is_err() {
                return BackendCallResult::threw();
            }
        }
        let code = [
            (kernel_op_id & 0x00FF) as u8,
            ((kernel_op_id >> 8) & 0x00FF) as u8,
            arg_count,
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
            crate::vm::interpreter::OpcodeResult::Return(value) => BackendCallResult::completed(value),
            crate::vm::interpreter::OpcodeResult::Suspend(reason) => {
                jit_suspend_task(bridge, reason)
            }
            crate::vm::interpreter::OpcodeResult::Error(error) => {
                jit_raise_vm_error(bridge, error);
                BackendCallResult::threw()
            }
            crate::vm::interpreter::OpcodeResult::PushFrame { .. } => {
                jit_raise_vm_error(
                    bridge,
                    VmError::RuntimeError(
                        "JIT kernel dispatch encountered nested frame and cannot bounce to the interpreter"
                            .to_string(),
                    ),
                );
                BackendCallResult::threw()
            }
        };
    }
    BackendCallResult::completed(Value::null())
}

unsafe extern "C" fn helper_interpreter_call(
    opcode_raw: u8,
    operand_u64: u64,
    operand_u32: u32,
    receiver_raw: u64,
    args_ptr: *const u64,
    arg_count: u16,
    module_ptr: *const (),
    shared_state: *mut (),
) -> BackendCallResult {
    if shared_state.is_null() || module_ptr.is_null() {
        return BackendCallResult::threw();
    }
    let bridge = &*(shared_state.cast::<JitRuntimeBridgeContext>());
    let Some(opcode) = Opcode::from_u8(opcode_raw) else {
        jit_raise_vm_error(
            bridge,
            VmError::RuntimeError(format!("JIT helper received unknown call opcode {}", opcode_raw)),
        );
        return BackendCallResult::threw();
    };
    let module = &*(module_ptr.cast::<Module>());
    let Some(task) = (!bridge.task_arc.is_null()).then(|| unsafe { &*bridge.task_arc }) else {
        jit_raise_vm_error(
            bridge,
            VmError::RuntimeError("JIT call helper missing task context".to_string()),
        );
        return BackendCallResult::threw();
    };
    let Some(mut interpreter) = jit_build_interpreter(bridge) else {
        jit_raise_vm_error(
            bridge,
            VmError::RuntimeError("JIT call helper could not build interpreter".to_string()),
        );
        return BackendCallResult::threw();
    };

    let args: Vec<Value> = if arg_count == 0 || args_ptr.is_null() {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, arg_count as usize)
            .iter()
            .copied()
            .map(|raw| unsafe { Value::from_raw(raw) })
            .collect()
    };

    let mut stack = Stack::new();
    if std::env::var("RAYA_JIT_DEBUG_CALLS").is_ok() {
        eprintln!(
            "jit interpreter_call: opcode={opcode:?} operand_u32={operand_u32} operand_u64={operand_u64:#x} receiver=0x{receiver_raw:016x} argc={arg_count}"
        );
    }
    match opcode {
        Opcode::Call => {
            if operand_u32 == 0xFFFF_FFFF {
                if stack.push(Value::from_raw(receiver_raw)).is_err() {
                    return BackendCallResult::threw();
                }
            }
            for arg in &args {
                if stack.push(*arg).is_err() {
                    return BackendCallResult::threw();
                }
            }
        }
        Opcode::CallMethodExact
        | Opcode::OptionalCallMethodExact
        | Opcode::CallMethodShape
        | Opcode::OptionalCallMethodShape
        | Opcode::ConstructType
        | Opcode::CallSuper => {
            if stack.push(Value::from_raw(receiver_raw)).is_err() {
                return BackendCallResult::threw();
            }
            for arg in &args {
                if stack.push(*arg).is_err() {
                    return BackendCallResult::threw();
                }
            }
        }
        Opcode::CallConstructor | Opcode::CallStatic => {
            for arg in &args {
                if stack.push(*arg).is_err() {
                    return BackendCallResult::threw();
                }
            }
        }
        _ => {
            jit_raise_vm_error(
                bridge,
                VmError::RuntimeError(format!(
                    "JIT call helper does not support opcode {:?} without an exact compiled ABI path",
                    opcode
                )),
            );
            return BackendCallResult::threw();
        }
    }

    let mut code = vec![opcode_raw];
    match opcode {
        Opcode::Call
        | Opcode::CallMethodExact
        | Opcode::OptionalCallMethodExact
        | Opcode::CallStatic => {
            code.extend_from_slice(&operand_u32.to_le_bytes());
            code.extend_from_slice(&arg_count.to_le_bytes());
        }
        Opcode::CallConstructor | Opcode::CallSuper => {
            code.extend_from_slice(&operand_u32.to_le_bytes());
            code.extend_from_slice(&arg_count.to_le_bytes());
        }
        Opcode::ConstructType => {
            code.extend_from_slice(&(operand_u32 as u16).to_le_bytes());
            code.push(arg_count as u8);
        }
        Opcode::CallMethodShape | Opcode::OptionalCallMethodShape => {
            code.extend_from_slice(&operand_u64.to_le_bytes());
            code.extend_from_slice(&(operand_u32 as u16).to_le_bytes());
            code.extend_from_slice(&arg_count.to_le_bytes());
        }
        _ => {}
    }

    let mut ip = 1usize;
    match interpreter.exec_call_ops(&mut stack, &mut ip, &code, module, task, opcode) {
        crate::vm::interpreter::OpcodeResult::Continue => {
            let value = stack.pop().unwrap_or_else(|_| Value::null());
            if std::env::var("RAYA_JIT_DEBUG_CALLS").is_ok() {
                eprintln!("jit interpreter_call continue: opcode={opcode:?} result={value:?}");
            }
            BackendCallResult::completed(value)
        }
        crate::vm::interpreter::OpcodeResult::Return(value) => {
            if std::env::var("RAYA_JIT_DEBUG_CALLS").is_ok() {
                eprintln!("jit interpreter_call return: opcode={opcode:?} result={value:?}");
            }
            BackendCallResult::completed(value)
        }
        crate::vm::interpreter::OpcodeResult::Suspend(reason) => {
            jit_raise_vm_error(
                bridge,
                VmError::RuntimeError(format!(
                    "JIT call helper encountered unsupported suspension {:?} without a resumable compiled frame",
                    reason
                )),
            );
            BackendCallResult::threw()
        }
        crate::vm::interpreter::OpcodeResult::Error(error) => {
            if std::env::var("RAYA_JIT_DEBUG_CALLS").is_ok() {
                eprintln!("jit interpreter_call error: opcode={opcode:?} error={error}");
            }
            jit_raise_vm_error(bridge, error);
            BackendCallResult::threw()
        }
        crate::vm::interpreter::OpcodeResult::PushFrame {
            func_id,
            arg_count,
            is_closure,
            closure_val,
            module: callee_module,
            return_action,
        } => {
            let callee_module = callee_module.unwrap_or_else(|| Arc::new(module.clone()));
            if !jit_function_is_sync_safe(
                callee_module.as_ref(),
                func_id,
                &mut FxHashSet::default(),
            ) {
                jit_raise_vm_error(
                    bridge,
                    VmError::RuntimeError(format!(
                        "JIT call helper cannot execute non-sync-safe nested call to function {} without an interpreter boundary",
                        func_id
                    )),
                );
                return BackendCallResult::threw();
            }
            match jit_execute_sync_frame(
                &mut interpreter,
                bridge,
                &mut stack,
                callee_module,
                func_id,
                arg_count,
                is_closure,
                closure_val,
                return_action,
            ) {
                JitNestedCallResult::Value(value) => {
                    if std::env::var("RAYA_JIT_DEBUG_CALLS").is_ok() {
                        eprintln!(
                            "jit interpreter_call nested: opcode={opcode:?} result={value:?}"
                        );
                    }
                    BackendCallResult::completed(value)
                }
                JitNestedCallResult::Fallback => {
                    if std::env::var("RAYA_JIT_DEBUG_CALLS").is_ok() {
                        eprintln!(
                            "jit interpreter_call nested fallback became hard failure: opcode={opcode:?}"
                        );
                    }
                    jit_raise_vm_error(
                        bridge,
                        VmError::RuntimeError(format!(
                            "JIT nested call {:?} required removed interpreter fallback",
                            opcode
                        )),
                    );
                    BackendCallResult::threw()
                }
                JitNestedCallResult::Exception => {
                    if std::env::var("RAYA_JIT_DEBUG_CALLS").is_ok() {
                        eprintln!("jit interpreter_call nested exception: opcode={opcode:?}");
                    }
                    BackendCallResult::threw()
                }
            }
        }
    }
}

unsafe extern "C" fn helper_throw_exception(exception_value: u64, shared_state: *mut ()) {
    if shared_state.is_null() {
        return;
    }
    let bridge = &*(shared_state.cast::<JitRuntimeBridgeContext>());
    if bridge.task.is_null() {
        return;
    }
    let task = &*bridge.task;
    task.set_exception(Value::from_raw(exception_value));
}

unsafe extern "C" fn helper_string_concat(_left: u64, _right: u64, _shared_state: *mut ()) -> u64 {
    Value::null().raw()
}

unsafe extern "C" fn helper_string_len(string_raw: u64, _shared_state: *mut ()) -> i32 {
    let value = Value::from_raw(string_raw);
    let Some(string_ptr) = (unsafe { value.as_ptr::<RayaString>() }) else {
        if std::env::var("RAYA_JIT_DEBUG_CALLS").is_ok() {
            eprintln!("jit string_len fallback: raw=0x{string_raw:016x} value={value:?}");
        }
        return JIT_STRING_LEN_FALLBACK_SENTINEL;
    };
    let string = unsafe { &*string_ptr.as_ptr() };
    if std::env::var("RAYA_JIT_DEBUG_CALLS").is_ok() {
        eprintln!("jit string_len: len={} value={value:?}", string.len());
    }
    i32::try_from(string.len()).unwrap_or(JIT_STRING_LEN_FALLBACK_SENTINEL)
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

            let bound = Object::new_bound_method(object_val, func_id, method_module);
            let mut gc = (&*bridge.gc).lock();
            let bm_ptr = gc.allocate(bound);
            Value::from_ptr(NonNull::new(bm_ptr.as_ptr()).unwrap()).raw()
        }
        StructuralSlotBinding::Dynamic(key) => object
            .dyn_props()
            .and_then(|dp| dp.get(key).map(|p| p.value))
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
            object
                .ensure_dyn_props()
                .insert(key, DynProp::data(Value::from_raw(value_raw)));
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
        !matches!(
            adapter.binding_for_slot(slot),
            StructuralSlotBinding::Missing
        )
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

unsafe extern "C" fn helper_object_get_shape_field(
    object_raw: u64,
    required_shape: u64,
    expected_slot: u32,
    optional: u8,
    _func_id: u32,
    _module_ptr: *const (),
    shared_state: *mut (),
) -> u64 {
    if shared_state.is_null() {
        return JIT_SHAPE_FIELD_FALLBACK_SENTINEL;
    }
    let bridge = &*(shared_state.cast::<JitRuntimeBridgeContext>());
    let object_val = Value::from_raw(object_raw);
    if optional != 0 && object_val.is_null() {
        return Value::null().raw();
    }
    let Some(object_ptr) = jit_object_ptr_checked(object_val) else {
        if std::env::var("RAYA_JIT_DEBUG_SHAPES").is_ok() {
            eprintln!(
                "jit shape field: non-object receiver raw=0x{object_raw:016x} shape={required_shape:#x} slot={expected_slot}"
            );
        }
        return JIT_SHAPE_FIELD_FALLBACK_SENTINEL;
    };
    let object = &*object_ptr.as_ptr();
    let Some(adapter) = jit_ensure_shape_adapter_for_object(bridge, object, required_shape) else {
        if std::env::var("RAYA_JIT_DEBUG_SHAPES").is_ok() {
            eprintln!(
                "jit shape field: missing adapter nominal={:?} layout={} shape={required_shape:#x} slot={expected_slot}",
                object.nominal_type_id_usize(),
                object.layout_id(),
            );
        }
        return JIT_SHAPE_FIELD_FALLBACK_SENTINEL;
    };
    let binding = adapter.binding_for_slot(expected_slot as usize);
    if std::env::var("RAYA_JIT_DEBUG_SHAPES").is_ok() {
        eprintln!(
            "jit shape field: nominal={:?} layout={} shape={required_shape:#x} slot={} binding={:?}",
            object.nominal_type_id_usize(),
            object.layout_id(),
            expected_slot,
            binding,
        );
    }
    match binding {
        StructuralSlotBinding::Field(slot) => object.get_field(slot).unwrap_or(Value::null()).raw(),
        StructuralSlotBinding::Dynamic(key) => object
            .dyn_props()
            .and_then(|dp| dp.get(key).map(|p| p.value))
            .unwrap_or(Value::null())
            .raw(),
        StructuralSlotBinding::Method(method_slot) => {
            let Some(nominal_type_id) = object.nominal_type_id_usize() else {
                return JIT_SHAPE_FIELD_FALLBACK_SENTINEL;
            };
            let (func_id, method_module) = {
                let classes = (&*bridge.classes).read();
                let Some(class) = classes.get_class(nominal_type_id) else {
                    return JIT_SHAPE_FIELD_FALLBACK_SENTINEL;
                };
                let Some(fid) = class.vtable.get_method(method_slot) else {
                    return JIT_SHAPE_FIELD_FALLBACK_SENTINEL;
                };
                (fid, class.module.clone())
            };
            let bound = Object::new_bound_method(object_val, func_id, method_module);
            let mut gc = (&*bridge.gc).lock();
            let bm_ptr = gc.allocate(bound);
            Value::from_ptr(NonNull::new(bm_ptr.as_ptr()).unwrap()).raw()
        }
        StructuralSlotBinding::Missing => Value::null().raw(),
    }
}

unsafe extern "C" fn helper_object_set_shape_field(
    object_raw: u64,
    required_shape: u64,
    expected_slot: u32,
    value_raw: u64,
    _func_id: u32,
    _module_ptr: *const (),
    shared_state: *mut (),
) -> i8 {
    if shared_state.is_null() {
        return JIT_STORE_FALLBACK;
    }
    let bridge = &*(shared_state.cast::<JitRuntimeBridgeContext>());
    let object_val = Value::from_raw(object_raw);
    let Some(object_ptr) = jit_object_ptr_checked(object_val) else {
        return JIT_STORE_FALLBACK;
    };
    let object = &mut *object_ptr.as_ptr();
    let Some(adapter) = jit_ensure_shape_adapter_for_object(bridge, object, required_shape) else {
        return JIT_STORE_FALLBACK;
    };
    match adapter.binding_for_slot(expected_slot as usize) {
        StructuralSlotBinding::Field(slot) => object
            .set_field(slot, Value::from_raw(value_raw))
            .map(|_| JIT_STORE_SUCCESS)
            .unwrap_or(JIT_STORE_FALLBACK),
        StructuralSlotBinding::Dynamic(key) => {
            object
                .ensure_dyn_props()
                .insert(key, DynProp::data(Value::from_raw(value_raw)));
            JIT_STORE_SUCCESS
        }
        StructuralSlotBinding::Method(_) | StructuralSlotBinding::Missing => JIT_STORE_FALLBACK,
    }
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
        let task = Arc::new(Task::new(0, module, None));
        let bridge = build_runtime_bridge_context(
            safepoint.as_ref(),
            &task,
            &shared.gc,
            &shared.classes,
            &shared.layouts,
            &shared.mutex_registry,
            &shared.semaphore_registry,
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
            &shared.module_layouts,
            &shared.module_registry,
            &shared.metadata,
            &shared.class_metadata,
            &shared.native_handler,
            &shared.resolved_natives,
            &shared.structural_shape_names,
            &shared.structural_layout_shapes,
            &shared.structural_shape_adapters,
            &shared.aot_profile,
            &shared.type_handles,
            &shared.class_value_slots,
            &shared.prop_keys,
            &shared.stack_pool,
            shared.max_preemptions,
            0,
            None,
        );

        let result = unsafe {
            helper_kernel_call_dispatch(
                crate::compiler::ir::encode_kernel_op_id(KernelOp::VmNative(0)),
                std::ptr::null(),
                0,
                std::ptr::null(),
                (&bridge as *const JitRuntimeBridgeContext) as *mut (),
            )
        };
        assert_eq!(result.status, crate::vm::suspend::BackendCallStatus::Completed);
        assert_eq!(result.payload, Value::i32(88).raw());
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
        let task = Arc::new(Task::new(0, module, None));
        let bridge = build_runtime_bridge_context(
            safepoint.as_ref(),
            &task,
            &shared.gc,
            &shared.classes,
            &shared.layouts,
            &shared.mutex_registry,
            &shared.semaphore_registry,
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
            &shared.module_layouts,
            &shared.module_registry,
            &shared.metadata,
            &shared.class_metadata,
            &shared.native_handler,
            &shared.resolved_natives,
            &shared.structural_shape_names,
            &shared.structural_layout_shapes,
            &shared.structural_shape_adapters,
            &shared.aot_profile,
            &shared.type_handles,
            &shared.class_value_slots,
            &shared.prop_keys,
            &shared.stack_pool,
            shared.max_preemptions,
            0,
            Some(&tx),
        );

        let result = unsafe {
            helper_kernel_call_dispatch(
                crate::compiler::ir::encode_kernel_op_id(KernelOp::VmNative(0)),
                std::ptr::null(),
                0,
                std::ptr::null(),
                (&bridge as *const JitRuntimeBridgeContext) as *mut (),
            )
        };
        assert_eq!(result.status, crate::vm::suspend::BackendCallStatus::Suspended);
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

        let mut target_module = Module::new("jit-target".to_string());
        target_module.classes.push(ClassDef {
            name: "Target".to_string(),
            field_count: 2,
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

        let task = Arc::new(Task::new(0, target_module.clone(), None));
        let bridge = build_runtime_bridge_context(
            safepoint.as_ref(),
            &task,
            &shared.gc,
            &shared.classes,
            &shared.layouts,
            &shared.mutex_registry,
            &shared.semaphore_registry,
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
            &shared.module_layouts,
            &shared.module_registry,
            &shared.metadata,
            &shared.class_metadata,
            &shared.native_handler,
            &shared.resolved_natives,
            &shared.structural_shape_names,
            &shared.structural_layout_shapes,
            &shared.structural_shape_adapters,
            &shared.aot_profile,
            &shared.type_handles,
            &shared.class_value_slots,
            &shared.prop_keys,
            &shared.stack_pool,
            shared.max_preemptions,
            0,
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
