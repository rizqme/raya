//! Data marshalling for cross-context value passing
//!
//! Marshalling provides safe, controlled data transfer between VmContexts.
//! Values are deep-copied across context boundaries to maintain heap isolation.

use crate::value::Value;
use crate::vm::VmContext;
use std::collections::HashMap;
use std::fmt;
use thiserror::Error;

/// Errors that can occur during marshalling
#[derive(Debug, Error)]
pub enum MarshalError {
    /// Value cannot be marshalled (e.g., contains pointers to other context)
    #[error("Value cannot be marshalled: {0}")]
    Unmarshallable(String),

    /// Heap allocation failed during unmarshalling
    #[error("Heap allocation failed: {0}")]
    AllocationFailed(String),

    /// Foreign handle not found
    #[error("Foreign handle not found: {0}")]
    ForeignHandleNotFound(u64),

    /// Maximum marshalling depth exceeded (prevents infinite recursion)
    #[error("Maximum marshalling depth exceeded")]
    MaxDepthExceeded,
}

/// Marshalled value that can be safely transferred between contexts
///
/// MarshalledValue represents a value that has been serialized in a way
/// that preserves its semantic meaning while being independent of any
/// specific VmContext's heap.
#[derive(Debug, Clone, PartialEq)]
pub enum MarshalledValue {
    /// Null value
    Null,

    /// Boolean value
    Bool(bool),

    /// 32-bit signed integer
    I32(i32),

    /// 64-bit floating point number
    F64(f64),

    /// String value (deep copy)
    String(String),

    /// Array of marshalled values (deep copy, recursive)
    Array(Vec<MarshalledValue>),

    /// Object/record with key-value pairs (deep copy, recursive)
    Object(HashMap<String, MarshalledValue>),

    /// Foreign handle (opaque reference to object in another context)
    ///
    /// Foreign handles are used when an object cannot be marshalled
    /// (e.g., it contains functions, native resources, etc.).
    /// The handle is valid only in the source context.
    ForeignHandle(u64),
}

impl MarshalledValue {
    /// Check if this value is null
    pub fn is_null(&self) -> bool {
        matches!(self, MarshalledValue::Null)
    }

    /// Get the type name as a string
    pub fn type_name(&self) -> &'static str {
        match self {
            MarshalledValue::Null => "null",
            MarshalledValue::Bool(_) => "boolean",
            MarshalledValue::I32(_) => "i32",
            MarshalledValue::F64(_) => "f64",
            MarshalledValue::String(_) => "string",
            MarshalledValue::Array(_) => "array",
            MarshalledValue::Object(_) => "object",
            MarshalledValue::ForeignHandle(_) => "foreign",
        }
    }
}

impl fmt::Display for MarshalledValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MarshalledValue::Null => write!(f, "null"),
            MarshalledValue::Bool(b) => write!(f, "{}", b),
            MarshalledValue::I32(i) => write!(f, "{}", i),
            MarshalledValue::F64(fl) => write!(f, "{}", fl),
            MarshalledValue::String(s) => write!(f, "\"{}\"", s),
            MarshalledValue::Array(arr) => {
                write!(f, "[")?;
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            MarshalledValue::Object(obj) => {
                write!(f, "{{")?;
                for (i, (k, v)) in obj.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
            MarshalledValue::ForeignHandle(id) => write!(f, "Foreign({})", id),
        }
    }
}

/// Marshal a value from a source context for transfer
///
/// This performs a deep copy of the value, converting heap-allocated
/// objects into their marshalled representation.
///
/// # Arguments
/// * `value` - The value to marshal
/// * `_from_ctx` - The source context (for future use with foreign handles)
///
/// # Returns
/// * `Ok(MarshalledValue)` - The marshalled value
/// * `Err(MarshalError)` - If the value cannot be marshalled
pub fn marshal(value: &Value, _from_ctx: &VmContext) -> Result<MarshalledValue, MarshalError> {
    marshal_recursive(value, _from_ctx, 0)
}

