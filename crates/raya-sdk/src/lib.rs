//! Raya SDK — Lightweight SDK for writing Raya native modules
//!
//! This crate provides all types and traits needed to write Raya native
//! modules **without depending on the engine**. It serves as a "C header"
//! — define your module against these types and the engine provides the
//! implementation at runtime.
//!
//! # Architecture
//!
//! - `NativeValue`: NaN-boxed u64 (same encoding as engine `Value` — zero-cost conversion)
//! - `NativeContext`: Trait abstracting VM operations (engine provides `EngineContext`)
//! - `NativeHandler`: Trait for stdlib-style ID-based dispatch
//! - Wrapper types: `NativeArray`, `NativeObject`, `NativeClass`, etc.
//!
//! # Example
//!
//! ```ignore
//! use raya_sdk::{NativeHandler, NativeContext, NativeValue, NativeCallResult};
//!
//! pub struct MyHandler;
//!
//! impl NativeHandler for MyHandler {
//!     fn call(&self, ctx: &dyn NativeContext, id: u16, args: &[NativeValue]) -> NativeCallResult {
//!         match id {
//!             0x8000 => {
//!                 let s = ctx.read_string(args[0]).unwrap();
//!                 NativeCallResult::Value(ctx.create_string(&s.to_uppercase()))
//!             }
//!             _ => NativeCallResult::Unhandled,
//!         }
//!     }
//! }
//! ```

#![warn(missing_docs)]

// Modules
mod value;
mod error;
mod context;
mod handler;
mod types;
mod convert;

// Re-export core types
pub use value::NativeValue;
pub use error::{NativeError, AbiResult};
pub use context::{NativeContext, ClassInfo};
pub use handler::{
    NativeHandler, NativeCallResult, NoopNativeHandler, NativeHandlerFn, NativeFunctionRegistry,
    IoRequest, IoCompletion,
};

// Re-export wrapper types
pub use types::{
    NativeArray,
    NativeObject,
    ObjectSchema,
    ObjectSchemaBuilder,
    NativeClass,
    NativeFunction,
    NativeMethod,
    NativeTask,
};

// Re-export conversion traits
pub use convert::{FromNativeObject, ToNativeObject};

// ============================================================================
// Third-party FFI types (kept in lib.rs for backward compatibility)
// ============================================================================

use std::collections::HashMap;

/// Native function signature for third-party FFI modules.
///
/// Takes array of NativeValue arguments and returns a NativeValue result.
pub type NativeFn = extern "C" fn(args: *const NativeValue, arg_count: usize) -> NativeValue;

/// Native module definition for third-party FFI modules.
///
/// Contains module metadata and registered functions.
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
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        NativeModule {
            name: name.into(),
            version: version.into(),
            functions: HashMap::new(),
        }
    }

    /// Register a function with the module.
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
// Value Conversion Traits (for third-party FFI)
// ============================================================================

/// Convert from NativeValue to Rust type.
pub trait FromRaya: Sized {
    /// Convert from NativeValue, returning an error if the type doesn't match.
    fn from_raya(value: NativeValue) -> Result<Self, NativeError>;
}

/// Convert from Rust type to NativeValue.
pub trait ToRaya {
    /// Convert to NativeValue.
    fn to_raya(self) -> NativeValue;
}

impl FromRaya for i32 {
    fn from_raya(value: NativeValue) -> Result<Self, NativeError> {
        value.as_i32().ok_or_else(|| NativeError::TypeMismatch {
            expected: "i32".to_string(),
            got: value.type_name().to_string(),
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
            got: value.type_name().to_string(),
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
            got: value.type_name().to_string(),
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
            got: value.type_name().to_string(),
        })
    }
}

impl ToRaya for bool {
    fn to_raya(self) -> NativeValue {
        NativeValue::bool(self)
    }
}

impl ToRaya for () {
    fn to_raya(self) -> NativeValue {
        NativeValue::null()
    }
}

impl<T: ToRaya, E: ToString> ToRaya for Result<T, E> {
    fn to_raya(self) -> NativeValue {
        match self {
            Ok(value) => value.to_raya(),
            // Error returns null (TODO: proper error propagation)
            Err(_) => NativeValue::null(),
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
        let null = NativeValue::null();
        assert!(null.is_null());

        let t = NativeValue::bool(true);
        let f = NativeValue::bool(false);
        assert_eq!(t.as_bool(), Some(true));
        assert_eq!(f.as_bool(), Some(false));

        let i = NativeValue::i32(42);
        assert_eq!(i.as_i32(), Some(42));

        let neg = NativeValue::i32(-100);
        assert_eq!(neg.as_i32(), Some(-100));

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
