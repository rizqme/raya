//! Register-based exception handling opcode handlers

use crate::compiler::bytecode::reg_opcode::{RegInstr, RegOpcode};
use crate::vm::interpreter::reg_execution::RegOpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{Object, RayaString};
use crate::vm::register_file::RegisterFile;
use crate::vm::scheduler::{ExceptionHandler, Task};
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_reg_exception_ops(
        &mut self,
        task: &Arc<Task>,
        regs: &mut RegisterFile,
        reg_base: usize,
        instr: RegInstr,
        extra: u32,
        frame_count: usize,
    ) -> RegOpcodeResult {
        let opcode = match instr.opcode() {
            Some(op) => op,
            None => return RegOpcodeResult::error(VmError::InvalidOpcode(instr.opcode_byte())),
        };

        match opcode {
            RegOpcode::Try => {
                // A = catch exception dest register
                // extra high 16 bits = catch_ip (absolute, 0xFFFF = no catch)
                // extra low 16 bits = finally_ip (absolute, 0xFFFF = no finally)
                let dest_reg = instr.a();
                let catch_ip = (extra >> 16) & 0xFFFF;
                let finally_ip = extra & 0xFFFF;

                let catch_offset = if catch_ip == 0xFFFF { -1 } else { catch_ip as i32 };
                let finally_offset = if finally_ip == 0xFFFF { -1 } else { finally_ip as i32 };

                let handler = ExceptionHandler {
                    catch_offset,
                    finally_offset,
                    stack_size: 0, // not used in register mode
                    frame_count,
                    mutex_count: task.held_mutex_count(),
                    catch_reg: dest_reg,
                };
                task.push_exception_handler(handler);
                RegOpcodeResult::Continue
            }

            RegOpcode::EndTry => {
                task.pop_exception_handler();
                RegOpcodeResult::Continue
            }

            RegOpcode::Throw => {
                // rA = exception value to throw
                let exception = match regs.get_reg(reg_base, instr.a()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                // If exception is an Error object, set its stack property
                if exception.is_ptr() {
                    if let Some(obj_ptr) = unsafe { exception.as_ptr::<Object>() } {
                        let obj = unsafe { &mut *obj_ptr.as_ptr() };
                        let classes = self.classes.read();

                        if let Some(class) = classes.get_class(obj.class_id) {
                            let is_error = class.name == "Error"
                                || class.name == "TypeError"
                                || class.name == "RangeError"
                                || class.name == "ReferenceError"
                                || class.name == "SyntaxError"
                                || class.name == "ChannelClosedError"
                                || class.name == "AssertionError"
                                || class.parent_id.is_some();

                            if is_error && obj.fields.len() >= 3 {
                                let error_name =
                                    if let Some(name_ptr) =
                                        unsafe { obj.fields[1].as_ptr::<RayaString>() }
                                    {
                                        unsafe { &*name_ptr.as_ptr() }.data.clone()
                                    } else {
                                        "Error".to_string()
                                    };

                                let error_message =
                                    if let Some(msg_ptr) =
                                        unsafe { obj.fields[0].as_ptr::<RayaString>() }
                                    {
                                        unsafe { &*msg_ptr.as_ptr() }.data.clone()
                                    } else {
                                        String::new()
                                    };

                                drop(classes);

                                let stack_trace =
                                    task.build_stack_trace(&error_name, &error_message);
                                let raya_string = RayaString::new(stack_trace);
                                let gc_ptr = self.gc.lock().allocate(raya_string);
                                let stack_value = unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                    )
                                };
                                obj.fields[2] = stack_value;
                            }
                        }
                    }
                }

                // Extract error message for VmError
                let error_msg = if exception.is_ptr() {
                    if let Some(obj_ptr) = unsafe { exception.as_ptr::<Object>() } {
                        let obj = unsafe { &*obj_ptr.as_ptr() };
                        if !obj.fields.is_empty() {
                            if let Some(msg_ptr) =
                                unsafe { obj.fields[0].as_ptr::<RayaString>() }
                            {
                                let msg = unsafe { &*msg_ptr.as_ptr() }.data.clone();
                                if msg.is_empty() {
                                    "throw".to_string()
                                } else {
                                    msg
                                }
                            } else {
                                "throw".to_string()
                            }
                        } else {
                            "throw".to_string()
                        }
                    } else {
                        "throw".to_string()
                    }
                } else {
                    "throw".to_string()
                };

                task.set_exception(exception);
                RegOpcodeResult::Error(VmError::RuntimeError(error_msg))
            }

            RegOpcode::Rethrow => {
                if let Some(exception) = task.caught_exception() {
                    task.set_exception(exception);
                    RegOpcodeResult::Error(VmError::RuntimeError("rethrow".to_string()))
                } else {
                    RegOpcodeResult::runtime_error(
                        "RETHROW with no active exception",
                    )
                }
            }

            _ => RegOpcodeResult::runtime_error(format!(
                "Not an exception opcode: {:?}",
                opcode
            )),
        }
    }
}