/// Marshal a value recursively with depth tracking
fn marshal_recursive(
    value: &Value,
    _from_ctx: &VmContext,
    depth: usize,
) -> Result<MarshalledValue, MarshalError> {
    // Prevent infinite recursion
    const MAX_DEPTH: usize = 64;
    if depth >= MAX_DEPTH {
        return Err(MarshalError::MaxDepthExceeded);
    }

    // For now, we only support primitives
    // TODO: Add support for heap-allocated values (strings, arrays, objects)
    if value.is_null() {
        Ok(MarshalledValue::Null)
    } else if let Some(b) = value.as_bool() {
        Ok(MarshalledValue::Bool(b))
    } else if let Some(i) = value.as_i32() {
        Ok(MarshalledValue::I32(i))
    } else if let Some(f) = value.as_f64() {
        Ok(MarshalledValue::F64(f))
    } else {
        // TODO: Handle strings, arrays, objects
        // For now, treat everything else as unmarshallable
        Err(MarshalError::Unmarshallable(format!(
            "Complex types not yet supported: {:?}",
            value
        )))
    }
}

/// Unmarshal a value into a target context
///
/// This creates a new value in the target context's heap, performing
/// deep copy of all data.
///
/// # Arguments
/// * `marshalled` - The marshalled value to unmarshal
/// * `_to_ctx` - The target context (for heap allocation)
///
/// # Returns
/// * `Ok(Value)` - The unmarshalled value in the target context
/// * `Err(MarshalError)` - If unmarshalling fails
pub fn unmarshal(
    marshalled: MarshalledValue,
    _to_ctx: &mut VmContext,
) -> Result<Value, MarshalError> {
    unmarshal_recursive(marshalled, _to_ctx, 0)
}

/// Unmarshal a value recursively with depth tracking
fn unmarshal_recursive(
    marshalled: MarshalledValue,
    _to_ctx: &mut VmContext,
    depth: usize,
) -> Result<Value, MarshalError> {
    // Prevent infinite recursion
    const MAX_DEPTH: usize = 64;
    if depth >= MAX_DEPTH {
        return Err(MarshalError::MaxDepthExceeded);
    }

    match marshalled {
        MarshalledValue::Null => Ok(Value::null()),
        MarshalledValue::Bool(b) => Ok(Value::bool(b)),
        MarshalledValue::I32(i) => Ok(Value::i32(i)),
        MarshalledValue::F64(f) => Ok(Value::f64(f)),
        MarshalledValue::String(_s) => {
            // TODO: Allocate string in target context's heap
            Err(MarshalError::AllocationFailed(
                "String marshalling not yet implemented".to_string(),
            ))
        }
        MarshalledValue::Array(_arr) => {
            // TODO: Recursively unmarshal array elements
            Err(MarshalError::AllocationFailed(
                "Array marshalling not yet implemented".to_string(),
            ))
        }
        MarshalledValue::Object(_obj) => {
            // TODO: Recursively unmarshal object properties
            Err(MarshalError::AllocationFailed(
                "Object marshalling not yet implemented".to_string(),
            ))
        }
        MarshalledValue::ForeignHandle(_id) => {
            // TODO: Look up foreign handle in handle table
            Err(MarshalError::AllocationFailed(
                "Foreign handle resolution not yet implemented".to_string(),
            ))
        }
    }
}

/// Foreign handle manager for cross-context object references
///
/// Maintains a mapping between foreign handles (u64 IDs) and actual
/// object pointers in the source context.
#[derive(Debug)]
pub struct ForeignHandleManager {
    next_id: u64,
    handles: HashMap<u64, Value>,
}

impl ForeignHandleManager {
    /// Create a new foreign handle manager
    pub fn new() -> Self {
        Self {
            next_id: 1,
            handles: HashMap::new(),
        }
    }

    /// Create a foreign handle for a value
    pub fn create_handle(&mut self, value: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.handles.insert(id, value);
        id
    }

    /// Get a value by foreign handle
    pub fn get_value(&self, handle: u64) -> Result<Value, MarshalError> {
        self.handles
            .get(&handle)
            .copied()
            .ok_or(MarshalError::ForeignHandleNotFound(handle))
    }

    /// Release a foreign handle
    pub fn release_handle(&mut self, handle: u64) -> bool {
        self.handles.remove(&handle).is_some()
    }

    /// Get the number of active handles
    pub fn handle_count(&self) -> usize {
        self.handles.len()
    }
}

