//! Integration tests for the register-based interpreter
//!
//! Tests build hand-crafted register bytecode using `RegBytecodeWriter`
//! and execute it through the VM, which auto-detects register mode.

use raya_engine::compiler::bytecode::module::{ClassDef, Method};
use raya_engine::compiler::bytecode::reg_opcode::{RegBytecodeWriter, RegOpcode};
use raya_engine::compiler::{Function, Module};
use raya_engine::vm::interpreter::Vm;
use raya_engine::vm::value::Value;

/// Helper: create a Module with a single "main" function using register bytecode
fn make_reg_module(reg_code: Vec<u32>, register_count: u16) -> Module {
    let mut module = Module::new("test".to_string());
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: Vec::new(), // empty stack code — triggers register mode
        register_count,
        reg_code,
    });
    module
}

/// Helper: execute register bytecode and return the result
fn exec_reg(reg_code: Vec<u32>, register_count: u16) -> Value {
    let module = make_reg_module(reg_code, register_count);
    let mut vm = Vm::new();
    vm.execute(&module).unwrap()
}

// ============================================================================
// Constants
// ============================================================================

#[test]
fn test_reg_load_int_return() {
    // r0 = 42; return r0
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 42);
    w.emit_abc(RegOpcode::Return, 0, 0, 0);

    let result = exec_reg(w.finish(), 1);
    assert_eq!(result, Value::i32(42));
}

#[test]
fn test_reg_load_negative_int() {
    // r0 = -10; return r0
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, -10);
    w.emit_abc(RegOpcode::Return, 0, 0, 0);

    let result = exec_reg(w.finish(), 1);
    assert_eq!(result, Value::i32(-10));
}

#[test]
fn test_reg_load_nil() {
    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::LoadNil, 0, 0, 0);
    w.emit_abc(RegOpcode::Return, 0, 0, 0);

    let result = exec_reg(w.finish(), 1);
    assert_eq!(result, Value::null());
}

#[test]
fn test_reg_load_true_false() {
    // r0 = true; return r0
    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::LoadTrue, 0, 0, 0);
    w.emit_abc(RegOpcode::Return, 0, 0, 0);

    let result = exec_reg(w.finish(), 1);
    assert_eq!(result, Value::bool(true));
}

#[test]
fn test_reg_move() {
    // r0 = 99; r1 = r0; return r1
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 99);
    w.emit_abc(RegOpcode::Move, 1, 0, 0);
    w.emit_abc(RegOpcode::Return, 1, 0, 0);

    let result = exec_reg(w.finish(), 2);
    assert_eq!(result, Value::i32(99));
}

#[test]
fn test_reg_return_void() {
    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::ReturnVoid, 0, 0, 0);

    let result = exec_reg(w.finish(), 1);
    assert_eq!(result, Value::null());
}

// ============================================================================
// Integer Arithmetic
// ============================================================================

#[test]
fn test_reg_iadd() {
    // r0 = 10; r1 = 20; r2 = r0 + r1; return r2
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 10);
    w.emit_asbx(RegOpcode::LoadInt, 1, 20);
    w.emit_abc(RegOpcode::Iadd, 2, 0, 1);
    w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let result = exec_reg(w.finish(), 3);
    assert_eq!(result, Value::i32(30));
}

#[test]
fn test_reg_isub() {
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 50);
    w.emit_asbx(RegOpcode::LoadInt, 1, 18);
    w.emit_abc(RegOpcode::Isub, 2, 0, 1);
    w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let result = exec_reg(w.finish(), 3);
    assert_eq!(result, Value::i32(32));
}

#[test]
fn test_reg_imul() {
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 6);
    w.emit_asbx(RegOpcode::LoadInt, 1, 7);
    w.emit_abc(RegOpcode::Imul, 2, 0, 1);
    w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let result = exec_reg(w.finish(), 3);
    assert_eq!(result, Value::i32(42));
}

#[test]
fn test_reg_idiv() {
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 100);
    w.emit_asbx(RegOpcode::LoadInt, 1, 4);
    w.emit_abc(RegOpcode::Idiv, 2, 0, 1);
    w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let result = exec_reg(w.finish(), 3);
    assert_eq!(result, Value::i32(25));
}

#[test]
fn test_reg_idiv_by_zero() {
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 10);
    w.emit_asbx(RegOpcode::LoadInt, 1, 0);
    w.emit_abc(RegOpcode::Idiv, 2, 0, 1);
    w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let module = make_reg_module(w.finish(), 3);
    let mut vm = Vm::new();
    let result = vm.execute(&module);
    assert!(result.is_err(), "Should fail with division by zero");
}

#[test]
fn test_reg_imod() {
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 17);
    w.emit_asbx(RegOpcode::LoadInt, 1, 5);
    w.emit_abc(RegOpcode::Imod, 2, 0, 1);
    w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let result = exec_reg(w.finish(), 3);
    assert_eq!(result, Value::i32(2));
}

#[test]
fn test_reg_ineg() {
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 42);
    w.emit_abc(RegOpcode::Ineg, 1, 0, 0);
    w.emit_abc(RegOpcode::Return, 1, 0, 0);

    let result = exec_reg(w.finish(), 2);
    assert_eq!(result, Value::i32(-42));
}

#[test]
fn test_reg_ipow() {
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 2);
    w.emit_asbx(RegOpcode::LoadInt, 1, 10);
    w.emit_abc(RegOpcode::Ipow, 2, 0, 1);
    w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let result = exec_reg(w.finish(), 3);
    assert_eq!(result, Value::i32(1024));
}

#[test]
fn test_reg_bitwise() {
    // Test AND: 0xFF & 0x0F = 0x0F (15)
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 0xFF);
    w.emit_asbx(RegOpcode::LoadInt, 1, 0x0F);
    w.emit_abc(RegOpcode::Iand, 2, 0, 1);
    w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let result = exec_reg(w.finish(), 3);
    assert_eq!(result, Value::i32(0x0F));
}

#[test]
fn test_reg_shift() {
    // 1 << 8 = 256
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 1);
    w.emit_asbx(RegOpcode::LoadInt, 1, 8);
    w.emit_abc(RegOpcode::Ishl, 2, 0, 1);
    w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let result = exec_reg(w.finish(), 3);
    assert_eq!(result, Value::i32(256));
}

// ============================================================================
// Float Arithmetic
// ============================================================================

#[test]
fn test_reg_float_add() {
    // Use constant pool for f64 values
    let mut module = Module::new("test".to_string());
    let idx_a = module.constants.add_float(3.14);
    let idx_b = module.constants.add_float(2.86);

    let mut w = RegBytecodeWriter::new();
    // LoadConst with pool_type=1 (float), index
    w.emit_abx(RegOpcode::LoadConst, 0, (1 << 14) | idx_a as u16);
    w.emit_abx(RegOpcode::LoadConst, 1, (1 << 14) | idx_b as u16);
    w.emit_abc(RegOpcode::Fadd, 2, 0, 1);
    w.emit_abc(RegOpcode::Return, 2, 0, 0);

    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: Vec::new(),
        register_count: 3,
        reg_code: w.finish(),
    });

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();
    let f = result.as_f64().unwrap();
    assert!((f - 6.0).abs() < 1e-10, "Expected ~6.0, got {}", f);
}

#[test]
fn test_reg_float_mul() {
    let mut module = Module::new("test".to_string());
    let idx_a = module.constants.add_float(2.5);
    let idx_b = module.constants.add_float(4.0);

    let mut w = RegBytecodeWriter::new();
    w.emit_abx(RegOpcode::LoadConst, 0, (1 << 14) | idx_a as u16);
    w.emit_abx(RegOpcode::LoadConst, 1, (1 << 14) | idx_b as u16);
    w.emit_abc(RegOpcode::Fmul, 2, 0, 1);
    w.emit_abc(RegOpcode::Return, 2, 0, 0);

    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: Vec::new(),
        register_count: 3,
        reg_code: w.finish(),
    });

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();
    let f = result.as_f64().unwrap();
    assert!((f - 10.0).abs() < 1e-10, "Expected 10.0, got {}", f);
}

// ============================================================================
// Integer Comparison
// ============================================================================

#[test]
fn test_reg_ieq_true() {
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 42);
    w.emit_asbx(RegOpcode::LoadInt, 1, 42);
    w.emit_abc(RegOpcode::Ieq, 2, 0, 1);
    w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let result = exec_reg(w.finish(), 3);
    assert_eq!(result, Value::bool(true));
}

#[test]
fn test_reg_ilt() {
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 5);
    w.emit_asbx(RegOpcode::LoadInt, 1, 10);
    w.emit_abc(RegOpcode::Ilt, 2, 0, 1);
    w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let result = exec_reg(w.finish(), 3);
    assert_eq!(result, Value::bool(true));
}

// ============================================================================
// Control Flow: Conditional Jumps
// ============================================================================

#[test]
fn test_reg_jmpif_taken() {
    // r0 = true; if r0 then jump +1 (skip next); r1 = 0; (skipped); r1 = 42; return r1
    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::LoadTrue, 0, 0, 0);    // 0: r0 = true
    w.emit_asbx(RegOpcode::JmpIf, 0, 1);          // 1: if r0 then ip += 1 (skip instruction 2)
    w.emit_asbx(RegOpcode::LoadInt, 1, 0);         // 2: r1 = 0 (skipped)
    w.emit_asbx(RegOpcode::LoadInt, 1, 42);        // 3: r1 = 42
    w.emit_abc(RegOpcode::Return, 1, 0, 0);        // 4: return r1

    let result = exec_reg(w.finish(), 2);
    assert_eq!(result, Value::i32(42));
}

#[test]
fn test_reg_jmpifnot_taken() {
    // r0 = false; if !r0 then jump +1 (skip next); r1 = 0; (skipped); r1 = 99; return r1
    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::LoadFalse, 0, 0, 0);    // 0: r0 = false
    w.emit_asbx(RegOpcode::JmpIfNot, 0, 1);        // 1: if !r0 then ip += 1
    w.emit_asbx(RegOpcode::LoadInt, 1, 0);          // 2: r1 = 0 (skipped)
    w.emit_asbx(RegOpcode::LoadInt, 1, 99);         // 3: r1 = 99
    w.emit_abc(RegOpcode::Return, 1, 0, 0);         // 4: return r1

    let result = exec_reg(w.finish(), 2);
    assert_eq!(result, Value::i32(99));
}

#[test]
fn test_reg_unconditional_jump() {
    // jump +2 → skip two instructions
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::Jmp, 0, 2);              // 0: jump to 3
    w.emit_asbx(RegOpcode::LoadInt, 0, 0);           // 1: skipped
    w.emit_asbx(RegOpcode::LoadInt, 0, 0);           // 2: skipped
    w.emit_asbx(RegOpcode::LoadInt, 0, 77);          // 3: r0 = 77
    w.emit_abc(RegOpcode::Return, 0, 0, 0);          // 4: return r0

    let result = exec_reg(w.finish(), 1);
    assert_eq!(result, Value::i32(77));
}

// ============================================================================
// Loop: Sum 1..10 with backward jump
// ============================================================================

