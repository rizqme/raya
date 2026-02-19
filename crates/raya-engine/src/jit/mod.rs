//! JIT and AOT compilation infrastructure for the Raya VM
//!
//! This module provides the framework for compiling Raya bytecode to native code.
//! It includes:
//! - Bytecode analysis (decoding, CFG construction)
//! - JIT IR (SSA form intermediate representation)
//! - Stack-to-SSA lifting pipeline
//! - Backend-agnostic optimization passes
//! - Backend trait for pluggable code generation
//! - Profiling infrastructure for hot function detection
//! - Code cache and function patching
//! - Static analysis heuristics for JIT candidate selection
//! - Pre-warming: compile CPU-intensive functions at module load time

pub mod analysis;
pub mod ir;
pub mod pipeline;
pub mod backend;
pub mod runtime;
pub mod profiling;

mod engine;
pub use engine::{JitEngine, JitConfig};
