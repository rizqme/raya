//! Register-based arithmetic opcode handlers

use crate::compiler::bytecode::reg_opcode::{RegInstr, RegOpcode};
use crate::vm::interpreter::core::value_to_f64;
use crate::vm::interpreter::reg_execution::RegOpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::register_file::RegisterFile;
use crate::vm::value::Value;
use crate::vm::VmError;

/// Helper macro for binary integer operations
macro_rules! int_binop {
    ($regs:expr, $base:expr, $instr:expr, $op:expr) => {{
        let b = match $regs.get_reg($base, $instr.b()) {
            Ok(v) => v.as_i32().unwrap_or(0),
            Err(e) => return RegOpcodeResult::Error(e),
        };
        let a = match $regs.get_reg($base, $instr.c()) {
            Ok(v) => v.as_i32().unwrap_or(0),
            Err(e) => return RegOpcodeResult::Error(e),
        };
        let result = $op(b, a);
        if let Err(e) = $regs.set_reg($base, $instr.a(), Value::i32(result)) {
            return RegOpcodeResult::Error(e);
        }
        RegOpcodeResult::Continue
    }};
}

/// Helper macro for binary float operations
macro_rules! float_binop {
    ($regs:expr, $base:expr, $instr:expr, $op:expr) => {{
        let b = match $regs.get_reg($base, $instr.b()).and_then(|v| value_to_f64(v)) {
            Ok(v) => v,
            Err(e) => return RegOpcodeResult::Error(e),
        };
        let a = match $regs.get_reg($base, $instr.c()).and_then(|v| value_to_f64(v)) {
            Ok(v) => v,
            Err(e) => return RegOpcodeResult::Error(e),
        };
        let result = $op(b, a);
        if let Err(e) = $regs.set_reg($base, $instr.a(), Value::f64(result)) {
            return RegOpcodeResult::Error(e);
        }
        RegOpcodeResult::Continue
    }};
}

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_reg_arithmetic_ops(
        &mut self,
        regs: &mut RegisterFile,
        reg_base: usize,
        instr: RegInstr,
    ) -> RegOpcodeResult {
        let opcode = match instr.opcode() {
            Some(op) => op,
            None => return RegOpcodeResult::error(VmError::InvalidOpcode(instr.opcode_byte())),
        };

        match opcode {
            // =========================================================
            // Integer Arithmetic
            // =========================================================
            RegOpcode::Iadd => int_binop!(regs, reg_base, instr, |b: i32, a: i32| b
                .wrapping_add(a)),
            RegOpcode::Isub => int_binop!(regs, reg_base, instr, |b: i32, a: i32| b
                .wrapping_sub(a)),
            RegOpcode::Imul => int_binop!(regs, reg_base, instr, |b: i32, a: i32| b
                .wrapping_mul(a)),

            RegOpcode::Idiv => {
                let b = match regs.get_reg(reg_base, instr.b()) {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let c = match regs.get_reg(reg_base, instr.c()) {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                if c == 0 {
                    return RegOpcodeResult::runtime_error("division by zero");
                }
                if let Err(e) = regs.set_reg(reg_base, instr.a(), Value::i32(b.wrapping_div(c))) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::Imod => {
                let b = match regs.get_reg(reg_base, instr.b()) {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let c = match regs.get_reg(reg_base, instr.c()) {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                if c == 0 {
                    return RegOpcodeResult::runtime_error("division by zero");
                }
                if let Err(e) = regs.set_reg(reg_base, instr.a(), Value::i32(b.wrapping_rem(c))) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::Ineg => {
                // rA = -rB (C unused)
                let b = match regs.get_reg(reg_base, instr.b()) {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                if let Err(e) = regs.set_reg(reg_base, instr.a(), Value::i32(b.wrapping_neg())) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::Ipow => {
                let b = match regs.get_reg(reg_base, instr.b()) {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let c = match regs.get_reg(reg_base, instr.c()) {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let result = if c < 0 { 0 } else { b.wrapping_pow(c as u32) };
                if let Err(e) = regs.set_reg(reg_base, instr.a(), Value::i32(result)) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            // =========================================================
            // Integer Bitwise
            // =========================================================
            RegOpcode::Ishl => {
                int_binop!(regs, reg_base, instr, |b: i32, a: i32| b << (a & 31))
            }
            RegOpcode::Ishr => {
                int_binop!(regs, reg_base, instr, |b: i32, a: i32| b >> (a & 31))
            }
            RegOpcode::Iushr => int_binop!(regs, reg_base, instr, |b: i32, a: i32| ((b as u32)
                >> (a & 31))
                as i32),
            RegOpcode::Iand => int_binop!(regs, reg_base, instr, |b: i32, a: i32| b & a),
            RegOpcode::Ior => int_binop!(regs, reg_base, instr, |b: i32, a: i32| b | a),
            RegOpcode::Ixor => int_binop!(regs, reg_base, instr, |b: i32, a: i32| b ^ a),

            RegOpcode::Inot => {
                // rA = ~rB (C unused)
                let b = match regs.get_reg(reg_base, instr.b()) {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                if let Err(e) = regs.set_reg(reg_base, instr.a(), Value::i32(!b)) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            // =========================================================
            // Float Arithmetic
            // =========================================================
            RegOpcode::Fadd => float_binop!(regs, reg_base, instr, |b: f64, a: f64| b + a),
            RegOpcode::Fsub => float_binop!(regs, reg_base, instr, |b: f64, a: f64| b - a),
            RegOpcode::Fmul => float_binop!(regs, reg_base, instr, |b: f64, a: f64| b * a),
            RegOpcode::Fdiv => float_binop!(regs, reg_base, instr, |b: f64, a: f64| b / a),

            RegOpcode::Fneg => {
                // rA = -rB (C unused)
                let b = match regs.get_reg(reg_base, instr.b()).and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                if let Err(e) = regs.set_reg(reg_base, instr.a(), Value::f64(-b)) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::Fpow => float_binop!(regs, reg_base, instr, |b: f64, a: f64| b.powf(a)),
            RegOpcode::Fmod => float_binop!(regs, reg_base, instr, |b: f64, a: f64| b % a),

            _ => RegOpcodeResult::runtime_error(format!(
                "Not an arithmetic opcode: {:?}",
                opcode
            )),
        }
    }
}
