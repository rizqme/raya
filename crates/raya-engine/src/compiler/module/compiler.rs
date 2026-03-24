//! Multi-module compiler
//!
//! Orchestrates compilation of multiple Raya source files with import resolution.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::compiler::bytecode::{
    Function as BytecodeFunction, Module as BytecodeModule, NominalTypeExport, Opcode,
};
use crate::compiler::{
    module_id_from_name, symbol_id_from_name, CompileError, Compiler, Export, Import, SymbolScope,
    SymbolType,
};
use crate::parser::ast::{
    ExportDecl, Expression, ImportSpecifier, Module as AstModule, Pattern, Statement,
};
use crate::parser::checker::{
    check_early_errors_with_options, Binder, CheckerPolicy, EarlyErrorOptions, ScopeId, ScopeKind,
    Symbol, SymbolFlags, SymbolKind, TypeChecker, TypeSystemMode,
};
use crate::parser::{Interner, Parser, Span, TypeContext};

use super::cache::ModuleCache;
use super::declaration::{
    builtin_global_exports, declaration_runtime_identity_path, load_declaration_module,
    specialization_template_from_symbol, BuiltinSurfaceMode, DeclarationError, DeclarationModule,
    DeclarationSourceKind, LateLinkRequirement, LateLinkSymbolRequirement,
};
use super::exports::{
    extract_module_exports, has_top_level_declaration_before_offset, inject_ambient_exports,
    module_exports_from_bytecode, ExportRegistry, ExportedSymbol, ModuleExports,
};
use super::graph::{GraphError, ModuleGraph};
use super::resolver::{ModuleResolver, ResolveError};
use super::std_modules::StdModuleRegistry;

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
    /// Whether this module is a declaration-only placeholder.
    pub declaration_only: bool,
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
    /// Embedded std/node module source registry.
    std_modules: StdModuleRegistry,
    /// Virtual source files materialized for std/node imports.
    virtual_sources: HashMap<PathBuf, String>,
    /// Virtual declaration modules keyed by virtual path.
    declaration_modules: HashMap<PathBuf, DeclarationModule>,
    /// Stable module identity -> virtual declaration path.
    declaration_virtual_by_identity: HashMap<String, PathBuf>,
    /// Late-link requirements collected from declaration-backed imports.
    late_link_requirements: HashMap<u64, LateLinkRequirement>,
    /// Checker mode (Raya strict or JS-like compatibility).
    checker_mode: TypeSystemMode,
    /// Checker policy override (TS flags etc.) propagated into bind/check passes.
    checker_policy: CheckerPolicy,
    /// Builtin declaration surface used for global symbol seeding.
    builtin_surface_mode: BuiltinSurfaceMode,
    /// Optional override loaded from compiled builtin artifacts instead of declarations.
    builtin_globals_override: Option<ModuleExports>,
    /// Cached builtin global exports for the configured surface mode.
    builtin_globals: Option<ModuleExports>,
    /// Current executable entry path, if compiling a program graph.
    root_entry_path: Option<PathBuf>,
}

impl ModuleCompiler {
    fn escape_signature_atom(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace(':', "\\:")
            .replace(',', "\\,")
            .replace('|', "\\|")
    }

    fn namespace_contract_from_exports(module_exports: &ModuleExports) -> (u64, String) {
        let mut members = module_exports
            .symbols
            .values()
            .filter(|exported| {
                exported.scope == SymbolScope::Module
                    && matches!(
                        exported.kind,
                        SymbolKind::Function
                            | SymbolKind::Class
                            | SymbolKind::Variable
                            | SymbolKind::EnumMember
                    )
            })
            .map(|exported| {
                format!(
                    "prop:{}:ro:req:{}",
                    Self::escape_signature_atom(&exported.name),
                    exported.type_signature
                )
            })
            .collect::<Vec<_>>();
        members.sort();
        let signature = format!("obj({})", members.join(","));
        let hash = crate::parser::types::signature_hash(&signature);
        (hash, signature)
    }

    fn apply_default_export_type_overrides(
        ast: &AstModule,
        symbols: &mut crate::parser::checker::SymbolTable,
        interner: &Interner,
        expr_types: &rustc_hash::FxHashMap<usize, crate::parser::TypeId>,
    ) {
        for stmt in &ast.statements {
            let Statement::ExportDecl(ExportDecl::Default { expression, .. }) = stmt else {
                continue;
            };

            let resolved_ty = match expression.as_ref() {
                Expression::Identifier(ident) => {
                    let name = interner.resolve(ident.name).to_string();
                    symbols.resolve(&name).map(|sym| sym.ty).or_else(|| {
                        let expr_id = expression.as_ref() as *const _ as usize;
                        expr_types.get(&expr_id).copied()
                    })
                }
                _ => {
                    let expr_id = expression.as_ref() as *const _ as usize;
                    expr_types.get(&expr_id).copied()
                }
            };

            let Some(default_symbol) = symbols.resolve("default") else {
                continue;
            };

            if let Some(default_ty) = resolved_ty {
                symbols.update_type(default_symbol.scope_id, "default", default_ty);
            }
        }
    }

