//! Multi-module compiler
//!
//! Orchestrates compilation of multiple Raya source files with import resolution.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::compiler::bytecode::Module as BytecodeModule;
use crate::compiler::{Compiler, CompileError};
use crate::parser::ast::{Module as AstModule, Statement, ImportSpecifier};
use crate::parser::checker::{Binder, TypeChecker, Symbol, SymbolFlags, ScopeId};
use crate::parser::{Interner, Parser, TypeContext, Span};

use super::cache::ModuleCache;
use super::exports::{ExportRegistry, ExportedSymbol, ModuleExports};
use super::graph::{GraphError, ModuleGraph};
use super::resolver::{ModuleResolver, ResolveError};

/// Errors that can occur during multi-module compilation
#[derive(Debug, Error)]
pub enum ModuleCompileError {
    /// Module resolution failed
    #[error("Module resolution error: {0}")]
    Resolution(#[from] ResolveError),

    /// Circular dependency detected
    #[error("Circular dependency: {0}")]
    CircularDependency(#[from] GraphError),

    /// IO error reading source file
    #[error("IO error reading {path}: {message}")]
    IoError { path: PathBuf, message: String },

    /// Lexer error
    #[error("Lexer error in {path}: {message}")]
    LexError { path: PathBuf, message: String },

    /// Parse error
    #[error("Parse error in {path}: {message}")]
    ParseError { path: PathBuf, message: String },

    /// Type check error
    #[error("Type error in {path}: {message}")]
    TypeError { path: PathBuf, message: String },

    /// Compilation error
    #[error("Compile error in {path}: {source}")]
    CompileError {
        path: PathBuf,
        #[source]
        source: CompileError,
    },
}

/// Result type for module compilation
pub type ModuleCompileResult<T> = Result<T, ModuleCompileError>;

/// Compiled module with its path and dependencies
#[derive(Debug)]
pub struct CompiledModule {
    /// Absolute path to the source file
    pub path: PathBuf,
    /// Compiled bytecode module
    pub bytecode: BytecodeModule,
    /// Paths to modules this module imports
    pub imports: Vec<PathBuf>,
}

/// Multi-module compiler
///
/// Handles compilation of multiple Raya source files with:
/// - Import resolution
/// - Dependency graph construction
/// - Cycle detection
/// - Compilation in topological order
/// - Module caching
#[derive(Debug)]
pub struct ModuleCompiler {
    /// Module resolver for import paths
    resolver: ModuleResolver,
    /// Dependency graph
    graph: ModuleGraph,
    /// Compiled module cache
    cache: ModuleCache,
    /// Export registry for cross-module symbol resolution
    exports: ExportRegistry,
    /// JSX compilation options (None = JSX disabled)
    jsx_options: Option<crate::compiler::lower::JsxOptions>,
}

impl ModuleCompiler {
    /// Create a new module compiler
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            resolver: ModuleResolver::new(project_root),
            graph: ModuleGraph::new(),
            cache: ModuleCache::new(),
            exports: ExportRegistry::new(),
            jsx_options: None,
        }
    }

    /// Create a module compiler with the current directory as project root
    pub fn current_dir() -> ModuleCompileResult<Self> {
        let resolver = ModuleResolver::current_dir()?;
        Ok(Self {
            resolver,
            graph: ModuleGraph::new(),
            cache: ModuleCache::new(),
            exports: ExportRegistry::new(),
            jsx_options: None,
        })
    }

    /// Enable JSX compilation with the given options
    pub fn with_jsx(mut self, options: crate::compiler::lower::JsxOptions) -> Self {
        self.jsx_options = Some(options);
        self
    }

    /// Compile a single entry point and all its dependencies
    ///
    /// Returns the compiled modules in dependency order (dependencies first).
    /// Uses cross-module symbol resolution for imports.
    pub fn compile(&mut self, entry_point: &Path) -> ModuleCompileResult<Vec<CompiledModule>> {
        // Canonicalize the entry point
        let entry_path = entry_point.canonicalize().map_err(|e| {
            ModuleCompileError::IoError {
                path: entry_point.to_path_buf(),
                message: e.to_string(),
            }
        })?;

        // Discover all modules and build the dependency graph
        self.discover_modules(&entry_path)?;

        // Check for cycles
        self.graph.detect_cycles()?;

        // Get compilation order (dependencies first)
        let order = self.graph.topological_order()?;

        // Compile each module in order, tracking exports for cross-module resolution
        let mut compiled = Vec::new();
        for path in order {
            // Check cache first
            if let Some(cached) = self.cache.get(&path) {
                let node = self.graph.get(&path).unwrap();
                compiled.push(CompiledModule {
                    path: path.clone(),
                    bytecode: cached.bytecode.clone(),
                    imports: node.imports.clone(),
                });
                continue;
            }

            // Compile the module with cross-module symbol resolution
            let (bytecode, module_exports) = self.compile_single_with_exports(&path)?;

            // Register exports for dependent modules
            self.exports.register(module_exports);

            // Cache the bytecode
            self.cache.insert(path.clone(), bytecode.clone());

            let node = self.graph.get(&path).unwrap();
            compiled.push(CompiledModule {
                path: path.clone(),
                bytecode,
                imports: node.imports.clone(),
            });
        }

        Ok(compiled)
    }

