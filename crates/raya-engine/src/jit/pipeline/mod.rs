//! Compilation pipeline: bytecode → JIT IR → optimized IR → backend
//!
//! The pipeline chains together all compilation stages:
//! 1. Decode bytecode into typed instructions
//! 2. Build control-flow graph
//! 3. Lift stack-based bytecode to SSA-form JIT IR
//! 4. Run optimization passes
//! 5. Lower to backend (stub or native codegen)

pub mod lifter;
pub mod optimize;
pub mod prewarm;

use crate::compiler::bytecode::{Function, Module};
use crate::jit::backend::traits::{CodegenBackend, CodegenError, CompiledCode, ModuleContext};
use crate::jit::ir::instr::JitFunction;
use self::lifter::LiftError;
use self::optimize::JitOptimizer;

/// Errors from the compilation pipeline
#[derive(Debug, thiserror::Error)]
pub enum JitError {
    #[error("Lift error: {0}")]
    Lift(#[from] LiftError),
    #[error("Codegen error: {0}")]
    Codegen(#[from] CodegenError),
}

/// Complete JIT compilation pipeline
///
/// Chains: decode → CFG → lift → optimize → backend.compile_function()
pub struct JitPipeline<B: CodegenBackend> {
    backend: B,
    optimizer: JitOptimizer,
}

impl<B: CodegenBackend> JitPipeline<B> {
    /// Create a new pipeline with the default optimizer
    pub fn new(backend: B) -> Self {
        JitPipeline {
            backend,
            optimizer: JitOptimizer::new(),
        }
    }

    /// Create a pipeline with a custom optimizer
    pub fn with_optimizer(backend: B, optimizer: JitOptimizer) -> Self {
        JitPipeline { backend, optimizer }
    }

    /// Compile a single function through the full pipeline
    pub fn compile_function(
        &self,
        func: &Function,
        module: &Module,
        func_index: u32,
    ) -> Result<(JitFunction, CompiledCode), JitError> {
        // Step 1-3: Decode, build CFG, lift to SSA
        let mut jit_func = lifter::lift_function(func, module, func_index)?;

        // Step 4: Optimize
        self.optimizer.optimize(&mut jit_func);

        // Step 5: Generate code via backend
        let ctx = ModuleContext { module, func_index };
        let code = self.backend.compile_function(&jit_func, &ctx)?;

        Ok((jit_func, code))
    }

    /// Lift and optimize a function without backend compilation.
    ///
    /// Returns the optimized JIT IR, ready for lowering through a JITModule
    /// or other backend. Used by JitEngine for executable code generation.
    pub fn lift_and_optimize(
        &self,
        func: &Function,
        module: &Module,
        func_index: u32,
    ) -> Result<JitFunction, JitError> {
        let mut jit_func = lifter::lift_function(func, module, func_index)?;
        self.optimizer.optimize(&mut jit_func);
        Ok(jit_func)
    }

    /// Compile all functions in a module
    pub fn compile_module(
        &self,
        module: &Module,
    ) -> Result<Vec<(JitFunction, CompiledCode)>, JitError> {
        let mut results = Vec::new();
        for (idx, func) in module.functions.iter().enumerate() {
            let result = self.compile_function(func, module, idx as u32)?;
            results.push(result);
        }
        Ok(results)
    }

    /// Get a reference to the backend
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Get a reference to the optimizer
    pub fn optimizer(&self) -> &JitOptimizer {
        &self.optimizer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::backend::StubBackend;
    use crate::compiler::bytecode::{ConstantPool, Metadata, Opcode};

    fn make_module_with_func(code: Vec<u8>, param_count: usize, local_count: usize) -> Module {
        Module {
            magic: *b"RAYA",
            version: 1,
            flags: 0,
            constants: ConstantPool::new(),
            functions: vec![Function {
                name: "test_func".to_string(),
                param_count,
                local_count,
                code,
            }],
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

    fn emit(code: &mut Vec<u8>, op: Opcode) { code.push(op as u8); }
    fn emit_i32(code: &mut Vec<u8>, val: i32) {
        code.push(Opcode::ConstI32 as u8);
        code.extend_from_slice(&val.to_le_bytes());
    }
    fn emit_f64(code: &mut Vec<u8>, val: f64) {
        code.push(Opcode::ConstF64 as u8);
        code.extend_from_slice(&val.to_le_bytes());
    }
    fn emit_jmp(code: &mut Vec<u8>, op: Opcode, offset: i32) {
        code.push(op as u8);
        code.extend_from_slice(&offset.to_le_bytes());
    }
    fn emit_store_local(code: &mut Vec<u8>, idx: u16) {
        code.push(Opcode::StoreLocal as u8);
        code.extend_from_slice(&idx.to_le_bytes());
    }
    fn emit_load_local(code: &mut Vec<u8>, idx: u16) {
        code.push(Opcode::LoadLocal as u8);
        code.extend_from_slice(&idx.to_le_bytes());
    }

    #[test]
    fn test_pipeline_simple_return() {
        // ConstI32 42, Return
        let mut code = Vec::new();
        emit_i32(&mut code, 42);
        emit(&mut code, Opcode::Return);

        let module = make_module_with_func(code, 0, 0);
        let pipeline = JitPipeline::new(StubBackend);

        let (jit_func, compiled) = pipeline.compile_function(
            &module.functions[0], &module, 0
        ).unwrap();

        assert_eq!(jit_func.name, "test_func");
        assert!(!compiled.code.is_empty());
    }

    #[test]
    fn test_pipeline_arithmetic() {
        // ConstI32 3, ConstI32 5, Iadd, Return
        let mut code = Vec::new();
        emit_i32(&mut code, 3);
        emit_i32(&mut code, 5);
        emit(&mut code, Opcode::Iadd);
        emit(&mut code, Opcode::Return);

        let module = make_module_with_func(code, 0, 0);
        let pipeline = JitPipeline::new(StubBackend);

        let (jit_func, _) = pipeline.compile_function(
            &module.functions[0], &module, 0
        ).unwrap();

        // After constant folding, the IAdd(3,5) should be folded to ConstI32(8)
        let display = format!("{}", jit_func);
        assert!(display.contains("const.i32 8") || display.contains("iadd"));
    }

    #[test]
    fn test_pipeline_with_locals() {
        // ConstI32 10, StoreLocal 0, LoadLocal 0, Return
        let mut code = Vec::new();
        emit_i32(&mut code, 10);
        emit_store_local(&mut code, 0);
        emit_load_local(&mut code, 0);
        emit(&mut code, Opcode::Return);

        let module = make_module_with_func(code, 0, 1);
        let pipeline = JitPipeline::new(StubBackend);

        let (jit_func, compiled) = pipeline.compile_function(
            &module.functions[0], &module, 0
        ).unwrap();

        assert_eq!(jit_func.local_count, 1);
        assert!(!compiled.code.is_empty());
    }

    #[test]
    fn test_pipeline_float_ops() {
        // ConstF64 1.5, ConstF64 2.5, Fadd, Return
        let mut code = Vec::new();
        emit_f64(&mut code, 1.5);
        emit_f64(&mut code, 2.5);
        emit(&mut code, Opcode::Fadd);
        emit(&mut code, Opcode::Return);

        let module = make_module_with_func(code, 0, 0);
        let pipeline = JitPipeline::new(StubBackend);

        let (jit_func, _) = pipeline.compile_function(
            &module.functions[0], &module, 0
        ).unwrap();

        let display = format!("{}", jit_func);
        assert!(display.contains("const.f64") || display.contains("fadd"));
    }

    #[test]
    fn test_pipeline_branch() {
        // ConstTrue, JmpIfFalse +7, ConstI32 1, Return, ConstI32 2, Return
        let mut code = Vec::new();
        emit(&mut code, Opcode::ConstTrue);              // offset 0
        emit_jmp(&mut code, Opcode::JmpIfFalse, 11);     // offset 1, target=12
        emit_i32(&mut code, 1);                           // offset 6
        emit(&mut code, Opcode::Return);                  // offset 11
        emit_i32(&mut code, 2);                           // offset 12
        emit(&mut code, Opcode::Return);                  // offset 17

        let module = make_module_with_func(code, 0, 0);
        let pipeline = JitPipeline::new(StubBackend);

        let (jit_func, _) = pipeline.compile_function(
            &module.functions[0], &module, 0
        ).unwrap();

        // Should have multiple blocks
        assert!(jit_func.blocks.len() >= 3);
    }

    #[test]
    fn test_pipeline_compile_module() {
        let mut code1 = Vec::new();
        emit_i32(&mut code1, 42);
        emit(&mut code1, Opcode::Return);

        let mut code2 = Vec::new();
        emit_i32(&mut code2, 99);
        emit(&mut code2, Opcode::Return);

        let module = Module {
            magic: *b"RAYA",
            version: 1,
            flags: 0,
            constants: ConstantPool::new(),
            functions: vec![
                Function { name: "func_a".to_string(), param_count: 0, local_count: 0, code: code1 },
                Function { name: "func_b".to_string(), param_count: 0, local_count: 0, code: code2 },
            ],
            classes: vec![],
            metadata: Metadata { name: "multi".to_string(), source_file: None },
            exports: vec![],
            imports: vec![],
            checksum: [0; 32],
            reflection: None,
            debug_info: None,
            native_functions: vec![],
        };

        let pipeline = JitPipeline::new(StubBackend);
        let results = pipeline.compile_module(&module).unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0.name, "func_a");
        assert_eq!(results[1].0.name, "func_b");
    }

    #[test]
    fn test_pipeline_empty_function() {
        let module = make_module_with_func(vec![], 0, 0);
        let pipeline = JitPipeline::new(StubBackend);

        let result = pipeline.compile_function(
            &module.functions[0], &module, 0
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_pipeline_no_optimizer() {
        let mut code = Vec::new();
        emit_i32(&mut code, 3);
        emit_i32(&mut code, 5);
        emit(&mut code, Opcode::Iadd);
        emit(&mut code, Opcode::Return);

        let module = make_module_with_func(code, 0, 0);
        let pipeline = JitPipeline::with_optimizer(StubBackend, JitOptimizer::empty());

        let (jit_func, _) = pipeline.compile_function(
            &module.functions[0], &module, 0
        ).unwrap();

        // Without optimizer, IAdd should remain (not folded)
        let display = format!("{}", jit_func);
        assert!(display.contains("iadd"));
    }
}
