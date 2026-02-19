//! Top-level JIT engine: manages compilation pipeline, code cache, and pre-warming.

use crate::compiler::bytecode::Module;

use crate::jit::analysis::heuristics::HeuristicsAnalyzer;
use crate::jit::backend::CraneliftBackend;
use crate::jit::backend::traits::CodegenError;
use crate::jit::pipeline::JitPipeline;
use crate::jit::pipeline::prewarm::{PrewarmConfig, PrewarmResult, prewarm_module};

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
}

impl Default for JitConfig {
    fn default() -> Self {
        JitConfig {
            max_prewarm_functions: 16,
            max_compile_time_ms: 100,
            min_score: 10.0,
            min_instruction_count: 8,
        }
    }
}

/// Top-level JIT engine managing compilation and caching
pub struct JitEngine {
    pipeline: JitPipeline<CraneliftBackend>,
    prewarm_config: PrewarmConfig,
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

        Ok(JitEngine {
            pipeline,
            prewarm_config,
        })
    }

    /// Pre-warm a module by analyzing and compiling CPU-intensive functions.
    ///
    /// Returns the pre-warm result with compiled functions and skip reasons.
    pub fn prewarm(&self, module: &Module) -> PrewarmResult {
        prewarm_module(module, &self.prewarm_config, &self.pipeline)
    }

    /// Get a reference to the compilation pipeline
    pub fn pipeline(&self) -> &JitPipeline<CraneliftBackend> {
        &self.pipeline
    }
}

// JitEngine is safe to share across threads
unsafe impl Send for JitEngine {}
unsafe impl Sync for JitEngine {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::backend::traits::CodegenBackend;

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
        let engine = JitEngine::new().unwrap();
        let module = crate::compiler::bytecode::Module::new("test".to_string());
        let result = engine.prewarm(&module);
        assert!(result.compiled.is_empty());
    }
}
