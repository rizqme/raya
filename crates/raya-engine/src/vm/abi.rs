//! Engine ABI implementation
//!
//! Provides `EngineContext` — the engine's concrete implementation of
//! `raya_sdk::NativeContext`. This bridges the SDK trait interface to
//! the engine's internal VM subsystems (GC, class registry, scheduler).
//!
//! # Value Conversion
//!
//! SDK `NativeValue` and engine `Value` use identical NaN-boxing (u64).
//! Conversion is zero-cost via `value_to_native`/`native_to_value`.

use parking_lot::{Mutex, RwLock};
use std::ptr::NonNull;
#[allow(unused_imports)]
use std::sync::Arc;

use raya_sdk::{AbiResult, ClassInfo, NativeContext, NativeValue};
use rustc_hash::FxHashMap;

use crate::compiler::Module;
use crate::vm::gc::GarbageCollector as Gc;
use crate::vm::interpreter::{
    ClassRegistry, Interpreter, PromiseHandle, RuntimeLayoutRegistry, SharedVmState,
};
use crate::vm::object::{Array, Buffer, ChannelObject, Class, Object, RayaString};
use crate::vm::reflect::ClassMetadataRegistry;
use crate::vm::scheduler::Task;
use crate::vm::scheduler::{TaskId, TaskState};
use crate::vm::value::Value;
use crossbeam_deque::Injector;
use crate::compiler::compiled_support::CompiledNumericIntrinsicOp;

// ============================================================================
// Zero-cost Value Conversion
// ============================================================================

/// Convert engine Value → SDK NativeValue (zero-cost — same u64 bits)
#[inline(always)]
pub fn value_to_native(val: Value) -> NativeValue {
    NativeValue::from_bits(val.raw())
}

/// Convert SDK NativeValue → engine Value (zero-cost — same u64 bits)
#[inline(always)]
pub fn native_to_value(val: NativeValue) -> Value {
    unsafe { Value::from_raw(val.to_bits()) }
}

/// Execute an exact numeric intrinsic for compiled backends.
pub fn dispatch_compiled_numeric_intrinsic(
    op: CompiledNumericIntrinsicOp,
    lhs_raw: u64,
    rhs_raw: u64,
) -> u64 {
    match op {
        CompiledNumericIntrinsicOp::I32Pow => {
            let lhs = lhs_raw as i32;
            let rhs = rhs_raw as i32;
            let result = if rhs < 0 {
                0
            } else {
                lhs.wrapping_pow(rhs as u32)
            };
            (result as i64) as u64
        }
        CompiledNumericIntrinsicOp::F64Pow => {
            let lhs = f64::from_bits(lhs_raw);
            let rhs = f64::from_bits(rhs_raw);
            lhs.powf(rhs).to_bits()
        }
        CompiledNumericIntrinsicOp::F64Mod => {
            let lhs = f64::from_bits(lhs_raw);
            let rhs = f64::from_bits(rhs_raw);
            (lhs % rhs).to_bits()
        }
    }
}

// ============================================================================
// EngineContext
// ============================================================================

/// Engine's concrete implementation of `NativeContext`.
///
/// Bridges the SDK trait to the engine's internal subsystems:
/// - GC for memory allocation
/// - ClassRegistry for type information
/// - ClassMetadataRegistry for field/method names (reflection)
/// - Scheduler for task management
pub struct EngineContext<'a> {
    /// GC for allocating strings, buffers, objects, arrays
    pub(crate) gc: &'a Mutex<Gc>,

    /// Class registry for type information and instance creation
    pub(crate) classes: &'a RwLock<ClassRegistry>,

    /// Physical runtime layout registry for object allocation metadata.
    pub(crate) layouts: &'a RwLock<RuntimeLayoutRegistry>,

    /// Current task ID
    pub(crate) current_task: TaskId,

    /// Reflect metadata for field/method name lookups
    pub(crate) class_metadata: &'a RwLock<ClassMetadataRegistry>,

    /// Optional shared task registry/injector for task inspection/cancellation.
    pub(crate) tasks: Option<&'a Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>>,
    pub(crate) injector: Option<&'a Arc<Injector<Arc<Task>>>>,

    /// Optional full runtime handles for spawn/call services.
    pub(crate) shared_runtime: Option<&'a SharedVmState>,
    pub(crate) current_task_arc: Option<&'a Arc<Task>>,
}

