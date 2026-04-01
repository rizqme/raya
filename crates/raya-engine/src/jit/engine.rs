//! Top-level JIT engine: manages compilation pipeline, code cache, and pre-warming.
//!
//! The engine owns a `cranelift_jit::JITModule` for producing executable native
//! code and a shared `CodeCache` for runtime dispatch by the interpreter.

use std::sync::Arc;

use cranelift_codegen::ir;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::Module as CraneliftModule;

use crate::compiler::bytecode::Module;
use crate::compiler::Opcode;
use crate::jit::analysis::heuristics::HeuristicsAnalyzer;
use crate::jit::backend::cranelift::lowering::{jit_entry_signature, LoweringContext};
use crate::jit::backend::traits::{CodegenError, ExecutableCode};
use crate::jit::backend::CraneliftBackend;
use crate::jit::ir::instr::JitFunction;
use crate::jit::pipeline::prewarm::PrewarmConfig;
use crate::jit::pipeline::JitPipeline;
use crate::jit::runtime::code_cache::{CodeCache, LayoutDependency};

/// Default code cache size: 64 MB
const DEFAULT_CODE_CACHE_SIZE: usize = 64 * 1024 * 1024;

/// Configuration for the JIT engine
#[derive(Clone)]
pub struct JitConfig {
    /// Maximum functions to pre-compile per module (default: 4)
    pub max_prewarm_functions: usize,
    /// Maximum compilation time per function in ms (default: 100)
    pub max_compile_time_ms: u64,
    /// Minimum heuristic score to be a JIT candidate (default: 10.0)
    pub min_score: f64,
    /// Minimum instruction count to be a JIT candidate (default: 8)
    pub min_instruction_count: usize,
    /// Maximum code cache size in bytes (default: 64 MB)
    pub max_code_cache_size: usize,
    /// Enable on-the-fly compilation based on runtime profiling (default: false).
    ///
    /// Disabled by default for stability until typed-lowering invariants are
    /// fully hardened for all real-world bytecode patterns.
    pub adaptive_compilation: bool,
    /// Call count threshold before compiling a function on-the-fly (default: 1000)
    pub call_threshold: u32,
    /// Loop iteration threshold before compiling a function on-the-fly (default: 10_000)
    pub loop_threshold: u32,
    /// Maximum bytecode size for on-the-fly compilation candidates (default: 4096)
    pub max_adaptive_function_size: usize,
}

impl Default for JitConfig {
    fn default() -> Self {
        JitConfig {
            // Enable static-analysis prewarm by default for better first-run performance.
            max_prewarm_functions: 4,
            max_compile_time_ms: 100,
            min_score: 10.0,
            min_instruction_count: 8,
            max_code_cache_size: DEFAULT_CODE_CACHE_SIZE,
            adaptive_compilation: false,
            call_threshold: 1000,
            loop_threshold: 10_000,
            max_adaptive_function_size: 4096,
        }
    }
}

/// Top-level JIT engine managing compilation and caching
pub struct JitEngine {
    /// Pipeline for lifting bytecode to optimized JIT IR
    pipeline: JitPipeline<CraneliftBackend>,
    /// Pre-warming configuration (heuristics, limits)
    prewarm_config: PrewarmConfig,
    /// Cranelift JIT module — owns executable memory for compiled functions
    jit_module: JITModule,
    /// Shared code cache — read by interpreter threads for dispatch
    code_cache: Arc<CodeCache>,
}

impl JitEngine {
    /// Create a new JIT engine with default configuration
    pub fn new() -> Result<Self, CodegenError> {
        Self::with_config(JitConfig::default())
    }

