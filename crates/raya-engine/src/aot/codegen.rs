#![allow(missing_docs)]
//! AOT code generation
//!
//! Compiles all modules (project + dependencies) into a single contiguous
//! code blob. Each function is compiled independently via Cranelift, since
//! all inter-function calls go through the AotHelperTable (indirect calls
//! through function pointers) — no relocations are needed.
//!
//! Pipeline per function:
//! 1. `AotCompilable::analyze()` → `SuspensionAnalysis`
//! 2. `AotCompilable::emit_blocks()` → `Vec<SmBlock>`
//! 3. `transform_to_state_machine()` → `StateMachineFunction`
//! 4. `lower_function()` → Cranelift IR
//! 5. `ctx.compile(isa)` → machine code bytes
//! 6. Append to contiguous code blob with alignment

use std::sync::Arc;

use cranelift_codegen::ir::UserFuncName;
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

use super::lowering::{self, aot_entry_signature};
use super::traits::{AotCompilable, AotError, compile_to_state_machine};

// =============================================================================
// Public types
// =============================================================================

/// A global function ID combining module index and function index.
///
/// Upper 16 bits = module_index, lower 16 bits = func_index.
/// This allows up to 65536 modules with 65536 functions each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlobalFuncId(pub u32);

impl GlobalFuncId {
    /// Create a new global function ID.
    pub fn new(module_index: u16, func_index: u16) -> Self {
        Self((module_index as u32) << 16 | func_index as u32)
    }

    /// Extract the module index.
    pub fn module_index(self) -> u16 {
        (self.0 >> 16) as u16
    }

    /// Extract the function index within the module.
    pub fn func_index(self) -> u16 {
        self.0 as u16
    }
}

/// Entry in the function table, mapping global IDs to code offsets.
#[derive(Debug, Clone)]
pub struct FuncTableEntry {
    /// Global function ID (module_index << 16 | func_index).
    pub global_func_id: GlobalFuncId,

    /// Byte offset of this function's code within the code section.
    pub code_offset: u64,

    /// Size of this function's code in bytes.
    pub code_size: u64,

    /// Number of local variables (for frame allocation).
    pub local_count: u32,

    /// Number of parameters.
    pub param_count: u32,

    /// Module index this function belongs to.
    pub module_index: u16,

    /// Function name (for debugging).
    pub name: String,
}

/// The compiled AOT bundle: machine code + metadata.
///
/// This is the output of the compilation pipeline, ready to be
/// serialized into the bundle format.
#[derive(Debug)]
pub struct AotBundle {
    /// Raw machine code for all functions (contiguous blob).
    /// Functions are laid out sequentially, 16-byte aligned.
    pub code: Vec<u8>,

    /// Function table: maps global function IDs to code offsets.
    pub func_table: Vec<FuncTableEntry>,

    /// Target triple this code was compiled for (e.g. "aarch64-apple-darwin").
    pub target_triple: String,
}

/// Input to the AOT compiler — either source IR or bytecode.
#[derive(Debug)]
pub enum AotModuleInput {
    /// Path A: compiled from source, has full IR.
    Source {
        /// Module name (for diagnostics and linking).
        module_name: String,
    },

    /// Path B: loaded from .ryb, lifted through JIT pipeline.
    Bytecode {
        /// Module name.
        module_name: String,
        /// The decoded bytecode module.
        module: crate::compiler::bytecode::module::Module,
    },
}

impl AotModuleInput {
    /// Get the module name.
    pub fn name(&self) -> &str {
        match self {
            AotModuleInput::Source { module_name, .. } => module_name,
            AotModuleInput::Bytecode { module_name, .. } => module_name,
        }
    }
}

impl AotBundle {
    /// Create an empty bundle.
    pub fn empty(target_triple: String) -> Self {
        Self {
            code: Vec::new(),
            func_table: Vec::new(),
            target_triple,
        }
    }

    /// Total number of compiled functions.
    pub fn function_count(&self) -> usize {
        self.func_table.len()
    }

    /// Total code size in bytes.
    pub fn code_size(&self) -> usize {
        self.code.len()
    }
}

// =============================================================================
// Compilation pipeline
// =============================================================================

/// A function to be compiled, with its module assignment.
pub struct CompilableFunction<'a> {
    /// The compilable function (either IR adapter or lifted bytecode).
    pub func: &'a dyn AotCompilable,
    /// Module index for the global function ID.
    pub module_index: u16,
    /// Function index within the module.
    pub func_index: u16,
}

/// Alignment for each function's code within the blob (16 bytes).
const FUNC_ALIGN: usize = 16;

