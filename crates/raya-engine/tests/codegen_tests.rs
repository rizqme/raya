//! Comprehensive tests for IR to Bytecode code generation
//!
//! Tests cover:
//! - Basic expressions (literals, binary ops, unary ops)
//! - Control flow (branches, loops)
//! - Function calls
//! - Classes and objects
//! - Closures

use raya_engine::compiler::codegen;
use raya_engine::compiler::ir::block::{BasicBlock, BasicBlockId, Terminator};
use raya_engine::compiler::ir::function::IrFunction;
use raya_engine::compiler::ir::instr::{BinaryOp, ClassId, FunctionId, IrInstr, UnaryOp};
use raya_engine::compiler::ir::module::{IrClass, IrField, IrModule};
use raya_engine::compiler::ir::value::{IrConstant, IrValue, Register, RegisterId};
use raya_engine::compiler::Opcode;
use raya_engine::parser::TypeId;

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

fn make_reg(id: u32, ty: u32) -> Register {
    Register::new(RegisterId::new(id), TypeId::new(ty))
}

fn decode_i32(code: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes([
        code[offset],
        code[offset + 1],
        code[offset + 2],
        code[offset + 3],
    ])
}

fn decode_u16(code: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([code[offset], code[offset + 1]])
}

fn decode_f64(code: &[u8], offset: usize) -> f64 {
    f64::from_le_bytes([
        code[offset],
        code[offset + 1],
        code[offset + 2],
        code[offset + 3],
        code[offset + 4],
        code[offset + 5],
        code[offset + 6],
        code[offset + 7],
    ])
}

// =============================================================================
// BASIC EXPRESSION TESTS
// =============================================================================

#[test]
fn test_integer_literal() {
    let mut module = IrModule::new("test");
    let mut func = IrFunction::new("main", vec![], TypeId::new(0));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let r0 = make_reg(0, 1);
    entry.add_instr(IrInstr::Assign {
        dest: r0.clone(),
        value: IrValue::Constant(IrConstant::I32(42)),
    });
    entry.set_terminator(Terminator::Return(Some(r0)));
    func.add_block(entry);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    // Should emit: CONST_I32 42, STORE_LOCAL, LOAD_LOCAL, RETURN
    assert_eq!(code[0], Opcode::ConstI32 as u8);
    assert_eq!(decode_i32(code, 1), 42);
}

#[test]
fn test_float_literal() {
    let mut module = IrModule::new("test");
    let mut func = IrFunction::new("main", vec![], TypeId::new(0));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let r0 = make_reg(0, 2);
    entry.add_instr(IrInstr::Assign {
        dest: r0.clone(),
        value: IrValue::Constant(IrConstant::F64(3.14)),
    });
    entry.set_terminator(Terminator::Return(Some(r0)));
    func.add_block(entry);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    assert_eq!(code[0], Opcode::ConstF64 as u8);
    let value = decode_f64(code, 1);
    assert!((value - 3.14).abs() < 0.0001);
}

#[test]
fn test_boolean_literals() {
    let mut module = IrModule::new("test");
    let mut func = IrFunction::new("main", vec![], TypeId::new(0));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let r0 = make_reg(0, 3);
    let r1 = make_reg(1, 3);
    entry.add_instr(IrInstr::Assign {
        dest: r0.clone(),
        value: IrValue::Constant(IrConstant::Boolean(true)),
    });
    entry.add_instr(IrInstr::Assign {
        dest: r1.clone(),
        value: IrValue::Constant(IrConstant::Boolean(false)),
    });
    entry.set_terminator(Terminator::Return(Some(r0)));
    func.add_block(entry);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    assert_eq!(code[0], Opcode::ConstTrue as u8);
}

#[test]
fn test_null_literal() {
    let mut module = IrModule::new("test");
    let mut func = IrFunction::new("main", vec![], TypeId::new(0));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let r0 = make_reg(0, 0);
    entry.add_instr(IrInstr::Assign {
        dest: r0.clone(),
        value: IrValue::Constant(IrConstant::Null),
    });
    entry.set_terminator(Terminator::Return(Some(r0)));
    func.add_block(entry);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    assert_eq!(code[0], Opcode::ConstNull as u8);
}