impl<'a> EngineContext<'a> {
    /// Create a new engine context (VM-internal only)
    #[doc(hidden)]
    pub fn new(
        gc: &'a Mutex<Gc>,
        classes: &'a RwLock<ClassRegistry>,
        layouts: &'a RwLock<RuntimeLayoutRegistry>,
        current_task: TaskId,
        class_metadata: &'a RwLock<ClassMetadataRegistry>,
    ) -> Self {
        Self {
            gc,
            classes,
            layouts,
            current_task,
            class_metadata,
            tasks: None,
            injector: None,
            shared_runtime: None,
            current_task_arc: None,
        }
    }

    pub fn with_scheduler(
        mut self,
        tasks: &'a Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,
        injector: &'a Arc<Injector<Arc<Task>>>,
    ) -> Self {
        self.tasks = Some(tasks);
        self.injector = Some(injector);
        self
    }

    pub fn with_shared_runtime(
        mut self,
        shared_runtime: &'a SharedVmState,
        current_task_arc: &'a Arc<Task>,
    ) -> Self {
        self.tasks = Some(&shared_runtime.tasks);
        self.injector = Some(&shared_runtime.injector);
        self.shared_runtime = Some(shared_runtime);
        self.current_task_arc = Some(current_task_arc);
        self
    }

    /// Allocate a GC pointer and wrap as NativeValue
    fn alloc_ptr<T: 'static>(&self, obj: T) -> NativeValue {
        let gc_ptr = self.gc.lock().allocate(obj);
        let ptr = NonNull::new(gc_ptr.as_ptr()).unwrap();
        value_to_native(unsafe { Value::from_ptr(ptr) })
    }

    fn register_runtime_class(&self, class: Class) -> usize {
        self.register_runtime_class_with_layout_names(class, None::<&[&str]>)
    }

    fn register_runtime_class_with_layout_names(
        &self,
        class: Class,
        layout_names: impl Into<Option<&'static [&'static str]>>,
    ) -> usize {
        let layout_id = self.layouts.write().allocate_nominal_layout_id();
        let field_count = class.field_count;
        let class_name = class.name.clone();
        let id = self.classes.write().register_class(class);
        self.layouts
            .write()
            .register_nominal_layout(id, layout_id, field_count, Some(class_name));
        if let Some(layout_names) = layout_names.into() {
            let owned_names = layout_names
                .iter()
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>();
            self.layouts
                .write()
                .register_layout_shape(layout_id, &owned_names);
        }
        id
    }

    fn read_buffer_from_handle(&self, handle: u64) -> AbiResult<Vec<u8>> {
        let buf_ptr = handle as *const Buffer;
        if buf_ptr.is_null() {
            return Err("Invalid buffer handle (null)".into());
        }
        let buffer = unsafe { &*buf_ptr };
        Ok((0..buffer.length())
            .filter_map(|i| buffer.get_byte(i))
            .collect())
    }

    fn read_buffer_from_object(&self, obj: &Object) -> AbiResult<Vec<u8>> {
        let nominal_type_id = obj
            .nominal_type_id_usize()
            .ok_or_else(|| "Expected nominal Buffer object".to_string())?;
        let class_name = {
            let classes = self.classes.read();
            let class = classes
                .get_class(nominal_type_id)
                .ok_or_else(|| "Buffer class metadata missing".to_string())?;
            class.name.clone()
        };
        if class_name != "Buffer" {
            return Err("Expected Buffer object".into());
        }

        let handle_field_index = {
            let class_metadata = self.class_metadata.read();
            class_metadata
                .get(nominal_type_id)
                .and_then(|meta| meta.get_field_index("bufferPtr"))
                .unwrap_or(0)
        };
        let handle = obj
            .get_field(handle_field_index)
            .and_then(|v| v.as_u64())
            .ok_or_else(|| "Buffer object missing valid bufferPtr handle".to_string())?;
        self.read_buffer_from_handle(handle)
    }

    fn shared_task(&self, task_id: u64) -> AbiResult<Arc<Task>> {
        let Some(tasks) = self.tasks else {
            return Err(
                "task services require scheduler-backed EngineContext handles".to_string().into(),
            );
        };
        tasks.read()
            .get(&TaskId::from_u64(task_id))
            .cloned()
            .ok_or_else(|| format!("Task {} not found", task_id).into())
    }

    fn build_runtime_interpreter(&self) -> AbiResult<Interpreter<'_>> {
        let Some(shared) = self.shared_runtime else {
            return Err(
                "spawn_function requires a shared-runtime EngineContext".to_string().into(),
            );
        };

        Ok(Interpreter::new(
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
        ))
    }
}

