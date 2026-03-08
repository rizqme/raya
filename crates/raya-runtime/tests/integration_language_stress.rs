//! raya-runtime integration tests — stress, diagnostics, and real-world mixes.

#[path = "e2e/harness.rs"]
mod harness;
pub use harness::*;
#[path = "e2e/bug_hunting.rs"]
mod bug_hunting;
#[path = "e2e/bug_hunting_2.rs"]
mod bug_hunting_2;
#[path = "e2e/bug_hunting_3.rs"]
mod bug_hunting_3;
#[path = "e2e/bug_hunting_4.rs"]
mod bug_hunting_4;
#[path = "e2e/bug_hunting_5.rs"]
mod bug_hunting_5;
#[path = "e2e/compiler_edge_cases.rs"]
mod compiler_edge_cases;
#[path = "e2e/cross_feature.rs"]
mod cross_feature;
#[path = "e2e/diagnostics.rs"]
mod diagnostics;
#[path = "e2e/missing_features.rs"]
mod missing_features;
#[path = "e2e/module_vars.rs"]
mod module_vars;
#[path = "e2e/parser_stress.rs"]
mod parser_stress;
#[path = "e2e/real_world.rs"]
mod real_world;
#[path = "e2e/type_checker.rs"]
mod type_checker;
#[path = "e2e/type_system_edge_cases.rs"]
mod type_system_edge_cases;
