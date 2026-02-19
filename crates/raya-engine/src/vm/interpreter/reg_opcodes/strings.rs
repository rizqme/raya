//! Register-based string opcode handlers

use crate::compiler::bytecode::reg_opcode::{RegInstr, RegOpcode};
use crate::vm::interpreter::reg_execution::RegOpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::RayaString;
use crate::vm::register_file::RegisterFile;
use crate::vm::value::Value;
use crate::vm::VmError;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_reg_string_ops(
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
            RegOpcode::Sconcat => {
                // rA = rB + rC (string concatenation)
                let b = match regs.get_reg(reg_base, instr.b()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let c = match regs.get_reg(reg_base, instr.c()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                let b_str = value_to_string(b);
                let c_str = value_to_string(c);
                let result = format!("{}{}", b_str, c_str);
                let raya_str = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_str);
                let val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };

                if let Err(e) = regs.set_reg(reg_base, instr.a(), val) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::Slen => {
                // rA = rB.length
                let b = match regs.get_reg(reg_base, instr.b()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let len = if b.is_ptr() {
                    if let Some(ptr) = (unsafe { b.as_ptr::<RayaString>() }) {
                        let s = unsafe { &*ptr.as_ptr() };
                        s.len() as i32
                    } else {
                        0
                    }
                } else {
                    0
                };
                if let Err(e) = regs.set_reg(reg_base, instr.a(), Value::i32(len)) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            // String comparisons
            RegOpcode::Seq => string_cmp(regs, reg_base, instr, |a, b| a == b),
            RegOpcode::Sne => string_cmp(regs, reg_base, instr, |a, b| a != b),
            RegOpcode::Slt => string_cmp(regs, reg_base, instr, |a, b| a < b),
            RegOpcode::Sle => string_cmp(regs, reg_base, instr, |a, b| a <= b),
            RegOpcode::Sgt => string_cmp(regs, reg_base, instr, |a, b| a > b),
            RegOpcode::Sge => string_cmp(regs, reg_base, instr, |a, b| a >= b),

            RegOpcode::ToString => {
                // rA = toString(rB)
                let b = match regs.get_reg(reg_base, instr.b()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let s = value_to_string(b);
                let raya_str = RayaString::new(s);
                let gc_ptr = self.gc.lock().allocate(raya_str);
                let val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };
                if let Err(e) = regs.set_reg(reg_base, instr.a(), val) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            _ => RegOpcodeResult::runtime_error(format!(
                "Not a string opcode: {:?}",
                opcode
            )),
        }
    }
}

fn value_to_string(val: Value) -> String {
    if val.is_null() {
        "null".to_string()
    } else if val.is_bool() {
        if val.as_bool().unwrap_or(false) {
            "true".to_string()
        } else {
            "false".to_string()
        }
    } else if val.is_i32() {
        val.as_i32().unwrap_or(0).to_string()
    } else if val.is_f64() {
        let f = val.as_f64().unwrap_or(0.0);
        if f == f.floor() && f.abs() < 1e15 {
            format!("{}", f as i64)
        } else {
            format!("{}", f)
        }
    } else if val.is_ptr() {
        if let Some(ptr) = (unsafe { val.as_ptr::<RayaString>() }) {
            let s = unsafe { &*ptr.as_ptr() };
            s.data.clone()
        } else {
            "[object]".to_string()
        }
    } else {
        "undefined".to_string()
    }
}

fn string_cmp(
    regs: &mut RegisterFile,
    reg_base: usize,
    instr: RegInstr,
    cmp: fn(&str, &str) -> bool,
) -> RegOpcodeResult {
    let b = match regs.get_reg(reg_base, instr.b()) {
        Ok(v) => v,
        Err(e) => return RegOpcodeResult::Error(e),
    };
    let c = match regs.get_reg(reg_base, instr.c()) {
        Ok(v) => v,
        Err(e) => return RegOpcodeResult::Error(e),
    };

    let b_str = extract_str(b);
    let c_str = extract_str(c);
    let result = cmp(&b_str, &c_str);

    if let Err(e) = regs.set_reg(reg_base, instr.a(), Value::bool(result)) {
        return RegOpcodeResult::Error(e);
    }
    RegOpcodeResult::Continue
}

fn extract_str(val: Value) -> String {
    if val.is_ptr() {
        if let Some(ptr) = (unsafe { val.as_ptr::<RayaString>() }) {
            let s = unsafe { &*ptr.as_ptr() };
            return s.data.clone();
        }
    }
    String::new()
}