#[test]
fn test_string_literal() {
    let mut module = IrModule::new("test");
    let mut func = IrFunction::new("main", vec![], TypeId::new(0));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let r0 = make_reg(0, 4);
    entry.add_instr(IrInstr::Assign {
        dest: r0.clone(),
        value: IrValue::Constant(IrConstant::String("hello".to_string())),
    });
    entry.set_terminator(Terminator::Return(Some(r0)));
    func.add_block(entry);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    assert_eq!(code[0], Opcode::ConstStr as u8);
    // String index should be 0 (first string in pool)
    assert_eq!(decode_u16(code, 1), 0);
}

// =============================================================================
// BINARY OPERATION TESTS
// =============================================================================

#[test]
fn test_binary_add() {
    let mut module = IrModule::new("test");
    // Use TypeId(2) = Boolean which falls through to integer opcodes
    let mut func = IrFunction::new("add", vec![], TypeId::new(2));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let r0 = make_reg(0, 2);
    let r1 = make_reg(1, 2);
    let r2 = make_reg(2, 2);

    entry.add_instr(IrInstr::Assign {
        dest: r0.clone(),
        value: IrValue::Constant(IrConstant::I32(10)),
    });
    entry.add_instr(IrInstr::Assign {
        dest: r1.clone(),
        value: IrValue::Constant(IrConstant::I32(20)),
    });
    entry.add_instr(IrInstr::BinaryOp {
        dest: r2.clone(),
        op: BinaryOp::Add,
        left: r0,
        right: r1,
    });
    entry.set_terminator(Terminator::Return(Some(r2)));
    func.add_block(entry);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    // Find IADD opcode in the bytecode
    let has_iadd = code.iter().any(|&b| b == Opcode::Iadd as u8);
    assert!(has_iadd, "Should emit IADD opcode");
}

#[test]
fn test_binary_operations() {
    let ops = vec![
        (BinaryOp::Add, Opcode::Iadd),
        (BinaryOp::Sub, Opcode::Isub),
        (BinaryOp::Mul, Opcode::Imul),
        (BinaryOp::Div, Opcode::Idiv),
        (BinaryOp::Mod, Opcode::Imod),
        (BinaryOp::Equal, Opcode::Ieq),
        (BinaryOp::NotEqual, Opcode::Ine),
        (BinaryOp::Less, Opcode::Ilt),
        (BinaryOp::LessEqual, Opcode::Ile),
        (BinaryOp::Greater, Opcode::Igt),
        (BinaryOp::GreaterEqual, Opcode::Ige),
    ];

    for (ir_op, expected_opcode) in ops {
        let mut module = IrModule::new("test");
        // Use TypeId(2) = Boolean which falls through to integer opcodes
        let mut func = IrFunction::new("op", vec![], TypeId::new(2));
        let mut entry = BasicBlock::new(BasicBlockId(0));

        let r0 = make_reg(0, 2);
        let r1 = make_reg(1, 2);
        let r2 = make_reg(2, 2);

        entry.add_instr(IrInstr::Assign {
            dest: r0.clone(),
            value: IrValue::Constant(IrConstant::I32(5)),
        });
        entry.add_instr(IrInstr::Assign {
            dest: r1.clone(),
            value: IrValue::Constant(IrConstant::I32(3)),
        });
        entry.add_instr(IrInstr::BinaryOp {
            dest: r2.clone(),
            op: ir_op,
            left: r0,
            right: r1,
        });
        entry.set_terminator(Terminator::Return(Some(r2)));
        func.add_block(entry);
        module.add_function(func);

        let result = codegen::generate(&module, false).unwrap();
        let code = &result.functions[0].code;

        let has_opcode = code.iter().any(|&b| b == expected_opcode as u8);
        assert!(
            has_opcode,
            "IR op {:?} should emit {:?}",
            ir_op, expected_opcode
        );
    }
}

// =============================================================================
// UNARY OPERATION TESTS
// =============================================================================

#[test]
fn test_unary_neg() {
    let mut module = IrModule::new("test");
    let mut func = IrFunction::new("neg", vec![], TypeId::new(1));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let r0 = make_reg(0, 1);
    let r1 = make_reg(1, 1);

    entry.add_instr(IrInstr::Assign {
        dest: r0.clone(),
        value: IrValue::Constant(IrConstant::I32(42)),
    });
    entry.add_instr(IrInstr::UnaryOp {
        dest: r1.clone(),
        op: UnaryOp::Neg,
        operand: r0,
    });
    entry.set_terminator(Terminator::Return(Some(r1)));
    func.add_block(entry);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    let has_ineg = code.iter().any(|&b| b == Opcode::Ineg as u8);
    assert!(has_ineg, "Should emit INEG opcode");
}

