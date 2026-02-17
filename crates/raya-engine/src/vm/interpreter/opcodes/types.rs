//! Type operation opcode handlers: InstanceOf, Cast, Typeof, JsonGet, JsonSet,
//! NewMutex, NewChannel, LoadStatic, StoreStatic

use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{ChannelObject, Object, RayaString};
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use crate::compiler::{Module, Opcode};
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_type_ops(
        &mut self,
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        module: &Module,
        _task: &Arc<Task>,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::InstanceOf => {
                let class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
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

                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Cast => {
                let class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Null check - null can be cast to any type (it represents absence of value)
                if obj_val.is_null() {
                    if let Err(e) = stack.push(obj_val) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }

                // Check if object is an instance of the target class
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
                    // Cast is valid, push object back
                    if let Err(e) = stack.push(obj_val) {
                        return OpcodeResult::Error(e);
                    }
                    OpcodeResult::Continue
                } else {
                    // Cast failed - throw TypeError
                    OpcodeResult::Error(VmError::TypeError(format!(
                        "Cannot cast object to class index {}",
                        class_index
                    )))
                }
            }

            // =========================================================
            // JSON Operations (Duck Typing)
            // =========================================================
            Opcode::JsonGet => {
                use crate::vm::json::{self, JsonValue};

                // Read property name index from constant pool
                let prop_index = match Self::read_u32(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Get property name from constant pool
                let prop_name = match module.constants.get_string(prop_index) {
                    Some(s) => s.to_string(),
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid constant index {} for JSON property",
                            prop_index
                        )));
                    }
                };

                // Pop the JSON object from stack
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Handle different value types
                let result = if obj_val.is_null() {
                    // Accessing property on null returns null
                    Value::null()
                } else if obj_val.is_ptr() {
                    // Try to access as JsonValue (stored on heap by json_to_value)
                    let ptr = unsafe { obj_val.as_ptr::<JsonValue>() };
                    if let Some(json_ptr) = ptr {
                        let json_val = unsafe { &*json_ptr.as_ptr() };
                        // Use JsonValue's get_property method for duck typing
                        let prop_val = json_val.get_property(&prop_name);
                        // Convert the result to a Value
                        json::json_to_value(&prop_val, &mut self.gc.lock())
                    } else {
                        return OpcodeResult::Error(VmError::TypeError(
                            "Expected JSON object for property access".to_string(),
                        ));
                    }
                } else {
                    // Primitive types don't support property access
                    Value::null()
                };

                if let Err(e) = stack.push(result) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::JsonSet => {
                use crate::vm::json::{self, JsonValue};

                // Read property name index from constant pool
                let prop_index = match Self::read_u32(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Get property name from constant pool
                let prop_name = match module.constants.get_string(prop_index) {
                    Some(s) => s.to_string(),
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid constant index {} for JSON property",
                            prop_index
                        )));
                    }
                };

                // Pop value and object from stack
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
                        "Expected JSON object for property assignment".to_string(),
                    ));
                }

                // Try to access as JsonValue and set property
                let ptr = unsafe { obj_val.as_ptr::<JsonValue>() };
                if let Some(json_ptr) = ptr {
                    let json_val = unsafe { &*json_ptr.as_ptr() };
                    // Get the inner HashMap from the JsonValue::Object
                    if let Some(obj_ptr) = json_val.as_object() {
                        let map = unsafe { &mut *obj_ptr.as_ptr() };
                        // Convert Value to JsonValue
                        let new_json_val = json::value_to_json(value, &mut self.gc.lock());
                        map.insert(prop_name, new_json_val);
                    } else {
                        return OpcodeResult::Error(VmError::TypeError(
                            "Expected JSON object for property assignment".to_string(),
                        ));
                    }
                } else {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected JSON object for property assignment".to_string(),
                    ));
                }

                OpcodeResult::Continue
            }

            // =========================================================
            // Mutex Creation
            // =========================================================
            Opcode::NewMutex => {
                let (mutex_id, _) = self.mutex_registry.create_mutex();
                if let Err(e) = stack.push(Value::i64(mutex_id.as_u64() as i64)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Channel Creation
            // =========================================================
            Opcode::NewChannel => {
                self.safepoint.poll();
                let capacity_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let capacity = capacity_val.as_i32().unwrap_or(0) as usize;
                let channel = ChannelObject::new(capacity);
                let gc_ptr = self.gc.lock().allocate(channel);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Static Fields
            // =========================================================
            Opcode::LoadStatic => {
                let class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let field_offset = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Get static field from the class registry
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
                let value = class.get_static_field(field_offset).unwrap_or(Value::null());
                drop(classes);

                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreStatic => {
                let class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let field_offset = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Set static field in the class registry
                let mut classes = self.classes.write();
                let class = match classes.get_class_mut(class_index) {
                    Some(c) => c,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid class index: {}",
                            class_index
                        )));
                    }
                };
                if let Err(e) = class.set_static_field(field_offset, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Type Operators
            // =========================================================
            Opcode::Typeof => {
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let type_str = if value.is_null() {
                    "null"
                } else if value.is_bool() {
                    "boolean"
                } else if value.is_i32() || value.is_i64() || value.is_f64() {
                    "number"
                } else if value.is_ptr() {
                    // Check if it's a string
                    if let Some(ptr) = unsafe { value.as_ptr::<RayaString>() } {
                        let _ = ptr; // Validate it's a string
                        "string"
                    } else {
                        "object"
                    }
                } else {
                    "undefined"
                };

                let raya_string = RayaString::new(type_str.to_string());
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let str_value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(str_value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            _ => OpcodeResult::Error(VmError::RuntimeError(format!(
                "Unexpected opcode in exec_type_ops: {:?}",
                opcode
            ))),
        }
    }
}
