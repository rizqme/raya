//! Cranelift code generation backend
//!
//! Implements `CodegenBackend` using Cranelift to produce real native code
//! from JIT IR. Supports x86_64 and AArch64 targets.

pub mod abi;
pub mod lowering;

use std::sync::Arc;
use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::{ir, Context};
use cranelift_frontend::FunctionBuilderContext;
use target_lexicon::Architecture;

use crate::jit::backend::traits::*;
use crate::jit::ir::instr::JitFunction;
use self::lowering::{LoweringContext, jit_entry_signature};

/// Cranelift-based code generation backend
pub struct CraneliftBackend {
    /// The target ISA (instruction set architecture)
    isa: Arc<dyn TargetIsa>,
}

impl CraneliftBackend {
    /// Create a backend targeting the host machine
    pub fn host() -> Result<Self, CodegenError> {
        let mut flag_builder = settings::builder();
        flag_builder.set("opt_level", "speed").map_err(|e|
            CodegenError::BackendError(format!("Failed to set opt_level: {}", e))
        )?;
        // Enable position-independent code for safety
        flag_builder.set("is_pic", "true").map_err(|e|
            CodegenError::BackendError(format!("Failed to set is_pic: {}", e))
        )?;

        let flags = settings::Flags::new(flag_builder);

        let isa = cranelift_native::builder()
            .map_err(|e| CodegenError::BackendError(format!("Failed to create native ISA builder: {}", e)))?
            .finish(flags)
            .map_err(|e| CodegenError::BackendError(format!("Failed to finish ISA: {}", e)))?;

        Ok(CraneliftBackend { isa })
    }

    /// Create a backend with a specific ISA
    pub fn with_isa(isa: Arc<dyn TargetIsa>) -> Self {
        CraneliftBackend { isa }
    }
}

impl CodegenBackend for CraneliftBackend {
    fn name(&self) -> &str {
        "cranelift"
    }

    fn compile_function(
        &self,
        func: &JitFunction,
        _ctx: &ModuleContext,
    ) -> Result<CompiledCode, CodegenError> {
        let mut codegen_ctx = Context::new();
        let mut func_builder_ctx = FunctionBuilderContext::new();

        // Set up the function signature (JitEntryFn ABI)
        let call_conv = self.isa.default_call_conv();
        codegen_ctx.func.signature = jit_entry_signature(call_conv);
        codegen_ctx.func.name = ir::UserFuncName::user(0, func.func_index);

        // Build Cranelift IR from JIT IR
        {
            let builder = cranelift_frontend::FunctionBuilder::new(
                &mut codegen_ctx.func,
                &mut func_builder_ctx,
            );

            // lower() takes ownership of builder (finalize() consumes it)
            LoweringContext::lower(func, builder).map_err(|e| {
                CodegenError::BackendError(format!("Lowering failed: {}", e))
            })?;
        }

        // Compile to machine code
        let mut ctrl_plane = ControlPlane::default();
        let code = codegen_ctx
            .compile(&*self.isa, &mut ctrl_plane)
            .map_err(|e| {
                CodegenError::BackendError(format!("Cranelift compilation failed: {:?}", e))
            })?;

        let code_bytes = code.code_buffer().to_vec();

        Ok(CompiledCode {
            code: code_bytes,
            entry_offset: 0,
            stack_maps: vec![],
            deopt_info: vec![],
            relocations: vec![],
        })
    }

    fn finalize(
        &self,
        _code: &mut CompiledCode,
        _resolver: &dyn SymbolResolver,
    ) -> Result<ExecutableCode, CodegenError> {
        // Full finalization with executable memory mapping deferred to JitEngine
        // which uses cranelift_jit::JITModule for proper memory management
        Err(CodegenError::BackendError(
            "Use JitEngine for executable code; CraneliftBackend::finalize not yet implemented".to_string()
        ))
    }

    fn target_info(&self) -> TargetInfo {
        let arch = match self.isa.triple().architecture {
            Architecture::X86_64 => TargetArch::X86_64,
            Architecture::Aarch64(_) => TargetArch::AArch64,
            _ => TargetArch::X86_64, // fallback
        };
        TargetInfo {
            arch,
            pointer_size: self.isa.pointer_bytes() as usize,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::ir::instr::{JitBlock, JitBlockId, JitInstr, JitTerminator, Reg};
    use crate::jit::ir::types::JitType;
    use crate::jit::pipeline::JitPipeline;
    use crate::compiler::bytecode::{ConstantPool, Function, Metadata, Module, Opcode};

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
            jit_hints: vec![],
        }
    }

