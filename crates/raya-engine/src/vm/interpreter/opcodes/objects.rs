//! Object opcode handlers: New, LoadField, StoreField, OptionalField, LoadFieldFast, StoreFieldFast, ObjectLiteral, InitObject, BindMethod

use crate::compiler::Opcode;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{BoundMethod, Object};
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_object_ops(
        &mut self,
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::New => {
                self.safepoint.poll();
                let class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let classes = self.classes.read();
                let class = match classes.get_class(class_index) {
                    Some(c) => c,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid class index: {}",
                            class_index
                        )));
                    }
                };
                let field_count = class.field_count;
                drop(classes);

                let obj = Object::new(class_index, field_count);
                let gc_ptr = self.gc.lock().allocate(obj);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadField => {
                let field_offset = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !obj_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected object for field access".to_string(),
                    ));
                }

                // Check if the object is a proxy - if so, unwrap to target
                // TODO: Full trap support would call handler.get(target, fieldName)
                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                let value = match obj.get_field(field_offset) {
                    Some(v) => v,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Field offset {} out of bounds (class_id={})",
                            field_offset, obj.class_id
                        )));
                    }
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreField => {
                let field_offset = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !obj_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected object for field access".to_string(),
                    ));
                }

                // Check if the object is a proxy - if so, unwrap to target
                // TODO: Full trap support would call handler.set(target, fieldName, value)
                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
                let obj = unsafe { &mut *obj_ptr.unwrap().as_ptr() };
                if let Err(e) = obj.set_field(field_offset, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                OpcodeResult::Continue
            }

            Opcode::OptionalField => {
                let field_offset = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // If null, return null (optional chaining semantics)
                if obj_val.is_null() {
                    if let Err(e) = stack.push(Value::null()) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }

                if !obj_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected object or null for optional field access".to_string(),
                    ));
                }

                // Check if the object is a proxy - if so, unwrap to target
                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                let value = match obj.get_field(field_offset) {
                    Some(v) => v,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Field offset {} out of bounds",
                            field_offset
                        )));
                    }
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadFieldFast => {
                let field_offset = match Self::read_u8(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !obj_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected object for field access".to_string(),
                    ));
                }

                // Check if the object is a proxy - if so, unwrap to target
                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                let value = match obj.get_field(field_offset) {
                    Some(v) => v,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Field offset {} out of bounds",
                            field_offset
                        )));
                    }
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreFieldFast => {
                let field_offset = match Self::read_u8(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !obj_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected object for field access".to_string(),
                    ));
                }

                // Check if the object is a proxy - if so, unwrap to target
                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
                let obj = unsafe { &mut *obj_ptr.unwrap().as_ptr() };
                if let Err(e) = obj.set_field(field_offset, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                OpcodeResult::Continue
            }

            Opcode::ObjectLiteral => {
                self.safepoint.poll();
                let class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let field_count = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let obj = Object::new(class_index, field_count);
                let gc_ptr = self.gc.lock().allocate(obj);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::InitObject => {
                let field_offset = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.peek() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !obj_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected object for field initialization".to_string(),
                    ));
                }

                let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
                let obj = unsafe { &mut *obj_ptr.unwrap().as_ptr() };
                if let Err(e) = obj.set_field(field_offset, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                OpcodeResult::Continue
            }

            Opcode::BindMethod => {
                let method_slot = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !obj_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected object for method binding".to_string(),
                    ));
                }

                let obj = unsafe { &*obj_val.as_ptr::<Object>().unwrap().as_ptr() };
                let classes = self.classes.read();
                let class = match classes.get_class(obj.class_id) {
                    Some(c) => c,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid class index: {}",
                            obj.class_id
                        )));
                    }
                };
                let func_id = match class.vtable.get_method(method_slot) {
                    Some(fid) => fid,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid method slot: {} for class {}",
                            method_slot, class.name
                        )));
                    }
                };
                drop(classes);

                let bm = BoundMethod {
                    receiver: obj_val,
                    func_id,
                };
                let gc_ptr = self.gc.lock().allocate(bm);
                let value = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            _ => OpcodeResult::Error(VmError::RuntimeError(format!(
                "Unexpected opcode in exec_object_ops: {:?}",
                opcode
            ))),
        }
    }
}