#[test]
fn test_unary_not() {
    let mut module = IrModule::new("test");
    let mut func = IrFunction::new("not", vec![], TypeId::new(3));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let r0 = make_reg(0, 3);
    let r1 = make_reg(1, 3);

    entry.add_instr(IrInstr::Assign {
        dest: r0.clone(),
        value: IrValue::Constant(IrConstant::Boolean(true)),
    });
    entry.add_instr(IrInstr::UnaryOp {
        dest: r1.clone(),
        op: UnaryOp::Not,
        operand: r0,
    });
    entry.set_terminator(Terminator::Return(Some(r1)));
    func.add_block(entry);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    let has_not = code.iter().any(|&b| b == Opcode::Not as u8);
    assert!(has_not, "Should emit NOT opcode");
}

// =============================================================================
// CONTROL FLOW TESTS
// =============================================================================

#[test]
fn test_unconditional_jump() {
    let mut module = IrModule::new("test");
    let mut func = IrFunction::new("jump", vec![], TypeId::new(0));

    // bb0: jump bb1
    let mut bb0 = BasicBlock::new(BasicBlockId(0));
    bb0.set_terminator(Terminator::Jump(BasicBlockId(1)));

    // bb1: return null
    let mut bb1 = BasicBlock::new(BasicBlockId(1));
    bb1.set_terminator(Terminator::Return(None));

    func.add_block(bb0);
    func.add_block(bb1);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    let has_jmp = code.iter().any(|&b| b == Opcode::Jmp as u8);
    assert!(has_jmp, "Should emit JMP opcode");
}

#[test]
fn test_conditional_branch() {
    let mut module = IrModule::new("test");
    let mut func = IrFunction::new("branch", vec![], TypeId::new(1));

    let cond = make_reg(0, 3);
    let result_reg = make_reg(1, 1);

    // bb0: cond = true; branch cond ? bb1 : bb2
    let mut bb0 = BasicBlock::new(BasicBlockId(0));
    bb0.add_instr(IrInstr::Assign {
        dest: cond.clone(),
        value: IrValue::Constant(IrConstant::Boolean(true)),
    });
    bb0.set_terminator(Terminator::Branch {
        cond: cond.clone(),
        then_block: BasicBlockId(1),
        else_block: BasicBlockId(2),
    });

    // bb1: result = 1; jump bb3
    let mut bb1 = BasicBlock::new(BasicBlockId(1));
    bb1.add_instr(IrInstr::Assign {
        dest: result_reg.clone(),
        value: IrValue::Constant(IrConstant::I32(1)),
    });
    bb1.set_terminator(Terminator::Jump(BasicBlockId(3)));

    // bb2: result = 0; jump bb3
    let mut bb2 = BasicBlock::new(BasicBlockId(2));
    bb2.add_instr(IrInstr::Assign {
        dest: result_reg.clone(),
        value: IrValue::Constant(IrConstant::I32(0)),
    });
    bb2.set_terminator(Terminator::Jump(BasicBlockId(3)));

    // bb3: return result
    let mut bb3 = BasicBlock::new(BasicBlockId(3));
    bb3.set_terminator(Terminator::Return(Some(result_reg)));

    func.add_block(bb0);
    func.add_block(bb1);
    func.add_block(bb2);
    func.add_block(bb3);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    let has_jmp_if_false = code.iter().any(|&b| b == Opcode::JmpIfFalse as u8);
    assert!(has_jmp_if_false, "Should emit JMP_IF_FALSE opcode");
}

