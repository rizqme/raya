//! Raya Standard Library
//!
//! Native implementations for standard library functions.
//! Includes the `StdNativeHandler` that routes native call IDs
//! to their Rust implementations.

#![warn(missing_docs)]

pub mod handler;
pub mod logger;
pub mod math;

pub use handler::StdNativeHandler;
