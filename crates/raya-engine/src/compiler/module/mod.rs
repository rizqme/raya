//! Module compilation and resolution
//!
//! This module provides multi-file compilation support with:
//! - Local import resolution (`./path`, `../path`)
//! - Package import resolution (`"logging"`, `"logging@1.2.0"`)
//! - Module dependency graph construction
//! - Cycle detection
//! - Module caching
//! - Cross-module symbol resolution
//! - Type definition parsing (`.d.raya` files)

mod cache;
mod compiler;
mod exports;
mod graph;
mod resolver;
mod std_modules;
mod typedef;

pub use cache::ModuleCache;
pub use compiler::{CompiledModule, ModuleCompiler, ModuleCompileError};
pub use exports::{ExportedSymbol, ExportRegistry, ModuleExports};
pub use graph::{ModuleGraph, ModuleNode};
pub use resolver::{
    ModuleResolver, PackageResolverConfig, PackageSpecifier,
    ResolveError, ResolvedModule, ResolvedPackageInfo,
};
pub use std_modules::StdModuleRegistry;
pub use typedef::{
    ClassMemberSignature, ClassSignature, FunctionSignature, TypeAliasSignature,
    TypeDefError, TypeDefExport, TypeDefFile, TypeDefParser, VariableSignature,
    load_typedef,
};
