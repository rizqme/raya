//! Register-based array and tuple opcode handlers

use crate::compiler::bytecode::reg_opcode::{RegInstr, RegOpcode};
use crate::vm::interpreter::reg_execution::RegOpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::Array;
use crate::vm::register_file::RegisterFile;
use crate::vm::value::Value;
use crate::vm::VmError;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_reg_array_ops(
        &mut self,
        regs: &mut RegisterFile,
        reg_base: usize,
        instr: RegInstr,
        extra: u32,
    ) -> RegOpcodeResult {
        let opcode = match instr.opcode() {
            Some(op) => op,
            None => return RegOpcodeResult::error(VmError::InvalidOpcode(instr.opcode_byte())),
        };

        match opcode {
            RegOpcode::NewArray => {
                // rA = new Array(rB); extra = type_id
                let dest_reg = instr.a();
                let len_reg = instr.b();
                let type_id = extra as usize;

                let len_val = match regs.get_reg(reg_base, len_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let len = if let Some(i) = len_val.as_i32() {
                    i as usize
                } else if let Some(f) = len_val.as_f64() {
                    f as usize
                } else {
                    0
                };

                let arr = Array::new(type_id, len);
                let gc_ptr = self.gc.lock().allocate(arr);
                let value = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, value) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::LoadElem => {
                // rA = rB[rC]
                let dest_reg = instr.a();
                let arr_reg = instr.b();
                let idx_reg = instr.c();

                let arr_val = match regs.get_reg(reg_base, arr_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let idx_val = match regs.get_reg(reg_base, idx_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                if !arr_val.is_ptr() {
                    return RegOpcodeResult::runtime_error("Expected array");
                }

                let index = if let Some(i) = idx_val.as_i32() {
                    i as usize
                } else if let Some(f) = idx_val.as_f64() {
                    f as usize
                } else {
                    0
                };

                let arr_ptr = match unsafe { arr_val.as_ptr::<Array>() } {
                    Some(p) => p,
                    None => return RegOpcodeResult::runtime_error("Expected array"),
                };
                let arr = unsafe { &*arr_ptr.as_ptr() };
                let value = match arr.get(index) {
                    Some(v) => v,
                    None => {
                        return RegOpcodeResult::runtime_error(format!(
                            "Array index {} out of bounds",
                            index
                        ));
                    }
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, value) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::StoreElem => {
                // rA[rB] = rC
                let arr_reg = instr.a();
                let idx_reg = instr.b();
                let value_reg = instr.c();

                let arr_val = match regs.get_reg(reg_base, arr_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let idx_val = match regs.get_reg(reg_base, idx_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let value = match regs.get_reg(reg_base, value_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                if !arr_val.is_ptr() {
                    return RegOpcodeResult::runtime_error("Expected array");
                }

                let index = if let Some(i) = idx_val.as_i32() {
                    i as usize
                } else if let Some(f) = idx_val.as_f64() {
                    f as usize
                } else {
                    0
                };

                let arr_ptr = match unsafe { arr_val.as_ptr::<Array>() } {
                    Some(p) => p,
                    None => return RegOpcodeResult::runtime_error("Expected array"),
                };
                let arr = unsafe { &mut *arr_ptr.as_ptr() };
                if let Err(e) = arr.set(index, value) {
                    return RegOpcodeResult::runtime_error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::ArrayLen => {
                // rA = rB.length (C unused)
                let dest_reg = instr.a();
                let arr_reg = instr.b();

                let arr_val = match regs.get_reg(reg_base, arr_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                if !arr_val.is_ptr() {
                    return RegOpcodeResult::runtime_error("Expected array");
                }

                let arr_ptr = match unsafe { arr_val.as_ptr::<Array>() } {
                    Some(p) => p,
                    None => return RegOpcodeResult::runtime_error("Expected array"),
                };
                let arr = unsafe { &*arr_ptr.as_ptr() };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, Value::i32(arr.len() as i32)) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::ArrayLiteral => {
                // rA = [rB, rB+1, ..., rB+C-1]; extra = type_id
                let dest_reg = instr.a();
                let elem_base = instr.b();
                let elem_count = instr.c() as usize;
                let type_id = extra as usize;

                let mut arr = Array::new(type_id, elem_count);
                for i in 0..elem_count {
                    let val = match regs.get_reg(reg_base, elem_base.wrapping_add(i as u8)) {
                        Ok(v) => v,
                        Err(e) => return RegOpcodeResult::Error(e),
                    };
                    if let Err(e) = arr.set(i, val) {
                        return RegOpcodeResult::runtime_error(e);
                    }
                }

                let gc_ptr = self.gc.lock().allocate(arr);
                let value = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, value) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::ArrayPush => {
                // rA.push(rB) (C unused)
                let arr_reg = instr.a();
                let elem_reg = instr.b();

                let arr_val = match regs.get_reg(reg_base, arr_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let element = match regs.get_reg(reg_base, elem_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                if !arr_val.is_ptr() {
                    return RegOpcodeResult::runtime_error("Expected array");
                }

                let arr_ptr = match unsafe { arr_val.as_ptr::<Array>() } {
                    Some(p) => p,
                    None => return RegOpcodeResult::runtime_error("Expected array"),
                };
                let arr = unsafe { &mut *arr_ptr.as_ptr() };
                arr.push(element);
                RegOpcodeResult::Continue
            }

            RegOpcode::ArrayPop => {
                // rA = rB.pop() (C unused)
                let dest_reg = instr.a();
                let arr_reg = instr.b();

                let arr_val = match regs.get_reg(reg_base, arr_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                if !arr_val.is_ptr() {
                    return RegOpcodeResult::runtime_error("Expected array");
                }

                let arr_ptr = match unsafe { arr_val.as_ptr::<Array>() } {
                    Some(p) => p,
                    None => return RegOpcodeResult::runtime_error("Expected array"),
                };
                let arr = unsafe { &mut *arr_ptr.as_ptr() };
                let value = arr.pop().unwrap_or(Value::null());

                if let Err(e) = regs.set_reg(reg_base, dest_reg, value) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::TupleLiteral => {
                // rA = (rB, rB+1, ..., rB+C-1); extra = type_id
                // Tuples are implemented as arrays
                let dest_reg = instr.a();
                let elem_base = instr.b();
                let elem_count = instr.c() as usize;
                let type_id = extra as usize;

                let mut arr = Array::new(type_id, elem_count);
                for i in 0..elem_count {
                    let val = match regs.get_reg(reg_base, elem_base.wrapping_add(i as u8)) {
                        Ok(v) => v,
                        Err(e) => return RegOpcodeResult::Error(e),
                    };
                    if let Err(e) = arr.set(i, val) {
                        return RegOpcodeResult::runtime_error(e);
                    }
                }

                let gc_ptr = self.gc.lock().allocate(arr);
                let value = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, value) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::TupleGet => {
                // rA = rB[C] (constant tuple index)
                let dest_reg = instr.a();
                let tuple_reg = instr.b();
                let index = instr.c() as usize;

                let tuple_val = match regs.get_reg(reg_base, tuple_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                if !tuple_val.is_ptr() {
                    return RegOpcodeResult::runtime_error("Expected tuple");
                }

                let arr_ptr = match unsafe { tuple_val.as_ptr::<Array>() } {
                    Some(p) => p,
                    None => return RegOpcodeResult::runtime_error("Expected tuple"),
                };
                let arr = unsafe { &*arr_ptr.as_ptr() };
                let value = match arr.get(index) {
                    Some(v) => v,
                    None => {
                        return RegOpcodeResult::runtime_error(format!(
                            "Tuple index {} out of bounds",
                            index
                        ));
                    }
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, value) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            _ => RegOpcodeResult::runtime_error(format!(
                "Not an array opcode: {:?}",
                opcode
            )),
        }
    }
}
