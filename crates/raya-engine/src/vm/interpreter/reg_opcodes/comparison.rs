//! Register-based comparison and logical opcode handlers

use crate::compiler::bytecode::reg_opcode::{RegInstr, RegOpcode};
use crate::vm::interpreter::core::value_to_f64;
use crate::vm::interpreter::reg_execution::RegOpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::register_file::RegisterFile;
use crate::vm::value::Value;
use crate::vm::VmError;

/// Helper macro for integer comparison: rA = rB <op> rC
macro_rules! int_cmp {
    ($regs:expr, $base:expr, $instr:expr, $op:tt) => {{
        let b = match $regs.get_reg($base, $instr.b()) {
            Ok(v) => v.as_i32().unwrap_or(0),
            Err(e) => return RegOpcodeResult::Error(e),
        };
        let c = match $regs.get_reg($base, $instr.c()) {
            Ok(v) => v.as_i32().unwrap_or(0),
            Err(e) => return RegOpcodeResult::Error(e),
        };
        if let Err(e) = $regs.set_reg($base, $instr.a(), Value::bool(b $op c)) {
            return RegOpcodeResult::Error(e);
        }
        RegOpcodeResult::Continue
    }};
}

/// Helper macro for float comparison: rA = rB <op> rC
macro_rules! float_cmp {
    ($regs:expr, $base:expr, $instr:expr, $op:tt) => {{
        let b = match $regs.get_reg($base, $instr.b()).and_then(|v| value_to_f64(v)) {
            Ok(v) => v,
            Err(e) => return RegOpcodeResult::Error(e),
        };
        let c = match $regs.get_reg($base, $instr.c()).and_then(|v| value_to_f64(v)) {
            Ok(v) => v,
            Err(e) => return RegOpcodeResult::Error(e),
        };
        if let Err(e) = $regs.set_reg($base, $instr.a(), Value::bool(b $op c)) {
            return RegOpcodeResult::Error(e);
        }
        RegOpcodeResult::Continue
    }};
}

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_reg_comparison_ops(
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
            // Integer Comparison
            // =========================================================
            RegOpcode::Ieq => int_cmp!(regs, reg_base, instr, ==),
            RegOpcode::Ine => int_cmp!(regs, reg_base, instr, !=),
            RegOpcode::Ilt => int_cmp!(regs, reg_base, instr, <),
            RegOpcode::Ile => int_cmp!(regs, reg_base, instr, <=),
            RegOpcode::Igt => int_cmp!(regs, reg_base, instr, >),
            RegOpcode::Ige => int_cmp!(regs, reg_base, instr, >=),

            // =========================================================
            // Float Comparison
            // =========================================================
            RegOpcode::Feq => float_cmp!(regs, reg_base, instr, ==),
            RegOpcode::Fne => float_cmp!(regs, reg_base, instr, !=),
            RegOpcode::Flt => float_cmp!(regs, reg_base, instr, <),
            RegOpcode::Fle => float_cmp!(regs, reg_base, instr, <=),
            RegOpcode::Fgt => float_cmp!(regs, reg_base, instr, >),
            RegOpcode::Fge => float_cmp!(regs, reg_base, instr, >=),

            // =========================================================
            // Generic Comparison (bitwise equality)
            // =========================================================
            RegOpcode::Eq | RegOpcode::StrictEq => {
                let b = match regs.get_reg(reg_base, instr.b()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let c = match regs.get_reg(reg_base, instr.c()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                if let Err(e) = regs.set_reg(reg_base, instr.a(), Value::bool(b.raw() == c.raw()))
                {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }
            RegOpcode::Ne | RegOpcode::StrictNe => {
                let b = match regs.get_reg(reg_base, instr.b()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let c = match regs.get_reg(reg_base, instr.c()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                if let Err(e) = regs.set_reg(reg_base, instr.a(), Value::bool(b.raw() != c.raw()))
                {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            // =========================================================
            // Logical
            // =========================================================
            RegOpcode::Not => {
                let b = match regs.get_reg(reg_base, instr.b()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                if let Err(e) = regs.set_reg(reg_base, instr.a(), Value::bool(!b.is_truthy())) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }
            RegOpcode::And => {
                let b = match regs.get_reg(reg_base, instr.b()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let result = if !b.is_truthy() {
                    b
                } else {
                    match regs.get_reg(reg_base, instr.c()) {
                        Ok(v) => v,
                        Err(e) => return RegOpcodeResult::Error(e),
                    }
                };
                if let Err(e) = regs.set_reg(reg_base, instr.a(), result) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }
            RegOpcode::Or => {
                let b = match regs.get_reg(reg_base, instr.b()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let result = if b.is_truthy() {
                    b
                } else {
                    match regs.get_reg(reg_base, instr.c()) {
                        Ok(v) => v,
                        Err(e) => return RegOpcodeResult::Error(e),
                    }
                };
                if let Err(e) = regs.set_reg(reg_base, instr.a(), result) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::Typeof => {
                let b = match regs.get_reg(reg_base, instr.b()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                // Return type tag as integer:
                // 0 = null, 1 = bool, 2 = i32, 3 = f64, 4 = ptr (string/object)
                let type_id = if b.is_null() {
                    0i32
                } else if b.is_bool() {
                    1
                } else if b.is_i32() {
                    2
                } else if b.is_f64() {
                    3
                } else {
                    4
                };
                if let Err(e) = regs.set_reg(reg_base, instr.a(), Value::i32(type_id)) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            _ => RegOpcodeResult::runtime_error(format!(
                "Not a comparison/logical opcode: {:?}",
                opcode
            )),
        }
    }
}
