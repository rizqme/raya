//! Type operation opcode handlers: InstanceOf, Cast, Typeof, DynGet, DynSet, DynGetKeyed,
//! DynSetKeyed, DynNewObject, DynKeys, DynHas, DynDelete, NewMutex, NewChannel, LoadStatic, StoreStatic

use crate::compiler::{Module, Opcode};
use crate::vm::gc::GcHeader;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{Array, BoundMethod, ChannelObject, Closure, Object, RayaString};
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::ptr::NonNull;
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

fn object_ptr_checked(value: Value) -> Option<NonNull<Object>> {
    if !value.is_ptr() {
        return None;
    }
    let ptr = unsafe { value.as_ptr::<u8>() }?;
    let header = unsafe {
        let hp = ptr.as_ptr().sub(std::mem::size_of::<GcHeader>());
        &*(hp as *const GcHeader)
    };
    if header.type_id() == std::any::TypeId::of::<Object>() {
        unsafe { value.as_ptr::<Object>() }
    } else {
        None
    }
}

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_type_ops(
        &mut self,
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        module: &Module,
        task: &Arc<Task>,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::InstanceOf => {
                let local_class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let class_index = self.resolve_class_id(module, local_class_index);
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let result = if obj_val.is_ptr() {
                    if let Some(obj_ptr) = object_ptr_checked(obj_val) {
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
                let cast_target_raw = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if std::env::var("RAYA_DEBUG_VM_CALLS").is_ok() {
                    eprintln!(
                        "[cast] target={} obj_raw=0x{:016X} is_null={} is_ptr={} is_i32={}",
                        cast_target_raw,
                        obj_val.raw(),
                        obj_val.is_null(),
                        obj_val.is_ptr(),
                        obj_val.is_i32()
                    );
                }

                let cast_target = cast_target_raw as u16;

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
                        let Some(object_ptr) = object_ptr_checked(obj_val) else {
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
                        let func_id = obj_val.as_i32().map(|v| v as usize).or_else(|| {
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

                // Class-cast target is encoded as module-local class ID.
                let cast_target_class = self.resolve_class_id(module, cast_target as usize);

                // Check if object is an instance of the target class
                let valid_cast = if obj_val.is_ptr() {
                    if let Some(obj_ptr) = object_ptr_checked(obj_val) {
                        let obj = unsafe { &*obj_ptr.as_ptr() };
                        let classes = self.classes.read();
                        let mut current_class_id = Some(obj.class_id);
                        let mut matches = false;
                        while let Some(cid) = current_class_id {
                            if cid == cast_target_class {
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
                    let target_name = {
                        let classes = self.classes.read();
                        classes
                            .get_class(cast_target_class)
                            .map(|c| c.name.clone())
                            .unwrap_or_else(|| "<unknown>".to_string())
                    };
                    let (actual_id, actual_name) =
                        if let Some(obj_ptr) = object_ptr_checked(obj_val) {
                            let obj = unsafe { &*obj_ptr.as_ptr() };
                            let class_id = obj.class_id;
                            let class_name = {
                                let classes = self.classes.read();
                                classes
                                    .get_class(class_id)
                                    .map(|c| c.name.clone())
                                    .unwrap_or_else(|| "<unknown>".to_string())
                            };
                            (class_id, class_name)
                        } else {
                            (usize::MAX, "<non-object>".to_string())
                        };
                    let current_func_id = task.current_func_id();
                    let current_func_name = module
                        .functions
                        .get(current_func_id)
                        .map(|f| f.name.as_str())
                        .unwrap_or("<unknown>");
                    OpcodeResult::Error(VmError::TypeError(format!(
                        "Cannot cast object(class_id={}, class_name={}) to class index {} ({}) in {}#{}",
                        actual_id, actual_name, cast_target_class, target_name, current_func_name, current_func_id
                    )))
                }
            }

            // =========================================================
            // Dynamic-object Operations
            // =========================================================
            Opcode::DynGet => {
                use crate::vm::json::view::{js_classify, JSView};

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

                // Pop the object from stack
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let result = match js_classify(obj_val) {
                    JSView::Struct { ptr, class_id } => {
                        // Typed class instance: name → slot lookup
                        let obj = unsafe { &*ptr };
                        let class_metadata = self.class_metadata.read();
                        let field_index = class_metadata
                            .get(class_id)
                            .and_then(|meta| meta.get_field_index(&prop_name));
                        let method_slot = if field_index.is_none() {
                            class_metadata
                                .get(class_id)
                                .and_then(|meta| meta.get_method_index(&prop_name))
                        } else {
                            None
                        };
                        drop(class_metadata);

                        if let Some(index) = field_index {
                            obj.get_field(index).unwrap_or(Value::null())
                        } else {
                            let classes = self.classes.read();
                            let (func_id, method_module) =
                                classes.get_class(class_id).map_or((None, None), |class| {
                                    let method_module = class.module.clone();
                                    let search_module =
                                        method_module.as_ref().map_or(module, |m| m);
                                    let func_id = method_slot
                                        .and_then(|slot| class.vtable.get_method(slot))
                                        .or_else(|| {
                                            class.vtable.methods.iter().copied().find(|fid| {
                                                search_module
                                                    .functions
                                                    .get(*fid)
                                                    .map(|f| {
                                                        let name = f.name.as_str();
                                                        name == prop_name
                                                            || name.ends_with(&format!(
                                                                ".{}",
                                                                prop_name
                                                            ))
                                                            || name.ends_with(&format!(
                                                                "::{}",
                                                                prop_name
                                                            ))
                                                    })
                                                    .unwrap_or(false)
                                            })
                                        });
                                    (func_id, method_module)
                                });
                            drop(classes);
                            if let Some(func_id) = func_id {
                                let gc_ptr = self.gc.lock().allocate(BoundMethod {
                                    receiver: obj_val,
                                    func_id,
                                    module: method_module,
                                });
                                unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                    )
                                }
                            } else {
                                if std::env::var("RAYA_DEBUG_DYNGET").is_ok() {
                                    let class_name = {
                                        let classes = self.classes.read();
                                        classes
                                            .get_class(class_id)
                                            .map(|class| class.name.clone())
                                            .unwrap_or_else(|| "<unknown>".to_string())
                                    };
                                    let metadata_methods = {
                                        let class_metadata = self.class_metadata.read();
                                        class_metadata
                                            .get(class_id)
                                            .map(|meta| {
                                                meta.method_names
                                                    .iter()
                                                    .filter(|name| !name.is_empty())
                                                    .cloned()
                                                    .collect::<Vec<_>>()
                                            })
                                            .unwrap_or_default()
                                    };
                                    eprintln!(
                                        "[dynget] unresolved struct member '{}.{}' class_id={} metadata_methods={:?}",
                                        class_name, prop_name, class_id, metadata_methods
                                    );
                                }
                                Value::null()
                            }
                        }
                    }
                    JSView::Dyn(ptr) => {
                        // DynObject: hashmap lookup
                        let value = unsafe { (*ptr).get(&prop_name) }.unwrap_or(Value::null());
                        if value.is_null() && std::env::var("RAYA_DEBUG_DYNGET").is_ok() {
                            eprintln!("[dynget] unresolved dyn member '.{}'", prop_name);
                        }
                        value
                    }
                    _ => {
                        if std::env::var("RAYA_DEBUG_DYNGET").is_ok() {
                            eprintln!("[dynget] unresolved non-object member '.{}'", prop_name);
                        }
                        Value::null()
                    }
                };

                if let Err(e) = stack.push(result) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::DynSet => {
                use crate::vm::json::view::{js_classify, JSView};
                use crate::vm::object::DynObject;

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
                            "Invalid constant index {} for DynSet property",
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
                        "Expected object for property assignment".to_string(),
                    ));
                }

                match js_classify(obj_val) {
                    JSView::Struct { ptr, class_id } => {
                        let obj = unsafe { &mut *(ptr as *mut Object) };
                        let class_metadata = self.class_metadata.read();
                        let field_index = class_metadata
                            .get(class_id)
                            .and_then(|meta| meta.get_field_index(&prop_name));
                        drop(class_metadata);
                        if let Some(index) = field_index {
                            let _ = obj.set_field(index, value);
                        } else {
                            return OpcodeResult::Error(VmError::TypeError(format!(
                                "Field '{}' not found on object",
                                prop_name
                            )));
                        }
                    }
                    JSView::Dyn(ptr) => {
                        let obj = unsafe { &mut *(ptr as *mut DynObject) };
                        obj.set(prop_name, value);
                    }
                    _ => {
                        return OpcodeResult::Error(VmError::TypeError(
                            "Expected object for property assignment".to_string(),
                        ));
                    }
                }

                OpcodeResult::Continue
            }

            Opcode::DynDelete => {
                use crate::vm::json::view::{js_classify, JSView};
                use crate::vm::object::DynObject;

                let prop_index = match Self::read_u32(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let prop_name = match module.constants.get_string(prop_index) {
                    Some(s) => s.to_string(),
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid constant index {} for DynDelete",
                            prop_index
                        )))
                    }
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let JSView::Dyn(ptr) = js_classify(obj_val) {
                    unsafe { &mut *(ptr as *mut DynObject) }
                        .props
                        .remove(&prop_name);
                }
                OpcodeResult::Continue
            }

            Opcode::DynGetKeyed => {
                use crate::vm::json::view::{js_classify, JSView};
                use crate::vm::object::RayaString;

                // Stack: [..., object, key] → key popped first
                let key_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Extract string key
                let key_str = match js_classify(key_val) {
                    JSView::Str(ptr) => unsafe { &*ptr }.data.clone(),
                    _ => {
                        return OpcodeResult::Error(VmError::TypeError(
                            "DynGetKeyed key must be a string".to_string(),
                        ))
                    }
                };

                let result = match js_classify(obj_val) {
                    JSView::Dyn(ptr) => unsafe { (*ptr).get(&key_str) }.unwrap_or(Value::null()),
                    JSView::Struct { ptr, class_id } => {
                        let obj = unsafe { &*ptr };
                        let class_metadata = self.class_metadata.read();
                        let field_index = class_metadata
                            .get(class_id)
                            .and_then(|meta| meta.get_field_index(&key_str));
                        drop(class_metadata);
                        if let Some(index) = field_index {
                            obj.get_field(index).unwrap_or(Value::null())
                        } else {
                            Value::null()
                        }
                    }
                    _ => Value::null(),
                };

                if let Err(e) = stack.push(result) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::DynSetKeyed => {
                use crate::vm::json::view::{js_classify, JSView};
                use crate::vm::object::DynObject;

                // Stack: [..., object, key, value] → value popped first
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let key_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let key_str = match js_classify(key_val) {
                    JSView::Str(ptr) => unsafe { &*ptr }.data.clone(),
                    _ => {
                        return OpcodeResult::Error(VmError::TypeError(
                            "DynSetKeyed key must be a string".to_string(),
                        ))
                    }
                };

                match js_classify(obj_val) {
                    JSView::Dyn(ptr) => {
                        let obj = unsafe { &mut *(ptr as *mut DynObject) };
                        obj.set(key_str, value);
                    }
                    JSView::Struct { ptr, class_id } => {
                        let obj = unsafe { &mut *(ptr as *mut Object) };
                        let class_metadata = self.class_metadata.read();
                        let field_index = class_metadata
                            .get(class_id)
                            .and_then(|meta| meta.get_field_index(&key_str));
                        drop(class_metadata);
                        if let Some(index) = field_index {
                            let _ = obj.set_field(index, value);
                        } else {
                            return OpcodeResult::Error(VmError::TypeError(format!(
                                "Field '{}' not found on struct",
                                key_str
                            )));
                        }
                    }
                    _ => {
                        return OpcodeResult::Error(VmError::TypeError(
                            "DynSetKeyed target must be an object".to_string(),
                        ))
                    }
                }
                OpcodeResult::Continue
            }

            Opcode::DynNewObject => {
                use crate::vm::object::DynObject;

                let dyn_obj = DynObject::new();
                let gc_ptr = self.gc.lock().allocate(dyn_obj);
                let val =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(val) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::DynKeys => {
                use crate::vm::json::view::{js_classify, JSView};
                use crate::vm::object::{Array, RayaString};

                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let keys: Vec<Value> = match js_classify(obj_val) {
                    JSView::Dyn(ptr) => {
                        let obj = unsafe { &*ptr };
                        obj.props
                            .keys()
                            .map(|k| {
                                let raya_str = RayaString::new(k.clone());
                                let gc_ptr = self.gc.lock().allocate(raya_str);
                                unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                    )
                                }
                            })
                            .collect()
                    }
                    _ => vec![],
                };

                let arr = Array {
                    type_id: 0,
                    elements: keys,
                };
                let arr_ptr = self.gc.lock().allocate(arr);
                let result =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(result) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::DynHas => {
                use crate::vm::json::view::{js_classify, JSView};

                let prop_index = match Self::read_u32(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let prop_name = match module.constants.get_string(prop_index) {
                    Some(s) => s.to_string(),
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid constant index {} for DynHas",
                            prop_index
                        )))
                    }
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let has = match js_classify(obj_val) {
                    JSView::Dyn(ptr) => unsafe { &*ptr }.has(&prop_name),
                    _ => false,
                };
                if let Err(e) = stack.push(Value::bool(has)) {
                    return OpcodeResult::Error(e);
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
                let local_class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let class_index = self.resolve_class_id(module, local_class_index);
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
                let local_class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let class_index = self.resolve_class_id(module, local_class_index);
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
