//! Internal ABI for native handlers
//!
//! Provides controlled access to VM internals for advanced native handlers.
//! This includes:
//! - Memory allocation (GC, strings, buffers)
//! - Class registry (type info, instance creation)
//! - Task scheduler (spawn, cancel, wait)
//! - VM operations (execute, compile)
//!
//! This is an INTERNAL interface - not a public API. It's designed to allow
//! raya-stdlib to implement functionality that needs VM access.

use parking_lot::{Mutex, RwLock};
use std::ptr::NonNull;
use std::sync::Arc;

use crate::vm::gc::GarbageCollector as Gc;
use crate::vm::object::{Buffer, Object, RayaString, Array, Class};
use crate::vm::scheduler::{Scheduler, TaskId};
use crate::vm::interpreter::ClassRegistry;
use crate::vm::value::Value;
use crate::vm::VmError;

// ============================================================================
// Opaque Context Handle
// ============================================================================

/// Context for native handlers with full VM access
///
/// Provides controlled access to VM subsystems:
/// - GC for allocation
/// - Class registry for type operations
/// - Scheduler for task management
/// - Type registry for reflection
pub struct NativeContext<'a> {
    /// GC for allocating strings, buffers, objects
    pub(crate) gc: &'a Mutex<Gc>,

    /// Class registry for type information and instance creation
    pub(crate) classes: &'a RwLock<ClassRegistry>,

    /// Task scheduler for spawning and managing tasks
    pub(crate) scheduler: &'a Arc<Scheduler>,

    /// Current task ID
    pub(crate) current_task: TaskId,
}

impl<'a> NativeContext<'a> {
    /// Create a new native context (VM-internal only)
    #[doc(hidden)]
    pub fn new(
        gc: &'a Mutex<Gc>,
        classes: &'a RwLock<ClassRegistry>,
        scheduler: &'a Arc<Scheduler>,
        current_task: TaskId,
    ) -> Self {
        Self {
            gc,
            classes,
            scheduler,
            current_task,
        }
    }

    /// Get current task ID
    pub fn current_task_id(&self) -> u64 {
        self.current_task.as_u64()
    }
}

// ============================================================================
// Argument Types
// ============================================================================

/// Represents a value passed to/from a native handler
///
/// This is a safe wrapper around the VM's Value type with
/// controlled conversion operations.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct NativeValue(pub(crate) Value);

impl NativeValue {
    /// Create from raw Value (VM-internal only)
    #[doc(hidden)]
    pub fn from_value(v: Value) -> Self {
        Self(v)
    }

    /// Extract raw Value (VM-internal only)
    #[doc(hidden)]
    pub fn into_value(self) -> Value {
        self.0
    }

    /// Check if this is a pointer (object, string, buffer, etc.)
    pub fn is_ptr(&self) -> bool {
        self.0.is_ptr()
    }

    /// Try to extract as i32
    pub fn as_i32(&self) -> Option<i32> {
        self.0.as_i32()
    }

    /// Try to extract as f64
    pub fn as_f64(&self) -> Option<f64> {
        self.0.as_f64()
    }

    /// Try to extract as bool
    pub fn as_bool(&self) -> Option<bool> {
        self.0.as_bool()
    }

    /// Check if value is null
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }

    /// Create an i32 value
    pub fn i32(val: i32) -> Self {
        Self(Value::i32(val))
    }

    /// Create an f64 value
    pub fn f64(val: f64) -> Self {
        Self(Value::f64(val))
    }

    /// Create a bool value
    pub fn bool(val: bool) -> Self {
        Self(Value::bool(val))
    }

    /// Create a null value
    pub fn null() -> Self {
        Self(Value::null())
    }
}

// ============================================================================
// Buffer Operations
// ============================================================================

/// Read bytes from a Buffer value
///
/// # Safety
/// Assumes the value is a valid Buffer pointer. Returns error if not.
pub fn buffer_read_bytes(val: NativeValue) -> Result<Vec<u8>, String> {
    if !val.is_ptr() {
        return Err("Expected Buffer, got non-pointer".to_string());
    }

    let buf_ptr = unsafe { val.0.as_ptr::<Buffer>() }
        .ok_or_else(|| "Expected Buffer".to_string())?;

    let buffer = unsafe { &*buf_ptr.as_ptr() };
    Ok((0..buffer.length())
        .filter_map(|i| buffer.get_byte(i))
        .collect())
}

/// Allocate a new Buffer with the given bytes
pub fn buffer_allocate(ctx: &NativeContext, data: &[u8]) -> NativeValue {
    let mut buffer = Buffer::new(data.len());
    for (i, &byte) in data.iter().enumerate() {
        let _ = buffer.set_byte(i, byte);
    }
    let gc_ptr = ctx.gc.lock().allocate(buffer);
    let ptr = unsafe { NonNull::new(gc_ptr.as_ptr()).unwrap() };
    NativeValue(unsafe { Value::from_ptr(ptr) })
}

// ============================================================================
// String Operations
// ============================================================================

/// Read string data from a String value
///
/// # Safety
/// Assumes the value is a valid RayaString pointer. Returns error if not.
pub fn string_read(val: NativeValue) -> Result<String, String> {
    if !val.is_ptr() {
        return Err("Expected string, got non-pointer".to_string());
    }

    let s_ptr = unsafe { val.0.as_ptr::<RayaString>() }
        .ok_or_else(|| "Expected string".to_string())?;

    let s = unsafe { &*s_ptr.as_ptr() };
    Ok(s.data.clone())
}

