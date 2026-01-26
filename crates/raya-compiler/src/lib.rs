//! Raya Compiler - AST to Bytecode Code Generation
//!
//! This crate implements the compiler that transforms typed AST into bytecode.
//!
//! # Architecture
//!
//! The compilation pipeline is:
//! 1. AST (from raya-parser) → IR (intermediate representation)
//! 2. IR → Monomorphization (generic specialization)
//! 3. IR → Optimizations (constant folding, DCE)
//! 4. IR → Bytecode
//!
//! The IR uses Three-Address Code (TAC) with Basic Blocks.

pub mod bytecode;
pub mod codegen;
pub mod error;
pub mod ir;
pub mod lower;
pub mod module_builder;
pub mod monomorphize;
pub mod optimize;

pub use codegen::CodeGenerator;
pub use error::{CompileError, CompileResult};
pub use module_builder::ModuleBuilder;

// Re-export bytecode types for convenience
pub use bytecode::{
    BytecodeReader, BytecodeWriter, ClassDef, ConstantPool, DecodeError, Export, Function,
    Import, Metadata, Method, Module, ModuleError, Opcode, SymbolType, VerifyError, verify_module,
};

use raya_parser::ast;
use raya_parser::Interner;
use raya_parser::TypeContext;

/// Main compiler entry point
pub struct Compiler<'a> {
    type_ctx: TypeContext,
    interner: &'a Interner,
}

impl<'a> Compiler<'a> {
    pub fn new(type_ctx: TypeContext, interner: &'a Interner) -> Self {
        Self { type_ctx, interner }
    }

    /// Compile a module into bytecode
    pub fn compile(&mut self, module: &ast::Module) -> CompileResult<Module> {
        let mut codegen = CodeGenerator::new(&self.type_ctx, self.interner);
        codegen.compile_program(module)
    }

    /// Compile a module to IR (for debugging/inspection)
    pub fn compile_to_ir(&self, module: &ast::Module) -> ir::IrModule {
        let mut lowerer = lower::Lowerer::new(&self.type_ctx, self.interner);
        lowerer.lower_module(module)
    }

    /// Compile a module to IR with monomorphization
    ///
    /// This performs the full IR compilation pipeline including:
    /// 1. AST lowering to IR
    /// 2. Monomorphization (generic specialization)
    /// 3. Optimization passes
    pub fn compile_to_optimized_ir(&self, module: &ast::Module) -> ir::IrModule {
        // Step 1: Lower AST to IR
        let mut lowerer = lower::Lowerer::new(&self.type_ctx, self.interner);
        let mut ir_module = lowerer.lower_module(module);

        // Step 2: Monomorphization
        let _mono_result = monomorphize::monomorphize(&mut ir_module, &self.type_ctx, self.interner);

        // Step 3: Optimization passes
        let optimizer = optimize::Optimizer::basic();
        optimizer.optimize(&mut ir_module);

        ir_module
    }
}
