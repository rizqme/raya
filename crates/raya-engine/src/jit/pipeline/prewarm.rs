//! Pre-warming: compile JIT candidates at module load time
//!
//! Analyzes a module's functions using static heuristics and compiles
//! the most CPU-intensive candidates before execution begins.

use std::time::Instant;

use crate::compiler::bytecode::Module;

use crate::jit::analysis::heuristics::{FunctionScore, HeuristicsAnalyzer};
use crate::jit::backend::traits::{CodegenBackend, CompiledCode};
use crate::jit::pipeline::JitPipeline;

/// Configuration for pre-warming
pub struct PrewarmConfig {
    /// Maximum number of functions to pre-compile (default: 16)
    pub max_functions: usize,
    /// Maximum compilation time per function in milliseconds (default: 100)
    pub max_compile_time_ms: u64,
    /// The heuristics analyzer
    pub analyzer: HeuristicsAnalyzer,
}

impl Default for PrewarmConfig {
    fn default() -> Self {
        PrewarmConfig {
            max_functions: 16,
            max_compile_time_ms: 100,
            analyzer: HeuristicsAnalyzer::default(),
        }
    }
}

/// Result of pre-warming a module
#[derive(Debug)]
pub struct PrewarmResult {
    /// Successfully compiled functions: (func_index, compiled code)
    pub compiled: Vec<(usize, CompiledCode)>,
    /// Skipped functions: (func_index, reason)
    pub skipped: Vec<(usize, String)>,
    /// Total time spent pre-warming in milliseconds
    pub total_time_ms: u64,
}