#[test]
fn test_simple_loop() {
    let mut module = IrModule::new("test");
    let mut func = IrFunction::new("loop", vec![], TypeId::new(1));

    let i = make_reg(0, 1);
    let limit = make_reg(1, 1);
    let cond = make_reg(2, 3);
    let one = make_reg(3, 1);
    let new_i = make_reg(4, 1);

    // bb0: i = 0; limit = 10; jump bb1
    let mut bb0 = BasicBlock::new(BasicBlockId(0));
    bb0.add_instr(IrInstr::Assign {
        dest: i.clone(),
        value: IrValue::Constant(IrConstant::I32(0)),
    });
    bb0.add_instr(IrInstr::Assign {
        dest: limit.clone(),
        value: IrValue::Constant(IrConstant::I32(10)),
    });
    bb0.set_terminator(Terminator::Jump(BasicBlockId(1)));

    // bb1: cond = i < limit; branch cond ? bb2 : bb3
    let mut bb1 = BasicBlock::new(BasicBlockId(1));
    bb1.add_instr(IrInstr::BinaryOp {
        dest: cond.clone(),
        op: BinaryOp::Less,
        left: i.clone(),
        right: limit.clone(),
    });
    bb1.set_terminator(Terminator::Branch {
        cond: cond.clone(),
        then_block: BasicBlockId(2),
        else_block: BasicBlockId(3),
    });

    // bb2: one = 1; new_i = i + one; i = new_i; jump bb1
    let mut bb2 = BasicBlock::new(BasicBlockId(2));
    bb2.add_instr(IrInstr::Assign {
        dest: one.clone(),
        value: IrValue::Constant(IrConstant::I32(1)),
    });
    bb2.add_instr(IrInstr::BinaryOp {
        dest: new_i.clone(),
        op: BinaryOp::Add,
        left: i.clone(),
        right: one.clone(),
    });
    bb2.add_instr(IrInstr::Assign {
        dest: i.clone(),
        value: IrValue::Register(new_i.clone()),
    });
    bb2.set_terminator(Terminator::Jump(BasicBlockId(1)));

    // bb3: return i
    let mut bb3 = BasicBlock::new(BasicBlockId(3));
    bb3.set_terminator(Terminator::Return(Some(i)));

    func.add_block(bb0);
    func.add_block(bb1);
    func.add_block(bb2);
    func.add_block(bb3);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    // Should have multiple jumps for the loop
    let jmp_count = code.iter().filter(|&&b| b == Opcode::Jmp as u8).count();
    assert!(jmp_count >= 2, "Loop should have at least 2 jumps");
}

// =============================================================================
// FUNCTION CALL TESTS
// =============================================================================

#[test]
fn test_function_call() {
    let mut module = IrModule::new("test");

    // Add a helper function
    let mut helper = IrFunction::new("helper", vec![make_reg(0, 1)], TypeId::new(1));
    let mut helper_entry = BasicBlock::new(BasicBlockId(0));
    helper_entry.set_terminator(Terminator::Return(Some(make_reg(0, 1))));
    helper.add_block(helper_entry);
    module.add_function(helper);

    // Main function that calls helper
    let mut main = IrFunction::new("main", vec![], TypeId::new(1));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let arg = make_reg(0, 1);
    let result = make_reg(1, 1);

    entry.add_instr(IrInstr::Assign {
        dest: arg.clone(),
        value: IrValue::Constant(IrConstant::I32(42)),
    });
    entry.add_instr(IrInstr::Call {
        dest: Some(result.clone()),
        func: FunctionId::new(0),
        args: vec![arg],
    });
    entry.set_terminator(Terminator::Return(Some(result)));
    main.add_block(entry);
    module.add_function(main);

    let result = codegen::generate(&module, false).unwrap();

    // Main function should have CALL opcode
    let main_code = &result.functions[1].code;
    let has_call = main_code.iter().any(|&b| b == Opcode::Call as u8);
    assert!(has_call, "Should emit CALL opcode");
}

// =============================================================================
// LOCAL VARIABLE TESTS
// =============================================================================

#[test]
fn test_local_variables() {
    let mut module = IrModule::new("test");
    let mut func = IrFunction::new("locals", vec![], TypeId::new(1));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let r0 = make_reg(0, 1);
    let r1 = make_reg(1, 1);
    let r2 = make_reg(2, 1);

    // r0 = 10
    entry.add_instr(IrInstr::Assign {
        dest: r0.clone(),
        value: IrValue::Constant(IrConstant::I32(10)),
    });

    // r1 = 20
    entry.add_instr(IrInstr::Assign {
        dest: r1.clone(),
        value: IrValue::Constant(IrConstant::I32(20)),
    });

    // r2 = r0 + r1
    entry.add_instr(IrInstr::BinaryOp {
        dest: r2.clone(),
        op: BinaryOp::Add,
        left: r0,
        right: r1,
    });

    entry.set_terminator(Terminator::Return(Some(r2)));
    func.add_block(entry);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();

    // Check function has correct local count
    assert!(
        result.functions[0].local_count >= 3,
        "Should have at least 3 locals"
    );
}