    /// Create a new JIT engine with custom configuration
    pub fn with_config(config: JitConfig) -> Result<Self, CodegenError> {
        let backend = CraneliftBackend::host()?;
        let pipeline = JitPipeline::new(backend);

        let mut analyzer = HeuristicsAnalyzer::new();
        analyzer.min_score = config.min_score;
        analyzer.min_instruction_count = config.min_instruction_count;

        let prewarm_config = PrewarmConfig {
            max_functions: config.max_prewarm_functions,
            max_compile_time_ms: config.max_compile_time_ms,
            analyzer,
        };

        // Create the cranelift_jit JITModule for executable code
        let jit_module = Self::create_jit_module()?;

        let code_cache = Arc::new(CodeCache::new(config.max_code_cache_size));

        Ok(JitEngine {
            pipeline,
            prewarm_config,
            jit_module,
            code_cache,
        })
    }

    /// Create a cranelift_jit JITModule targeting the host
    fn create_jit_module() -> Result<JITModule, CodegenError> {
        let mut flag_builder = settings::builder();
        flag_builder
            .set("opt_level", "speed")
            .map_err(|e| CodegenError::BackendError(format!("Failed to set opt_level: {}", e)))?;
        // JITModule manages its own memory layout, so PIC is not needed
        flag_builder
            .set("is_pic", "false")
            .map_err(|e| CodegenError::BackendError(format!("Failed to set is_pic: {}", e)))?;
        let flags = settings::Flags::new(flag_builder);

        let isa = cranelift_native::builder()
            .map_err(|e| CodegenError::BackendError(format!("Failed to create native ISA: {}", e)))?
            .finish(flags)
            .map_err(|e| CodegenError::BackendError(format!("Failed to finish ISA: {}", e)))?;

        let builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        Ok(JITModule::new(builder))
    }

