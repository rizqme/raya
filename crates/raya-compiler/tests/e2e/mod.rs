//! End-to-end tests for the Raya compiler
//!
//! These tests compile Raya source code and execute it in the VM,
//! verifying the results are correct.

mod harness;
mod literals;
mod operators;
mod variables;
mod conditionals;
mod loops;
mod functions;
mod strings;
mod arrays;
mod classes;
mod decorators;
mod async_await;
mod closures;
mod concurrency;
mod exceptions;

pub use harness::*;
