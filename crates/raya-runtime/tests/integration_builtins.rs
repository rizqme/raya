//! raya-runtime integration tests — builtins surface.

#[path = "e2e/harness.rs"]
mod harness;
pub use harness::*;
#[path = "e2e/builtins.rs"]
mod builtins;