    /// Pre-warm a module: analyze, compile hot functions, and cache them.
    ///
    /// Selects JIT candidates via heuristics, lifts them through the pipeline,
    /// compiles to native code via JITModule, and stores pointers in the CodeCache.
    /// Returns the number of functions successfully compiled.
    pub fn prewarm(&mut self, module: &Module) -> PrewarmSummary {
        let module_id = self.code_cache.register_module(module.checksum);
        let candidates = self.prewarm_config.analyzer.select_candidates(module);

        let mut compiled_count = 0u32;
        let mut failed_count = 0u32;

        for candidate in candidates.iter().take(self.prewarm_config.max_functions) {
            let func_idx = candidate.func_index;
            if func_idx >= module.functions.len() {
                continue;
            }

            let compile_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.compile_to_cache(module, func_idx, module_id)
            }))
            .map_err(|_| "panic during JIT compile".to_string())
            .and_then(|r| r);

            match compile_result {
                Ok(()) => compiled_count += 1,
                Err(err) => {
                    failed_count += 1;
                    if std::env::var("RAYA_JIT_DEBUG").is_ok() {
                        eprintln!(
                            "JIT prewarm compile failed: func={} name={} err={}",
                            func_idx, module.functions[func_idx].name, err
                        );
                    }
                }
            }
        }

        PrewarmSummary {
            module_id,
            compiled: compiled_count,
            failed: failed_count,
        }
    }

    /// Compile a selected set of functions synchronously and store them in the cache.
    ///
    /// This is used for first-run acceleration of entry functions that only
    /// execute once, where background compilation would arrive too late.
    pub fn compile_selected<I>(
        &mut self,
        module: &Module,
        module_id: u64,
        func_indices: I,
    ) -> PrewarmSummary
    where
        I: IntoIterator<Item = usize>,
    {
        let mut compiled_count = 0u32;
        let mut failed_count = 0u32;

        for func_idx in func_indices {
            if func_idx >= module.functions.len() {
                continue;
            }

            let compile_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.compile_to_cache(module, func_idx, module_id)
            }))
            .map_err(|_| "panic during JIT compile".to_string())
            .and_then(|r| r);

            match compile_result {
                Ok(()) => compiled_count += 1,
                Err(err) => {
                    failed_count += 1;
                    if std::env::var("RAYA_JIT_DEBUG").is_ok() {
                        eprintln!(
                            "JIT selected compile failed: func={} name={} err={}",
                            func_idx, module.functions[func_idx].name, err
                        );
                    }
                }
            }
        }

        PrewarmSummary {
            module_id,
            compiled: compiled_count,
            failed: failed_count,
        }
    }

    /// Compile a single function and store the executable code in the cache.
    fn compile_to_cache(
        &mut self,
        module: &Module,
        func_idx: usize,
        module_id: u64,
    ) -> Result<(), String> {
        // Step 1: Lift bytecode → optimized JIT IR
        let jit_func = self
            .pipeline
            .lift_and_optimize(&module.functions[func_idx], module, func_idx as u32)
            .map_err(|e| format!("Lift/optimize failed: {}", e))?;

        // Step 2: Lower JIT IR → Cranelift IR → executable code via JITModule
        self.compile_jit_function(&jit_func, func_idx, module, module_id)
    }

    /// Lower a JitFunction through the JITModule to produce executable code.
    fn compile_jit_function(
        &mut self,
        jit_func: &JitFunction,
        func_idx: usize,
        module: &Module,
        module_id: u64,
    ) -> Result<(), String> {
        let call_conv = self.jit_module.isa().default_call_conv();
        let sig = jit_entry_signature(call_conv);

        // Each function gets a unique name in the JITModule
        let name = format!("jit_m{}_f{}", module_id, func_idx);
        let func_id = self
            .jit_module
            .declare_function(&name, cranelift_module::Linkage::Local, &sig)
            .map_err(|e| format!("Declare failed: {}", e))?;

        // Build Cranelift IR from JIT IR
        let mut ctx = Context::new();
        ctx.func.signature = jit_entry_signature(call_conv);
        ctx.func.name = ir::UserFuncName::user(0, jit_func.func_index);

        {
            let mut func_builder_ctx = cranelift_frontend::FunctionBuilderContext::new();
            let builder =
                cranelift_frontend::FunctionBuilder::new(&mut ctx.func, &mut func_builder_ctx);
            LoweringContext::lower(jit_func, module, builder)
                .map_err(|e| format!("Lowering failed: {}", e))?;
        }

        // Define and finalize — this compiles and places code in executable memory
        self.jit_module
            .define_function(func_id, &mut ctx)
            .map_err(|e| format!("Define failed: {:?}", e))?;

        // Get compiled code size from the context (available after define_function)
        let code_size = ctx
            .compiled_code()
            .map(|c| c.code_buffer().len())
            .unwrap_or(0);

        self.jit_module
            .finalize_definitions()
            .map_err(|e| format!("Finalize failed: {}", e))?;

        // Get the executable function pointer
        let code_ptr = self.jit_module.get_finalized_function(func_id);

        let executable = ExecutableCode {
            code_ptr,
            code_size,
            entry_offset: 0,
            stack_maps: vec![],
        };

        let layout_dependencies = self.collect_layout_dependencies(module, func_idx);
        self.code_cache.insert_with_dependencies(
            module_id,
            func_idx as u32,
            executable,
            layout_dependencies,
        );
        Ok(())
    }

    fn collect_layout_dependencies(
        &self,
        module: &Module,
        func_idx: usize,
    ) -> Vec<LayoutDependency> {
        let Some(function) = module.functions.get(func_idx) else {
            return Vec::new();
        };
        let mut deps = std::collections::BTreeSet::new();
        let mut ip = 0usize;
        let code = &function.code;
        while ip < code.len() {
            let Some(opcode) = Opcode::from_u8(code[ip]) else {
                break;
            };
            ip += 1;
            match opcode {
                Opcode::NewType
                | Opcode::ConstructType
                | Opcode::IsNominal
                | Opcode::CastNominal
                | Opcode::LoadFieldExact
                | Opcode::StoreFieldExact
                | Opcode::OptionalFieldExact
                | Opcode::CallMethodExact
                | Opcode::OptionalCallMethodExact
                | Opcode::CallMethodShape
                | Opcode::OptionalCallMethodShape
                | Opcode::LoadFieldShape
                | Opcode::StoreFieldShape
                | Opcode::OptionalFieldShape
                | Opcode::ImplementsShape
                | Opcode::CastShape
                | Opcode::DynGetKeyed
                | Opcode::DynSetKeyed
                | Opcode::ObjectLiteral
                | Opcode::InitObject
                | Opcode::BindMethod => {
                    deps.insert(LayoutDependency::AnyLayout);
                }
                _ => {}
            }
            let operand_len = crate::compiler::codegen::emit::opcode_size(opcode).saturating_sub(1);
            ip = ip.saturating_add(operand_len.min(code.len().saturating_sub(ip)));
        }
        deps.into_iter().collect()
    }

    /// Get the shared code cache (for passing to interpreter threads)
    pub fn code_cache(&self) -> &Arc<CodeCache> {
        &self.code_cache
    }

    /// Get a reference to the compilation pipeline
    pub fn pipeline(&self) -> &JitPipeline<CraneliftBackend> {
        &self.pipeline
    }

    /// Register a module in the code cache and return its ID.
    ///
    /// Call this before `start_background()` to get a module ID for
    /// constructing `CompilationRequest`s.
    pub fn register_module(&self, checksum: [u8; 32]) -> u64 {
        self.code_cache.register_module(checksum)
    }

    /// Start the background compilation thread for on-the-fly JIT compilation.
    ///
    /// Consumes the engine and moves it to a dedicated thread that processes
    /// `CompilationRequest`s from interpreter worker threads. Compiled code is
    /// inserted into the shared `CodeCache`, where workers pick it up on the
    /// next function call (no explicit notification needed).
    ///
    /// Returns a `BackgroundCompiler` handle for submitting requests.
    /// Dropping the handle closes the channel and the thread exits.
    pub fn start_background(self) -> crate::jit::profiling::BackgroundCompiler {
        let (tx, rx) = crossbeam::channel::bounded::<crate::jit::profiling::CompilationRequest>(64);

        std::thread::Builder::new()
            .name("jit-compiler".into())
            .spawn(move || {
                let mut engine = self;
                while let Ok(req) = rx.recv() {
                    // Skip if already compiled (another request may have beaten us)
                    if engine
                        .code_cache
                        .contains(req.module_id, req.func_index as u32)
                    {
                        if let Some(fp) = req.module_profile.get(req.func_index) {
                            fp.finish_compile()
                        }
                        continue;
                    }

                    let compile_result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            engine.compile_to_cache(&req.module, req.func_index, req.module_id)
                        }))
                        .map_err(|_| "panic during JIT compile".to_string())
                        .and_then(|r| r);

                    match compile_result {
                        Ok(()) => {
                            // Mark profile so workers see jit_available and stop requesting
                            if let Some(fp) = req.module_profile.get(req.func_index) {
                                fp.finish_compile();
                            }
                        }
                        Err(err) => {
                            // Compilation failed (e.g., loops in SSA lifter).
                            // Clear the compiling flag so it's not stuck, but don't mark available.
                            if let Some(fp) = req.module_profile.get(req.func_index) {
                                fp.finish_compile_failed();
                            }
                            if std::env::var("RAYA_JIT_DEBUG").is_ok() {
                                let name = req
                                    .module
                                    .functions
                                    .get(req.func_index)
                                    .map(|f| f.name.as_str())
                                    .unwrap_or("<unknown>");
                                eprintln!(
                                    "JIT adaptive compile failed: func={} name={} err={}",
                                    req.func_index, name, err
                                );
                            }
                        }
                    }
                }
            })
            .expect("Failed to spawn JIT compiler thread");

        crate::jit::profiling::BackgroundCompiler::new(tx)
    }
}