    /// Create a new module compiler
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            resolver: ModuleResolver::new(project_root),
            graph: ModuleGraph::new(),
            cache: ModuleCache::new(),
            exports: ExportRegistry::new(),
            jsx_options: None,
            std_modules: StdModuleRegistry::new(),
            virtual_sources: HashMap::new(),
            declaration_modules: HashMap::new(),
            declaration_virtual_by_identity: HashMap::new(),
            late_link_requirements: HashMap::new(),
            checker_mode: TypeSystemMode::Raya,
            checker_policy: CheckerPolicy::for_mode(TypeSystemMode::Raya),
            builtin_surface_mode: BuiltinSurfaceMode::RayaStrict,
            builtin_globals_override: None,
            builtin_globals: None,
            root_entry_path: None,
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
            std_modules: StdModuleRegistry::new(),
            virtual_sources: HashMap::new(),
            declaration_modules: HashMap::new(),
            declaration_virtual_by_identity: HashMap::new(),
            late_link_requirements: HashMap::new(),
            checker_mode: TypeSystemMode::Raya,
            checker_policy: CheckerPolicy::for_mode(TypeSystemMode::Raya),
            builtin_surface_mode: BuiltinSurfaceMode::RayaStrict,
            builtin_globals_override: None,
            builtin_globals: None,
            root_entry_path: None,
        })
    }

    /// Enable JSX compilation with the given options
    pub fn with_jsx(mut self, options: crate::compiler::lower::JsxOptions) -> Self {
        self.jsx_options = Some(options);
        self
    }

    /// Configure checker mode for graph compilation.
    pub fn with_checker_mode(mut self, mode: TypeSystemMode) -> Self {
        self.checker_mode = mode;
        self.checker_policy = CheckerPolicy::for_mode(mode);
        self
    }

    /// Configure checker policy for graph compilation.
    pub fn with_checker_policy(mut self, policy: CheckerPolicy) -> Self {
        self.checker_policy = policy;
        self
    }

    /// Configure builtin declaration surface for global symbol seeding.
    pub fn with_builtin_surface_mode(mut self, mode: BuiltinSurfaceMode) -> Self {
        if self.builtin_surface_mode != mode {
            self.builtin_surface_mode = mode;
            self.builtin_globals = None;
        }
        self
    }

    /// Override builtin global contracts with data derived from compiled builtin artifacts.
    pub fn with_builtin_globals_override(mut self, exports: ModuleExports) -> Self {
        self.builtin_globals_override = Some(exports.clone());
        self.builtin_globals = Some(exports);
        self
    }

    fn std_virtual_path(canonical_name: &str) -> PathBuf {
        let encoded = canonical_name.replace('/', "__");
        PathBuf::from(format!("__raya_std__/{}.raya", encoded))
    }

    fn declaration_virtual_path(module_identity: &str) -> PathBuf {
        let module_id = module_id_from_name(module_identity);
        PathBuf::from(format!("__raya_decl__/{}.raya", module_id))
    }

    fn is_virtual_module(&self, path: &Path) -> bool {
        self.virtual_sources.contains_key(path) || self.declaration_modules.contains_key(path)
    }

    fn read_module_source(&self, path: &Path) -> ModuleCompileResult<String> {
        if let Some(source) = self.virtual_sources.get(path) {
            return Ok(source.clone());
        }
        fs::read_to_string(path).map_err(|e| ModuleCompileError::IoError {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
    }

    fn module_identity(&self, path: &Path) -> String {
        self.declaration_modules
            .get(path)
            .map(|decl| decl.module_identity.clone())
            .or_else(|| {
                self.exports
                    .get(&path.to_path_buf())
                    .map(|exports| exports.module_name.clone())
            })
            .unwrap_or_else(|| path.to_string_lossy().to_string())
    }

    fn is_binary_module(path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext == "ryb")
            .unwrap_or(false)
    }

    fn load_binary_module(&self, path: &Path) -> ModuleCompileResult<BytecodeModule> {
        let bytes = fs::read(path).map_err(|error| ModuleCompileError::IoError {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        BytecodeModule::decode(&bytes).map_err(|error| ModuleCompileError::TypeError {
            path: path.to_path_buf(),
            message: format!("Failed to decode bytecode module: {error}"),
        })
    }

    fn extract_binary_imports(bytecode: &BytecodeModule) -> Vec<String> {
        let mut imports = Vec::new();
        let mut seen = HashSet::new();
        for import in &bytecode.imports {
            if seen.insert(import.module_specifier.clone()) {
                imports.push(import.module_specifier.clone());
            }
        }
        imports
    }

    fn populate_binary_reexports(
        &mut self,
        current_path: &Path,
        bytecode: &BytecodeModule,
        module_exports: &mut ModuleExports,
    ) -> ModuleCompileResult<()> {
        for import in &bytecode.imports {
            if import.symbol != "*" || import.alias.is_some() {
                continue;
            }
            if let Some(reexport_path) =
                self.resolve_import_path(&import.module_specifier, current_path)?
            {
                module_exports.add_reexport(reexport_path);
            }
        }
        Ok(())
    }

    fn ensure_declaration_virtual_module(
        &mut self,
        declaration_path: &Path,
        module_identity: String,
    ) -> ModuleCompileResult<PathBuf> {
        if let Some(existing) = self.declaration_virtual_by_identity.get(&module_identity) {
            return Ok(existing.clone());
        }

        let virtual_path = Self::declaration_virtual_path(&module_identity);
        let declaration =
            load_declaration_module(declaration_path, &module_identity, &virtual_path)
                .map_err(|error| self.map_declaration_error(error))?;
        self.virtual_sources
            .insert(virtual_path.clone(), declaration.normalized_source.clone());
        self.declaration_virtual_by_identity
            .insert(module_identity.clone(), virtual_path.clone());
        self.late_link_requirements
            .entry(module_id_from_name(&module_identity))
            .or_insert(LateLinkRequirement {
                module_identity: module_identity.clone(),
                module_id: module_id_from_name(&module_identity),
                declaration_path: declaration.declaration_path.clone(),
                source_kind: declaration.source_kind,
                module_specifiers: Vec::new(),
                symbols: Vec::new(),
            });
        self.declaration_modules
            .insert(virtual_path.clone(), declaration);
        Ok(virtual_path)
    }

    fn resolve_local_declaration_module(
        &mut self,
        specifier: &str,
        from_path: &Path,
    ) -> ModuleCompileResult<Option<PathBuf>> {
        if !(specifier.starts_with("./")
            || specifier.starts_with("../")
            || specifier.starts_with('/'))
        {
            return Ok(None);
        }

        let from_dir = from_path
            .parent()
            .ok_or_else(|| ModuleCompileError::Resolution(ResolveError::NoParentDirectory))?;
        let base = if Path::new(specifier).is_absolute() {
            PathBuf::from(specifier)
        } else {
            from_dir.join(specifier)
        };

        let mut candidates = Vec::new();
        if base.extension().is_some() {
            candidates.push(base.clone());
        } else {
            candidates.push(base.with_extension("d.ts"));
            candidates.push(base.join("index.d.ts"));
        }

        for candidate in candidates {
            if !candidate.is_file() {
                continue;
            }
            let canonical_decl =
                candidate
                    .canonicalize()
                    .map_err(|e| ModuleCompileError::IoError {
                        path: candidate.clone(),
                        message: e.to_string(),
                    })?;
            if DeclarationSourceKind::from_path(&canonical_decl).is_none() {
                continue;
            }
            let runtime_identity_path = declaration_runtime_identity_path(&canonical_decl)
                .unwrap_or_else(|| canonical_decl.clone());
            let module_identity = runtime_identity_path.to_string_lossy().to_string();
            let virtual_path =
                self.ensure_declaration_virtual_module(&canonical_decl, module_identity)?;
            return Ok(Some(virtual_path));
        }

        Ok(None)
    }

    fn map_declaration_error(&self, error: DeclarationError) -> ModuleCompileError {
        match error {
            DeclarationError::IoError { path, message } => {
                ModuleCompileError::IoError { path, message }
            }
            DeclarationError::LexError { path, message } => {
                ModuleCompileError::LexError { path, message }
            }
            DeclarationError::ParseError { path, message } => {
                ModuleCompileError::ParseError { path, message }
            }
            DeclarationError::UnsupportedTsSyntax {
                path,
                line,
                column,
                snippet,
            } => ModuleCompileError::TypeError {
                path,
                message: format!(
                    "Unsupported .d.ts syntax at line {}, column {}: {}",
                    line, column, snippet
                ),
            },
            DeclarationError::InvalidDeclaration {
                path,
                line,
                column,
                message,
            } => ModuleCompileError::TypeError {
                path,
                message: format!(
                    "Invalid declaration at line {}, column {}: {}",
                    line, column, message
                ),
            },
        }
    }

    fn resolve_import_path(
        &mut self,
        specifier: &str,
        from_path: &Path,
    ) -> ModuleCompileResult<Option<PathBuf>> {
        if let Some((canonical_name, source)) = self.std_modules.resolve_specifier(specifier) {
            let virtual_path = Self::std_virtual_path(&canonical_name);
            self.virtual_sources
                .entry(virtual_path.clone())
                .or_insert_with(|| source.to_string());
            return Ok(Some(virtual_path));
        }

        if StdModuleRegistry::is_node_import(specifier)
            && !StdModuleRegistry::is_supported_node_import(specifier)
        {
            let mut supported = StdModuleRegistry::supported_node_module_names()
                .map(|name| format!("node:{name}"))
                .collect::<Vec<_>>();
            supported.sort();
            return Err(ModuleCompileError::TypeError {
                path: from_path.to_path_buf(),
                message: format!(
                    "Unsupported node module import '{}'. Supported node modules: {}",
                    specifier,
                    supported.join(", ")
                ),
            });
        }

        if StdModuleRegistry::is_std_import(specifier) {
            return Err(ModuleCompileError::Resolution(ResolveError::StdModule(
                specifier.to_string(),
            )));
        }

        match self.resolver.resolve(specifier, from_path) {
            Ok(resolved) => {
                if Self::is_binary_module(&resolved.path) {
                    return Ok(Some(resolved.path));
                }
                Ok(Some(resolved.path))
            }
            Err(ResolveError::PackageNotSupported(_)) | Err(ResolveError::UrlNotLocked(_)) => {
                Ok(None)
            }
            Err(error @ ResolveError::ModuleNotFound { .. }) => {
                if let Some(declaration_path) =
                    self.resolve_local_declaration_module(specifier, from_path)?
                {
                    Ok(Some(declaration_path))
                } else {
                    Err(ModuleCompileError::Resolution(error))
                }
            }
            Err(error) => Err(ModuleCompileError::Resolution(error)),
        }
    }

    /// Compile a single entry point and all its dependencies
    ///
    /// Returns the compiled modules in dependency order (dependencies first).
    /// Uses cross-module symbol resolution for imports.
    pub fn compile(&mut self, entry_point: &Path) -> ModuleCompileResult<Vec<CompiledModule>> {
        let entry_path = entry_point
            .canonicalize()
            .map_err(|e| ModuleCompileError::IoError {
                path: entry_point.to_path_buf(),
                message: e.to_string(),
            })?;
        self.compile_resolved_entry(entry_path)
    }

    /// Compile a module graph from an in-memory virtual entry source.
    ///
    /// The entry path does not need to exist on disk.
    pub fn compile_with_virtual_entry_source(
        &mut self,
        entry_path: &Path,
        source: String,
    ) -> ModuleCompileResult<Vec<CompiledModule>> {
        let entry = entry_path.to_path_buf();
        self.virtual_sources.insert(entry.clone(), source);
        self.compile_resolved_entry(entry)
    }

    fn compile_resolved_entry(
        &mut self,
        entry_path: PathBuf,
    ) -> ModuleCompileResult<Vec<CompiledModule>> {
        self.root_entry_path = Some(entry_path.clone());
        // Discover all modules and build the dependency graph
        self.discover_modules(&entry_path)?;

        // Check for cycles
        self.graph.detect_cycles()?;

        // Get compilation order (dependencies first)
        let order = self.graph.topological_order()?;

        // Compile each module in order, tracking exports for cross-module resolution
        let mut compiled = Vec::new();
        for path in order {
            if let Some(declaration_module) = self.declaration_modules.get(&path).cloned() {
                let (bytecode, module_exports) =
                    self.compile_declaration_placeholder(&path, &declaration_module)?;
                self.exports.register(module_exports);
                let node = self.graph.get(&path).unwrap();
                compiled.push(CompiledModule {
                    path: path.clone(),
                    bytecode,
                    imports: node.imports.clone(),
                    declaration_only: true,
                });
                continue;
            }

            // Check cache first
            if !self.is_virtual_module(&path) {
                if let Some(cached) = self.cache.get(&path) {
                    let cached_bytecode = cached.bytecode.clone();
                    let mut module_exports = module_exports_from_bytecode(&path, &cached_bytecode);
                    self.populate_binary_reexports(&path, &cached_bytecode, &mut module_exports)?;
                    self.exports.register(module_exports);
                    let node = self.graph.get(&path).unwrap();
                    compiled.push(CompiledModule {
                        path: path.clone(),
                        bytecode: cached_bytecode,
                        imports: node.imports.clone(),
                        declaration_only: false,
                    });
                    continue;
                }
            }

            if Self::is_binary_module(&path) {
                let bytecode = self.load_binary_module(&path)?;
                let mut module_exports = module_exports_from_bytecode(&path, &bytecode);
                self.populate_binary_reexports(&path, &bytecode, &mut module_exports)?;
                self.exports.register(module_exports);

                if !self.is_virtual_module(&path) {
                    self.cache.insert(path.clone(), bytecode.clone());
                }

                let node = self.graph.get(&path).unwrap();
                compiled.push(CompiledModule {
                    path: path.clone(),
                    bytecode,
                    imports: node.imports.clone(),
                    declaration_only: false,
                });
                continue;
            }

            // Compile the module with cross-module symbol resolution
            let (bytecode, mut module_exports) = self.compile_single_with_exports(&path)?;

            // Record `export * from "..."` chains so import resolution can
            // follow re-exported symbols transitively through ExportRegistry.
            for import in &bytecode.imports {
                if import.symbol == "*" && import.alias.is_none() {
                    if let Some(reexport_path) =
                        self.resolve_import_path(&import.module_specifier, &path)?
                    {
                        module_exports.add_reexport(reexport_path);
                    }
                }
            }

            // Register exports for dependent modules
            self.exports.register(module_exports);

            // Cache the bytecode
            if !self.is_virtual_module(&path) {
                self.cache.insert(path.clone(), bytecode.clone());
            }

            let node = self.graph.get(&path).unwrap();
            compiled.push(CompiledModule {
                path: path.clone(),
                bytecode,
                imports: node.imports.clone(),
                declaration_only: false,
            });
        }

        self.root_entry_path = None;
        Ok(compiled)
    }

    /// Discover all modules reachable from the given entry point
    fn discover_modules(&mut self, entry_path: &Path) -> ModuleCompileResult<()> {
        let mut to_visit = vec![entry_path.to_path_buf()];
        let mut visited = HashSet::new();

        while let Some(path) = to_visit.pop() {
            if visited.contains(&path) {
                continue;
            }
            visited.insert(path.clone());

            // Add to graph
            self.graph.add_module(path.clone());

            // Parse the module to find imports
            let imports = if Self::is_binary_module(&path) {
                let bytecode = self.load_binary_module(&path)?;
                Self::extract_binary_imports(&bytecode)
            } else {
                let source = self.read_module_source(&path)?;
                self.extract_imports(&source, &path)?
            };

            // Resolve and add each import
            for import_specifier in imports {
                if let Some(resolved_path) = self.resolve_import_path(&import_specifier, &path)? {
                    self.graph
                        .add_dependency(path.clone(), resolved_path.clone());
                    if !visited.contains(&resolved_path) {
                        to_visit.push(resolved_path);
                    }
                }
            }
        }

        Ok(())
    }

    fn compile_declaration_placeholder(
        &self,
        path: &Path,
        declaration_module: &DeclarationModule,
    ) -> ModuleCompileResult<(BytecodeModule, ModuleExports)> {
        let mut bytecode = BytecodeModule::new(declaration_module.module_identity.clone());

        let mut function_exports = Vec::new();
        let mut class_exports = Vec::new();
        let mut constant_exports = Vec::new();

        for exported in declaration_module.exports.symbols.values() {
            let symbol_type = match exported.kind {
                crate::parser::checker::SymbolKind::Function => SymbolType::Function,
                crate::parser::checker::SymbolKind::Class
                | crate::parser::checker::SymbolKind::Interface => SymbolType::Class,
                crate::parser::checker::SymbolKind::Variable
                | crate::parser::checker::SymbolKind::EnumMember => SymbolType::Constant,
                _ => continue,
            };

            match symbol_type {
                SymbolType::Function => function_exports.push(exported),
                SymbolType::Class => class_exports.push(exported),
                SymbolType::Constant => constant_exports.push(exported),
            }
        }

        function_exports.sort_by(|a, b| a.name.cmp(&b.name));
        class_exports.sort_by(|a, b| a.name.cmp(&b.name));
        constant_exports.sort_by(|a, b| a.name.cmp(&b.name));

        for exported in function_exports {
            let function_index = bytecode.functions.len();
            bytecode.functions.push(BytecodeFunction {
                name: format!("__decl_stub_{}", exported.name),
                param_count: 0,
                uses_js_this_slot: false,
                is_constructible: false,
                is_generator: false,
                visible_length: 0,
                is_strict_js: false,
                uses_builtin_this_coercion: false,
                js_arguments_mapping: Vec::new(),
                local_count: 0,
                code: vec![Opcode::ConstNull.to_u8(), Opcode::Return.to_u8()],
            });
            bytecode.exports.push(Export {
                name: exported.name.clone(),
                symbol_type: SymbolType::Function,
                index: function_index,
                symbol_id: exported.symbol_id,
                scope: exported.scope,
                signature_hash: exported.signature_hash,
                type_signature: Some(exported.type_signature.clone()),
                runtime_global_slot: None,
                nominal_type: None,
            });
        }

        for exported in class_exports {
            let class_index = bytecode.classes.len();
            bytecode.classes.push(crate::compiler::ClassDef {
                name: exported.name.clone(),
                field_count: 0,
                parent_id: None,
                parent_name: None,
                methods: Vec::new(),
                static_methods: Vec::new(),
                runtime_instance_publication: false,
                runtime_static_publication: false,
            });
            bytecode.exports.push(Export {
                name: exported.name.clone(),
                symbol_type: SymbolType::Class,
                index: class_index,
                symbol_id: exported.symbol_id,
                scope: exported.scope,
                signature_hash: exported.signature_hash,
                type_signature: Some(exported.type_signature.clone()),
                runtime_global_slot: None,
                nominal_type: Some(NominalTypeExport {
                    local_nominal_type_index: class_index as u32,
                    constructor_function_index: None,
                }),
            });
        }

        for exported in constant_exports {
            let index = bytecode.constants.integers.len();
            bytecode.constants.integers.push(0);
            bytecode.exports.push(Export {
                name: exported.name.clone(),
                symbol_type: SymbolType::Constant,
                index,
                symbol_id: exported.symbol_id,
                scope: exported.scope,
                signature_hash: exported.signature_hash,
                type_signature: Some(exported.type_signature.clone()),
                runtime_global_slot: None,
                nominal_type: None,
            });
        }

        let encoded = bytecode.encode();
        let decoded =
            BytecodeModule::decode(&encoded).map_err(|e| ModuleCompileError::TypeError {
                path: path.to_path_buf(),
                message: format!("Failed to finalize declaration placeholder module checksum: {e}"),
            })?;

        Ok((decoded, declaration_module.exports.clone()))
    }

    /// Extract import specifiers from source code
    fn extract_imports(&self, source: &str, path: &Path) -> ModuleCompileResult<Vec<String>> {
        let parser =
            Parser::new_with_mode(source, self.parser_mode_for_path(path)).map_err(|e| {
                ModuleCompileError::LexError {
                    path: path.to_path_buf(),
                    message: format!("{:?}", e),
                }
            })?;

        let (ast, interner) = parser.parse().map_err(|e| ModuleCompileError::ParseError {
            path: path.to_path_buf(),
            message: format!("{:?}", e),
        })?;

        let mut imports = Vec::new();
        for stmt in &ast.statements {
            match stmt {
                Statement::ImportDecl(import) => {
                    let specifier = interner.resolve(import.source.value).to_string();
                    imports.push(specifier);
                }
                Statement::ExportDecl(ExportDecl::Named {
                    source: Some(source),
                    ..
                })
                | Statement::ExportDecl(ExportDecl::All { source, .. }) => {
                    let specifier = interner.resolve(source.value).to_string();
                    imports.push(specifier);
                }
                _ => {}
            }
        }

        Ok(imports)
    }

    fn should_inject_builtin_globals(&self, path: &Path) -> bool {
        let logical = path.to_string_lossy();
        !(logical.starts_with("__raya_builtin__/") || logical.starts_with("<__raya_builtin_"))
    }

    fn parser_mode_for_path(&self, path: &Path) -> TypeSystemMode {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("js" | "mjs" | "cjs" | "jsx") => TypeSystemMode::Js,
            Some("ts" | "mts" | "cts" | "tsx") => TypeSystemMode::Ts,
            _ => TypeSystemMode::Raya,
        }
    }

    fn early_error_options_for_path(&self, path: &Path) -> EarlyErrorOptions {
        let mut options = EarlyErrorOptions::for_mode(self.checker_mode);
        let is_entry = self
            .root_entry_path
            .as_ref()
            .is_some_and(|entry| entry == path);
        if is_entry {
            options.allow_top_level_return = true;
            options.allow_await_outside_async = true;
        }
        options
    }

    fn inject_builtin_globals(
        &mut self,
        binder: &mut Binder<'_>,
        ast: &AstModule,
        interner: &Interner,
        current_path: &Path,
    ) -> ModuleCompileResult<()> {
        if self.builtin_globals.is_none() {
            let exports = if let Some(override_exports) = self.builtin_globals_override.as_ref() {
                override_exports.clone()
            } else {
                builtin_global_exports(self.builtin_surface_mode).map_err(|e| {
                    ModuleCompileError::TypeError {
                        path: current_path.to_path_buf(),
                        message: format!("Failed to load builtin declaration surface: {e}"),
                    }
                })?
            };
            self.builtin_globals = Some(exports);
        }

        let Some(exports) = self.builtin_globals.as_ref() else {
            return Ok(());
        };
        inject_ambient_exports(binder, ast, interner, exports);

        Ok(())
    }

    /// Compile a single module with cross-module symbol resolution
    ///
    /// Returns the bytecode and the module's exports for use by dependent modules.
    fn compile_single_with_exports(
        &mut self,
        path: &PathBuf,
    ) -> ModuleCompileResult<(BytecodeModule, ModuleExports)> {
        let debug_stages = std::env::var("RAYA_DEBUG_MODULE_COMPILE_STAGES").is_ok();
        // Read source
        let source = self.read_module_source(path)?;

        if debug_stages {
            eprintln!("[module-compile] parse:start path={}", path.display());
        }
        // Parse
        let parser =
            Parser::new_with_mode(&source, self.parser_mode_for_path(path)).map_err(|e| {
                ModuleCompileError::LexError {
                    path: path.clone(),
                    message: format!("{:?}", e),
                }
            })?;

        let (ast, interner) = parser.parse().map_err(|e| ModuleCompileError::ParseError {
            path: path.clone(),
            message: format!("{:?}", e),
        })?;
        check_early_errors_with_options(&ast, &interner, self.early_error_options_for_path(path))
            .map_err(|e| ModuleCompileError::ParseError {
            path: path.clone(),
            message: e
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        })?;
        if debug_stages {
            eprintln!("[module-compile] parse:done path={}", path.display());
            eprintln!("[module-compile] bind:start path={}", path.display());
        }

        // Bind
        let mut type_ctx = TypeContext::new();
        let mut binder = Binder::new(&mut type_ctx, &interner)
            .with_mode(self.checker_mode)
            .with_policy(self.checker_policy);

        if self.should_inject_builtin_globals(path) {
            binder.register_builtins(&[]);
            self.inject_builtin_globals(&mut binder, &ast, &interner, path)?;
        }

        // Inject imported symbols from the export registry
        self.inject_imports(&ast, path, &mut binder, &interner, None)?;

        let mut symbols = binder
            .bind_module(&ast)
            .map_err(|e| ModuleCompileError::TypeError {
                path: path.clone(),
                message: format!(
                    "Binding error: {}",
                    e.iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
            })?;
        if debug_stages {
            eprintln!("[module-compile] bind:done path={}", path.display());
            eprintln!("[module-compile] check:start path={}", path.display());
        }

        if std::env::var("RAYA_DEBUG_IMPORT_TYPES").is_ok() {
            for scope in [ScopeId(0), ScopeId(1)] {
                if let Some(sym) = symbols.resolve_from_scope("process", scope) {
                    eprintln!(
                        "[module-compiler] post-bind symbol 'process' scope={} kind={:?} imported={} ty='{}'",
                        scope.0,
                        sym.kind,
                        sym.flags.is_imported,
                        type_ctx.format_type(sym.ty)
                    );
                }
            }
        }

        // Type check
        let mut checker = TypeChecker::new(&mut type_ctx, &symbols, &interner)
            .with_mode(self.checker_mode)
            .with_policy(self.checker_policy);
        let check_result =
            checker
                .check_module(&ast)
                .map_err(|e| ModuleCompileError::TypeError {
                    path: path.clone(),
                    message: e
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; "),
                })?;
        if debug_stages {
            eprintln!("[module-compile] check:done path={}", path.display());
            eprintln!(
                "[module-compile] lower+codegen:start path={}",
                path.display()
            );
        }

        // Apply inferred types
        for ((scope_id, name), ty) in check_result.inferred_types {
            symbols.update_type(ScopeId(scope_id), &name, ty);
        }
        Self::apply_default_export_type_overrides(
            &ast,
            &mut symbols,
            &interner,
            &check_result.expr_types,
        );

        let module_name = self.module_identity(path);
        // Extract exports for dependent modules
        let module_exports =
            extract_module_exports(&ast, path, &module_name, &symbols, &interner, &type_ctx);

        let ambient_builtin_globals: Vec<String> = self
            .builtin_globals
            .as_ref()
            .map(|exports| {
                let mut names = exports
                    .symbols
                    .iter()
                    .filter_map(|(name, exported)| match exported.kind {
                        crate::parser::checker::SymbolKind::TypeAlias
                        | crate::parser::checker::SymbolKind::TypeParameter
                        | crate::parser::checker::SymbolKind::Interface => None,
                        _ => Some(name.clone()),
                    })
                    .collect::<Vec<_>>();
                if !names.iter().any(|name| name == "EventEmitter") {
                    names.push("EventEmitter".to_string());
                }
                names.sort();
                names
            })
            .unwrap_or_else(|| vec!["EventEmitter".to_string()]);

        // Compile
        let allow_unresolved_runtime_fallback = !matches!(self.checker_mode, TypeSystemMode::Raya);
        let js_compat_lowering = !matches!(self.checker_mode, TypeSystemMode::Raya);
        let mut compiler = Compiler::new(type_ctx, &interner)
            .with_expr_types(check_result.expr_types)
            .with_type_annotation_types(check_result.type_annotation_types)
            .with_js_this_binding_compat(js_compat_lowering)
            .with_allow_unresolved_runtime_fallback(allow_unresolved_runtime_fallback);
        if let Some(ref jsx_opts) = self.jsx_options {
            compiler = compiler.with_jsx(jsx_opts.clone());
        }
        compiler = compiler.with_module_identity(module_name.clone());
        compiler = compiler.with_emit_generic_templates(true);
        compiler = compiler.with_ambient_builtin_globals(ambient_builtin_globals);

        let (mut bytecode, lowering_metadata) = compiler
            .compile_via_ir_with_lowering_metadata(&ast)
            .map_err(|e| ModuleCompileError::CompileError {
                path: path.clone(),
                source: e,
            })?;
        if debug_stages {
            eprintln!(
                "[module-compile] lower+codegen:done path={}",
                path.display()
            );
        }
        bytecode.metadata.name = module_name;
        self.populate_link_tables(
            &mut bytecode,
            path,
            &ast,
            &interner,
            &module_exports,
            &lowering_metadata.module_global_slots,
        )?;
        let encoded = bytecode.encode();
        bytecode = BytecodeModule::decode(&encoded).map_err(|e| ModuleCompileError::TypeError {
            path: path.clone(),
            message: format!("Failed to finalize module checksum: {e}"),
        })?;

        Ok((bytecode, module_exports))
    }

    /// Inject symbols from imported modules into the binder
    fn inject_imports(
        &mut self,
        ast: &AstModule,
        current_path: &Path,
        binder: &mut Binder<'_>,
        interner: &Interner,
        linked_user_offset: Option<usize>,
    ) -> ModuleCompileResult<()> {
        for stmt in &ast.statements {
            if let Statement::ImportDecl(import) = stmt {
                let specifier = interner.resolve(import.source.value).to_string();

                // Resolve the import path
                let Some(resolved_path) = self.resolve_import_path(&specifier, current_path)?
                else {
                    continue;
                };

                // Imported module not yet compiled (shouldn't happen with topo order).
                if !self.exports.has_module(&resolved_path) {
                    continue;
                }

                // Inject each imported specifier
                for spec in &import.specifiers {
                    match spec {
                        ImportSpecifier::Named { name, alias } => {
                            let import_name = interner.resolve(name.name).to_string();
                            let local_name = alias
                                .as_ref()
                                .map(|a| interner.resolve(a.name).to_string())
                                .unwrap_or_else(|| import_name.clone());
                            if linked_user_offset.is_some_and(|offset| {
                                Self::has_top_level_declaration_before_offset(
                                    ast,
                                    interner,
                                    &local_name,
                                    offset,
                                )
                            }) {
                                continue;
                            }

                            if let Some(exported) =
                                self.exports.resolve_symbol(&resolved_path, &import_name)
                            {
                                let imported_ty = binder
                                    .hydrate_imported_signature_type(&exported.type_signature);
                                if binder.needs_import_namespace_fallback(imported_ty) {
                                    return Err(ModuleCompileError::TypeError {
                                        path: current_path.to_path_buf(),
                                        message: format!(
                                            "Unresolved structural type signature for import '{}.{}' (local '{}'). Raya strict forbids dynamic fallback.",
                                            specifier, import_name, local_name
                                        ),
                                    });
                                }
                                if std::env::var("RAYA_DEBUG_IMPORT_TYPES").is_ok() {
                                    eprintln!(
                                        "[module-compiler] named import '{} as {}' from '{}' kind={:?} signature='{}' hydrated='{}'",
                                        import_name,
                                        local_name,
                                        specifier,
                                        exported.kind,
                                        exported.type_signature,
                                        binder.format_type_id(imported_ty)
                                    );
                                }
                                if matches!(
                                    exported.kind,
                                    SymbolKind::Class
                                        | SymbolKind::Interface
                                        | SymbolKind::TypeAlias
                                ) {
                                    binder.register_imported_named_type(&import_name, imported_ty);
                                    binder.register_imported_named_type(&local_name, imported_ty);
                                }
                                let symbol = Symbol {
                                    name: local_name,
                                    kind: exported.kind,
                                    ty: imported_ty,
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
                            // import * as ns from "./module" - create a structural namespace object.
                            let local_name = interner.resolve(alias.name).to_string();
                            if linked_user_offset.is_some_and(|offset| {
                                Self::has_top_level_declaration_before_offset(
                                    ast,
                                    interner,
                                    &local_name,
                                    offset,
                                )
                            }) {
                                continue;
                            }
                            let mut members = Vec::new();
                            if let Some(module_exports) = self.exports.get(&resolved_path) {
                                let mut export_names =
                                    module_exports.symbols.keys().cloned().collect::<Vec<_>>();
                                export_names.sort();
                                for export_name in export_names {
                                    if let Some(exported) = module_exports.symbols.get(&export_name)
                                    {
                                        if exported.scope != SymbolScope::Module {
                                            continue;
                                        }
                                        if matches!(
                                            exported.kind,
                                            SymbolKind::TypeAlias
                                                | SymbolKind::TypeParameter
                                                | SymbolKind::Interface
                                        ) {
                                            continue;
                                        }
                                        let member_ty = binder.hydrate_imported_signature_type(
                                            &exported.type_signature,
                                        );
                                        if binder.needs_import_namespace_fallback(member_ty) {
                                            return Err(ModuleCompileError::TypeError {
                                                path: current_path.to_path_buf(),
                                                message: format!(
                                                    "Unresolved structural type signature for namespace import '{}.*' member '{}'. Raya strict forbids dynamic fallback.",
                                                    specifier, export_name
                                                ),
                                            });
                                        }
                                        members.push((export_name, member_ty));
                                    }
                                }
                            }
                            let namespace_ty = binder.object_type_from_members(members);
                            let symbol = Symbol {
                                name: local_name,
                                kind: SymbolKind::Variable,
                                ty: namespace_ty,
                                flags: SymbolFlags {
                                    is_exported: false,
                                    is_const: true,
                                    is_async: false,
                                    is_readonly: true,
                                    is_imported: false,
                                },
                                scope_id: ScopeId(0),
                                span: Span::new(0, 0, 0, 0),
                                referenced: false,
                            };
                            let _ = binder.define_imported(symbol);
                        }
                        ImportSpecifier::Default(local) => {
                            // import foo from "./module" - look for default export
                            let local_name = interner.resolve(local.name).to_string();
                            if linked_user_offset.is_some_and(|offset| {
                                Self::has_top_level_declaration_before_offset(
                                    ast,
                                    interner,
                                    &local_name,
                                    offset,
                                )
                            }) {
                                continue;
                            }
                            if let Some(exported) =
                                self.exports.resolve_symbol(&resolved_path, "default")
                            {
                                let imported_ty = binder
                                    .hydrate_imported_signature_type(&exported.type_signature);
                                if matches!(
                                    exported.kind,
                                    SymbolKind::Class
                                        | SymbolKind::Interface
                                        | SymbolKind::TypeAlias
                                ) {
                                    binder.register_imported_named_type(&local_name, imported_ty);
                                }
                                if std::env::var("RAYA_DEBUG_IMPORT_TYPES").is_ok() {
                                    eprintln!(
                                        "[module-compiler] default import '{}' from '{}' signature='{}' hydrated='{}'",
                                        local_name,
                                        specifier,
                                        exported.type_signature,
                                        binder.format_type_id(imported_ty)
                                    );
                                }
                                if binder.needs_import_namespace_fallback(imported_ty) {
                                    return Err(ModuleCompileError::TypeError {
                                        path: current_path.to_path_buf(),
                                        message: format!(
                                            "Unresolved structural type signature for default import '{}.default' (local '{}'). Raya strict forbids dynamic fallback.",
                                            specifier, local_name
                                        ),
                                    });
                                }
                                let symbol = Symbol {
                                    name: local_name,
                                    kind: exported.kind,
                                    ty: imported_ty,
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
                                let import_name = symbol.name.clone();
                                let import_kind = symbol.kind;
                                let import_ty = symbol.ty;
                                let define_result = binder.define_imported(symbol);
                                if std::env::var("RAYA_DEBUG_IMPORT_TYPES").is_ok() {
                                    eprintln!(
                                        "[module-compiler] define default import '{}' kind={:?} ty='{}' result={}",
                                        import_name,
                                        import_kind,
                                        binder.format_type_id(import_ty),
                                        if define_result.is_ok() { "ok" } else { "err" }
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Extract exported symbols from a compiled module's symbol table
    fn extract_exports(
        &self,
        ast: &AstModule,
        path: &Path,
        module_name: &str,
        symbols: &crate::parser::checker::SymbolTable,
        interner: &Interner,
        type_ctx: &TypeContext,
    ) -> ModuleExports {
        let mut exports = ModuleExports::new(path.to_path_buf(), module_name.to_string());

        for symbol in symbols.get_exported_symbols() {
            let scope_kind = symbols.get_scope(symbol.scope_id).kind;
            let scope = Self::scope_kind_to_symbol_scope(scope_kind);
            exports.add_symbol(ExportedSymbol::from_symbol(
                symbol,
                module_name,
                scope,
                type_ctx,
            ));
        }

        for stmt in &ast.statements {
            match stmt {
                Statement::ExportDecl(ExportDecl::Named {
                    specifiers,
                    source: None,
                    ..
                }) => {
                    for specifier in specifiers {
                        let local_name = interner.resolve(specifier.name.name).to_string();
                        let exported_name = specifier
                            .alias
                            .as_ref()
                            .map(|ident| interner.resolve(ident.name).to_string())
                            .unwrap_or_else(|| local_name.clone());
                        if exports.has(&exported_name) {
                            continue;
                        }
                        let Some(symbol) = Self::resolve_exported_symbol(
                            ast,
                            interner,
                            symbols,
                            &local_name,
                            stmt.span().start,
                        ) else {
                            continue;
                        };
                        let scope = Self::scope_kind_to_symbol_scope(
                            symbols.get_scope(symbol.scope_id).kind,
                        );
                        exports.add_symbol(ExportedSymbol::with_alias(
                            symbol,
                            exported_name,
                            module_name,
                            scope,
                            type_ctx,
                        ));
                    }
                }
                Statement::ExportDecl(ExportDecl::Default { expression, .. }) => {
                    let Expression::Identifier(identifier) = expression.as_ref() else {
                        continue;
                    };
                    let local_name = interner.resolve(identifier.name).to_string();
                    let Some(symbol) = Self::resolve_exported_symbol(
                        ast,
                        interner,
                        symbols,
                        &local_name,
                        stmt.span().start,
                    ) else {
                        continue;
                    };
                    let scope =
                        Self::scope_kind_to_symbol_scope(symbols.get_scope(symbol.scope_id).kind);
                    exports.add_symbol(ExportedSymbol::with_alias(
                        symbol,
                        "default".to_string(),
                        module_name,
                        scope,
                        type_ctx,
                    ));
                }
                _ => {}
            }
        }

        exports
    }

    fn top_level_module_scope_id(symbols: &crate::parser::checker::SymbolTable) -> Option<ScopeId> {
        for idx in 0..symbols.scope_count() {
            let scope_id = ScopeId(idx as u32);
            let scope = symbols.get_scope(scope_id);
            if scope.kind == ScopeKind::Module && scope.parent == Some(ScopeId(0)) {
                return Some(scope_id);
            }
        }
        None
    }

    fn resolve_exported_symbol<'a>(
        ast: &AstModule,
        interner: &Interner,
        symbols: &'a crate::parser::checker::SymbolTable,
        local_name: &str,
        export_offset: usize,
    ) -> Option<&'a Symbol> {
        if Self::has_top_level_declaration_before_offset(ast, interner, local_name, export_offset) {
            if let Some(module_scope_id) = Self::top_level_module_scope_id(symbols) {
                if let Some(symbol) = symbols.resolve_from_scope(local_name, module_scope_id) {
                    if symbols.get_scope(symbol.scope_id).kind == ScopeKind::Module {
                        return Some(symbol);
                    }
                }
            }
        }
        symbols.resolve(local_name)
    }

    fn scope_kind_to_symbol_scope(kind: ScopeKind) -> SymbolScope {
        match kind {
            ScopeKind::Global => SymbolScope::Global,
            ScopeKind::Module => SymbolScope::Module,
            ScopeKind::Function | ScopeKind::Block | ScopeKind::Class | ScopeKind::Loop => {
                SymbolScope::Local
            }
        }
    }

    fn collect_pattern_binding_names(
        pattern: &Pattern,
        interner: &Interner,
        out: &mut Vec<String>,
    ) {
        match pattern {
            Pattern::Identifier(ident) => out.push(interner.resolve(ident.name).to_string()),
            Pattern::Array(array) => {
                for element in &array.elements {
                    if let Some(element) = element {
                        Self::collect_pattern_binding_names(&element.pattern, interner, out);
                    }
                }
                if let Some(rest) = &array.rest {
                    Self::collect_pattern_binding_names(rest, interner, out);
                }
            }
            Pattern::Object(object) => {
                for property in &object.properties {
                    Self::collect_pattern_binding_names(&property.value, interner, out);
                }
                if let Some(rest) = &object.rest {
                    out.push(interner.resolve(rest.name).to_string());
                }
            }
            Pattern::Rest(rest) => {
                Self::collect_pattern_binding_names(&rest.argument, interner, out);
            }
        }
    }

    fn top_level_declaration_stmt<'a>(stmt: &'a Statement) -> Option<&'a Statement> {
        match stmt {
            Statement::ExportDecl(ExportDecl::Declaration(inner)) => Some(inner.as_ref()),
            _ => Some(stmt),
        }
    }

    fn has_top_level_declaration_before_offset(
        ast: &AstModule,
        interner: &Interner,
        name: &str,
        offset: usize,
    ) -> bool {
        for stmt in &ast.statements {
            if stmt.span().start >= offset {
                continue;
            }
            let Some(stmt) = Self::top_level_declaration_stmt(stmt) else {
                continue;
            };
            match stmt {
                Statement::FunctionDecl(function) => {
                    if interner.resolve(function.name.name) == name {
                        return true;
                    }
                }
                Statement::ClassDecl(class) => {
                    if interner.resolve(class.name.name) == name {
                        return true;
                    }
                }
                Statement::VariableDecl(variable) => {
                    let mut names = Vec::new();
                    Self::collect_pattern_binding_names(&variable.pattern, interner, &mut names);
                    if names.iter().any(|candidate| candidate == name) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn record_late_link_symbol_requirement(
        &mut self,
        module_identity: &str,
        module_specifier: &str,
        exported: &ExportedSymbol,
    ) {
        let module_id = module_id_from_name(module_identity);
        let requirement =
            self.late_link_requirements
                .entry(module_id)
                .or_insert(LateLinkRequirement {
                    module_identity: module_identity.to_string(),
                    module_id,
                    declaration_path: PathBuf::new(),
                    source_kind: super::declaration::DeclarationSourceKind::DTs,
                    module_specifiers: Vec::new(),
                    symbols: Vec::new(),
                });

        if !module_specifier.is_empty()
            && !requirement
                .module_specifiers
                .iter()
                .any(|existing| existing == module_specifier)
        {
            requirement
                .module_specifiers
                .push(module_specifier.to_string());
        }

        let symbol_type = match exported.kind {
            crate::parser::checker::SymbolKind::Function => SymbolType::Function,
            crate::parser::checker::SymbolKind::Class
            | crate::parser::checker::SymbolKind::Interface => SymbolType::Class,
            crate::parser::checker::SymbolKind::Variable
            | crate::parser::checker::SymbolKind::EnumMember => SymbolType::Constant,
            _ => return,
        };

        if requirement
            .symbols
            .iter()
            .any(|symbol| symbol.symbol_id == exported.symbol_id)
        {
            return;
        }

        requirement.symbols.push(LateLinkSymbolRequirement {
            symbol: exported.name.clone(),
            symbol_id: exported.symbol_id,
            scope: exported.scope,
            symbol_type,
            signature_hash: exported.signature_hash,
            type_signature: exported.type_signature.clone(),
            specialization_template: specialization_template_from_symbol(&exported.name),
        });
    }

    fn populate_link_tables(
        &mut self,
        bytecode: &mut BytecodeModule,
        current_path: &Path,
        ast: &AstModule,
        interner: &Interner,
        module_exports: &ModuleExports,
        module_global_slots: &HashMap<String, u32>,
    ) -> ModuleCompileResult<()> {
        bytecode.exports.clear();
        bytecode.imports.clear();

        // Export table: map exported symbols to runtime bytecode indices where available.
        for exported in module_exports.symbols.values() {
            let symbol_type = match exported.kind {
                crate::parser::checker::SymbolKind::Function => Some(SymbolType::Function),
                crate::parser::checker::SymbolKind::Class
                | crate::parser::checker::SymbolKind::Interface => Some(SymbolType::Class),
                crate::parser::checker::SymbolKind::Variable
                | crate::parser::checker::SymbolKind::EnumMember => Some(SymbolType::Constant),
                _ => None,
            };
            let Some(symbol_type) = symbol_type else {
                continue;
            };

            let index = match symbol_type {
                SymbolType::Function => bytecode
                    .functions
                    .iter()
                    .position(|f| f.name == exported.local_name),
                SymbolType::Class => bytecode
                    .classes
                    .iter()
                    .position(|c| c.name == exported.local_name),
                SymbolType::Constant => module_global_slots
                    .get(&exported.local_name)
                    .copied()
                    .map(|slot| slot as usize),
            };
            let Some(index) = index else {
                continue;
            };

            bytecode.exports.push(Export {
                name: exported.name.clone(),
                symbol_type: symbol_type.clone(),
                index,
                symbol_id: exported.symbol_id,
                scope: exported.scope,
                signature_hash: exported.signature_hash,
                type_signature: Some(exported.type_signature.clone()),
                runtime_global_slot: module_global_slots.get(&exported.local_name).copied(),
                nominal_type: matches!(symbol_type, SymbolType::Class).then_some(
                    NominalTypeExport {
                        local_nominal_type_index: index as u32,
                        constructor_function_index: bytecode
                            .functions
                            .iter()
                            .position(|function| {
                                function.name == format!("{}::constructor", exported.local_name)
                            })
                            .map(|idx| idx as u32),
                    },
                ),
            });
        }

        // Import table: capture named/default imports with deterministic target IDs.
        for stmt in &ast.statements {
            match stmt {
                Statement::ImportDecl(import_decl) => {
                    let specifier = interner.resolve(import_decl.source.value).to_string();
                    let Some(resolved_path) = self.resolve_import_path(&specifier, current_path)?
                    else {
                        continue;
                    };
                    let target_module_name = self.module_identity(&resolved_path);
                    let target_module_id = module_id_from_name(&target_module_name);
                    let declaration_target = self.declaration_modules.contains_key(&resolved_path);

                    for spec in &import_decl.specifiers {
                        match spec {
                            ImportSpecifier::Named { name, alias } => {
                                let import_name = interner.resolve(name.name).to_string();
                                let alias_name =
                                    alias.as_ref().map(|a| interner.resolve(a.name).to_string());
                                let local_name =
                                    alias_name.clone().unwrap_or_else(|| import_name.clone());
                                let exported = self
                                    .exports
                                    .resolve_symbol(&resolved_path, &import_name)
                                    .ok_or_else(|| ModuleCompileError::TypeError {
                                        path: current_path.to_path_buf(),
                                        message: format!(
                                            "Unresolved import '{}.{}' while emitting binary link metadata",
                                            target_module_name, import_name
                                        ),
                                    })?
                                    .clone();
                                if matches!(
                                    exported.kind,
                                    crate::parser::checker::SymbolKind::TypeAlias
                                        | crate::parser::checker::SymbolKind::TypeParameter
                                ) {
                                    continue;
                                }
                                if declaration_target {
                                    self.record_late_link_symbol_requirement(
                                        &target_module_name,
                                        &specifier,
                                        &exported,
                                    );
                                }
                                bytecode.imports.push(Import {
                                    module_specifier: specifier.clone(),
                                    symbol: import_name,
                                    alias: alias_name,
                                    module_id: target_module_id,
                                    symbol_id: exported.symbol_id,
                                    scope: SymbolScope::Module,
                                    signature_hash: exported.signature_hash,
                                    type_signature: Some(exported.type_signature.clone()),
                                    runtime_global_slot: module_global_slots
                                        .get(&local_name)
                                        .copied()
                                        .map(|slot| slot as u32),
                                });
                            }
                            ImportSpecifier::Default(local) => {
                                let local_name = interner.resolve(local.name).to_string();
                                let default_name = "default".to_string();
                                let exported = self
                                    .exports
                                    .resolve_symbol(&resolved_path, "default")
                                    .ok_or_else(|| ModuleCompileError::TypeError {
                                        path: current_path.to_path_buf(),
                                        message: format!(
                                            "Unresolved default import '{}.default' while emitting binary link metadata",
                                            target_module_name
                                        ),
                                    })?
                                    .clone();
                                if matches!(
                                    exported.kind,
                                    crate::parser::checker::SymbolKind::TypeAlias
                                        | crate::parser::checker::SymbolKind::TypeParameter
                                ) {
                                    continue;
                                }
                                if declaration_target {
                                    self.record_late_link_symbol_requirement(
                                        &target_module_name,
                                        &specifier,
                                        &exported,
                                    );
                                }
                                bytecode.imports.push(Import {
                                    module_specifier: specifier.clone(),
                                    symbol: default_name,
                                    alias: Some(local_name),
                                    module_id: target_module_id,
                                    symbol_id: exported.symbol_id,
                                    scope: SymbolScope::Module,
                                    signature_hash: exported.signature_hash,
                                    type_signature: Some(exported.type_signature.clone()),
                                    runtime_global_slot: module_global_slots
                                        .get(interner.resolve(local.name))
                                        .copied()
                                        .map(|slot| slot as u32),
                                });
                            }
                            ImportSpecifier::Namespace(alias) => {
                                let Some(target_exports) = self.exports.get(&resolved_path) else {
                                    return Err(ModuleCompileError::TypeError {
                                        path: current_path.to_path_buf(),
                                        message: format!(
                                            "Missing export metadata for namespace import '{}'",
                                            specifier
                                        ),
                                    });
                                };
                                let (namespace_hash, namespace_signature) =
                                    Self::namespace_contract_from_exports(target_exports);
                                let alias_name = interner.resolve(alias.name).to_string();
                                let namespace_symbol = "*".to_string();
                                bytecode.imports.push(Import {
                                    module_specifier: specifier.clone(),
                                    symbol: namespace_symbol.clone(),
                                    alias: Some(alias_name),
                                    module_id: target_module_id,
                                    // Namespace imports are resolved by module_id at link/hydration time.
                                    symbol_id: 0,
                                    scope: SymbolScope::Module,
                                    signature_hash: namespace_hash,
                                    type_signature: Some(namespace_signature),
                                    runtime_global_slot: module_global_slots
                                        .get(interner.resolve(alias.name))
                                        .copied()
                                        .map(|slot| slot as u32),
                                });
                            }
                        }
                    }
                }
                Statement::ExportDecl(ExportDecl::Named {
                    specifiers,
                    source: Some(source),
                    ..
                }) => {
                    let specifier = interner.resolve(source.value).to_string();
                    let Some(resolved_path) = self.resolve_import_path(&specifier, current_path)?
                    else {
                        continue;
                    };
                    let target_module_name = self.module_identity(&resolved_path);
                    let target_module_id = module_id_from_name(&target_module_name);
                    let declaration_target = self.declaration_modules.contains_key(&resolved_path);

                    for spec in specifiers {
                        let import_name = interner.resolve(spec.name.name).to_string();
                        let alias_name = spec
                            .alias
                            .as_ref()
                            .map(|a| interner.resolve(a.name).to_string());
                        let exported = self
                            .exports
                            .resolve_symbol(&resolved_path, &import_name)
                            .ok_or_else(|| ModuleCompileError::TypeError {
                                path: current_path.to_path_buf(),
                                message: format!(
                                    "Unresolved re-export '{}.{}' while emitting binary link metadata",
                                    target_module_name, import_name
                                ),
                            })?
                            .clone();
                        if matches!(
                            exported.kind,
                            crate::parser::checker::SymbolKind::TypeAlias
                                | crate::parser::checker::SymbolKind::TypeParameter
                        ) {
                            continue;
                        }
                        if declaration_target {
                            self.record_late_link_symbol_requirement(
                                &target_module_name,
                                &specifier,
                                &exported,
                            );
                        }
                        bytecode.imports.push(Import {
                            module_specifier: specifier.clone(),
                            symbol: import_name,
                            alias: alias_name,
                            module_id: target_module_id,
                            symbol_id: exported.symbol_id,
                            scope: SymbolScope::Module,
                            signature_hash: exported.signature_hash,
                            type_signature: Some(exported.type_signature.clone()),
                            runtime_global_slot: None,
                        });
                    }
                }
                Statement::ExportDecl(ExportDecl::All { source, .. }) => {
                    let specifier = interner.resolve(source.value).to_string();
                    let Some(resolved_path) = self.resolve_import_path(&specifier, current_path)?
                    else {
                        continue;
                    };
                    let Some(target_exports) = self.exports.get(&resolved_path) else {
                        return Err(ModuleCompileError::TypeError {
                            path: current_path.to_path_buf(),
                            message: format!(
                                "Missing export metadata for namespace re-export '{}'",
                                specifier
                            ),
                        });
                    };
                    let (namespace_hash, namespace_signature) =
                        Self::namespace_contract_from_exports(target_exports);
                    let target_module_name = self.module_identity(&resolved_path);
                    let target_module_id = module_id_from_name(&target_module_name);
                    let namespace_symbol = "*".to_string();
                    bytecode.imports.push(Import {
                        module_specifier: specifier,
                        symbol: namespace_symbol.clone(),
                        alias: None,
                        module_id: target_module_id,
                        symbol_id: symbol_id_from_name(
                            &target_module_name,
                            SymbolScope::Module,
                            &namespace_symbol,
                        ),
                        scope: SymbolScope::Module,
                        signature_hash: namespace_hash,
                        type_signature: Some(namespace_signature),
                        runtime_global_slot: None,
                    });
                }
                _ => {}
            }
        }

        Ok(())
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
        self.virtual_sources.clear();
        self.declaration_modules.clear();
        self.declaration_virtual_by_identity.clear();
        self.late_link_requirements.clear();
    }

    /// Get the export registry
    pub fn exports(&self) -> &ExportRegistry {
        &self.exports
    }

    /// Get unresolved late-link requirements collected from declaration-backed imports.
    pub fn late_link_requirements(&self) -> Vec<LateLinkRequirement> {
        let mut requirements = self
            .late_link_requirements
            .values()
            .cloned()
            .collect::<Vec<_>>();
        requirements.sort_by_key(|req| req.module_id);
        for requirement in &mut requirements {
            requirement.module_specifiers.sort();
            requirement.module_specifiers.dedup();
            requirement
                .symbols
                .sort_by(|a, b| a.symbol_id.cmp(&b.symbol_id));
        }
        requirements
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
               let x: number = value + 1;"#, // Uses imported `value`
        )
        .unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let result = compiler.compile(&main_path);

        assert!(result.is_ok(), "Compilation failed: {:?}", result.err());
        let compiled = result.unwrap();

        // Should have 2 modules: utils first, then main
        assert_eq!(compiled.len(), 2);

        // utils should be first (dependency)
        assert_eq!(compiled[0].path, utils_path.canonicalize().unwrap());
        assert!(!compiled[0].bytecode.metadata.name.is_empty());

        // main should be second
        assert_eq!(compiled[1].path, main_path.canonicalize().unwrap());
        assert!(
            !compiled[1].bytecode.imports.is_empty(),
            "entry module should emit import metadata"
        );
        assert!(compiled[1].bytecode.imports[0].module_id != 0);
        assert!(compiled[1].bytecode.imports[0].symbol_id != 0);
    }

    #[test]
    fn test_import_metadata_emits_runtime_global_slot() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");
        let utils_path = temp_dir.path().join("utils.raya");

        fs::write(
            &utils_path,
            "export function inc(x: number): number { return x + 1; }",
        )
        .unwrap();
        fs::write(
            &main_path,
            r#"
            import { inc as plusOne } from "./utils";
            return plusOne(1);
            "#,
        )
        .unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let compiled = compiler.compile(&main_path).expect("compile");
        let entry = compiled
            .iter()
            .find(|module| module.path == main_path.canonicalize().unwrap())
            .expect("entry module");
        let import = entry
            .bytecode
            .imports
            .iter()
            .find(|import| import.module_specifier == "./utils" && import.symbol == "inc")
            .expect("inc import");

        assert_eq!(import.runtime_global_slot, Some(0));
    }

    #[test]
    fn test_constant_exports_are_emitted_in_link_table() {
        let temp_dir = create_test_project();
        let utils_path = temp_dir.path().join("utils.raya");

        fs::write(&utils_path, "export const answer: number = 42;").unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let compiled = compiler.compile(&utils_path).expect("compile");
        let exported_names = compiler
            .exports()
            .get(&utils_path.canonicalize().unwrap())
            .map(|exports| {
                let mut names = exports.symbols.keys().cloned().collect::<Vec<_>>();
                names.sort();
                names
            })
            .unwrap_or_default();
        let utils = compiled
            .iter()
            .find(|module| module.path == utils_path.canonicalize().unwrap())
            .expect("utils module");
        let export = utils
            .bytecode
            .exports
            .iter()
            .find(|export| export.name == "answer")
            .expect("constant export");

        assert_eq!(export.symbol_type, SymbolType::Constant);
        assert!(export.signature_hash != 0);
    }

    #[test]
    fn test_default_export_expression_emits_link_table_entry() {
        let temp_dir = create_test_project();
        let utils_path = temp_dir.path().join("utils.raya");

        fs::write(&utils_path, "export default { answer: 42 };").unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let compiled = compiler.compile(&utils_path).expect("compile");
        let utils = compiled
            .iter()
            .find(|module| module.path == utils_path.canonicalize().unwrap())
            .expect("utils module");
        let export = utils
            .bytecode
            .exports
            .iter()
            .find(|export| export.name == "default")
            .expect("default export");

        assert_eq!(export.symbol_type, SymbolType::Constant);
        assert_eq!(export.index, 0);
        assert!(export.signature_hash != 0);
    }

    #[test]
    fn test_default_export_local_class_emits_link_table_entry() {
        let temp_dir = create_test_project();
        let utils_path = temp_dir.path().join("utils.raya");

        fs::write(
            &utils_path,
            r#"
            class Foo {}
            export { Foo };
            export default Foo;
            "#,
        )
        .unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let compiled = compiler.compile(&utils_path).expect("compile");
        let exported_names = compiler
            .exports()
            .get(&utils_path.canonicalize().unwrap())
            .map(|exports| {
                let mut names = exports.symbols.keys().cloned().collect::<Vec<_>>();
                names.sort();
                names
            })
            .unwrap_or_default();
        let utils = compiled
            .iter()
            .find(|module| module.path == utils_path.canonicalize().unwrap())
            .expect("utils module");
        let export = utils
            .bytecode
            .exports
            .iter()
            .find(|export| export.name == "default")
            .unwrap_or_else(|| {
                panic!(
                    "missing default export; module_exports={:?} class table={:?} exports={:?}",
                    exported_names,
                    utils
                        .bytecode
                        .classes
                        .iter()
                        .map(|class| class.name.clone())
                        .collect::<Vec<_>>(),
                    utils
                        .bytecode
                        .exports
                        .iter()
                        .map(|export| {
                            (
                                export.name.clone(),
                                export.symbol_type.clone(),
                                export.index,
                            )
                        })
                        .collect::<Vec<_>>()
                )
            });

        assert_eq!(export.symbol_type, SymbolType::Class);
        assert!(utils.bytecode.classes.get(export.index).is_some());
    }

    #[test]
    fn test_local_class_can_shadow_builtin_global() {
        let temp_dir = create_test_project();
        let utils_path = temp_dir.path().join("utils.raya");

        fs::write(
            &utils_path,
            r#"
            class Buffer {}
            export { Buffer };
            export default Buffer;
            "#,
        )
        .unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let compiled = compiler.compile(&utils_path).expect("compile");
        let utils = compiled
            .iter()
            .find(|module| module.path == utils_path.canonicalize().unwrap())
            .expect("utils module");

        assert!(utils
            .bytecode
            .exports
            .iter()
            .any(|export| { export.name == "Buffer" && export.symbol_type == SymbolType::Class }));
        assert!(utils
            .bytecode
            .exports
            .iter()
            .any(|export| { export.name == "default" && export.symbol_type == SymbolType::Class }));
    }

    #[test]
    fn test_binary_module_compiler_supports_std_and_node_imports() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");

        fs::write(
            &main_path,
            r#"
            import { join } from "std:path";
            import { ParsedPath } from "node:path";
            let p = join;
            let np = ParsedPath;
            return 1;
            "#,
        )
        .unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let compiled = compiler.compile(&main_path).expect("compile");

        assert!(
            compiled.len() >= 3,
            "expected entry + std + node-linked modules, got {}",
            compiled.len()
        );
    }

    #[test]
    fn test_binary_module_compiler_supports_std_default_import() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");

        fs::write(
            &main_path,
            r#"
            import path from "std:path";
            let p = path;
            return 1;
            "#,
        )
        .unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let compiled = compiler.compile(&main_path).expect("compile");
        let entry = compiled
            .iter()
            .find(|module| module.path == main_path.canonicalize().unwrap())
            .expect("entry module");

        assert!(entry
            .bytecode
            .imports
            .iter()
            .any(|import| import.module_specifier == "std:path" && import.symbol == "default"));
    }

    #[test]
    fn test_strict_default_import_rejects_unknown_signature_without_dynamic_fallback() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");
        let dep_decl_path = temp_dir.path().join("dep.d.ts");

        fs::write(
            &main_path,
            r#"
            import dep from "./dep";
            return dep;
            "#,
        )
        .unwrap();
        fs::write(
            &dep_decl_path,
            "export const dep: unknown = null; export default dep;",
        )
        .unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let result = compiler.compile(&main_path);

        match result {
            Err(ModuleCompileError::TypeError { message, .. }) => {
                assert!(
                    message.contains("Raya strict forbids dynamic fallback"),
                    "expected strict no-fallback diagnostic, got: {message}"
                );
            }
            other => panic!(
                "expected TypeError for unknown default import signature, got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_strict_named_import_rejects_unknown_signature_without_dynamic_fallback() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");
        let dep_decl_path = temp_dir.path().join("dep.d.ts");

        fs::write(
            &main_path,
            r#"
            import { dep } from "./dep";
            return dep;
            "#,
        )
        .unwrap();
        fs::write(&dep_decl_path, "export const dep: unknown = null;").unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let result = compiler.compile(&main_path);

        match result {
            Err(ModuleCompileError::TypeError { message, .. }) => {
                assert!(
                    message.contains("Raya strict forbids dynamic fallback"),
                    "expected strict no-fallback diagnostic, got: {message}"
                );
            }
            other => panic!(
                "expected TypeError for unknown named import signature, got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_strict_namespace_import_rejects_unknown_member_signature_without_dynamic_fallback() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");
        let dep_decl_path = temp_dir.path().join("dep.d.ts");

        fs::write(
            &main_path,
            r#"
            import * as depNs from "./dep";
            return depNs.dep;
            "#,
        )
        .unwrap();
        fs::write(&dep_decl_path, "export const dep: unknown = null;").unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let result = compiler.compile(&main_path);

        match result {
            Err(ModuleCompileError::TypeError { message, .. }) => {
                assert!(
                    message.contains("Raya strict forbids dynamic fallback"),
                    "expected strict no-fallback diagnostic, got: {message}"
                );
            }
            other => panic!(
                "expected TypeError for unknown namespace member signature, got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_namespace_import_emits_structural_signature_metadata() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");
        let dep_path = temp_dir.path().join("dep.raya");

        fs::write(
            &main_path,
            r#"
            import * as depNs from "./dep";
            return depNs.answer;
            "#,
        )
        .unwrap();
        fs::write(&dep_path, "export const answer: number = 42;").unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let compiled = compiler
            .compile(&main_path)
            .expect("compile namespace import");
        let main_module = compiled
            .iter()
            .find(|module| module.path == main_path.canonicalize().unwrap())
            .expect("main module");
        let namespace_import = main_module
            .bytecode
            .imports
            .iter()
            .find(|import| import.symbol == "*")
            .expect("namespace import metadata");
        assert_ne!(namespace_import.signature_hash, 0);
        assert_eq!(
            namespace_import.type_signature.as_deref(),
            Some("obj(prop:answer:ro:req:number)")
        );
    }

    #[test]
    fn test_strict_named_imported_class_static_call_has_no_late_bound_fallback() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");
        let dep_path = temp_dir.path().join("dep.raya");

        fs::write(
            &dep_path,
            r#"
            export class AppCommon {
                static answer(): number {
                    return 42;
                }
            }
            "#,
        )
        .unwrap();
        fs::write(
            &main_path,
            r#"
            import { AppCommon } from "./dep";
            return AppCommon.answer();
            "#,
        )
        .unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let result = compiler.compile(&main_path);
        assert!(
            result.is_ok(),
            "strict named imported class static call should compile: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_cross_module_function_import() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");
        let utils_path = temp_dir.path().join("utils.raya");

        // Test importing and calling a function symbol.
        fs::write(
            &utils_path,
            r#"export function add(a: number, b: number): number {
            return a + b;
        }"#,
        )
        .unwrap();
        fs::write(
            &main_path,
            r#"import { add } from "./utils";
               let x: number = add(1, 2);"#,
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

        fs::write(
            &a_path,
            r#"import { y } from "./b"; export let x: number = 1;"#,
        )
        .unwrap();
        fs::write(
            &b_path,
            r#"import { x } from "./a"; export let y: number = 2;"#,
        )
        .unwrap();

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
        fs::write(
            &a_path,
            r#"import { value } from "./shared"; export let a: number = value + 1;"#,
        )
        .unwrap();
        fs::write(
            &b_path,
            r#"import { value } from "./shared"; export let b: number = value + 2;"#,
        )
        .unwrap();
        fs::write(
            &main_path,
            r#"import { a } from "./a";
               import { b } from "./b";
               let result: number = a + b;"#, // Uses imported symbols
        )
        .unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let result = compiler.compile(&main_path);

        assert!(result.is_ok(), "Compilation failed: {:?}", result.err());
        let compiled = result.unwrap();

        // Should have 4 modules
        assert_eq!(compiled.len(), 4);

        // shared should be first
        assert_eq!(compiled[0].path, shared_path.canonicalize().unwrap());

        // main should be last
        assert_eq!(compiled[3].path, main_path.canonicalize().unwrap());
    }

    #[test]
    fn test_module_not_found() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");

        fs::write(&main_path, r#"import { foo } from "./nonexistent";"#).unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let result = compiler.compile(&main_path);

        assert!(matches!(result, Err(ModuleCompileError::Resolution(_))));
    }

    #[test]
    fn test_declaration_import_uses_d_ts_when_source_missing() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");
        let dep_decl_path = temp_dir.path().join("dep.d.ts");

        fs::write(
            &main_path,
            r#"
            import { foo } from "./dep";
            let x: number = 1;
            "#,
        )
        .unwrap();
        fs::write(&dep_decl_path, "export function foo(a: number): number;").unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let compiled = compiler.compile(&main_path).expect("compile");
        let entry = compiled
            .iter()
            .find(|module| module.path == main_path.canonicalize().unwrap())
            .expect("entry");
        assert!(compiled.iter().any(|module| module.declaration_only));
        assert!(
            entry
                .bytecode
                .imports
                .iter()
                .any(|import| import.module_specifier == "./dep" && import.symbol == "foo"),
            "entry import metadata should include declaration-backed import"
        );

        let late_links = compiler.late_link_requirements();
        assert_eq!(late_links.len(), 1);
        assert!(late_links[0]
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "foo"));
    }

    #[test]
    fn test_declaration_import_falls_back_to_d_ts() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");
        let dep_decl_path = temp_dir.path().join("dep.d.ts");

        fs::write(
            &main_path,
            r#"
            import { foo } from "./dep";
            let x: number = 1;
            "#,
        )
        .unwrap();
        fs::write(&dep_decl_path, "export function foo(a: string): string;").unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let compiled = compiler.compile(&main_path).expect("compile");
        assert!(compiled.iter().any(|module| module.declaration_only));

        let late_links = compiler.late_link_requirements();
        assert_eq!(late_links.len(), 1);
        assert_eq!(late_links[0].source_kind, DeclarationSourceKind::DTs);
    }

    #[test]
    fn test_declaration_import_records_d_ts_contract() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");
        let dep_d_ts = temp_dir.path().join("dep.d.ts");

        fs::write(
            &main_path,
            r#"
            import { foo } from "./dep";
            let x: number = 1;
            "#,
        )
        .unwrap();
        fs::write(&dep_d_ts, "export function foo(a: string): string;").unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let compiled = compiler.compile(&main_path).expect("compile");
        let entry = compiled
            .iter()
            .find(|module| module.path == main_path.canonicalize().unwrap())
            .expect("entry");
        let foo_import = entry
            .bytecode
            .imports
            .iter()
            .find(|import| import.module_specifier == "./dep" && import.symbol == "foo")
            .expect("foo import");
        let type_signature = foo_import.type_signature.as_ref().expect("type signature");
        assert!(
            type_signature.contains("string"),
            "expected .d.ts function signature, got: {}",
            type_signature
        );

        let late_links = compiler.late_link_requirements();
        assert_eq!(late_links.len(), 1);
        assert_eq!(late_links[0].source_kind, DeclarationSourceKind::DTs);
    }

    #[test]
    fn test_unsupported_d_ts_syntax_reports_deterministic_diagnostic() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");
        let dep_decl_path = temp_dir.path().join("dep.d.ts");

        fs::write(
            &main_path,
            r#"
            import { Mode } from "./dep";
            return 1;
            "#,
        )
        .unwrap();
        fs::write(&dep_decl_path, "export enum Mode { A, B }").unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let err = compiler.compile(&main_path).unwrap_err();
        let message = format!("{err}");
        assert!(
            message.contains("Unsupported .d.ts syntax at line 1"),
            "unexpected diagnostic: {}",
            message
        );
    }

    #[test]
    fn test_reexport_dependency_is_discovered_and_recorded() {
        let temp_dir = create_test_project();
        let main_path = temp_dir.path().join("main.raya");
        let a_path = temp_dir.path().join("a.raya");
        let b_path = temp_dir.path().join("b.raya");

        fs::write(&b_path, "export let value: number = 42;").unwrap();
        fs::write(&a_path, r#"export * from "./b";"#).unwrap();
        fs::write(
            &main_path,
            r#"
            import { value } from "./a";
            let x: number = 0;
            "#,
        )
        .unwrap();

        let mut compiler = ModuleCompiler::new(temp_dir.path().to_path_buf());
        let result = compiler.compile(&main_path);

        assert!(result.is_ok(), "Compilation failed: {:?}", result.err());
        let compiled = result.unwrap();
        assert_eq!(
            compiled.len(),
            3,
            "re-export edge should include ./b module"
        );

        let a_module = compiled
            .iter()
            .find(|m| m.path == a_path.canonicalize().unwrap())
            .expect("missing a.raya module");
        assert!(
            a_module
                .bytecode
                .imports
                .iter()
                .any(|import| import.module_specifier == "./b" && import.symbol == "*"),
            "re-exporting module should emit import metadata for export * source"
        );
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
