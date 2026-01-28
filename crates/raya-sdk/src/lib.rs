//! Raya SDK - Lightweight SDK for writing native modules
//!
//! This crate provides the minimal types and traits needed to write Raya native
//! modules without depending on the full raya-engine.
//!
//! # Example
//!
//! ```ignore
//! use raya_sdk::NativeModule;
//! use raya_native::{function, module};
//!
//! #[function]
//! fn add(a: i32, b: i32) -> i32 {
//!     a + b
//! }
//!
//! #[module]
//! pub fn init() -> NativeModule {
//!     let mut module = NativeModule::new("math", "1.0.0");
//!     module.register_function("add", add_ffi);
//!     module
//! }
//! ```

#![warn(missing_docs)]

use std::collections::HashMap;

// ============================================================================
// Native Value
// ============================================================================

/// Native value handle for FFI functions.
///
/// This is a lightweight, opaque value that can be passed across FFI boundaries.
/// The actual data is stored as a tagged union internally.
///
/// # Thread Safety
///
/// NativeValue is Send + Sync. Values are owned exclusively by each handle.
///
/// # Memory Management
///
/// - Primitive values (null, bool, i32, i64, f64) are stored inline
/// - Heap values (strings, objects) are reference-counted
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NativeValue {
    tag: u8,
    data: u64,
}

// Value type tags
const TAG_NULL: u8 = 0;
const TAG_BOOL: u8 = 1;
const TAG_I32: u8 = 2;
const TAG_I64: u8 = 3;
const TAG_F64: u8 = 4;
const TAG_PTR: u8 = 5; // Opaque pointer to heap value

unsafe impl Send for NativeValue {}
unsafe impl Sync for NativeValue {}

impl NativeValue {
    /// Create a null value
    pub fn null() -> Self {
        NativeValue {
            tag: TAG_NULL,
            data: 0,
        }
    }

    /// Create a boolean value
    pub fn bool(b: bool) -> Self {
        NativeValue {
            tag: TAG_BOOL,
            data: b as u64,
        }
    }

    /// Create a 32-bit integer value
    pub fn i32(i: i32) -> Self {
        NativeValue {
            tag: TAG_I32,
            data: i as u64,
        }
    }

    /// Create a 64-bit integer value
    pub fn i64(i: i64) -> Self {
        NativeValue {
            tag: TAG_I64,
            data: i as u64,
        }
    }

    /// Create a 64-bit float value
    pub fn f64(f: f64) -> Self {
        NativeValue {
            tag: TAG_F64,
            data: f.to_bits(),
        }
    }

    /// Create from an opaque pointer (used by VM)
    ///
    /// # Safety
    /// The pointer must be valid and properly managed by the VM.
    pub unsafe fn from_ptr(ptr: *mut ()) -> Self {
        NativeValue {
            tag: TAG_PTR,
            data: ptr as u64,
        }
    }

    /// Create an error value (returns null for now)
    ///
    /// TODO: Implement proper error values
    pub fn error(_msg: String) -> Self {
        NativeValue::null()
    }

    /// Check if this is a null value
    pub fn is_null(&self) -> bool {
        self.tag == TAG_NULL
    }

    /// Get as boolean if this is a bool
    pub fn as_bool(&self) -> Option<bool> {
        if self.tag == TAG_BOOL {
            Some(self.data != 0)
        } else {
            None
        }
    }

    /// Get as i32 if this is an i32
    pub fn as_i32(&self) -> Option<i32> {
        if self.tag == TAG_I32 {
            Some(self.data as i32)
        } else {
            None
        }
    }

    /// Get as i64 if this is an i64
    pub fn as_i64(&self) -> Option<i64> {
        if self.tag == TAG_I64 {
            Some(self.data as i64)
        } else {
            None
        }
    }

    /// Get as f64 if this is an f64
    pub fn as_f64(&self) -> Option<f64> {
        if self.tag == TAG_F64 {
            Some(f64::from_bits(self.data))
        } else {
            None
        }
    }

    /// Get as opaque pointer if this is a pointer value
    ///
    /// # Safety
    /// The returned pointer is only valid while the VM is running and
    /// the value hasn't been garbage collected.
    pub unsafe fn as_ptr(&self) -> Option<*mut ()> {
        if self.tag == TAG_PTR {
            Some(self.data as *mut ())
        } else {
            None
        }
    }