/// Allocate a new String
pub fn string_allocate(ctx: &NativeContext, s: String) -> NativeValue {
    let raya_str = RayaString::new(s);
    let gc_ptr = ctx.gc.lock().allocate(raya_str);
    let ptr = unsafe { NonNull::new(gc_ptr.as_ptr()).unwrap() };
    NativeValue(unsafe { Value::from_ptr(ptr) })
}

// ============================================================================
// Array Operations
// ============================================================================

/// Read array length
pub fn array_length(val: NativeValue) -> AbiResult<usize> {
    if !val.is_ptr() {
        return Err("Expected Array, got non-pointer".to_string());
    }

    let arr_ptr = unsafe { val.0.as_ptr::<Array>() }
        .ok_or_else(|| "Expected Array".to_string())?;

    let array = unsafe { &*arr_ptr.as_ptr() };
    Ok(array.len())
}

/// Read array element at index
pub fn array_get(val: NativeValue, index: usize) -> AbiResult<NativeValue> {
    if !val.is_ptr() {
        return Err("Expected Array, got non-pointer".to_string());
    }

    let arr_ptr = unsafe { val.0.as_ptr::<Array>() }
        .ok_or_else(|| "Expected Array".to_string())?;

    let array = unsafe { &*arr_ptr.as_ptr() };
    array
        .get(index)
        .map(NativeValue::from_value)
        .ok_or_else(|| format!("Array index {} out of bounds", index))
}

/// Allocate a new Array
pub fn array_allocate(ctx: &NativeContext, items: &[NativeValue]) -> NativeValue {
    let mut arr = Array::new(0, 0);
    for item in items {
        arr.push(item.0);
    }
    let gc_ptr = ctx.gc.lock().allocate(arr);
    let ptr = unsafe { NonNull::new(gc_ptr.as_ptr()).unwrap() };
    NativeValue(unsafe { Value::from_ptr(ptr) })
}

// ============================================================================
// Object Operations
// ============================================================================

/// Get object field by index
pub fn object_get_field(val: NativeValue, field_index: usize) -> AbiResult<NativeValue> {
    if !val.is_ptr() {
        return Err("Expected Object, got non-pointer".to_string());
    }

    let obj_ptr = unsafe { val.0.as_ptr::<Object>() }
        .ok_or_else(|| "Expected Object".to_string())?;

    let obj = unsafe { &*obj_ptr.as_ptr() };
    obj.get_field(field_index)
        .map(NativeValue::from_value)
        .ok_or_else(|| format!("Field index {} out of bounds", field_index))
}

/// Set object field by index
pub fn object_set_field(val: NativeValue, field_index: usize, value: NativeValue) -> AbiResult<()> {
    if !val.is_ptr() {
        return Err("Expected Object, got non-pointer".to_string());
    }

    let obj_ptr = unsafe { val.0.as_ptr::<Object>() }
        .ok_or_else(|| "Expected Object".to_string())?;

    let obj = unsafe { &mut *(obj_ptr.as_ptr() as *mut Object) };
    obj.set_field(field_index, value.0);
    Ok(())
}

/// Get object class ID
pub fn object_class_id(val: NativeValue) -> AbiResult<usize> {
    if !val.is_ptr() {
        return Err("Expected Object, got non-pointer".to_string());
    }

    let obj_ptr = unsafe { val.0.as_ptr::<Object>() }
        .ok_or_else(|| "Expected Object".to_string())?;

    let obj = unsafe { &*obj_ptr.as_ptr() };
    Ok(obj.class_id)
}

/// Allocate a new Object with the given class ID and field count
pub fn object_allocate(ctx: &NativeContext, class_id: usize, field_count: usize) -> NativeValue {
    let obj = Object::new(class_id, field_count);
    let gc_ptr = ctx.gc.lock().allocate(obj);
    let ptr = unsafe { NonNull::new(gc_ptr.as_ptr()).unwrap() };
    NativeValue(unsafe { Value::from_ptr(ptr) })
}

// ============================================================================
// Class Registry Operations
// ============================================================================

/// Get class information by ID
pub fn class_get_info(ctx: &NativeContext, class_id: usize) -> AbiResult<ClassInfo> {
    let classes = ctx.classes.read();
    let class = classes
        .get_class(class_id)
        .ok_or_else(|| format!("Class {} not found", class_id))?;

    Ok(ClassInfo {
        class_id,
        field_count: class.field_count,
        name: class.name.clone(),
    })
}

/// Information about a class
#[derive(Debug, Clone)]
pub struct ClassInfo {
    pub class_id: usize,
    pub field_count: usize,
    pub name: String,
}

// ============================================================================
// Task Scheduler Operations
// ============================================================================

/// Spawn a new task
///
/// Returns the task ID as u64
pub fn task_spawn(
    ctx: &NativeContext,
    function_id: usize,
    args: &[NativeValue],
) -> AbiResult<u64> {
    // TODO: Implement task spawning via scheduler
    // This requires access to the module and bytecode, which needs more plumbing
    Err("task_spawn not yet implemented".to_string())
}

/// Cancel a task by ID
pub fn task_cancel(ctx: &NativeContext, task_id: u64) -> AbiResult<()> {
    let tid = TaskId::from_u64(task_id);
    // TODO: Implement task cancellation
    // This requires TaskRegistry access
    Err("task_cancel not yet implemented".to_string())
}

/// Check if a task is done
pub fn task_is_done(ctx: &NativeContext, task_id: u64) -> AbiResult<bool> {
    let tid = TaskId::from_u64(task_id);
    // TODO: Implement task status check
    Err("task_is_done not yet implemented".to_string())
}

// ============================================================================
// Error Result
// ============================================================================

/// Result type for ABI calls
pub type AbiResult<T> = Result<T, String>;