// Safety: JitEngine is only mutated from the thread that owns the Vm.
// The JITModule's executable memory is immutable after finalization and
// accessed read-only through the CodeCache (which uses RwLock internally).
unsafe impl Send for JitEngine {}
unsafe impl Sync for JitEngine {}

/// Summary of a pre-warm operation
#[derive(Debug, Clone)]
pub struct PrewarmSummary {
    /// Module ID assigned for cache lookups
    pub module_id: u64,
    /// Number of functions successfully compiled
    pub compiled: u32,
    /// Number of functions that failed compilation
    pub failed: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::bytecode::{ConstantPool, Function, Metadata, Opcode};
    use crate::jit::backend::traits::CodegenBackend;

    fn make_module(functions: Vec<Function>) -> Module {
        Module {
            magic: *b"RAYA",
            version: 1,
            flags: 0,
            constants: ConstantPool::new(),
            functions,
            classes: vec![],
            metadata: Metadata {
                name: "test_module".to_string(),
                source_file: None,
                generic_templates: vec![],
                template_symbol_table: vec![],
                mono_debug_map: vec![],
                js_global_bindings: vec![],
                structural_shapes: vec![],
                structural_layouts: vec![],
            },
            exports: vec![],
            imports: vec![],
            checksum: [0; 32],
            reflection: None,
            debug_info: None,
            native_functions: vec![],
            jit_hints: vec![],
        }
    }

