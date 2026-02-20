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

use crate::vm::gc::GarbageCollector as Gc;
use crate::vm::object::{Array, Buffer, ChannelObject, Object, RayaString};
use crate::vm::scheduler::TaskId;
use crate::vm::interpreter::ClassRegistry;
use crate::vm::reflect::ClassMetadataRegistry;
use crate::vm::value::Value;

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

    /// Current task ID
    pub(crate) current_task: TaskId,

    /// Reflect metadata for field/method name lookups
    pub(crate) class_metadata: &'a RwLock<ClassMetadataRegistry>,
}

impl<'a> EngineContext<'a> {
    /// Create a new engine context (VM-internal only)
    #[doc(hidden)]
    pub fn new(
        gc: &'a Mutex<Gc>,
        classes: &'a RwLock<ClassRegistry>,
        current_task: TaskId,
        class_metadata: &'a RwLock<ClassMetadataRegistry>,
    ) -> Self {
        Self {
            gc,
            classes,
            current_task,
            class_metadata,
        }
    }

    /// Allocate a GC pointer and wrap as NativeValue
    fn alloc_ptr<T: 'static>(&self, obj: T) -> NativeValue {
        let gc_ptr = self.gc.lock().allocate(obj);
        let ptr = unsafe { NonNull::new(gc_ptr.as_ptr()).unwrap() };
        value_to_native(unsafe { Value::from_ptr(ptr) })
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
        let mut buffer = Buffer::new(data.len());
        for (i, &byte) in data.iter().enumerate() {
            let _ = buffer.set_byte(i, byte);
        }
        self.alloc_ptr(buffer)
    }

    fn create_array(&self, items: &[NativeValue]) -> NativeValue {
        let mut arr = Array::new(0, 0);
        for item in items {
            arr.push(native_to_value(*item));
        }
        self.alloc_ptr(arr)
    }

    fn create_object_by_id(&self, class_id: usize) -> AbiResult<NativeValue> {
        let field_count = {
            let classes = self.classes.read();
            let class = classes
                .get_class(class_id)
                .ok_or_else(|| format!("Class {} not found", class_id))?;
            class.field_count
        };
        let obj = Object::new(class_id, field_count);
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
        let s_ptr = unsafe { v.as_ptr::<RayaString>() }
            .ok_or_else(|| "Expected string".to_string())?;
        let s = unsafe { &*s_ptr.as_ptr() };
        Ok(s.data.clone())
    }

    fn read_buffer(&self, val: NativeValue) -> AbiResult<Vec<u8>> {
        let v = native_to_value(val);
        if !v.is_ptr() {
            return Err("Expected Buffer, got non-pointer".into());
        }
        let buf_ptr = unsafe { v.as_ptr::<Buffer>() }
            .ok_or_else(|| "Expected Buffer".to_string())?;
        let buffer = unsafe { &*buf_ptr.as_ptr() };
        Ok((0..buffer.length())
            .filter_map(|i| buffer.get_byte(i))
            .collect())
    }

    // ========================================================================
    // Array Operations
    // ========================================================================

    fn array_len(&self, val: NativeValue) -> AbiResult<usize> {
        let v = native_to_value(val);
        if !v.is_ptr() {
            return Err("Expected Array, got non-pointer".into());
        }
        let arr_ptr = unsafe { v.as_ptr::<Array>() }
            .ok_or_else(|| "Expected Array".to_string())?;
        let array = unsafe { &*arr_ptr.as_ptr() };
        Ok(array.len())
    }

    fn array_get(&self, val: NativeValue, index: usize) -> AbiResult<NativeValue> {
        let v = native_to_value(val);
        if !v.is_ptr() {
            return Err("Expected Array, got non-pointer".into());
        }
        let arr_ptr = unsafe { v.as_ptr::<Array>() }
            .ok_or_else(|| "Expected Array".to_string())?;
        let array = unsafe { &*arr_ptr.as_ptr() };
        array
            .get(index)
            .map(value_to_native)
            .ok_or_else(|| format!("Array index {} out of bounds (len={})", index, array.len()).into())
    }

    // ========================================================================
    // Object Operations
    // ========================================================================

    fn object_get_field(&self, val: NativeValue, index: usize) -> AbiResult<NativeValue> {
        let v = native_to_value(val);
        if !v.is_ptr() {
            return Err("Expected Object, got non-pointer".into());
        }
        let obj_ptr = unsafe { v.as_ptr::<Object>() }
            .ok_or_else(|| "Expected Object".to_string())?;
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
        let obj_ptr = unsafe { v.as_ptr::<Object>() }
            .ok_or_else(|| "Expected Object".to_string())?;
        let obj = unsafe { &mut *(obj_ptr.as_ptr() as *mut Object) };
        let _ = obj.set_field(index, native_to_value(value));
        Ok(())
    }

    fn object_class_id(&self, val: NativeValue) -> AbiResult<usize> {
        let v = native_to_value(val);
        if !v.is_ptr() {
            return Err("Expected Object, got non-pointer".into());
        }
        let obj_ptr = unsafe { v.as_ptr::<Object>() }
            .ok_or_else(|| "Expected Object".to_string())?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        Ok(obj.class_id)
    }

    // ========================================================================
    // Class Operations
    // ========================================================================

    fn class_info(&self, class_id: usize) -> AbiResult<ClassInfo> {
        let classes = self.classes.read();
        let class = classes
            .get_class(class_id)
            .ok_or_else(|| format!("Class {} not found", class_id))?;

        Ok(ClassInfo {
            class_id,
            field_count: class.field_count,
            name: class.name.clone(),
            parent_id: class.parent_id,
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
            class_id: class.id,
            field_count: class.field_count,
            name: class.name.clone(),
            parent_id: class.parent_id,
            constructor_id: None,
            method_count: 0,
        })
    }

    fn class_field_names(&self, class_id: usize) -> AbiResult<Vec<(String, usize)>> {
        let meta = self.class_metadata.read();
        match meta.get(class_id) {
            Some(m) => Ok(m
                .field_names
                .iter()
                .enumerate()
                .map(|(i, name)| (name.clone(), i))
                .collect()),
            None => Ok(Vec::new()),
        }
    }

    fn class_method_entries(&self, class_id: usize) -> AbiResult<Vec<(String, usize)>> {
        let meta = self.class_metadata.read();
        match meta.get(class_id) {
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

    fn spawn_function(&self, _func_id: usize, _args: &[NativeValue]) -> AbiResult<u64> {
        // TODO: Implement task spawning via scheduler
        Err("spawn_function not yet implemented".into())
    }

    fn await_task(&self, _task_id: u64) -> AbiResult<NativeValue> {
        // TODO: Implement task await
        Err("await_task not yet implemented".into())
    }

    fn task_is_done(&self, _task_id: u64) -> bool {
        false // TODO
    }

    fn task_cancel(&self, _task_id: u64) {
        // TODO
    }

    // ========================================================================
    // Function Execution
    // ========================================================================

    fn call_function(&self, _func_id: usize, _args: &[NativeValue]) -> AbiResult<NativeValue> {
        // TODO: Implement function execution
        Err("call_function not yet implemented".into())
    }

    fn call_method(
        &self,
        _receiver: NativeValue,
        _class_id: usize,
        _method_name: &str,
        _args: &[NativeValue],
    ) -> AbiResult<NativeValue> {
        // TODO: Implement method calls
        Err("call_method not yet implemented".into())
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
    ///
    /// Handles both direct channel pointers and Channel<T> objects (field 0 = channelId).
    fn extract_channel(&self, val: NativeValue) -> AbiResult<&ChannelObject> {
        let v = native_to_value(val);
        if !v.is_ptr() {
            return Err("Expected channel, got non-pointer".into());
        }

        // Try direct ChannelObject pointer first
        if let Some(ptr) = unsafe { v.as_ptr::<ChannelObject>() } {
            return Ok(unsafe { &*ptr.as_ptr() });
        }

        // Try as Object (Channel<T> class) — field 0 is channelId
        if let Some(obj_ptr) = unsafe { v.as_ptr::<Object>() } {
            let obj = unsafe { &*obj_ptr.as_ptr() };
            if let Some(field_val) = obj.get_field(0) {
                if field_val.is_ptr() {
                    if let Some(ch_ptr) = unsafe { field_val.as_ptr::<ChannelObject>() } {
                        return Ok(unsafe { &*ch_ptr.as_ptr() });
                    }
                }
            }
        }

        Err("Cannot extract channel from value".into())
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
    let buf_ptr = unsafe { v.as_ptr::<Buffer>() }
        .ok_or_else(|| "Expected Buffer".to_string())?;
    let buffer = unsafe { &*buf_ptr.as_ptr() };
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
    let s_ptr = unsafe { v.as_ptr::<RayaString>() }
        .ok_or_else(|| "Expected string".to_string())?;
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
    let arr_ptr = unsafe { v.as_ptr::<Array>() }
        .ok_or_else(|| "Expected Array".to_string())?;
    Ok(unsafe { &*arr_ptr.as_ptr() }.len())
}

/// Read array element at index
pub fn array_get(val: NativeValue, index: usize) -> AbiResult<NativeValue> {
    let v = native_to_value(val);
    if !v.is_ptr() {
        return Err("Expected Array, got non-pointer".into());
    }
    let arr_ptr = unsafe { v.as_ptr::<Array>() }
        .ok_or_else(|| "Expected Array".to_string())?;
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
    let obj_ptr = unsafe { v.as_ptr::<Object>() }
        .ok_or_else(|| "Expected Object".to_string())?;
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
    let obj_ptr = unsafe { v.as_ptr::<Object>() }
        .ok_or_else(|| "Expected Object".to_string())?;
    let obj = unsafe { &mut *(obj_ptr.as_ptr() as *mut Object) };
    let _ = obj.set_field(field_index, native_to_value(value));
    Ok(())
}

/// Get object class ID
pub fn object_class_id(val: NativeValue) -> AbiResult<usize> {
    let v = native_to_value(val);
    if !v.is_ptr() {
        return Err("Expected Object, got non-pointer".into());
    }
    let obj_ptr = unsafe { v.as_ptr::<Object>() }
        .ok_or_else(|| "Expected Object".to_string())?;
    Ok(unsafe { &*obj_ptr.as_ptr() }.class_id)
}

/// Allocate a new Object
pub fn object_allocate(ctx: &EngineContext, class_id: usize, field_count: usize) -> NativeValue {
    let obj = Object::new(class_id, field_count);
    ctx.alloc_ptr(obj)
}

/// Get class information by ID
pub fn class_get_info(ctx: &EngineContext, class_id: usize) -> AbiResult<ClassInfo> {
    ctx.class_info(class_id)
}

/// Spawn a new task (TODO)
pub fn task_spawn(
    _ctx: &EngineContext,
    _function_id: usize,
    _args: &[NativeValue],
) -> AbiResult<u64> {
    Err("task_spawn not yet implemented".into())
}

/// Cancel a task (TODO)
pub fn task_cancel(_ctx: &EngineContext, _task_id: u64) -> AbiResult<()> {
    Err("task_cancel not yet implemented".into())
}

/// Check if a task is done (TODO)
pub fn task_is_done(_ctx: &EngineContext, _task_id: u64) -> AbiResult<bool> {
    Err("task_is_done not yet implemented".into())
}
