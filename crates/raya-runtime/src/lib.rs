//! Raya Runtime
//!
//! The primary API for compiling, loading, and executing Raya code.
//!
//! # Example
//!
//! ```rust,ignore
//! use raya_runtime::{Runtime, RuntimeOptions};
//! use std::path::Path;
//!
//! // Simple: run a file
//! let rt = Runtime::new();
//! rt.run_file(Path::new("app.raya"))?;
//!
//! // Evaluate inline code
//! let value = rt.eval("return 1 + 2;")?;
//!
//! // With options
//! let rt = Runtime::with_options(RuntimeOptions {
//!     threads: 4,
//!     heap_limit: 512 * 1024 * 1024,
//!     ..Default::default()
//! });
//! rt.run_file(Path::new("server.raya"))?;
//! ```

mod builtin_manifest;
mod builtins;
pub mod bundle;
pub mod compile;
pub mod deps;
pub mod error;
pub mod loader;
pub mod module_system;
pub mod session;
pub mod test_runner;
mod vm_setup;

// Re-export key types from raya-engine for convenience
pub use crate::compile::TsCompilerOptions;
pub use crate::compile::TypeMode;
pub use raya_engine::compiler::Module;
pub use raya_engine::vm::Value;

// Backward-compatible re-exports
pub use raya_stdlib::StdNativeHandler;
pub use raya_stdlib_posix::register_posix;

pub use error::RuntimeError;
pub use module_system::{CompiledProgram, ProgramDiagnostics};
pub use session::Session;

use raya_engine::compiler::module::{
    builtin_global_exports, BuiltinSurfaceMode, LateLinkRequirement, LateLinkSymbolRequirement,
};
use raya_engine::compiler::{
    module_id_from_name, symbol_id_from_name, Export, Import, NominalTypeExport, SymbolScope,
    SymbolType,
};
use raya_engine::parser::ast::{Pattern, Statement};
use raya_engine::parser::checker::SymbolKind;
use raya_engine::parser::types::{
    signature_hash, structural_signature_is_assignable, try_hydrate_type_from_canonical_signature,
    Type, TypeContext,
};
use raya_engine::parser::{Interner, Parser};
use raya_engine::vm::json::JSView;
use raya_engine::vm::module::{ModuleLinker, ResolvedSymbol};
use raya_engine::vm::object::{
    layout_id_from_ordered_names, Closure, Object, RayaString, TypeHandle,
};
#[cfg(feature = "aot")]
use raya_engine::aot::{
    allocate_initial_frame, build_task_context, install_registered_aot_functions, run_aot_function,
    dispatch_registered_aot_entry, AotEntryFn, AotRunResult, RegisteredAotClone,
    RegisteredAotFunctionEntry,
};
#[cfg(feature = "aot")]
use raya_engine::vm::scheduler::{Task, TaskState};
#[cfg(feature = "aot")]
use raya_engine::vm::VmError;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::OnceLock;

const IMPORTED_CLASS_TYPE_HANDLE_KEY: &str = "__raya_type_handle__";

static STRICT_BUILTIN_RUNTIME_MODULES: OnceLock<Result<Vec<Module>, String>> = OnceLock::new();
static NODE_BUILTIN_RUNTIME_MODULES: OnceLock<Result<Vec<Module>, String>> = OnceLock::new();

// ────────────────────────────────────────────────────────────────────────────

/// Configuration for the Raya runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuiltinMode {
    /// Raya-first builtin surface.
    /// Promise-first async and core builtins are always enabled.
    /// Excludes JS legacy object meta-programming APIs.
    #[default]
    RayaStrict,
    /// Enables JS legacy object meta-programming APIs (Object.define* descriptor APIs).
    /// Does not alter Promise/channel/mutex core builtin behavior.
    NodeCompat,
}

/// Configuration for the Raya runtime.
#[derive(Debug, Clone)]
pub struct RuntimeOptions {
    /// Worker thread count (0 = auto-detect from CPU count).
    pub threads: usize,
    /// Maximum heap size in bytes (0 = unlimited).
    pub heap_limit: usize,
    /// Execution timeout in milliseconds (0 = unlimited).
    pub timeout: u64,
    /// Disable JIT compilation (interpreter only).
    pub no_jit: bool,
    /// JIT adaptive compilation call threshold.
    pub jit_threshold: u32,
    /// Enable CPU profiling and write output to this path.
    /// None = profiling disabled. Format is inferred from extension:
    /// `.cpuprofile` → Chrome DevTools JSON, anything else → folded stacks.
    pub cpu_prof: Option<std::path::PathBuf>,
    /// Profiling sample interval in microseconds (default: 10_000 = 10ms / 100Hz).
    pub prof_interval_us: u64,
    /// Builtin API mode (strict Raya vs node-compat surface).
    pub builtin_mode: BuiltinMode,
    /// Optional type-system mode override.
    /// None = inferred from builtin mode.
    pub type_mode: Option<TypeMode>,
    /// Optional TS compiler options payload for `TypeMode::Ts`.
    pub ts_options: Option<TsCompilerOptions>,
}

