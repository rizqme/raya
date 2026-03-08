//! raya-runtime integration tests — classes, runtime reflection, and TS surfaces.

#[path = "e2e/harness.rs"]
mod harness;
pub use harness::*;
#[path = "e2e/classes.rs"]
mod classes;
#[path = "e2e/closure_captures.rs"]
mod closure_captures;
#[path = "e2e/closures.rs"]
mod closures;
#[path = "e2e/inheritance.rs"]
mod inheritance;
#[path = "e2e/narrowing.rs"]
mod narrowing;
#[path = "e2e/reflect.rs"]
mod reflect;
#[path = "e2e/runtime.rs"]
mod runtime;
#[path = "e2e/ts_abstract_classes.rs"]
mod ts_abstract_classes;
#[path = "e2e/ts_advanced_classes.rs"]
mod ts_advanced_classes;
#[path = "e2e/ts_discriminated_unions.rs"]
mod ts_discriminated_unions;
#[path = "e2e/ts_generics.rs"]
mod ts_generics;
#[path = "e2e/ts_int_number.rs"]
mod ts_int_number;
#[path = "e2e/ts_intersection_types.rs"]
mod ts_intersection_types;
#[path = "e2e/ts_narrowing.rs"]
mod ts_narrowing;
#[path = "e2e/ts_type_aliases.rs"]
mod ts_type_aliases;