#[test]
fn test_reg_loop_sum_1_to_10() {
    // r0 = sum (accumulator), r1 = i (counter), r2 = limit, r3 = temp
    //
    // r0 = 0        (sum)
    // r1 = 1        (i = 1)
    // r2 = 11       (limit, exclusive)
    // loop:
    //   r0 = r0 + r1     (sum += i)
    //   r3 = 1
    //   r1 = r1 + r3     (i += 1)
    //   r3 = (r1 < r2)   (i < 11?)
    //   if r3 then jump back to loop
    // return r0

    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 0);           // 0: r0 = 0
    w.emit_asbx(RegOpcode::LoadInt, 1, 1);           // 1: r1 = 1
    w.emit_asbx(RegOpcode::LoadInt, 2, 11);          // 2: r2 = 11
    // loop body starts at index 3
    w.emit_abc(RegOpcode::Iadd, 0, 0, 1);            // 3: r0 = r0 + r1
    w.emit_asbx(RegOpcode::LoadInt, 3, 1);           // 4: r3 = 1
    w.emit_abc(RegOpcode::Iadd, 1, 1, 3);            // 5: r1 = r1 + r3
    w.emit_abc(RegOpcode::Ilt, 3, 1, 2);             // 6: r3 = (r1 < r2)
    w.emit_asbx(RegOpcode::JmpIf, 3, -5);            // 7: if r3, jump to index 3 (ip=8, 8 + (-5) = 3)
    w.emit_abc(RegOpcode::Return, 0, 0, 0);          // 8: return r0

    let result = exec_reg(w.finish(), 4);
    assert_eq!(result, Value::i32(55)); // 1+2+...+10 = 55
}

// ============================================================================
// Logical operations
// ============================================================================

#[test]
fn test_reg_not() {
    // r0 = true; r1 = !r0; return r1
    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::LoadTrue, 0, 0, 0);
    w.emit_abc(RegOpcode::Not, 1, 0, 0);
    w.emit_abc(RegOpcode::Return, 1, 0, 0);

    let result = exec_reg(w.finish(), 2);
    assert_eq!(result, Value::bool(false));
}

#[test]
fn test_reg_and_or() {
    // r0 = true; r1 = false; r2 = r0 && r1; return r2
    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::LoadTrue, 0, 0, 0);
    w.emit_abc(RegOpcode::LoadFalse, 1, 0, 0);
    w.emit_abc(RegOpcode::And, 2, 0, 1);
    w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let result = exec_reg(w.finish(), 3);
    // And(true, false) -> false (returns rC since rB is truthy)
    assert_eq!(result, Value::bool(false));
}

// ============================================================================
// Constants from pool
// ============================================================================

#[test]
fn test_reg_load_const_integer() {
    let mut module = Module::new("test".to_string());
    let idx = module.constants.add_integer(999999);

    let mut w = RegBytecodeWriter::new();
    // pool_type=0 (int), index
    w.emit_abx(RegOpcode::LoadConst, 0, (0 << 14) | idx as u16);
    w.emit_abc(RegOpcode::Return, 0, 0, 0);

    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: Vec::new(),
        register_count: 1,
        reg_code: w.finish(),
    });

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(999999));
}

#[test]
fn test_reg_load_const_string() {
    let mut module = Module::new("test".to_string());
    let idx = module.constants.add_string("hello".to_string());

    let mut w = RegBytecodeWriter::new();
    // pool_type=2 (string), index
    w.emit_abx(RegOpcode::LoadConst, 0, (2 << 14) | idx as u16);
    // We can't easily test the string value through Value comparison,
    // so test string length instead via Slen
    w.emit_abc(RegOpcode::Slen, 1, 0, 0);
    w.emit_abc(RegOpcode::Return, 1, 0, 0);

    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: Vec::new(),
        register_count: 2,
        reg_code: w.finish(),
    });

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(5)); // "hello".len() = 5
}

// ============================================================================
// Globals
// ============================================================================

#[test]
fn test_reg_globals_store_load() {
    // r0 = 42; globals[0] = r0; r1 = globals[0]; return r1
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 42);
    w.emit_abx(RegOpcode::StoreGlobal, 0, 0);
    w.emit_abx(RegOpcode::LoadGlobal, 1, 0);
    w.emit_abc(RegOpcode::Return, 1, 0, 0);

    let result = exec_reg(w.finish(), 2);
    assert_eq!(result, Value::i32(42));
}

// ============================================================================
// Implicit return (fall off end of function)
// ============================================================================

#[test]
fn test_reg_implicit_return() {
    // Just load a value, no explicit return
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 42);
    // No return instruction

    let result = exec_reg(w.finish(), 1);
    assert_eq!(result, Value::null()); // implicit return is null
}

// ============================================================================
// Complex expressions
// ============================================================================

#[test]
fn test_reg_complex_expression() {
    // Compute: (10 + 20) * (30 - 5) = 30 * 25 = 750
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 10);
    w.emit_asbx(RegOpcode::LoadInt, 1, 20);
    w.emit_abc(RegOpcode::Iadd, 2, 0, 1);    // r2 = 30
    w.emit_asbx(RegOpcode::LoadInt, 3, 30);
    w.emit_asbx(RegOpcode::LoadInt, 4, 5);
    w.emit_abc(RegOpcode::Isub, 5, 3, 4);    // r5 = 25
    w.emit_abc(RegOpcode::Imul, 6, 2, 5);    // r6 = 750
    w.emit_abc(RegOpcode::Return, 6, 0, 0);

    let result = exec_reg(w.finish(), 7);
    assert_eq!(result, Value::i32(750));
}

#[test]
fn test_reg_if_else_pattern() {
    // if (5 > 3) { return 1 } else { return 0 }
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 5);           // 0: r0 = 5
    w.emit_asbx(RegOpcode::LoadInt, 1, 3);           // 1: r1 = 3
    w.emit_abc(RegOpcode::Igt, 2, 0, 1);             // 2: r2 = (5 > 3)
    w.emit_asbx(RegOpcode::JmpIfNot, 2, 2);           // 3: if !r2, skip to else
    // then:
    w.emit_asbx(RegOpcode::LoadInt, 3, 1);           // 4: r3 = 1
    w.emit_asbx(RegOpcode::Jmp, 0, 1);                // 5: skip else
    // else:
    w.emit_asbx(RegOpcode::LoadInt, 3, 0);           // 6: r3 = 0
    // end:
    w.emit_abc(RegOpcode::Return, 3, 0, 0);          // 7: return r3

    let result = exec_reg(w.finish(), 4);
    assert_eq!(result, Value::i32(1));
}

// ============================================================================
// Function Calls (Phase 2)
// ============================================================================

/// Helper: build a Module with multiple register-based functions
fn make_multi_func_module(funcs: Vec<(&str, Vec<u32>, u16, usize)>) -> Module {
    let mut module = Module::new("test".to_string());
    for (name, reg_code, register_count, param_count) in funcs {
        module.functions.push(Function {
            name: name.to_string(),
            param_count,
            local_count: 0,
            code: Vec::new(),
            register_count,
            reg_code,
        });
    }
    module
}

/// Helper: execute a multi-function module (entry = function 0)
fn exec_multi(funcs: Vec<(&str, Vec<u32>, u16, usize)>) -> Value {
    let module = make_multi_func_module(funcs);
    let mut vm = Vm::new();
    vm.execute(&module).unwrap()
}

#[test]
fn test_reg_simple_call() {
    // func add1(x): return x + 1   (func_id = 0)
    // main():       r0 = 41; r1 = call add1(r0); return r1  (func_id = 1)
    // Entry = func 1 (main)

    // add1: r0 = param x, r1 = 1, r2 = x+1, return r2
    let mut f0 = RegBytecodeWriter::new();
    f0.emit_asbx(RegOpcode::LoadInt, 1, 1);        // r1 = 1
    f0.emit_abc(RegOpcode::Iadd, 2, 0, 1);         // r2 = r0 + r1
    f0.emit_abc(RegOpcode::Return, 2, 0, 0);       // return r2

    // main: r0=41, call add1(r0) → r1, return r1
    let mut f1 = RegBytecodeWriter::new();
    f1.emit_asbx(RegOpcode::LoadInt, 0, 41);       // r0 = 41
    f1.emit_abcx(RegOpcode::Call, 1, 0, 1, 0);     // r1 = call func[0](r0) — 1 arg
    f1.emit_abc(RegOpcode::Return, 1, 0, 0);       // return r1

    let module = make_multi_func_module(vec![
        ("add1", f0.finish(), 3, 1),
        ("main", f1.finish(), 2, 0),
    ]);
    // Execute starting at function index 1 (main)
    module.functions.get(0).unwrap(); // add1 at index 0
    let mut vm = Vm::new();
    // We need to set entry to func 1. The VM uses entry_function (first function).
    // So let's swap the order: main at 0, add1 at 1.
    drop(module);

    // Reorder: main = func[0], add1 = func[1]
    let mut f0 = RegBytecodeWriter::new();
    f0.emit_asbx(RegOpcode::LoadInt, 0, 41);       // r0 = 41
    f0.emit_abcx(RegOpcode::Call, 1, 0, 1, 1);     // r1 = call func[1](r0) — 1 arg
    f0.emit_abc(RegOpcode::Return, 1, 0, 0);       // return r1

    // add1 at index 1
    let mut f1 = RegBytecodeWriter::new();
    f1.emit_asbx(RegOpcode::LoadInt, 1, 1);        // r1 = 1
    f1.emit_abc(RegOpcode::Iadd, 2, 0, 1);         // r2 = r0 + r1
    f1.emit_abc(RegOpcode::Return, 2, 0, 0);       // return r2

    let result = exec_multi(vec![
        ("main", f0.finish(), 2, 0),
        ("add1", f1.finish(), 3, 1),
    ]);
    assert_eq!(result, Value::i32(42));
}

#[test]
fn test_reg_call_two_args() {
    // add(a, b): return a + b
    // main(): r0=10, r1=32, r2 = call add(r0, r1); return r2

    // main at func[0]
    let mut main_w = RegBytecodeWriter::new();
    main_w.emit_asbx(RegOpcode::LoadInt, 0, 10);
    main_w.emit_asbx(RegOpcode::LoadInt, 1, 32);
    main_w.emit_abcx(RegOpcode::Call, 2, 0, 2, 1);  // r2 = call func[1](r0, r1)
    main_w.emit_abc(RegOpcode::Return, 2, 0, 0);

    // add at func[1]
    let mut add_w = RegBytecodeWriter::new();
    add_w.emit_abc(RegOpcode::Iadd, 2, 0, 1);       // r2 = r0 + r1
    add_w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let result = exec_multi(vec![
        ("main", main_w.finish(), 3, 0),
        ("add", add_w.finish(), 3, 2),
    ]);
    assert_eq!(result, Value::i32(42));
}

#[test]
fn test_reg_nested_calls() {
    // double(x): return x * 2
    // add1(x): return x + 1
    // main(): r0 = double(add1(20)); return r0
    //
    // Execution: add1(20) = 21, double(21) = 42

    // main at func[0]
    let mut main_w = RegBytecodeWriter::new();
    main_w.emit_asbx(RegOpcode::LoadInt, 0, 20);
    main_w.emit_abcx(RegOpcode::Call, 1, 0, 1, 1);  // r1 = add1(r0)
    main_w.emit_abcx(RegOpcode::Call, 2, 1, 1, 2);  // r2 = double(r1)
    main_w.emit_abc(RegOpcode::Return, 2, 0, 0);

    // add1 at func[1]
    let mut add1_w = RegBytecodeWriter::new();
    add1_w.emit_asbx(RegOpcode::LoadInt, 1, 1);
    add1_w.emit_abc(RegOpcode::Iadd, 2, 0, 1);
    add1_w.emit_abc(RegOpcode::Return, 2, 0, 0);

    // double at func[2]
    let mut double_w = RegBytecodeWriter::new();
    double_w.emit_asbx(RegOpcode::LoadInt, 1, 2);
    double_w.emit_abc(RegOpcode::Imul, 2, 0, 1);
    double_w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let result = exec_multi(vec![
        ("main", main_w.finish(), 3, 0),
        ("add1", add1_w.finish(), 3, 1),
        ("double", double_w.finish(), 3, 1),
    ]);
    assert_eq!(result, Value::i32(42));
}