impl NativeContext for EngineContext<'_> {
    // ========================================================================
    // Value Creation
    // ========================================================================

    fn create_string(&self, s: &str) -> NativeValue {
        self.alloc_ptr(RayaString::new(s.to_string()))
    }

    fn create_buffer(&self, data: &[u8]) -> NativeValue {
        // Create the raw Buffer.
        let mut buffer = Buffer::new(data.len());
        for (i, &byte) in data.iter().enumerate() {
            let _ = buffer.set_byte(i, byte);
        }

        // Buffer values are always wrapped as Buffer class objects.
        let (buffer_nominal_type_id, buffer_field_count, buffer_layout_id) = {
            let mut classes = self.classes.write();
            if let Some(id) = classes.get_class_by_name("Buffer").map(|class| class.id) {
                let (layout_id, field_count) = self
                    .layouts
                    .read()
                    .nominal_allocation(id)
                    .expect("registered Buffer allocation");
                (id, field_count.max(2), layout_id)
            } else {
                drop(classes);
                let id = self.register_runtime_class_with_layout_names(
                    Class::new(0, "Buffer".to_string(), 2),
                    Some(crate::vm::object::BUFFER_LAYOUT_FIELDS),
                );
                let (layout_id, field_count) = self
                    .layouts
                    .read()
                    .nominal_allocation(id)
                    .expect("registered Buffer allocation");
                (id, field_count.max(2), layout_id)
            }
        };

        let obj_ptr = {
            let mut gc = self.gc.lock();
            // Buffer handles are exposed as opaque u64 values to native callers.
            // Use a stable boxed allocation so the handle never dangles due to GC.
            let handle = Box::into_raw(Box::new(buffer)) as u64;

            let mut obj = Object::new_nominal(
                buffer_layout_id,
                buffer_nominal_type_id as u32,
                buffer_field_count,
            );
            let _ = obj.set_field(0, Value::u64(handle));
            let _ = obj.set_field(1, Value::i32(data.len() as i32));
            gc.allocate(obj)
        };

        let value = unsafe { Value::from_ptr(NonNull::new(obj_ptr.as_ptr()).unwrap()) };
        value_to_native(value)
    }

    fn create_array(&self, items: &[NativeValue]) -> NativeValue {
        let mut arr = Array::new(0, 0);
        for item in items {
            arr.push(native_to_value(*item));
        }
        self.alloc_ptr(arr)
    }

    fn create_object_by_nominal_type_id(&self, nominal_type_id: usize) -> AbiResult<NativeValue> {
        let (layout_id, field_count) = self
            .layouts
            .read()
            .nominal_allocation(nominal_type_id)
            .ok_or_else(|| format!("Nominal type {} not found", nominal_type_id))?;
        let obj = Object::new_nominal(layout_id, nominal_type_id as u32, field_count);
        Ok(self.alloc_ptr(obj))
    }

    // ========================================================================
    // Value Reading
    // ========================================================================

    fn read_string(&self, val: NativeValue) -> AbiResult<String> {
        let v = native_to_value(val);
        if !v.is_ptr() {
            return Err("Expected string, got non-pointer".into());
        }
        let s_ptr =
            unsafe { v.as_ptr::<RayaString>() }.ok_or_else(|| "Expected string".to_string())?;
        let s = unsafe { &*s_ptr.as_ptr() };
        Ok(s.data.clone())
    }

    fn read_buffer(&self, val: NativeValue) -> AbiResult<Vec<u8>> {
        let v = native_to_value(val);
        if !v.is_ptr() {
            return Err("Expected Buffer".into());
        }
        let obj_ptr =
            unsafe { v.as_ptr::<Object>() }.ok_or_else(|| "Expected Buffer object".to_string())?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        self.read_buffer_from_object(obj)
    }

    // ========================================================================
    // Array Operations
    // ========================================================================

    fn array_len(&self, val: NativeValue) -> AbiResult<usize> {
        let v = native_to_value(val);
        if !v.is_ptr() {
            return Err("Expected Array, got non-pointer".into());
        }
        let arr_ptr = unsafe { v.as_ptr::<Array>() }.ok_or_else(|| "Expected Array".to_string())?;
        let array = unsafe { &*arr_ptr.as_ptr() };
        Ok(array.len())
    }

    fn array_get(&self, val: NativeValue, index: usize) -> AbiResult<NativeValue> {
        let v = native_to_value(val);
        if !v.is_ptr() {
            return Err("Expected Array, got non-pointer".into());
        }
        let arr_ptr = unsafe { v.as_ptr::<Array>() }.ok_or_else(|| "Expected Array".to_string())?;
        let array = unsafe { &*arr_ptr.as_ptr() };
        array.get(index).map(value_to_native).ok_or_else(|| {
            format!("Array index {} out of bounds (len={})", index, array.len()).into()
        })
    }

    // ========================================================================
    // Object Operations
    // ========================================================================

    fn object_get_field(&self, val: NativeValue, index: usize) -> AbiResult<NativeValue> {
        let v = native_to_value(val);
        if !v.is_ptr() {
            return Err("Expected Object, got non-pointer".into());
        }
        let obj_ptr =
            unsafe { v.as_ptr::<Object>() }.ok_or_else(|| "Expected Object".to_string())?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        obj.get_field(index)
            .map(value_to_native)
            .ok_or_else(|| format!("Field index {} out of bounds", index).into())
    }

    fn object_set_field(
        &self,
        val: NativeValue,
        index: usize,
        value: NativeValue,
    ) -> AbiResult<()> {
        let v = native_to_value(val);
        if !v.is_ptr() {
            return Err("Expected Object, got non-pointer".into());
        }
        let obj_ptr =
            unsafe { v.as_ptr::<Object>() }.ok_or_else(|| "Expected Object".to_string())?;
        let obj = unsafe { &mut *obj_ptr.as_ptr() };
        let _ = obj.set_field(index, native_to_value(value));
        Ok(())
    }

    fn object_nominal_type_id(&self, val: NativeValue) -> AbiResult<usize> {
        let v = native_to_value(val);
        if !v.is_ptr() {
            return Err("Expected Object, got non-pointer".into());
        }
        let obj_ptr =
            unsafe { v.as_ptr::<Object>() }.ok_or_else(|| "Expected Object".to_string())?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        obj.nominal_type_id_usize()
            .ok_or_else(|| "Object has no nominal class identity".into())
    }

    // ========================================================================
    // Class Operations
    // ========================================================================

    fn nominal_type_info(&self, nominal_type_id: usize) -> AbiResult<ClassInfo> {
        let classes = self.classes.read();
        let class = classes
            .get_class(nominal_type_id)
            .ok_or_else(|| format!("Nominal type {} not found", nominal_type_id))?;

        Ok(ClassInfo {
            nominal_type_id,
            field_count: class.field_count,
            name: class.name.clone(),
            parent_nominal_type_id: class.parent_id,
            constructor_id: None, // ClassRegistry doesn't store this directly
            method_count: 0,      // Resolved from metadata below if available
        })
    }

    fn class_by_name(&self, name: &str) -> AbiResult<ClassInfo> {
        let classes = self.classes.read();
        let class = classes
            .get_class_by_name(name)
            .ok_or_else(|| format!("Class '{}' not found", name))?;

        Ok(ClassInfo {
            nominal_type_id: class.id,
            field_count: class.field_count,
            name: class.name.clone(),
            parent_nominal_type_id: class.parent_id,
            constructor_id: None,
            method_count: 0,
        })
    }

    fn nominal_type_field_names(&self, nominal_type_id: usize) -> AbiResult<Vec<(String, usize)>> {
        let meta = self.class_metadata.read();
        match meta.get(nominal_type_id) {
            Some(m) => Ok(m
                .field_names
                .iter()
                .enumerate()
                .map(|(i, name)| (name.clone(), i))
                .collect()),
            None => Ok(Vec::new()),
        }
    }

    fn nominal_type_method_entries(
        &self,
        nominal_type_id: usize,
    ) -> AbiResult<Vec<(String, usize)>> {
        let meta = self.class_metadata.read();
        match meta.get(nominal_type_id) {
            Some(m) => Ok(m
                .method_names
                .iter()
                .enumerate()
                .filter(|(_, name)| !name.is_empty())
                .map(|(i, name)| (name.clone(), i))
                .collect()),
            None => Ok(Vec::new()),
        }
    }

    // ========================================================================
    // Task Operations
    // ========================================================================

    fn current_task_id(&self) -> u64 {
        self.current_task.as_u64()
    }

    fn spawn_function(&self, func_id: usize, args: &[NativeValue]) -> AbiResult<u64> {
        let _ = (func_id, args);
        Err(
            "spawn_function requires the exact scheduler-backed runtime spawn ABI and is unavailable through plain EngineContext"
                .to_string()
                .into(),
        )
    }

    fn await_task(&self, task_id: u64) -> AbiResult<NativeValue> {
        let task = self.shared_task(task_id)?;
        if task.is_cancelled() {
            return Err(format!("Awaited task {} cancelled", task_id).into());
        }
        match task.state() {
            TaskState::Completed => Ok(value_to_native(task.result().unwrap_or(Value::null()))),
            TaskState::Failed => Err(format!("Awaited task {} failed", task_id).into()),
            _ => Err(
                "await_task cannot suspend through NativeContext; the task is not complete"
                    .to_string()
                    .into(),
            ),
        }
    }

    fn task_is_done(&self, task_id: u64) -> bool {
        self.shared_task(task_id).map_or(false, |task| {
            task.is_cancelled() || matches!(task.state(), TaskState::Completed | TaskState::Failed)
        })
    }

    fn task_cancel(&self, task_id: u64) {
        if let Ok(task) = self.shared_task(task_id) {
            task.cancel();
            if let Some(injector) = self.injector {
                injector.push(task);
            }
        }
    }

    // ========================================================================
    // Function Execution
    // ========================================================================

    fn call_function(&self, _func_id: usize, _args: &[NativeValue]) -> AbiResult<NativeValue> {
        Err(
            "call_function requires the exact compiled/runtime call ABI and is unavailable through plain EngineContext"
                .to_string()
                .into(),
        )
    }

    fn call_method(
        &self,
        _receiver: NativeValue,
        _class_id: usize,
        _method_name: &str,
        _args: &[NativeValue],
    ) -> AbiResult<NativeValue> {
        Err(
            "call_method requires the exact compiled/runtime call ABI and is unavailable through plain EngineContext"
                .to_string()
                .into(),
        )
    }

    // ========================================================================
    // Channel Operations
    // ========================================================================

    fn channel_send(&self, channel: NativeValue, value: NativeValue) -> AbiResult<bool> {
        let ch = self.extract_channel(channel)?;
        Ok(ch.try_send(native_to_value(value)))
    }

    fn channel_receive(&self, channel: NativeValue) -> AbiResult<Option<NativeValue>> {
        let ch = self.extract_channel(channel)?;
        Ok(ch.try_receive().map(value_to_native))
    }

    fn channel_try_receive(&self, channel: NativeValue) -> Option<NativeValue> {
        if let Ok(ch) = self.extract_channel(channel) {
            ch.try_receive().map(value_to_native)
        } else {
            None
        }
    }

    fn channel_try_send(&self, channel: NativeValue, value: NativeValue) -> bool {
        if let Ok(ch) = self.extract_channel(channel) {
            ch.try_send(native_to_value(value))
        } else {
            false
        }
    }

    fn channel_close(&self, channel: NativeValue) {
        if let Ok(ch) = self.extract_channel(channel) {
            ch.close();
        }
    }

    fn channel_is_closed(&self, channel: NativeValue) -> bool {
        if let Ok(ch) = self.extract_channel(channel) {
            ch.is_closed()
        } else {
            true
        }
    }
}

