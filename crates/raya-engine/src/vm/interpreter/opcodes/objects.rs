//! Object opcode handlers: New, LoadField, StoreField, OptionalField, LoadFieldFast, StoreFieldFast, ObjectLiteral, InitObject, BindMethod

use crate::compiler::Opcode;
use crate::vm::gc::GcHeader;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{Array, BoundMethod, Closure, Object, RayaString};
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;

const NODE_DESCRIPTOR_METADATA_KEY: &str = "__node_compat_descriptor";

impl<'a> Interpreter<'a> {
    fn builtin_field_name_for_class_name(class_name: &str, field_offset: usize) -> Option<String> {
        let name = match class_name {
            "Object" => match field_offset {
                0 => "value",
                1 => "writable",
                2 => "configurable",
                3 => "enumerable",
                4 => "get",
                5 => "set",
                _ => return None,
            },
            "Map" => match field_offset {
                0 => "mapPtr",
                1 => "size",
                _ => return None,
            },
            "Set" => match field_offset {
                0 => "setPtr",
                1 => "size",
                _ => return None,
            },
            "Buffer" => match field_offset {
                0 => "bufferPtr",
                1 => "length",
                _ => return None,
            },
            "Error" | "TypeError" | "RangeError" | "ReferenceError" | "SyntaxError"
            | "URIError" | "EvalError" | "AggregateError" | "ChannelClosedError"
            | "AssertionError" => match field_offset {
                0 => "message",
                1 => "name",
                2 => "stack",
                3 => "cause",
                4 => "code",
                5 => "errno",
                6 => "syscall",
                7 => "path",
                _ => return None,
            },
            _ => return None,
        };
        Some(name.to_string())
    }

    fn builtin_field_index_for_class_name(class_name: &str, field_name: &str) -> Option<usize> {
        match class_name {
            "Object" => match field_name {
                "value" => Some(0),
                "writable" => Some(1),
                "configurable" => Some(2),
                "enumerable" => Some(3),
                "get" => Some(4),
                "set" => Some(5),
                _ => None,
            },
            "Map" => match field_name {
                "mapPtr" => Some(0),
                "size" => Some(1),
                _ => None,
            },
            "Set" => match field_name {
                "setPtr" => Some(0),
                "size" => Some(1),
                _ => None,
            },
            "Buffer" => match field_name {
                "bufferPtr" => Some(0),
                "length" => Some(1),
                _ => None,
            },
            "Error" | "TypeError" | "RangeError" | "ReferenceError" | "SyntaxError"
            | "URIError" | "EvalError" | "AggregateError" | "ChannelClosedError"
            | "AssertionError" => match field_name {
                "message" => Some(0),
                "name" => Some(1),
                "stack" => Some(2),
                "cause" => Some(3),
                "code" => Some(4),
                "errno" => Some(5),
                "syscall" => Some(6),
                "path" => Some(7),
                _ => None,
            },
            _ => None,
        }
    }

    fn field_name_for_offset(&self, obj: &Object, field_offset: usize) -> Option<String> {
        let class_metadata = self.class_metadata.read();
        let from_metadata = class_metadata
            .get(obj.class_id)
            .and_then(|meta| meta.field_names.get(field_offset))
            .cloned()
            .filter(|name| !name.is_empty());
        if from_metadata.is_some() {
            return from_metadata;
        }
        let classes = self.classes.read();
        let class_name = classes.get_class(obj.class_id)?.name.as_str();
        Self::builtin_field_name_for_class_name(class_name, field_offset)
    }

    fn field_index_for_value(&self, obj_val: Value, field_name: &str) -> Option<usize> {
        let obj_ptr = unsafe { obj_val.as_ptr::<Object>() }?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let class_metadata = self.class_metadata.read();
        let from_metadata = class_metadata
            .get(obj.class_id)
            .and_then(|meta| meta.get_field_index(field_name));
        if from_metadata.is_some() {
            return from_metadata;
        }
        let classes = self.classes.read();
        let class_name = classes.get_class(obj.class_id)?.name.as_str();
        Self::builtin_field_index_for_class_name(class_name, field_name)
    }

    fn get_value_field_by_name(&self, obj_val: Value, field_name: &str) -> Option<Value> {
        let index = self.field_index_for_value(obj_val, field_name)?;
        let obj_ptr = unsafe { obj_val.as_ptr::<Object>() }?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        obj.get_field(index)
    }

