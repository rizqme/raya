//! Top-level JIT engine: manages compilation pipeline, code cache, and pre-warming.
//!
//! The engine owns a `cranelift_jit::JITModule` for producing executable native
//! code and a shared `CodeCache` for runtime dispatch by the interpreter.

use std::sync::Arc;

use cranelift_codegen::ir;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_frontend::FunctionBuilderContext;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::Module as CraneliftModule;

use crate::compiler::bytecode::Module;
use crate::jit::analysis::heuristics::HeuristicsAnalyzer;
use crate::jit::backend::cranelift::lowering::{jit_entry_signature, LoweringContext};
use crate::jit::backend::CraneliftBackend;
use crate::jit::backend::traits::{CodegenError, ExecutableCode};
use crate::jit::ir::instr::JitFunction;
use crate::jit::pipeline::prewarm::PrewarmConfig;
use crate::jit::pipeline::JitPipeline;
use crate::jit::runtime::code_cache::CodeCache;

/// Default code cache size: 64 MB
const DEFAULT_CODE_CACHE_SIZE: usize = 64 * 1024 * 1024;

/// Configuration for the JIT engine
pub struct JitConfig {
    /// Maximum functions to pre-compile per module (default: 16)
    pub max_prewarm_functions: usize,
    /// Maximum compilation time per function in ms (default: 100)
    pub max_compile_time_ms: u64,
    /// Minimum heuristic score to be a JIT candidate (default: 10.0)
    pub min_score: f64,
    /// Minimum instruction count to be a JIT candidate (default: 8)
    pub min_instruction_count: usize,
    /// Maximum code cache size in bytes (default: 64 MB)
    pub max_code_cache_size: usize,
}

impl Default for JitConfig {
    fn default() -> Self {
        JitConfig {
            max_prewarm_functions: 16,
            max_compile_time_ms: 100,
            min_score: 10.0,
            min_instruction_count: 8,
            max_code_cache_size: DEFAULT_CODE_CACHE_SIZE,
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
    /// Reusable context for building Cranelift IR
    func_builder_ctx: FunctionBuilderContext,
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
            func_builder_ctx: FunctionBuilderContext::new(),
            code_cache,
        })
    }

    /// Create a cranelift_jit JITModule targeting the host
    fn create_jit_module() -> Result<JITModule, CodegenError> {
        let mut flag_builder = settings::builder();
        flag_builder.set("opt_level", "speed").map_err(|e| {
            CodegenError::BackendError(format!("Failed to set opt_level: {}", e))
        })?;
        // JITModule manages its own memory layout, so PIC is not needed
        flag_builder.set("is_pic", "false").map_err(|e| {
            CodegenError::BackendError(format!("Failed to set is_pic: {}", e))
        })?;
        let flags = settings::Flags::new(flag_builder);

        let isa = cranelift_native::builder()
            .map_err(|e| {
                CodegenError::BackendError(format!("Failed to create native ISA: {}", e))
            })?
            .finish(flags)
            .map_err(|e| {
                CodegenError::BackendError(format!("Failed to finish ISA: {}", e))
            })?;

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

            match self.compile_to_cache(module, func_idx, module_id) {
                Ok(()) => compiled_count += 1,
                Err(_) => failed_count += 1,
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
        self.compile_jit_function(&jit_func, func_idx, module_id)
    }

    /// Lower a JitFunction through the JITModule to produce executable code.
    fn compile_jit_function(
        &mut self,
        jit_func: &JitFunction,
        func_idx: usize,
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
            let builder = cranelift_frontend::FunctionBuilder::new(
                &mut ctx.func,
                &mut self.func_builder_ctx,
            );
            LoweringContext::lower(jit_func, builder)
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
            code_ptr: code_ptr as *const u8,
            code_size,
            entry_offset: 0,
            stack_maps: vec![],
            deopt_info: vec![],
        };

        self.code_cache
            .insert(module_id, func_idx as u32, executable);
        Ok(())
    }

    /// Get the shared code cache (for passing to interpreter threads)
    pub fn code_cache(&self) -> &Arc<CodeCache> {
        &self.code_cache
    }

    /// Get a reference to the compilation pipeline
    pub fn pipeline(&self) -> &JitPipeline<CraneliftBackend> {
        &self.pipeline
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
    use crate::jit::backend::traits::CodegenBackend;
    use crate::compiler::bytecode::{ConstantPool, Function, Metadata, Opcode};

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
            },
            exports: vec![],
            imports: vec![],
            checksum: [0; 32],
            reflection: None,
            debug_info: None,
            native_functions: vec![],
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
        }]);

        let summary = engine.prewarm(&module);

        // The function should be compiled and cached
        assert!(summary.compiled > 0, "Expected at least one compiled function");
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
