//! Native method handlers for built-in types
//!
//! This module contains handlers for built-in method calls on arrays, strings,
//! regexp, and reflect operations. Each handler module implements methods for
//! a specific type category.

pub mod array;
pub mod reflect;
pub mod regexp;
pub mod string;

// Re-export handler functions and context types for use in task_interpreter
pub use array::{call_array_method, ArrayHandlerContext};
pub use reflect::{call_reflect_method, ReflectHandlerContext};
pub use regexp::{call_regexp_method, RegExpHandlerContext};
pub use string::{call_string_method, StringHandlerContext};
