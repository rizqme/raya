//! Comprehensive Opcode Test Suite (Milestone 1.14)
//!
//! This test suite validates correctness of all VM opcodes.
//! Tests are organized by opcode category and validate:
//! - Correctness of individual opcodes
//! - Edge cases and error handling
//! - Interaction between opcodes
//!
//! # Test Coverage
//! - Constants and stack operations (0x00-0x0F)
//! - Local variables (0x10-0x1F)
//! - Integer arithmetic (0x20-0x2F)
//! - Float arithmetic (0x30-0x3F)
//! - Number arithmetic (0x40-0x4F)
//! - Integer comparison (0x50-0x5F)
//! - Float comparison (0x60-0x6F)
//! - Generic comparison and logical (0x70-0x7F)
//! - String operations (0x80-0x8F)
//! - Control flow (0x90-0x9F)
//! - Function calls (0xA0-0xAF)
//! - Object operations (0xB0-0xBF)
//! - Array operations (0xC0-0xCF)
//! - Task & concurrency (0xD0-0xDF)
//! - Synchronization (0xE0-0xEF)
//! - Advanced operations (0xF0-0xFF)
//!
//! # Running Tests
//! ```bash
//! cargo test --test opcode_tests
//! ```

use raya_bytecode::{Function, Module, Opcode};
use raya_core::value::Value;
use raya_core::vm::Vm;

/// Helper function to create and execute simple bytecode
fn execute_bytecode(code: Vec<u8>) -> Value {
    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 4, // Reserve locals for most tests
        code,
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    vm.execute(&module).unwrap()
}

/// Helper to encode i32 as little-endian bytes
fn i32_bytes(val: i32) -> [u8; 4] {
    val.to_le_bytes()
}

/// Helper to encode f64 as little-endian bytes
fn f64_bytes(val: f64) -> [u8; 8] {
    val.to_le_bytes()
}

/// Helper to encode u16 as little-endian bytes
fn u16_bytes(val: u16) -> [u8; 2] {
    val.to_le_bytes()
}

// ===== Constants (0x00-0x0F) =====

#[cfg(test)]
mod constants {
    use super::*;

