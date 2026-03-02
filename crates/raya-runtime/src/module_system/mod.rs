//! Runtime module system v2 (graph-first file compilation path).
//!
//! This module provides a canonical file-program compilation/checking pipeline
//! that resolves a module graph first, then compiles/checks a deterministic
//! merged program source.

pub mod graph;
pub mod linker;
pub mod loader;
pub mod program;
pub mod resolver;

pub use program::{CompiledProgram, ProgramCompiler, ProgramDiagnostics};