impl EngineContext<'_> {
    /// Extract a ChannelObject reference from a NativeValue.
    fn extract_channel(&self, val: NativeValue) -> AbiResult<&ChannelObject> {
        let v = native_to_value(val);
        let handle = v
            .as_u64()
            .ok_or_else(|| "Expected channel handle (u64)".to_string())?;
        let ch_ptr = handle as *const ChannelObject;
        if ch_ptr.is_null() {
            return Err("Expected channel handle (u64)".into());
        }
        Ok(unsafe { &*ch_ptr })
    }
}

// ============================================================================
// Backward-compatible free functions (delegates to EngineContext methods)
// ============================================================================

/// Read bytes from a Buffer value
pub fn buffer_read_bytes(val: NativeValue) -> AbiResult<Vec<u8>> {
    let v = native_to_value(val);
    if !v.is_ptr() {
        return Err("Expected Buffer, got non-pointer".into());
    }
    let obj_ptr =
        unsafe { v.as_ptr::<Object>() }.ok_or_else(|| "Expected Buffer object".to_string())?;
    let obj = unsafe { &*obj_ptr.as_ptr() };
    let handle = obj
        .get_field(0)
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "Buffer object missing valid bufferPtr handle".to_string())?;
    let buf_ptr = handle as *const Buffer;
    if buf_ptr.is_null() {
        return Err("Invalid buffer handle (null)".into());
    }
    let buffer = unsafe { &*buf_ptr };
    Ok((0..buffer.length())
        .filter_map(|i| buffer.get_byte(i))
        .collect())
}