    /// Get the type tag
    pub fn tag(&self) -> u8 {
        self.tag
    }
}

impl Default for NativeValue {
    fn default() -> Self {
        Self::null()
    }
}

impl std::fmt::Debug for NativeValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.tag {
            TAG_NULL => write!(f, "NativeValue::Null"),
            TAG_BOOL => write!(f, "NativeValue::Bool({})", self.data != 0),
            TAG_I32 => write!(f, "NativeValue::I32({})", self.data as i32),
            TAG_I64 => write!(f, "NativeValue::I64({})", self.data as i64),
            TAG_F64 => write!(f, "NativeValue::F64({})", f64::from_bits(self.data)),
            TAG_PTR => write!(f, "NativeValue::Ptr({:#x})", self.data),
            _ => write!(f, "NativeValue::Unknown(tag={}, data={})", self.tag, self.data),
        }
    }
}

// ============================================================================
// Native Function
// ============================================================================

/// Native function signature.
///
/// Takes array of NativeValue arguments and returns a NativeValue result.
/// Function is responsible for:
/// - Validating argument count and types
/// - Converting arguments from NativeValue to Rust types
/// - Executing the logic
/// - Converting result from Rust type to NativeValue
/// - Catching panics and returning errors
pub type NativeFn = extern "C" fn(args: *const NativeValue, arg_count: usize) -> NativeValue;

// ============================================================================
// Native Module
// ============================================================================

/// Native module definition.
///
/// Contains module metadata and registered functions.
///
/// # Thread Safety
///
/// NativeModule is Send + Sync after registration (immutable use).
#[derive(Debug)]
pub struct NativeModule {
    name: String,
    version: String,
    functions: HashMap<String, NativeFn>,
}

unsafe impl Send for NativeModule {}
unsafe impl Sync for NativeModule {}

impl NativeModule {
    /// Create a new native module.
    ///
    /// # Arguments
    /// * `name` - Module name (e.g., "json", "fs", "std:math")
    /// * `version` - Semantic version (e.g., "1.0.0")
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        NativeModule {
            name: name.into(),
            version: version.into(),
            functions: HashMap::new(),
        }
    }

    /// Register a function with the module.
    ///
    /// # Arguments
    /// * `name` - Function name as it will appear in Raya code
    /// * `func` - Function pointer (typically generated by `#[function]` macro)
    pub fn register_function(&mut self, name: impl Into<String>, func: NativeFn) {
        self.functions.insert(name.into(), func);
    }

    /// Get module name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get module version
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Get function by name
    pub fn get_function(&self, name: &str) -> Option<NativeFn> {
        self.functions.get(name).copied()
    }

    /// Get all function names
    pub fn function_names(&self) -> Vec<&str> {
        self.functions.keys().map(|s| s.as_str()).collect()
    }

    /// Get all function names as owned strings
    pub fn functions(&self) -> Vec<String> {
        self.functions.keys().cloned().collect()
    }

    /// Get number of registered functions
    pub fn function_count(&self) -> usize {
        self.functions.len()
    }
}

// ============================================================================
// Error Type
// ============================================================================

/// Native module error types
#[derive(Debug, Clone, thiserror::Error)]
pub enum NativeError {
    /// Type mismatch during conversion
    #[error("Type mismatch: expected {expected}, got {got}")]
    TypeMismatch {
        /// Expected type name
        expected: String,
        /// Actual type name
        got: String,
    },

    /// Invalid argument
    #[error("Argument error: {0}")]
    ArgumentError(String),

    /// Function panicked
    #[error("Function panicked: {0}")]
    Panic(String),

    /// Module-level error
    #[error("Module error: {0}")]
    ModuleError(String),
}

// ============================================================================
// Value Conversion Traits
// ============================================================================

/// Convert from NativeValue to Rust type.
///
/// Implement this trait to allow your type to be received as a function argument.
pub trait FromRaya: Sized {
    /// Convert from NativeValue, returning an error if the type doesn't match.
    fn from_raya(value: NativeValue) -> Result<Self, NativeError>;
}

