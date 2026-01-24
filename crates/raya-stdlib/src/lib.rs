//! Raya Standard Library
//!
//! Native implementations for standard library functions.

#![warn(missing_docs)]

pub mod console;
pub mod json;
pub mod json_native;

// Re-export the native module initializer
pub use json_native::init as json_module_init;
