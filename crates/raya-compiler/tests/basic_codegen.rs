//! Basic code generation tests for Phase 1

use raya_compiler::Opcode;
use raya_compiler::Compiler;
use raya_parser::{Interner, Parser, TypeContext};

fn parse(source: &str) -> (raya_parser::ast::Module, Interner) {
    let parser = Parser::new(source).expect("Failed to tokenize");
    parser.parse().expect("Failed to parse")
}

fn compile(source: &str) -> raya_compiler::Module {
    let (module, interner) = parse(source);
    let type_ctx = TypeContext::new();
    let mut compiler = Compiler::new(type_ctx, &interner);
    compiler.compile(&module).expect("Failed to compile")
}

#[test]
fn test_compile_integer_literal() {
    let module = compile("42;");
    
    assert_eq!(module.functions.len(), 1);
    let main = &module.functions[0];
    assert_eq!(main.name, "main");
    
    // Should contain: ConstI32, Pop, ConstNull, Return
    assert!(main.code.contains(&(Opcode::ConstI32 as u8)));
    assert!(main.code.contains(&(Opcode::Pop as u8)));
    assert!(main.code.contains(&(Opcode::ConstNull as u8)));
    assert!(main.code.contains(&(Opcode::Return as u8)));
}

#[test]
fn test_compile_string_literal() {
    let module = compile(r#""hello";"#);
    
    assert_eq!(module.functions.len(), 1);
    let main = &module.functions[0];
    
    // Should have added string to constant pool
    assert_eq!(module.constants.get_string(0), Some("hello"));
    
    // Should contain: ConstStr opcode
    assert!(main.code.contains(&(Opcode::ConstStr as u8)));
}

#[test]
fn test_compile_boolean_literals() {
    let module = compile("true; false;");
    
    let main = &module.functions[0];
    
    // Should contain both ConstTrue and ConstFalse
    assert!(main.code.contains(&(Opcode::ConstTrue as u8)));
    assert!(main.code.contains(&(Opcode::ConstFalse as u8)));
}

#[test]
fn test_compile_null_literal() {
    let module = compile("null;");
    
    let main = &module.functions[0];
    
    // Should have two ConstNull: one for the expression, one for implicit return
    let null_count = main.code.iter().filter(|&&b| b == Opcode::ConstNull as u8).count();
    assert!(null_count >= 1);
}

#[test]
fn test_compile_variable_declaration() {
    let module = compile("let x = 42;");
    
    let main = &module.functions[0];
    
    // Should contain: ConstI32, StoreLocal
    assert!(main.code.contains(&(Opcode::ConstI32 as u8)));
    assert!(main.code.contains(&(Opcode::StoreLocal as u8)));
    
    // Should have 1 local variable
    assert_eq!(main.local_count, 1);
}

#[test]
fn test_compile_variable_read() {
    let module = compile("let x = 42; x;");
    
    let main = &module.functions[0];
    
    // Should contain: LoadLocal
    assert!(main.code.contains(&(Opcode::LoadLocal as u8)));
}

#[test]
fn test_compile_binary_add() {
    let module = compile("1 + 2;");
    
    let main = &module.functions[0];
    
    // Should contain: ConstI32 (twice), Iadd
    assert!(main.code.contains(&(Opcode::Iadd as u8)));
}

#[test]
fn test_compile_binary_comparison() {
    let module = compile("1 < 2;");
    
    let main = &module.functions[0];
    
    // Should contain: Ilt
    assert!(main.code.contains(&(Opcode::Ilt as u8)));
}

#[test]
fn test_compile_unary_negation() {
    let module = compile("-42;");
    
    let main = &module.functions[0];
    
    // Should contain: ConstI32, Ineg
    assert!(main.code.contains(&(Opcode::Ineg as u8)));
}

#[test]
fn test_compile_unary_not() {
    let module = compile("!true;");
    
    let main = &module.functions[0];
    
    // Should contain: ConstTrue, Not
    assert!(main.code.contains(&(Opcode::Not as u8)));
}

#[test]
fn test_compile_assignment() {
    let module = compile("let x = 1; x = 2;");
    
    let main = &module.functions[0];
    
    // Should have multiple StoreLocal (one for declaration, one for assignment)
    let store_count = main.code.iter().filter(|&&b| b == Opcode::StoreLocal as u8).count();
    assert!(store_count >= 2);
}

#[test]
fn test_compile_multiple_variables() {
    // Note: Raya doesn't support standalone block statements (they're object literals)
    // Test multiple variable declarations instead
    let module = compile("let x = 1; let y = 2;");

    let main = &module.functions[0];

    // Should have 2 local variables
    assert_eq!(main.local_count, 2);
}

#[test]
fn test_compile_return_statement() {
    let module = compile("return 42;");
    
    let main = &module.functions[0];
    
    // Should contain: ConstI32, Return
    // Note: there will be unreachable code after explicit return
    assert!(main.code.contains(&(Opcode::Return as u8)));
}

#[test]
fn test_compile_complex_expression() {
    let module = compile("let result = (10 + 20) * 2 - 5;");
    
    let main = &module.functions[0];
    
    // Should contain multiple arithmetic opcodes
    assert!(main.code.contains(&(Opcode::Iadd as u8)));
    assert!(main.code.contains(&(Opcode::Imul as u8)));
    assert!(main.code.contains(&(Opcode::Isub as u8)));
}