/// Create the native code ISA for the current platform.
pub fn create_native_isa() -> Result<Arc<dyn TargetIsa>, AotError> {
    let mut flag_builder = settings::builder();
    flag_builder.set("opt_level", "speed")
        .map_err(|e| AotError::CodegenFailed(format!("Failed to set opt_level: {}", e)))?;

    let flags = settings::Flags::new(flag_builder);

    cranelift_native::builder()
        .map_err(|e| AotError::CodegenFailed(format!("Failed to create native ISA: {}", e)))?
        .finish(flags)
        .map_err(|e| AotError::CodegenFailed(format!("Failed to finish ISA: {}", e)))
}

/// Compile a set of functions into an `AotBundle`.
///
/// Each function is compiled independently (no inter-function relocations
/// needed since all calls go through the AotHelperTable). The resulting
/// machine code is concatenated into a contiguous blob with 16-byte alignment
/// between functions.
pub fn compile_functions(
    functions: &[CompilableFunction],
    isa: Arc<dyn TargetIsa>,
) -> Result<AotBundle, AotError> {
    let target_triple = isa.triple().to_string();
    let call_conv = isa.default_call_conv();

    if functions.is_empty() {
        return Ok(AotBundle::empty(target_triple));
    }

    let mut code_blob = Vec::new();
    let mut func_table = Vec::new();
    let mut func_builder_ctx = FunctionBuilderContext::new();

    for compilable in functions {
        let global_id = GlobalFuncId::new(compilable.module_index, compilable.func_index);
        let func_name = compilable.func.name().unwrap_or("anon").to_string();

        // 1. Run through the full pipeline: analyze → emit → transform
        let sm_func = compile_to_state_machine(compilable.func, global_id.0);

        // 2. Build Cranelift IR
        let mut codegen_ctx = Context::new();
        codegen_ctx.func.signature = aot_entry_signature(call_conv);
        codegen_ctx.func.name = UserFuncName::user(
            compilable.module_index as u32,
            compilable.func_index as u32,
        );

        {
            let builder = FunctionBuilder::new(
                &mut codegen_ctx.func,
                &mut func_builder_ctx,
            );

            lowering::lower_function(&sm_func, builder)
                .map_err(|e| AotError::LoweringFailed(format!(
                    "Failed to lower '{}': {}", func_name, e
                )))?;
        }

        // 3. Compile to machine code
        let mut ctrl_plane = cranelift_codegen::control::ControlPlane::default();
        codegen_ctx.compile(&*isa, &mut ctrl_plane)
            .map_err(|e| AotError::CodegenFailed(format!(
                "Failed to compile '{}': {:?}", func_name, e
            )))?;

        let compiled = codegen_ctx.compiled_code()
            .ok_or_else(|| AotError::CodegenFailed(format!(
                "No compiled code for '{}'", func_name
            )))?;

        let machine_code = compiled.code_buffer();

        // 4. Align to FUNC_ALIGN boundary
        let padding = (FUNC_ALIGN - (code_blob.len() % FUNC_ALIGN)) % FUNC_ALIGN;
        code_blob.extend(std::iter::repeat(0u8).take(padding));

        let code_offset = code_blob.len() as u64;
        let code_size = machine_code.len() as u64;
        code_blob.extend_from_slice(machine_code);

        // 5. Record function table entry
        func_table.push(FuncTableEntry {
            global_func_id: global_id,
            code_offset,
            code_size,
            local_count: sm_func.local_count,
            param_count: sm_func.param_count,
            module_index: compilable.module_index,
            name: func_name,
        });
    }

    Ok(AotBundle {
        code: code_blob,
        func_table,
        target_triple,
    })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aot::statemachine::*;
    use crate::aot::analysis::SuspensionAnalysis;

    #[test]
    fn test_global_func_id() {
        let id = GlobalFuncId::new(3, 42);
        assert_eq!(id.module_index(), 3);
        assert_eq!(id.func_index(), 42);
        assert_eq!(id.0, (3 << 16) | 42);
    }

    #[test]
    fn test_global_func_id_max() {
        let id = GlobalFuncId::new(0xFFFF, 0xFFFF);
        assert_eq!(id.module_index(), 0xFFFF);
        assert_eq!(id.func_index(), 0xFFFF);
    }

    #[test]
    fn test_empty_bundle() {
        let bundle = AotBundle::empty("aarch64-apple-darwin".to_string());
        assert_eq!(bundle.function_count(), 0);
        assert_eq!(bundle.code_size(), 0);
    }

    #[test]
    fn test_create_native_isa() {
        let isa = create_native_isa();
        assert!(isa.is_ok(), "Failed to create native ISA: {:?}", isa.err());
    }

    /// A test-only compilable function that produces a simple constant return.
    struct TestCompilable {
        param_count: u32,
        local_count: u32,
        name: String,
    }

    impl AotCompilable for TestCompilable {
        fn analyze(&self) -> SuspensionAnalysis {
            SuspensionAnalysis::none()
        }

        fn emit_blocks(&self) -> Vec<SmBlock> {
            vec![SmBlock {
                id: SmBlockId(0),
                kind: SmBlockKind::Body,
                instructions: vec![
                    SmInstr::ConstI32 { dest: 0, value: 42 },
                    SmInstr::BoxI32 { dest: 1, src: 0 },
                ],
                terminator: SmTerminator::Return { value: 1 },
            }]
        }

        fn param_count(&self) -> u32 { self.param_count }
        fn local_count(&self) -> u32 { self.local_count }
        fn name(&self) -> Option<&str> { Some(&self.name) }
    }

    #[test]
    fn test_compile_single_function() {
        let isa = create_native_isa().expect("Failed to create ISA");

        let func = TestCompilable {
            param_count: 0,
            local_count: 1,
            name: "test_func".to_string(),
        };

        let functions = vec![CompilableFunction {
            func: &func,
            module_index: 0,
            func_index: 0,
        }];

        let bundle = compile_functions(&functions, isa).expect("Compilation failed");

        assert_eq!(bundle.function_count(), 1);
        assert!(bundle.code_size() > 0, "Code should be non-empty");
        assert_eq!(bundle.func_table[0].global_func_id, GlobalFuncId::new(0, 0));
        assert_eq!(bundle.func_table[0].local_count, 1);
        assert_eq!(bundle.func_table[0].param_count, 0);
        assert_eq!(bundle.func_table[0].name, "test_func");
        assert!(bundle.func_table[0].code_size > 0);
        assert_eq!(bundle.func_table[0].code_offset, 0); // First function starts at 0
    }

    #[test]
    fn test_compile_multiple_functions() {
        let isa = create_native_isa().expect("Failed to create ISA");

        let func_a = TestCompilable {
            param_count: 0,
            local_count: 1,
            name: "func_a".to_string(),
        };
        let func_b = TestCompilable {
            param_count: 2,
            local_count: 3,
            name: "func_b".to_string(),
        };

        let functions = vec![
            CompilableFunction {
                func: &func_a,
                module_index: 0,
                func_index: 0,
            },
            CompilableFunction {
                func: &func_b,
                module_index: 0,
                func_index: 1,
            },
        ];

        let bundle = compile_functions(&functions, isa).expect("Compilation failed");

        assert_eq!(bundle.function_count(), 2);
        assert!(bundle.code_size() > 0);
        assert_eq!(bundle.func_table[0].name, "func_a");
        assert_eq!(bundle.func_table[1].name, "func_b");
        assert_eq!(bundle.func_table[1].param_count, 2);
        assert_eq!(bundle.func_table[1].local_count, 3);

        // Second function should be after the first, aligned
        assert!(bundle.func_table[1].code_offset >= bundle.func_table[0].code_size);
        // Alignment check
        assert_eq!(bundle.func_table[1].code_offset as usize % FUNC_ALIGN, 0);
    }

    #[test]
    fn test_compile_empty() {
        let isa = create_native_isa().expect("Failed to create ISA");
        let functions: Vec<CompilableFunction> = vec![];
        let bundle = compile_functions(&functions, isa).expect("Compilation failed");
        assert_eq!(bundle.function_count(), 0);
        assert_eq!(bundle.code_size(), 0);
    }

    #[test]
    fn test_compile_arithmetic_function() {
        let isa = create_native_isa().expect("Failed to create ISA");

        /// A function that does: unbox a, unbox b, add, box result
        struct AddFunc;
        impl AotCompilable for AddFunc {
            fn analyze(&self) -> SuspensionAnalysis { SuspensionAnalysis::none() }
            fn emit_blocks(&self) -> Vec<SmBlock> {
                vec![SmBlock {
                    id: SmBlockId(0),
                    kind: SmBlockKind::Body,
                    instructions: vec![
                        SmInstr::LoadLocal { dest: 0, index: 0 },
                        SmInstr::LoadLocal { dest: 1, index: 1 },
                        SmInstr::UnboxI32 { dest: 2, src: 0 },
                        SmInstr::UnboxI32 { dest: 3, src: 1 },
                        SmInstr::I32BinOp { dest: 4, op: SmI32BinOp::Add, left: 2, right: 3 },
                        SmInstr::BoxI32 { dest: 5, src: 4 },
                    ],
                    terminator: SmTerminator::Return { value: 5 },
                }]
            }
            fn param_count(&self) -> u32 { 2 }
            fn local_count(&self) -> u32 { 2 }
            fn name(&self) -> Option<&str> { Some("add") }
        }

        let func = AddFunc;
        let functions = vec![CompilableFunction {
            func: &func,
            module_index: 0,
            func_index: 0,
        }];

        let bundle = compile_functions(&functions, isa).expect("Compilation failed");

        assert_eq!(bundle.function_count(), 1);
        assert!(bundle.code_size() > 0);
        assert_eq!(bundle.func_table[0].name, "add");
        assert_eq!(bundle.func_table[0].param_count, 2);
    }
}
