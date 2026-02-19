//! Static analysis heuristics for JIT candidate detection
//!
//! Analyzes bytecode to score functions for JIT compilation suitability.
//! CPU-intensive functions (math-heavy, string processing, no I/O) score high;
//! I/O-bound functions (spawn, await, mutex) score low.

use crate::compiler::bytecode::{Function, Module, Opcode};
use super::decoder::{decode_function, Operands};

/// Analysis result for a single function
#[derive(Debug, Clone)]
pub struct FunctionScore {
    /// Index in the module's function table
    pub func_index: usize,
    /// Function name
    pub name: String,
    /// Overall JIT suitability score (higher = more suitable)
    pub score: f64,
    /// Whether the function is CPU-bound (no I/O or concurrency ops)
    pub is_cpu_bound: bool,
    /// Whether the function contains loops (backward jumps)
    pub has_loops: bool,
    /// Ratio of arithmetic ops to total ops
    pub arithmetic_density: f64,
    /// Total decoded instruction count
    pub instruction_count: usize,
}

/// Configurable heuristics analyzer
pub struct HeuristicsAnalyzer {
    /// Minimum score to be considered a JIT candidate
    pub min_score: f64,
    /// Minimum instruction count (skip trivial functions)
    pub min_instruction_count: usize,
}

impl Default for HeuristicsAnalyzer {
    fn default() -> Self {
        HeuristicsAnalyzer {
            min_score: 10.0,
            min_instruction_count: 8,
        }
    }
}

// Scoring weights
const WEIGHT_LOOP: f64 = 5.0;
const WEIGHT_INT_ARITH: f64 = 1.0;
const WEIGHT_FLOAT_ARITH: f64 = 1.5;
const WEIGHT_BITWISE: f64 = 0.8;
const WEIGHT_COMPARISON: f64 = 0.5;
const WEIGHT_ARRAY_ACCESS: f64 = 0.3;
const WEIGHT_LOCAL: f64 = 0.2;
const WEIGHT_CONSTANT: f64 = 0.1;

// Penalty weights
const PENALTY_SPAWN: f64 = -100.0;
const PENALTY_AWAIT: f64 = -100.0;
const PENALTY_SLEEP: f64 = -100.0;
const PENALTY_MUTEX: f64 = -50.0;
const PENALTY_CHANNEL: f64 = -50.0;
const PENALTY_NATIVE_CALL: f64 = -5.0;
const PENALTY_CALL: f64 = -2.0;
const PENALTY_TRY: f64 = -10.0;
const PENALTY_JSON: f64 = -3.0;