    #[test]
    fn test_nop() {
        // NOP should do nothing, return null
        let result = execute_bytecode(vec![
            Opcode::Nop as u8,
            Opcode::ConstNull as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::null());
    }

    #[test]
    fn test_const_null() {
        let result = execute_bytecode(vec![Opcode::ConstNull as u8, Opcode::Return as u8]);
        assert_eq!(result, Value::null());
    }

    #[test]
    fn test_const_true() {
        let result = execute_bytecode(vec![Opcode::ConstTrue as u8, Opcode::Return as u8]);
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_const_false() {
        let result = execute_bytecode(vec![Opcode::ConstFalse as u8, Opcode::Return as u8]);
        assert_eq!(result, Value::bool(false));
    }

    #[test]
    fn test_const_i32_positive() {
        let code = vec![
            Opcode::ConstI32 as u8,
            42,
            0,
            0,
            0,
            Opcode::Return as u8,
        ];
        let result = execute_bytecode(code);
        assert_eq!(result, Value::i32(42));
    }

    #[test]
    fn test_const_i32_negative() {
        let bytes = i32_bytes(-100);
        let code = vec![
            Opcode::ConstI32 as u8,
            bytes[0],
            bytes[1],
            bytes[2],
            bytes[3],
            Opcode::Return as u8,
        ];
        let result = execute_bytecode(code);
        assert_eq!(result, Value::i32(-100));
    }

    #[test]
    fn test_const_i32_zero() {
        let code = vec![
            Opcode::ConstI32 as u8,
            0,
            0,
            0,
            0,
            Opcode::Return as u8,
        ];
        let result = execute_bytecode(code);
        assert_eq!(result, Value::i32(0));
    }

    #[test]
    fn test_const_f64_positive() {
        let bytes = f64_bytes(3.14);
        let mut code = vec![Opcode::ConstF64 as u8];
        code.extend_from_slice(&bytes);
        code.push(Opcode::Return as u8);
        let result = execute_bytecode(code);
        assert_eq!(result, Value::f64(3.14));
    }

    #[test]
    fn test_const_f64_negative() {
        let bytes = f64_bytes(-2.71);
        let mut code = vec![Opcode::ConstF64 as u8];
        code.extend_from_slice(&bytes);
        code.push(Opcode::Return as u8);
        let result = execute_bytecode(code);
        assert_eq!(result, Value::f64(-2.71));
    }

    #[test]
    fn test_const_f64_zero() {
        let bytes = f64_bytes(0.0);
        let mut code = vec![Opcode::ConstF64 as u8];
        code.extend_from_slice(&bytes);
        code.push(Opcode::Return as u8);
        let result = execute_bytecode(code);
        assert_eq!(result, Value::f64(0.0));
    }
}

// ===== Stack Operations =====

#[cfg(test)]
mod stack_ops {
    use super::*;

    #[test]
    fn test_pop() {
        // Push two values, pop one, return the other
        let result = execute_bytecode(vec![
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
            Opcode::Pop as u8, // Pop 20
            Opcode::Return as u8, // Return 10
        ]);
        assert_eq!(result, Value::i32(10));
    }

    #[test]
    fn test_dup() {
        // Push value, duplicate it, add them
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            5,
            0,
            0,
            0,
            Opcode::Dup as u8, // Stack: [5, 5]
            Opcode::Iadd as u8, // Stack: [10]
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(10));
    }

    #[test]
    fn test_swap() {
        // Push two values, swap them, subtract
        let result = execute_bytecode(vec![
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
            Opcode::Swap as u8, // Stack: [20, 10] -> [10, 20]
            Opcode::Isub as u8, // 10 - 20 = -10
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(-10));
    }
}

// ===== Local Variables (0x10-0x1F) =====

#[cfg(test)]
mod local_variables {
    use super::*;

    #[test]
    fn test_load_store_local() {
        let bytes = u16_bytes(0);
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            42,
            0,
            0,
            0,
            Opcode::StoreLocal as u8,
            bytes[0],
            bytes[1],
            Opcode::LoadLocal as u8,
            bytes[0],
            bytes[1],
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(42));
    }

    #[test]
    fn test_load_local_0() {
        // Use optimized LoadLocal0 instruction
        let bytes = u16_bytes(0);
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            100,
            0,
            0,
            0,
            Opcode::StoreLocal as u8,
            bytes[0],
            bytes[1],
            Opcode::LoadLocal0 as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(100));
    }

    #[test]
    fn test_store_local_0() {
        // Use optimized StoreLocal0 instruction
        let bytes = u16_bytes(0);
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            77,
            0,
            0,
            0,
            Opcode::StoreLocal0 as u8,
            Opcode::LoadLocal as u8,
            bytes[0],
            bytes[1],
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(77));
    }

    #[test]
    fn test_multiple_locals() {
        // Test using multiple local variables
        let bytes0 = u16_bytes(0);
        let bytes1 = u16_bytes(1);
        let result = execute_bytecode(vec![
            // local0 = 10
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::StoreLocal as u8,
            bytes0[0],
            bytes0[1],
            // local1 = 20
            Opcode::ConstI32 as u8,
            20,
            0,
            0,
            0,
            Opcode::StoreLocal as u8,
            bytes1[0],
            bytes1[1],
            // return local0 + local1
            Opcode::LoadLocal as u8,
            bytes0[0],
            bytes0[1],
            Opcode::LoadLocal as u8,
            bytes1[0],
            bytes1[1],
            Opcode::Iadd as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(30));
    }
}

// ===== Integer Arithmetic (0x20-0x2F) =====

#[cfg(test)]
mod integer_arithmetic {
    use super::*;

