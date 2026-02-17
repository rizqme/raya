//! Native method handlers for built-in types
//!
//! This module contains handlers for built-in method calls.

pub mod reflect;
pub mod runtime;

// Re-export handler functions and context types for use in interpreter
pub use runtime::{call_runtime_method, RuntimeHandlerContext};
