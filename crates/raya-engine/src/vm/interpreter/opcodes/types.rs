//! Type operation opcode handlers: nominal checks/casts, generic casts, dynamic keyed access,
//! and static/runtime type helpers.

use crate::compiler::type_registry::TypeRegistry;
use crate::compiler::{Module, Opcode};
use crate::compiler::native_id;
use crate::parser::TypeContext;
use crate::vm::builtin::{array, channel, map, regexp, set, string};
use crate::vm::gc::header_ptr_from_value_ptr;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::{Interpreter, ReturnAction};
use crate::vm::object::{
    layout_id_from_ordered_names, Array, BoundMethod, BoundNativeMethod, ChannelObject, Closure,
    MapObject, Object, RayaString, RegExpObject, SetObject,
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
        &*header_ptr_from_value_ptr(ptr.as_ptr())
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
        &*header_ptr_from_value_ptr(ptr.as_ptr())
    };
    if header.type_id() == std::any::TypeId::of::<Object>() {
        unsafe { value.as_ptr::<Object>() }
    } else {
        None
    }
}

pub(in crate::vm::interpreter) fn builtin_handle_native_method_id(
    value: Value,
    key: &str,
) -> Option<u16> {
    if let Some(ptr) = object_ptr_checked(value) {
        let obj = unsafe { &*ptr.as_ptr() };
        if obj.nominal_type_id().is_some() {
            return match key {
                "hashCode" => Some(native_id::OBJECT_HASH_CODE),
                "equals" => Some(native_id::OBJECT_EQUAL),
                _ => None,
            };
        }
    }
    if value.is_ptr() {
        let ptr = unsafe { value.as_ptr::<u8>() }?;
        let header = unsafe {
            &*header_ptr_from_value_ptr(ptr.as_ptr())
        };
        let ty = header.type_id();
        if ty == std::any::TypeId::of::<Array>() {
            return match key {
                "push" => Some(array::PUSH),
                "pop" => Some(array::POP),
                "shift" => Some(array::SHIFT),
                "unshift" => Some(array::UNSHIFT),
                "indexOf" => Some(array::INDEX_OF),
                "includes" => Some(array::INCLUDES),
                "slice" => Some(array::SLICE),
                "splice" => Some(array::SPLICE),
                "concat" => Some(array::CONCAT),
                "reverse" => Some(array::REVERSE),
                "join" => Some(array::JOIN),
                "forEach" => Some(array::FOR_EACH),
                "filter" => Some(array::FILTER),
                "find" => Some(array::FIND),
                "findIndex" => Some(array::FIND_INDEX),
                "every" => Some(array::EVERY),
                "some" => Some(array::SOME),
                "lastIndexOf" => Some(array::LAST_INDEX_OF),
                "sort" => Some(array::SORT),
                "map" => Some(array::MAP),
                "reduce" => Some(array::REDUCE),
                "fill" => Some(array::FILL),
                "flat" => Some(array::FLAT),
                _ => None,
            };
        }
        if ty == std::any::TypeId::of::<RayaString>() {
            return match key {
                "charAt" => Some(string::CHAR_AT),
                "substring" => Some(string::SUBSTRING),
                "toUpperCase" => Some(string::TO_UPPER_CASE),
                "toLowerCase" => Some(string::TO_LOWER_CASE),
                "trim" => Some(string::TRIM),
                "indexOf" => Some(string::INDEX_OF),
                "includes" => Some(string::INCLUDES),
                "split" => Some(string::SPLIT),
                "startsWith" => Some(string::STARTS_WITH),
                "endsWith" => Some(string::ENDS_WITH),
                "replace" => Some(string::REPLACE),
                "repeat" => Some(string::REPEAT),
                "slice" => Some(string::SLICE),
                "trimStart" => Some(string::TRIM_START),
                "trimEnd" => Some(string::TRIM_END),
                _ => None,
            };
        }
    }
    let handle = value.as_u64()?;
    if handle == 0 {
        return None;
    }
    let ptr = handle as *const u8;
    let header = unsafe {
        &*header_ptr_from_value_ptr(ptr)
    };
    let ty = header.type_id();
    if ty == std::any::TypeId::of::<ChannelObject>() {
        return match key {
            "send" => Some(channel::SEND),
            "receive" => Some(channel::RECEIVE),
            "trySend" => Some(channel::TRY_SEND),
            "tryReceive" => Some(channel::TRY_RECEIVE),
            "close" => Some(channel::CLOSE),
            "isClosed" => Some(channel::IS_CLOSED),
            "length" => Some(channel::LENGTH),
            "capacity" => Some(channel::CAPACITY),
            _ => None,
        };
    }
    if ty == std::any::TypeId::of::<MapObject>() {
        return match key {
            "size" => Some(map::SIZE),
            "get" => Some(map::GET),
            "set" => Some(map::SET),
            "has" => Some(map::HAS),
            "delete" => Some(map::DELETE),
            "clear" => Some(map::CLEAR),
            "keys" => Some(map::KEYS),
            "values" => Some(map::VALUES),
            "entries" => Some(map::ENTRIES),
            "forEach" => Some(map::FOR_EACH),
            _ => None,
        };
    }
    if ty == std::any::TypeId::of::<SetObject>() {
        return match key {
            "size" => Some(set::SIZE),
            "add" => Some(set::ADD),
            "has" => Some(set::HAS),
            "delete" => Some(set::DELETE),
            "clear" => Some(set::CLEAR),
            "values" => Some(set::VALUES),
            "keys" => Some(set::VALUES),
            "entries" => Some(set::VALUES),
            "forEach" => Some(set::FOR_EACH),
            "union" => Some(set::UNION),
            "intersection" => Some(set::INTERSECTION),
            "difference" => Some(set::DIFFERENCE),
            _ => None,
        };
    }
    if ty == std::any::TypeId::of::<RegExpObject>() {
        return match key {
            "test" => Some(regexp::TEST),
            "exec" => Some(regexp::EXEC),
            "execAll" => Some(regexp::EXEC_ALL),
            "replace" => Some(regexp::REPLACE),
            "replaceWith" => Some(regexp::REPLACE_WITH),
            "split" => Some(regexp::SPLIT),
            _ => None,
        };
    }
    None
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
    fn exec_shape_cast(
        &self,
        stack: &mut Stack,
        obj_val: Value,
        required_shape: u64,
    ) -> OpcodeResult {
        let Some(object_ptr) = object_ptr_checked(obj_val) else {
            return OpcodeResult::Error(VmError::TypeError(format!(
                "Cannot cast non-object value to structural shape @{required_shape:016x}"
            )));
        };
        let obj = unsafe { &*object_ptr.as_ptr() };
        self.record_aot_shape_site(
            crate::aot_profile::AotSiteKind::CastShape,
            obj.layout_id(),
        );
        let Some(adapter) = self.ensure_shape_adapter_for_object(obj, required_shape) else {
            return OpcodeResult::Error(VmError::TypeError(format!(
                "Cannot cast object(layout_id={}) to structural shape @{required_shape:016x}",
                obj.layout_id()
            )));
        };
        for slot in 0..adapter.len() {
            if matches!(adapter.binding_for_slot(slot), crate::vm::interpreter::shared_state::StructuralSlotBinding::Missing) {
                return OpcodeResult::Error(VmError::TypeError(format!(
                    "Cannot cast object(layout_id={}) to structural shape @{required_shape:016x}: missing required slot {}",
                    obj.layout_id(),
                    slot
                )));
            }
        }
        if let Err(error) = stack.push(obj_val) {
            return OpcodeResult::Error(error);
        }
        OpcodeResult::Continue
    }

    fn exec_tuple_len_cast(
        &self,
        stack: &mut Stack,
        obj_val: Value,
        expected_len: usize,
    ) -> OpcodeResult {
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
        OpcodeResult::Continue
    }

    fn exec_object_min_fields_cast(
        &self,
        stack: &mut Stack,
        obj_val: Value,
        required_fields: usize,
    ) -> OpcodeResult {
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
        OpcodeResult::Continue
    }

    fn exec_array_elem_kind_cast(
        &self,
        stack: &mut Stack,
        obj_val: Value,
        expected_elem_mask: u16,
    ) -> OpcodeResult {
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
        OpcodeResult::Continue
    }

    fn exec_kind_mask_cast(
        &self,
        stack: &mut Stack,
        module: &Module,
        obj_val: Value,
        expected: u16,
    ) -> OpcodeResult {
        let mut actual = value_kind_mask(obj_val);
        if (actual & CAST_KIND_INT) != 0 {
            actual |= CAST_KIND_NUMBER;
        }
        if (actual & expected) != 0 {
            if let Err(e) = stack.push(obj_val) {
                return OpcodeResult::Error(e);
            }
            return OpcodeResult::Continue;
        }
        if expected == CAST_KIND_FUNCTION {
            let func_id = obj_val.as_i32().map(|v| v as usize).or_else(|| {
                obj_val.as_f64().and_then(|v| {
                    if v.is_finite() && v.fract() == 0.0 && v >= 0.0 && v <= usize::MAX as f64 {
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
        OpcodeResult::Error(VmError::TypeError(format!(
            "Cannot cast value to runtime kind mask 0x{:04X}",
            expected
        )))
    }

    fn exec_implements_shape(
        &self,
        stack: &mut Stack,
        obj_val: Value,
        required_shape: u64,
    ) -> OpcodeResult {
        let Some(object_ptr) = object_ptr_checked(obj_val) else {
            return stack
                .push(Value::bool(false))
                .map_or_else(OpcodeResult::Error, |_| OpcodeResult::Continue);
        };
        let obj = unsafe { &*object_ptr.as_ptr() };
        self.record_aot_shape_site(
            crate::aot_profile::AotSiteKind::ImplementsShape,
            obj.layout_id(),
        );
        let result = self
            .ensure_shape_adapter_for_object(obj, required_shape)
            .is_some_and(|adapter| {
                (0..adapter.len()).all(|slot| {
                    !matches!(
                        adapter.binding_for_slot(slot),
                        crate::vm::interpreter::shared_state::StructuralSlotBinding::Missing
                    )
                })
            });
        stack
            .push(Value::bool(result))
            .map_or_else(OpcodeResult::Error, |_| OpcodeResult::Continue)
    }

    fn builtin_native_method_for_class(class_name: &str, method_name: &str) -> Option<u16> {
        static TYPE_REGISTRY: OnceLock<TypeRegistry> = OnceLock::new();
        TYPE_REGISTRY
            .get_or_init(|| {
                let type_ctx = TypeContext::new();
                TypeRegistry::new(&type_ctx)
            })
            .native_method_id_for_type_name(class_name, method_name)
    }

    fn exec_nominal_cast(
        &self,
        stack: &mut Stack,
        module: &Module,
        task: &Arc<Task>,
        obj_val: Value,
        target_nominal_type_id: usize,
    ) -> OpcodeResult {
        if obj_val.is_null() {
            if let Err(error) = stack.push(obj_val) {
                return OpcodeResult::Error(error);
            }
            return OpcodeResult::Continue;
        }

        let valid_cast = if obj_val.is_ptr() {
            if let Some(obj_ptr) = object_ptr_checked(obj_val) {
                let obj = unsafe { &*obj_ptr.as_ptr() };
                let classes = self.classes.read();
                let mut current_nominal_type_id = obj.nominal_type_id_usize();
                let mut matches = false;
                while let Some(cid) = current_nominal_type_id {
                    if cid == target_nominal_type_id {
                        matches = true;
                        break;
                    }
                    if let Some(class) = classes.get_class(cid) {
                        current_nominal_type_id = class.parent_id;
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
            if let Err(error) = stack.push(obj_val) {
                return OpcodeResult::Error(error);
            }
            return OpcodeResult::Continue;
        }

        let target_name = {
            let classes = self.classes.read();
            classes
                .get_class(target_nominal_type_id)
                .map(|class| class.name.clone())
                .unwrap_or_else(|| "<unknown>".to_string())
        };
        let (actual_nominal_type_id, actual_name) = if let Some(obj_ptr) = object_ptr_checked(obj_val) {
            let obj = unsafe { &*obj_ptr.as_ptr() };
            if let Some(nominal_type_id) = obj.nominal_type_id_usize() {
                let class_name = {
                    let classes = self.classes.read();
                    classes
                        .get_class(nominal_type_id)
                        .map(|class| class.name.clone())
                        .unwrap_or_else(|| "<unknown>".to_string())
                };
                (nominal_type_id, class_name)
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
            .map(|function| function.name.as_str())
            .unwrap_or("<unknown>");
        OpcodeResult::Error(VmError::TypeError(format!(
            "Cannot cast object(nominal_type_id={}, nominal_type_name={}) to nominal type {} ({}) in {}#{}",
            actual_nominal_type_id,
            actual_name,
            target_nominal_type_id,
            target_name,
            current_func_name,
            current_func_id
        )))
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
            Opcode::IsNominal => {
                let local_class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let target_nominal_type_id =
                    match self.resolve_nominal_type_id(module, local_class_index) {
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
                        let mut current_nominal_type_id = obj.nominal_type_id_usize();
                        let mut matches = false;
                        while let Some(cid) = current_nominal_type_id {
                            if cid == target_nominal_type_id {
                                matches = true;
                                break;
                            }
                            if let Some(class) = classes.get_class(cid) {
                                current_nominal_type_id = class.parent_id;
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

            Opcode::CastNominal => {
                let local_nominal_type_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(error) => return OpcodeResult::Error(error),
                };
                let target_nominal_type_id =
                    match self.resolve_nominal_type_id(module, local_nominal_type_index) {
                        Ok(id) => id,
                        Err(error) => return OpcodeResult::Error(error),
                };
                let obj_val = match stack.pop() {
                    Ok(value) => value,
                    Err(error) => return OpcodeResult::Error(error),
                };
                self.exec_nominal_cast(stack, module, task, obj_val, target_nominal_type_id)
            }

            Opcode::CastShape => {
                let required_shape = match Self::read_u64(code, ip) {
                    Ok(v) => v,
                    Err(error) => return OpcodeResult::Error(error),
                };
                let obj_val = match stack.pop() {
                    Ok(value) => value,
                    Err(error) => return OpcodeResult::Error(error),
                };
                self.exec_shape_cast(stack, obj_val, required_shape)
            }

            Opcode::ImplementsShape => {
                let required_shape = match Self::read_u64(code, ip) {
                    Ok(v) => v,
                    Err(error) => return OpcodeResult::Error(error),
                };
                let obj_val = match stack.pop() {
                    Ok(value) => value,
                    Err(error) => return OpcodeResult::Error(error),
                };
                self.exec_implements_shape(stack, obj_val, required_shape)
            }

            Opcode::CastTupleLen => {
                let expected_len = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                self.exec_tuple_len_cast(stack, obj_val, expected_len)
            }
            Opcode::CastObjectMinFields => {
                let required_fields = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                self.exec_object_min_fields_cast(stack, obj_val, required_fields)
            }
            Opcode::CastArrayElemKind => {
                let expected_elem_mask = match Self::read_u16(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                self.exec_array_elem_kind_cast(stack, obj_val, expected_elem_mask)
            }
            Opcode::CastKindMask => {
                let expected_kind_mask = match Self::read_u16(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                self.exec_kind_mask_cast(stack, module, obj_val, expected_kind_mask)
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
                        let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);
                        let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() }
                            .expect("JSView::Struct should always be an object");
                        let obj = unsafe { &*obj_ptr.as_ptr() };
                        let key_str = key_str
                            .as_deref()
                            .expect("dyn object property access should always have a key string");
                        if let Some(getter) = self.descriptor_accessor(actual_obj, &key_str, "get") {
                            match self.callable_frame_for_value(
                                getter,
                                stack,
                                &[],
                                ReturnAction::PushReturnValue,
                            ) {
                                Ok(Some(frame)) => return frame,
                                Ok(None) => {
                                    return OpcodeResult::Error(VmError::TypeError(format!(
                                        "Property '{}' getter is not callable",
                                        key_str
                                    )));
                                }
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        }
                        let field_index = self.get_field_index_for_value(obj_val, &key_str);
                        if let Some(index) = field_index {
                            obj.get_field(index).unwrap_or(Value::null())
                        } else if let Some(method_slot) = obj.nominal_type_id_usize().and_then(
                            |nominal_type_id| {
                                let class_metadata = self.class_metadata.read();
                                class_metadata
                                    .get(nominal_type_id)
                                    .and_then(|meta| meta.get_method_index(key_str))
                                    .or_else(|| {
                                        drop(class_metadata);
                                        let classes = self.classes.read();
                                        let class = classes.get_class(nominal_type_id)?;
                                        let module = class.module.as_ref()?;
                                        module
                                            .classes
                                            .iter()
                                            .find(|class_def| class_def.name == class.name)
                                            .and_then(|class_def| {
                                                class_def.methods.iter().find_map(|method| {
                                                    let plain_name = method
                                                        .name
                                                        .rsplit("::")
                                                        .next()
                                                        .unwrap_or(method.name.as_str());
                                                    if method.name == key_str || plain_name == key_str {
                                                        Some(method.slot)
                                                    } else {
                                                        None
                                                    }
                                                })
                                            })
                                    })
                            },
                        )
                        {
                            match self.bound_method_value_for_slot(obj_val, method_slot) {
                                Ok(value) => value,
                                Err(_) => Value::null(),
                            }
                        } else {
                            let key = self.intern_prop_key(&key_str);
                            obj.dyn_map()
                                .and_then(|dyn_map| dyn_map.get(&key).copied())
                                .unwrap_or(Value::null())
                        }
                    }
                    _ => {
                        if let Some(key_str) = key_str.as_deref() {
                            if let Some(native_id) = builtin_handle_native_method_id(obj_val, key_str) {
                                let method = BoundNativeMethod {
                                    receiver: obj_val,
                                    native_id,
                                };
                                let method_ptr = self.gc.lock().allocate(method);
                                unsafe {
                                    Value::from_ptr(
                                        NonNull::new(method_ptr.as_ptr()).expect("bound native method ptr"),
                                    )
                                }
                            } else {
                                Value::null()
                            }
                        } else {
                            Value::null()
                        }
                    }
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
                        let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);
                        let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() }
                            .expect("JSView::Struct should always be an object");
                        let obj = unsafe { &mut *obj_ptr.as_ptr() };
                        let key_str = key_str
                            .as_deref()
                            .expect("dyn object property access should always have a key string");
                        let field_index = self.get_field_index_for_value(obj_val, &key_str);
                        if let Some(setter) = self.descriptor_accessor(actual_obj, &key_str, "set") {
                            match self.callable_frame_for_value(
                                setter,
                                stack,
                                &[value],
                                ReturnAction::Discard,
                            ) {
                                Ok(Some(frame)) => return frame,
                                Ok(None) => {
                                    return OpcodeResult::Error(VmError::TypeError(format!(
                                        "Property '{}' setter is not callable",
                                        key_str
                                    )));
                                }
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        }
                        if self
                            .descriptor_accessor(actual_obj, &key_str, "get")
                            .is_some()
                            && !self.is_field_writable(actual_obj, &key_str)
                        {
                            return OpcodeResult::Error(VmError::TypeError(format!(
                                "Cannot set property '{}' which has only a getter",
                                key_str
                            )));
                        }
                        if !self.is_field_writable(actual_obj, &key_str) {
                            return OpcodeResult::Error(VmError::TypeError(format!(
                                "Cannot assign to non-writable property '{}'",
                                key_str
                            )));
                        }
                        if let Some(index) = field_index {
                            let _ = obj.set_field(index, value);
                            self.sync_descriptor_value(actual_obj, &key_str, value);
                        } else {
                            let key = self.intern_prop_key(&key_str);
                            obj.ensure_dyn_map().insert(key, value);
                            self.sync_descriptor_value(actual_obj, &key_str, value);
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

            Opcode::NewSemaphore => {
                let permits_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let permits = permits_val
                    .as_i64()
                    .filter(|count| *count >= 0)
                    .unwrap_or(0) as usize;
                let (semaphore_id, _) = self.semaphore_registry.create_semaphore(permits);
                if let Err(e) = stack.push(Value::i64(semaphore_id.as_u64() as i64)) {
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
                            "Invalid nominal type id: {}",
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
                            "Invalid nominal type id: {}",
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
