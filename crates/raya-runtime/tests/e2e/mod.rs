//! End-to-end tests for the Raya compiler
//!
//! These tests compile Raya source code and execute it in the VM,
//! verifying the results are correct.

mod arrays;
mod async_await;
mod builtins;
mod classes;
mod closure_captures;
mod closures;
mod concurrency;
mod concurrency_edge_cases;
mod conditionals;
mod crypto;
mod decorators;
mod edge_cases;
mod env;
mod exceptions;
mod fs;
mod functions;
mod fundamentals;
mod harness;
mod inheritance;
mod io;
mod json;
mod literals;
mod logger;
mod loops;
mod math;
mod module_vars;
mod narrowing;
mod operators;
mod os;
mod path;
mod process;
mod reflect;
mod rest_params;
mod runtime;
mod scope_analysis;
mod stream;
mod strings;
mod syntax_edge_cases;
mod time;
mod type_checker;
mod variables;

// TypeScript conformance test adaptations
mod ts_abstract_classes;
mod ts_advanced_classes;
mod ts_discriminated_unions;
mod ts_generics;
mod ts_int_number;
mod ts_intersection_types;
mod ts_narrowing;
mod ts_type_aliases;

// Real-world application e2e tests
mod real_world;

// Language completeness tests (cross-feature interactions, stress tests, edge cases)
mod bug_hunting;
mod bug_hunting_2;
mod bug_hunting_3;
mod bug_hunting_4;
mod bug_hunting_5;
mod compiler_edge_cases;
mod cross_feature;
mod diagnostics;
mod missing_features;
mod parser_stress;
mod type_system_edge_cases;

pub use harness::*;
mod optional_params;
