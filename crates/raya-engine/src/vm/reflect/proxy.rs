//! Proxy Object Support
//!
//! This module provides helper functions for working with proxy objects,
//! including detection, unwrapping, and trap dispatch.
//!
//! ## Proxy Traps
//!
//! Proxy traps allow intercepting operations on objects:
//! - `get(target, property)` - intercept property read
//! - `set(target, property, value)` - intercept property write
//! - `has(target, property)` - intercept `in` operator
//! - `invoke(target, method, args)` - intercept method call
//!
//! ## Usage in Interpreter
//!
//! When accessing fields or calling methods on an object, the interpreter
//! should first check if the object is a proxy:
//!
//! ```rust,ignore
//! if let Some(unwrapped) = try_unwrap_proxy(value) {
//!     // Use unwrapped.target for the actual object
//!     // Optionally call trap via unwrapped.handler
//! }
//! ```

use crate::vm::gc::GcHeader;
use crate::vm::object::{Closure, Object, Proxy, RayaString};
use crate::vm::value::Value;
use std::any::TypeId;

/// Result of unwrapping a proxy
#[derive(Debug, Clone, Copy)]
pub struct UnwrappedProxy {
    /// The underlying target object
    pub target: Value,
    /// The handler object with trap methods
    pub handler: Value,
    /// The proxy's unique ID
    pub proxy_id: u64,
}

/// Check the GcHeader type_id to see if this pointer points to a Proxy
///
/// # Safety
/// The pointer must be a valid GC-allocated object with a GcHeader preceding it.
unsafe fn is_proxy_type(ptr: *const u8) -> bool {
    let header_ptr = ptr.sub(std::mem::size_of::<GcHeader>()) as *const GcHeader;
    let header = &*header_ptr;
    header.type_id() == TypeId::of::<Proxy>()
}

/// Check if a value is a proxy and return the unwrapped contents
///
/// Returns `Some(UnwrappedProxy)` if the value is a proxy, `None` otherwise.
pub fn try_unwrap_proxy(value: Value) -> Option<UnwrappedProxy> {
    if !value.is_ptr() || value.is_null() {
        return None;
    }

    // Get raw pointer and check the GcHeader type_id
    let raw_ptr = unsafe { value.as_ptr::<u8>()? };
    if !unsafe { is_proxy_type(raw_ptr.as_ptr()) } {
        return None;
    }

    // Now it's safe to cast to Proxy
    let proxy = unsafe { &*(raw_ptr.as_ptr() as *const Proxy) };

    Some(UnwrappedProxy {
        target: proxy.target,
        handler: proxy.handler,
        proxy_id: proxy.proxy_id,
    })
}

/// Check if a value is a proxy
pub fn is_proxy(value: Value) -> bool {
    if !value.is_ptr() || value.is_null() {
        return false;
    }
    if let Some(raw_ptr) = unsafe { value.as_ptr::<u8>() } {
        unsafe { is_proxy_type(raw_ptr.as_ptr()) }
    } else {
        false
    }
}

/// Get the target from a proxy, or return the original value if not a proxy
///
/// This is useful for "transparent" proxy behavior where we want to
/// pass through to the underlying object.
pub fn unwrap_proxy_target(value: Value) -> Value {
    if let Some(unwrapped) = try_unwrap_proxy(value) {
        unwrapped.target
    } else {
        value
    }
}

/// Recursively unwrap nested proxies to get the innermost target
pub fn unwrap_proxy_deep(value: Value) -> Value {
    let mut current = value;
    while let Some(unwrapped) = try_unwrap_proxy(current) {
        current = unwrapped.target;
    }
    current
}

/// Information about a proxy trap method on the handler
#[derive(Debug, Clone, Copy)]
pub enum TrapMethod {
    /// Handler has this trap as a closure
    Closure(Value),
    /// Handler has this trap as a bound method (function_id)
    Method(usize),
    /// Handler does not have this trap defined
    NotDefined,
}