impl Default for RuntimeOptions {
    fn default() -> Self {
        Self {
            threads: 0,
            heap_limit: 0,
            timeout: 0,
            no_jit: false,
            jit_threshold: 1000,
            cpu_prof: None,
            prof_interval_us: 10_000,
            builtin_mode: BuiltinMode::RayaStrict,
            type_mode: None,
            ts_options: None,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────

/// A compiled module ready for execution.
#[allow(dead_code)]
pub struct CompiledModule {
    /// The bytecode module.
    pub(crate) module: Module,
    /// String interner (present when compiled from source, None for .ryb loads).
    pub(crate) interner: Option<Interner>,
}

impl CompiledModule {
    /// Serialize to .ryb bytecode bytes.
    pub fn encode(&self) -> Vec<u8> {
        self.module.encode()
    }

    /// Module name from metadata.
    pub fn name(&self) -> &str {
        &self.module.metadata.name
    }

    /// Access the underlying bytecode module.
    pub fn module(&self) -> &Module {
        &self.module
    }
}

// ────────────────────────────────────────────────────────────────────────────

/// The Raya runtime — compiles, loads, and executes Raya code.
///
/// # Example
///
/// ```rust,ignore
/// let rt = Runtime::new();
/// let exit_code = rt.run_file(Path::new("app.raya"))?;
/// ```
pub struct Runtime {
    options: RuntimeOptions,
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

impl Runtime {
    fn is_source_file_extension(extension: &str) -> bool {
        matches!(
            extension,
            "raya" | "js" | "mjs" | "cjs" | "ts" | "mts" | "cts" | "jsx" | "tsx"
        )
    }

    fn compile_program_source_with_virtual_entry(
        &self,
        source: &str,
        virtual_entry: &Path,
    ) -> Result<CompiledProgram, RuntimeError> {
        let type_mode = self
            .options
            .type_mode
            .unwrap_or_else(|| compile::default_type_mode_for_builtin(self.options.builtin_mode));
        let ts_options = self.resolve_ts_options_for_inline()?;
        let compiler = module_system::ProgramCompiler {
            builtin_mode: self.options.builtin_mode,
            type_mode,
            ts_options,
            compile_options: None,
        };
        compiler.compile_program_source(source, virtual_entry)
    }

    fn resolve_ts_options_for_inline(&self) -> Result<Option<TsCompilerOptions>, RuntimeError> {
        if !matches!(self.options.type_mode, Some(TypeMode::Ts)) {
            return Ok(self.options.ts_options.clone());
        }
        if let Some(opts) = &self.options.ts_options {
            return Ok(Some(opts.clone()));
        }
        let cwd = match std::env::current_dir() {
            Ok(path) => path,
            Err(_) => return Ok(Some(TsCompilerOptions::default())),
        };
        match loader::find_tsconfig(&cwd) {
            Some(tsconfig) => Ok(Some(loader::load_ts_compiler_options(&tsconfig)?)),
            None => Ok(Some(TsCompilerOptions::default())),
        }
    }

    fn resolve_ts_options_for_path(
        &self,
        path: &Path,
    ) -> Result<Option<TsCompilerOptions>, RuntimeError> {
        if !matches!(self.options.type_mode, Some(TypeMode::Ts)) {
            return Ok(self.options.ts_options.clone());
        }
        if let Some(opts) = &self.options.ts_options {
            return Ok(Some(opts.clone()));
        }
        let search_root = path.parent().unwrap_or(path);
        match loader::find_tsconfig(search_root) {
            Some(tsconfig) => Ok(Some(loader::load_ts_compiler_options(&tsconfig)?)),
            None => Ok(Some(TsCompilerOptions::default())),
        }
    }

    /// Create a runtime with default options.
    pub fn new() -> Self {
        Self {
            options: RuntimeOptions::default(),
        }
    }

    /// Create a runtime with custom options.
    pub fn with_options(options: RuntimeOptions) -> Self {
        Self { options }
    }

    /// Access the runtime options.
    pub fn options(&self) -> &RuntimeOptions {
        &self.options
    }

    // ── Compilation ──────────────────────────────────────────────────────

    /// Compile a Raya source string to a bytecode module.
    ///
    /// Uses inline compilation path for plain source and automatically routes
    /// to binary module-graph compilation when `std:`/`node:` imports are present.
    pub fn compile(&self, source: &str) -> Result<CompiledModule, RuntimeError> {
        let type_mode = self
            .options
            .type_mode
            .unwrap_or_else(|| compile::default_type_mode_for_builtin(self.options.builtin_mode));
        let ts_options = self.resolve_ts_options_for_inline()?;
        let (module, interner) = compile::compile_source_with_modes_and_ts_options(
            source,
            self.options.builtin_mode,
            type_mode,
            ts_options.as_ref(),
        )?;
        Ok(CompiledModule {
            module,
            interner: Some(interner),
        })
    }

    /// Compile a Raya source string into a full binary-linked program graph.
    ///
    /// Returns the entry module plus all compiled dependencies and late-link metadata.
    pub fn compile_program_source(&self, source: &str) -> Result<CompiledProgram, RuntimeError> {
        self.compile_program_source_with_virtual_entry(source, Path::new("<inline>.raya"))
    }

    /// Compile a source file to a bytecode module.
    pub fn compile_file(&self, path: &Path) -> Result<CompiledModule, RuntimeError> {
        Ok(self.compile_program_file(path)?.entry)
    }

    /// Compile a Raya source string with options (e.g., source map).
    pub fn compile_with_options(
        &self,
        source: &str,
        options: &compile::CompileOptions,
    ) -> Result<CompiledModule, RuntimeError> {
        let type_mode = self
            .options
            .type_mode
            .unwrap_or_else(|| compile::default_type_mode_for_builtin(self.options.builtin_mode));
        let ts_options = self.resolve_ts_options_for_inline()?;
        let (module, interner) = compile::compile_source_with_options_and_modes_and_ts_options(
            source,
            options,
            self.options.builtin_mode,
            type_mode,
            ts_options.as_ref(),
        )?;
        Ok(CompiledModule {
            module,
            interner: Some(interner),
        })
    }

    /// Compile a source file with options (e.g., source map).
    pub fn compile_file_with_options(
        &self,
        path: &Path,
        options: &compile::CompileOptions,
    ) -> Result<CompiledModule, RuntimeError> {
        Ok(self.compile_program_file_with_options(path, options)?.entry)
    }

    // ── Checking ─────────────────────────────────────────────────────────

    /// Type-check a Raya source string without generating bytecode.
    ///
    /// Returns diagnostics (errors + warnings) without compiling.
    pub fn check(&self, source: &str) -> Result<compile::CheckDiagnostics, RuntimeError> {
        let type_mode = self
            .options
            .type_mode
            .unwrap_or_else(|| compile::default_type_mode_for_builtin(self.options.builtin_mode));
        let ts_options = self.resolve_ts_options_for_inline()?;
        compile::check_source_with_modes_and_ts_options(
            source,
            self.options.builtin_mode,
            type_mode,
            ts_options.as_ref(),
        )
    }

    /// Type-check a source file without generating bytecode.
    pub fn check_file(&self, path: &Path) -> Result<compile::CheckDiagnostics, RuntimeError> {
        Ok(self.check_program_file(path)?.diagnostics)
    }

    /// Compile a full file program (entry + resolved local module graph).
    pub fn compile_program_file(&self, path: &Path) -> Result<CompiledProgram, RuntimeError> {
        let type_mode = self
            .options
            .type_mode
            .unwrap_or_else(|| compile::default_type_mode_for_builtin(self.options.builtin_mode));
        let ts_options = self.resolve_ts_options_for_path(path)?;

        let compiler = module_system::ProgramCompiler {
            builtin_mode: self.options.builtin_mode,
            type_mode,
            ts_options,
            compile_options: None,
        };
        compiler.compile_program_file(path)
    }

    /// Compile a full file program (entry + resolved local module graph) with options.
    pub fn compile_program_file_with_options(
        &self,
        path: &Path,
        options: &compile::CompileOptions,
    ) -> Result<CompiledProgram, RuntimeError> {
        let type_mode = self
            .options
            .type_mode
            .unwrap_or_else(|| compile::default_type_mode_for_builtin(self.options.builtin_mode));
        let ts_options = self.resolve_ts_options_for_path(path)?;

        let compiler = module_system::ProgramCompiler {
            builtin_mode: self.options.builtin_mode,
            type_mode,
            ts_options,
            compile_options: if options.sourcemap
                || options.emit_generic_templates
                || !matches!(
                    options.monomorphization_mode,
                    raya_engine::compiler::MonomorphizationMode::ConsumerLink
                ) {
                Some(options.clone())
            } else {
                None
            },
        };
        compiler.compile_program_file(path)
    }

    /// Type-check a full file program (entry + resolved local module graph).
    pub fn check_program_file(&self, path: &Path) -> Result<ProgramDiagnostics, RuntimeError> {
        let type_mode = self
            .options
            .type_mode
            .unwrap_or_else(|| compile::default_type_mode_for_builtin(self.options.builtin_mode));
        let ts_options = self.resolve_ts_options_for_path(path)?;

        let compiler = module_system::ProgramCompiler {
            builtin_mode: self.options.builtin_mode,
            type_mode,
            ts_options,
            compile_options: None,
        };
        compiler.check_program_file(path)
    }

    // ── Loading ──────────────────────────────────────────────────────────

    /// Load a .ryb bytecode file.
    pub fn load_bytecode(&self, path: &Path) -> Result<CompiledModule, RuntimeError> {
        loader::load_bytecode_file(path)
    }

    /// Load bytecode from raw bytes.
    pub fn load_bytecode_bytes(&self, bytes: &[u8]) -> Result<CompiledModule, RuntimeError> {
        loader::load_bytecode_bytes(bytes)
    }

    // ── Execution ────────────────────────────────────────────────────────

    /// Execute a compiled module and return the VM result value.
    pub fn execute(&self, module: &CompiledModule) -> Result<Value, RuntimeError> {
        let mut vm = vm_setup::create_vm(&self.options);
        self.ensure_ambient_builtin_globals_seeded(&mut vm)?;
        vm.shared_state()
            .register_module(Arc::new(module.module.clone()))
            .map_err(RuntimeError::Dependency)?;
        self.maybe_enable_jit(&mut vm);
        self.maybe_enable_profiling(&vm);
        let result = if self.options.builtin_mode == BuiltinMode::RayaStrict {
            vm.execute_entry_only(&module.module)?
        } else {
            vm.execute(&module.module)?
        };
        self.maybe_write_profile(&vm, &module.module);
        self.maybe_emit_jit_telemetry(&vm);
        Ok(result)
    }

    /// Execute a compiled module with pre-loaded dependency modules.
    ///
    /// Each dependency is registered with the VM before the main module executes,
    /// so that imports can be resolved at runtime.
    pub fn execute_with_deps(
        &self,
        module: &CompiledModule,
        deps: &[CompiledModule],
    ) -> Result<Value, RuntimeError> {
        let mut vm = vm_setup::create_vm(&self.options);
        let result = self.execute_with_deps_in_vm(&mut vm, module, deps)?;
        self.maybe_write_profile(&vm, &module.module);
        self.maybe_emit_jit_telemetry(&vm);
        Ok(result)
    }

    /// Execute a compiled program graph, resolving declaration-backed late links at runtime.
    pub fn execute_program(&self, program: &CompiledProgram) -> Result<Value, RuntimeError> {
        let mut vm = vm_setup::create_vm(&self.options);
        let result = self.execute_program_with_vm(program, &mut vm)?;
        self.maybe_write_profile(&vm, &program.entry.module);
        self.maybe_emit_jit_telemetry(&vm);
        Ok(result)
    }

    /// Execute a compiled program graph using a caller-provided VM.
    ///
    /// Useful when callers need VM lifetime control (for example, tests that
    /// keep returned pointer values valid across assertions).
    pub fn execute_program_with_vm(
        &self,
        program: &CompiledProgram,
        vm: &mut raya_engine::vm::Vm,
    ) -> Result<Value, RuntimeError> {
        if program.dependencies.is_empty() && program.late_link_requirements.is_empty() {
            self.ensure_ambient_builtin_globals_seeded(vm)?;
            vm.shared_state()
                .register_module(Arc::new(program.entry.module.clone()))
                .map_err(RuntimeError::Dependency)?;
            self.maybe_enable_jit(vm);
            self.maybe_enable_profiling(vm);
            let result = if self.options.builtin_mode == BuiltinMode::RayaStrict {
                vm.execute_entry_only(&program.entry.module)?
            } else {
                vm.execute(&program.entry.module)?
            };
            return Ok(result);
        }

        let deps = self.collect_program_dependencies(program)?;
        self.execute_with_deps_in_vm(vm, &program.entry, &deps)
    }

    // ── Convenience ──────────────────────────────────────────────────────

    /// Compile and execute a source string. Returns the result value.
    ///
    /// ```rust,ignore
    /// let rt = Runtime::new();
    /// let value = rt.eval("return 1 + 2;")?;
    /// ```
    pub fn eval(&self, code: &str) -> Result<Value, RuntimeError> {
        let program =
            self.compile_program_source_with_virtual_entry(code, Path::new("<eval>.raya"))?;
        self.execute_program(&program)
    }

    /// Run a file (source or .ryb), auto-detecting format by extension.
    ///
    /// - `.raya` files are compiled from source then executed.
    /// - `.ryb` files are loaded as bytecode then executed.
    ///
    /// If the file is in a project with `raya.toml`, dependencies are
    /// automatically resolved and loaded.
    ///
    /// Returns process exit code (0 = success, 1 = runtime error).
    pub fn run_file(&self, path: &Path) -> Result<i32, RuntimeError> {
        let path = if path.is_relative() {
            std::env::current_dir()?.join(path)
        } else {
            path.to_path_buf()
        };

        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        if Self::is_source_file_extension(extension)
            && self.can_use_binary_program_execution()
        {
            let program = self.compile_program_file(&path)?;
            let result = if program.dependencies.is_empty() && program.late_link_requirements.is_empty() {
                self.execute(&program.entry)
            } else {
                self.execute_program(&program)
            };
            return match result {
                Ok(_) => Ok(0),
                Err(RuntimeError::Vm(e)) => {
                    eprintln!("Runtime error: {}", e);
                    Ok(1)
                }
                Err(e) => Err(e),
            };
        }

        let module = match extension {
            "ryb" => self.load_bytecode(&path)?,
            ext if Self::is_source_file_extension(ext) => self.compile_file(&path)?,
            #[cfg(feature = "aot")]
            "bundle" => {
                return self.run_bundle_file(&path);
            }
            #[cfg(not(feature = "aot"))]
            "bundle" => self.load_bundle_entry_module(&path)?,
            _ => {
                return Err(RuntimeError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Unknown file type: {}", path.display()),
                )));
            }
        };

        // Try to load dependencies from raya.toml or adjacent .ryb files
        let file_dir = path.parent().unwrap_or(Path::new("."));
        let dep_modules = self.resolve_deps_for_file(&path, file_dir)?;

        let result = if dep_modules.is_empty() {
            self.execute(&module)
        } else {
            self.execute_with_deps(&module, &dep_modules)
        };

        match result {
            Ok(_) => Ok(0),
            Err(RuntimeError::Vm(e)) => {
                eprintln!("Runtime error: {}", e);
                Ok(1)
            }
            Err(e) => Err(e),
        }
    }

    #[cfg(feature = "aot")]
    fn run_bundle_file(&self, path: &Path) -> Result<i32, RuntimeError> {
        let payload = bundle::loader::detect_bundle_at(path).ok_or_else(|| {
            RuntimeError::Bytecode(format!("Invalid or unsupported bundle: {}", path.display()))
        })?;
        let module = self.load_bundle_entry_module_from_payload(&payload)?;
        let mut vm = vm_setup::create_vm(&self.options);
        let result = self.execute_bundle_with_vm(&mut vm, &module, &payload);
        self.maybe_write_profile(&vm, &module.module);
        self.maybe_emit_jit_telemetry(&vm);
        match result {
            Ok(_) => Ok(0),
            Err(RuntimeError::Vm(e)) => {
                eprintln!("Runtime error: {}", e);
                Ok(1)
            }
            Err(e) => Err(e),
        }
    }

    /// Run a file with explicitly provided dependency modules.
    pub fn run_file_with_deps(
        &self,
        path: &Path,
        deps: &[CompiledModule],
    ) -> Result<i32, RuntimeError> {
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let module = match extension {
            "ryb" => self.load_bytecode(path)?,
            ext if Self::is_source_file_extension(ext) => self.compile_file(path)?,
            _ => {
                return Err(RuntimeError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Unknown file type: {}", path.display()),
                )));
            }
        };

        match self.execute_with_deps(&module, deps) {
            Ok(_) => Ok(0),
            Err(RuntimeError::Vm(e)) => {
                eprintln!("Runtime error: {}", e);
                Ok(1)
            }
            Err(e) => Err(e),
        }
    }