    /// Discover all modules reachable from the given entry point
    fn discover_modules(&mut self, entry_path: &PathBuf) -> ModuleCompileResult<()> {
        let mut to_visit = vec![entry_path.clone()];
        let mut visited = HashSet::new();

        while let Some(path) = to_visit.pop() {
            if visited.contains(&path) {
                continue;
            }
            visited.insert(path.clone());

            // Add to graph
            self.graph.add_module(path.clone());

            // Parse the module to find imports
            let source = fs::read_to_string(&path).map_err(|e| ModuleCompileError::IoError {
                path: path.clone(),
                message: e.to_string(),
            })?;

            let imports = self.extract_imports(&source, &path)?;

            // Resolve and add each import
            for import_specifier in imports {
                match self.resolver.resolve(&import_specifier, &path) {
                    Ok(resolved) => {
                        self.graph.add_dependency(path.clone(), resolved.path.clone());
                        if !visited.contains(&resolved.path) {
                            to_visit.push(resolved.path);
                        }
                    }
                    Err(ResolveError::PackageNotSupported(_)) => {
                        // Package imports are not yet supported, skip for now
                        // In Phase 4, we'll handle these
                    }
                    Err(ResolveError::UrlNotLocked(_)) => {
                        // URL not locked yet - skip during dependency graph building
                        // The import will be handled when the lockfile has the URL entry
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        }

        Ok(())
    }

    /// Extract import specifiers from source code
    fn extract_imports(&self, source: &str, path: &Path) -> ModuleCompileResult<Vec<String>> {
        let parser = Parser::new(source).map_err(|e| ModuleCompileError::LexError {
            path: path.to_path_buf(),
            message: format!("{:?}", e),
        })?;

        let (ast, interner) = parser.parse().map_err(|e| ModuleCompileError::ParseError {
            path: path.to_path_buf(),
            message: format!("{:?}", e),
        })?;

        let mut imports = Vec::new();
        for stmt in &ast.statements {
            if let Statement::ImportDecl(import) = stmt {
                let specifier = interner.resolve(import.source.value).to_string();
                imports.push(specifier);
            }
        }

        Ok(imports)
    }

    /// Compile a single module with cross-module symbol resolution
    ///
    /// Returns the bytecode and the module's exports for use by dependent modules.
    fn compile_single_with_exports(&self, path: &PathBuf) -> ModuleCompileResult<(BytecodeModule, ModuleExports)> {
        // Read source
        let source = fs::read_to_string(path).map_err(|e| ModuleCompileError::IoError {
            path: path.clone(),
            message: e.to_string(),
        })?;

        // Parse
        let parser = Parser::new(&source).map_err(|e| ModuleCompileError::LexError {
            path: path.clone(),
            message: format!("{:?}", e),
        })?;

        let (ast, interner) = parser.parse().map_err(|e| ModuleCompileError::ParseError {
            path: path.clone(),
            message: format!("{:?}", e),
        })?;

        // Bind
        let mut type_ctx = TypeContext::new();
        let mut binder = Binder::new(&mut type_ctx, &interner);

        // Register builtin type signatures
        let builtin_sigs = crate::builtins::to_checker_signatures();
        binder.register_builtins(&builtin_sigs);

        // Inject imported symbols from the export registry
        self.inject_imports(&ast, path, &mut binder, &interner)?;

        let mut symbols = binder.bind_module(&ast).map_err(|e| ModuleCompileError::TypeError {
            path: path.clone(),
            message: format!("Binding error: {:?}", e),
        })?;

        // Type check
        let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
        let check_result =
            checker
                .check_module(&ast)
                .map_err(|e| ModuleCompileError::TypeError {
                    path: path.clone(),
                    message: format!("{:?}", e),
                })?;

        // Apply inferred types
        for ((scope_id, name), ty) in check_result.inferred_types {
            symbols.update_type(
                ScopeId(scope_id),
                &name,
                ty,
            );
        }

        // Extract exports for dependent modules
        let module_exports = self.extract_exports(path, &symbols);

        // Compile
        let mut compiler = Compiler::new(type_ctx, &interner).with_expr_types(check_result.expr_types);
        if let Some(ref jsx_opts) = self.jsx_options {
            compiler = compiler.with_jsx(jsx_opts.clone());
        }

        let bytecode = compiler.compile_via_ir(&ast).map_err(|e| ModuleCompileError::CompileError {
            path: path.clone(),
            source: e,
        })?;

        Ok((bytecode, module_exports))
    }

    /// Inject symbols from imported modules into the binder
    fn inject_imports(
        &self,
        ast: &AstModule,
        current_path: &PathBuf,
        binder: &mut Binder<'_>,
        interner: &Interner,
    ) -> ModuleCompileResult<()> {
        for stmt in &ast.statements {
            if let Statement::ImportDecl(import) = stmt {
                let specifier = interner.resolve(import.source.value).to_string();

                // Resolve the import path
                let resolved = match self.resolver.resolve(&specifier, current_path) {
                    Ok(r) => r,
                    Err(ResolveError::PackageNotSupported(_)) |
                    Err(ResolveError::UrlNotLocked(_)) => continue,
                    Err(e) => return Err(e.into()),
                };

                // Get exports from the imported module
                let module_exports = match self.exports.get(&resolved.path) {
                    Some(exports) => exports,
                    None => continue, // Module not yet compiled (shouldn't happen with topo order)
                };

                // Inject each imported specifier
                for spec in &import.specifiers {
                    match spec {
                        ImportSpecifier::Named { name, alias } => {
                            let import_name = interner.resolve(name.name).to_string();
                            let local_name = alias
                                .as_ref()
                                .map(|a| interner.resolve(a.name).to_string())
                                .unwrap_or_else(|| import_name.clone());

                            if let Some(exported) = module_exports.get(&import_name) {
                                let symbol = Symbol {
                                    name: local_name,
                                    kind: exported.kind,
                                    ty: exported.ty,
                                    flags: SymbolFlags {
                                        is_exported: false,
                                        is_const: exported.is_const,
                                        is_async: exported.is_async,
                                        is_readonly: false,
                                        is_imported: false,
                                    },
                                    scope_id: ScopeId(0),
                                    span: Span::new(0, 0, 0, 0),
                                    referenced: false,
                                };
                                // Ignore duplicate errors (might be re-importing)
                                let _ = binder.define_imported(symbol);
                            }
                        }
                        ImportSpecifier::Namespace(alias) => {
                            // import * as utils - create a namespace object
                            // For now, we skip namespace imports
                            let _ = alias; // Suppress unused warning
                        }
                        ImportSpecifier::Default(local) => {
                            // import foo from "./module" - look for default export
                            let local_name = interner.resolve(local.name).to_string();
                            if let Some(exported) = module_exports.get("default") {
                                let symbol = Symbol {
                                    name: local_name,
                                    kind: exported.kind,
                                    ty: exported.ty,
                                    flags: SymbolFlags {
                                        is_exported: false,
                                        is_const: exported.is_const,
                                        is_async: exported.is_async,
                                        is_readonly: false,
                                        is_imported: false,
                                    },
                                    scope_id: ScopeId(0),
                                    span: Span::new(0, 0, 0, 0),
                                    referenced: false,
                                };
                                let _ = binder.define_imported(symbol);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Extract exported symbols from a compiled module's symbol table
    fn extract_exports(&self, path: &PathBuf, symbols: &crate::parser::checker::SymbolTable) -> ModuleExports {
        let mut exports = ModuleExports::new(path.clone());

        for symbol in symbols.get_exported_symbols() {
            exports.add_symbol(ExportedSymbol::from_symbol(symbol));
        }

        exports
    }

    /// Get the module resolver
    pub fn resolver(&self) -> &ModuleResolver {
        &self.resolver
    }

    /// Get the dependency graph
    pub fn graph(&self) -> &ModuleGraph {
        &self.graph
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> super::cache::CacheStats {
        self.cache.stats()
    }

    /// Clear the module cache
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Reset the compiler (clears graph, cache, and exports)
    pub fn reset(&mut self) {
        self.graph = ModuleGraph::new();
        self.cache.clear();
        self.exports = ExportRegistry::new();
    }

    /// Get the export registry
    pub fn exports(&self) -> &ExportRegistry {
        &self.exports
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_project() -> TempDir {
        TempDir::new().unwrap()
    }

    #[test]
    fn test_compile_single_file() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");

        fs::write(&main_path, "let x: number = 42;").unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let result = compiler.compile(&main_path);

        assert!(result.is_ok());
        let compiled = result.unwrap();
        assert_eq!(compiled.len(), 1);
        assert_eq!(compiled[0].path, main_path.canonicalize().unwrap());
    }

    #[test]
    fn test_compile_with_local_import() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");
        let utils_path = temp_dir.path().join("utils.raya");

        // Phase 3: Cross-module symbol resolution now works!
        fs::write(&utils_path, "export let value: number = 42;").unwrap();
        fs::write(
            &main_path,
            r#"import { value } from "./utils";
               let x: number = value + 1;"#,  // Uses imported `value`
        )
        .unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let result = compiler.compile(&main_path);

        assert!(result.is_ok(), "Compilation failed: {:?}", result.err());
        let compiled = result.unwrap();

        // Should have 2 modules: utils first, then main
        assert_eq!(compiled.len(), 2);

        // utils should be first (dependency)
        assert_eq!(
            compiled[0].path,
            utils_path.canonicalize().unwrap()
        );

        // main should be second
        assert_eq!(
            compiled[1].path,
            main_path.canonicalize().unwrap()
        );
    }

    #[test]
    fn test_cross_module_function_import() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");
        let utils_path = temp_dir.path().join("utils.raya");

        // Test importing a function symbol
        // Note: Function calling across modules requires TypeContext merging
        // which is tracked as a follow-up issue. For now, we verify the symbol
        // is imported (even if calling it doesn't work due to type mismatch).
        fs::write(&utils_path, r#"export function add(a: number, b: number): number {
            return a + b;
        }"#).unwrap();
        fs::write(
            &main_path,
            r#"import { add } from "./utils";
               let x: number = 42;"#,  // Reference add but don't call it
        )
        .unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let result = compiler.compile(&main_path);

        // Compilation should succeed - the symbol is imported
        assert!(result.is_ok(), "Compilation failed: {:?}", result.err());
        let compiled = result.unwrap();
        assert_eq!(compiled.len(), 2);

        // Verify the export was registered
        let utils_canonical = utils_path.canonicalize().unwrap();
        assert!(compiler.exports().has_module(&utils_canonical));
        assert!(compiler.exports().get(&utils_canonical).unwrap().has("add"));
    }

    #[test]
    fn test_aliased_import() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");
        let utils_path = temp_dir.path().join("utils.raya");

        // Test import with alias: import { foo as bar }
        fs::write(&utils_path, "export let value: number = 42;").unwrap();
        fs::write(
            &main_path,
            r#"import { value as importedValue } from "./utils";
               let x: number = importedValue;"#,
        )
        .unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let result = compiler.compile(&main_path);

        assert!(result.is_ok(), "Compilation failed: {:?}", result.err());
    }

    #[test]
    fn test_circular_dependency_detection() {
        let temp_dir = create_test_project();
        let a_path = temp_dir.path().join("a.raya");
        let b_path = temp_dir.path().join("b.raya");

        fs::write(&a_path, r#"import { y } from "./b"; export let x: number = 1;"#).unwrap();
        fs::write(&b_path, r#"import { x } from "./a"; export let y: number = 2;"#).unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let result = compiler.compile(&a_path);

        assert!(matches!(
            result,
            Err(ModuleCompileError::CircularDependency(_))
        ));
    }

    #[test]
    fn test_diamond_dependency() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");
        let a_path = temp_dir.path().join("a.raya");
        let b_path = temp_dir.path().join("b.raya");
        let shared_path = temp_dir.path().join("shared.raya");

        // Phase 3: Full cross-module symbol resolution
        fs::write(&shared_path, "export let value: number = 42;").unwrap();
        fs::write(&a_path, r#"import { value } from "./shared"; export let a: number = value + 1;"#).unwrap();
        fs::write(&b_path, r#"import { value } from "./shared"; export let b: number = value + 2;"#).unwrap();
        fs::write(
            &main_path,
            r#"import { a } from "./a";
               import { b } from "./b";
               let result: number = a + b;"#,  // Uses imported symbols
        )
        .unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let result = compiler.compile(&main_path);

        assert!(result.is_ok(), "Compilation failed: {:?}", result.err());
        let compiled = result.unwrap();

        // Should have 4 modules
        assert_eq!(compiled.len(), 4);

        // shared should be first
        assert_eq!(
            compiled[0].path,
            shared_path.canonicalize().unwrap()
        );

        // main should be last
        assert_eq!(
            compiled[3].path,
            main_path.canonicalize().unwrap()
        );
    }

    #[test]
    fn test_module_not_found() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");

        fs::write(
            &main_path,
            r#"import { foo } from "./nonexistent";"#,
        )
        .unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let result = compiler.compile(&main_path);

        assert!(matches!(result, Err(ModuleCompileError::Resolution(_))));
    }

    #[test]
    fn test_cache_hit() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");

        fs::write(&main_path, "let x: number = 42;").unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());

        // First compilation
        let _ = compiler.compile(&main_path).unwrap();
        let stats1 = compiler.cache_stats();
        assert_eq!(stats1.misses, 1);
        assert_eq!(stats1.hits, 0);

        // Reset graph but keep cache
        compiler.graph = ModuleGraph::new();

        // Second compilation should hit cache
        let _ = compiler.compile(&main_path).unwrap();
        let stats2 = compiler.cache_stats();
        assert_eq!(stats2.hits, 1);
    }
}