/// Pre-warm a module by analyzing and compiling CPU-intensive functions.
///
/// 1. Run heuristics to identify JIT candidates
/// 2. Take the top `max_functions` candidates by score
/// 3. Compile each through the full pipeline
/// 4. Return compiled code for caching
pub fn prewarm_module<B: CodegenBackend>(
    module: &Module,
    config: &PrewarmConfig,
    pipeline: &JitPipeline<B>,
) -> PrewarmResult {
    let start = Instant::now();
    let mut compiled = Vec::new();
    let mut skipped = Vec::new();

    // Step 1: Select candidates
    let candidates = config.analyzer.select_candidates(module);

    // Step 2: Take top N
    let to_compile: Vec<&FunctionScore> = candidates.iter()
        .take(config.max_functions)
        .collect();

    // Step 3: Compile each candidate
    for candidate in to_compile {
        let func_idx = candidate.func_index;

        if func_idx >= module.functions.len() {
            skipped.push((func_idx, "Invalid function index".to_string()));
            continue;
        }

        let func_start = Instant::now();

        match pipeline.compile_function(&module.functions[func_idx], module, func_idx as u32) {
            Ok((_jit_func, code)) => {
                let elapsed = func_start.elapsed().as_millis() as u64;
                if elapsed > config.max_compile_time_ms {
                    // Still compiled, but log that it exceeded time budget
                    skipped.push((func_idx, format!("Compiled but slow ({}ms)", elapsed)));
                }
                compiled.push((func_idx, code));
            }
            Err(e) => {
                skipped.push((func_idx, format!("Compilation failed: {}", e)));
            }
        }
    }

    let total_time_ms = start.elapsed().as_millis() as u64;

    PrewarmResult {
        compiled,
        skipped,
        total_time_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::backend::StubBackend;
    use crate::compiler::bytecode::{ConstantPool, Function, Metadata, Opcode};

    fn make_module(functions: Vec<Function>) -> Module {
        Module {
            magic: *b"RAYA",
            version: 1,
            flags: 0,
            constants: ConstantPool::new(),
            functions,
            classes: vec![],
            metadata: Metadata { name: "test".to_string(), source_file: None },
            exports: vec![],
            imports: vec![],
            checksum: [0; 32],
            reflection: None,
            debug_info: None,
            native_functions: vec![],
        }
    }

    fn emit_i32(code: &mut Vec<u8>, val: i32) {
        code.push(Opcode::ConstI32 as u8);
        code.extend_from_slice(&val.to_le_bytes());
    }
    fn emit(code: &mut Vec<u8>, op: Opcode) { code.push(op as u8); }
    fn emit_jmp(code: &mut Vec<u8>, op: Opcode, offset: i32) {
        code.push(op as u8);
        code.extend_from_slice(&offset.to_le_bytes());
    }
    fn emit_local(code: &mut Vec<u8>, op: Opcode, idx: u16) {
        code.push(op as u8);
        code.extend_from_slice(&idx.to_le_bytes());
    }

    #[test]
    fn test_prewarm_compiles_candidates() {
        // Create a module with one arithmetic-heavy function and one trivial.
        // Uses straight-line code (no loops) because the SSA lifter doesn't yet
        // propagate stack state across block boundaries for backward jumps.
        let mut math_code = Vec::new();
        // Lots of straight-line arithmetic: (1+2)*3 + (4+5)*6 + (7+8)*9 + (10+11)*12
        for _ in 0..4 {
            emit_i32(&mut math_code, 1);
            emit_i32(&mut math_code, 2);
            emit(&mut math_code, Opcode::Iadd);
            emit_i32(&mut math_code, 3);
            emit(&mut math_code, Opcode::Imul);
        }
        // Chain results together
        for _ in 0..3 {
            emit(&mut math_code, Opcode::Iadd);
        }
        emit(&mut math_code, Opcode::Return);

        let mut trivial_code = Vec::new();
        emit_i32(&mut trivial_code, 42);
        emit(&mut trivial_code, Opcode::Return);

        let module = make_module(vec![
            Function { name: "compute".to_string(), param_count: 0, local_count: 0, code: math_code },
            Function { name: "trivial".to_string(), param_count: 0, local_count: 0, code: trivial_code },
        ]);

        let pipeline = JitPipeline::new(StubBackend);
        // Straight-line arithmetic scores ~15.8: 12 ConstI32(1.2) + 4 Iadd(4.0) + 4 Imul(4.0) + 3 Iadd(3.0) + Return
        let mut config = PrewarmConfig::default();
        config.analyzer.min_score = 5.0;
        let result = prewarm_module(&module, &config, &pipeline);

        // Only the math function should be compiled
        assert_eq!(result.compiled.len(), 1);
        assert_eq!(result.compiled[0].0, 0); // func index 0 = "compute"
        assert!(!result.compiled[0].1.code.is_empty());
    }

    #[test]
    fn test_prewarm_empty_module() {
        let module = make_module(vec![]);
        let pipeline = JitPipeline::new(StubBackend);
        let config = PrewarmConfig::default();
        let result = prewarm_module(&module, &config, &pipeline);

        assert!(result.compiled.is_empty());
        assert!(result.skipped.is_empty());
    }

    #[test]
    fn test_prewarm_max_functions_limit() {
        // Create many math-heavy functions
        let mut functions = Vec::new();
        for i in 0..20 {
            let mut code = Vec::new();
            emit_i32(&mut code, 0);
            emit_local(&mut code, Opcode::StoreLocal, 0);
            let loop_start = code.len();
            emit_local(&mut code, Opcode::LoadLocal, 0);
            emit_i32(&mut code, 1);
            emit(&mut code, Opcode::Iadd);
            emit_local(&mut code, Opcode::StoreLocal, 0);
            emit_local(&mut code, Opcode::LoadLocal, 0);
            emit_i32(&mut code, 100);
            emit(&mut code, Opcode::Ilt);
            let back_offset = (loop_start as i32) - (code.len() as i32) - 5;
            emit_jmp(&mut code, Opcode::JmpIfTrue, back_offset);
            emit_local(&mut code, Opcode::LoadLocal, 0);
            emit(&mut code, Opcode::Return);

            functions.push(Function {
                name: format!("func_{}", i),
                param_count: 0,
                local_count: 1,
                code,
            });
        }

        let module = make_module(functions);
        let pipeline = JitPipeline::new(StubBackend);
        let mut config = PrewarmConfig::default();
        config.max_functions = 5;

        let result = prewarm_module(&module, &config, &pipeline);

        assert!(result.compiled.len() <= 5, "Should respect max_functions limit");
    }
}
