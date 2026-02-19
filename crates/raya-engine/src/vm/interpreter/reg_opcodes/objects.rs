//! Register-based object opcode handlers

use crate::compiler::bytecode::reg_opcode::{RegInstr, RegOpcode};
use crate::vm::interpreter::reg_execution::RegOpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::Object;
use crate::vm::register_file::RegisterFile;
use crate::vm::value::Value;
use crate::vm::VmError;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_reg_object_ops(
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
            RegOpcode::New => {
                // rA = new Class; extra = class_id
                let dest_reg = instr.a();
                let class_id = extra as usize;

                let classes = self.classes.read();
                let class = match classes.get_class(class_id) {
                    Some(c) => c,
                    None => {
                        return RegOpcodeResult::runtime_error(format!(
                            "Invalid class index: {}",
                            class_id
                        ));
                    }
                };
                let field_count = class.field_count;
                drop(classes);

                let obj = Object::new(class_id, field_count);
                let gc_ptr = self.gc.lock().allocate(obj);
                let value = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, value) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::LoadField => {
                // rA = rB.field[C]
                let dest_reg = instr.a();
                let obj_reg = instr.b();
                let field_offset = instr.c() as usize;

                let obj_val = match regs.get_reg(reg_base, obj_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                if !obj_val.is_ptr() {
                    return RegOpcodeResult::runtime_error(
                        "Expected object for field access",
                    );
                }

                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);
                let obj_ptr = match unsafe { actual_obj.as_ptr::<Object>() } {
                    Some(p) => p,
                    None => {
                        return RegOpcodeResult::runtime_error(
                            "Expected object for field access",
                        );
                    }
                };
                let obj = unsafe { &*obj_ptr.as_ptr() };
                let value = match obj.get_field(field_offset) {
                    Some(v) => v,
                    None => {
                        return RegOpcodeResult::runtime_error(format!(
                            "Field offset {} out of bounds (class_id={})",
                            field_offset, obj.class_id
                        ));
                    }
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, value) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::StoreField => {
                // rA.field[B] = rC
                let obj_reg = instr.a();
                let field_offset = instr.b() as usize;
                let value_reg = instr.c();

                let obj_val = match regs.get_reg(reg_base, obj_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let value = match regs.get_reg(reg_base, value_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                if !obj_val.is_ptr() {
                    return RegOpcodeResult::runtime_error(
                        "Expected object for field access",
                    );
                }

                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);
                let obj_ptr = match unsafe { actual_obj.as_ptr::<Object>() } {
                    Some(p) => p,
                    None => {
                        return RegOpcodeResult::runtime_error(
                            "Expected object for field access",
                        );
                    }
                };
                let obj = unsafe { &mut *obj_ptr.as_ptr() };
                if let Err(e) = obj.set_field(field_offset, value) {
                    return RegOpcodeResult::runtime_error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::OptionalField => {
                // rA = rB?.field[C]
                let dest_reg = instr.a();
                let obj_reg = instr.b();
                let field_offset = instr.c() as usize;

                let obj_val = match regs.get_reg(reg_base, obj_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                // Optional chaining: null → null
                if obj_val.is_null() {
                    if let Err(e) = regs.set_reg(reg_base, dest_reg, Value::null()) {
                        return RegOpcodeResult::Error(e);
                    }
                    return RegOpcodeResult::Continue;
                }

                if !obj_val.is_ptr() {
                    return RegOpcodeResult::runtime_error(
                        "Expected object or null for optional field access",
                    );
                }

                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);
                let obj_ptr = match unsafe { actual_obj.as_ptr::<Object>() } {
                    Some(p) => p,
                    None => {
                        return RegOpcodeResult::runtime_error(
                            "Expected object for optional field access",
                        );
                    }
                };
                let obj = unsafe { &*obj_ptr.as_ptr() };
                let value = match obj.get_field(field_offset) {
                    Some(v) => v,
                    None => {
                        return RegOpcodeResult::runtime_error(format!(
                            "Field offset {} out of bounds",
                            field_offset
                        ));
                    }
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, value) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::ObjectLiteral => {
                // rA = { fields from rB..rB+C-1 }; extra = class_id
                let dest_reg = instr.a();
                let field_base = instr.b();
                let field_count = instr.c() as usize;
                let class_id = extra as usize;

                let mut obj = Object::new(class_id, field_count);
                for i in 0..field_count {
                    let val = match regs.get_reg(reg_base, field_base.wrapping_add(i as u8)) {
                        Ok(v) => v,
                        Err(e) => return RegOpcodeResult::Error(e),
                    };
                    if let Err(e) = obj.set_field(i, val) {
                        return RegOpcodeResult::runtime_error(e);
                    }
                }

                let gc_ptr = self.gc.lock().allocate(obj);
                let value = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, value) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::LoadStatic => {
                // rA = static[Bx]  (ABx format)
                // Bx encodes: high 8 bits = class_index, low 8 bits = field_offset
                let dest_reg = instr.a();
                let bx = instr.bx();
                let class_index = (bx >> 8) as usize;
                let field_offset = (bx & 0xFF) as usize;

                let classes = self.classes.read();
                let class = match classes.get_class(class_index) {
                    Some(c) => c,
                    None => {
                        return RegOpcodeResult::runtime_error(format!(
                            "Invalid class index: {}",
                            class_index
                        ));
                    }
                };
                let value = class.get_static_field(field_offset).unwrap_or(Value::null());
                drop(classes);

                if let Err(e) = regs.set_reg(reg_base, dest_reg, value) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::StoreStatic => {
                // static[Bx] = rA  (ABx format)
                // Bx encodes: high 8 bits = class_index, low 8 bits = field_offset
                let src_reg = instr.a();
                let bx = instr.bx();
                let class_index = (bx >> 8) as usize;
                let field_offset = (bx & 0xFF) as usize;

                let value = match regs.get_reg(reg_base, src_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                let mut classes = self.classes.write();
                let class = match classes.get_class_mut(class_index) {
                    Some(c) => c,
                    None => {
                        return RegOpcodeResult::runtime_error(format!(
                            "Invalid class index: {}",
                            class_index
                        ));
                    }
                };
                if let Err(e) = class.set_static_field(field_offset, value) {
                    return RegOpcodeResult::runtime_error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::InstanceOf => {
                // rA = rB instanceof class; extra = class_id
                let dest_reg = instr.a();
                let obj_reg = instr.b();
                let class_index = extra as usize;

                let obj_val = match regs.get_reg(reg_base, obj_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                let result = if obj_val.is_ptr() {
                    if let Some(obj_ptr) = unsafe { obj_val.as_ptr::<Object>() } {
                        let obj = unsafe { &*obj_ptr.as_ptr() };
                        let classes = self.classes.read();
                        let mut current_class_id = Some(obj.class_id);
                        let mut matches = false;
                        while let Some(cid) = current_class_id {
                            if cid == class_index {
                                matches = true;
                                break;
                            }
                            if let Some(class) = classes.get_class(cid) {
                                current_class_id = class.parent_id;
                            } else {
                                break;
                            }
                        }
                        matches
                    } else {
                        false
                    }
                } else {
                    false
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, Value::bool(result)) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::Cast => {
                // rA = rB as class; extra = class_id
                let dest_reg = instr.a();
                let obj_reg = instr.b();
                let class_index = extra as usize;

                let obj_val = match regs.get_reg(reg_base, obj_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                // null can be cast to any type
                if obj_val.is_null() {
                    if let Err(e) = regs.set_reg(reg_base, dest_reg, obj_val) {
                        return RegOpcodeResult::Error(e);
                    }
                    return RegOpcodeResult::Continue;
                }

                let valid_cast = if obj_val.is_ptr() {
                    if let Some(obj_ptr) = unsafe { obj_val.as_ptr::<Object>() } {
                        let obj = unsafe { &*obj_ptr.as_ptr() };
                        let classes = self.classes.read();
                        let mut current_class_id = Some(obj.class_id);
                        let mut matches = false;
                        while let Some(cid) = current_class_id {
                            if cid == class_index {
                                matches = true;
                                break;
                            }
                            if let Some(class) = classes.get_class(cid) {
                                current_class_id = class.parent_id;
                            } else {
                                break;
                            }
                        }
                        matches
                    } else {
                        false
                    }
                } else {
                    false
                };

                if valid_cast {
                    if let Err(e) = regs.set_reg(reg_base, dest_reg, obj_val) {
                        return RegOpcodeResult::Error(e);
                    }
                    RegOpcodeResult::Continue
                } else {
                    RegOpcodeResult::Error(VmError::TypeError(format!(
                        "Cannot cast object to class index {}",
                        class_index
                    )))
                }
            }

            _ => RegOpcodeResult::runtime_error(format!(
                "Not an object opcode: {:?}",
                opcode
            )),
        }
    }
}
