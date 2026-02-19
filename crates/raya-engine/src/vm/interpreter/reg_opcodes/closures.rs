//! Register-based closure opcode handlers

use crate::compiler::bytecode::reg_opcode::{RegInstr, RegOpcode};
use crate::vm::interpreter::reg_execution::RegOpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::Closure;
use crate::vm::register_file::RegisterFile;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;
use crate::vm::scheduler::Task;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_reg_closure_ops(
        &mut self,
        task: &Arc<Task>,
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
            RegOpcode::MakeClosure => {
                // rA = closure(func_id, captures from rB..rB+C-1); extra = func_id
                let dest_reg = instr.a();
                let capture_base = instr.b();
                let capture_count = instr.c() as usize;
                let func_id = extra as usize;

                let mut captures = Vec::with_capacity(capture_count);
                for i in 0..capture_count {
                    let val = match regs.get_reg(reg_base, capture_base.wrapping_add(i as u8)) {
                        Ok(v) => v,
                        Err(e) => return RegOpcodeResult::Error(e),
                    };
                    captures.push(val);
                }

                let closure = Closure::new(func_id, captures);
                let gc_ptr = self.gc.lock().allocate(closure);
                let value = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, value) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::LoadCaptured => {
                // rA = captured[Bx] (ABx format)
                let dest_reg = instr.a();
                let capture_index = instr.bx() as usize;

                let closure_val = match task.current_closure() {
                    Some(v) => v,
                    None => {
                        return RegOpcodeResult::runtime_error(
                            "LoadCaptured without active closure",
                        );
                    }
                };

                let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
                let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                let value = match closure.get_captured(capture_index) {
                    Some(v) => v,
                    None => {
                        return RegOpcodeResult::runtime_error(format!(
                            "Capture index {} out of bounds",
                            capture_index
                        ));
                    }
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, value) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::StoreCaptured => {
                // captured[Bx] = rA (ABx format)
                let src_reg = instr.a();
                let capture_index = instr.bx() as usize;

                let value = match regs.get_reg(reg_base, src_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                let closure_val = match task.current_closure() {
                    Some(v) => v,
                    None => {
                        return RegOpcodeResult::runtime_error(
                            "StoreCaptured without active closure",
                        );
                    }
                };

                let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
                let closure = unsafe { &mut *closure_ptr.unwrap().as_ptr() };
                if let Err(e) = closure.set_captured(capture_index, value) {
                    return RegOpcodeResult::runtime_error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::SetClosureCapture => {
                // rA.captures[B] = rC
                let closure_reg = instr.a();
                let capture_index = instr.b() as usize;
                let value_reg = instr.c();

                let closure_val = match regs.get_reg(reg_base, closure_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let value = match regs.get_reg(reg_base, value_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                if !closure_val.is_ptr() {
                    return RegOpcodeResult::runtime_error("Expected closure");
                }

                let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
                let closure = unsafe { &mut *closure_ptr.unwrap().as_ptr() };
                if let Err(e) = closure.set_captured(capture_index, value) {
                    return RegOpcodeResult::runtime_error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::NewRefCell => {
                // rA = RefCell(rB) (C unused)
                use crate::vm::object::RefCell;

                let dest_reg = instr.a();
                let initial_val = match regs.get_reg(reg_base, instr.b()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                let refcell = RefCell::new(initial_val);
                let gc_ptr = self.gc.lock().allocate(refcell);
                let value = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, value) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::LoadRefCell => {
                // rA = rB.value (load RefCell, C unused)
                use crate::vm::object::RefCell;

                let dest_reg = instr.a();
                let refcell_val = match regs.get_reg(reg_base, instr.b()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                if !refcell_val.is_ptr() {
                    return RegOpcodeResult::runtime_error("Expected RefCell");
                }

                let refcell_ptr = match unsafe { refcell_val.as_ptr::<RefCell>() } {
                    Some(p) => p,
                    None => return RegOpcodeResult::runtime_error("Expected RefCell"),
                };
                let refcell = unsafe { &*refcell_ptr.as_ptr() };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, refcell.get()) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::StoreRefCell => {
                // rA.value = rB (store RefCell, C unused)
                use crate::vm::object::RefCell;

                let refcell_val = match regs.get_reg(reg_base, instr.a()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let value = match regs.get_reg(reg_base, instr.b()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                if !refcell_val.is_ptr() {
                    return RegOpcodeResult::runtime_error("Expected RefCell");
                }

                let refcell_ptr = match unsafe { refcell_val.as_ptr::<RefCell>() } {
                    Some(p) => p,
                    None => return RegOpcodeResult::runtime_error("Expected RefCell"),
                };
                let refcell = unsafe { &mut *refcell_ptr.as_ptr() };
                refcell.set(value);

                RegOpcodeResult::Continue
            }

            _ => RegOpcodeResult::runtime_error(format!(
                "Not a closure opcode: {:?}",
                opcode
            )),
        }
    }
}