    // ── Profiling helpers ────────────────────────────────────────────────

    fn build_module_linker(&self, modules: &[Arc<Module>]) -> Result<ModuleLinker, RuntimeError> {
        let mut linker = ModuleLinker::new();
        for module in modules {
            linker.add_module(module.clone()).map_err(|message| {
                RuntimeError::Dependency(format!(
                    "Module linker registration failed for '{}': {}",
                    module.metadata.name, message
                ))
            })?;
        }
        Ok(linker)
    }

    fn compute_module_init_order(
        &self,
        linker: &ModuleLinker,
        entry_module: &Arc<Module>,
    ) -> Result<Vec<Arc<Module>>, RuntimeError> {
        fn visit(
            module: Arc<Module>,
            linker: &ModuleLinker,
            marks: &mut HashMap<String, u8>,
            ordered: &mut Vec<Arc<Module>>,
        ) -> Result<(), RuntimeError> {
            let module_key = module.metadata.name.clone();
            match marks.get(&module_key).copied() {
                Some(2) => return Ok(()),
                Some(1) => {
                    return Err(RuntimeError::Dependency(format!(
                        "Circular runtime module initialization dependency detected at '{}'",
                        module.metadata.name
                    )))
                }
                _ => {}
            }

            marks.insert(module_key.clone(), 1);
            for import in &module.imports {
                if import.module_id == 0 {
                    return Err(RuntimeError::Dependency(format!(
                        "Module '{}' has import '{}' with missing target module ID",
                        module.metadata.name, import.module_specifier
                    )));
                }
                let dep_module = linker
                    .get_module_by_id(import.module_id)
                    .cloned()
                    .ok_or_else(|| {
                        RuntimeError::Dependency(format!(
                            "Module '{}' references unresolved dependency module ID {} ('{}')",
                            module.metadata.name, import.module_id, import.module_specifier
                        ))
                    })?;
                visit(dep_module, linker, marks, ordered)?;
            }

            marks.insert(module_key, 2);
            ordered.push(module);
            Ok(())
        }

        let mut marks = HashMap::new();
        let mut ordered = Vec::new();
        visit(entry_module.clone(), linker, &mut marks, &mut ordered)?;
        Ok(ordered)
    }

    fn collect_program_dependencies(
        &self,
        program: &CompiledProgram,
    ) -> Result<Vec<CompiledModule>, RuntimeError> {
        let mut deps = program
            .dependencies
            .iter()
            .map(|dep| CompiledModule {
                module: dep.module.clone(),
                interner: None,
            })
            .collect::<Vec<_>>();

        if !program.late_link_requirements.is_empty() {
            let late_linked = self.resolve_late_link_modules(program)?;
            for module in late_linked {
                if deps
                    .iter()
                    .any(|dep| dep.module.metadata.name == module.module.metadata.name)
                {
                    continue;
                }
                deps.push(module);
            }
        }

        Ok(deps)
    }

    fn builtin_surface_mode_for_runtime(mode: BuiltinMode) -> BuiltinSurfaceMode {
        match mode {
            BuiltinMode::RayaStrict => BuiltinSurfaceMode::RayaStrict,
            BuiltinMode::NodeCompat => BuiltinSurfaceMode::NodeCompat,
        }
    }

    fn runtime_only_ambient_builtin_names(mode: BuiltinMode) -> Vec<String> {
        match mode {
            BuiltinMode::RayaStrict | BuiltinMode::NodeCompat => {
                vec!["EventEmitter".to_string()]
            }
        }
    }

    fn ambient_builtin_export_names(mode: BuiltinMode) -> Result<Vec<String>, RuntimeError> {
        let exports = builtin_global_exports(Self::builtin_surface_mode_for_runtime(mode))
            .map_err(|error| {
                RuntimeError::Dependency(format!(
                    "Failed to load builtin declaration exports for runtime seeding: {error}"
                ))
            })?;
        let mut names = exports
            .symbols
            .iter()
            .filter_map(|(name, exported)| match exported.kind {
                raya_engine::parser::checker::SymbolKind::TypeAlias
                | raya_engine::parser::checker::SymbolKind::TypeParameter
                | raya_engine::parser::checker::SymbolKind::Interface => None,
                _ => Some(name.clone()),
            })
            .collect::<Vec<_>>();
        names.extend(Self::runtime_only_ambient_builtin_names(mode));
        names.sort();
        names.dedup();
        Ok(names)
    }

    fn collect_pattern_names(pattern: &Pattern, interner: &Interner, out: &mut Vec<String>) {
        match pattern {
            Pattern::Identifier(id) => out.push(interner.resolve(id.name).to_string()),
            Pattern::Array(arr) => {
                for elem in arr.elements.iter().flatten() {
                    Self::collect_pattern_names(&elem.pattern, interner, out);
                }
                if let Some(rest) = &arr.rest {
                    Self::collect_pattern_names(rest, interner, out);
                }
            }
            Pattern::Object(obj) => {
                for prop in &obj.properties {
                    Self::collect_pattern_names(&prop.value, interner, out);
                }
                if let Some(rest) = &obj.rest {
                    out.push(interner.resolve(rest.name).to_string());
                }
            }
            Pattern::Rest(rest) => Self::collect_pattern_names(&rest.argument, interner, out),
        }
    }

