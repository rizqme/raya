//! Register-based opcode handler modules for the VM interpreter
//!
//! Each module implements a category of register opcode handlers as methods on `Interpreter`.
//! These are the register-based counterparts to the stack-based handlers in `opcodes/`.

pub mod arithmetic;
pub mod arrays;
pub mod calls;
pub mod closures;
pub mod comparison;
pub mod concurrency;
pub mod constants;
pub mod control_flow;
pub mod exceptions;
pub mod json;
pub mod native;
pub mod objects;
pub mod strings;
