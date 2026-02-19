//! Register-based constant and move opcode handlers

use crate::compiler::bytecode::reg_opcode::{RegInstr, RegOpcode};
use crate::compiler::Module;
use crate::vm::interpreter::reg_execution::RegOpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::RayaString;
use crate::vm::register_file::RegisterFile;
use crate::vm::value::Value;
use crate::vm::VmError;
use parking_lot::RwLock;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_reg_constant_ops(
        &mut self,
        regs: &mut RegisterFile,
        reg_base: usize,
        instr: RegInstr,
        module: &Module,
        globals: &RwLock<Vec<Value>>,
    ) -> RegOpcodeResult {
        let opcode = match instr.opcode() {
            Some(op) => op,
            None => return RegOpcodeResult::error(VmError::InvalidOpcode(instr.opcode_byte())),
        };

        match opcode {
            RegOpcode::Nop => RegOpcodeResult::Continue,

            RegOpcode::Move => {
                // rA = rB
                let val = match regs.get_reg(reg_base, instr.b()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                if let Err(e) = regs.set_reg(reg_base, instr.a(), val) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::LoadNil => {
                if let Err(e) = regs.set_reg(reg_base, instr.a(), Value::null()) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::LoadTrue => {
                if let Err(e) = regs.set_reg(reg_base, instr.a(), Value::bool(true)) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::LoadFalse => {
                if let Err(e) = regs.set_reg(reg_base, instr.a(), Value::bool(false)) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::LoadInt => {
                // rA = sBx (signed 16-bit immediate)
                let val = instr.sbx() as i32;
                if let Err(e) = regs.set_reg(reg_base, instr.a(), Value::i32(val)) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::LoadConst => {
                // rA = constants[Bx]
                // Bx encoding: bits 15-14 = pool type (0=int, 1=float, 2=string)
                //               bits 13-0  = index within that pool
                let bx = instr.bx();
                let pool_type = (bx >> 14) & 0x3;
                let idx = (bx & 0x3FFF) as u32;

                let val = match pool_type {
                    0 => {
                        // Integer constant
                        match module.constants.get_integer(idx) {
                            Some(i) => Value::i32(i),
                            None => {
                                return RegOpcodeResult::runtime_error(format!(
                                    "integer constant index {} out of range",
                                    idx
                                ));
                            }
                        }
                    }
                    1 => {
                        // Float constant
                        match module.constants.get_float(idx) {
                            Some(f) => Value::f64(f),
                            None => {
                                return RegOpcodeResult::runtime_error(format!(
                                    "float constant index {} out of range",
                                    idx
                                ));
                            }
                        }
                    }
                    2 => {
                        // String constant — allocate via GC
                        match module.constants.get_string(idx) {
                            Some(s) => {
                                let raya_str = RayaString::new(s.to_string());
                                let gc_ptr = self.gc.lock().allocate(raya_str);
                                unsafe {
                                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                                }
                            }
                            None => {
                                return RegOpcodeResult::runtime_error(format!(
                                    "string constant index {} out of range",
                                    idx
                                ));
                            }
                        }
                    }
                    _ => {
                        return RegOpcodeResult::runtime_error(format!(
                            "invalid constant pool type: {}",
                            pool_type
                        ));
                    }
                };

                if let Err(e) = regs.set_reg(reg_base, instr.a(), val) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::LoadGlobal => {
                // rA = globals[Bx]
                let idx = instr.bx() as usize;
                let globals_guard = globals.read();
                if idx >= globals_guard.len() {
                    return RegOpcodeResult::runtime_error(format!(
                        "global index {} out of range (max {})",
                        idx,
                        globals_guard.len()
                    ));
                }
                let val = globals_guard[idx];
                drop(globals_guard);
                if let Err(e) = regs.set_reg(reg_base, instr.a(), val) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::StoreGlobal => {
                // globals[Bx] = rA
                let idx = instr.bx() as usize;
                let val = match regs.get_reg(reg_base, instr.a()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let mut globals_guard = globals.write();
                if idx >= globals_guard.len() {
                    globals_guard.resize(idx + 1, Value::null());
                }
                globals_guard[idx] = val;
                RegOpcodeResult::Continue
            }

            _ => RegOpcodeResult::runtime_error(format!(
                "Not a constant/move opcode: {:?}",
                opcode
            )),
        }
    }
}