#[test]
fn test_reg_recursive_factorial() {
    // factorial(n):
    //   if n <= 1: return 1
    //   return n * factorial(n - 1)
    //
    // main(): return factorial(5)  => 120

    // main at func[0]
    let mut main_w = RegBytecodeWriter::new();
    main_w.emit_asbx(RegOpcode::LoadInt, 0, 5);
    main_w.emit_abcx(RegOpcode::Call, 1, 0, 1, 1);  // r1 = factorial(r0=5)
    main_w.emit_abc(RegOpcode::Return, 1, 0, 0);

    // factorial at func[1]
    // r0 = n (param)
    // r1 = 1 (constant)
    // r2 = (n <= 1)
    // r3 = n - 1
    // r4 = factorial(n-1)
    // r5 = n * factorial(n-1)
    let mut fact_w = RegBytecodeWriter::new();
    fact_w.emit_asbx(RegOpcode::LoadInt, 1, 1);         // 0: r1 = 1
    fact_w.emit_abc(RegOpcode::Ile, 2, 0, 1);           // 1: r2 = (n <= 1)
    fact_w.emit_asbx(RegOpcode::JmpIfNot, 2, 1);        // 2: if not, skip return 1
    fact_w.emit_abc(RegOpcode::Return, 1, 0, 0);        // 3: return 1
    fact_w.emit_abc(RegOpcode::Isub, 3, 0, 1);          // 4: r3 = n - 1
    fact_w.emit_abcx(RegOpcode::Call, 4, 3, 1, 1);      // 5-6: r4 = factorial(r3)
    fact_w.emit_abc(RegOpcode::Imul, 5, 0, 4);          // 7: r5 = n * r4
    fact_w.emit_abc(RegOpcode::Return, 5, 0, 0);        // 8: return r5

    let result = exec_multi(vec![
        ("main", main_w.finish(), 2, 0),
        ("factorial", fact_w.finish(), 6, 1),
    ]);
    assert_eq!(result, Value::i32(120));
}

#[test]
fn test_reg_call_void_return() {
    // noop(): return void
    // main(): call noop(); return 99

    // main at func[0]
    let mut main_w = RegBytecodeWriter::new();
    main_w.emit_abcx(RegOpcode::Call, 0, 0, 0, 1);  // r0 = call noop()
    main_w.emit_asbx(RegOpcode::LoadInt, 1, 99);
    main_w.emit_abc(RegOpcode::Return, 1, 0, 0);

    // noop at func[1]
    let mut noop_w = RegBytecodeWriter::new();
    noop_w.emit_abc(RegOpcode::ReturnVoid, 0, 0, 0);

    let result = exec_multi(vec![
        ("main", main_w.finish(), 2, 0),
        ("noop", noop_w.finish(), 1, 0),
    ]);
    assert_eq!(result, Value::i32(99));
}

#[test]
fn test_reg_call_multiple_returns() {
    // Returns from multiple functions chain correctly
    // a(): return 10
    // b(): return 20
    // main(): r0 = a(); r1 = b(); r2 = r0 + r1; return r2

    let mut main_w = RegBytecodeWriter::new();
    main_w.emit_abcx(RegOpcode::Call, 0, 0, 0, 1);  // r0 = a()
    main_w.emit_abcx(RegOpcode::Call, 1, 0, 0, 2);  // r1 = b()
    main_w.emit_abc(RegOpcode::Iadd, 2, 0, 1);      // r2 = r0 + r1
    main_w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let mut a_w = RegBytecodeWriter::new();
    a_w.emit_asbx(RegOpcode::LoadInt, 0, 10);
    a_w.emit_abc(RegOpcode::Return, 0, 0, 0);

    let mut b_w = RegBytecodeWriter::new();
    b_w.emit_asbx(RegOpcode::LoadInt, 0, 20);
    b_w.emit_abc(RegOpcode::Return, 0, 0, 0);

    let result = exec_multi(vec![
        ("main", main_w.finish(), 3, 0),
        ("a", a_w.finish(), 1, 0),
        ("b", b_w.finish(), 1, 0),
    ]);
    assert_eq!(result, Value::i32(30));
}

#[test]
fn test_reg_fibonacci() {
    // fib(n):
    //   if n <= 1: return n
    //   return fib(n-1) + fib(n-2)
    //
    // main(): return fib(10)  => 55

    // main at func[0]
    let mut main_w = RegBytecodeWriter::new();
    main_w.emit_asbx(RegOpcode::LoadInt, 0, 10);
    main_w.emit_abcx(RegOpcode::Call, 1, 0, 1, 1);
    main_w.emit_abc(RegOpcode::Return, 1, 0, 0);

    // fib at func[1]
    // r0 = n, r1 = 1, r2 = cmp, r3 = n-1, r4 = n-2, r5 = fib(n-1), r6 = fib(n-2), r7 = result
    let mut fib_w = RegBytecodeWriter::new();
    fib_w.emit_asbx(RegOpcode::LoadInt, 1, 1);          // 0: r1 = 1
    fib_w.emit_abc(RegOpcode::Ile, 2, 0, 1);            // 1: r2 = (n <= 1)
    fib_w.emit_asbx(RegOpcode::JmpIfNot, 2, 1);         // 2: if not, skip
    fib_w.emit_abc(RegOpcode::Return, 0, 0, 0);         // 3: return n
    fib_w.emit_abc(RegOpcode::Isub, 3, 0, 1);           // 4: r3 = n - 1
    fib_w.emit_abcx(RegOpcode::Call, 5, 3, 1, 1);       // 5-6: r5 = fib(r3)
    fib_w.emit_asbx(RegOpcode::LoadInt, 4, 2);          // 7: r4 = 2
    fib_w.emit_abc(RegOpcode::Isub, 4, 0, 4);           // 8: r4 = n - 2
    fib_w.emit_abcx(RegOpcode::Call, 6, 4, 1, 1);       // 9-10: r6 = fib(r4)
    fib_w.emit_abc(RegOpcode::Iadd, 7, 5, 6);           // 11: r7 = r5 + r6
    fib_w.emit_abc(RegOpcode::Return, 7, 0, 0);         // 12: return r7

    let result = exec_multi(vec![
        ("main", main_w.finish(), 2, 0),
        ("fib", fib_w.finish(), 8, 1),
    ]);
    assert_eq!(result, Value::i32(55));
}

// ============================================================================
// Closures (Phase 2)
// ============================================================================

#[test]
fn test_reg_make_closure_and_call() {
    // closure body (func[1]): captured[0] + r0
    //   r1 = LoadCaptured 0
    //   r2 = r1 + r0
    //   return r2
    //
    // main (func[0]):
    //   r0 = 100
    //   r1 = MakeClosure(func[1], captures=[r0])  — captures r0=100
    //   r2 = 42
    //   r3 = CallClosure r1(r2)  — calls closure with arg 42
    //   return r3  → 142

    // main at func[0]
    let mut main_w = RegBytecodeWriter::new();
    main_w.emit_asbx(RegOpcode::LoadInt, 0, 100);         // r0 = 100
    main_w.emit_abcx(RegOpcode::MakeClosure, 1, 0, 1, 1); // r1 = closure(func[1], captures=[r0])
    main_w.emit_asbx(RegOpcode::LoadInt, 2, 42);          // r2 = 42
    // CallClosure: rA = rB(rB+1, ..., rB+C-1)
    // r3 = r1(r2) — but CallClosure encoding is rA=dest, rB=closure, C=arg_count
    // We want: dest=r3, closure=r1, args at r1+1..r1+C
    // But arg (r2) is not at r1+1. We need to put it there.
    // Actually, CallClosure says: rA = rB(rB+1, ..., rB+C-1)
    // So closure is at rB, args are at rB+1..rB+C-1
    // We need args right after the closure register.
    // r1 = closure, so r2 = first arg. C=1 means 1 arg at r2.
    main_w.emit_abc(RegOpcode::CallClosure, 3, 1, 1);     // r3 = r1(r2) — 1 arg
    main_w.emit_abc(RegOpcode::Return, 3, 0, 0);

    // closure body at func[1]: r0 = arg, captured[0] = 100
    let mut body_w = RegBytecodeWriter::new();
    body_w.emit_abx(RegOpcode::LoadCaptured, 1, 0);       // r1 = captured[0]
    body_w.emit_abc(RegOpcode::Iadd, 2, 1, 0);            // r2 = captured + arg
    body_w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let result = exec_multi(vec![
        ("main", main_w.finish(), 4, 0),
        ("closure_body", body_w.finish(), 3, 1),
    ]);
    assert_eq!(result, Value::i32(142));
}

#[test]
fn test_reg_closure_multiple_captures() {
    // closure body (func[1]): captured[0] + captured[1] + r0
    //
    // main:
    //   r0 = 10, r1 = 20
    //   r2 = MakeClosure(func[1], captures=[r0, r1])
    //   r3 = 12
    //   r4 = CallClosure r2(r3)
    //   return r4  → 42

    let mut main_w = RegBytecodeWriter::new();
    main_w.emit_asbx(RegOpcode::LoadInt, 0, 10);
    main_w.emit_asbx(RegOpcode::LoadInt, 1, 20);
    main_w.emit_abcx(RegOpcode::MakeClosure, 2, 0, 2, 1); // r2 = closure(func[1], [r0, r1])
    main_w.emit_asbx(RegOpcode::LoadInt, 3, 12);
    main_w.emit_abc(RegOpcode::CallClosure, 4, 2, 1);      // r4 = r2(r3)
    main_w.emit_abc(RegOpcode::Return, 4, 0, 0);

    // closure body at func[1]
    let mut body_w = RegBytecodeWriter::new();
    body_w.emit_abx(RegOpcode::LoadCaptured, 1, 0);       // r1 = captured[0] (10)
    body_w.emit_abx(RegOpcode::LoadCaptured, 2, 1);       // r2 = captured[1] (20)
    body_w.emit_abc(RegOpcode::Iadd, 3, 1, 2);            // r3 = 10 + 20 = 30
    body_w.emit_abc(RegOpcode::Iadd, 4, 3, 0);            // r4 = 30 + arg (12) = 42
    body_w.emit_abc(RegOpcode::Return, 4, 0, 0);

    let result = exec_multi(vec![
        ("main", main_w.finish(), 5, 0),
        ("closure_body", body_w.finish(), 5, 1),
    ]);
    assert_eq!(result, Value::i32(42));
}