    fn top_level_runtime_names(source: &str) -> Result<Vec<String>, RuntimeError> {
        let parser = Parser::new(source).map_err(|errors| {
            RuntimeError::Parse(
                errors
                    .iter()
                    .map(|error| error.to_string())
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })?;
        let (ast, interner) = parser.parse().map_err(|errors| {
            RuntimeError::Parse(
                errors
                    .iter()
                    .map(|error| error.to_string())
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })?;
        let mut names = Vec::new();
        for stmt in &ast.statements {
            match stmt {
                Statement::ClassDecl(class_decl) => {
                    names.push(interner.resolve(class_decl.name.name).to_string());
                }
                Statement::FunctionDecl(func_decl) => {
                    names.push(interner.resolve(func_decl.name.name).to_string());
                }
                Statement::VariableDecl(var_decl) => {
                    Self::collect_pattern_names(&var_decl.pattern, &interner, &mut names);
                }
                _ => {}
            }
        }
        names.retain(|name| !name.is_empty());
        names.sort();
        names.dedup();
        Ok(names)
    }

    fn collect_module_global_slots(
        ast: &raya_engine::parser::ast::Module,
        interner: &Interner,
    ) -> HashMap<String, u32> {
        let mut slots = HashMap::new();
        let mut next_slot = 0u32;

        for stmt in &ast.statements {
            if let Statement::ImportDecl(import_decl) = stmt {
                for spec in &import_decl.specifiers {
                    let local_name = match spec {
                        raya_engine::parser::ast::ImportSpecifier::Named { name, alias } => alias
                            .as_ref()
                            .map(|a| interner.resolve(a.name).to_string())
                            .unwrap_or_else(|| interner.resolve(name.name).to_string()),
                        raya_engine::parser::ast::ImportSpecifier::Default(local) => {
                            interner.resolve(local.name).to_string()
                        }
                        raya_engine::parser::ast::ImportSpecifier::Namespace(alias) => {
                            interner.resolve(alias.name).to_string()
                        }
                    };
                    slots.entry(local_name).or_insert_with(|| {
                        let slot = next_slot;
                        next_slot = next_slot.saturating_add(1);
                        slot
                    });
                }
            }
            if matches!(
                stmt,
                Statement::ExportDecl(raya_engine::parser::ast::ExportDecl::Default { .. })
            ) {
                slots.entry("default".to_string()).or_insert_with(|| {
                    let slot = next_slot;
                    next_slot = next_slot.saturating_add(1);
                    slot
                });
            }
        }

        for stmt in &ast.statements {
            if let Statement::VariableDecl(variable) = stmt {
                let mut names = Vec::new();
                Self::collect_pattern_names(&variable.pattern, interner, &mut names);
                for name in names {
                    slots.entry(name).or_insert_with(|| {
                        let slot = next_slot;
                        next_slot = next_slot.saturating_add(1);
                        slot
                    });
                }
            }
        }

        slots
    }

    fn export_symbol_type(kind: SymbolKind) -> Option<SymbolType> {
        match kind {
            SymbolKind::Function => Some(SymbolType::Function),
            SymbolKind::Class | SymbolKind::Interface => Some(SymbolType::Class),
            SymbolKind::Variable | SymbolKind::EnumMember => Some(SymbolType::Constant),
            SymbolKind::TypeAlias | SymbolKind::TypeParameter => None,
        }
    }

    fn infer_runtime_export_symbol_type(
        ast: &raya_engine::parser::ast::Module,
        interner: &Interner,
        export_name: &str,
    ) -> Option<SymbolType> {
        for stmt in &ast.statements {
            match stmt {
                Statement::ClassDecl(class_decl)
                    if interner.resolve(class_decl.name.name) == export_name =>
                {
                    return Some(SymbolType::Class);
                }
                Statement::FunctionDecl(func_decl)
                    if interner.resolve(func_decl.name.name) == export_name =>
                {
                    return Some(SymbolType::Function);
                }
                Statement::VariableDecl(var_decl) => {
                    let mut names = Vec::new();
                    Self::collect_pattern_names(&var_decl.pattern, interner, &mut names);
                    if names.iter().any(|name| name == export_name) {
                        return Some(SymbolType::Constant);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn populate_builtin_runtime_exports(
        module: &mut Module,
        ast: &raya_engine::parser::ast::Module,
        interner: &Interner,
        declared_exports: &raya_engine::compiler::module::ModuleExports,
        export_names: &[String],
    ) {
        module.exports.clear();
        let module_global_slots = Self::collect_module_global_slots(ast, interner);
        let module_name = module.metadata.name.clone();

        for export_name in export_names {
            let declared = declared_exports.symbols.get(export_name);
            let Some(symbol_type) = declared
                .and_then(|declared| Self::export_symbol_type(declared.kind))
                .or_else(|| Self::infer_runtime_export_symbol_type(ast, interner, export_name))
            else {
                continue;
            };

            let index = match symbol_type {
                SymbolType::Function => {
                    module.functions.iter().position(|f| f.name == *export_name)
                }
                SymbolType::Class => module.classes.iter().position(|c| c.name == *export_name),
                SymbolType::Constant => module_global_slots
                    .get(export_name)
                    .copied()
                    .map(|slot| slot as usize),
            };
            let Some(index) = index else {
                continue;
            };

            module.exports.push(Export {
                name: export_name.clone(),
                symbol_type: symbol_type.clone(),
                index,
                symbol_id: symbol_id_from_name(&module_name, SymbolScope::Module, export_name),
                scope: SymbolScope::Module,
                signature_hash: declared.map(|declared| declared.signature_hash).unwrap_or(0),
                type_signature: declared.map(|declared| declared.type_signature.clone()),
                nominal_type: matches!(symbol_type, SymbolType::Class).then_some(
                    NominalTypeExport {
                        local_nominal_type_index: index as u32,
                        constructor_function_index: module
                            .functions
                            .iter()
                            .position(|function| {
                                function.name == format!("{}::constructor", export_name)
                            })
                            .map(|idx| idx as u32),
                    },
                ),
            });
        }
    }

    fn compiled_builtin_runtime_modules(mode: BuiltinMode) -> Result<Vec<Module>, RuntimeError> {
        let cache = match mode {
            BuiltinMode::RayaStrict => &STRICT_BUILTIN_RUNTIME_MODULES,
            BuiltinMode::NodeCompat => &NODE_BUILTIN_RUNTIME_MODULES,
        };

        let cached = cache.get_or_init(|| {
            let declared_exports =
                match builtin_global_exports(Self::builtin_surface_mode_for_runtime(mode)) {
                    Ok(exports) => exports,
                    Err(error) => return Err(error.to_string()),
                };
            let ambient_names = {
                let mut names = declared_exports
                    .symbols
                    .iter()
                    .filter_map(|(name, exported)| match exported.kind {
                        SymbolKind::TypeAlias | SymbolKind::TypeParameter | SymbolKind::Interface => {
                            None
                        }
                        _ => Some(name.clone()),
                    })
                    .collect::<Vec<_>>();
                names.extend(Self::runtime_only_ambient_builtin_names(mode));
                names.sort();
                names.dedup();
                names
            };
            let ambient_names = match Ok::<Vec<String>, RuntimeError>(ambient_names) {
                Ok(names) => names,
                Err(error) => return Err(error.to_string()),
            };
            // Phase 1: declaration contract (declared ambient builtin symbols).
            let declared_name_set = ambient_names.iter().cloned().collect::<HashSet<_>>();
            struct ParsedBuiltinUnit {
                logical_path: &'static str,
                ast: raya_engine::parser::ast::Module,
                interner: Interner,
                export_names: Vec<String>,
            }

            let checker_mode = match compile::default_type_mode_for_builtin(mode) {
                TypeMode::Raya => raya_engine::parser::checker::TypeSystemMode::Raya,
                TypeMode::Js => raya_engine::parser::checker::TypeSystemMode::Js,
                TypeMode::Ts => raya_engine::parser::checker::TypeSystemMode::Ts,
            };
            let checker_policy =
                raya_engine::parser::checker::CheckerPolicy::for_mode(checker_mode);

            // Phase 2a: parse/materialize each builtin source module with explicit exports.
            let mut parsed_units = Vec::new();
            for (logical_path, module_source) in builtins::builtin_source_modules_for_mode(mode) {
                let mut export_names = match Self::top_level_runtime_names(module_source) {
                    Ok(names) => names,
                    Err(error) => {
                        return Err(format!(
                            "Failed to collect top-level names for '{}': {}",
                            logical_path, error
                        ))
                    }
                };
                export_names.retain(|name| declared_name_set.contains(name));
                export_names.sort();
                export_names.dedup();

                let mut source = (*module_source).to_string();
                if !export_names.is_empty() {
                    source.push_str("\nexport { ");
                    source.push_str(&export_names.join(", "));
                    source.push_str(" };");
                }

                let parser = match Parser::new(&source) {
                    Ok(parser) => parser,
                    Err(errors) => {
                        return Err(format!(
                            "Failed to parse builtin source '{}': {}",
                            logical_path,
                            errors
                                .iter()
                                .map(|error| error.to_string())
                                .collect::<Vec<_>>()
                                .join("; ")
                        ))
                    }
                };
                let (ast, interner) = match parser.parse() {
                    Ok(parsed) => parsed,
                    Err(errors) => {
                        return Err(format!(
                            "Failed to parse builtin source '{}': {}",
                            logical_path,
                            errors
                                .iter()
                                .map(|error| error.to_string())
                                .collect::<Vec<_>>()
                                .join("; ")
                        ))
                    }
                };

                parsed_units.push(ParsedBuiltinUnit {
                    logical_path,
                    ast,
                    interner,
                    export_names,
                });
            }

            // Phase 2b: bind declarations for the whole builtin content graph first.
            let mut shared_type_ctx = TypeContext::new();
            for unit in &parsed_units {
                let mut binder = raya_engine::parser::checker::binder::Binder::new(
                    &mut shared_type_ctx,
                    &unit.interner,
                )
                .with_mode(checker_mode)
                .with_policy(checker_policy);
                if let Err(errors) = binder.bind_module(&unit.ast) {
                    return Err(format!(
                        "Failed to bind builtin source '{}': {}",
                        unit.logical_path,
                        errors
                            .iter()
                            .map(|error| error.to_string())
                            .collect::<Vec<_>>()
                            .join("; ")
                    ));
                }
            }

            // Phase 2c: compile each module with stable file identity and shared declarations.
            let mut modules = Vec::with_capacity(parsed_units.len());
            for unit in parsed_units {
                let module_identity = format!("__raya_builtin__/{}", unit.logical_path);
                let allow_unresolved_runtime_fallback = !matches!(
                    checker_mode,
                    raya_engine::parser::checker::TypeSystemMode::Raya
                );
                let mut compiler =
                    raya_engine::compiler::Compiler::new(shared_type_ctx.clone(), &unit.interner)
                        .with_module_identity(module_identity)
                        .with_js_this_binding_compat(true)
                        .with_allow_unresolved_runtime_fallback(
                            allow_unresolved_runtime_fallback,
                        )
                        .with_ambient_builtin_globals(declared_name_set.clone());
                let mut module = match compiler.compile_via_ir(&unit.ast) {
                    Ok(module) => module,
                    Err(error) => {
                        return Err(format!(
                            "Failed to compile builtin source '{}': {}",
                            unit.logical_path, error
                        ))
                    }
                };
                Self::populate_builtin_runtime_exports(
                    &mut module,
                    &unit.ast,
                    &unit.interner,
                    &declared_exports,
                    &unit.export_names,
                );
                let encoded = module.encode();
                let finalized = match Module::decode(&encoded) {
                    Ok(module) => module,
                    Err(error) => {
                        return Err(format!(
                            "Failed to finalize builtin source '{}': {}",
                            unit.logical_path, error
                        ))
                    }
                };
                modules.push(finalized);
            }

            Ok(modules)
        });

        cached
            .as_ref()
            .cloned()
            .map_err(|message| RuntimeError::Dependency(message.clone()))
    }

    fn ensure_ambient_builtin_globals_seeded(
        &self,
        vm: &mut raya_engine::vm::Vm,
    ) -> Result<(), RuntimeError> {
        let ambient_names = Self::ambient_builtin_export_names(self.options.builtin_mode)?;
        if ambient_names.is_empty() {
            return Ok(());
        }
        if ambient_names
            .iter()
            .all(|name| vm.shared_state().get_builtin_global(name).is_some())
        {
            return Ok(());
        }

        for module in Self::compiled_builtin_runtime_modules(self.options.builtin_mode)? {
            let builtin_module = Arc::new(module);
            vm.shared_state()
                .register_module(builtin_module.clone())
                .map_err(RuntimeError::Dependency)?;

            if !vm.shared_state().is_module_initialized(&builtin_module) {
                match vm.execute_entry_only(&builtin_module) {
                    Ok(_) => {}
                    Err(raya_engine::vm::VmError::RuntimeError(message))
                        if message == "No main function" => {}
                    Err(error) => return Err(RuntimeError::Vm(error)),
                }
                vm.shared_state().mark_module_initialized(&builtin_module);
            }

            for export in &builtin_module.exports {
                let value = Self::materialize_export_value(vm, &builtin_module, export)?;
                vm.shared_state()
                    .set_builtin_global(export.name.clone(), value);
            }
        }

        Ok(())
    }

    fn execute_with_deps_in_vm(
        &self,
        vm: &mut raya_engine::vm::Vm,
        module: &CompiledModule,
        deps: &[CompiledModule],
    ) -> Result<Value, RuntimeError> {
        self.ensure_ambient_builtin_globals_seeded(vm)?;
        // Register dependency modules.
        for dep in deps {
            vm.shared_state()
                .register_module(Arc::new(dep.module.clone()))
                .map_err(RuntimeError::Dependency)?;
        }
        vm.shared_state()
            .register_module(Arc::new(module.module.clone()))
            .map_err(RuntimeError::Dependency)?;

        let mut modules = deps
            .iter()
            .map(|dep| Arc::new(dep.module.clone()))
            .collect::<Vec<_>>();
        modules.push(Arc::new(module.module.clone()));
        let linker = self.build_module_linker(&modules)?;
        let entry_module = modules
            .iter()
            .find(|loaded| loaded.metadata.name == module.module.metadata.name)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::Dependency(format!(
                    "Entry module '{}' missing from runtime module set",
                    module.module.metadata.name
                ))
            })?;
        let init_order = self.compute_module_init_order(&linker, &entry_module)?;

        let mut entry_result = None;
        for current_module in init_order {
            if vm.shared_state().is_module_initialized(&current_module) {
                continue;
            }

            self.hydrate_module_import_globals(vm, &linker, &current_module)?;

            let is_entry = current_module.metadata.name == entry_module.metadata.name;
            if is_entry {
                self.maybe_enable_jit(vm);
                self.maybe_enable_profiling(vm);
                let result = if self.options.builtin_mode == BuiltinMode::RayaStrict {
                    vm.execute_entry_only(&current_module)?
                } else {
                    vm.execute(&current_module)?
                };
                entry_result = Some(result);
            } else {
                // Dependency modules must execute once to materialize module-level state
                // (default export objects, initialized globals, static setup).
                match vm.execute_entry_only(&current_module) {
                    Ok(_) => {}
                    // Pure library modules can legally export symbols without a top-level
                    // `main`; they still need hydration/registration but have no init body.
                    Err(raya_engine::vm::VmError::RuntimeError(message))
                        if message == "No main function" => {}
                    Err(error) => return Err(RuntimeError::Vm(error)),
                }
            }

            vm.shared_state().mark_module_initialized(&current_module);
        }

        entry_result.ok_or_else(|| {
            RuntimeError::Dependency(format!(
                "Entry module '{}' was not executed during dependency initialization",
                entry_module.metadata.name
            ))
        })
    }

    fn hydrate_module_import_globals(
        &self,
        vm: &mut raya_engine::vm::Vm,
        linker: &ModuleLinker,
        module: &Arc<Module>,
    ) -> Result<(), RuntimeError> {
        if module.imports.is_empty() {
            return Ok(());
        }

        let resolved = linker.link_module(module).map_err(|error| {
            RuntimeError::Dependency(format!(
                "Runtime module link validation failed for '{}': {}",
                module.metadata.name, error
            ))
        })?;

        for (import, resolved_symbol) in module.imports.iter().zip(resolved.iter()) {
            let Some(local_global_slot) = import.runtime_global_slot.map(|slot| slot as usize)
            else {
                continue;
            };
            let global_slot = vm
                .shared_state()
                .resolve_global_slot(module, local_global_slot);

            let value = if import.symbol == "*" {
                Self::materialize_namespace_import_value(
                    vm,
                    module,
                    import,
                    &resolved_symbol.module,
                )?
            } else {
                Self::materialize_import_value(vm, module, import, resolved_symbol)?
            };
            let mut globals = vm.shared_state().globals_by_index.write();
            if global_slot >= globals.len() {
                globals.resize(global_slot + 1, Value::null());
            }
            globals[global_slot] = value;
        }

        Ok(())
    }

    fn materialize_import_value(
        vm: &mut raya_engine::vm::Vm,
        consumer_module: &Module,
        import: &Import,
        resolved: &ResolvedSymbol,
    ) -> Result<Value, RuntimeError> {
        let value = Self::materialize_export_value(vm, &resolved.module, &resolved.export)?;
        match resolved.export.symbol_type {
            SymbolType::Constant => Self::register_structural_constant_slot_view(
                vm,
                consumer_module,
                value,
                import,
                resolved,
            ),
            SymbolType::Class => Ok(value),
            SymbolType::Function => Ok(value),
        }
    }

    fn materialize_namespace_import_value(
        vm: &mut raya_engine::vm::Vm,
        consumer_module: &Module,
        import: &Import,
        module: &Arc<Module>,
    ) -> Result<Value, RuntimeError> {
        let export_names: Vec<String> = module
            .exports
            .iter()
            .filter(|export| export.name != "*")
            .map(|export| export.name.clone())
            .collect();
        let layout_id = layout_id_from_ordered_names(&export_names);
        let mut namespace = Object::new_structural(layout_id, export_names.len());
        for export in &module.exports {
            if export.name == "*" {
                continue;
            }
            let value = Self::materialize_export_value(vm, module, export)?;
            let slot = export_names
                .iter()
                .position(|name| name == &export.name)
                .expect("namespace export slot");
            namespace
                .set_field(slot, value)
                .map_err(raya_engine::vm::VmError::RuntimeError)
                .map_err(RuntimeError::Vm)?;
        }
        vm.shared_state()
            .register_structural_layout_shape(layout_id, &export_names);
        let gc_ptr = vm.shared_state().gc.lock().allocate(namespace);
        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };

        let Some(expected_sig) = import.type_signature.as_deref() else {
            return Err(RuntimeError::Dependency(format!(
                "Namespace import '{}::*' in '{}' is missing structural signature metadata",
                module.metadata.name, consumer_module.metadata.name
            )));
        };
        let Some(expected_layout) = Self::structural_member_layout_from_signature(expected_sig)
        else {
            return Err(RuntimeError::Dependency(format!(
                "Namespace import '{}::*' in '{}' has an invalid structural signature",
                module.metadata.name, consumer_module.metadata.name
            )));
        };
        let required_shape = Self::shape_id_for_member_names(&expected_layout);
        vm.shared_state()
            .register_structural_shape_names(required_shape, &expected_layout);
        let slot_map = Self::slot_map_from_layouts(&expected_layout, &export_names)
            .into_iter()
            .map(|mapped| {
                mapped
                    .map(raya_engine::vm::interpreter::StructuralSlotBinding::Field)
                    .unwrap_or(raya_engine::vm::interpreter::StructuralSlotBinding::Missing)
            })
            .collect();
        vm.shared_state()
            .register_structural_shape_adapter(layout_id, required_shape, slot_map);
        Ok(value)
    }

    fn materialize_export_value(
        vm: &mut raya_engine::vm::Vm,
        module: &Arc<Module>,
        export: &Export,
    ) -> Result<Value, RuntimeError> {
        match export.symbol_type {
            SymbolType::Function => {
                let closure = Closure::with_module(export.index, Vec::new(), module.clone());
                let gc_ptr = vm.shared_state().gc.lock().allocate(closure);
                Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) })
            }
            SymbolType::Constant => {
                let resolved = ResolvedSymbol {
                    module: module.clone(),
                    export: export.clone(),
                    index: export.index,
                };
                Self::materialize_constant_export(vm, &resolved)
            }
            SymbolType::Class => {
                let Some(nominal_export) = export.nominal_type else {
                    return Err(RuntimeError::Dependency(format!(
                        "Imported class symbol '{}' from '{}' is missing nominal constructor metadata",
                        export.name, module.metadata.name
                    )));
                };
                let class_name = module
                    .classes
                    .get(export.index)
                    .map(|class_def| class_def.name.clone())
                    .unwrap_or_else(|| export.name.clone());

                let nominal_type_id = vm
                    .shared_state()
                    .resolve_nominal_type_id(module, nominal_export.local_nominal_type_index as usize)
                    .ok_or_else(|| {
                        RuntimeError::Dependency(format!(
                            "invalid module-local nominal type index {} for export '{}'",
                            nominal_export.local_nominal_type_index, export.name
                        ))
                    })?;

                let classes = vm.shared_state().classes.read();
                let resolved_class = classes.get_class(nominal_type_id).ok_or_else(|| {
                    RuntimeError::Dependency(format!(
                        "Imported class symbol '{}' from '{}' resolved to missing runtime nominal type {}",
                        export.name, module.metadata.name, nominal_type_id
                    ))
                })?;
                if resolved_class.name != class_name
                    || !resolved_class
                        .module
                        .as_ref()
                        .is_some_and(|class_module| class_module.checksum == module.checksum)
                {
                    return Err(RuntimeError::Dependency(format!(
                        "Imported class symbol '{}' from '{}' resolved to wrong runtime class '{}' (nominal type {})",
                        export.name, module.metadata.name, resolved_class.name, nominal_type_id
                    )));
                }
                if let Some(expected_ctor_index) = nominal_export.constructor_function_index {
                    let actual_ctor_index = resolved_class.constructor_id.map(|id| id as u32);
                    if actual_ctor_index != Some(expected_ctor_index) {
                        return Err(RuntimeError::Dependency(format!(
                            "Imported class symbol '{}' from '{}' resolved to constructor {:?}, expected {}",
                            export.name,
                            module.metadata.name,
                            actual_ctor_index,
                            expected_ctor_index
                        )));
                    }
                }
                drop(classes);

                if nominal_type_id > u32::MAX as usize {
                    return Err(RuntimeError::Dependency(format!(
                        "Imported class symbol '{}' from '{}' has nominal type ID {} outside u32 range",
                        export.name, module.metadata.name, nominal_type_id
                    )));
                }
                let shape_id = export.type_signature.as_deref().map(signature_hash);
                let layout_id = vm.shared_state().nominal_layout_id(nominal_type_id).ok_or_else(|| {
                    RuntimeError::Dependency(format!(
                        "class '{}' missing runtime layout id",
                        class_name
                    ))
                })?;
                let handle_id =
                    vm.shared_state()
                        .register_type_handle(nominal_type_id as u32, layout_id, shape_id);
                let handle = TypeHandle {
                    handle_id,
                    shape_id,
                };
                let handle_gc = vm.shared_state().gc.lock().allocate(handle);
                let handle_value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(handle_gc.as_ptr()).unwrap()) };

                // Class imports hydrate as class objects carrying:
                // - hidden constructor handle for `new ImportedClass(...)`
                // - static methods as callable closures
                let static_prefix = format!("{}::static::", class_name);
                let static_method_names: Vec<String> = module
                    .functions
                    .iter()
                    .filter_map(|function| {
                        function
                            .name
                            .strip_prefix(&static_prefix)
                            .map(str::to_string)
                            .filter(|name| !name.is_empty())
                    })
                    .collect();
                let class_layout_id = layout_id_from_ordered_names(&static_method_names);
                let mut class_object =
                    Object::new_dynamic(class_layout_id, static_method_names.len());
                let handle_key = vm
                    .shared_state()
                    .intern_prop_key(IMPORTED_CLASS_TYPE_HANDLE_KEY);
                class_object
                    .ensure_dyn_map()
                    .insert(handle_key, handle_value);

                for (function_id, function) in module.functions.iter().enumerate() {
                    let Some(method_name) = function.name.strip_prefix(&static_prefix) else {
                        continue;
                    };
                    if method_name.is_empty() {
                        continue;
                    }
                    let closure = Closure::with_module(function_id, Vec::new(), module.clone());
                    let closure_gc = vm.shared_state().gc.lock().allocate(closure);
                    let closure_value = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(closure_gc.as_ptr()).unwrap())
                    };
                    let slot = static_method_names
                        .iter()
                        .position(|name| name == method_name)
                        .expect("class static method slot");
                    class_object
                        .set_field(slot, closure_value)
                        .map_err(raya_engine::vm::VmError::RuntimeError)
                        .map_err(RuntimeError::Vm)?;
                }
                vm.shared_state()
                    .register_structural_layout_shape(class_layout_id, &static_method_names);