#[test]
fn test_optimized_local_slots() {
    let mut module = IrModule::new("test");
    let mut func = IrFunction::new("opt", vec![], TypeId::new(1));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let r0 = make_reg(0, 1);

    entry.add_instr(IrInstr::Assign {
        dest: r0.clone(),
        value: IrValue::Constant(IrConstant::I32(42)),
    });
    entry.set_terminator(Terminator::Return(Some(r0)));
    func.add_block(entry);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    // First local (slot 0) should use optimized STORE_LOCAL_0/LOAD_LOCAL_0
    let has_store_local0 = code.iter().any(|&b| b == Opcode::StoreLocal0 as u8);
    let has_load_local0 = code.iter().any(|&b| b == Opcode::LoadLocal0 as u8);
    assert!(
        has_store_local0 || has_load_local0,
        "Should use optimized local slot 0 instructions"
    );
}

// =============================================================================
// ARRAY TESTS
// =============================================================================

#[test]
fn test_array_literal() {
    let mut module = IrModule::new("test");
    let mut func = IrFunction::new("arr", vec![], TypeId::new(5));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let r0 = make_reg(0, 1);
    let r1 = make_reg(1, 1);
    let r2 = make_reg(2, 1);
    let arr = make_reg(3, 5);

    entry.add_instr(IrInstr::Assign {
        dest: r0.clone(),
        value: IrValue::Constant(IrConstant::I32(1)),
    });
    entry.add_instr(IrInstr::Assign {
        dest: r1.clone(),
        value: IrValue::Constant(IrConstant::I32(2)),
    });
    entry.add_instr(IrInstr::Assign {
        dest: r2.clone(),
        value: IrValue::Constant(IrConstant::I32(3)),
    });
    entry.add_instr(IrInstr::ArrayLiteral {
        dest: arr.clone(),
        elements: vec![r0, r1, r2],
        elem_ty: TypeId::new(1),
    });
    entry.set_terminator(Terminator::Return(Some(arr)));
    func.add_block(entry);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    let has_array_literal = code.iter().any(|&b| b == Opcode::ArrayLiteral as u8);
    assert!(has_array_literal, "Should emit ARRAY_LITERAL opcode");
}

#[test]
fn test_array_access() {
    let mut module = IrModule::new("test");
    let mut func = IrFunction::new("access", vec![], TypeId::new(1));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let arr = make_reg(0, 5);
    let elem = make_reg(1, 1);
    let idx = make_reg(2, 1);
    let result = make_reg(3, 1);

    // Create array
    entry.add_instr(IrInstr::Assign {
        dest: elem.clone(),
        value: IrValue::Constant(IrConstant::I32(42)),
    });
    entry.add_instr(IrInstr::ArrayLiteral {
        dest: arr.clone(),
        elements: vec![elem],
        elem_ty: TypeId::new(1),
    });

    // Access element
    entry.add_instr(IrInstr::Assign {
        dest: idx.clone(),
        value: IrValue::Constant(IrConstant::I32(0)),
    });
    entry.add_instr(IrInstr::LoadElement {
        dest: result.clone(),
        array: arr,
        index: idx,
    });

    entry.set_terminator(Terminator::Return(Some(result)));
    func.add_block(entry);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    let has_load_elem = code.iter().any(|&b| b == Opcode::LoadElem as u8);
    assert!(has_load_elem, "Should emit LOAD_ELEM opcode");
}

// =============================================================================
// OBJECT TESTS
// =============================================================================

#[test]
fn test_new_object() {
    let mut module = IrModule::new("test");

    // Add a class
    let class = IrClass::new("Point");
    module.add_class(class);

    let mut func = IrFunction::new("create", vec![], TypeId::new(10));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let obj = make_reg(0, 10);
    entry.add_instr(IrInstr::NewObject {
        dest: obj.clone(),
        class: ClassId::new(0),
    });
    entry.set_terminator(Terminator::Return(Some(obj)));
    func.add_block(entry);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    let has_new = code.iter().any(|&b| b == Opcode::New as u8);
    assert!(has_new, "Should emit NEW opcode");
}

#[test]
fn test_field_access() {
    let mut module = IrModule::new("test");

    // Add a class with a field
    let mut class = IrClass::new("Point");
    class.add_field(IrField::new("x", TypeId::new(1), 0));
    module.add_class(class);

    let mut func = IrFunction::new("getX", vec![], TypeId::new(1));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let obj = make_reg(0, 10);
    let x = make_reg(1, 1);

    entry.add_instr(IrInstr::NewObject {
        dest: obj.clone(),
        class: ClassId::new(0),
    });
    entry.add_instr(IrInstr::LoadField {
        dest: x.clone(),
        object: obj,
        field: 0,
    });
    entry.set_terminator(Terminator::Return(Some(x)));
    func.add_block(entry);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    let has_load_field = code.iter().any(|&b| b == Opcode::LoadField as u8);
    assert!(has_load_field, "Should emit LOAD_FIELD opcode");
}