impl HeuristicsAnalyzer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Analyze a single function and return its JIT suitability score
    pub fn analyze_function(&self, func: &Function, func_index: usize) -> FunctionScore {
        let instrs = match decode_function(&func.code) {
            Ok(instrs) => instrs,
            Err(_) => {
                return FunctionScore {
                    func_index,
                    name: func.name.clone(),
                    score: 0.0,
                    is_cpu_bound: false,
                    has_loops: false,
                    arithmetic_density: 0.0,
                    instruction_count: 0,
                };
            }
        };

        let instruction_count = instrs.len();
        let mut score = 0.0;
        let mut arithmetic_ops = 0usize;
        let mut has_loops = false;
        let mut has_io = false;

        for instr in &instrs {
            match instr.opcode {
                // === Loops (backward jumps) ===
                Opcode::Jmp | Opcode::JmpIfTrue | Opcode::JmpIfFalse | Opcode::JmpIfNull | Opcode::JmpIfNotNull => {
                    if let Operands::I32(offset) = instr.operands {
                        if offset < 0 {
                            has_loops = true;
                            score += WEIGHT_LOOP;
                        }
                    }
                }

                // === Integer arithmetic ===
                Opcode::Iadd | Opcode::Isub | Opcode::Imul | Opcode::Idiv | Opcode::Imod
                | Opcode::Ineg | Opcode::Ipow => {
                    score += WEIGHT_INT_ARITH;
                    arithmetic_ops += 1;
                }

                // === Float arithmetic ===
                Opcode::Fadd | Opcode::Fsub | Opcode::Fmul | Opcode::Fdiv
                | Opcode::Fneg | Opcode::Fpow | Opcode::Fmod => {
                    score += WEIGHT_FLOAT_ARITH;
                    arithmetic_ops += 1;
                }

                // === Bitwise ===
                Opcode::Ishl | Opcode::Ishr | Opcode::Iushr
                | Opcode::Iand | Opcode::Ior | Opcode::Ixor | Opcode::Inot => {
                    score += WEIGHT_BITWISE;
                    arithmetic_ops += 1;
                }

                // === Comparison ===
                Opcode::Ieq | Opcode::Ine | Opcode::Ilt | Opcode::Ile | Opcode::Igt | Opcode::Ige
                | Opcode::Feq | Opcode::Fne | Opcode::Flt | Opcode::Fle | Opcode::Fgt | Opcode::Fge
                | Opcode::Seq | Opcode::Sne | Opcode::Slt | Opcode::Sle | Opcode::Sgt | Opcode::Sge
                | Opcode::Eq | Opcode::Ne => {
                    score += WEIGHT_COMPARISON;
                }

                // === Array access ===
                Opcode::LoadElem | Opcode::StoreElem | Opcode::ArrayLen
                | Opcode::ArrayPush | Opcode::ArrayPop => {
                    score += WEIGHT_ARRAY_ACCESS;
                }

                // === Local load/store ===
                Opcode::LoadLocal | Opcode::StoreLocal
                | Opcode::LoadLocal0 | Opcode::LoadLocal1 => {
                    score += WEIGHT_LOCAL;
                }

                // === Constants ===
                Opcode::ConstI32 | Opcode::ConstF64 | Opcode::ConstTrue | Opcode::ConstFalse
                | Opcode::ConstNull | Opcode::ConstStr | Opcode::LoadConst => {
                    score += WEIGHT_CONSTANT;
                }

                // === Penalties: I/O and concurrency ===
                Opcode::Spawn | Opcode::SpawnClosure => {
                    score += PENALTY_SPAWN;
                    has_io = true;
                }
                Opcode::Await => {
                    score += PENALTY_AWAIT;
                    has_io = true;
                }
                Opcode::Sleep => {
                    score += PENALTY_SLEEP;
                    has_io = true;
                }
                Opcode::MutexLock | Opcode::MutexUnlock => {
                    score += PENALTY_MUTEX;
                    has_io = true;
                }
                Opcode::NewChannel => {
                    score += PENALTY_CHANNEL;
                    has_io = true;
                }

                // === Penalties: calls ===
                Opcode::NativeCall | Opcode::ModuleNativeCall => {
                    score += PENALTY_NATIVE_CALL;
                }
                Opcode::Call | Opcode::CallMethod | Opcode::CallConstructor
                | Opcode::CallSuper | Opcode::CallStatic => {
                    score += PENALTY_CALL;
                }

                // === Penalties: exception handling ===
                Opcode::Try => {
                    score += PENALTY_TRY;
                }

                // === Penalties: JSON ops ===
                Opcode::JsonGet | Opcode::JsonSet | Opcode::JsonDelete
                | Opcode::JsonIndex | Opcode::JsonIndexSet | Opcode::JsonPush
                | Opcode::JsonPop | Opcode::JsonNewObject | Opcode::JsonNewArray
                | Opcode::JsonKeys | Opcode::JsonLength => {
                    score += PENALTY_JSON;
                }

                // Everything else: neutral
                _ => {}
            }
        }

        let arithmetic_density = if instruction_count > 0 {
            arithmetic_ops as f64 / instruction_count as f64
        } else {
            0.0
        };

        FunctionScore {
            func_index,
            name: func.name.clone(),
            score,
            is_cpu_bound: !has_io,
            has_loops,
            arithmetic_density,
            instruction_count,
        }
    }

    /// Analyze all functions in a module
    pub fn analyze_module(&self, module: &Module) -> Vec<FunctionScore> {
        module.functions.iter().enumerate()
            .map(|(idx, func)| self.analyze_function(func, idx))
            .collect()
    }

    /// Select JIT compilation candidates from a module, sorted by score descending
    pub fn select_candidates(&self, module: &Module) -> Vec<FunctionScore> {
        let mut scores = self.analyze_module(module);

        // Filter: must meet minimum thresholds and be CPU-bound
        scores.retain(|s| {
            s.score >= self.min_score
                && s.is_cpu_bound
                && s.instruction_count >= self.min_instruction_count
        });

        // Sort by score descending (best candidates first)
        scores.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        scores
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::bytecode::{ConstantPool, Metadata};

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
    fn test_trivial_function_low_score() {
        // ConstI32 42, Return — too simple
        let mut code = Vec::new();
        emit_i32(&mut code, 42);
        emit(&mut code, Opcode::Return);

        let func = Function {
            name: "trivial".to_string(),
            param_count: 0,
            local_count: 0,
            code,
        };

        let analyzer = HeuristicsAnalyzer::new();
        let score = analyzer.analyze_function(&func, 0);

        assert!(score.score < 10.0, "Trivial function should score low: {}", score.score);
        assert!(!score.has_loops);
        assert!(score.is_cpu_bound);
    }

    #[test]
    fn test_math_loop_high_score() {
        // Simulate: for (i = 0; i < n; i++) { sum += arr[i] * 2 }
        let mut code = Vec::new();
        emit_i32(&mut code, 0);                              // const 0
        emit_local(&mut code, Opcode::StoreLocal, 0);        // i = 0
        emit_i32(&mut code, 100);                            // const 100
        emit_local(&mut code, Opcode::StoreLocal, 1);        // n = 100
        // loop start
        let loop_start = code.len();
        emit_local(&mut code, Opcode::LoadLocal, 0);         // load i
        emit_local(&mut code, Opcode::LoadLocal, 1);         // load n
        emit(&mut code, Opcode::Ilt);                         // i < n
        let jmp_offset_pos = code.len();
        emit_jmp(&mut code, Opcode::JmpIfFalse, 0);          // placeholder
        // loop body
        emit_local(&mut code, Opcode::LoadLocal, 0);         // load i
        emit_i32(&mut code, 2);                              // const 2
        emit(&mut code, Opcode::Imul);                        // i * 2
        emit_local(&mut code, Opcode::LoadLocal, 2);         // load sum
        emit(&mut code, Opcode::Iadd);                        // sum + (i * 2)
        emit_local(&mut code, Opcode::StoreLocal, 2);        // sum = ...
        // increment i
        emit_local(&mut code, Opcode::LoadLocal, 0);         // load i
        emit_i32(&mut code, 1);                              // const 1
        emit(&mut code, Opcode::Iadd);                        // i + 1
        emit_local(&mut code, Opcode::StoreLocal, 0);        // i = ...
        // backward jump
        let back_offset = (loop_start as i32) - (code.len() as i32) - 5; // -5 for Jmp + i32
        emit_jmp(&mut code, Opcode::Jmp, back_offset);
        // fix forward jump
        let end_pos = code.len();
        let fwd_offset = (end_pos as i32) - (jmp_offset_pos as i32) - 5;
        code[jmp_offset_pos + 1..jmp_offset_pos + 5].copy_from_slice(&fwd_offset.to_le_bytes());
        // return
        emit_local(&mut code, Opcode::LoadLocal, 2);
        emit(&mut code, Opcode::Return);

        let func = Function {
            name: "math_loop".to_string(),
            param_count: 0,
            local_count: 3,
            code,
        };

        let analyzer = HeuristicsAnalyzer::new();
        let score = analyzer.analyze_function(&func, 0);

        assert!(score.score >= 10.0, "Math loop should score high: {}", score.score);
        assert!(score.has_loops);
        assert!(score.is_cpu_bound);
        assert!(score.arithmetic_density > 0.0);
    }

    #[test]
    fn test_io_function_not_cpu_bound() {
        // Function with Spawn → I/O bound
        let mut code = Vec::new();
        emit_i32(&mut code, 0);
        // Spawn: func_index (u32) + arg_count (u16) — matches decoder
        code.push(Opcode::Spawn as u8);
        code.extend_from_slice(&1u32.to_le_bytes());
        code.extend_from_slice(&0u16.to_le_bytes());
        emit(&mut code, Opcode::Return);

        let func = Function {
            name: "spawner".to_string(),
            param_count: 0,
            local_count: 0,
            code,
        };

        let analyzer = HeuristicsAnalyzer::new();
        let score = analyzer.analyze_function(&func, 0);

        assert!(!score.is_cpu_bound, "Spawning function should not be CPU-bound");
        assert!(score.score < 0.0, "Should have negative score due to spawn penalty: {}", score.score);
    }

    #[test]
    fn test_select_candidates_filters_correctly() {
        // Create a module with 3 functions: math-heavy, trivial, I/O-bound
        let mut math_code = Vec::new();
        // Simple loop with arithmetic
        emit_i32(&mut math_code, 0);
        emit_local(&mut math_code, Opcode::StoreLocal, 0);
        let loop_start = math_code.len();
        emit_local(&mut math_code, Opcode::LoadLocal, 0);
        emit_i32(&mut math_code, 1);
        emit(&mut math_code, Opcode::Iadd);
        emit_local(&mut math_code, Opcode::StoreLocal, 0);
        emit_local(&mut math_code, Opcode::LoadLocal, 0);
        emit_i32(&mut math_code, 2);
        emit(&mut math_code, Opcode::Imul);
        emit_local(&mut math_code, Opcode::StoreLocal, 1);
        // Additional arithmetic to push score above min_score (10.0)
        emit_local(&mut math_code, Opcode::LoadLocal, 1);
        emit_i32(&mut math_code, 3);
        emit(&mut math_code, Opcode::Imod);
        emit(&mut math_code, Opcode::Pop);
        emit_local(&mut math_code, Opcode::LoadLocal, 0);
        emit_i32(&mut math_code, 100);
        emit(&mut math_code, Opcode::Ilt);
        let back_offset = (loop_start as i32) - (math_code.len() as i32) - 5;
        emit_jmp(&mut math_code, Opcode::JmpIfTrue, back_offset);
        emit_local(&mut math_code, Opcode::LoadLocal, 1);
        emit(&mut math_code, Opcode::Return);

        let mut trivial_code = Vec::new();
        emit_i32(&mut trivial_code, 42);
        emit(&mut trivial_code, Opcode::Return);

        let mut io_code = Vec::new();
        emit_i32(&mut io_code, 0);
        code_push_spawn(&mut io_code);
        emit(&mut io_code, Opcode::Return);

        let module = make_module(vec![
            Function { name: "compute".to_string(), param_count: 0, local_count: 3, code: math_code },
            Function { name: "trivial".to_string(), param_count: 0, local_count: 0, code: trivial_code },
            Function { name: "io_bound".to_string(), param_count: 0, local_count: 0, code: io_code },
        ]);

        let analyzer = HeuristicsAnalyzer::new();
        let candidates = analyzer.select_candidates(&module);

        // Only the math-heavy function should be selected
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].name, "compute");
        assert!(candidates[0].has_loops);
        assert!(candidates[0].is_cpu_bound);
    }

    fn code_push_spawn(code: &mut Vec<u8>) {
        code.push(Opcode::Spawn as u8);
        code.extend_from_slice(&1u32.to_le_bytes()); // func_index: u32
        code.extend_from_slice(&0u16.to_le_bytes()); // arg_count: u16
    }

    #[test]
    fn test_float_heavy_function() {
        // Float-heavy computation: a * b + c * d
        let mut code = Vec::new();
        code.push(Opcode::ConstF64 as u8);
        code.extend_from_slice(&1.5f64.to_le_bytes());
        code.push(Opcode::ConstF64 as u8);
        code.extend_from_slice(&2.5f64.to_le_bytes());
        emit(&mut code, Opcode::Fmul);
        code.push(Opcode::ConstF64 as u8);
        code.extend_from_slice(&3.0f64.to_le_bytes());
        code.push(Opcode::ConstF64 as u8);
        code.extend_from_slice(&4.0f64.to_le_bytes());
        emit(&mut code, Opcode::Fmul);
        emit(&mut code, Opcode::Fadd);
        emit(&mut code, Opcode::Return);

        let func = Function {
            name: "float_compute".to_string(),
            param_count: 0,
            local_count: 0,
            code,
        };

        let analyzer = HeuristicsAnalyzer::new();
        let score = analyzer.analyze_function(&func, 0);

        // 4 ConstF64 (0.4) + 2 Fmul (3.0) + 1 Fadd (1.5) = 4.9
        assert!(score.score > 4.0, "Float-heavy should score decently: {}", score.score);
        assert!(score.is_cpu_bound);
        assert!(score.arithmetic_density > 0.3);
    }

    #[test]
    fn test_empty_function() {
        let func = Function {
            name: "empty".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![],
        };

        let analyzer = HeuristicsAnalyzer::new();
        let score = analyzer.analyze_function(&func, 0);

        assert_eq!(score.score, 0.0);
        assert_eq!(score.instruction_count, 0);
    }

    #[test]
    fn test_candidates_sorted_by_score() {
        let mut fast_code = Vec::new();
        // Two loops of arithmetic = higher score
        for _ in 0..5 {
            emit_i32(&mut fast_code, 1);
            emit_i32(&mut fast_code, 2);
            emit(&mut fast_code, Opcode::Iadd);
            emit_i32(&mut fast_code, 3);
            emit(&mut fast_code, Opcode::Imul);
        }
        let back_offset = -(fast_code.len() as i32) - 5;
        emit_jmp(&mut fast_code, Opcode::Jmp, back_offset);

        let mut slow_code = Vec::new();
        // One loop, less arithmetic (but enough to pass min_score)
        for _ in 0..5 {
            emit_i32(&mut slow_code, 1);
            emit_i32(&mut slow_code, 2);
            emit(&mut slow_code, Opcode::Iadd);
        }
        let back_offset2 = -(slow_code.len() as i32) - 5;
        emit_jmp(&mut slow_code, Opcode::Jmp, back_offset2);

        let module = make_module(vec![
            Function { name: "slower".to_string(), param_count: 0, local_count: 0, code: slow_code },
            Function { name: "faster".to_string(), param_count: 0, local_count: 0, code: fast_code },
        ]);

        let analyzer = HeuristicsAnalyzer::new();
        let candidates = analyzer.select_candidates(&module);

        assert!(candidates.len() >= 2);
        assert!(candidates[0].score >= candidates[1].score, "Should be sorted by score descending");
    }
}