                let class_gc = vm.shared_state().gc.lock().allocate(class_object);
                Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new(class_gc.as_ptr()).unwrap()) })
            }
        }
    }

    fn materialize_constant_export(
        vm: &mut raya_engine::vm::Vm,
        resolved: &ResolvedSymbol,
    ) -> Result<Value, RuntimeError> {
        fn normalize_integral_numeric_constant(value: Value) -> Value {
            if let Some(f) = value.as_f64() {
                if f.is_finite()
                    && f.fract() == 0.0
                    && f >= i32::MIN as f64
                    && f <= i32::MAX as f64
                {
                    return Value::i32(f as i32);
                }
            }
            value
        }

        let index = resolved.export.index;

        // Prefer global-slot-backed constants when the export index falls within
        // this module's reserved global range.
        if let Some(layout) = vm
            .shared_state()
            .module_layouts
            .read()
            .get(&resolved.module.checksum)
        {
            if index < layout.global_len {
                let global_slot = layout.global_base + index;
                if let Some(value) = vm
                    .shared_state()
                    .globals_by_index
                    .read()
                    .get(global_slot)
                    .copied()
                {
                    return Ok(normalize_integral_numeric_constant(value));
                }
            }
        } else {
            // Fallback when module layout cannot be located by checksum.
            let global_slot = vm
                .shared_state()
                .resolve_global_slot(&resolved.module, resolved.export.index);
            if let Some(value) = vm
                .shared_state()
                .globals_by_index
                .read()
                .get(global_slot)
                .copied()
            {
                return Ok(normalize_integral_numeric_constant(value));
            }
        }

        // Constant-pool-backed constants are addressed by flattened pool index.
        let constants = &resolved.module.constants;
        let string_len = constants.strings.len();
        if index < string_len {
            let string = RayaString::new(constants.strings[index].clone());
            let gc_ptr = vm.shared_state().gc.lock().allocate(string);
            return Ok(unsafe {
                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
            });
        }

        let int_base = string_len;
        let int_len = constants.integers.len();
        if index < int_base + int_len {
            return Ok(Value::i32(constants.integers[index - int_base]));
        }

        let float_base = int_base + int_len;
        let float_len = constants.floats.len();
        if index < float_base + float_len {
            return Ok(normalize_integral_numeric_constant(Value::f64(
                constants.floats[index - float_base],
            )));
        }

        Err(RuntimeError::Dependency(format!(
            "Constant export index {} is out of range in module '{}'",
            index, resolved.module.metadata.name
        )))
    }

    fn register_structural_constant_slot_view(
        vm: &mut raya_engine::vm::Vm,
        consumer_module: &Module,
        value: Value,
        import: &Import,
        resolved: &ResolvedSymbol,
    ) -> Result<Value, RuntimeError> {
        let Some(expected_sig) = import.type_signature.as_deref() else {
            return Ok(value);
        };
        let Some(actual_sig) = resolved.export.type_signature.as_deref() else {
            return Ok(value);
        };
        let Some(expected_layout) = Self::structural_member_layout_from_signature(expected_sig)
        else {
            return Ok(value);
        };
        let Some(actual_sig_layout) = Self::structural_member_layout_from_signature(actual_sig)
        else {
            return Ok(value);
        };
        if expected_layout.is_empty() {
            return Ok(value);
        }
        let required_shape = Self::shape_id_for_member_names(&expected_layout);
        vm.shared_state()
            .register_structural_shape_names(required_shape, &expected_layout);
        let expected_methods =
            Self::structural_method_layout_from_signature(expected_sig).unwrap_or_default();

        let JSView::Struct {
            ptr,
            layout_id,
            nominal_type_id,
        } = raya_engine::vm::json::js_classify(value)
        else {
            return Ok(value);
        };

        let source = unsafe { &*ptr };
        let provider_layout = source.layout_id();
        if provider_layout == 0 {
            return Err(RuntimeError::Vm(raya_engine::vm::VmError::RuntimeError(
                "structural export value is missing a physical layout id".to_string(),
            )));
        }
        let actual_layout = if nominal_type_id.is_none() {
            vm.shared_state()
                .structural_layout_names(provider_layout)
                .unwrap_or(actual_sig_layout)
        } else {
            actual_sig_layout
        };
        if nominal_type_id.is_none() {
            vm.shared_state()
                .register_structural_layout_shape(provider_layout, &actual_layout);
        }
        let slot_map = if let Some(nominal_type_id) = nominal_type_id {
            let class_metadata = vm.shared_state().class_metadata.read();
            if let Some(meta) = class_metadata.get(nominal_type_id as usize) {
                expected_layout
                    .iter()
                    .map(|name| {
                        let prefer_method = expected_methods.contains(name);
                        let method_binding = meta
                            .get_method_index(name)
                            .map(raya_engine::vm::interpreter::StructuralSlotBinding::Method);
                        let field_binding = meta.get_field_index(name).map(|idx| {
                            raya_engine::vm::interpreter::StructuralSlotBinding::Field(idx)
                        });
                        if prefer_method {
                            method_binding.or(field_binding).unwrap_or(
                                raya_engine::vm::interpreter::StructuralSlotBinding::Missing,
                            )
                        } else {
                            field_binding.or(method_binding).unwrap_or(
                                raya_engine::vm::interpreter::StructuralSlotBinding::Missing,
                            )
                        }
                    })
                    .collect()
            } else {
                let fallback = Self::slot_map_from_layouts(&expected_layout, &actual_layout);
                fallback
                    .into_iter()
                    .map(|mapped| {
                        mapped
                            .map(raya_engine::vm::interpreter::StructuralSlotBinding::Field)
                            .unwrap_or(raya_engine::vm::interpreter::StructuralSlotBinding::Missing)
                    })
                    .collect()
            }
        } else {
            let fallback = Self::slot_map_from_layouts(&expected_layout, &actual_layout);
            fallback
                .into_iter()
                .map(|mapped| {
                    mapped
                        .map(raya_engine::vm::interpreter::StructuralSlotBinding::Field)
                        .unwrap_or(raya_engine::vm::interpreter::StructuralSlotBinding::Missing)
                })
                .collect()
        };
        let _ = consumer_module;
        let _ = source.object_id();
        vm.shared_state().register_structural_shape_adapter(
            provider_layout,
            required_shape,
            slot_map,
        );
        Ok(value)
    }

    fn collect_structural_field_names(
        type_ctx: &TypeContext,
        ty: raya_engine::parser::TypeId,
        out: &mut BTreeSet<String>,
    ) -> bool {
        let Some(ty) = type_ctx.get(ty) else {
            return false;
        };
        match ty {
            Type::Object(obj) => {
                out.extend(obj.properties.iter().map(|property| property.name.clone()));
                true
            }
            Type::Interface(interface) => {
                out.extend(
                    interface
                        .properties
                        .iter()
                        .map(|property| property.name.clone()),
                );
                out.extend(interface.methods.iter().map(|method| method.name.clone()));
                true
            }
            Type::Class(class) => {
                out.extend(
                    class
                        .properties
                        .iter()
                        .map(|property| property.name.clone()),
                );
                out.extend(class.methods.iter().map(|method| method.name.clone()));
                true
            }
            Type::Union(union) => {
                let mut any_object_like = false;
                for &member in &union.members {
                    any_object_like |= Self::collect_structural_field_names(type_ctx, member, out);
                }
                any_object_like
            }
            Type::TypeVar(type_var) => {
                type_var.constraint.is_some_and(|constraint| {
                    Self::collect_structural_field_names(type_ctx, constraint, out)
                }) || type_var.default.is_some_and(|default| {
                    Self::collect_structural_field_names(type_ctx, default, out)
                })
            }
            Type::Reference(reference) => type_ctx
                .lookup_named_type(&reference.name)
                .is_some_and(|named| Self::collect_structural_field_names(type_ctx, named, out)),
            Type::Generic(generic) => {
                Self::collect_structural_field_names(type_ctx, generic.base, out)
            }
            _ => false,
        }
    }

    fn collect_structural_method_names(
        type_ctx: &TypeContext,
        ty: raya_engine::parser::TypeId,
        out: &mut BTreeSet<String>,
    ) -> bool {
        let Some(ty) = type_ctx.get(ty) else {
            return false;
        };
        match ty {
            Type::Object(_) => false,
            Type::Interface(interface) => {
                out.extend(interface.methods.iter().map(|method| method.name.clone()));
                true
            }
            Type::Class(class) => {
                out.extend(class.methods.iter().map(|method| method.name.clone()));
                true
            }
            Type::Union(union) => {
                let mut any = false;
                for &member in &union.members {
                    any |= Self::collect_structural_method_names(type_ctx, member, out);
                }
                any
            }
            Type::TypeVar(type_var) => {
                type_var.constraint.is_some_and(|constraint| {
                    Self::collect_structural_method_names(type_ctx, constraint, out)
                }) || type_var.default.is_some_and(|default| {
                    Self::collect_structural_method_names(type_ctx, default, out)
                })
            }
            Type::Reference(reference) => type_ctx
                .lookup_named_type(&reference.name)
                .is_some_and(|named| Self::collect_structural_method_names(type_ctx, named, out)),
            Type::Generic(generic) => {
                Self::collect_structural_method_names(type_ctx, generic.base, out)
            }
            _ => false,
        }
    }

    fn structural_slot_layout(
        type_ctx: &TypeContext,
        ty: raya_engine::parser::TypeId,
    ) -> Option<Vec<String>> {
        let mut fields = BTreeSet::new();
        if !Self::collect_structural_field_names(type_ctx, ty, &mut fields) {
            return None;
        }
        Some(fields.into_iter().collect())
    }

    fn structural_method_layout(
        type_ctx: &TypeContext,
        ty: raya_engine::parser::TypeId,
    ) -> Option<Vec<String>> {
        let mut methods = BTreeSet::new();
        if !Self::collect_structural_method_names(type_ctx, ty, &mut methods) {
            return None;
        }
        Some(methods.into_iter().collect())
    }

    fn structural_member_layout(
        type_ctx: &TypeContext,
        ty: raya_engine::parser::TypeId,
    ) -> Option<Vec<String>> {
        let mut members = BTreeSet::new();
        let found_fields = Self::collect_structural_field_names(type_ctx, ty, &mut members);
        let found_methods = Self::collect_structural_method_names(type_ctx, ty, &mut members);
        if !found_fields && !found_methods {
            return None;
        }
        Some(members.into_iter().collect())
    }

    fn structural_slot_map(expected_sig: &str, actual_sig: &str) -> Option<Vec<Option<usize>>> {
        let expected_layout = Self::structural_member_layout_from_signature(expected_sig)?;
        let actual_layout = Self::structural_member_layout_from_signature(actual_sig)?;
        Some(Self::slot_map_from_layouts(
            &expected_layout,
            &actual_layout,
        ))
    }

    fn slot_map_from_layouts(
        expected_layout: &[String],
        actual_layout: &[String],
    ) -> Vec<Option<usize>> {
        let actual_index: HashMap<&str, usize> = actual_layout
            .iter()
            .enumerate()
            .map(|(idx, name)| (name.as_str(), idx))
            .collect();

        let mut slot_map = Vec::with_capacity(expected_layout.len());
        for expected_field in expected_layout {
            slot_map.push(actual_index.get(expected_field.as_str()).copied());
        }
        slot_map
    }

    fn structural_slot_layout_from_signature(signature: &str) -> Option<Vec<String>> {
        let mut type_ctx = TypeContext::new();
        let ty = try_hydrate_type_from_canonical_signature(signature, &mut type_ctx)?;
        Self::structural_slot_layout(&type_ctx, ty)
    }

    fn structural_method_layout_from_signature(signature: &str) -> Option<Vec<String>> {
        let mut type_ctx = TypeContext::new();
        let ty = try_hydrate_type_from_canonical_signature(signature, &mut type_ctx)?;
        Self::structural_method_layout(&type_ctx, ty)
    }

    fn structural_member_layout_from_signature(signature: &str) -> Option<Vec<String>> {
        let mut type_ctx = TypeContext::new();
        let ty = try_hydrate_type_from_canonical_signature(signature, &mut type_ctx)?;
        Self::structural_member_layout(&type_ctx, ty)
    }

    fn dynamic_layout_id_from_member_names(names: &[String]) -> u32 {
        raya_engine::vm::object::layout_id_from_ordered_names(names)
    }

    fn shape_id_for_member_names(names: &[String]) -> u64 {
        raya_engine::vm::object::shape_id_from_member_names(names)
    }

    fn maybe_enable_jit(&self, vm: &mut raya_engine::vm::Vm) {
        #[cfg(feature = "jit")]
        {
            if self.options.no_jit {
                return;
            }

            let config = raya_engine::jit::JitConfig {
                adaptive_compilation: true,
                call_threshold: self.options.jit_threshold,
                ..Default::default()
            };

            if let Err(e) = vm.enable_jit_with_config(config) {
                eprintln!("Warning: failed to enable JIT; falling back to interpreter: {e}");
            }
        }

        #[cfg(not(feature = "jit"))]
        {
            let _ = vm;
        }
    }

    fn maybe_enable_profiling(&self, vm: &raya_engine::vm::Vm) {
        if self.options.cpu_prof.is_some() {
            let config = raya_engine::profiler::ProfileConfig {
                interval_us: self.options.prof_interval_us,
                ..Default::default()
            };
            vm.enable_profiling(config);
        }
    }

    fn maybe_write_profile(&self, vm: &raya_engine::vm::Vm, module: &Module) {
        let Some(ref path) = self.options.cpu_prof else {
            return;
        };
        let Some(data) = vm.stop_profiling() else {
            return;
        };
        let resolved = data.resolve(module);
        let is_cpuprofile = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e == "cpuprofile")
            .unwrap_or(false);

        let output = if is_cpuprofile {
            resolved.to_cpuprofile_json()
        } else {
            resolved.to_flamegraph()
        };

        if let Err(e) = std::fs::write(path, &output) {
            eprintln!(
                "Warning: failed to write profile to {}: {}",
                path.display(),
                e
            );
        } else {
            eprintln!("Profile written to {}", path.display());
        }
    }

    fn maybe_emit_jit_telemetry(&self, vm: &raya_engine::vm::Vm) {
        #[cfg(feature = "jit")]
        {
            if std::env::var("RAYA_JIT_TELEMETRY").is_ok() {
                let t = vm.get_jit_telemetry();
                eprintln!(
                    "JIT telemetry: calls={} loops={} hits={} misses={} submit_ok={} submit_drop={}",
                    t.call_samples,
                    t.loop_samples,
                    t.cache_hits,
                    t.cache_misses,
                    t.compile_requests_submitted,
                    t.compile_requests_dropped
                );
            }
        }

        #[cfg(not(feature = "jit"))]
        {
            let _ = vm;
        }
    }

    fn resolve_late_link_modules(
        &self,
        program: &CompiledProgram,
    ) -> Result<Vec<CompiledModule>, RuntimeError> {
        let mut resolved = Vec::new();
        for requirement in &program.late_link_requirements {
            let module = self.load_late_link_module(requirement, &program.entry_path)?;
            self.validate_late_link_requirement(requirement, &module.module)?;
            resolved.push(module);
        }
        Ok(resolved)
    }

    fn load_late_link_module(
        &self,
        requirement: &LateLinkRequirement,
        entry_path: &Path,
    ) -> Result<CompiledModule, RuntimeError> {
        let candidates = self.collect_late_link_candidates(requirement, entry_path);
        let mut load_errors = Vec::new();

        for candidate in &candidates {
            if !candidate.exists() {
                continue;
            }
            let extension = candidate
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("");
            let loaded = match extension {
                "ryb" => self.load_bytecode(candidate),
                ext if Self::is_source_file_extension(ext) => self.compile_file(candidate),
                _ => continue,
            };

            match loaded {
                Ok(module) => return Ok(module),
                Err(error) => load_errors.push(format!("{}: {}", candidate.display(), error)),
            }
        }

        let candidate_text = candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let error_text = if load_errors.is_empty() {
            "no load attempts succeeded".to_string()
        } else {
            load_errors.join(" | ")
        };

        Err(RuntimeError::Dependency(format!(
            "Unable to resolve late-link module '{}' (id {}) from declaration '{}'. Candidates: [{}]. Errors: {}",
            requirement.module_identity,
            requirement.module_id,
            requirement.declaration_path.display(),
            candidate_text,
            error_text
        )))
    }

    fn collect_late_link_candidates(
        &self,
        requirement: &LateLinkRequirement,
        entry_path: &Path,
    ) -> Vec<std::path::PathBuf> {
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();

        fn push_unique(
            out: &mut Vec<std::path::PathBuf>,
            seen: &mut HashSet<std::path::PathBuf>,
            path: std::path::PathBuf,
        ) {
            if seen.insert(path.clone()) {
                out.push(path);
            }
        }

        fn add_expanded(
            out: &mut Vec<std::path::PathBuf>,
            seen: &mut HashSet<std::path::PathBuf>,
            base: &Path,
        ) {
            match base.extension().and_then(|ext| ext.to_str()) {
                Some("ryb") => {
                    push_unique(out, seen, base.to_path_buf());
                    push_unique(out, seen, base.with_extension("raya"));
                }
                Some("raya") => {
                    push_unique(out, seen, base.with_extension("ryb"));
                    push_unique(out, seen, base.to_path_buf());
                }
                Some(_) => push_unique(out, seen, base.to_path_buf()),
                None => {
                    push_unique(out, seen, base.with_extension("ryb"));
                    push_unique(out, seen, base.with_extension("raya"));
                    push_unique(out, seen, base.join("index.ryb"));
                    push_unique(out, seen, base.join("index.raya"));
                }
            }
        }

        if !requirement.module_identity.is_empty() {
            add_expanded(
                &mut candidates,
                &mut seen,
                &std::path::PathBuf::from(&requirement.module_identity),
            );
        }

        let entry_dir = entry_path.parent().unwrap_or_else(|| Path::new("."));
        for specifier in &requirement.module_specifiers {
            if specifier.starts_with("./")
                || specifier.starts_with("../")
                || specifier.starts_with('/')
            {
                let base = if std::path::Path::new(specifier).is_absolute() {
                    std::path::PathBuf::from(specifier)
                } else {
                    entry_dir.join(specifier)
                };
                add_expanded(&mut candidates, &mut seen, &base);
                continue;
            }

            let fallback_name = specifier
                .split('/')
                .next_back()
                .filter(|segment| !segment.is_empty())
                .unwrap_or(specifier);
            add_expanded(&mut candidates, &mut seen, &entry_dir.join(fallback_name));
            add_expanded(
                &mut candidates,
                &mut seen,
                &entry_dir
                    .join(".raya")
                    .join("packages")
                    .join(fallback_name)
                    .join("lib"),
            );
            if let Some(home) = dirs::home_dir() {
                add_expanded(
                    &mut candidates,
                    &mut seen,
                    &home
                        .join(".raya")
                        .join("packages")
                        .join(fallback_name)
                        .join("lib"),
                );
            }
        }

        candidates
    }

    fn validate_late_link_requirement(
        &self,
        requirement: &LateLinkRequirement,
        module: &Module,
    ) -> Result<(), RuntimeError> {
        let actual_module_id = module_id_from_name(&module.metadata.name);
        if actual_module_id != requirement.module_id {
            return Err(RuntimeError::Dependency(format!(
                "Late-link module identity mismatch: expected '{}' (id {}), got '{}' (id {})",
                requirement.module_identity,
                requirement.module_id,
                module.metadata.name,
                actual_module_id
            )));
        }

        for symbol in &requirement.symbols {
            self.validate_late_link_symbol(requirement, module, symbol)?;
        }

        Ok(())
    }

    fn validate_late_link_symbol(
        &self,
        requirement: &LateLinkRequirement,
        module: &Module,
        symbol: &LateLinkSymbolRequirement,
    ) -> Result<(), RuntimeError> {
        let Some(exported) = module
            .exports
            .iter()
            .find(|export| export.symbol_id == symbol.symbol_id)
        else {
            return Err(RuntimeError::Dependency(format!(
                "Late-link symbol '{}' (id {}) missing from module '{}'",
                symbol.symbol, symbol.symbol_id, requirement.module_identity
            )));
        };

        if exported.scope != symbol.scope {
            return Err(RuntimeError::Dependency(format!(
                "Late-link scope mismatch for '{}': expected {:?}, got {:?}",
                symbol.symbol, symbol.scope, exported.scope
            )));
        }

        if exported.symbol_type != symbol.symbol_type {
            return Err(RuntimeError::Dependency(format!(
                "Late-link symbol type mismatch for '{}': expected {:?}, got {:?}",
                symbol.symbol, symbol.symbol_type, exported.symbol_type
            )));
        }

        if symbol.signature_hash == 0 || exported.signature_hash == 0 {
            return Err(RuntimeError::Dependency(format!(
                "Late-link symbol '{}' is missing structural type signature hash (expected={:#x}, actual={:#x})",
                symbol.symbol, symbol.signature_hash, exported.signature_hash
            )));
        }

        if exported.signature_hash != symbol.signature_hash {
            let assignable = exported.type_signature.as_deref().is_some_and(|actual| {
                structural_signature_is_assignable(&symbol.type_signature, actual)
            });
            if !assignable {
                let actual_pretty = exported
                    .type_signature
                    .as_deref()
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("hash:{:016x}", exported.signature_hash));
                return Err(RuntimeError::Dependency(format!(
                    "Late-link type signature mismatch for '{}': expected {:#x} ({}), got {:#x} ({})",
                    symbol.symbol,
                    symbol.signature_hash,
                    symbol.type_signature,
                    exported.signature_hash,
                    actual_pretty
                )));
            }
        }

        if let Some(template) = &symbol.specialization_template {
            let Some(mono_entry) = module
                .metadata
                .mono_debug_map
                .iter()
                .find(|entry| entry.specialized_symbol == exported.name)
            else {
                return Err(RuntimeError::Dependency(format!(
                    "Late-link specialization contract missing for '{}': module '{}' does not expose mono-debug entry",
                    symbol.symbol, requirement.module_identity
                )));
            };

            let template_matches = mono_entry.template_id == format!("fn-template:{template}")
                || mono_entry.template_id == format!("class-template:{template}")
                || mono_entry.template_id.ends_with(&format!(":{template}"));
            if !template_matches {
                return Err(RuntimeError::Dependency(format!(
                    "Late-link specialization template mismatch for '{}': expected template '{}', got '{}'",
                    symbol.symbol, template, mono_entry.template_id
                )));
            }

            if symbol.symbol_type == SymbolType::Function {
                let has_template_symbol = module
                    .metadata
                    .template_symbol_table
                    .iter()
                    .any(|entry| entry.symbol == *template);
                let has_template_export =
                    module.exports.iter().any(|entry| entry.name == *template);
                if !(has_template_symbol || has_template_export) {
                    return Err(RuntimeError::Dependency(format!(
                        "Late-link specialization contract missing template symbol '{}' for '{}'",
                        template, symbol.symbol
                    )));
                }
            }
        }

        Ok(())
    }

    fn can_use_binary_program_execution(&self) -> bool {
        let type_mode = self
            .options
            .type_mode
            .unwrap_or_else(|| compile::default_type_mode_for_builtin(self.options.builtin_mode));
        if !matches!(type_mode, TypeMode::Raya | TypeMode::Js) {
            return false;
        }
        if self.options.ts_options.is_some() {
            return false;
        }
        true
    }

    // ── Internal helpers ─────────────────────────────────────────────────

    /// Resolve dependencies for a file, checking both raya.toml and adjacent .ryb files.
    fn resolve_deps_for_file(
        &self,
        file_path: &Path,
        file_dir: &Path,
    ) -> Result<Vec<CompiledModule>, RuntimeError> {
        // 1. Check for package.json/raya.toml project
        if let Some(manifest_dir) = deps::find_manifest_dir(file_path) {
            let package_json_path = manifest_dir.join("package.json");
            if package_json_path.exists() {
                let package_json_deps = deps::load_dependencies_from_package_json(&manifest_dir)?;
                if !package_json_deps.is_empty() {
                    return Ok(package_json_deps);
                }
            }
            let manifest_path = manifest_dir.join("raya.toml");
            if let Ok(manifest) = raya_pm::PackageManifest::from_file(&manifest_path) {
                if !manifest.dependencies.is_empty() {
                    return deps::load_dependencies(&manifest, &manifest_dir);
                }
            }
        }

        // 2. For .ryb files, auto-resolve imports from adjacent files
        if file_path.extension().and_then(|e| e.to_str()) == Some("ryb") {
            let module = self.load_bytecode(file_path)?;
            if !module.module.imports.is_empty() {
                return loader::resolve_ryb_deps(&module, file_dir);
            }
        }

        Ok(Vec::new())
    }

    fn load_bundle_entry_module(&self, path: &Path) -> Result<CompiledModule, RuntimeError> {
        let payload = bundle::loader::detect_bundle_at(path).ok_or_else(|| {
            RuntimeError::Bytecode(format!("Invalid or unsupported bundle: {}", path.display()))
        })?;
        self.load_bundle_entry_module_from_payload(&payload)
    }

    fn load_bundle_entry_module_from_payload(
        &self,
        payload: &bundle::loader::BundlePayload,
    ) -> Result<CompiledModule, RuntimeError> {
        let entry_path = payload
            .vfs
            .paths()
            .find(|p| p.ends_with(".ryb"))
            .ok_or_else(|| RuntimeError::Bytecode("Bundle contains no embedded .ryb".to_string()))?
            .to_string();

        let bytes = payload.vfs.read(&entry_path).ok_or_else(|| {
            RuntimeError::Bytecode(format!("Failed to read embedded bytecode: {entry_path}"))
        })?;

        self.load_bytecode_bytes(&bytes)
    }

    #[cfg(feature = "aot")]
    fn execute_bundle_with_vm(
        &self,
        vm: &mut raya_engine::vm::Vm,
        module: &CompiledModule,
        payload: &bundle::loader::BundlePayload,
    ) -> Result<Value, RuntimeError> {
        self.ensure_ambient_builtin_globals_seeded(vm)?;
        let runtime_module = Arc::new(module.module.clone());
        vm.shared_state()
            .register_module(runtime_module.clone())
            .map_err(RuntimeError::Dependency)?;

        let linker = self.build_module_linker(std::slice::from_ref(&runtime_module))?;
        self.hydrate_module_import_globals(vm, &linker, &runtime_module)?;

        let entry_main_fn_id = runtime_module
            .functions
            .iter()
            .rposition(|f| f.name == "main")
            .ok_or_else(|| RuntimeError::Vm(VmError::RuntimeError("No main function".to_string())))?;
        let global_func_id = entry_main_fn_id as u32;
        let loaded_entry = payload.functions.get(&global_func_id).ok_or_else(|| {
            RuntimeError::Bytecode(format!(
                "Bundle missing native entry for main function index {}",
                entry_main_fn_id
            ))
        })?;

        let entry_ptr = if payload.profile_clones.contains_key(&global_func_id) {
            dispatch_registered_aot_entry
        } else {
            unsafe {
                std::mem::transmute::<*const u8, AotEntryFn>(
                    payload.code.get_fn_ptr(loaded_entry.code_offset),
                )
            }
        };
        let _registry_guard = install_registered_aot_functions(payload.functions.iter().map(
            |(func_id, loaded)| unsafe {
                let clones = payload
                    .profile_clones
                    .get(func_id)
                    .into_iter()
                    .flatten()
                    .map(|clone| RegisteredAotClone {
                        ptr: std::mem::transmute::<*const u8, AotEntryFn>(
                            payload.code.get_fn_ptr(clone.code_offset),
                        ),
                        guard_bytecode_offset: clone.guard_bytecode_offset,
                        guard_layout_id: clone.guard_layout_id,
                        guard_arg_index: clone.guard_arg_index,
                    })
                    .collect::<Vec<_>>();
                RegisteredAotFunctionEntry {
                    func_id: *func_id,
                    baseline: std::mem::transmute::<*const u8, AotEntryFn>(
                        payload.code.get_fn_ptr(loaded.code_offset),
                    ),
                    clones,
                }
            },
        ));

        let main_task = Arc::new(Task::new(entry_main_fn_id, runtime_module.clone(), None));
        main_task.set_state(TaskState::Running);
        vm.shared_state()
            .tasks
            .write()
            .insert(main_task.id(), main_task.clone());

        let mut ctx = build_task_context(main_task.preempt_flag_ptr(), raya_engine::aot::helpers::create_default_helper_table(), None);
        ctx.shared_state = vm.shared_state() as *const _ as *mut ();
        ctx.current_task = Arc::as_ptr(&main_task) as *mut ();
        ctx.module = Arc::as_ptr(&runtime_module) as *const ();

        let frame = unsafe {
            allocate_initial_frame(
                global_func_id,
                loaded_entry.local_count,
                entry_ptr,
                &[],
                &ctx.helpers,
            )
        };
        if frame.is_null() {
            vm.shared_state().tasks.write().remove(&main_task.id());
            return Err(RuntimeError::Vm(VmError::RuntimeError(
                "AOT bundle entry frame allocation failed".to_string(),
            )));
        }

        let run_result = unsafe { run_aot_function(frame, &mut ctx, 100) };
        let result = self.finish_bundle_aot_result(vm, &main_task, run_result);
        vm.shared_state().tasks.write().remove(&main_task.id());
        vm.shared_state().mark_module_initialized(&runtime_module);
        result
    }

    #[cfg(feature = "aot")]
    fn finish_bundle_aot_result(
        &self,
        _vm: &mut raya_engine::vm::Vm,
        task: &Arc<Task>,
        run_result: AotRunResult,
    ) -> Result<Value, RuntimeError> {
        match run_result.result {
            raya_engine::vm::interpreter::ExecutionResult::Completed(value) => {
                if task.has_exception() {
                    task.fail();
                    return Err(RuntimeError::Vm(VmError::RuntimeError(
                        Self::task_exception_message(task),
                    )));
                }
                task.complete(value);
                Ok(value)
            }
            raya_engine::vm::interpreter::ExecutionResult::Failed(error) => {
                task.fail();
                Err(RuntimeError::Vm(error))
            }
            raya_engine::vm::interpreter::ExecutionResult::Suspended(reason) => {
                task.set_state(TaskState::Suspended);
                Err(RuntimeError::Vm(VmError::RuntimeError(format!(
                    "AOT bundle execution suspended with {:?}; scheduler-backed native bundle resume is not implemented yet",
                    reason
                ))))
            }
        }
    }

    #[cfg(feature = "aot")]
    fn task_exception_message(task: &Task) -> String {
        let Some(exc) = task.current_exception() else {
            return "AOT bundle task failed".to_string();
        };
        if exc.is_ptr() {
            if let Some(s) = unsafe { exc.as_ptr::<RayaString>() } {
                return unsafe { &*s.as_ptr() }.data.clone();
            }
            if let Some(obj) = unsafe { exc.as_ptr::<Object>() } {
                if let Some(msg_val) = unsafe { &*obj.as_ptr() }.get_field(0) {
                    if let Some(s) = unsafe { msg_val.as_ptr::<RayaString>() } {
                        return unsafe { &*s.as_ptr() }.data.clone();
                    }
                }
            }
        }
        format!("{:?}", exc)
    }
}