#[test]
fn test_reg_store_captured() {
    // Closure that mutates a captured variable
    // closure body (func[1]):
    //   r1 = LoadCaptured 0     — load counter
    //   r2 = r1 + r0           — add arg
    //   StoreCaptured 0 = r2   — update counter
    //   return r2
    //
    // main:
    //   r0 = 0 (initial counter)
    //   r1 = MakeClosure(func[1], [r0])
    //   r2 = 10
    //   r3 = CallClosure r1(r2)  — returns 10, counter = 10
    //   r4 = 20
    //   r5 = CallClosure r1(r4)  — returns 30, counter = 30
    //   return r5  → 30

    let mut main_w = RegBytecodeWriter::new();
    main_w.emit_asbx(RegOpcode::LoadInt, 0, 0);
    main_w.emit_abcx(RegOpcode::MakeClosure, 1, 0, 1, 1);
    main_w.emit_asbx(RegOpcode::LoadInt, 2, 10);
    main_w.emit_abc(RegOpcode::CallClosure, 3, 1, 1);      // r3 = r1(r2=10)
    main_w.emit_asbx(RegOpcode::LoadInt, 4, 20);
    // Need to put arg right after closure for CallClosure
    // r1 is closure, so r2 is arg slot. But r2 still has 10...
    // Actually, we already have r4=20. We need r4 right after r1.
    // Let's use Move to place arg correctly.
    main_w.emit_abc(RegOpcode::Move, 2, 4, 0);             // r2 = r4 = 20
    main_w.emit_abc(RegOpcode::CallClosure, 5, 1, 1);      // r5 = r1(r2=20)
    main_w.emit_abc(RegOpcode::Return, 5, 0, 0);

    // closure body at func[1]
    let mut body_w = RegBytecodeWriter::new();
    body_w.emit_abx(RegOpcode::LoadCaptured, 1, 0);       // r1 = captured[0]
    body_w.emit_abc(RegOpcode::Iadd, 2, 1, 0);            // r2 = captured + arg
    body_w.emit_abx(RegOpcode::StoreCaptured, 2, 0);      // captured[0] = r2
    body_w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let result = exec_multi(vec![
        ("main", main_w.finish(), 6, 0),
        ("closure_body", body_w.finish(), 3, 1),
    ]);
    assert_eq!(result, Value::i32(30));
}

#[test]
fn test_reg_refcell() {
    // Test NewRefCell, LoadRefCell, StoreRefCell
    //
    // r0 = 10
    // r1 = NewRefCell(r0)     — RefCell containing 10
    // r2 = LoadRefCell(r1)    — r2 = 10
    // r3 = 5
    // r4 = r2 + r3            — r4 = 15
    // StoreRefCell r1, r4     — RefCell now contains 15
    // r5 = LoadRefCell(r1)    — r5 = 15
    // return r5

    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 10);
    w.emit_abc(RegOpcode::NewRefCell, 1, 0, 0);       // r1 = RefCell(10)
    w.emit_abc(RegOpcode::LoadRefCell, 2, 1, 0);      // r2 = r1.value = 10
    w.emit_asbx(RegOpcode::LoadInt, 3, 5);
    w.emit_abc(RegOpcode::Iadd, 4, 2, 3);             // r4 = 15
    w.emit_abc(RegOpcode::StoreRefCell, 1, 4, 0);     // r1.value = 15
    w.emit_abc(RegOpcode::LoadRefCell, 5, 1, 0);      // r5 = r1.value = 15
    w.emit_abc(RegOpcode::Return, 5, 0, 0);

    let result = exec_reg(w.finish(), 6);
    assert_eq!(result, Value::i32(15));
}

#[test]
fn test_reg_closure_with_refcell() {
    // Pattern: shared mutable state via RefCell
    //
    // increment body (func[1]):
    //   r1 = LoadCaptured 0          — get refcell from captures
    //   r2 = LoadRefCell r1          — load current value
    //   r3 = r2 + r0                 — add increment amount
    //   StoreRefCell r1, r3          — store back
    //   return r3
    //
    // main:
    //   r0 = 0
    //   r1 = NewRefCell(r0)          — refcell containing 0
    //   r2 = MakeClosure(func[1], [r1])  — capture the refcell
    //   r3 = 10
    //   r4 = CallClosure r2(r3)      — increment by 10, returns 10
    //   r5 = 20
    //   Move r3, r5                  — put arg right after closure
    //   r6 = CallClosure r2(r3)      — increment by 20, returns 30
    //   return r6

    let mut main_w = RegBytecodeWriter::new();
    main_w.emit_asbx(RegOpcode::LoadInt, 0, 0);
    main_w.emit_abc(RegOpcode::NewRefCell, 1, 0, 0);          // r1 = RefCell(0)
    main_w.emit_abcx(RegOpcode::MakeClosure, 2, 1, 1, 1);     // r2 = closure(func[1], [r1])
    main_w.emit_asbx(RegOpcode::LoadInt, 3, 10);
    main_w.emit_abc(RegOpcode::CallClosure, 4, 2, 1);          // r4 = r2(r3=10) → 10
    main_w.emit_asbx(RegOpcode::LoadInt, 3, 20);               // r3 = 20
    main_w.emit_abc(RegOpcode::CallClosure, 6, 2, 1);          // r6 = r2(r3=20) → 30
    main_w.emit_abc(RegOpcode::Return, 6, 0, 0);

    // increment body at func[1]
    let mut body_w = RegBytecodeWriter::new();
    body_w.emit_abx(RegOpcode::LoadCaptured, 1, 0);           // r1 = captured[0] (refcell)
    body_w.emit_abc(RegOpcode::LoadRefCell, 2, 1, 0);         // r2 = refcell.value
    body_w.emit_abc(RegOpcode::Iadd, 3, 2, 0);                // r3 = value + arg
    body_w.emit_abc(RegOpcode::StoreRefCell, 1, 3, 0);        // refcell.value = r3
    body_w.emit_abc(RegOpcode::Return, 3, 0, 0);

    let result = exec_multi(vec![
        ("main", main_w.finish(), 7, 0),
        ("increment", body_w.finish(), 4, 1),
    ]);
    assert_eq!(result, Value::i32(30));
}

#[test]
fn test_reg_call_static() {
    // CallStatic is essentially the same as Call — dispatches by func_id
    // static_add(a, b): return a + b
    // main(): r0=7, r1=35; r2 = CallStatic add(r0, r1); return r2

    let mut main_w = RegBytecodeWriter::new();
    main_w.emit_asbx(RegOpcode::LoadInt, 0, 7);
    main_w.emit_asbx(RegOpcode::LoadInt, 1, 35);
    main_w.emit_abcx(RegOpcode::CallStatic, 2, 0, 2, 1);   // r2 = static func[1](r0, r1)
    main_w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let mut add_w = RegBytecodeWriter::new();
    add_w.emit_abc(RegOpcode::Iadd, 2, 0, 1);
    add_w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let result = exec_multi(vec![
        ("main", main_w.finish(), 3, 0),
        ("static_add", add_w.finish(), 3, 2),
    ]);
    assert_eq!(result, Value::i32(42));
}

#[test]
fn test_reg_set_closure_capture() {
    // SetClosureCapture modifies a closure's capture array
    //
    // closure body (func[1]): return captured[0]
    //
    // main:
    //   r0 = 10
    //   r1 = MakeClosure(func[1], [r0])  — captures[0] = 10
    //   r2 = 42
    //   SetClosureCapture r1, 0, r2      — closure.captures[0] = 42
    //   r3 = CallClosure r1()
    //   return r3  → 42

    let mut main_w = RegBytecodeWriter::new();
    main_w.emit_asbx(RegOpcode::LoadInt, 0, 10);
    main_w.emit_abcx(RegOpcode::MakeClosure, 1, 0, 1, 1);   // r1 = closure([r0=10])
    main_w.emit_asbx(RegOpcode::LoadInt, 2, 42);
    main_w.emit_abc(RegOpcode::SetClosureCapture, 1, 0, 2);  // r1.captures[0] = r2(42)
    main_w.emit_abc(RegOpcode::CallClosure, 3, 1, 0);        // r3 = r1() — 0 args
    main_w.emit_abc(RegOpcode::Return, 3, 0, 0);

    // closure body at func[1]: just return captured[0]
    let mut body_w = RegBytecodeWriter::new();
    body_w.emit_abx(RegOpcode::LoadCaptured, 0, 0);
    body_w.emit_abc(RegOpcode::Return, 0, 0, 0);

    let result = exec_multi(vec![
        ("main", main_w.finish(), 4, 0),
        ("closure_body", body_w.finish(), 1, 0),
    ]);
    assert_eq!(result, Value::i32(42));
}

#[test]
fn test_reg_adder_factory() {
    // Higher-order function: adder factory
    //
    // make_adder body (func[1]):
    //   r1 = MakeClosure(func[2], captures=[r0])  — capture n
    //   return r1
    //
    // adder body (func[2]):
    //   r1 = LoadCaptured 0      — n
    //   r2 = r1 + r0             — n + x
    //   return r2
    //
    // main:
    //   r0 = 40
    //   r1 = call make_adder(r0)  → closure that adds 40
    //   r2 = 2
    //   r3 = CallClosure r1(r2)   → 42
    //   return r3

    // main at func[0]
    let mut main_w = RegBytecodeWriter::new();
    main_w.emit_asbx(RegOpcode::LoadInt, 0, 40);
    main_w.emit_abcx(RegOpcode::Call, 1, 0, 1, 1);            // r1 = make_adder(40)
    main_w.emit_asbx(RegOpcode::LoadInt, 2, 2);
    main_w.emit_abc(RegOpcode::CallClosure, 3, 1, 1);          // r3 = r1(2) = 42
    main_w.emit_abc(RegOpcode::Return, 3, 0, 0);

    // make_adder at func[1]
    let mut maker_w = RegBytecodeWriter::new();
    maker_w.emit_abcx(RegOpcode::MakeClosure, 1, 0, 1, 2);    // r1 = closure(func[2], [r0])
    maker_w.emit_abc(RegOpcode::Return, 1, 0, 0);

    // adder body at func[2]
    let mut adder_w = RegBytecodeWriter::new();
    adder_w.emit_abx(RegOpcode::LoadCaptured, 1, 0);           // r1 = captured[0]
    adder_w.emit_abc(RegOpcode::Iadd, 2, 1, 0);                // r2 = n + x
    adder_w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let result = exec_multi(vec![
        ("main", main_w.finish(), 4, 0),
        ("make_adder", maker_w.finish(), 2, 1),
        ("adder", adder_w.finish(), 3, 1),
    ]);
    assert_eq!(result, Value::i32(42));
}

// ============================================================================
// Objects (Phase 3)
// ============================================================================

/// Helper: build a Module with register functions and class definitions
fn make_module_with_classes(
    funcs: Vec<(&str, Vec<u32>, u16, usize)>,
    classes: Vec<ClassDef>,
) -> Module {
    let mut module = Module::new("test".to_string());
    for (name, reg_code, register_count, param_count) in funcs {
        module.functions.push(Function {
            name: name.to_string(),
            param_count,
            local_count: 0,
            code: Vec::new(),
            register_count,
            reg_code,
        });
    }
    module.classes = classes;
    module
}

