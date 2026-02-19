//! Source compilation pipeline.
//!
//! Parse → Bind → TypeCheck → Compile to bytecode.

use raya_engine::compiler::{Compiler, Module};
use raya_engine::parser::checker::{Binder, TypeChecker, ScopeId};
use raya_engine::parser::{Interner, Parser, TypeContext};

use crate::builtins;
use crate::error::RuntimeError;

/// Compile Raya source code to a bytecode module.
///
/// Prepends builtin class sources and standard library sources so that
/// user code can reference Map, Set, Channel, Logger, Math, etc.
pub fn compile_source(source: &str) -> Result<(Module, Interner), RuntimeError> {
    let full_source = format!(
        "{}\n{}\n{}",
        builtins::builtin_sources(),
        builtins::std_sources(),
        source,
    );

    // Parse
    let parser = Parser::new(&full_source)
        .map_err(|e| RuntimeError::Lex(format!("{:?}", e)))?;
    let (ast, interner) = parser
        .parse()
        .map_err(|e| RuntimeError::Parse(format!("{:?}", e)))?;

    // Bind (creates symbol table)
    let mut type_ctx = TypeContext::new();
    let mut binder = Binder::new(&mut type_ctx, &interner);

    // Register only intrinsics (__NATIVE_CALL, etc.) — builtin class sources
    // are included in the source text, so their types come from parsing.
    let empty_sigs: Vec<raya_engine::parser::checker::BuiltinSignatures> = vec![];
    binder.register_builtins(&empty_sigs);

    let mut symbols = binder
        .bind_module(&ast)
        .map_err(|e| RuntimeError::TypeCheck(format!("Binding error: {:?}", e)))?;

    // Type check
    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let check_result = checker
        .check_module(&ast)
        .map_err(|e| RuntimeError::TypeCheck(format!("{:?}", e)))?;

    // Apply inferred types to symbol table
    for ((scope_id, name), ty) in check_result.inferred_types {
        symbols.update_type(ScopeId(scope_id), &name, ty);
    }

    // Compile via IR pipeline
    let compiler = Compiler::new(type_ctx, &interner)
        .with_expr_types(check_result.expr_types);
    let bytecode = compiler.compile_via_ir(&ast)?;

    Ok((bytecode, interner))
}