    fn emit_i32(code: &mut Vec<u8>, val: i32) {
        code.push(Opcode::ConstI32 as u8);
        code.extend_from_slice(&val.to_le_bytes());
    }
    fn emit(code: &mut Vec<u8>, op: Opcode) { code.push(op as u8); }

    #[test]
    fn test_cranelift_backend_creation() {
        let backend = CraneliftBackend::host().unwrap();
        assert_eq!(backend.name(), "cranelift");
        let info = backend.target_info();
        assert_eq!(info.pointer_size, 8);
    }

    #[test]
    fn test_cranelift_compile_simple_return() {
        // Build JIT IR directly: ConstI32(42), Return
        let mut func = JitFunction::new(0, "test".to_string(), 0, 0);
        let entry = func.add_block();

        let r0 = func.alloc_reg(JitType::I32);
        func.block_mut(entry).instrs.push(JitInstr::ConstI32 { dest: r0, value: 42 });
        func.block_mut(entry).terminator = JitTerminator::Return(Some(r0));

        let backend = CraneliftBackend::host().unwrap();
        let module = make_module_with_func(vec![], 0, 0);
        let ctx = ModuleContext { module: &module, func_index: 0 };

        let compiled = backend.compile_function(&func, &ctx).unwrap();
        assert!(!compiled.code.is_empty());
        assert_eq!(compiled.entry_offset, 0);
    }

    #[test]
    fn test_cranelift_compile_arithmetic() {
        // Build JIT IR: r0 = 3, r1 = 5, r2 = r0 + r1, Return r2
        let mut func = JitFunction::new(0, "arith".to_string(), 0, 0);
        let entry = func.add_block();

        let r0 = func.alloc_reg(JitType::I32);
        let r1 = func.alloc_reg(JitType::I32);
        let r2 = func.alloc_reg(JitType::I32);

        func.block_mut(entry).instrs.push(JitInstr::ConstI32 { dest: r0, value: 3 });
        func.block_mut(entry).instrs.push(JitInstr::ConstI32 { dest: r1, value: 5 });
        func.block_mut(entry).instrs.push(JitInstr::IAdd { dest: r2, left: r0, right: r1 });
        func.block_mut(entry).terminator = JitTerminator::Return(Some(r2));

        let backend = CraneliftBackend::host().unwrap();
        let module = make_module_with_func(vec![], 0, 0);
        let ctx = ModuleContext { module: &module, func_index: 0 };

        let compiled = backend.compile_function(&func, &ctx).unwrap();
        assert!(!compiled.code.is_empty());
        // Should produce real native code (more than 1 byte unlike the stub)
        assert!(compiled.code.len() > 4);
    }

    #[test]
    fn test_cranelift_compile_float_arithmetic() {
        // r0 = 1.5, r1 = 2.5, r2 = r0 + r1, Return r2
        let mut func = JitFunction::new(0, "float_arith".to_string(), 0, 0);
        let entry = func.add_block();

        let r0 = func.alloc_reg(JitType::F64);
        let r1 = func.alloc_reg(JitType::F64);
        let r2 = func.alloc_reg(JitType::F64);

        func.block_mut(entry).instrs.push(JitInstr::ConstF64 { dest: r0, value: 1.5 });
        func.block_mut(entry).instrs.push(JitInstr::ConstF64 { dest: r1, value: 2.5 });
        func.block_mut(entry).instrs.push(JitInstr::FAdd { dest: r2, left: r0, right: r1 });
        func.block_mut(entry).terminator = JitTerminator::Return(Some(r2));

        let backend = CraneliftBackend::host().unwrap();
        let module = make_module_with_func(vec![], 0, 0);
        let ctx = ModuleContext { module: &module, func_index: 0 };

        let compiled = backend.compile_function(&func, &ctx).unwrap();
        assert!(!compiled.code.is_empty());
    }