#[test]
fn test_reg_new_object_and_fields() {
    // class Point { x: int; y: int }  → class_id=0, 2 fields
    //
    // main:
    //   r0 = New(class_id=0)
    //   r1 = 10
    //   StoreField r0.field[0] = r1  (x = 10)
    //   r2 = 20
    //   StoreField r0.field[1] = r2  (y = 20)
    //   r3 = LoadField r0.field[0]   (x)
    //   r4 = LoadField r0.field[1]   (y)
    //   r5 = r3 + r4
    //   return r5  → 30

    let mut w = RegBytecodeWriter::new();
    w.emit_abcx(RegOpcode::New, 0, 0, 0, 0);           // r0 = new class[0]
    w.emit_asbx(RegOpcode::LoadInt, 1, 10);
    w.emit_abc(RegOpcode::StoreField, 0, 0, 1);         // r0.field[0] = r1
    w.emit_asbx(RegOpcode::LoadInt, 2, 20);
    w.emit_abc(RegOpcode::StoreField, 0, 1, 2);         // r0.field[1] = r2
    w.emit_abc(RegOpcode::LoadField, 3, 0, 0);          // r3 = r0.field[0]
    w.emit_abc(RegOpcode::LoadField, 4, 0, 1);          // r4 = r0.field[1]
    w.emit_abc(RegOpcode::Iadd, 5, 3, 4);
    w.emit_abc(RegOpcode::Return, 5, 0, 0);

    let classes = vec![ClassDef {
        name: "Point".to_string(),
        field_count: 2,
        parent_id: None,
        methods: vec![],
    }];

    let module = make_module_with_classes(
        vec![("main", w.finish(), 6, 0)],
        classes,
    );
    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(30));
}

#[test]
fn test_reg_object_literal() {
    // ObjectLiteral creates object with fields from registers
    //
    // main:
    //   r0 = 3
    //   r1 = 7
    //   r2 = ObjectLiteral(class_id=0, fields=[r0, r1])
    //   r3 = LoadField r2.field[0]  → 3
    //   r4 = LoadField r2.field[1]  → 7
    //   r5 = r3 * r4
    //   return r5  → 21

    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 3);
    w.emit_asbx(RegOpcode::LoadInt, 1, 7);
    w.emit_abcx(RegOpcode::ObjectLiteral, 2, 0, 2, 0);  // r2 = { r0, r1 } class[0]
    w.emit_abc(RegOpcode::LoadField, 3, 2, 0);
    w.emit_abc(RegOpcode::LoadField, 4, 2, 1);
    w.emit_abc(RegOpcode::Imul, 5, 3, 4);
    w.emit_abc(RegOpcode::Return, 5, 0, 0);

    let classes = vec![ClassDef {
        name: "Pair".to_string(),
        field_count: 2,
        parent_id: None,
        methods: vec![],
    }];

    let module = make_module_with_classes(
        vec![("main", w.finish(), 6, 0)],
        classes,
    );
    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(21));
}

#[test]
fn test_reg_optional_field_null() {
    // OptionalField on null → null
    //
    // main:
    //   r0 = null
    //   r1 = r0?.field[0]
    //   r2 = (r1 == null)? 1 : 0
    //   return r2  → 1 (since r1 is null)

    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::LoadNil, 0, 0, 0);
    w.emit_abc(RegOpcode::OptionalField, 1, 0, 0);
    w.emit_asbx(RegOpcode::JmpIfNotNull, 1, 2);  // if not null, jump +2
    w.emit_asbx(RegOpcode::LoadInt, 2, 1);        // r2 = 1 (was null)
    w.emit_asbx(RegOpcode::Jmp, 0, 1);            // skip else
    w.emit_asbx(RegOpcode::LoadInt, 2, 0);        // r2 = 0 (not null)
    w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let result = exec_reg(w.finish(), 3);
    assert_eq!(result, Value::i32(1));
}

#[test]
fn test_reg_optional_field_object() {
    // OptionalField on a real object reads the field normally
    //
    // main:
    //   r0 = New(class[0])   — 1 field
    //   r1 = 42
    //   StoreField r0.field[0] = r1
    //   r2 = r0?.field[0]    — should be 42
    //   return r2

    let mut w = RegBytecodeWriter::new();
    w.emit_abcx(RegOpcode::New, 0, 0, 0, 0);
    w.emit_asbx(RegOpcode::LoadInt, 1, 42);
    w.emit_abc(RegOpcode::StoreField, 0, 0, 1);
    w.emit_abc(RegOpcode::OptionalField, 2, 0, 0);
    w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let classes = vec![ClassDef {
        name: "Box".to_string(),
        field_count: 1,
        parent_id: None,
        methods: vec![],
    }];

    let module = make_module_with_classes(
        vec![("main", w.finish(), 3, 0)],
        classes,
    );
    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(42));
}

#[test]
fn test_reg_call_method_vtable() {
    // class Foo { getValue(): int }
    //   getValue body (func[1]): return 42
    //
    // main:
    //   r0 = New(class[0])
    //   r1 = CallMethod r0.method[0]()
    //   return r1

    // main at func[0]
    let mut main_w = RegBytecodeWriter::new();
    main_w.emit_abcx(RegOpcode::New, 0, 0, 0, 0);         // r0 = new Foo()
    main_w.emit_abcx(RegOpcode::CallMethod, 1, 0, 1, 0);   // r1 = r0.method[0]() — 1 arg (receiver)
    main_w.emit_abc(RegOpcode::Return, 1, 0, 0);

    // getValue at func[1]
    let mut method_w = RegBytecodeWriter::new();
    // r0 = this (receiver), we ignore it and return 42
    method_w.emit_asbx(RegOpcode::LoadInt, 1, 42);
    method_w.emit_abc(RegOpcode::Return, 1, 0, 0);

    let classes = vec![ClassDef {
        name: "Foo".to_string(),
        field_count: 0,
        parent_id: None,
        methods: vec![Method {
            name: "getValue".to_string(),
            function_id: 1,
            slot: 0,
        }],
    }];

    let module = make_module_with_classes(
        vec![
            ("main", main_w.finish(), 2, 0),
            ("getValue", method_w.finish(), 2, 1),
        ],
        classes,
    );
    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(42));
}

#[test]
fn test_reg_method_with_field_access() {
    // class Counter { count: int; getCount(): int { return this.count } }
    //
    // main:
    //   r0 = New(class[0])      — Counter with 1 field
    //   r1 = 99
    //   StoreField r0.field[0] = r1  (count = 99)
    //   r2 = CallMethod r0.method[0]()  → getCount() → 99
    //   return r2

    let mut main_w = RegBytecodeWriter::new();
    main_w.emit_abcx(RegOpcode::New, 0, 0, 0, 0);
    main_w.emit_asbx(RegOpcode::LoadInt, 1, 99);
    main_w.emit_abc(RegOpcode::StoreField, 0, 0, 1);         // r0.field[0] = r1
    main_w.emit_abcx(RegOpcode::CallMethod, 2, 0, 1, 0);     // r2 = r0.method[0]()
    main_w.emit_abc(RegOpcode::Return, 2, 0, 0);

    // getCount: r0 = this, return this.field[0]
    let mut get_w = RegBytecodeWriter::new();
    get_w.emit_abc(RegOpcode::LoadField, 1, 0, 0);  // r1 = this.field[0]
    get_w.emit_abc(RegOpcode::Return, 1, 0, 0);

    let classes = vec![ClassDef {
        name: "Counter".to_string(),
        field_count: 1,
        parent_id: None,
        methods: vec![Method {
            name: "getCount".to_string(),
            function_id: 1,
            slot: 0,
        }],
    }];

    let module = make_module_with_classes(
        vec![
            ("main", main_w.finish(), 3, 0),
            ("getCount", get_w.finish(), 2, 1),
        ],
        classes,
    );
    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(99));
}

// ============================================================================
// Arrays (Phase 3)
// ============================================================================

#[test]
fn test_reg_array_literal() {
    // r0 = 10, r1 = 20, r2 = 30
    // r3 = ArrayLiteral([r0, r1, r2], type_id=0)
    // r4 = ArrayLen(r3)
    // return r4  → 3

    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 10);
    w.emit_asbx(RegOpcode::LoadInt, 1, 20);
    w.emit_asbx(RegOpcode::LoadInt, 2, 30);
    w.emit_abcx(RegOpcode::ArrayLiteral, 3, 0, 3, 0);   // r3 = [r0, r1, r2]
    w.emit_abc(RegOpcode::ArrayLen, 4, 3, 0);             // r4 = r3.length
    w.emit_abc(RegOpcode::Return, 4, 0, 0);

    let result = exec_reg(w.finish(), 5);
    assert_eq!(result, Value::i32(3));
}

#[test]
fn test_reg_array_load_elem() {
    // r0 = 10, r1 = 20, r2 = 30
    // r3 = ArrayLiteral([r0, r1, r2])
    // r4 = 1 (index)
    // r5 = r3[r4]
    // return r5  → 20

    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 10);
    w.emit_asbx(RegOpcode::LoadInt, 1, 20);
    w.emit_asbx(RegOpcode::LoadInt, 2, 30);
    w.emit_abcx(RegOpcode::ArrayLiteral, 3, 0, 3, 0);
    w.emit_asbx(RegOpcode::LoadInt, 4, 1);
    w.emit_abc(RegOpcode::LoadElem, 5, 3, 4);
    w.emit_abc(RegOpcode::Return, 5, 0, 0);

    let result = exec_reg(w.finish(), 6);
    assert_eq!(result, Value::i32(20));
}

#[test]
fn test_reg_array_store_elem() {
    // r0 = 0, r1 = 0
    // r2 = ArrayLiteral([r0, r1])   — [0, 0]
    // r3 = 1 (index)
    // r4 = 42
    // StoreElem r2[r3] = r4         — [0, 42]
    // r5 = LoadElem r2[r3]
    // return r5  → 42

    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 0);
    w.emit_asbx(RegOpcode::LoadInt, 1, 0);
    w.emit_abcx(RegOpcode::ArrayLiteral, 2, 0, 2, 0);
    w.emit_asbx(RegOpcode::LoadInt, 3, 1);
    w.emit_asbx(RegOpcode::LoadInt, 4, 42);
    w.emit_abc(RegOpcode::StoreElem, 2, 3, 4);
    w.emit_abc(RegOpcode::LoadElem, 5, 2, 3);
    w.emit_abc(RegOpcode::Return, 5, 0, 0);

    let result = exec_reg(w.finish(), 6);
    assert_eq!(result, Value::i32(42));
}

#[test]
fn test_reg_array_push_pop() {
    // r0 = 0
    // r1 = NewArray(r0, type=0)    — empty array (length 0)
    // r2 = 10
    // ArrayPush r1, r2             — [10]
    // r3 = 20
    // ArrayPush r1, r3             — [10, 20]
    // r4 = 30
    // ArrayPush r1, r4             — [10, 20, 30]
    // r5 = ArrayPop r1             — [10, 20], r5=30
    // r6 = ArrayLen r1             — 2
    // r7 = r5 + r6                 — 32
    // return r7

    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 0);
    w.emit_abcx(RegOpcode::NewArray, 1, 0, 0, 0);
    w.emit_asbx(RegOpcode::LoadInt, 2, 10);
    w.emit_abc(RegOpcode::ArrayPush, 1, 2, 0);
    w.emit_asbx(RegOpcode::LoadInt, 3, 20);
    w.emit_abc(RegOpcode::ArrayPush, 1, 3, 0);
    w.emit_asbx(RegOpcode::LoadInt, 4, 30);
    w.emit_abc(RegOpcode::ArrayPush, 1, 4, 0);
    w.emit_abc(RegOpcode::ArrayPop, 5, 1, 0);
    w.emit_abc(RegOpcode::ArrayLen, 6, 1, 0);
    w.emit_abc(RegOpcode::Iadd, 7, 5, 6);
    w.emit_abc(RegOpcode::Return, 7, 0, 0);

    let result = exec_reg(w.finish(), 8);
    assert_eq!(result, Value::i32(32));
}

