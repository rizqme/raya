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
mod rest_params;

// TypeScript conformance test adaptations
mod ts_type_aliases;
mod ts_generics;
mod ts_abstract_classes;
mod ts_discriminated_unions;
mod ts_intersection_types;
mod ts_advanced_classes;
mod ts_narrowing;
mod ts_int_number;

// Real-world application e2e tests
mod real_world;

// Language completeness tests (cross-feature interactions, stress tests, edge cases)
mod cross_feature;
mod parser_stress;
mod type_system_edge_cases;
mod compiler_edge_cases;
mod missing_features;
mod diagnostics;
mod bug_hunting;
mod bug_hunting_2;
mod bug_hunting_3;


pub use harness::*;
