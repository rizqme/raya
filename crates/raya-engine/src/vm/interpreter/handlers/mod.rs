//! Native call method handlers for built-in types
//!
//! Each module implements handler methods as `impl Interpreter` blocks.

pub mod array;
pub mod buffer;
pub mod channel;
pub mod map;
pub mod reflect;
pub mod regexp;
pub mod set;
pub mod string;
