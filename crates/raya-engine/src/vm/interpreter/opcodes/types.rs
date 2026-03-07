//! Type operation opcode handlers: InstanceOf, Cast, Typeof, DynGet, DynSet, DynGetKeyed,
//! DynSetKeyed, DynNewObject, DynKeys, DynHas, DynDelete, NewMutex, NewChannel, LoadStatic, StoreStatic

use crate::compiler::type_registry::TypeRegistry;
use crate::compiler::{Module, Opcode};
use crate::parser::TypeContext;
use crate::vm::gc::GcHeader;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{
    layout_id_from_ordered_names, Array, BoundMethod, BoundNativeMethod, ChannelObject, Closure,
    Object, RayaString,
};
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::ptr::NonNull;
use std::sync::{Arc, OnceLock};

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
        || header.type_id() == std::any::TypeId::of::<BoundNativeMethod>()
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

fn dyn_key_parts(key_val: Value) -> Result<(Option<String>, Option<usize>), VmError> {
    use crate::vm::json::view::{js_classify, JSView};

    match js_classify(key_val) {
        JSView::Str(ptr) => {
            let key = unsafe { &*ptr }.data.clone();
            let index = key.parse::<usize>().ok();
            Ok((Some(key), index))
        }
        JSView::Int(index) if index >= 0 => {
            let index = index as usize;
            Ok((Some(index.to_string()), Some(index)))
        }
        JSView::Number(number) if number.is_finite() && number.fract() == 0.0 && number >= 0.0 => {
            let index = number as usize;
            Ok((Some(index.to_string()), Some(index)))
        }
        _ => Err(VmError::TypeError(
            "DynGetKeyed key must be a string or non-negative integer".to_string(),
        )),
    }
}