#[test]
fn test_reg_new_array() {
    // NewArray with initial size
    //
    // r0 = 3
    // r1 = NewArray(r0, type=0)  — array of length 3 (all null)
    // r2 = ArrayLen r1
    // return r2  → 3

    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 3);
    w.emit_abcx(RegOpcode::NewArray, 1, 0, 0, 0);
    w.emit_abc(RegOpcode::ArrayLen, 2, 1, 0);
    w.emit_abc(RegOpcode::Return, 2, 0, 0);

    let result = exec_reg(w.finish(), 3);
    assert_eq!(result, Value::i32(3));
}

#[test]
fn test_reg_tuple_literal_and_get() {
    // r0 = 10, r1 = 20, r2 = 30
    // r3 = TupleLiteral(r0, r1, r2)
    // r4 = TupleGet r3[0]  → 10
    // r5 = TupleGet r3[2]  → 30
    // r6 = r4 + r5
    // return r6  → 40

    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 10);
    w.emit_asbx(RegOpcode::LoadInt, 1, 20);
    w.emit_asbx(RegOpcode::LoadInt, 2, 30);
    w.emit_abcx(RegOpcode::TupleLiteral, 3, 0, 3, 0);
    w.emit_abc(RegOpcode::TupleGet, 4, 3, 0);
    w.emit_abc(RegOpcode::TupleGet, 5, 3, 2);
    w.emit_abc(RegOpcode::Iadd, 6, 4, 5);
    w.emit_abc(RegOpcode::Return, 6, 0, 0);

    let result = exec_reg(w.finish(), 7);
    assert_eq!(result, Value::i32(40));
}

#[test]
fn test_reg_array_sum_loop() {
    // Sum elements of an array using a loop
    //
    // r0=1, r1=2, r2=3, r3=4, r4=5
    // r5 = ArrayLiteral([r0..r4])   — [1,2,3,4,5]
    // r6 = ArrayLen(r5)             — 5
    // r7 = 0 (sum)
    // r8 = 0 (i)
    // loop:
    //   if r8 >= r6: break
    //   r9 = r5[r8]
    //   r7 = r7 + r9
    //   r8 = r8 + 1
    //   jmp loop
    // return r7  → 15

    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 1);
    w.emit_asbx(RegOpcode::LoadInt, 1, 2);
    w.emit_asbx(RegOpcode::LoadInt, 2, 3);
    w.emit_asbx(RegOpcode::LoadInt, 3, 4);
    w.emit_asbx(RegOpcode::LoadInt, 4, 5);
    w.emit_abcx(RegOpcode::ArrayLiteral, 5, 0, 5, 0);   // 5-6: r5 = [1,2,3,4,5]
    w.emit_abc(RegOpcode::ArrayLen, 6, 5, 0);             // 7: r6 = 5
    w.emit_asbx(RegOpcode::LoadInt, 7, 0);                // 8: r7 = 0 (sum)
    w.emit_asbx(RegOpcode::LoadInt, 8, 0);                // 9: r8 = 0 (i)
    // loop (ip=10):
    w.emit_abc(RegOpcode::Ige, 10, 8, 6);                 // 10: r10 = (i >= len)
    w.emit_asbx(RegOpcode::JmpIf, 10, 5);                 // 11: if true, break → ip=17
    w.emit_abc(RegOpcode::LoadElem, 9, 5, 8);             // 12: r9 = arr[i]
    w.emit_abc(RegOpcode::Iadd, 7, 7, 9);                 // 13: sum += r9
    w.emit_asbx(RegOpcode::LoadInt, 11, 1);               // 14: r11 = 1
    w.emit_abc(RegOpcode::Iadd, 8, 8, 11);                // 15: i++
    w.emit_asbx(RegOpcode::Jmp, 0, -7);                   // 16: jmp to ip=10
    // after loop (ip=17):
    w.emit_abc(RegOpcode::Return, 7, 0, 0);               // 17: return sum

    let result = exec_reg(w.finish(), 12);
    assert_eq!(result, Value::i32(15));
}

// ============================================================================
// Phase 4: Exception Handling
// ============================================================================

#[test]
fn test_reg_try_catch_no_throw() {
    // try { r0 = 42 } catch(e) { r0 = -1 }; return r0
    //
    // ip 0: Try(catch_reg=1, extra = catch_ip=4 << 16 | 0xFFFF)
    // ip 2: LoadInt r0, 42
    // ip 3: EndTry
    // ip 4: Jmp +2 → ip 6 (skip catch)
    //   catch (ip=5):
    // ip 5: LoadInt r0, -1
    //   end (ip=6):
    // ip 6: Return r0
    //
    // Note: Try is extended (ABCx), so ip 0 = Try instr, ip 1 = extra word.
    // After EndTry, we skip over the catch block.
    let mut w = RegBytecodeWriter::new();
    // Try: A=1 (catch_reg), extra = (4 << 16) | 0xFFFF (catch at ip=4, no finally)
    // But wait - ip 4 would be AFTER the Jmp that skips catch.
    // Let me re-layout:
    //
    // ip 0: Try (A=1, extra = catch_ip=5 high, finally=0xFFFF low)   [2 words]
    // ip 2: LoadInt r0, 42                                            [1 word]
    // ip 3: EndTry                                                    [1 word]
    // ip 4: Jmp +2 → ip 6 (skip catch block)                         [1 word]
    // ip 5: LoadInt r0, -1   ← catch block                           [1 word]
    // ip 6: Return r0                                                 [1 word]
    let catch_ip = 5u32;
    let finally_ip = 0xFFFFu32;
    let try_extra = (catch_ip << 16) | finally_ip;
    w.emit_abcx(RegOpcode::Try, 1, 0, 0, try_extra);    // ip 0-1
    w.emit_asbx(RegOpcode::LoadInt, 0, 42);              // ip 2
    w.emit_abc(RegOpcode::EndTry, 0, 0, 0);              // ip 3
    w.emit_asbx(RegOpcode::Jmp, 0, 1);                   // ip 4: jmp to ip 6
    w.emit_asbx(RegOpcode::LoadInt, 0, -1);              // ip 5: catch
    w.emit_abc(RegOpcode::Return, 0, 0, 0);              // ip 6

    let result = exec_reg(w.finish(), 2);
    assert_eq!(result, Value::i32(42)); // no throw, catch not entered
}

#[test]
fn test_reg_throw_and_catch() {
    // try { throw 99 } catch(e) { return e }
    // The Throw opcode reads from a register. We throw a simple value.
    // The exception handler writes the exception to the catch_reg.
    //
    // But our Throw handler creates a RuntimeError string, not the raw value.
    // The exception value set on the task IS the raw value from the register.
    // So catch_reg should receive that value.
    //
    // ip 0-1: Try (catch_reg=1, catch_ip=5, no finally)
    // ip 2: LoadInt r0, 99
    // ip 3: Throw r0           → triggers error, handler catches
    // ip 4: Jmp +2 → ip 6     (skip catch, never reached)
    // ip 5: Return r1          ← catch block (r1 = exception)
    // ip 6: Return r0          (fallthrough, never reached)
    let catch_ip = 5u32;
    let try_extra = (catch_ip << 16) | 0xFFFF;
    let mut w = RegBytecodeWriter::new();
    w.emit_abcx(RegOpcode::Try, 1, 0, 0, try_extra);    // ip 0-1
    w.emit_asbx(RegOpcode::LoadInt, 0, 99);              // ip 2
    w.emit_abc(RegOpcode::Throw, 0, 0, 0);               // ip 3
    w.emit_asbx(RegOpcode::Jmp, 0, 1);                   // ip 4
    w.emit_abc(RegOpcode::Return, 1, 0, 0);              // ip 5: catch
    w.emit_abc(RegOpcode::Return, 0, 0, 0);              // ip 6

    let result = exec_reg(w.finish(), 2);
    // The caught exception is the raw value (i32 99) that was set on the task
    assert_eq!(result, Value::i32(99));
}

#[test]
fn test_reg_try_catch_with_recovery() {
    // try { throw 0 } catch(e) { r0 = 100 }; return r0
    //
    // ip 0-1: Try (catch_reg=2, catch_ip=5, no finally)
    // ip 2: LoadInt r0, 0
    // ip 3: Throw r0
    // ip 4: Jmp +2 → ip 6     (skip catch)
    // ip 5: LoadInt r0, 100    ← catch block
    // ip 6: Return r0
    let catch_ip = 5u32;
    let try_extra = (catch_ip << 16) | 0xFFFF;
    let mut w = RegBytecodeWriter::new();
    w.emit_abcx(RegOpcode::Try, 2, 0, 0, try_extra);
    w.emit_asbx(RegOpcode::LoadInt, 0, 0);
    w.emit_abc(RegOpcode::Throw, 0, 0, 0);
    w.emit_asbx(RegOpcode::Jmp, 0, 1);
    w.emit_asbx(RegOpcode::LoadInt, 0, 100);             // catch: set r0 = 100
    w.emit_abc(RegOpcode::Return, 0, 0, 0);

    let result = exec_reg(w.finish(), 3);
    assert_eq!(result, Value::i32(100));
}

#[test]
fn test_reg_nested_try_catch() {
    // try {
    //   try { throw 1 } catch(e1) { r0 = e1 + 10 }
    //   throw r0   ← rethrow modified value
    // } catch(e2) {
    //   return e2  ← should be 11
    // }
    //
    // ip 0-1: Try outer (catch_reg=3, catch_ip=13, no finally)
    // ip 2-3: Try inner (catch_reg=2, catch_ip=8, no finally)
    // ip 4:   LoadInt r0, 1
    // ip 5:   Throw r0
    // ip 6:   Jmp +3 → ip 9    (skip inner catch)
    // ip 7:   <unused>
    // ip 8:   LoadInt r1, 10    ← inner catch: r2 has exception
    // ip 9:   Iadd r0, r2, r1  → r0 = e1 + 10 = 11
    // ip 10:  Throw r0          ← re-throw 11
    // ip 11:  Jmp +2 → ip 13   (skip outer catch, never reached)
    // ip 12:  <unused>
    // ip 13:  Return r3         ← outer catch (r3 = exception = 11)
    // ip 14:  ReturnVoid
    let mut w = RegBytecodeWriter::new();
    // Outer try
    let outer_catch = 13u32;
    w.emit_abcx(RegOpcode::Try, 3, 0, 0, (outer_catch << 16) | 0xFFFF);  // ip 0-1
    // Inner try
    let inner_catch = 8u32;
    w.emit_abcx(RegOpcode::Try, 2, 0, 0, (inner_catch << 16) | 0xFFFF);  // ip 2-3
    w.emit_asbx(RegOpcode::LoadInt, 0, 1);               // ip 4
    w.emit_abc(RegOpcode::Throw, 0, 0, 0);               // ip 5
    w.emit_asbx(RegOpcode::Jmp, 0, 2);                   // ip 6: skip inner catch → ip 9
    w.emit_abc(RegOpcode::Nop, 0, 0, 0);                 // ip 7: padding
    w.emit_asbx(RegOpcode::LoadInt, 1, 10);              // ip 8: inner catch starts
    w.emit_abc(RegOpcode::Iadd, 0, 2, 1);                // ip 9: r0 = r2 + r1 = 1 + 10
    w.emit_abc(RegOpcode::Throw, 0, 0, 0);               // ip 10: re-throw 11
    w.emit_asbx(RegOpcode::Jmp, 0, 1);                   // ip 11: skip outer catch
    w.emit_abc(RegOpcode::Nop, 0, 0, 0);                 // ip 12: padding
    w.emit_abc(RegOpcode::Return, 3, 0, 0);              // ip 13: outer catch
    w.emit_abc(RegOpcode::ReturnVoid, 0, 0, 0);          // ip 14

    let result = exec_reg(w.finish(), 4);
    assert_eq!(result, Value::i32(11));
}

