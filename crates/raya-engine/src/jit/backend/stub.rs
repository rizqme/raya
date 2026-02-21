//! Stub backend for testing the pipeline without real codegen
//!
//! Produces placeholder machine code (INT3 / BRK) to verify
//! the pipeline runs end-to-end without panics.

use super::traits::*;
use crate::jit::ir::instr::JitFunction;

/// A stub backend that produces placeholder code (INT3 on x86, BRK on ARM)
pub struct StubBackend;

impl CodegenBackend for StubBackend {
    fn name(&self) -> &str {
        "stub"
    }

    fn compile_function(
        &self,
        _func: &JitFunction,
        _ctx: &ModuleContext<'_>,
    ) -> Result<CompiledCode, CodegenError> {
        // Emit a single-byte trap instruction as placeholder
        let trap_byte = match self.target_info().arch {
            TargetArch::X86_64 => 0xCC,   // INT3
            TargetArch::AArch64 => 0x00,   // BRK #0 (placeholder)
        };

        Ok(CompiledCode {
            code: vec![trap_byte],
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
        Err(CodegenError::BackendError(
            "stub backend cannot produce executable code".to_string(),
        ))
    }

    fn target_info(&self) -> TargetInfo {
        #[cfg(target_arch = "x86_64")]
        { TargetInfo { arch: TargetArch::X86_64, pointer_size: 8 } }

        #[cfg(target_arch = "aarch64")]
        { TargetInfo { arch: TargetArch::AArch64, pointer_size: 8 } }

        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        { TargetInfo { arch: TargetArch::X86_64, pointer_size: 8 } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_module() -> crate::compiler::bytecode::Module {
        crate::compiler::bytecode::Module {
            magic: *b"RAYA",
            version: 1,
            flags: 0,
            constants: crate::compiler::bytecode::ConstantPool::new(),
            functions: vec![],
            classes: vec![],
            metadata: crate::compiler::bytecode::Metadata {
                name: "test".to_string(),
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

    #[test]
    fn test_stub_compile() {
        let stub = StubBackend;
        assert_eq!(stub.name(), "stub");

        let func = JitFunction::new(0, "test".to_string(), 0, 0);

        let module = make_module();
        let ctx = ModuleContext {
            module: &module,
            func_index: 0,
        };

        let result = stub.compile_function(&func, &ctx);
        assert!(result.is_ok());
        let code = result.unwrap();
        assert!(!code.code.is_empty());
    }

    #[test]
    fn test_stub_finalize_fails() {
        let stub = StubBackend;
        let module = make_module();
        let ctx = ModuleContext {
            module: &module,
            func_index: 0,
        };
        let func = JitFunction::new(0, "test".to_string(), 0, 0);
        let mut code = stub.compile_function(&func, &ctx).unwrap();

        struct NoResolver;
        impl SymbolResolver for NoResolver {
            fn resolve_runtime_helper(&self, _: RuntimeHelper) -> Option<usize> { None }
            fn resolve_jit_function(&self, _: u32) -> Option<usize> { None }
        }

        let result = stub.finalize(&mut code, &NoResolver);
        assert!(result.is_err());
    }
}
