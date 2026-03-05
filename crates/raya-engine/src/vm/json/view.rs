//! Lightweight, stack-only value type dispatch for runtime values.
//!
//! `JSView` is a pure Rust enum — **never heap-allocated**.
//! Produced by `js_classify(val)`, which is the single entry point for
//! all GC-header TypeId comparisons on the JSON/dynamic value dispatch path.
//!
//! All consumers of `JSView` should use `match js_classify(val) { ... }`.

use std::any::TypeId;

use crate::vm::gc::GcHeader;
use crate::vm::object::{Array, DynObject, Object, RayaString};
use crate::vm::value::Value;

/// Stack-only view of a `Value`'s type.
///
/// All pointer variants hold raw pointers that are valid only as long as the
/// `Value` they were derived from remains GC-reachable (not freed).
pub enum JSView {
    /// `null`
    Null,
    /// Boolean
    Bool(bool),
    /// 32-bit integer
    Int(i32),
    /// 64-bit float
    Number(f64),
    /// Heap-allocated string
    Str(*const RayaString),
    /// Heap-allocated array
    Arr(*const Array),
    /// Fixed-field object instance with optional nominal identity.
    Struct {
        ptr: *const Object,
        nominal_type_id: Option<usize>,
    },
    /// Dynamic hashmap object (JSON.parse result, `JsonObject`-typed).
    Dyn(*const DynObject),
    /// Any other heap type (Closure, BoundMethod, RefCell, Channel, …).
    Other,
}

/// Classify a `Value` into a `JSView`.
///
/// This is the **only** place in the codebase that performs GC-header
/// TypeId comparisons for the JSON/dynamic value dispatch path.
/// All consumers should use `match js_classify(val) { ... }`.
///
/// # Safety
///
/// The raw pointers in the returned `JSView` variants are valid for the
/// lifetime of the `val` they were derived from.  The caller must ensure
/// the value remains GC-reachable while the view is live.
pub fn js_classify(val: Value) -> JSView {
    if val.is_null() {
        return JSView::Null;
    }
    if let Some(b) = val.as_bool() {
        return JSView::Bool(b);
    }
    if let Some(i) = val.as_i32() {
        return JSView::Int(i);
    }
    if let Some(n) = val.as_f64() {
        return JSView::Number(n);
    }
    if !val.is_ptr() {
        return JSView::Other;
    }

    let ptr = match unsafe { val.as_ptr::<u8>() } {
        Some(p) => p.as_ptr(),
        None => return JSView::Other,
    };

    // The GcHeader is stored immediately before the object data.
    let type_id = unsafe {
        let header = &*ptr.cast::<GcHeader>().sub(1);
        header.type_id()
    };

    if type_id == TypeId::of::<RayaString>() {
        JSView::Str(ptr as *const RayaString)
    } else if type_id == TypeId::of::<Array>() {
        JSView::Arr(ptr as *const Array)
    } else if type_id == TypeId::of::<Object>() {
        let obj_ptr = ptr as *const Object;
        let nominal_type_id = unsafe { (*obj_ptr).nominal_class_id() };
        JSView::Struct {
            ptr: obj_ptr,
            nominal_type_id,
        }
    } else if type_id == TypeId::of::<DynObject>() {
        JSView::Dyn(ptr as *const DynObject)
    } else {
        JSView::Other
    }
}