// =============================================================================
// RETURN TESTS
// =============================================================================

#[test]
fn test_return_void() {
    let mut module = IrModule::new("test");
    let mut func = IrFunction::new("void_fn", vec![], TypeId::new(0));
    let mut entry = BasicBlock::new(BasicBlockId(0));
    entry.set_terminator(Terminator::Return(None));
    func.add_block(entry);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    // Return(None) emits ConstNull + Return
    let has_const_null = code.iter().any(|&b| b == Opcode::ConstNull as u8);
    let has_return = code.iter().any(|&b| b == Opcode::Return as u8);
    assert!(has_const_null && has_return, "Should emit CONST_NULL + RETURN");
}

#[test]
fn test_return_value() {
    let mut module = IrModule::new("test");
    let mut func = IrFunction::new("ret_val", vec![], TypeId::new(1));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let r0 = make_reg(0, 1);
    entry.add_instr(IrInstr::Assign {
        dest: r0.clone(),
        value: IrValue::Constant(IrConstant::I32(42)),
    });
    entry.set_terminator(Terminator::Return(Some(r0)));
    func.add_block(entry);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    let has_return = code.iter().any(|&b| b == Opcode::Return as u8);
    assert!(has_return, "Should emit RETURN opcode");
}

// =============================================================================
// MULTIPLE FUNCTIONS TEST
// =============================================================================

#[test]
fn test_multiple_functions() {
    let mut module = IrModule::new("test");

    // Function 1
    let mut func1 = IrFunction::new("first", vec![], TypeId::new(1));
    let mut entry1 = BasicBlock::new(BasicBlockId(0));
    let r0 = make_reg(0, 1);
    entry1.add_instr(IrInstr::Assign {
        dest: r0.clone(),
        value: IrValue::Constant(IrConstant::I32(1)),
    });
    entry1.set_terminator(Terminator::Return(Some(r0)));
    func1.add_block(entry1);
    module.add_function(func1);

    // Function 2
    let mut func2 = IrFunction::new("second", vec![], TypeId::new(1));
    let mut entry2 = BasicBlock::new(BasicBlockId(0));
    let r1 = make_reg(0, 1);
    entry2.add_instr(IrInstr::Assign {
        dest: r1.clone(),
        value: IrValue::Constant(IrConstant::I32(2)),
    });
    entry2.set_terminator(Terminator::Return(Some(r1)));
    func2.add_block(entry2);
    module.add_function(func2);

    let result = codegen::generate(&module, false).unwrap();

    assert_eq!(result.functions.len(), 2);
    assert_eq!(result.functions[0].name, "first");
    assert_eq!(result.functions[1].name, "second");
}

// =============================================================================
// CLOSURE TESTS
// =============================================================================

#[test]
fn test_make_closure() {
    let mut module = IrModule::new("test");

    // Add a function that will be wrapped in a closure
    let mut inner = IrFunction::new("inner", vec![make_reg(0, 1)], TypeId::new(1));
    let mut inner_entry = BasicBlock::new(BasicBlockId(0));
    inner_entry.set_terminator(Terminator::Return(Some(make_reg(0, 1))));
    inner.add_block(inner_entry);
    module.add_function(inner);

    // Main function that creates a closure
    let mut main = IrFunction::new("main", vec![], TypeId::new(10));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let captured = make_reg(0, 1);
    let closure = make_reg(1, 10);

    // Capture a value
    entry.add_instr(IrInstr::Assign {
        dest: captured.clone(),
        value: IrValue::Constant(IrConstant::I32(42)),
    });

    // Create closure
    entry.add_instr(IrInstr::MakeClosure {
        dest: closure.clone(),
        func: FunctionId::new(0),
        captures: vec![captured],
    });

    entry.set_terminator(Terminator::Return(Some(closure)));
    main.add_block(entry);
    module.add_function(main);

    let result = codegen::generate(&module, false).unwrap();
    let main_code = &result.functions[1].code;

    let has_make_closure = main_code.iter().any(|&b| b == Opcode::MakeClosure as u8);
    assert!(has_make_closure, "Should emit MAKE_CLOSURE opcode");
}