/// Get the trap method from a handler object
///
/// The handler should be an Object with optional fields:
/// - "get": closure for property read
/// - "set": closure for property write
/// - "has": closure for property existence check
/// - "invoke": closure for method call
///
/// This function looks up the trap by name in the handler's fields.
/// Since Raya uses positional field access at runtime, the handler
/// must be compiled with the expected field layout.
///
/// Field layout for ProxyHandler:
/// - 0: get (optional)
/// - 1: set (optional)
/// - 2: has (optional)
/// - 3: invoke (optional)
pub fn get_trap_method(handler: Value, trap_name: &str) -> TrapMethod {
    if !handler.is_ptr() || handler.is_null() {
        return TrapMethod::NotDefined;
    }

    // Try to get handler as an Object
    let obj_ptr = unsafe { handler.as_ptr::<Object>() };
    if obj_ptr.is_none() {
        return TrapMethod::NotDefined;
    }
    let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };

    // Map trap name to field index (based on ProxyHandler interface layout)
    let field_index = match trap_name {
        "get" => 0,
        "set" => 1,
        "has" => 2,
        "invoke" => 3,
        _ => return TrapMethod::NotDefined,
    };

    // Get the field value
    let field_value = match obj.get_field(field_index) {
        Some(v) if !v.is_null() => v,
        _ => return TrapMethod::NotDefined,
    };

    // Check if it's a closure
    if field_value.is_ptr() {
        if let Some(_closure_ptr) = unsafe { field_value.as_ptr::<Closure>() } {
            return TrapMethod::Closure(field_value);
        }
    }

    TrapMethod::NotDefined
}

/// Result of calling a proxy trap
#[derive(Debug)]
pub enum TrapResult {
    /// Trap was called and returned a value
    Value(Value),
    /// Trap was not defined, fall through to target
    Passthrough,
    /// Trap exists but cannot be called (needs interpreter context)
    NeedsInterpreter {
        trap_closure: Value,
        target: Value,
        args: Vec<Value>,
    },
}

/// Prepare a get trap call
///
/// This function checks if the handler has a `get` trap and prepares the call.
/// The actual invocation must be done by the interpreter since it requires
/// setting up a call frame.
///
/// Arguments for the get trap: (target, propertyName)
pub fn prepare_get_trap(
    handler: Value,
    target: Value,
    property_name: &str,
) -> TrapResult {
    match get_trap_method(handler, "get") {
        TrapMethod::Closure(closure) => {
            // Create property name as a Value (would need GC allocation)
            // For now, signal that we need the interpreter
            TrapResult::NeedsInterpreter {
                trap_closure: closure,
                target,
                args: vec![], // Property name would be added here
            }
        }
        TrapMethod::Method(_) => TrapResult::Passthrough, // Not supported yet
        TrapMethod::NotDefined => TrapResult::Passthrough,
    }
}

/// Prepare a set trap call
///
/// This function checks if the handler has a `set` trap and prepares the call.
/// The actual invocation must be done by the interpreter.
///
/// Arguments for the set trap: (target, propertyName, value)
pub fn prepare_set_trap(
    handler: Value,
    target: Value,
    _property_name: &str,
    value: Value,
) -> TrapResult {
    match get_trap_method(handler, "set") {
        TrapMethod::Closure(closure) => {
            TrapResult::NeedsInterpreter {
                trap_closure: closure,
                target,
                args: vec![value], // Would also include property name
            }
        }
        TrapMethod::Method(_) => TrapResult::Passthrough,
        TrapMethod::NotDefined => TrapResult::Passthrough,
    }
}

/// Prepare a has trap call
///
/// This function checks if the handler has a `has` trap and prepares the call.
///
/// Arguments for the has trap: (target, propertyName)
pub fn prepare_has_trap(
    handler: Value,
    target: Value,
    _property_name: &str,
) -> TrapResult {
    match get_trap_method(handler, "has") {
        TrapMethod::Closure(closure) => {
            TrapResult::NeedsInterpreter {
                trap_closure: closure,
                target,
                args: vec![], // Would include property name
            }
        }
        TrapMethod::Method(_) => TrapResult::Passthrough,
        TrapMethod::NotDefined => TrapResult::Passthrough,
    }
}

