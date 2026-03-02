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

use raya_engine::parser::Interner;
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
        Ok(compiler.compile_program_source(source, &virtual_entry)?.entry)
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
        Ok(compiler.compile_program_source(source, &virtual_entry)?.entry)
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
        self.compile_program_file_with_options(path, &compile::CompileOptions::default())
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
            compile_options: Some(options.clone()),
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

        // Register dependency modules
        for dep in deps {
            let shared = vm.shared_state();
            shared
                .register_module(Arc::new(dep.module.clone()))
                .map_err(RuntimeError::Dependency)?;
        }

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
