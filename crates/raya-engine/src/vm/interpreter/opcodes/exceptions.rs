//! Exception handling opcode handlers: Try, EndTry, Throw, Rethrow

use crate::compiler::Opcode;
use crate::vm::gc::header_ptr_from_value_ptr;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{Object, RayaString};
use crate::vm::scheduler::{ExceptionHandler, Task};
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::any::TypeId;
use std::ptr::NonNull;
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    #[inline]
    fn object_ptr_from_value(value: Value) -> Option<NonNull<Object>> {
        let ptr = unsafe { value.as_ptr::<u8>() }?;
        let header = unsafe { &*header_ptr_from_value_ptr(ptr.as_ptr()) };
        if header.type_id() == TypeId::of::<Object>() {
            Some(ptr.cast())
        } else {
            None
        }
    }

    #[inline]
    fn string_ptr_from_value(value: Value) -> Option<NonNull<RayaString>> {
        let ptr = unsafe { value.as_ptr::<u8>() }?;
        let header = unsafe { &*header_ptr_from_value_ptr(ptr.as_ptr()) };
        if header.type_id() == TypeId::of::<RayaString>() {
            Some(ptr.cast())
        } else {
            None
        }
    }

    fn legacy_error_field_index(field_name: &str, field_count: usize) -> Option<usize> {
        let idx = match field_name {
            "message" => 0,
            "name" => 1,
            "stack" => 2,
            "cause" => 3,
            "code" => 4,
            "errno" => 5,
            "syscall" => 6,
            "path" => 7,
            "errors" => 8,
            _ => return None,
        };
        (idx < field_count).then_some(idx)
    }

    pub(in crate::vm::interpreter) fn get_object_named_field_index(
        &self,
        object: &Object,
        field_name: &str,
    ) -> Option<usize> {
        if let Some(nominal_type_id) = object.nominal_type_id_usize() {
            let class_metadata = self.class_metadata.read();
            if let Some(index) = class_metadata
                .get(nominal_type_id)
                .and_then(|meta| meta.get_field_index(field_name))
            {
                return Some(index);
            }
        }
        if let Some(index) = self.structural_field_slot_index_for_object(object, field_name) {
            return Some(index);
        }
        Self::legacy_error_field_index(field_name, object.field_count())
    }

    pub(in crate::vm::interpreter) fn get_object_named_field_value(
        &self,
        object: &Object,
        field_name: &str,
    ) -> Option<Value> {
        if let Some(index) = self.get_object_named_field_index(object, field_name) {
            if let Some(value) = object.get_field(index) {
                return Some(value);
            }
        }
        let key = self.intern_prop_key(field_name);
        object.dyn_map().and_then(|map| map.get(&key).copied())
    }

    pub(in crate::vm::interpreter) fn has_object_named_field(
        &self,
        object: &Object,
        field_name: &str,
    ) -> bool {
        if self
            .get_object_named_field_index(object, field_name)
            .is_some()
        {
            return true;
        }
        let Some(map) = object.dyn_map() else {
            return false;
        };
        let key = self.intern_prop_key(field_name);
        map.contains_key(&key)
    }

    pub(in crate::vm::interpreter) fn set_object_named_field_value(
        &self,
        object: &mut Object,
        field_name: &str,
        value: Value,
    ) -> bool {
        if let Some(index) = self.get_object_named_field_index(object, field_name) {
            return object.set_field(index, value).is_ok();
        }
        let Some(map) = object.dyn_map_mut() else {
            return false;
        };
        let key = self.intern_prop_key(field_name);
        if map.contains_key(&key) {
            map.insert(key, value);
            true
        } else {
            false
        }
    }

    fn value_to_plain_string(value: Value) -> Option<String> {
        if value.is_null() {
            return Some(String::new());
        }
        if let Some(ptr) = Self::string_ptr_from_value(value) {
            return Some(unsafe { &*ptr.as_ptr() }.data.clone());
        }
        if let Some(i) = value.as_i32() {
            return Some(i.to_string());
        }
        if let Some(f) = value.as_f64() {
            if f.fract() == 0.0 {
                return Some(format!("{}", f as i64));
            }
            return Some(f.to_string());
        }
        if let Some(b) = value.as_bool() {
            return Some(b.to_string());
        }
        None
    }

    fn exception_surface(&self, object: &Object) -> Option<(String, String)> {
        let has_message = self.has_object_named_field(object, "message");
        let has_name = self.has_object_named_field(object, "name");
        let has_stack = self.has_object_named_field(object, "stack");
        if !(has_message || has_name || has_stack) {
            return None;
        }

        let error_name = self
            .get_object_named_field_value(object, "name")
            .and_then(Self::value_to_plain_string)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Error".to_string());
        let error_message = self
            .get_object_named_field_value(object, "message")
            .and_then(Self::value_to_plain_string)
            .unwrap_or_default();
        Some((error_name, error_message))
    }

    pub(in crate::vm::interpreter) fn exec_exception_ops(
        &mut self,
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        task: &Arc<Task>,
        frame_depth: usize,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::Try => {
                let catch_rel = match Self::read_i32(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let catch_abs = if catch_rel >= 0 {
                    *ip as i32 + catch_rel
                } else {
                    -1
                };

                let finally_rel = match Self::read_i32(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let finally_abs = if finally_rel > 0 {
                    *ip as i32 + finally_rel
                } else {
                    -1
                };

                let handler = ExceptionHandler {
                    catch_offset: catch_abs,
                    finally_offset: finally_abs,
                    stack_size: stack.depth(),
                    frame_count: frame_depth,
                    mutex_count: task.held_mutex_count(),
                };
                task.push_exception_handler(handler);
                OpcodeResult::Continue
            }

            Opcode::EndTry => {
                task.pop_exception_handler();
                OpcodeResult::Continue
            }

            Opcode::Throw => {
                let exception = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Populate stack on any error-like object with a structural
                // `name` / `message` / `stack` surface.
                if exception.is_ptr() {
                    if let Some(obj_ptr) = Self::object_ptr_from_value(exception) {
                        let obj = unsafe { &mut *obj_ptr.as_ptr() };
                        if let Some((error_name, error_message)) = self.exception_surface(obj) {
                            let stack_trace = task.build_stack_trace(&error_name, &error_message);
                            let raya_string = RayaString::new(stack_trace);
                            let gc_ptr = self.gc.lock().allocate(raya_string);
                            let stack_value = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            };
                            let _ = self.set_object_named_field_value(obj, "stack", stack_value);
                        }
                    }
                }

                // Extract a structural `message` field when present.
                let error_msg = if exception.is_ptr() {
                    if let Some(obj_ptr) = Self::object_ptr_from_value(exception) {
                        let obj = unsafe { &*obj_ptr.as_ptr() };
                        if let Some(msg) = self
                            .get_object_named_field_value(obj, "message")
                            .and_then(Self::value_to_plain_string)
                        {
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
                };

                task.set_exception(exception);
                OpcodeResult::Error(VmError::RuntimeError(error_msg))
            }

            Opcode::Rethrow => {
                if let Some(exception) = task.caught_exception() {
                    task.set_exception(exception);
                    OpcodeResult::Error(VmError::RuntimeError("rethrow".to_string()))
                } else {
                    OpcodeResult::Error(VmError::RuntimeError(
                        "RETHROW with no active exception".to_string(),
                    ))
                }
            }

            _ => OpcodeResult::Error(VmError::RuntimeError(format!(
                "Unexpected opcode in exec_exception_ops: {:?}",
                opcode
            ))),
        }
    }
}
