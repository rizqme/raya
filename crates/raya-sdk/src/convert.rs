//! Traits for converting between Raya objects and Rust structs.
//!
//! Implement `FromNativeObject` and `ToNativeObject` to define a mapping
//! between a Raya class and a Rust struct. Manual implementation for now;
//! derive macros via `raya-native` can be added later.
//!
//! # Example
//!
//! ```ignore
//! use raya_sdk::{FromNativeObject, ToNativeObject, NativeObject, NativeClass, NativeValue, NativeContext, AbiResult};
//!
//! struct Point { x: f64, y: f64 }
//!
//! impl FromNativeObject for Point {
//!     fn from_native_object(obj: &NativeObject) -> AbiResult<Self> {
//!         Ok(Point {
//!             x: obj.get_f64("x")?,
//!             y: obj.get_f64("y")?,
//!         })
//!     }
//! }
//!
//! impl ToNativeObject for Point {
//!     fn class_name() -> &'static str { "Point" }
//!     fn to_native_object(&self, ctx: &dyn NativeContext) -> AbiResult<NativeValue> {
//!         let class = NativeClass::from_name(ctx, "Point")?;
//!         let schema = class.schema(ctx)?;
//!         let val = class.instantiate(ctx)?;
//!         let obj = NativeObject::wrap(ctx, val, &schema)?;
//!         obj.set("x", NativeValue::f64(self.x))?;
//!         obj.set("y", NativeValue::f64(self.y))?;
//!         Ok(obj.into_value())
//!     }
//! }
//! ```

use crate::context::NativeContext;
use crate::error::AbiResult;
use crate::types::NativeObject;
use crate::value::NativeValue;

/// Convert a Raya object into a Rust struct.
///
/// Implement this trait to allow extracting a Rust struct from a
/// `NativeObject` with named field access.
pub trait FromNativeObject: Sized {
    /// Convert from a NativeObject wrapper (with schema-based field access)
    fn from_native_object(obj: &NativeObject) -> AbiResult<Self>;
}

/// Convert a Rust struct into a Raya object.
///
/// Implement this trait to allow creating a Raya object from a Rust struct.
/// The implementation should create a class instance and set its fields.
pub trait ToNativeObject {
    /// The Raya class name this type maps to
    fn class_name() -> &'static str;

    /// Convert to a NativeValue (creates a Raya object and sets fields)
    fn to_native_object(&self, ctx: &dyn NativeContext) -> AbiResult<NativeValue>;
}
