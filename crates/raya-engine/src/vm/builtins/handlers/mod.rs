//! Native method handlers for built-in types
//!
//! This module contains handlers for built-in method calls on arrays, strings,
//! numbers, regexp, and reflect operations. Each handler module implements
//! methods for a specific type category.

pub mod array;
pub mod number;
pub mod reflect;
pub mod regexp;
pub mod runtime;
pub mod string;
pub mod time;

// Re-export handler functions and context types for use in interpreter
pub use array::{call_array_method, ArrayHandlerContext};
pub use number::{call_number_method, NumberHandlerContext};
pub use reflect::{call_reflect_method, ReflectHandlerContext};
pub use regexp::{call_regexp_method, RegExpHandlerContext};
pub use runtime::{call_runtime_method, RuntimeHandlerContext};
pub use string::{call_string_method, StringHandlerContext};
pub use time::call_time_method;
