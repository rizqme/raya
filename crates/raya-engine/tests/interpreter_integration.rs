//! Integration tests for Basic Bytecode Interpreter (Milestone 1.5)
//!
//! Tests cover:
//! - Simple arithmetic operations
//! - Local variables
//! - Conditional branches
//! - Function calls
//! - Loops

use raya_engine::compiler::{Function, Module, Opcode};
use raya_engine::vm::value::Value;
use raya_engine::vm::vm::Vm;

#[test]
fn test_simple_arithmetic() {
    // Bytecode: 10 + 20
    // CONST_I32 10
    // CONST_I32 20
    // IADD
    // RETURN

    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            20,
            0,
            0,
            0,
            Opcode::Iadd as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();

    assert_eq!(result, Value::i32(30));
}

#[test]
fn test_local_variables() {
    // Bytecode:
    // local x = 42
    // local y = 10
    // return x + y

    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 2,
        code: vec![
            Opcode::ConstI32 as u8,
            42,
            0,
            0,
            0,
            Opcode::StoreLocal as u8,
            0, 0,
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::StoreLocal as u8,
            1, 0,
            Opcode::LoadLocal as u8,
            0, 0,
            Opcode::LoadLocal as u8,
            1, 0,
            Opcode::Iadd as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();

    assert_eq!(result, Value::i32(52));
}

#[test]
fn test_conditional_branch() {
    // Bytecode: if (10 > 5) { return 1 } else { return 0 }

    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            5,
            0,
            0,
            0,
            Opcode::Igt as u8,
            Opcode::JmpIfFalse as u8,
            8,
            0, // Skip to else branch
            Opcode::ConstI32 as u8,
            1,
            0,
            0,
            0,
            Opcode::Return as u8,
            // Else branch
            Opcode::ConstI32 as u8,
            0,
            0,
            0,
            0,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();

    assert_eq!(result, Value::i32(1));
}

#[test]
fn test_subtraction_and_multiplication() {
    // Bytecode: (100 - 50) * 2
    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            Opcode::ConstI32 as u8,
            100,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            50,
            0,
            0,
            0,
            Opcode::Isub as u8,
            Opcode::ConstI32 as u8,
            2,
            0,
            0,
            0,
            Opcode::Imul as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();

    assert_eq!(result, Value::i32(100));
}

#[test]
fn test_division_and_modulo() {
    // Bytecode: 17 / 5 (should be 3 for integer division)
    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 2,
        code: vec![
            // Store 17 / 5 in local 0
            Opcode::ConstI32 as u8,
            17,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            5,
            0,
            0,
            0,
            Opcode::Idiv as u8,
            Opcode::StoreLocal as u8,
            0, 0,
            // Store 17 % 5 in local 1
            Opcode::ConstI32 as u8,
            17,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            5,
            0,
            0,
            0,
            Opcode::Imod as u8,
            Opcode::StoreLocal as u8,
            1, 0,
            // Return local 0 + local 1 (3 + 2 = 5)
            Opcode::LoadLocal as u8,
            0, 0,
            Opcode::LoadLocal as u8,
            1, 0,
            Opcode::Iadd as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();

    assert_eq!(result, Value::i32(5)); // 3 + 2
}

#[test]
fn test_comparison_operations() {
    // Bytecode: (10 < 20) && (30 > 15)
    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            // 10 < 20
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            20,
            0,
            0,
            0,
            Opcode::Ilt as u8,
            // 30 > 15
            Opcode::ConstI32 as u8,
            30,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            15,
            0,
            0,
            0,
            Opcode::Igt as u8,
            // AND
            Opcode::And as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();

    assert_eq!(result, Value::bool(true));
}

#[test]
fn test_simple_loop() {
    // Bytecode: sum = 0; for (i = 0; i < 5; i++) { sum += i }
    let mut module = Module::new("test".to_string());

    let mut code = Vec::new();

    // sum = 0
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&0i32.to_le_bytes());
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());

    // i = 0
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&0i32.to_le_bytes());
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&1u16.to_le_bytes());

    // Loop start
    let loop_start = code.len();

    // Check: i < 5
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&1u16.to_le_bytes());
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&5i32.to_le_bytes());
    code.push(Opcode::Ilt as u8);
    code.push(Opcode::JmpIfFalse as u8);
    let jmp_if_false_offset_pos = code.len();
    code.extend_from_slice(&0i16.to_le_bytes()); // Placeholder

    // sum = sum + i
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&1u16.to_le_bytes());
    code.push(Opcode::Iadd as u8);
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());

    // i = i + 1
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&1u16.to_le_bytes());
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&1i32.to_le_bytes());
    code.push(Opcode::Iadd as u8);
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&1u16.to_le_bytes());

    // Jump back to loop start
    code.push(Opcode::Jmp as u8);
    let current_pos = code.len() + 2;
    let backward_offset = (loop_start as isize - current_pos as isize) as i16;
    code.extend_from_slice(&backward_offset.to_le_bytes());

    // Loop end - patch forward jump
    let loop_end = code.len();
    let forward_offset = (loop_end as isize - (jmp_if_false_offset_pos + 2) as isize) as i16;
    code[jmp_if_false_offset_pos..jmp_if_false_offset_pos + 2]
        .copy_from_slice(&forward_offset.to_le_bytes());

    // Return sum
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());
    code.push(Opcode::Return as u8);

    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 2, // local 0: sum, local 1: i
        code,
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();

    assert_eq!(result, Value::i32(10)); // 0 + 1 + 2 + 3 + 4 = 10
}

#[test]
fn test_equality_operations() {
    // Bytecode: (42 == 42) && (10 != 20)
    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            // 42 == 42
            Opcode::ConstI32 as u8,
            42,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            42,
            0,
            0,
            0,
            Opcode::Ieq as u8,
            // 10 != 20
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            20,
            0,
            0,
            0,
            Opcode::Ine as u8,
            // AND
            Opcode::And as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();

    assert_eq!(result, Value::bool(true));
}

#[test]
fn test_boolean_operations() {
    // Bytecode: (true || false) && (!false)
    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            // true || false
            Opcode::ConstTrue as u8,
            Opcode::ConstFalse as u8,
            Opcode::Or as u8,
            // !false
            Opcode::ConstFalse as u8,
            Opcode::Not as u8,
            // AND
            Opcode::And as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();

    assert_eq!(result, Value::bool(true));
}

#[test]
fn test_float_arithmetic() {
    // Bytecode: 3.5 + 2.5
    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            Opcode::ConstF64 as u8,
            // 3.5 as f64 bytes (little endian)
            0,
            0,
            0,
            0,
            0,
            0,
            12,
            64, // 3.5
            Opcode::ConstF64 as u8,
            // 2.5 as f64 bytes
            0,
            0,
            0,
            0,
            0,
            0,
            4,
            64, // 2.5
            Opcode::Fadd as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();

    assert_eq!(result, Value::f64(6.0));
}