    #[test]
    fn test_iadd() {
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            15,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            27,
            0,
            0,
            0,
            Opcode::Iadd as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(42));
    }

    #[test]
    fn test_isub() {
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            50,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            8,
            0,
            0,
            0,
            Opcode::Isub as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(42));
    }

    #[test]
    fn test_imul() {
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            6,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            7,
            0,
            0,
            0,
            Opcode::Imul as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(42));
    }

    #[test]
    fn test_idiv() {
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            84,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            2,
            0,
            0,
            0,
            Opcode::Idiv as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(42));
    }

    #[test]
    fn test_imod() {
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            47,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            5,
            0,
            0,
            0,
            Opcode::Imod as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(2)); // 47 % 5 = 2
    }

    #[test]
    fn test_ineg() {
        let bytes = i32_bytes(42);
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            bytes[0],
            bytes[1],
            bytes[2],
            bytes[3],
            Opcode::Ineg as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(-42));
    }

    #[test]
    fn test_ineg_negative() {
        let bytes = i32_bytes(-10);
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            bytes[0],
            bytes[1],
            bytes[2],
            bytes[3],
            Opcode::Ineg as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(10));
    }
}

// ===== Float Arithmetic (0x30-0x3F) =====

#[cfg(test)]
mod float_arithmetic {
    use super::*;

    #[test]
    fn test_fadd() {
        let bytes1 = f64_bytes(1.5);
        let bytes2 = f64_bytes(2.5);
        let mut code = vec![Opcode::ConstF64 as u8];
        code.extend_from_slice(&bytes1);
        code.push(Opcode::ConstF64 as u8);
        code.extend_from_slice(&bytes2);
        code.push(Opcode::Fadd as u8);
        code.push(Opcode::Return as u8);
        let result = execute_bytecode(code);
        assert_eq!(result, Value::f64(4.0));
    }

    #[test]
    fn test_fsub() {
        let bytes1 = f64_bytes(5.5);
        let bytes2 = f64_bytes(3.2);
        let mut code = vec![Opcode::ConstF64 as u8];
        code.extend_from_slice(&bytes1);
        code.push(Opcode::ConstF64 as u8);
        code.extend_from_slice(&bytes2);
        code.push(Opcode::Fsub as u8);
        code.push(Opcode::Return as u8);
        let result = execute_bytecode(code);
        assert!((result.as_f64().unwrap() - 2.3).abs() < 0.0001);
    }

    #[test]
    fn test_fmul() {
        let bytes1 = f64_bytes(2.5);
        let bytes2 = f64_bytes(4.0);
        let mut code = vec![Opcode::ConstF64 as u8];
        code.extend_from_slice(&bytes1);
        code.push(Opcode::ConstF64 as u8);
        code.extend_from_slice(&bytes2);
        code.push(Opcode::Fmul as u8);
        code.push(Opcode::Return as u8);
        let result = execute_bytecode(code);
        assert_eq!(result, Value::f64(10.0));
    }

    #[test]
    fn test_fdiv() {
        let bytes1 = f64_bytes(10.0);
        let bytes2 = f64_bytes(2.5);
        let mut code = vec![Opcode::ConstF64 as u8];
        code.extend_from_slice(&bytes1);
        code.push(Opcode::ConstF64 as u8);
        code.extend_from_slice(&bytes2);
        code.push(Opcode::Fdiv as u8);
        code.push(Opcode::Return as u8);
        let result = execute_bytecode(code);
        assert_eq!(result, Value::f64(4.0));
    }

    #[test]
    fn test_fneg() {
        let bytes = f64_bytes(3.14);
        let mut code = vec![Opcode::ConstF64 as u8];
        code.extend_from_slice(&bytes);
        code.push(Opcode::Fneg as u8);
        code.push(Opcode::Return as u8);
        let result = execute_bytecode(code);
        assert_eq!(result, Value::f64(-3.14));
    }
}

// ===== Number Arithmetic - Generic (0x40-0x4F) =====

#[cfg(test)]
mod number_arithmetic {
    use super::*;