/// Allocate a new Buffer
pub fn buffer_allocate(ctx: &dyn NativeContext, data: &[u8]) -> NativeValue {
    ctx.create_buffer(data)
}

/// Read string data
pub fn string_read(val: NativeValue) -> AbiResult<String> {
    let v = native_to_value(val);
    if !v.is_ptr() {
        return Err("Expected string, got non-pointer".into());
    }
    let s_ptr = unsafe { v.as_ptr::<RayaString>() }.ok_or_else(|| "Expected string".to_string())?;
    let s = unsafe { &*s_ptr.as_ptr() };
    Ok(s.data.clone())
}

/// Allocate a new String
pub fn string_allocate(ctx: &dyn NativeContext, s: String) -> NativeValue {
    ctx.create_string(&s)
}

/// Read array length
pub fn array_length(val: NativeValue) -> AbiResult<usize> {
    let v = native_to_value(val);
    if !v.is_ptr() {
        return Err("Expected Array, got non-pointer".into());
    }
    let arr_ptr = unsafe { v.as_ptr::<Array>() }.ok_or_else(|| "Expected Array".to_string())?;
    Ok(unsafe { &*arr_ptr.as_ptr() }.len())
}

/// Read array element at index
pub fn array_get(val: NativeValue, index: usize) -> AbiResult<NativeValue> {
    let v = native_to_value(val);
    if !v.is_ptr() {
        return Err("Expected Array, got non-pointer".into());
    }
    let arr_ptr = unsafe { v.as_ptr::<Array>() }.ok_or_else(|| "Expected Array".to_string())?;
    let array = unsafe { &*arr_ptr.as_ptr() };
    array
        .get(index)
        .map(value_to_native)
        .ok_or_else(|| format!("Array index {} out of bounds", index).into())
}

