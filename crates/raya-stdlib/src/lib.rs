//! Raya Standard Library
//!
//! Native implementations for standard library functions.
//! Includes the `StdNativeHandler` that routes native call IDs
//! to their Rust implementations.

#![warn(missing_docs)]

pub mod handler;
pub mod logger;
pub mod math;
pub mod crypto;
pub mod path;
pub mod stream;
pub mod registry;

pub use handler::StdNativeHandler;
pub use registry::register_stdlib;