#[test]
fn test_reg_throw_across_call() {
    // func thrower(): throw 77
    // main(): try { call thrower() } catch(e) { return e }
    //
    // func 0 = thrower:
    //   ip 0: LoadInt r0, 77
    //   ip 1: Throw r0
    //
    // func 1 = main:
    //   ip 0-1: Try (catch_reg=1, catch_ip=6, no finally)
    //   ip 2-3: Call (dest=0, arg_base=0, arg_count=0, extra=func_id=0)
    //   ip 4:   EndTry
    //   ip 5:   Jmp +1 → ip 7
    //   ip 6:   Return r1    ← catch
    //   ip 7:   Return r0

    let mut f_thrower = RegBytecodeWriter::new();
    f_thrower.emit_asbx(RegOpcode::LoadInt, 0, 77);
    f_thrower.emit_abc(RegOpcode::Throw, 0, 0, 0);

    let mut f_main = RegBytecodeWriter::new();
    let catch_ip = 6u32;
    f_main.emit_abcx(RegOpcode::Try, 1, 0, 0, (catch_ip << 16) | 0xFFFF);  // ip 0-1
    f_main.emit_abcx(RegOpcode::Call, 0, 2, 0, 0);       // ip 2-3: call func 0
    f_main.emit_abc(RegOpcode::EndTry, 0, 0, 0);          // ip 4
    f_main.emit_asbx(RegOpcode::Jmp, 0, 1);               // ip 5: skip catch
    f_main.emit_abc(RegOpcode::Return, 1, 0, 0);          // ip 6: catch
    f_main.emit_abc(RegOpcode::Return, 0, 0, 0);          // ip 7

    let result = exec_multi(vec![
        ("thrower", f_thrower.finish(), 1, 0),
        ("main", f_main.finish(), 3, 0),
    ]);
    assert_eq!(result, Value::i32(77));
}

#[test]
fn test_reg_rethrow() {
    // try {
    //   try { throw 55 } catch(e) { rethrow }
    // } catch(e2) { return e2 }
    //
    // ip 0-1: Try outer (catch_reg=2, catch_ip=10)
    // ip 2-3: Try inner (catch_reg=1, catch_ip=7)
    // ip 4:   LoadInt r0, 55
    // ip 5:   Throw r0
    // ip 6:   Jmp +2 → ip 8
    // ip 7:   Rethrow              ← inner catch: rethrow
    // ip 8:   EndTry (outer)
    // ip 9:   Jmp +1 → ip 11
    // ip 10:  Return r2            ← outer catch
    // ip 11:  ReturnVoid
    let mut w = RegBytecodeWriter::new();
    w.emit_abcx(RegOpcode::Try, 2, 0, 0, (10u32 << 16) | 0xFFFF);  // ip 0-1
    w.emit_abcx(RegOpcode::Try, 1, 0, 0, (7u32 << 16) | 0xFFFF);   // ip 2-3
    w.emit_asbx(RegOpcode::LoadInt, 0, 55);              // ip 4
    w.emit_abc(RegOpcode::Throw, 0, 0, 0);               // ip 5
    w.emit_asbx(RegOpcode::Jmp, 0, 1);                   // ip 6
    w.emit_abc(RegOpcode::Rethrow, 0, 0, 0);             // ip 7: inner catch
    w.emit_abc(RegOpcode::EndTry, 0, 0, 0);              // ip 8
    w.emit_asbx(RegOpcode::Jmp, 0, 1);                   // ip 9
    w.emit_abc(RegOpcode::Return, 2, 0, 0);              // ip 10: outer catch
    w.emit_abc(RegOpcode::ReturnVoid, 0, 0, 0);          // ip 11

    let result = exec_reg(w.finish(), 3);
    assert_eq!(result, Value::i32(55));
}

#[test]
fn test_reg_unhandled_throw_fails() {
    // throw 42 without try-catch → should return error
    let mut w = RegBytecodeWriter::new();
    w.emit_asbx(RegOpcode::LoadInt, 0, 42);
    w.emit_abc(RegOpcode::Throw, 0, 0, 0);

    let module = make_reg_module(w.finish(), 1);
    let mut vm = Vm::new();
    let result = vm.execute(&module);
    assert!(result.is_err());
}

// ============================================================================
// Phase 4: Concurrency (Spawn + Await)
// ============================================================================

#[test]
fn test_reg_spawn_and_await() {
    // func worker(): return 42
    // main(): r0 = spawn worker(); r1 = await r0; return r1
    //
    // func 0 = worker:
    //   ip 0: LoadInt r0, 42
    //   ip 1: Return r0
    //
    // func 1 = main:
    //   ip 0-1: Spawn (dest=0, arg_base=1, arg_count=0, extra=func_id=0)
    //   ip 2:   Await (dest=1, task=0)
    //   ip 3:   Return r1

    let mut f_worker = RegBytecodeWriter::new();
    f_worker.emit_asbx(RegOpcode::LoadInt, 0, 42);
    f_worker.emit_abc(RegOpcode::Return, 0, 0, 0);

    let mut f_main = RegBytecodeWriter::new();
    f_main.emit_abcx(RegOpcode::Spawn, 0, 1, 0, 0);     // ip 0-1: spawn worker (func 0)
    f_main.emit_abc(RegOpcode::Await, 1, 0, 0);          // ip 2: r1 = await r0
    f_main.emit_abc(RegOpcode::Return, 1, 0, 0);         // ip 3

    let result = exec_multi(vec![
        ("worker", f_worker.finish(), 1, 0),
        ("main", f_main.finish(), 2, 0),
    ]);
    assert_eq!(result, Value::i32(42));
}

#[test]
fn test_reg_spawn_with_args() {
    // func double(x): return x * 2
    // main(): r0 = 21; r1 = spawn double(r0); r2 = await r1; return r2
    //
    // func 0 = double:
    //   ip 0: LoadInt r1, 2
    //   ip 1: Imul r2, r0, r1
    //   ip 2: Return r2
    //
    // func 1 = main:
    //   ip 0: LoadInt r0, 21
    //   ip 1-2: Spawn (dest=1, arg_base=0, arg_count=1, extra=func_id=0)
    //   ip 3: Await (dest=2, task=1)
    //   ip 4: Return r2

    let mut f_double = RegBytecodeWriter::new();
    f_double.emit_asbx(RegOpcode::LoadInt, 1, 2);
    f_double.emit_abc(RegOpcode::Imul, 2, 0, 1);
    f_double.emit_abc(RegOpcode::Return, 2, 0, 0);

    let mut f_main = RegBytecodeWriter::new();
    f_main.emit_asbx(RegOpcode::LoadInt, 0, 21);         // ip 0
    f_main.emit_abcx(RegOpcode::Spawn, 1, 0, 1, 0);     // ip 1-2: spawn double(r0)
    f_main.emit_abc(RegOpcode::Await, 2, 1, 0);          // ip 3: r2 = await r1
    f_main.emit_abc(RegOpcode::Return, 2, 0, 0);         // ip 4

    let result = exec_multi(vec![
        ("double", f_double.finish(), 3, 1),
        ("main", f_main.finish(), 3, 0),
    ]);
    assert_eq!(result, Value::i32(42));
}

#[test]
fn test_reg_multiple_spawns() {
    // func add1(x): return x + 1
    // main():
    //   r0 = 10; r1 = spawn add1(r0)
    //   r2 = 20; r3 = spawn add1(r2)
    //   r4 = await r1; r5 = await r3
    //   r6 = r4 + r5; return r6   → 11 + 21 = 32
    //
    // func 0 = add1:
    //   r1 = 1; r2 = r0 + r1; return r2
    //
    // func 1 = main:
    //   ip 0: LoadInt r0, 10
    //   ip 1-2: Spawn (dest=1, arg_base=0, arg_count=1, extra=0)
    //   ip 3: LoadInt r2, 20
    //   ip 4-5: Spawn (dest=3, arg_base=2, arg_count=1, extra=0)
    //   ip 6: Await (dest=4, task=1)
    //   ip 7: Await (dest=5, task=3)
    //   ip 8: Iadd r6, r4, r5
    //   ip 9: Return r6

    let mut f_add1 = RegBytecodeWriter::new();
    f_add1.emit_asbx(RegOpcode::LoadInt, 1, 1);
    f_add1.emit_abc(RegOpcode::Iadd, 2, 0, 1);
    f_add1.emit_abc(RegOpcode::Return, 2, 0, 0);

    let mut f_main = RegBytecodeWriter::new();
    f_main.emit_asbx(RegOpcode::LoadInt, 0, 10);
    f_main.emit_abcx(RegOpcode::Spawn, 1, 0, 1, 0);
    f_main.emit_asbx(RegOpcode::LoadInt, 2, 20);
    f_main.emit_abcx(RegOpcode::Spawn, 3, 2, 1, 0);
    f_main.emit_abc(RegOpcode::Await, 4, 1, 0);
    f_main.emit_abc(RegOpcode::Await, 5, 3, 0);
    f_main.emit_abc(RegOpcode::Iadd, 6, 4, 5);
    f_main.emit_abc(RegOpcode::Return, 6, 0, 0);

    let result = exec_multi(vec![
        ("add1", f_add1.finish(), 3, 1),
        ("main", f_main.finish(), 7, 0),
    ]);
    assert_eq!(result, Value::i32(32));
}

#[test]
fn test_reg_spawn_await_with_exception() {
    // func fail(): throw 99
    // main(): r0 = spawn fail(); try { r1 = await r0 } catch(e) { return 200 }
    //
    // func 0 = fail:
    //   ip 0: LoadInt r0, 99
    //   ip 1: Throw r0
    //
    // func 1 = main:
    //   ip 0-1: Spawn (dest=0, arg_base=1, arg_count=0, extra=0)
    //   ip 2-3: Try (catch_reg=2, catch_ip=7)
    //   ip 4:   Await (dest=1, task=0)
    //   ip 5:   EndTry
    //   ip 6:   Jmp +1 → ip 8
    //   ip 7:   LoadInt r1, 200   ← catch
    //   ip 8:   Return r1

    let mut f_fail = RegBytecodeWriter::new();
    f_fail.emit_asbx(RegOpcode::LoadInt, 0, 99);
    f_fail.emit_abc(RegOpcode::Throw, 0, 0, 0);

    let mut f_main = RegBytecodeWriter::new();
    f_main.emit_abcx(RegOpcode::Spawn, 0, 1, 0, 0);     // ip 0-1
    f_main.emit_abcx(RegOpcode::Try, 2, 0, 0, (7u32 << 16) | 0xFFFF);  // ip 2-3
    f_main.emit_abc(RegOpcode::Await, 1, 0, 0);          // ip 4
    f_main.emit_abc(RegOpcode::EndTry, 0, 0, 0);         // ip 5
    f_main.emit_asbx(RegOpcode::Jmp, 0, 1);              // ip 6
    f_main.emit_asbx(RegOpcode::LoadInt, 1, 200);        // ip 7: catch
    f_main.emit_abc(RegOpcode::Return, 1, 0, 0);         // ip 8

    let result = exec_multi(vec![
        ("fail", f_fail.finish(), 1, 0),
        ("main", f_main.finish(), 3, 0),
    ]);
    assert_eq!(result, Value::i32(200));
}

