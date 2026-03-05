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
mod declaration;
mod exports;
mod graph;
mod resolver;
mod std_modules;
mod typedef;

/// The import specifier prefix for standard library modules (e.g., `"std:io"`).
pub const STD_MODULE_PREFIX: &str = "std:";
/// The import specifier prefix for Node-compat standard library modules
/// (e.g., `"node:fs"`).
pub const NODE_MODULE_PREFIX: &str = "node:";

pub use cache::ModuleCache;
pub use compiler::{CompiledModule, ModuleCompileError, ModuleCompiler};
pub use declaration::{
    builtin_global_exports, declaration_runtime_identity_path, load_declaration_module,
    load_declaration_module_from_source, specialization_template_from_symbol, BuiltinSurfaceMode,
    DeclarationError, DeclarationModule, DeclarationSourceKind, LateLinkRequirement,
    LateLinkSymbolRequirement,
};
pub use exports::{ExportRegistry, ExportedSymbol, ModuleExports};
pub use graph::{ModuleGraph, ModuleNode};
pub use resolver::{
    ModuleResolver, PackageResolverConfig, PackageSpecifier, ResolveError, ResolvedModule,
    ResolvedPackageInfo,
};
pub use std_modules::StdModuleRegistry;
pub use typedef::{
    load_typedef, ClassMemberSignature, ClassSignature, FunctionSignature, TypeAliasSignature,
    TypeDefError, TypeDefExport, TypeDefFile, TypeDefParser, VariableSignature,
};