    #[test]
    fn test_cranelift_compile_branch() {
        // entry: r0 = true, branch(r0, then, else)
        // then: r1 = 1, return r1
        // else: r2 = 2, return r2
        let mut func = JitFunction::new(0, "branch".to_string(), 0, 0);
        let entry = func.add_block();
        let then_block = func.add_block();
        let else_block = func.add_block();

        let r0 = func.alloc_reg(JitType::Bool);
        let r1 = func.alloc_reg(JitType::I32);
        let r2 = func.alloc_reg(JitType::I32);

        func.block_mut(entry).instrs.push(JitInstr::ConstBool { dest: r0, value: true });
        func.block_mut(entry).terminator = JitTerminator::Branch {
            cond: r0,
            then_block,
            else_block,
        };

        func.block_mut(then_block).instrs.push(JitInstr::ConstI32 { dest: r1, value: 1 });
        func.block_mut(then_block).terminator = JitTerminator::Return(Some(r1));

        func.block_mut(else_block).instrs.push(JitInstr::ConstI32 { dest: r2, value: 2 });
        func.block_mut(else_block).terminator = JitTerminator::Return(Some(r2));

        let backend = CraneliftBackend::host().unwrap();
        let module = make_module_with_func(vec![], 0, 0);
        let ctx = ModuleContext { module: &module, func_index: 0 };

        let compiled = backend.compile_function(&func, &ctx).unwrap();
        assert!(!compiled.code.is_empty());
    }

    #[test]
    fn test_cranelift_compile_boxing() {
        // r0 = const_i32(42), r1 = box_i32(r0), return r1
        let mut func = JitFunction::new(0, "boxing".to_string(), 0, 0);
        let entry = func.add_block();

        let r0 = func.alloc_reg(JitType::I32);
        let r1 = func.alloc_reg(JitType::Value);

        func.block_mut(entry).instrs.push(JitInstr::ConstI32 { dest: r0, value: 42 });
        func.block_mut(entry).instrs.push(JitInstr::BoxI32 { dest: r1, src: r0 });
        func.block_mut(entry).terminator = JitTerminator::Return(Some(r1));

        let backend = CraneliftBackend::host().unwrap();
        let module = make_module_with_func(vec![], 0, 0);
        let ctx = ModuleContext { module: &module, func_index: 0 };

        let compiled = backend.compile_function(&func, &ctx).unwrap();
        assert!(!compiled.code.is_empty());
    }

    #[test]
    fn test_cranelift_pipeline_integration() {
        // Test full pipeline: bytecode → decode → CFG → lift → optimize → Cranelift compile
        let mut code = Vec::new();
        emit_i32(&mut code, 42);
        emit(&mut code, Opcode::Return);

        let module = make_module_with_func(code, 0, 0);
        let backend = CraneliftBackend::host().unwrap();
        let pipeline = JitPipeline::new(backend);

        let (jit_func, compiled) = pipeline.compile_function(
            &module.functions[0], &module, 0
        ).unwrap();

        assert_eq!(jit_func.name, "test_func");
        assert!(!compiled.code.is_empty());
        // Cranelift should produce more than the stub's 1-byte trap
        assert!(compiled.code.len() > 1);
    }

    #[test]
    fn test_cranelift_pipeline_arithmetic() {
        // ConstI32 3, ConstI32 5, Iadd, Return
        let mut code = Vec::new();
        emit_i32(&mut code, 3);
        emit_i32(&mut code, 5);
        emit(&mut code, Opcode::Iadd);
        emit(&mut code, Opcode::Return);

        let module = make_module_with_func(code, 0, 0);
        let backend = CraneliftBackend::host().unwrap();
        let pipeline = JitPipeline::new(backend);

        let (_jit_func, compiled) = pipeline.compile_function(
            &module.functions[0], &module, 0
        ).unwrap();

        assert!(!compiled.code.is_empty());
    }

    #[test]
    fn test_cranelift_pipeline_float() {
        // ConstF64 1.5, ConstF64 2.5, Fadd, Return
        let mut code = Vec::new();
        code.push(Opcode::ConstF64 as u8);
        code.extend_from_slice(&1.5f64.to_le_bytes());
        code.push(Opcode::ConstF64 as u8);
        code.extend_from_slice(&2.5f64.to_le_bytes());
        emit(&mut code, Opcode::Fadd);
        emit(&mut code, Opcode::Return);

        let module = make_module_with_func(code, 0, 0);
        let backend = CraneliftBackend::host().unwrap();
        let pipeline = JitPipeline::new(backend);

        let (_jit_func, compiled) = pipeline.compile_function(
            &module.functions[0], &module, 0
        ).unwrap();

        assert!(!compiled.code.is_empty());
    }

    #[test]
    fn test_cranelift_null_return() {
        // Return void → returns null
        let mut func = JitFunction::new(0, "void_ret".to_string(), 0, 0);
        let entry = func.add_block();
        func.block_mut(entry).terminator = JitTerminator::Return(None);

        let backend = CraneliftBackend::host().unwrap();
        let module = make_module_with_func(vec![], 0, 0);
        let ctx = ModuleContext { module: &module, func_index: 0 };

        let compiled = backend.compile_function(&func, &ctx).unwrap();
        assert!(!compiled.code.is_empty());
    }
}
