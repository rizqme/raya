//! Opcode handler modules for the VM interpreter
//!
//! Each module implements a category of opcode handlers as methods on `Interpreter`.

pub mod arithmetic;
pub mod arrays;
pub mod calls;
pub mod closures;
pub mod comparison;
pub mod concurrency;
pub mod constants;
pub mod control_flow;
pub mod exceptions;
pub mod native;
pub mod objects;
pub mod stack;
pub mod strings;
pub mod types;
pub mod variables;
