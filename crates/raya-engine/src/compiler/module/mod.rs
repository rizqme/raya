//! Module compilation and resolution
//!
//! This module provides multi-file compilation support with:
//! - Local import resolution (`./path`, `../path`)
//! - Module dependency graph construction
//! - Cycle detection
//! - Module caching
//! - Cross-module symbol resolution

mod cache;
mod compiler;
mod exports;
mod graph;
mod resolver;

pub use cache::ModuleCache;
pub use compiler::{CompiledModule, ModuleCompiler, ModuleCompileError};
pub use exports::{ExportedSymbol, ExportRegistry, ModuleExports};
pub use graph::{ModuleGraph, ModuleNode};
pub use resolver::{ModuleResolver, ResolvedModule};