/// Allocate a new Array
pub fn array_allocate(ctx: &dyn NativeContext, items: &[NativeValue]) -> NativeValue {
    ctx.create_array(items)
}

/// Get object field by index
pub fn object_get_field(val: NativeValue, field_index: usize) -> AbiResult<NativeValue> {
    let v = native_to_value(val);
    if !v.is_ptr() {
        return Err("Expected Object, got non-pointer".into());
    }
    let obj_ptr = unsafe { v.as_ptr::<Object>() }.ok_or_else(|| "Expected Object".to_string())?;
    let obj = unsafe { &*obj_ptr.as_ptr() };
    obj.get_field(field_index)
        .map(value_to_native)
        .ok_or_else(|| format!("Field index {} out of bounds", field_index).into())
}

/// Set object field by index
pub fn object_set_field(val: NativeValue, field_index: usize, value: NativeValue) -> AbiResult<()> {
    let v = native_to_value(val);
    if !v.is_ptr() {
        return Err("Expected Object, got non-pointer".into());
    }
    let obj_ptr = unsafe { v.as_ptr::<Object>() }.ok_or_else(|| "Expected Object".to_string())?;
    let obj = unsafe { &mut *obj_ptr.as_ptr() };
    let _ = obj.set_field(field_index, native_to_value(value));
    Ok(())
}