    fn is_field_writable(&self, obj_val: Value, field_name: &str) -> bool {
        let metadata = self.metadata.lock();
        let Some(descriptor) =
            metadata.get_metadata_property(NODE_DESCRIPTOR_METADATA_KEY, obj_val, field_name)
        else {
            return true;
        };
        let Some(writable) = self.get_value_field_by_name(descriptor, "writable") else {
            return true;
        };
        if let Some(b) = writable.as_bool() {
            b
        } else if let Some(i) = writable.as_i32() {
            i != 0
        } else {
            true
        }
    }

    fn sync_descriptor_value(&self, obj_val: Value, field_name: &str, value: Value) {
        let descriptor = {
            let metadata = self.metadata.lock();
            metadata.get_metadata_property(NODE_DESCRIPTOR_METADATA_KEY, obj_val, field_name)
        };
        let Some(descriptor) = descriptor else {
            return;
        };
        let Some(value_index) = self.field_index_for_value(descriptor, "value") else {
            return;
        };
        if let Some(desc_ptr) = unsafe { descriptor.as_ptr::<Object>() } {
            let desc = unsafe { &mut *desc_ptr.as_ptr() };
            let _ = desc.set_field(value_index, value);
        }
    }

    fn ensure_object_receiver(value: Value, context: &'static str) -> Result<Value, VmError> {
        if !value.is_ptr() {
            return Err(VmError::TypeError(format!(
                "Expected object for {}",
                context
            )));
        }

        let header = unsafe {
            let hp = (value.as_ptr::<u8>().unwrap().as_ptr()).sub(std::mem::size_of::<GcHeader>());
            &*(hp as *const GcHeader)
        };
        if header.type_id() == std::any::TypeId::of::<Object>() {
            return Ok(value);
        }

        let kind = if header.type_id() == std::any::TypeId::of::<Array>() {
            "Array"
        } else if header.type_id() == std::any::TypeId::of::<RayaString>() {
            "RayaString"
        } else if header.type_id() == std::any::TypeId::of::<Closure>() {
            "Closure"
        } else if header.type_id() == std::any::TypeId::of::<BoundMethod>() {
            "BoundMethod"
        } else {
            "UnknownGcType"
        };

        Err(VmError::TypeError(format!(
            "Expected Object receiver for {}, got {}",
            context, kind
        )))
    }

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

                let obj_val = match Self::ensure_object_receiver(obj_val, "field access") {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Check if the object is a proxy - if so, unwrap to target
                // TODO: Full trap support would call handler.get(target, fieldName)
                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                // Missing fields resolve to null. This matches object destructuring defaults
                // and allows optional object properties to be absent at runtime.
                let value = obj.get_field(field_offset).unwrap_or(Value::null());
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

                let obj_val = match Self::ensure_object_receiver(obj_val, "field access") {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Check if the object is a proxy - if so, unwrap to target
                // TODO: Full trap support would call handler.set(target, fieldName, value)
                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
                let obj = unsafe { &mut *obj_ptr.unwrap().as_ptr() };
                if let Some(field_name) = self.field_name_for_offset(obj, field_offset) {
                    if !self.is_field_writable(actual_obj, &field_name) {
                        return OpcodeResult::Error(VmError::TypeError(format!(
                            "Cannot assign to non-writable property '{}'",
                            field_name
                        )));
                    }
                }
                if let Err(e) = obj.set_field(field_offset, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                if let Some(field_name) = self.field_name_for_offset(obj, field_offset) {
                    self.sync_descriptor_value(actual_obj, &field_name, value);
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

                let obj_val = match Self::ensure_object_receiver(obj_val, "optional field access") {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Check if the object is a proxy - if so, unwrap to target
                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                let value = obj.get_field(field_offset).unwrap_or(Value::null());
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

                let obj_val = match Self::ensure_object_receiver(obj_val, "field access") {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Check if the object is a proxy - if so, unwrap to target
                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                let value = obj.get_field(field_offset).unwrap_or(Value::null());
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

                let obj_val = match Self::ensure_object_receiver(obj_val, "field access") {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Check if the object is a proxy - if so, unwrap to target
                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
                let obj = unsafe { &mut *obj_ptr.unwrap().as_ptr() };
                if let Some(field_name) = self.field_name_for_offset(obj, field_offset) {
                    if !self.is_field_writable(actual_obj, &field_name) {
                        return OpcodeResult::Error(VmError::TypeError(format!(
                            "Cannot assign to non-writable property '{}'",
                            field_name
                        )));
                    }
                }
                if let Err(e) = obj.set_field(field_offset, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                if let Some(field_name) = self.field_name_for_offset(obj, field_offset) {
                    self.sync_descriptor_value(actual_obj, &field_name, value);
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

                let obj_val = match Self::ensure_object_receiver(obj_val, "field initialization") {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

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

                let obj_val = match Self::ensure_object_receiver(obj_val, "method binding") {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

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
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
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