impl<'a> Interpreter<'a> {
    fn builtin_native_method_for_class(class_name: &str, method_name: &str) -> Option<u16> {
        static TYPE_REGISTRY: OnceLock<TypeRegistry> = OnceLock::new();
        TYPE_REGISTRY
            .get_or_init(|| {
                let type_ctx = TypeContext::new();
                TypeRegistry::new(&type_ctx)
            })
            .native_method_id_for_type_name(class_name, method_name)
    }

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
                let class_index = match self.resolve_nominal_type_id(module, local_class_index) {
                    Ok(id) => id,
                    Err(error) => return OpcodeResult::Error(error),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let result = if obj_val.is_ptr() {
                    if let Some(obj_ptr) = object_ptr_checked(obj_val) {
                        let obj = unsafe { &*obj_ptr.as_ptr() };
                        let classes = self.classes.read();
                        let mut current_class_id = obj.nominal_class_id();
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
                        let effective_field_count = obj
                            .field_count()
                            .max(obj.dyn_map().map(|dyn_map| dyn_map.len()).unwrap_or(0));
                        if effective_field_count < required_fields {
                            return OpcodeResult::Error(VmError::TypeError(format!(
                                "Cannot cast object(field_count={}) to required field count {}",
                                effective_field_count, required_fields
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
                let cast_target_class =
                    match self.resolve_nominal_type_id(module, cast_target as usize) {
                        Ok(id) => id,
                        Err(error) => return OpcodeResult::Error(error),
                    };

                // Check if object is an instance of the target class
                let valid_cast = if obj_val.is_ptr() {
                    if let Some(obj_ptr) = object_ptr_checked(obj_val) {
                        let obj = unsafe { &*obj_ptr.as_ptr() };
                        let classes = self.classes.read();
                        let mut current_class_id = obj.nominal_class_id();
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
                            if let Some(class_id) = obj.nominal_class_id() {
                                let class_name = {
                                    let classes = self.classes.read();
                                    classes
                                        .get_class(class_id)
                                        .map(|c| c.name.clone())
                                        .unwrap_or_else(|| "<unknown>".to_string())
                                };
                                (class_id, class_name)
                            } else {
                                (usize::MAX, "<structural-object>".to_string())
                            }
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
                    JSView::Struct { ptr, .. } => {
                        // Typed class instance: name → slot lookup
                        let obj = unsafe { &*ptr };
                        let nominal_class_id = obj.nominal_class_id();
                        let field_index = self.get_field_index_for_value(obj_val, &prop_name);
                        let class_metadata = self.class_metadata.read();
                        let method_slot = if field_index.is_none() {
                            nominal_class_id.and_then(|class_id| {
                                class_metadata
                                    .get(class_id)
                                    .and_then(|meta| meta.get_method_index(&prop_name))
                            })
                        } else {
                            None
                        };
                        drop(class_metadata);

                        if let Some(index) = field_index {
                            obj.get_field(index).unwrap_or(Value::null())
                        } else {
                            let classes = self.classes.read();
                            let (func_id, method_module, class_name) = nominal_class_id
                                .and_then(|class_id| classes.get_class(class_id))
                                .map_or((None, None, None), |class| {
                                    let method_module = class.module.clone();
                                    let class_name = Some(class.name.clone());
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
                                    (func_id, method_module, class_name)
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
                                if let Some(native_id) = class_name.as_ref().and_then(|name| {
                                    Self::builtin_native_method_for_class(name, &prop_name)
                                }) {
                                    let gc_ptr = self.gc.lock().allocate(BoundNativeMethod {
                                        receiver: obj_val,
                                        native_id,
                                    });
                                    unsafe {
                                        Value::from_ptr(
                                            std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                        )
                                    }
                                } else {
                                    let dyn_value = {
                                        let key = self.intern_prop_key(&prop_name);
                                        obj.dyn_map().and_then(|dyn_map| dyn_map.get(&key).copied())
                                    };
                                    if let Some(value) = dyn_value {
                                        value
                                    } else {
                                        if std::env::var("RAYA_DEBUG_DYNGET").is_ok() {
                                            let class_debug = nominal_class_id
                                                .map(|id| id.to_string())
                                                .unwrap_or_else(|| "structural".to_string());
                                            let class_name = {
                                                nominal_class_id.map_or_else(
                                                    || "<structural-object>".to_string(),
                                                    |class_id| {
                                                        let classes = self.classes.read();
                                                        classes
                                                            .get_class(class_id)
                                                            .map(|class| class.name.clone())
                                                            .unwrap_or_else(|| {
                                                                "<unknown>".to_string()
                                                            })
                                                    },
                                                )
                                            };
                                            let metadata_methods = {
                                                let class_metadata = self.class_metadata.read();
                                                nominal_class_id
                                                    .and_then(|class_id| {
                                                        class_metadata.get(class_id)
                                                    })
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
                                                class_name, prop_name, class_debug, metadata_methods
                                            );
                                        }
                                        Value::null()
                                    }
                                }
                            }
                        }
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
                    JSView::Struct { ptr, .. } => {
                        let obj = unsafe { &mut *(ptr as *mut Object) };
                        let field_index = self.get_field_index_for_value(obj_val, &prop_name);
                        if let Some(index) = field_index {
                            let _ = obj.set_field(index, value);
                        } else {
                            let key = self.intern_prop_key(&prop_name);
                            obj.ensure_dyn_map().insert(key, value);
                        }
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
                match js_classify(obj_val) {
                    JSView::Struct { ptr, .. } => {
                        let obj = unsafe { &mut *(ptr as *mut Object) };
                        if self
                            .get_field_index_for_value(obj_val, &prop_name)
                            .is_none()
                        {
                            let key = self.intern_prop_key(&prop_name);
                            if let Some(dyn_map) = obj.dyn_map_mut() {
                                dyn_map.remove(&key);
                            }
                        }
                    }
                    _ => {}
                }
                OpcodeResult::Continue
            }

            Opcode::DynGetKeyed => {
                use crate::vm::json::view::{js_classify, JSView};

                // Stack: [..., object, key] → key popped first
                let key_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let (key_str, array_index) = match dyn_key_parts(key_val) {
                    Ok(parts) => parts,
                    Err(error) => return OpcodeResult::Error(error),
                };

                let result = match js_classify(obj_val) {
                    JSView::Arr(ptr) => {
                        let arr = unsafe { &*ptr };
                        if let Some(index) = array_index {
                            arr.get(index).unwrap_or(Value::null())
                        } else if key_str.as_deref() == Some("length") {
                            Value::i32(arr.len() as i32)
                        } else {
                            Value::null()
                        }
                    }
                    JSView::Struct { ptr, .. } => {
                        let obj = unsafe { &*ptr };
                        let key_str = key_str
                            .as_deref()
                            .expect("dyn object property access should always have a key string");
                        let field_index = self.get_field_index_for_value(obj_val, &key_str);
                        if let Some(index) = field_index {
                            obj.get_field(index).unwrap_or(Value::null())
                        } else {
                            let key = self.intern_prop_key(&key_str);
                            obj.dyn_map()
                                .and_then(|dyn_map| dyn_map.get(&key).copied())
                                .unwrap_or(Value::null())
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

                let (key_str, array_index) = match dyn_key_parts(key_val) {
                    Ok(parts) => parts,
                    Err(error) => return OpcodeResult::Error(error),
                };

                match js_classify(obj_val) {
                    JSView::Arr(ptr) => {
                        let Some(index) = array_index else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "DynSetKeyed array index must be a non-negative integer"
                                    .to_string(),
                            ));
                        };
                        let arr = unsafe { &mut *(ptr as *mut Array) };
                        if index >= arr.elements.len() {
                            arr.elements.resize(index + 1, Value::null());
                        }
                        arr.elements[index] = value;
                    }
                    JSView::Struct { ptr, .. } => {
                        let obj = unsafe { &mut *(ptr as *mut Object) };
                        let key_str = key_str
                            .as_deref()
                            .expect("dyn object property access should always have a key string");
                        let field_index = self.get_field_index_for_value(obj_val, &key_str);
                        if let Some(index) = field_index {
                            let _ = obj.set_field(index, value);
                        } else {
                            let key = self.intern_prop_key(&key_str);
                            obj.ensure_dyn_map().insert(key, value);
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
                let empty_layout = layout_id_from_ordered_names(&[]);
                let obj = Object::new_dynamic(empty_layout, 0);
                let gc_ptr = self.gc.lock().allocate(obj);
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
                    JSView::Struct { ptr, .. } => {
                        let obj = unsafe { &*ptr };
                        let mut names = Vec::new();
                        if let Some(class_id) = obj.nominal_class_id() {
                            let class_metadata = self.class_metadata.read();
                            if let Some(meta) = class_metadata.get(class_id) {
                                names.extend(
                                    meta.field_names
                                        .iter()
                                        .filter(|name| !name.is_empty())
                                        .cloned(),
                                );
                            }
                        } else if let Some(layout_names) =
                            self.structural_layout_names(obj.layout_id())
                        {
                            names.extend(layout_names);
                        }
                        if let Some(dyn_map) = obj.dyn_map() {
                            for key in dyn_map.keys() {
                                if let Some(name) = self.prop_key_name(*key) {
                                    if !names.iter().any(|existing| existing == &name) {
                                        names.push(name);
                                    }
                                }
                            }
                        }
                        names
                            .into_iter()
                            .map(|name| {
                                let raya_str = RayaString::new(name);
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
                    JSView::Struct { ptr, .. } => {
                        if self
                            .get_field_index_for_value(obj_val, &prop_name)
                            .is_some()
                        {
                            true
                        } else {
                            let obj = unsafe { &*ptr };
                            let key = self.intern_prop_key(&prop_name);
                            obj.dyn_map()
                                .map(|dyn_map| dyn_map.contains_key(&key))
                                .unwrap_or(false)
                        }
                    }
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
                let handle = gc_ptr.as_ptr() as u64;
                if let Err(e) = stack.push(Value::u64(handle)) {
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
                let class_index = match self.resolve_nominal_type_id(module, local_class_index) {
                    Ok(id) => id,
                    Err(error) => return OpcodeResult::Error(error),
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
                let local_class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let class_index = match self.resolve_nominal_type_id(module, local_class_index) {
                    Ok(id) => id,
                    Err(error) => return OpcodeResult::Error(error),
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
