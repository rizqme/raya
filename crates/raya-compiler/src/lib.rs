//! Raya Compiler - AST to Bytecode Code Generation
//!
//! This crate implements the compiler that transforms typed AST into bytecode.

pub mod bytecode;
pub mod codegen;
pub mod error;
pub mod module_builder;

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
}
