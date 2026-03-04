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

use raya_engine::compiler::module::{LateLinkRequirement, LateLinkSymbolRequirement};
use raya_engine::compiler::{module_id_from_name, SymbolType};
use raya_engine::parser::Interner;
use raya_engine::vm::module::{ModuleLinker, ResolvedSymbol};
use raya_engine::vm::object::{Closure, RayaString};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

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
    fn resolve_ts_options_for_inline(&self) -> Result<Option<TsCompilerOptions>, RuntimeError> {
        if !matches!(self.options.type_mode, Some(TypeMode::Ts)) {
            return Ok(self.options.ts_options.clone());
        }
        if let Some(opts) = &self.options.ts_options {
            return Ok(Some(opts.clone()));
        }
        let cwd = std::env::current_dir().map_err(|e| {
            RuntimeError::TypeCheck(format!("Failed to determine current directory: {}", e))
        })?;
        let tsconfig = loader::find_tsconfig(&cwd).ok_or_else(|| {
            RuntimeError::TypeCheck(
                "Type mode 'ts' requires a discoverable tsconfig.json".to_string(),
            )
        })?;
        Ok(Some(loader::load_ts_compiler_options(&tsconfig)?))
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
    /// Automatically includes builtin classes (Map, Set, Date, etc.) and
    /// standard library modules (logger, math, crypto, etc.).
    pub fn compile(&self, source: &str) -> Result<CompiledModule, RuntimeError> {
        Ok(self.compile_program_source(source)?.entry)
    }

    /// Compile a Raya source string into a full binary-linked program graph.
    ///
    /// Returns the entry module plus all compiled dependencies and late-link metadata.
    pub fn compile_program_source(&self, source: &str) -> Result<CompiledProgram, RuntimeError> {
        let type_mode = self
            .options
            .type_mode
            .unwrap_or_else(|| compile::default_type_mode_for_builtin(self.options.builtin_mode));
        let ts_options = self.resolve_ts_options_for_inline()?;
        let virtual_entry = std::env::current_dir()
            .map_err(RuntimeError::Io)?
            .join("__raya_inline_entry.raya");
        let compiler = module_system::ProgramCompiler {
            builtin_mode: self.options.builtin_mode,
            type_mode,
            ts_options,
            compile_options: None,
        };
        compiler.compile_program_source(source, &virtual_entry)
    }

    /// Compile a .raya source file to a bytecode module.
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
        let virtual_entry = std::env::current_dir()
            .map_err(RuntimeError::Io)?
            .join("__raya_inline_entry.raya");
        let compiler = module_system::ProgramCompiler {
            builtin_mode: self.options.builtin_mode,
            type_mode,
            ts_options,
            compile_options: Some(options.clone()),
        };
        Ok(compiler
            .compile_program_source(source, &virtual_entry)?
            .entry)
    }

    /// Compile a .raya source file with options (e.g., source map).
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
        let virtual_entry = std::env::current_dir()
            .map_err(RuntimeError::Io)?
            .join("__raya_inline_entry.raya");
        let compiler = module_system::ProgramCompiler {
            builtin_mode: self.options.builtin_mode,
            type_mode,
            ts_options,
            compile_options: None,
        };
        Ok(compiler
            .check_program_source(source, &virtual_entry)?
            .diagnostics)
    }

    /// Type-check a .raya source file without generating bytecode.
    pub fn check_file(&self, path: &Path) -> Result<compile::CheckDiagnostics, RuntimeError> {
        Ok(self.check_program_file(path)?.diagnostics)
    }

    /// Compile a full file program (entry + resolved local module graph).
    pub fn compile_program_file(&self, path: &Path) -> Result<CompiledProgram, RuntimeError> {
        let type_mode = self
            .options
            .type_mode
            .unwrap_or_else(|| compile::default_type_mode_for_builtin(self.options.builtin_mode));
        let ts_options = if matches!(type_mode, TypeMode::Ts) {
            let tsconfig =
                loader::find_tsconfig(path.parent().unwrap_or(path)).ok_or_else(|| {
                    RuntimeError::TypeCheck(
                        "Type mode 'ts' requires a discoverable tsconfig.json".to_string(),
                    )
                })?;
            Some(loader::load_ts_compiler_options(&tsconfig)?)
        } else {
            self.options.ts_options.clone()
        };

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
        let ts_options = if matches!(type_mode, TypeMode::Ts) {
            let tsconfig =
                loader::find_tsconfig(path.parent().unwrap_or(path)).ok_or_else(|| {
                    RuntimeError::TypeCheck(
                        "Type mode 'ts' requires a discoverable tsconfig.json".to_string(),
                    )
                })?;
            Some(loader::load_ts_compiler_options(&tsconfig)?)
        } else {
            self.options.ts_options.clone()
        };

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
        let ts_options = if matches!(type_mode, TypeMode::Ts) {
            let tsconfig =
                loader::find_tsconfig(path.parent().unwrap_or(path)).ok_or_else(|| {
                    RuntimeError::TypeCheck(
                        "Type mode 'ts' requires a discoverable tsconfig.json".to_string(),
                    )
                })?;
            Some(loader::load_ts_compiler_options(&tsconfig)?)
        } else {
            self.options.ts_options.clone()
        };

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
        let module = self.compile(code)?;
        self.execute(&module)
    }

    /// Run a file (.raya or .ryb), auto-detecting format by extension.
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

        if path.extension().and_then(|e| e.to_str()) == Some("raya")
            && self.can_use_binary_program_execution()
        {
            let program = self.compile_program_file(&path)?;
            let result = self.execute_program(&program);
            return match result {
                Ok(_) => Ok(0),
                Err(RuntimeError::Vm(e)) => {
                    eprintln!("Runtime error: {}", e);
                    Ok(1)
                }
                Err(e) => Err(e),
            };
        }

        let module = match path.extension().and_then(|e| e.to_str()) {
            Some("ryb") => self.load_bytecode(&path)?,
            Some("raya") => self.compile_file(&path)?,
            Some("bundle") => self.load_bundle_entry_module(&path)?,
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

    /// Run a file with explicitly provided dependency modules.
    pub fn run_file_with_deps(
        &self,
        path: &Path,
        deps: &[CompiledModule],
    ) -> Result<i32, RuntimeError> {
        let module = match path.extension().and_then(|e| e.to_str()) {
            Some("ryb") => self.load_bytecode(path)?,
            Some("raya") => self.compile_file(path)?,
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
                let dep_module = linker.get_module_by_id(import.module_id).cloned().ok_or_else(|| {
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

    fn execute_with_deps_in_vm(
        &self,
        vm: &mut raya_engine::vm::Vm,
        module: &CompiledModule,
        deps: &[CompiledModule],
    ) -> Result<Value, RuntimeError> {
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
                let _ = vm.execute_entry_only(&current_module)?;
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

            let value = Self::materialize_import_value(vm, import.symbol.as_str(), resolved_symbol)?;
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
        import_symbol: &str,
        resolved: &ResolvedSymbol,
    ) -> Result<Value, RuntimeError> {
        match resolved.export.symbol_type {
            SymbolType::Function => {
                let closure = Closure::with_module(
                    resolved.export.index,
                    Vec::new(),
                    resolved.module.clone(),
                );
                let gc_ptr = vm.shared_state().gc.lock().allocate(closure);
                Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) })
            }
            SymbolType::Constant => Self::materialize_constant_export(vm, resolved),
            SymbolType::Class => {
                let class_id = vm
                    .shared_state()
                    .resolve_class_id(&resolved.module, resolved.export.index);
                if class_id > i32::MAX as usize {
                    return Err(RuntimeError::Dependency(format!(
                        "Imported class symbol '{}' from '{}' has class ID {} outside i32 range",
                        import_symbol, resolved.module.metadata.name, class_id
                    )));
                }
                Ok(Value::i32(class_id as i32))
            }
        }
    }

    fn materialize_constant_export(
        vm: &mut raya_engine::vm::Vm,
        resolved: &ResolvedSymbol,
    ) -> Result<Value, RuntimeError> {
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
                if let Some(value) = vm.shared_state().globals_by_index.read().get(global_slot).copied() {
                    return Ok(value);
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
                return Ok(value);
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
            return Ok(Value::f64(constants.floats[index - float_base]));
        }

        Err(RuntimeError::Dependency(format!(
            "Constant export index {} is out of range in module '{}'",
            index, resolved.module.metadata.name
        )))
    }

    fn maybe_enable_jit(&self, vm: &mut raya_engine::vm::Vm) {
        #[cfg(feature = "jit")]
        {
            if self.options.no_jit {
                return;
            }

            let config = raya_engine::jit::JitConfig {
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
            let extension = candidate.extension().and_then(|ext| ext.to_str()).unwrap_or("");
            let loaded = match extension {
                "ryb" => self.load_bytecode(candidate),
                "raya" => self.compile_file(candidate),
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
            add_expanded(
                &mut candidates,
                &mut seen,
                &entry_dir.join(fallback_name),
            );
            add_expanded(
                &mut candidates,
                &mut seen,
                &entry_dir.join(".raya").join("packages").join(fallback_name).join("lib"),
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
        let Some(exported) = module.exports.iter().find(|export| export.symbol_id == symbol.symbol_id) else {
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

        if exported.type_symbol_id != symbol.type_symbol_id {
            let actual_pretty = exported
                .type_signature
                .as_deref()
                .map(str::to_string)
                .unwrap_or_else(|| format!("hash:{:016x}", exported.type_symbol_id));
            return Err(RuntimeError::Dependency(format!(
                "Late-link type signature mismatch for '{}': expected {:#x} ({}), got {:#x} ({})",
                symbol.symbol,
                symbol.type_symbol_id,
                symbol.type_signature,
                exported.type_symbol_id,
                actual_pretty
            )));
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
                let has_template_export = module.exports.iter().any(|entry| entry.name == *template);
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
        if self.options.builtin_mode != BuiltinMode::RayaStrict {
            return false;
        }
        let type_mode = self
            .options
            .type_mode
            .unwrap_or_else(|| compile::default_type_mode_for_builtin(self.options.builtin_mode));
        if type_mode != TypeMode::Raya {
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

        // For now, execute embedded bytecode entry from VFS as a fallback path.
        // This allows bundled artifacts to run by default even when native AOT
        // execution is not available on this host/runtime build.
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
}
