//! raya-runtime integration tests — core language and syntax.

#[path = "e2e/harness.rs"]
mod harness;
pub use harness::*;
#[path = "e2e/args.rs"]
mod args;
#[path = "e2e/arrays.rs"]
mod arrays;
#[path = "e2e/conditionals.rs"]
mod conditionals;
#[path = "e2e/decorators.rs"]
mod decorators;
#[path = "e2e/edge_cases.rs"]
mod edge_cases;
#[path = "e2e/functions.rs"]
mod functions;
#[path = "e2e/fundamentals.rs"]
mod fundamentals;
#[path = "e2e/js_syntax_conformance.rs"]
mod js_syntax_conformance;
#[path = "e2e/literals.rs"]
mod literals;
#[path = "e2e/loops.rs"]
mod loops;
#[path = "e2e/math.rs"]
mod math;
#[path = "e2e/operators.rs"]
mod operators;
#[path = "e2e/optional_params.rs"]
mod optional_params;
#[path = "e2e/rest_params.rs"]
mod rest_params;
#[path = "e2e/scope_analysis.rs"]
mod scope_analysis;
#[path = "e2e/strings.rs"]
mod strings;
#[path = "e2e/syntax_edge_cases.rs"]
mod syntax_edge_cases;
#[path = "e2e/template.rs"]
mod template;
#[path = "e2e/time.rs"]
mod time;
#[path = "e2e/variables.rs"]
mod variables;