#[test]
fn test_load_captured() {
    let mut module = IrModule::new("test");

    // A function that loads from captured variables
    let mut func = IrFunction::new("closure_body", vec![], TypeId::new(1));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let captured_val = make_reg(0, 1);

    // Load captured variable
    entry.add_instr(IrInstr::LoadCaptured {
        dest: captured_val.clone(),
        index: 0,
    });

    entry.set_terminator(Terminator::Return(Some(captured_val)));
    func.add_block(entry);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    let has_load_captured = code.iter().any(|&b| b == Opcode::LoadCaptured as u8);
    assert!(has_load_captured, "Should emit LOAD_CAPTURED opcode");
}

#[test]
fn test_store_captured() {
    let mut module = IrModule::new("test");

    // A function that stores to captured variables
    let mut func = IrFunction::new("closure_body", vec![], TypeId::new(0));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let new_val = make_reg(0, 1);

    // Create value to store
    entry.add_instr(IrInstr::Assign {
        dest: new_val.clone(),
        value: IrValue::Constant(IrConstant::I32(100)),
    });

    // Store to captured variable
    entry.add_instr(IrInstr::StoreCaptured {
        index: 0,
        value: new_val,
    });

    entry.set_terminator(Terminator::Return(None));
    func.add_block(entry);
    module.add_function(func);

    let result = codegen::generate(&module, false).unwrap();
    let code = &result.functions[0].code;

    let has_store_captured = code.iter().any(|&b| b == Opcode::StoreCaptured as u8);
    assert!(has_store_captured, "Should emit STORE_CAPTURED opcode");
}

#[test]
fn test_closure_with_multiple_captures() {
    let mut module = IrModule::new("test");

    // Inner function
    let mut inner = IrFunction::new("inner", vec![], TypeId::new(1));
    let mut inner_entry = BasicBlock::new(BasicBlockId(0));
    let r = make_reg(0, 1);
    inner_entry.add_instr(IrInstr::Assign {
        dest: r.clone(),
        value: IrValue::Constant(IrConstant::I32(0)),
    });
    inner_entry.set_terminator(Terminator::Return(Some(r)));
    inner.add_block(inner_entry);
    module.add_function(inner);

    // Main function with multiple captures
    let mut main = IrFunction::new("main", vec![], TypeId::new(10));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let cap1 = make_reg(0, 1);
    let cap2 = make_reg(1, 1);
    let cap3 = make_reg(2, 1);
    let closure = make_reg(3, 10);

    entry.add_instr(IrInstr::Assign {
        dest: cap1.clone(),
        value: IrValue::Constant(IrConstant::I32(1)),
    });
    entry.add_instr(IrInstr::Assign {
        dest: cap2.clone(),
        value: IrValue::Constant(IrConstant::I32(2)),
    });
    entry.add_instr(IrInstr::Assign {
        dest: cap3.clone(),
        value: IrValue::Constant(IrConstant::I32(3)),
    });

    entry.add_instr(IrInstr::MakeClosure {
        dest: closure.clone(),
        func: FunctionId::new(0),
        captures: vec![cap1, cap2, cap3],
    });

    entry.set_terminator(Terminator::Return(Some(closure)));
    main.add_block(entry);
    module.add_function(main);

    let result = codegen::generate(&module, false).unwrap();
    let main_code = &result.functions[1].code;

    let has_make_closure = main_code.iter().any(|&b| b == Opcode::MakeClosure as u8);
    assert!(has_make_closure, "Should emit MAKE_CLOSURE opcode");
}

// =============================================================================
// STRING COMPARISON OPTIMIZATION TESTS
// =============================================================================

use raya_engine::compiler::ir::instr::StringCompareMode;

#[test]
fn test_string_compare_index_mode() {
    // Test that StringCompare with Index mode emits IEQ
    let mut module = IrModule::new("test");

    let mut func = IrFunction::new("compare", vec![], TypeId::new(2)); // bool type
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let s1 = make_reg(0, 3); // string type
    let s2 = make_reg(1, 3);
    let result = make_reg(2, 2); // bool type

    // Load two string literals
    entry.add_instr(IrInstr::Assign {
        dest: s1.clone(),
        value: IrValue::Constant(IrConstant::String("hello".to_string())),
    });
    entry.add_instr(IrInstr::Assign {
        dest: s2.clone(),
        value: IrValue::Constant(IrConstant::String("hello".to_string())),
    });

    // Compare with Index mode (O(1) comparison for string literals)
    entry.add_instr(IrInstr::StringCompare {
        dest: result.clone(),
        left: s1,
        right: s2,
        mode: StringCompareMode::Index,
        negate: false,
    });

    entry.set_terminator(Terminator::Return(Some(result)));
    func.add_block(entry);
    module.add_function(func);

    let module_result = codegen::generate(&module, false).unwrap();
    let code = &module_result.functions[0].code;

    // Should emit IEQ for index-based comparison
    let has_ieq = code.iter().any(|&b| b == Opcode::Ieq as u8);
    assert!(has_ieq, "StringCompare with Index mode should emit IEQ opcode");

    // Should NOT emit SEQ
    let has_seq = code.iter().any(|&b| b == Opcode::Seq as u8);
    assert!(!has_seq, "StringCompare with Index mode should NOT emit SEQ opcode");
}

