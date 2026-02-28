//! Type operation opcode handlers: InstanceOf, Cast, Typeof, JsonGet, JsonSet,
//! NewMutex, NewChannel, LoadStatic, StoreStatic

use crate::compiler::{Module, Opcode};
use crate::vm::gc::GcHeader;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{Array, BoundMethod, ChannelObject, Closure, Object, RayaString};
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

const CAST_KIND_MASK_FLAG: u16 = 0x8000;
const CAST_TUPLE_LEN_FLAG: u16 = 0x4000;
const CAST_OBJECT_MIN_FIELDS_FLAG: u16 = 0x2000;
const CAST_ARRAY_ELEM_KIND_FLAG: u16 = 0x1000;
const CAST_KIND_NULL: u16 = 0x0001;
const CAST_KIND_BOOL: u16 = 0x0002;
const CAST_KIND_INT: u16 = 0x0004;
const CAST_KIND_NUMBER: u16 = 0x0008;
const CAST_KIND_STRING: u16 = 0x0010;
const CAST_KIND_ARRAY: u16 = 0x0020;
const CAST_KIND_OBJECT: u16 = 0x0040;
const CAST_KIND_FUNCTION: u16 = 0x0080;

fn value_kind_mask(value: Value) -> u16 {
    if value.is_null() {
        return CAST_KIND_NULL;
    }
    if value.as_bool().is_some() {
        return CAST_KIND_BOOL;
    }
    if value.as_i32().is_some() {
        return CAST_KIND_INT;
    }
    if value.as_f64().is_some() {
        return CAST_KIND_NUMBER;
    }
    if !value.is_ptr() {
        return 0;
    }
    let Some(ptr) = (unsafe { value.as_ptr::<u8>() }) else {
        return 0;
    };
    let header = unsafe {
        let hp = ptr.as_ptr().sub(std::mem::size_of::<GcHeader>());
        &*(hp as *const GcHeader)
    };
    if header.type_id() == std::any::TypeId::of::<RayaString>() {
        return CAST_KIND_STRING;
    }
    if header.type_id() == std::any::TypeId::of::<Array>() {
        return CAST_KIND_ARRAY;
    }
    if header.type_id() == std::any::TypeId::of::<Object>() {
        return CAST_KIND_OBJECT;
    }
    if header.type_id() == std::any::TypeId::of::<Closure>()
        || header.type_id() == std::any::TypeId::of::<BoundMethod>()
    {
        return CAST_KIND_FUNCTION;
    }
    0
}

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
                let cast_target = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let cast_target = cast_target as u16;

                // Kind-mask casts validate primitive/structural categories at runtime.
                if (cast_target & CAST_KIND_MASK_FLAG) != 0 {
                    // Tuple cast target with exact runtime arity check.
                    if (cast_target & CAST_TUPLE_LEN_FLAG) != 0 {
                        let expected_len = (cast_target & 0x3FFF) as usize;
                        if !obj_val.is_ptr() {
                            return OpcodeResult::Error(VmError::TypeError(format!(
                                "Cannot cast non-array value to tuple length {}",
                                expected_len
                            )));
                        }
                        let Some(array_ptr) = (unsafe { obj_val.as_ptr::<Array>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(format!(
                                "Cannot cast non-array value to tuple length {}",
                                expected_len
                            )));
                        };
                        let arr = unsafe { &*array_ptr.as_ptr() };
                        if arr.len() != expected_len {
                            return OpcodeResult::Error(VmError::TypeError(format!(
                                "Cannot cast array(length={}) to tuple(length={})",
                                arr.len(),
                                expected_len
                            )));
                        }
                        if let Err(e) = stack.push(obj_val) {
                            return OpcodeResult::Error(e);
                        }
                        return OpcodeResult::Continue;
                    }

                    // Object structural cast target with minimum required fields.
                    if (cast_target & CAST_OBJECT_MIN_FIELDS_FLAG) != 0 {
                        let required_fields = (cast_target & 0x1FFF) as usize;
                        if !obj_val.is_ptr() {
                            return OpcodeResult::Error(VmError::TypeError(format!(
                                "Cannot cast non-object value to object with {} required fields",
                                required_fields
                            )));
                        }
                        let Some(object_ptr) = (unsafe { obj_val.as_ptr::<Object>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(format!(
                                "Cannot cast non-object value to object with {} required fields",
                                required_fields
                            )));
                        };
                        let obj = unsafe { &*object_ptr.as_ptr() };
                        if obj.field_count() < required_fields {
                            return OpcodeResult::Error(VmError::TypeError(format!(
                                "Cannot cast object(field_count={}) to required field count {}",
                                obj.field_count(),
                                required_fields
                            )));
                        }
                        if let Err(e) = stack.push(obj_val) {
                            return OpcodeResult::Error(e);
                        }
                        return OpcodeResult::Continue;
                    }

                    // Array cast target with runtime-checkable element kinds.
                    if (cast_target & CAST_ARRAY_ELEM_KIND_FLAG) != 0 {
                        let expected_elem_mask = cast_target & 0x00FF;
                        if !obj_val.is_ptr() {
                            return OpcodeResult::Error(VmError::TypeError(format!(
                                "Cannot cast non-array value to array element mask 0x{:02X}",
                                expected_elem_mask
                            )));
                        }
                        let Some(array_ptr) = (unsafe { obj_val.as_ptr::<Array>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(format!(
                                "Cannot cast non-array value to array element mask 0x{:02X}",
                                expected_elem_mask
                            )));
                        };
                        let arr = unsafe { &*array_ptr.as_ptr() };
                        for elem in &arr.elements {
                            let mut actual = value_kind_mask(*elem);
                            if (actual & CAST_KIND_INT) != 0 {
                                actual |= CAST_KIND_NUMBER;
                            }
                            if (actual & expected_elem_mask) == 0 {
                                return OpcodeResult::Error(VmError::TypeError(format!(
                                    "Cannot cast array element to required kind mask 0x{:02X}",
                                    expected_elem_mask
                                )));
                            }
                        }
                        if let Err(e) = stack.push(obj_val) {
                            return OpcodeResult::Error(e);
                        }
                        return OpcodeResult::Continue;
                    }

                    let expected = cast_target & !CAST_KIND_MASK_FLAG;
                    let mut actual = value_kind_mask(obj_val);
                    // `number` accepts both integer and float values.
                    if (actual & CAST_KIND_INT) != 0 {
                        actual |= CAST_KIND_NUMBER;
                    }
                    if (actual & expected) != 0 {
                        if let Err(e) = stack.push(obj_val) {
                            return OpcodeResult::Error(e);
                        }
                        return OpcodeResult::Continue;
                    }
                    // Compatibility path: function references may be encoded as
                    // direct function IDs (int) instead of closure pointers.
                    if expected == CAST_KIND_FUNCTION {
                        let func_id = obj_val
                            .as_i32()
                            .map(|v| v as usize)
                            .or_else(|| {
                                obj_val.as_f64().and_then(|v| {
                                    if v.is_finite()
                                        && v.fract() == 0.0
                                        && v >= 0.0
                                        && v <= usize::MAX as f64
                                    {
                                        Some(v as usize)
                                    } else {
                                        None
                                    }
                                })
                            });
                        if let Some(func_id) = func_id {
                            if module.functions.get(func_id).is_some() {
                                if let Err(e) = stack.push(obj_val) {
                                    return OpcodeResult::Error(e);
                                }
                                return OpcodeResult::Continue;
                            }
                        }
                    }
                    // Explicit cast to `int` can accept a numeric value only when
                    // the runtime number is finite and integral.
                    if expected == CAST_KIND_INT {
                        if let Some(num) = obj_val.as_f64() {
                            if num.is_finite()
                                && num.fract() == 0.0
                                && num >= i32::MIN as f64
                                && num <= i32::MAX as f64
                            {
                                if let Err(e) = stack.push(Value::i32(num as i32)) {
                                    return OpcodeResult::Error(e);
                                }
                                return OpcodeResult::Continue;
                            }
                        }
                    }
                    return OpcodeResult::Error(VmError::TypeError(format!(
                        "Cannot cast value to runtime kind mask 0x{:04X}",
                        expected
                    )));
                }

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
                            if cid == cast_target as usize {
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
                        cast_target
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
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
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
                let value = class
                    .get_static_field(field_offset)
                    .unwrap_or(Value::null());
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
                } else if value.is_i32() {
                    "number"
                } else if value.is_i64() || value.is_f64() {
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
                let str_value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
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