impl Default for ForeignHandleManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_context() -> VmContext {
        VmContext::new()
    }

    #[test]
    fn test_marshal_primitives() {
        let ctx = create_test_context();

        // Null
        let marshalled = marshal(&Value::null(), &ctx).unwrap();
        assert_eq!(marshalled, MarshalledValue::Null);

        // Bool
        let marshalled = marshal(&Value::bool(true), &ctx).unwrap();
        assert_eq!(marshalled, MarshalledValue::Bool(true));

        // I32
        let marshalled = marshal(&Value::i32(42), &ctx).unwrap();
        assert_eq!(marshalled, MarshalledValue::I32(42));

        // F64
        let marshalled = marshal(&Value::f64(3.14), &ctx).unwrap();
        assert_eq!(marshalled, MarshalledValue::F64(3.14));
    }

    #[test]
    fn test_unmarshal_primitives() {
        let mut ctx = create_test_context();

        // Null
        let value = unmarshal(MarshalledValue::Null, &mut ctx).unwrap();
        assert_eq!(value, Value::null());

        // Bool
        let value = unmarshal(MarshalledValue::Bool(false), &mut ctx).unwrap();
        assert_eq!(value, Value::bool(false));

        // I32
        let value = unmarshal(MarshalledValue::I32(100), &mut ctx).unwrap();
        assert_eq!(value, Value::i32(100));

        // F64
        let value = unmarshal(MarshalledValue::F64(2.71), &mut ctx).unwrap();
        assert_eq!(value, Value::f64(2.71));
    }

    #[test]
    fn test_marshal_unmarshal_roundtrip() {
        let ctx1 = create_test_context();
        let mut ctx2 = create_test_context();

        let original = Value::i32(42);
        let marshalled = marshal(&original, &ctx1).unwrap();
        let result = unmarshal(marshalled, &mut ctx2).unwrap();

        assert_eq!(original, result);
    }

    #[test]
    fn test_marshalled_value_type_name() {
        assert_eq!(MarshalledValue::Null.type_name(), "null");
        assert_eq!(MarshalledValue::Bool(true).type_name(), "boolean");
        assert_eq!(MarshalledValue::I32(42).type_name(), "i32");
        assert_eq!(MarshalledValue::F64(3.14).type_name(), "f64");
        assert_eq!(
            MarshalledValue::String("test".to_string()).type_name(),
            "string"
        );
        assert_eq!(MarshalledValue::Array(vec![]).type_name(), "array");
        assert_eq!(MarshalledValue::Object(HashMap::new()).type_name(), "object");
        assert_eq!(MarshalledValue::ForeignHandle(1).type_name(), "foreign");
    }

    #[test]
    fn test_marshalled_value_display() {
        assert_eq!(MarshalledValue::Null.to_string(), "null");
        assert_eq!(MarshalledValue::Bool(true).to_string(), "true");
        assert_eq!(MarshalledValue::I32(42).to_string(), "42");
        assert_eq!(MarshalledValue::F64(3.14).to_string(), "3.14");
        assert_eq!(
            MarshalledValue::String("hello".to_string()).to_string(),
            "\"hello\""
        );
        assert_eq!(MarshalledValue::ForeignHandle(123).to_string(), "Foreign(123)");
    }

    #[test]
    fn test_foreign_handle_manager() {
        let mut manager = ForeignHandleManager::new();

        assert_eq!(manager.handle_count(), 0);

        // Create handles
        let handle1 = manager.create_handle(Value::i32(42));
        let handle2 = manager.create_handle(Value::bool(true));

        assert_eq!(manager.handle_count(), 2);
        assert_ne!(handle1, handle2);

        // Retrieve values
        let value1 = manager.get_value(handle1).unwrap();
        assert_eq!(value1, Value::i32(42));

        let value2 = manager.get_value(handle2).unwrap();
        assert_eq!(value2, Value::bool(true));

        // Release handle
        assert!(manager.release_handle(handle1));
        assert_eq!(manager.handle_count(), 1);

        // Try to get released handle
        assert!(manager.get_value(handle1).is_err());

        // Release non-existent handle
        assert!(!manager.release_handle(999));
    }

    #[test]
    fn test_marshal_max_depth() {
        let ctx = create_test_context();

        // This would cause infinite recursion in a real scenario
        // For now, we just test that depth limiting works
        let result = marshal_recursive(&Value::i32(42), &ctx, 64);
        assert!(result.is_err());
        matches!(result.unwrap_err(), MarshalError::MaxDepthExceeded);
    }
}