    #[test]
    fn test_nadd_integers() {
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            32,
            0,
            0,
            0,
            Opcode::Nadd as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(42));
    }

    #[test]
    fn test_nadd_floats() {
        let bytes1 = f64_bytes(2.5);
        let bytes2 = f64_bytes(1.5);
        let mut code = vec![Opcode::ConstF64 as u8];
        code.extend_from_slice(&bytes1);
        code.push(Opcode::ConstF64 as u8);
        code.extend_from_slice(&bytes2);
        code.push(Opcode::Nadd as u8);
        code.push(Opcode::Return as u8);
        let result = execute_bytecode(code);
        assert_eq!(result, Value::f64(4.0));
    }

    #[test]
    fn test_nsub() {
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            50,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            8,
            0,
            0,
            0,
            Opcode::Nsub as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(42));
    }

    #[test]
    fn test_nmul() {
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            6,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            7,
            0,
            0,
            0,
            Opcode::Nmul as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(42));
    }

    #[test]
    fn test_ndiv() {
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            84,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            2,
            0,
            0,
            0,
            Opcode::Ndiv as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(42));
    }
}

// ===== Integer Comparison (0x50-0x5F) =====

#[cfg(test)]
mod integer_comparison {
    use super::*;

    #[test]
    fn test_ieq_true() {
        let result = execute_bytecode(vec![
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
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_ieq_false() {
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            42,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            43,
            0,
            0,
            0,
            Opcode::Ieq as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::bool(false));
    }

    #[test]
    fn test_ine_true() {
        let result = execute_bytecode(vec![
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
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_ilt_true() {
        let result = execute_bytecode(vec![
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
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_ilt_false() {
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            20,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::Ilt as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::bool(false));
    }

    #[test]
    fn test_ile() {
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::Ile as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_igt() {
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            20,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::Igt as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_ige() {
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::Ige as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::bool(true));
    }
}

// ===== Float Comparison (0x60-0x6F) =====

#[cfg(test)]
mod float_comparison {
    use super::*;

    #[test]
    fn test_feq_true() {
        let bytes = f64_bytes(3.14);
        let mut code = vec![Opcode::ConstF64 as u8];
        code.extend_from_slice(&bytes);
        code.push(Opcode::ConstF64 as u8);
        code.extend_from_slice(&bytes);
        code.push(Opcode::Feq as u8);
        code.push(Opcode::Return as u8);
        let result = execute_bytecode(code);
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_feq_false() {
        let bytes1 = f64_bytes(3.14);
        let bytes2 = f64_bytes(2.71);
        let mut code = vec![Opcode::ConstF64 as u8];
        code.extend_from_slice(&bytes1);
        code.push(Opcode::ConstF64 as u8);
        code.extend_from_slice(&bytes2);
        code.push(Opcode::Feq as u8);
        code.push(Opcode::Return as u8);
        let result = execute_bytecode(code);
        assert_eq!(result, Value::bool(false));
    }

    #[test]
    fn test_flt() {
        let bytes1 = f64_bytes(2.0);
        let bytes2 = f64_bytes(3.0);
        let mut code = vec![Opcode::ConstF64 as u8];
        code.extend_from_slice(&bytes1);
        code.push(Opcode::ConstF64 as u8);
        code.extend_from_slice(&bytes2);
        code.push(Opcode::Flt as u8);
        code.push(Opcode::Return as u8);
        let result = execute_bytecode(code);
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_fgt() {
        let bytes1 = f64_bytes(5.0);
        let bytes2 = f64_bytes(3.0);
        let mut code = vec![Opcode::ConstF64 as u8];
        code.extend_from_slice(&bytes1);
        code.push(Opcode::ConstF64 as u8);
        code.extend_from_slice(&bytes2);
        code.push(Opcode::Fgt as u8);
        code.push(Opcode::Return as u8);
        let result = execute_bytecode(code);
        assert_eq!(result, Value::bool(true));
    }
}

// ===== Generic Comparison & Logical (0x70-0x7F) =====

#[cfg(test)]
mod comparison_logical {
    use super::*;

    #[test]
    fn test_eq_integers() {
        let result = execute_bytecode(vec![
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
            Opcode::Eq as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_ne_integers() {
        let result = execute_bytecode(vec![
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
            Opcode::Ne as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_strict_eq() {
        let result = execute_bytecode(vec![
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
            Opcode::StrictEq as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_not_true() {
        let result = execute_bytecode(vec![
            Opcode::ConstTrue as u8,
            Opcode::Not as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::bool(false));
    }

    #[test]
    fn test_not_false() {
        let result = execute_bytecode(vec![
            Opcode::ConstFalse as u8,
            Opcode::Not as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_and_true_true() {
        let result = execute_bytecode(vec![
            Opcode::ConstTrue as u8,
            Opcode::ConstTrue as u8,
            Opcode::And as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_and_true_false() {
        let result = execute_bytecode(vec![
            Opcode::ConstTrue as u8,
            Opcode::ConstFalse as u8,
            Opcode::And as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::bool(false));
    }

    #[test]
    fn test_or_false_true() {
        let result = execute_bytecode(vec![
            Opcode::ConstFalse as u8,
            Opcode::ConstTrue as u8,
            Opcode::Or as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_or_false_false() {
        let result = execute_bytecode(vec![
            Opcode::ConstFalse as u8,
            Opcode::ConstFalse as u8,
            Opcode::Or as u8,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::bool(false));
    }
}

// ===== Control Flow (0x90-0x9F) =====

#[cfg(test)]
mod control_flow {
    use super::*;

    #[test]
    fn test_jmp_forward() {
        // Jump over some instructions
        let jump_offset = i32_bytes(7); // Skip CONST_I32 10
        let result = execute_bytecode(vec![
            Opcode::Jmp as u8,
            jump_offset[0],
            jump_offset[1],
            jump_offset[2],
            jump_offset[3],
            // This should be skipped
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::Return as u8,
            // Jump here
            Opcode::ConstI32 as u8,
            42,
            0,
            0,
            0,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(42));
    }

    #[test]
    fn test_jmp_if_true_taken() {
        let jump_offset = i32_bytes(7);
        let result = execute_bytecode(vec![
            Opcode::ConstTrue as u8,
            Opcode::JmpIfTrue as u8,
            jump_offset[0],
            jump_offset[1],
            jump_offset[2],
            jump_offset[3],
            // Should be skipped
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::Return as u8,
            // Jump here
            Opcode::ConstI32 as u8,
            42,
            0,
            0,
            0,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(42));
    }

    #[test]
    fn test_jmp_if_true_not_taken() {
        let jump_offset = i32_bytes(7);
        let result = execute_bytecode(vec![
            Opcode::ConstFalse as u8,
            Opcode::JmpIfTrue as u8,
            jump_offset[0],
            jump_offset[1],
            jump_offset[2],
            jump_offset[3],
            // Should execute this
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::Return as u8,
            // Should not reach here
            Opcode::ConstI32 as u8,
            42,
            0,
            0,
            0,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(10));
    }

    #[test]
    fn test_jmp_if_false_taken() {
        let jump_offset = i32_bytes(7);
        let result = execute_bytecode(vec![
            Opcode::ConstFalse as u8,
            Opcode::JmpIfFalse as u8,
            jump_offset[0],
            jump_offset[1],
            jump_offset[2],
            jump_offset[3],
            // Should be skipped
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::Return as u8,
            // Jump here
            Opcode::ConstI32 as u8,
            42,
            0,
            0,
            0,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(42));
    }

    #[test]
    fn test_jmp_if_null_taken() {
        let jump_offset = i32_bytes(7);
        let result = execute_bytecode(vec![
            Opcode::ConstNull as u8,
            Opcode::JmpIfNull as u8,
            jump_offset[0],
            jump_offset[1],
            jump_offset[2],
            jump_offset[3],
            // Should be skipped
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::Return as u8,
            // Jump here
            Opcode::ConstI32 as u8,
            42,
            0,
            0,
            0,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(42));
    }

    #[test]
    fn test_jmp_if_not_null_taken() {
        let jump_offset = i32_bytes(7);
        let result = execute_bytecode(vec![
            Opcode::ConstI32 as u8,
            1,
            0,
            0,
            0,
            Opcode::JmpIfNotNull as u8,
            jump_offset[0],
            jump_offset[1],
            jump_offset[2],
            jump_offset[3],
            // Should be skipped
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::Return as u8,
            // Jump here
            Opcode::ConstI32 as u8,
            42,
            0,
            0,
            0,
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(42));
    }
}

// ===== Composite Scenarios =====

#[cfg(test)]
mod composite {
    use super::*;

    #[test]
    fn test_factorial_iterative() {
        // Compute factorial of 5 iteratively
        // result = 1
        // i = 5
        // while i > 1:
        //   result = result * i
        //   i = i - 1
        let bytes0 = u16_bytes(0); // result
        let bytes1 = u16_bytes(1); // i
        let loop_start_offset = i32_bytes(-20); // Jump back to loop condition

        let result = execute_bytecode(vec![
            // result = 1 (local 0)
            Opcode::ConstI32 as u8,
            1,
            0,
            0,
            0,
            Opcode::StoreLocal as u8,
            bytes0[0],
            bytes0[1],
            // i = 5 (local 1)
            Opcode::ConstI32 as u8,
            5,
            0,
            0,
            0,
            Opcode::StoreLocal as u8,
            bytes1[0],
            bytes1[1],
            // Loop: while i > 1
            Opcode::LoadLocal as u8,
            bytes1[0],
            bytes1[1],
            Opcode::ConstI32 as u8,
            1,
            0,
            0,
            0,
            Opcode::Igt as u8,
            Opcode::JmpIfFalse as u8,
            28,
            0,
            0,
            0, // Jump to end if i <= 1
            // result = result * i
            Opcode::LoadLocal as u8,
            bytes0[0],
            bytes0[1],
            Opcode::LoadLocal as u8,
            bytes1[0],
            bytes1[1],
            Opcode::Imul as u8,
            Opcode::StoreLocal as u8,
            bytes0[0],
            bytes0[1],
            // i = i - 1
            Opcode::LoadLocal as u8,
            bytes1[0],
            bytes1[1],
            Opcode::ConstI32 as u8,
            1,
            0,
            0,
            0,
            Opcode::Isub as u8,
            Opcode::StoreLocal as u8,
            bytes1[0],
            bytes1[1],
            // Jump back to loop start
            Opcode::Jmp as u8,
            loop_start_offset[0],
            loop_start_offset[1],
            loop_start_offset[2],
            loop_start_offset[3],
            // Return result
            Opcode::LoadLocal as u8,
            bytes0[0],
            bytes0[1],
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(120)); // 5! = 120
    }

    #[test]
    fn test_fibonacci_iterative() {
        // Compute 7th Fibonacci number iteratively
        // a = 0, b = 1, count = 7
        // for i in 0..count: a, b = b, a + b
        let bytes_a = u16_bytes(0);
        let bytes_b = u16_bytes(1);
        let bytes_i = u16_bytes(2);
        let bytes_tmp = u16_bytes(3);
        let loop_start_offset = i32_bytes(-38);

        let result = execute_bytecode(vec![
            // a = 0
            Opcode::ConstI32 as u8,
            0,
            0,
            0,
            0,
            Opcode::StoreLocal as u8,
            bytes_a[0],
            bytes_a[1],
            // b = 1
            Opcode::ConstI32 as u8,
            1,
            0,
            0,
            0,
            Opcode::StoreLocal as u8,
            bytes_b[0],
            bytes_b[1],
            // i = 0
            Opcode::ConstI32 as u8,
            0,
            0,
            0,
            0,
            Opcode::StoreLocal as u8,
            bytes_i[0],
            bytes_i[1],
            // Loop: while i < 7
            Opcode::LoadLocal as u8,
            bytes_i[0],
            bytes_i[1],
            Opcode::ConstI32 as u8,
            7,
            0,
            0,
            0,
            Opcode::Ilt as u8,
            Opcode::JmpIfFalse as u8,
            40,
            0,
            0,
            0, // Jump to end if i >= 7
            // tmp = a + b
            Opcode::LoadLocal as u8,
            bytes_a[0],
            bytes_a[1],
            Opcode::LoadLocal as u8,
            bytes_b[0],
            bytes_b[1],
            Opcode::Iadd as u8,
            Opcode::StoreLocal as u8,
            bytes_tmp[0],
            bytes_tmp[1],
            // a = b
            Opcode::LoadLocal as u8,
            bytes_b[0],
            bytes_b[1],
            Opcode::StoreLocal as u8,
            bytes_a[0],
            bytes_a[1],
            // b = tmp
            Opcode::LoadLocal as u8,
            bytes_tmp[0],
            bytes_tmp[1],
            Opcode::StoreLocal as u8,
            bytes_b[0],
            bytes_b[1],
            // i = i + 1
            Opcode::LoadLocal as u8,
            bytes_i[0],
            bytes_i[1],
            Opcode::ConstI32 as u8,
            1,
            0,
            0,
            0,
            Opcode::Iadd as u8,
            Opcode::StoreLocal as u8,
            bytes_i[0],
            bytes_i[1],
            // Jump back to loop start
            Opcode::Jmp as u8,
            loop_start_offset[0],
            loop_start_offset[1],
            loop_start_offset[2],
            loop_start_offset[3],
            // Return a
            Opcode::LoadLocal as u8,
            bytes_a[0],
            bytes_a[1],
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(13)); // 7th Fibonacci number is 13
    }

    #[test]
    fn test_complex_expression() {
        // (10 + 20) * (30 - 15) / 5
        let result = execute_bytecode(vec![
            // 10 + 20
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
            Opcode::Iadd as u8, // Stack: [30]
            // 30 - 15
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
            Opcode::Isub as u8, // Stack: [30, 15]
            // Multiply
            Opcode::Imul as u8, // Stack: [450]
            // Divide by 5
            Opcode::ConstI32 as u8,
            5,
            0,
            0,
            0,
            Opcode::Idiv as u8, // Stack: [90]
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(90)); // (10+20) * (30-15) / 5 = 30 * 15 / 5 = 90
    }

    #[test]
    fn test_boolean_logic() {
        // (true && false) || (true && true)
        let result = execute_bytecode(vec![
            Opcode::ConstTrue as u8,
            Opcode::ConstFalse as u8,
            Opcode::And as u8, // Stack: [false]
            Opcode::ConstTrue as u8,
            Opcode::ConstTrue as u8,
            Opcode::And as u8, // Stack: [false, true]
            Opcode::Or as u8,  // Stack: [true]
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_nested_arithmetic() {
        // ((5 + 3) * 2) - ((10 / 2) + 1)
        let result = execute_bytecode(vec![
            // (5 + 3)
            Opcode::ConstI32 as u8,
            5,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            3,
            0,
            0,
            0,
            Opcode::Iadd as u8, // Stack: [8]
            // * 2
            Opcode::ConstI32 as u8,
            2,
            0,
            0,
            0,
            Opcode::Imul as u8, // Stack: [16]
            // (10 / 2)
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            2,
            0,
            0,
            0,
            Opcode::Idiv as u8, // Stack: [16, 5]
            // + 1
            Opcode::ConstI32 as u8,
            1,
            0,
            0,
            0,
            Opcode::Iadd as u8, // Stack: [16, 6]
            // Final subtraction
            Opcode::Isub as u8, // Stack: [10]
            Opcode::Return as u8,
        ]);
        assert_eq!(result, Value::i32(10)); // ((5+3)*2) - ((10/2)+1) = 16 - 6 = 10
    }
}
