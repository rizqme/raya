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

use raya_engine::compiler::module::StdModuleRegistry;
use std::sync::OnceLock;

pub use program::{CompiledProgram, ProgramCompiler, ProgramDiagnostics};

pub(crate) fn std_module_registry() -> &'static StdModuleRegistry {
    static REGISTRY: OnceLock<StdModuleRegistry> = OnceLock::new();
    REGISTRY.get_or_init(StdModuleRegistry::new)
}
