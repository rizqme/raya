//! Native call method handlers for built-in types
//!
//! Each module implements handler methods as `impl Interpreter` blocks.

pub mod array;
pub mod iterator;
pub mod reflect;
pub mod regexp;
pub mod string;