#[test]
fn test_string_compare_full_mode() {
    // Test that StringCompare with Full mode emits SEQ
    let mut module = IrModule::new("test");

    let mut func = IrFunction::new("compare", vec![make_reg(0, 3)], TypeId::new(2));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let s1 = make_reg(0, 3); // parameter - computed string
    let s2 = make_reg(1, 3);
    let result = make_reg(2, 2);

    entry.add_instr(IrInstr::Assign {
        dest: s2.clone(),
        value: IrValue::Constant(IrConstant::String("test".to_string())),
    });

    // Compare with Full mode (O(n) comparison for computed strings)
    entry.add_instr(IrInstr::StringCompare {
        dest: result.clone(),
        left: s1,
        right: s2,
        mode: StringCompareMode::Full,
        negate: false,
    });

    entry.set_terminator(Terminator::Return(Some(result)));
    func.add_block(entry);
    module.add_function(func);

    let module_result = codegen::generate(&module, false).unwrap();
    let code = &module_result.functions[0].code;

    // Should emit SEQ for full string comparison
    let has_seq = code.iter().any(|&b| b == Opcode::Seq as u8);
    assert!(has_seq, "StringCompare with Full mode should emit SEQ opcode");
}

#[test]
fn test_string_compare_not_equal_index() {
    // Test that StringCompare with negate=true and Index mode emits INE
    let mut module = IrModule::new("test");

    let mut func = IrFunction::new("compare", vec![], TypeId::new(2));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let s1 = make_reg(0, 3);
    let s2 = make_reg(1, 3);
    let result = make_reg(2, 2);

    entry.add_instr(IrInstr::Assign {
        dest: s1.clone(),
        value: IrValue::Constant(IrConstant::String("a".to_string())),
    });
    entry.add_instr(IrInstr::Assign {
        dest: s2.clone(),
        value: IrValue::Constant(IrConstant::String("b".to_string())),
    });

    // Compare with Index mode and negate (!=)
    entry.add_instr(IrInstr::StringCompare {
        dest: result.clone(),
        left: s1,
        right: s2,
        mode: StringCompareMode::Index,
        negate: true,
    });

    entry.set_terminator(Terminator::Return(Some(result)));
    func.add_block(entry);
    module.add_function(func);

    let module_result = codegen::generate(&module, false).unwrap();
    let code = &module_result.functions[0].code;

    // Should emit INE for index-based not-equal comparison
    let has_ine = code.iter().any(|&b| b == Opcode::Ine as u8);
    assert!(has_ine, "StringCompare with negate=true and Index mode should emit INE opcode");
}

#[test]
fn test_string_compare_not_equal_full() {
    // Test that StringCompare with negate=true and Full mode emits SNE
    let mut module = IrModule::new("test");

    let mut func = IrFunction::new("compare", vec![make_reg(0, 3)], TypeId::new(2));
    let mut entry = BasicBlock::new(BasicBlockId(0));

    let s1 = make_reg(0, 3);
    let s2 = make_reg(1, 3);
    let result = make_reg(2, 2);

    entry.add_instr(IrInstr::Assign {
        dest: s2.clone(),
        value: IrValue::Constant(IrConstant::String("test".to_string())),
    });

    // Compare with Full mode and negate (!=)
    entry.add_instr(IrInstr::StringCompare {
        dest: result.clone(),
        left: s1,
        right: s2,
        mode: StringCompareMode::Full,
        negate: true,
    });

    entry.set_terminator(Terminator::Return(Some(result)));
    func.add_block(entry);
    module.add_function(func);

    let module_result = codegen::generate(&module, false).unwrap();
    let code = &module_result.functions[0].code;

    // Should emit SNE for full string not-equal comparison
    let has_sne = code.iter().any(|&b| b == Opcode::Sne as u8);
    assert!(has_sne, "StringCompare with negate=true and Full mode should emit SNE opcode");
}