/// Prepare an invoke trap call
///
/// This function checks if the handler has an `invoke` trap and prepares the call.
///
/// Arguments for the invoke trap: (target, methodName, argsArray)
pub fn prepare_invoke_trap(
    handler: Value,
    target: Value,
    _method_name: &str,
    args: Vec<Value>,
) -> TrapResult {
    match get_trap_method(handler, "invoke") {
        TrapMethod::Closure(closure) => {
            TrapResult::NeedsInterpreter {
                trap_closure: closure,
                target,
                args, // Would also include method name
            }
        }
        TrapMethod::Method(_) => TrapResult::Passthrough,
        TrapMethod::NotDefined => TrapResult::Passthrough,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::gc::GarbageCollector;

    #[test]
    fn test_try_unwrap_proxy() {
        let mut gc = GarbageCollector::default();

        let target = gc.allocate(Object::new(1, 2));
        let handler = gc.allocate(Object::new(0, 4));

        let target_val = unsafe {
            Value::from_ptr(std::ptr::NonNull::new(target.as_ptr() as *mut Object).unwrap())
        };
        let handler_val = unsafe {
            Value::from_ptr(std::ptr::NonNull::new(handler.as_ptr() as *mut Object).unwrap())
        };

        let proxy = Proxy::new(target_val, handler_val);
        let proxy_gc = gc.allocate(proxy);
        let proxy_val = unsafe {
            Value::from_ptr(std::ptr::NonNull::new(proxy_gc.as_ptr() as *mut Proxy).unwrap())
        };

        let unwrapped = try_unwrap_proxy(proxy_val).expect("Should unwrap proxy");
        assert_eq!(unwrapped.target.raw(), target_val.raw());
        assert_eq!(unwrapped.handler.raw(), handler_val.raw());
    }

    #[test]
    fn test_is_proxy() {
        let mut gc = GarbageCollector::default();

        let obj = gc.allocate(Object::new(1, 2));
        let obj_val = unsafe {
            Value::from_ptr(std::ptr::NonNull::new(obj.as_ptr() as *mut Object).unwrap())
        };

        // Regular object is not a proxy
        assert!(!is_proxy(obj_val));

        // Null is not a proxy
        assert!(!is_proxy(Value::null()));

        // Primitive is not a proxy
        assert!(!is_proxy(Value::i32(42)));

        // Actual proxy
        let handler = gc.allocate(Object::new(0, 4));
        let handler_val = unsafe {
            Value::from_ptr(std::ptr::NonNull::new(handler.as_ptr() as *mut Object).unwrap())
        };
        let proxy = Proxy::new(obj_val, handler_val);
        let proxy_gc = gc.allocate(proxy);
        let proxy_val = unsafe {
            Value::from_ptr(std::ptr::NonNull::new(proxy_gc.as_ptr() as *mut Proxy).unwrap())
        };
        assert!(is_proxy(proxy_val));
    }

    #[test]
    fn test_unwrap_proxy_target() {
        let mut gc = GarbageCollector::default();

        let target = gc.allocate(Object::new(42, 3));
        let handler = gc.allocate(Object::new(0, 4));

        let target_val = unsafe {
            Value::from_ptr(std::ptr::NonNull::new(target.as_ptr() as *mut Object).unwrap())
        };
        let handler_val = unsafe {
            Value::from_ptr(std::ptr::NonNull::new(handler.as_ptr() as *mut Object).unwrap())
        };

        let proxy = Proxy::new(target_val, handler_val);
        let proxy_gc = gc.allocate(proxy);
        let proxy_val = unsafe {
            Value::from_ptr(std::ptr::NonNull::new(proxy_gc.as_ptr() as *mut Proxy).unwrap())
        };

        // Unwrapping proxy returns target
        let unwrapped = unwrap_proxy_target(proxy_val);
        assert_eq!(unwrapped.raw(), target_val.raw());

        // Unwrapping non-proxy returns itself
        let unwrapped_obj = unwrap_proxy_target(target_val);
        assert_eq!(unwrapped_obj.raw(), target_val.raw());
    }

    #[test]
    fn test_get_trap_method_not_defined() {
        let mut gc = GarbageCollector::default();

        // Handler with no traps (all fields null)
        let handler = gc.allocate(Object::new(0, 4));
        let handler_val = unsafe {
            Value::from_ptr(std::ptr::NonNull::new(handler.as_ptr() as *mut Object).unwrap())
        };

        assert!(matches!(
            get_trap_method(handler_val, "get"),
            TrapMethod::NotDefined
        ));
        assert!(matches!(
            get_trap_method(handler_val, "set"),
            TrapMethod::NotDefined
        ));
        assert!(matches!(
            get_trap_method(handler_val, "has"),
            TrapMethod::NotDefined
        ));
        assert!(matches!(
            get_trap_method(handler_val, "invoke"),
            TrapMethod::NotDefined
        ));
    }
}
