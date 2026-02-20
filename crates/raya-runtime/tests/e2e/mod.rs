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
mod closure_captures;
mod concurrency;
mod exceptions;
mod builtins;
mod json;
mod logger;
mod math;
mod reflect;
mod runtime;
mod crypto;
mod time;
mod path;
mod stream;
mod fundamentals;
mod edge_cases;
mod concurrency_edge_cases;
mod syntax_edge_cases;
mod env;
mod os;
mod io;
mod fs;
mod process;
mod module_vars;
mod type_checker;
mod narrowing;
mod inheritance;
mod scope_analysis;

// TypeScript conformance test adaptations
mod ts_type_aliases;
mod ts_generics;
mod ts_abstract_classes;
mod ts_discriminated_unions;
mod ts_intersection_types;
mod ts_advanced_classes;
mod ts_narrowing;
mod ts_int_number;


pub use harness::*;