#[cfg(test)]
mod structural_slot_tests {
    use super::Runtime;

    #[test]
    fn structural_slot_map_maps_subset_by_member_name() {
        let expected = "obj(prop:a:rw:req:number,prop:c:rw:req:string)";
        let actual = "obj(prop:a:rw:req:number,prop:b:rw:req:boolean,prop:c:rw:req:string)";
        let slot_map = Runtime::structural_slot_map(expected, actual).expect("slot map expected");
        assert_eq!(slot_map, vec![Some(0), Some(2)]);
    }

    #[test]
    fn structural_slot_map_returns_none_for_non_object_signatures() {
        let expected = "fn(min=1,params=[number],rest=_,ret=number)";
        let actual = "fn(min=1,params=[number],rest=_,ret=number)";
        assert!(Runtime::structural_slot_map(expected, actual).is_none());
    }

    #[test]
    fn structural_slot_map_unions_merge_into_shared_layout_with_missing_slots() {
        let expected = "union(obj(prop:a:rw:req:number,prop:b:rw:req:number,prop:c:rw:req:number)|obj(prop:b:rw:req:number,prop:c:rw:req:number,prop:d:rw:req:number,prop:e:rw:req:number))";
        let actual = "obj(prop:a:rw:req:number,prop:b:rw:req:number,prop:c:rw:req:number)";
        let slot_map = Runtime::structural_slot_map(expected, actual).expect("slot map expected");
        assert_eq!(slot_map, vec![Some(0), Some(1), Some(2), None, None]);
    }
}
