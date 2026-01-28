//! Module system components
//!
//! This module contains the infrastructure for resolving and managing module imports.

mod deps;
mod import;
mod linker;

pub use deps::{DependencyGraph, GraphError};
pub use import::{ImportError, ImportResolver, ImportSpec};
pub use linker::{LinkError, ModuleLinker, ResolvedSymbol};