/// Get object nominal type ID.
pub fn object_nominal_type_id(val: NativeValue) -> AbiResult<usize> {
    let v = native_to_value(val);
    if !v.is_ptr() {
        return Err("Expected Object, got non-pointer".into());
    }
    let obj_ptr = unsafe { v.as_ptr::<Object>() }.ok_or_else(|| "Expected Object".to_string())?;
    let obj = unsafe { &*obj_ptr.as_ptr() };
    obj.nominal_type_id_usize()
        .ok_or_else(|| "Object has no nominal class identity".into())
}

/// Allocate a new Object
pub fn object_allocate(
    ctx: &EngineContext<'_>,
    nominal_type_id: usize,
    _field_count: usize,
) -> NativeValue {
    let (layout_id, field_count) = ctx
        .layouts
        .read()
        .nominal_allocation(nominal_type_id)
        .unwrap_or_else(|| panic!("Nominal type {} not found", nominal_type_id));
    let obj = Object::new_nominal(layout_id, nominal_type_id as u32, field_count);
    ctx.alloc_ptr(obj)
}

/// Get nominal type information by ID.
pub fn nominal_type_get_info(
    ctx: &EngineContext<'_>,
    nominal_type_id: usize,
) -> AbiResult<ClassInfo> {
    ctx.nominal_type_info(nominal_type_id)
}

/// Spawn a new task through the scheduler-backed engine context.
pub fn task_spawn(
    ctx: &EngineContext<'_>,
    function_id: usize,
    args: &[NativeValue],
) -> AbiResult<u64> {
    ctx.spawn_function(function_id, args)
}

/// Cancel a task through the scheduler-backed engine context.
pub fn task_cancel(ctx: &EngineContext<'_>, task_id: u64) -> AbiResult<()> {
    let _ = ctx.shared_task(task_id)?;
    ctx.task_cancel(task_id);
    Ok(())
}

/// Check whether a task has reached a terminal state.
pub fn task_is_done(ctx: &EngineContext<'_>, task_id: u64) -> AbiResult<bool> {
    let _ = ctx.shared_task(task_id)?;
    Ok(ctx.task_is_done(task_id))
}