#[test]
fn test_reg_yield() {
    // main(): yield; return 42
    // (yield just suspends briefly and resumes)
    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::Yield, 0, 0, 0);               // ip 0
    w.emit_asbx(RegOpcode::LoadInt, 0, 42);               // ip 1
    w.emit_abc(RegOpcode::Return, 0, 0, 0);               // ip 2

    let result = exec_reg(w.finish(), 1);
    assert_eq!(result, Value::i32(42));
}

// ============================================================================
// Phase 5: Native Calls and JSON
// ============================================================================

#[test]
fn test_reg_trap() {
    // Trap with error code 99 — should fail
    let mut w = RegBytecodeWriter::new();
    w.emit_abx(RegOpcode::Trap, 0, 99);

    let module = make_reg_module(w.finish(), 1);
    let mut vm = Vm::new();
    let result = vm.execute(&module);
    assert!(result.is_err());
}

#[test]
fn test_reg_json_new_object_and_length() {
    // r0 = {}; r1 = length(r0); return r1
    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::JsonNewObject, 0, 0, 0);        // ip 0: r0 = {}
    w.emit_abc(RegOpcode::JsonLength, 1, 0, 0);           // ip 1: r1 = length(r0)
    w.emit_abc(RegOpcode::Return, 1, 0, 0);               // ip 2: return r1

    let result = exec_reg(w.finish(), 2);
    assert_eq!(result, Value::i32(0));
}

#[test]
fn test_reg_json_new_array_and_length() {
    // r0 = []; r1 = length(r0); return r1
    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::JsonNewArray, 0, 0, 0);         // ip 0: r0 = []
    w.emit_abc(RegOpcode::JsonLength, 1, 0, 0);           // ip 1: r1 = length(r0)
    w.emit_abc(RegOpcode::Return, 1, 0, 0);               // ip 2: return r1

    let result = exec_reg(w.finish(), 2);
    assert_eq!(result, Value::i32(0));
}

#[test]
fn test_reg_json_array_push_pop_length() {
    // r0 = []; push 42 into r0; r1 = length(r0); return r1
    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::JsonNewArray, 0, 0, 0);         // ip 0: r0 = []
    w.emit_asbx(RegOpcode::LoadInt, 1, 42);               // ip 1: r1 = 42
    w.emit_abc(RegOpcode::JsonPush, 0, 1, 0);             // ip 2: r0.push(r1)
    w.emit_abc(RegOpcode::JsonLength, 2, 0, 0);           // ip 3: r2 = length(r0)
    w.emit_abc(RegOpcode::Return, 2, 0, 0);               // ip 4: return r2

    let result = exec_reg(w.finish(), 3);
    assert_eq!(result, Value::i32(1));
}

#[test]
fn test_reg_json_array_push_and_index() {
    // r0 = []; push 99; r2 = r0[0]; return r2
    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::JsonNewArray, 0, 0, 0);         // ip 0: r0 = []
    w.emit_asbx(RegOpcode::LoadInt, 1, 99);               // ip 1: r1 = 99
    w.emit_abc(RegOpcode::JsonPush, 0, 1, 0);             // ip 2: r0.push(r1)
    w.emit_asbx(RegOpcode::LoadInt, 2, 0);                // ip 3: r2 = 0 (index)
    w.emit_abc(RegOpcode::JsonIndex, 3, 0, 2);            // ip 4: r3 = r0[r2]
    w.emit_abc(RegOpcode::Return, 3, 0, 0);               // ip 5: return r3

    let result = exec_reg(w.finish(), 4);
    // JsonIndex returns a json_to_value result; 99 becomes f64(99.0) in JSON
    assert!(result.as_f64().unwrap() == 99.0 || result.as_i32() == Some(99));
}

#[test]
fn test_reg_json_array_pop() {
    // r0 = []; push 10; push 20; pop → r1; return r1
    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::JsonNewArray, 0, 0, 0);         // ip 0: r0 = []
    w.emit_asbx(RegOpcode::LoadInt, 1, 10);               // ip 1: r1 = 10
    w.emit_abc(RegOpcode::JsonPush, 0, 1, 0);             // ip 2: r0.push(10)
    w.emit_asbx(RegOpcode::LoadInt, 1, 20);               // ip 3: r1 = 20
    w.emit_abc(RegOpcode::JsonPush, 0, 1, 0);             // ip 4: r0.push(20)
    w.emit_abc(RegOpcode::JsonPop, 2, 0, 0);              // ip 5: r2 = r0.pop()
    w.emit_abc(RegOpcode::Return, 2, 0, 0);               // ip 6: return r2

    let result = exec_reg(w.finish(), 3);
    // Popped value 20, which is json Number(20.0)
    assert!(result.as_f64().unwrap() == 20.0 || result.as_i32() == Some(20));
}

#[test]
fn test_reg_json_object_set_get() {
    // r0 = {}; r0.name = 42; r1 = r0.name; return r1
    // Uses extended opcodes with const pool for property name
    let mut module = Module::new("test".to_string());
    let name_idx = module.constants.add_string("name".to_string());

    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::JsonNewObject, 0, 0, 0);        // ip 0: r0 = {}
    w.emit_asbx(RegOpcode::LoadInt, 1, 42);               // ip 1: r1 = 42
    w.emit_abcx(RegOpcode::JsonSet, 0, 1, 0, name_idx);   // ip 2-3: r0["name"] = r1
    w.emit_abcx(RegOpcode::JsonGet, 2, 0, 0, name_idx);   // ip 4-5: r2 = r0["name"]
    w.emit_abc(RegOpcode::Return, 2, 0, 0);               // ip 6: return r2

    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: Vec::new(),
        register_count: 3,
        reg_code: w.finish(),
    });

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();
    // JSON stores numbers as f64, so 42 → Number(42.0) → Value::f64(42.0)
    assert!(result.as_f64().unwrap() == 42.0 || result.as_i32() == Some(42));
}

#[test]
fn test_reg_json_object_delete() {
    // r0 = {}; r0.x = 10; delete r0.x; r1 = length(r0); return r1
    let mut module = Module::new("test".to_string());
    let x_idx = module.constants.add_string("x".to_string());

    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::JsonNewObject, 0, 0, 0);        // ip 0: r0 = {}
    w.emit_asbx(RegOpcode::LoadInt, 1, 10);               // ip 1: r1 = 10
    w.emit_abcx(RegOpcode::JsonSet, 0, 1, 0, x_idx);      // ip 2-3: r0["x"] = r1
    w.emit_abcx(RegOpcode::JsonDelete, 0, 0, 0, x_idx);   // ip 4-5: delete r0["x"]
    w.emit_abc(RegOpcode::JsonLength, 2, 0, 0);           // ip 6: r2 = length(r0)
    w.emit_abc(RegOpcode::Return, 2, 0, 0);               // ip 7: return r2

    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: Vec::new(),
        register_count: 3,
        reg_code: w.finish(),
    });

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(0)); // empty after delete
}

#[test]
fn test_reg_json_object_multiple_properties() {
    // r0 = {}; r0.a = 1; r0.b = 2; r1 = length(r0); return r1
    let mut module = Module::new("test".to_string());
    let a_idx = module.constants.add_string("a".to_string());
    let b_idx = module.constants.add_string("b".to_string());

    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::JsonNewObject, 0, 0, 0);        // ip 0: r0 = {}
    w.emit_asbx(RegOpcode::LoadInt, 1, 1);                // ip 1: r1 = 1
    w.emit_abcx(RegOpcode::JsonSet, 0, 1, 0, a_idx);      // ip 2-3: r0["a"] = 1
    w.emit_asbx(RegOpcode::LoadInt, 1, 2);                // ip 4: r1 = 2
    w.emit_abcx(RegOpcode::JsonSet, 0, 1, 0, b_idx);      // ip 5-6: r0["b"] = 2
    w.emit_abc(RegOpcode::JsonLength, 2, 0, 0);           // ip 7: r2 = length(r0)
    w.emit_abc(RegOpcode::Return, 2, 0, 0);               // ip 8: return r2

    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: Vec::new(),
        register_count: 3,
        reg_code: w.finish(),
    });

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(2));
}

#[test]
fn test_reg_json_array_index_set() {
    // r0 = []; push null; r0[0] = 77; r1 = r0[0]; return r1
    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::JsonNewArray, 0, 0, 0);         // ip 0: r0 = []
    w.emit_abc(RegOpcode::LoadNil, 1, 0, 0);              // ip 1: r1 = null
    w.emit_abc(RegOpcode::JsonPush, 0, 1, 0);             // ip 2: r0.push(null)
    w.emit_asbx(RegOpcode::LoadInt, 1, 0);                // ip 3: r1 = 0 (index)
    w.emit_asbx(RegOpcode::LoadInt, 2, 77);               // ip 4: r2 = 77
    w.emit_abc(RegOpcode::JsonIndexSet, 0, 1, 2);         // ip 5: r0[r1] = r2
    w.emit_asbx(RegOpcode::LoadInt, 1, 0);                // ip 6: r1 = 0 (index)
    w.emit_abc(RegOpcode::JsonIndex, 3, 0, 1);            // ip 7: r3 = r0[r1]
    w.emit_abc(RegOpcode::Return, 3, 0, 0);               // ip 8: return r3

    let result = exec_reg(w.finish(), 4);
    assert!(result.as_f64().unwrap() == 77.0 || result.as_i32() == Some(77));
}

#[test]
fn test_reg_json_get_null_safe() {
    // r0 = null; r1 = r0["prop"]; return r1
    // JsonGet on null should return null (not error)
    let mut module = Module::new("test".to_string());
    let prop_idx = module.constants.add_string("prop".to_string());

    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::LoadNil, 0, 0, 0);              // ip 0: r0 = null
    w.emit_abcx(RegOpcode::JsonGet, 1, 0, 0, prop_idx);   // ip 1-2: r1 = r0["prop"]
    w.emit_abc(RegOpcode::Return, 1, 0, 0);               // ip 3: return r1

    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: Vec::new(),
        register_count: 2,
        reg_code: w.finish(),
    });

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::null());
}

#[test]
fn test_reg_json_array_multiple_push_and_length() {
    // r0 = []; push 1,2,3; length → 3
    let mut w = RegBytecodeWriter::new();
    w.emit_abc(RegOpcode::JsonNewArray, 0, 0, 0);         // ip 0: r0 = []
    w.emit_asbx(RegOpcode::LoadInt, 1, 1);                // ip 1: r1 = 1
    w.emit_abc(RegOpcode::JsonPush, 0, 1, 0);             // ip 2: push 1
    w.emit_asbx(RegOpcode::LoadInt, 1, 2);                // ip 3: r1 = 2
    w.emit_abc(RegOpcode::JsonPush, 0, 1, 0);             // ip 4: push 2
    w.emit_asbx(RegOpcode::LoadInt, 1, 3);                // ip 5: r1 = 3
    w.emit_abc(RegOpcode::JsonPush, 0, 1, 0);             // ip 6: push 3
    w.emit_abc(RegOpcode::JsonLength, 2, 0, 0);           // ip 7: r2 = length(r0)
    w.emit_abc(RegOpcode::Return, 2, 0, 0);               // ip 8: return r2

    let result = exec_reg(w.finish(), 3);
    assert_eq!(result, Value::i32(3));
}