/// Convert from Rust type to NativeValue.
///
/// Implement this trait to allow your type to be returned from a function.
pub trait ToRaya {
    /// Convert to NativeValue.
    fn to_raya(self) -> NativeValue;
}

// ============================================================================
// Primitive Type Implementations
// ============================================================================

impl FromRaya for i32 {
    fn from_raya(value: NativeValue) -> Result<Self, NativeError> {
        value.as_i32().ok_or_else(|| NativeError::TypeMismatch {
            expected: "i32".to_string(),
            got: format!("tag {}", value.tag()),
        })
    }
}

impl ToRaya for i32 {
    fn to_raya(self) -> NativeValue {
        NativeValue::i32(self)
    }
}

impl FromRaya for i64 {
    fn from_raya(value: NativeValue) -> Result<Self, NativeError> {
        value.as_i64().ok_or_else(|| NativeError::TypeMismatch {
            expected: "i64".to_string(),
            got: format!("tag {}", value.tag()),
        })
    }
}

impl ToRaya for i64 {
    fn to_raya(self) -> NativeValue {
        NativeValue::i64(self)
    }
}

impl FromRaya for f64 {
    fn from_raya(value: NativeValue) -> Result<Self, NativeError> {
        value.as_f64().ok_or_else(|| NativeError::TypeMismatch {
            expected: "f64".to_string(),
            got: format!("tag {}", value.tag()),
        })
    }
}

impl ToRaya for f64 {
    fn to_raya(self) -> NativeValue {
        NativeValue::f64(self)
    }
}

impl FromRaya for bool {
    fn from_raya(value: NativeValue) -> Result<Self, NativeError> {
        value.as_bool().ok_or_else(|| NativeError::TypeMismatch {
            expected: "bool".to_string(),
            got: format!("tag {}", value.tag()),
        })
    }
}

impl ToRaya for bool {
    fn to_raya(self) -> NativeValue {
        NativeValue::bool(self)
    }
}

// Unit type (for functions that return void)
impl ToRaya for () {
    fn to_raya(self) -> NativeValue {
        NativeValue::null()
    }
}

// Result type (for fallible functions)
impl<T: ToRaya, E: ToString> ToRaya for Result<T, E> {
    fn to_raya(self) -> NativeValue {
        match self {
            Ok(value) => value.to_raya(),
            Err(error) => NativeValue::error(error.to_string()),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_native_value_primitives() {
        // Null
        let null = NativeValue::null();
        assert!(null.is_null());

        // Bool
        let t = NativeValue::bool(true);
        let f = NativeValue::bool(false);
        assert_eq!(t.as_bool(), Some(true));
        assert_eq!(f.as_bool(), Some(false));

        // i32
        let i = NativeValue::i32(42);
        assert_eq!(i.as_i32(), Some(42));

        // i64
        let i = NativeValue::i64(9999999999i64);
        assert_eq!(i.as_i64(), Some(9999999999i64));

        // f64
        let f = NativeValue::f64(3.14159);
        assert!((f.as_f64().unwrap() - 3.14159).abs() < 1e-10);
    }

    #[test]
    fn test_native_module() {
        extern "C" fn dummy(_args: *const NativeValue, _count: usize) -> NativeValue {
            NativeValue::null()
        }

        let mut module = NativeModule::new("test", "1.0.0");
        module.register_function("foo", dummy);
        module.register_function("bar", dummy);

        assert_eq!(module.name(), "test");
        assert_eq!(module.version(), "1.0.0");
        assert_eq!(module.function_count(), 2);
        assert!(module.get_function("foo").is_some());
        assert!(module.get_function("baz").is_none());
    }

    #[test]
    fn test_from_raya_traits() {
        let v = NativeValue::i32(42);
        assert_eq!(i32::from_raya(v).unwrap(), 42);

        let v = NativeValue::bool(true);
        assert_eq!(bool::from_raya(v).unwrap(), true);

        let v = NativeValue::f64(2.5);
        assert!((f64::from_raya(v).unwrap() - 2.5).abs() < 1e-10);
    }

    #[test]
    fn test_to_raya_traits() {
        assert_eq!(42i32.to_raya().as_i32(), Some(42));
        assert_eq!(true.to_raya().as_bool(), Some(true));
        assert!(().to_raya().is_null());
    }
}