    fn emit(code: &mut Vec<u8>, op: Opcode) {
        code.push(op as u8);
    }

    fn emit_i32(code: &mut Vec<u8>, val: i32) {
        code.push(Opcode::ConstI32 as u8);
        code.extend_from_slice(&val.to_le_bytes());
    }

    #[test]
    fn test_engine_creation() {
        let engine = JitEngine::new().unwrap();
        assert_eq!(engine.pipeline().backend().name(), "cranelift");
    }

    #[test]
    fn test_engine_with_config() {
        let config = JitConfig {
            max_prewarm_functions: 4,
            min_score: 5.0,
            ..Default::default()
        };
        let engine = JitEngine::with_config(config).unwrap();
        assert_eq!(engine.pipeline().backend().name(), "cranelift");
    }

    #[test]
    fn test_engine_default_stability_bias() {
        let config = JitConfig::default();
        assert_eq!(config.max_prewarm_functions, 4);
        assert!(!config.adaptive_compilation);
    }

    #[test]
    fn test_engine_prewarm_empty_module() {
        let mut engine = JitEngine::new().unwrap();
        let module = crate::compiler::bytecode::Module::new("test".to_string());
        let summary = engine.prewarm(&module);
        assert_eq!(summary.compiled, 0);
        assert_eq!(summary.failed, 0);
    }

    #[test]
    fn test_engine_prewarm_caches_code() {
        let config = JitConfig {
            min_score: 1.0,
            min_instruction_count: 2,
            max_prewarm_functions: 1,
            ..Default::default()
        };
        let mut engine = JitEngine::with_config(config).unwrap();

        // Build a math-heavy function that exceeds the heuristic threshold
        let mut code = Vec::new();
        for _ in 0..4 {
            emit_i32(&mut code, 1);
            emit_i32(&mut code, 2);
            emit(&mut code, Opcode::Iadd);
            emit_i32(&mut code, 3);
            emit(&mut code, Opcode::Imul);
        }
        for _ in 0..3 {
            emit(&mut code, Opcode::Iadd);
        }
        emit(&mut code, Opcode::Return);

        let module = make_module(vec![Function {
            name: "compute".to_string(),
            param_count: 0,
            local_count: 0,
            code,
            ..Default::default()
        }]);

        let summary = engine.prewarm(&module);

        // The function should be compiled and cached
        assert!(
            summary.compiled > 0,
            "Expected at least one compiled function"
        );
        assert!(
            engine.code_cache().contains(summary.module_id, 0),
            "Compiled function should be in the code cache"
        );
    }

    #[test]
    fn test_engine_code_cache_shared() {
        let engine = JitEngine::new().unwrap();
        let cache1 = engine.code_cache().clone();
        let cache2 = engine.code_cache().clone();
        // Both Arc clones point to the same cache
        assert!(Arc::ptr_eq(&cache1, &cache2));
    }
}
