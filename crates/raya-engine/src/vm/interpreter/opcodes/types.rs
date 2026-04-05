//! Type operation opcode handlers: nominal checks/casts, generic casts, dynamic keyed access,
//! and static/runtime type helpers.

use super::native::{
    checked_array_ptr, checked_bigint_ptr, checked_callable_ptr, checked_object_ptr,
    js_number_to_string,
};
use crate::compiler::native_id;
use crate::compiler::type_registry::TypeRegistry;
use crate::compiler::{Module, Opcode};
use crate::parser::TypeContext;
use crate::vm::builtin::{array, channel, map, regexp, set, string};
use crate::vm::gc::header_ptr_from_value_ptr;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::{Interpreter, ReturnAction};
use crate::vm::object::{
    layout_id_from_ordered_names, Array, CallableKind, ChannelObject, DynProp, MapObject, Object,
    RayaString, RegExpObject, SetObject, TypeHandle,
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

fn well_known_symbol_key(property: &str) -> Option<&'static str> {
    match property {
        "iterator" => Some("Symbol.iterator"),
        "toStringTag" => Some("Symbol.toStringTag"),
        "match" => Some("Symbol.match"),
        "matchAll" => Some("Symbol.matchAll"),
        "replace" => Some("Symbol.replace"),
        "search" => Some("Symbol.search"),
        "split" => Some("Symbol.split"),
        "species" => Some("Symbol.species"),
        "hasInstance" => Some("Symbol.hasInstance"),
        "isConcatSpreadable" => Some("Symbol.isConcatSpreadable"),
        "asyncIterator" => Some("Symbol.asyncIterator"),
        "toPrimitive" => Some("Symbol.toPrimitive"),
        "unscopables" => Some("Symbol.unscopables"),
        _ => None,
    }
}

fn well_known_symbol_lookup_name(property: &str) -> Option<&str> {
    if property.starts_with("Symbol.") {
        Some(property)
    } else {
        well_known_symbol_key(property)
    }
}

fn protocol_alias_names(key: &str) -> &'static [&'static str] {
    match key {
        "Symbol.iterator" => &["__iteratorObject", "iterator"],
        "Symbol.asyncIterator" => &["__asyncIteratorObject", "asyncIterator"],
        _ => &[],
    }
}

fn module_uses_js_numeric_semantics(module: &Module) -> bool {
    module
        .metadata
        .source_file
        .as_deref()
        .or(Some(module.metadata.name.as_str()))
        .map(|name| matches!(name.rsplit('.').next(), Some("js" | "mjs" | "cjs" | "jsx")))
        .unwrap_or(false)
}

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
    let header = unsafe { &*header_ptr_from_value_ptr(ptr.as_ptr()) };
    if header.type_id() == std::any::TypeId::of::<RayaString>() {
        return CAST_KIND_STRING;
    }
    if header.type_id() == std::any::TypeId::of::<Array>() {
        return CAST_KIND_ARRAY;
    }
    if header.type_id() == std::any::TypeId::of::<Object>() {
        let obj = unsafe { &*(ptr.as_ptr() as *const Object) };
        if obj.is_callable() {
            return CAST_KIND_FUNCTION;
        }
        return CAST_KIND_OBJECT;
    }
    0
}

fn object_ptr_checked(value: Value) -> Option<NonNull<Object>> {
    if !value.is_ptr() {
        return None;
    }
    let ptr = unsafe { value.as_ptr::<u8>() }?;
    let header = unsafe { &*header_ptr_from_value_ptr(ptr.as_ptr()) };
    if header.type_id() == std::any::TypeId::of::<Object>() {
        unsafe { value.as_ptr::<Object>() }
    } else {
        None
    }
}

pub(in crate::vm::interpreter) fn builtin_handle_native_method_id(
    pinned_handles: &parking_lot::RwLock<rustc_hash::FxHashSet<u64>>,
    value: Value,
    key: &str,
) -> Option<u16> {
    if value.is_ptr() {
        let ptr = unsafe { value.as_ptr::<u8>() }?;
        let header = unsafe { &*header_ptr_from_value_ptr(ptr.as_ptr()) };
        let ty = header.type_id();
        if ty == std::any::TypeId::of::<Object>() {
            let obj = unsafe { &*(ptr.as_ptr() as *const Object) };
            if obj.is_callable() {
                return match key {
                    "call" => Some(crate::compiler::native_id::FUNCTION_CALL_HELPER),
                    "apply" => Some(crate::compiler::native_id::FUNCTION_APPLY_HELPER),
                    "bind" => Some(crate::compiler::native_id::FUNCTION_BIND_HELPER),
                    _ => None,
                };
            }
        }
    }
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
        let header = unsafe { &*header_ptr_from_value_ptr(ptr.as_ptr()) };
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
                "values" => Some(array::VALUES),
                "keys" => Some(array::KEYS),
                "entries" => Some(array::ENTRIES),
                _ => None,
            };
        }
        if ty == std::any::TypeId::of::<RayaString>() {
            return match key {
                "charAt" => Some(string::CHAR_AT),
                "charCodeAt" => Some(string::CHAR_CODE_AT),
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
    if handle == 0 || !pinned_handles.read().contains(&handle) {
        return None;
    }
    let ptr = handle as *const u8;
    let header = unsafe { &*header_ptr_from_value_ptr(ptr) };
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

pub(in crate::vm::interpreter) fn dyn_key_parts(
    key_val: Value,
) -> Result<(Option<String>, Option<usize>), VmError> {
    use crate::vm::json::view::{js_classify, JSView};

    fn parse_array_index_string(key: &str) -> Option<usize> {
        if key.is_empty() {
            return None;
        }
        if key != "0" && key.starts_with('0') {
            return None;
        }
        let index = key.parse::<u32>().ok()?;
        if index == u32::MAX {
            return None;
        }
        if index.to_string() != key {
            return None;
        }
        Some(index as usize)
    }

    fn parse_array_index_number(number: f64) -> Option<usize> {
        if !number.is_finite() || number.fract() != 0.0 || number < 0.0 {
            return None;
        }
        if number >= u32::MAX as f64 {
            return None;
        }
        Some(number as usize)
    }

    if key_val.is_undefined() {
        return Ok((Some("undefined".to_string()), None));
    }

    match js_classify(key_val) {
        JSView::Str(ptr) => {
            let key = unsafe { &*ptr }.data.clone();
            let index = parse_array_index_string(&key);
            Ok((Some(key), index))
        }
        JSView::Int(index) => {
            if index >= 0 && i64::from(index) < u32::MAX as i64 {
                let index = index as usize;
                return Ok((Some(index.to_string()), Some(index)));
            }
            Ok((Some(index.to_string()), None))
        }
        JSView::Number(number) => {
            let key = js_number_to_string(number);
            let index = parse_array_index_number(number);
            Ok((Some(key), index))
        }
        JSView::Bool(value) => Ok((Some(if value { "true" } else { "false" }.to_string()), None)),
        JSView::Null => Ok((Some("null".to_string()), None)),
        _ => Err(VmError::TypeError(
            "DynGetKeyed key must be coercible to a property key".to_string(),
        )),
    }
}

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn task_promise_method_value(
        &self,
        target: Value,
        key: &str,
    ) -> Option<Value> {
        let debug_promise_lookup = std::env::var("RAYA_DEBUG_PROMISE_HANDLE_LOOKUP").is_ok();
        if self.promise_handle_from_value(target).is_none() {
            return None;
        }
        if !matches!(key, "then" | "catch" | "finally") {
            return None;
        }

        let promise_proto = self
            .builtin_global_value("Promise")
            .and_then(|promise_ctor| self.constructor_prototype_value(promise_ctor))
            .or_else(|| self.intrinsic_class_prototype_value("Promise"));
        if debug_promise_lookup {
            eprintln!(
                "[promise-handle] key={} proto={}",
                key,
                promise_proto
                    .map(|value| format!("{:#x}", value.raw()))
                    .unwrap_or_else(|| "none".to_string())
            );
        }
        let promise_proto = promise_proto?;
        let method = self
            .property_value_with_protocol_alias(promise_proto, key)
            .or_else(|| {
                self.prototype_chain_property_value_with_protocol_alias(
                    promise_proto,
                    promise_proto,
                    key,
                )
            });
        if debug_promise_lookup {
            eprintln!(
                "[promise-handle] key={} method={}",
                key,
                method
                    .map(|value| format!("{:#x}", value.raw()))
                    .unwrap_or_else(|| "none".to_string())
            );
        }
        let method = method?;
        if !Self::is_callable_value(method) {
            if debug_promise_lookup {
                eprintln!(
                    "[promise-handle] key={} callable=false raw={:#x}",
                    key,
                    method.raw()
                );
            }
            return None;
        }

        let bound = Object::new_bound_function(
            method,
            target,
            Vec::new(),
            format!("bound {key}"),
            Value::i32(0),
            false,
        );
        let bound_ptr = self.gc.lock().allocate(bound);
        Some(unsafe {
            Value::from_ptr(
                std::ptr::NonNull::new(bound_ptr.as_ptr()).expect("bound promise method"),
            )
        })
    }

    fn property_value_with_protocol_alias_for_receiver(
        &self,
        target: Value,
        receiver: Value,
        key: &str,
    ) -> Option<Value> {
        for candidate in std::iter::once(key).chain(protocol_alias_names(key).iter().copied()) {
            // Check fields first
            if let Some(value) = self.get_field_value_by_name(target, candidate) {
                return Some(value);
            }
            if let Some(value) = self.get_own_js_property_value_by_name(target, candidate) {
                return Some(value);
            }
            // Check dyn_props
            if let Some(obj_ptr) = checked_object_ptr(target) {
                let obj = unsafe { &*obj_ptr.as_ptr() };
                let key_id = self.intern_prop_key(candidate);
                if let Some(prop) = obj.dyn_props.as_deref().and_then(|dp| dp.get(key_id)) {
                    if !prop.is_accessor {
                        return Some(prop.value);
                    }
                }
            }
            // Check method vtable
            if let Some(obj_ptr) = checked_object_ptr(target) {
                let obj = unsafe { &*obj_ptr.as_ptr() };
                if let Some(method_slot) = obj.nominal_type_id_usize().and_then(|ntid| {
                    let class_metadata = self.class_metadata.read();
                    class_metadata
                        .get(ntid)
                        .and_then(|meta| meta.get_method_index(candidate))
                        .or_else(|| {
                            drop(class_metadata);
                            let classes = self.classes.read();
                            let class = classes.get_class(ntid)?;
                            let module = class.module.as_ref()?;
                            module
                                .classes
                                .iter()
                                .find(|cd| cd.name == class.name)
                                .and_then(|cd| {
                                    cd.methods.iter().find_map(|m| {
                                        let plain = m.name.rsplit("::").next().unwrap_or(&m.name);
                                        if m.name == candidate || plain == candidate {
                                            Some(m.slot)
                                        } else {
                                            None
                                        }
                                    })
                                })
                        })
                }) {
                    if let Ok(value) = self.bound_method_value_for_slot(receiver, method_slot) {
                        return Some(value);
                    }
                }
            }
            if let Some(value) = self.task_promise_method_value(receiver, candidate) {
                return Some(value);
            }
            // Check builtin native methods
            if let Some(native_id) =
                builtin_handle_native_method_id(self.pinned_handles, target, candidate)
            {
                let method = Object::new_bound_native(receiver, native_id);
                let method_ptr = self.gc.lock().allocate(method);
                let val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(method_ptr.as_ptr()).unwrap())
                };
                return Some(val);
            }
        }
        None
    }

    fn property_value_with_protocol_alias(&self, target: Value, key: &str) -> Option<Value> {
        self.property_value_with_protocol_alias_for_receiver(target, target, key)
    }

    fn prototype_chain_property_value_with_protocol_alias(
        &self,
        target: Value,
        receiver: Value,
        key: &str,
    ) -> Option<Value> {
        let mut current = self.prototype_of_value(target);
        let mut seen = vec![target.raw()];
        while let Some(prototype) = current {
            if seen.contains(&prototype.raw()) {
                break;
            }
            seen.push(prototype.raw());
            if let Some(value) =
                self.property_value_with_protocol_alias_for_receiver(prototype, receiver, key)
            {
                return Some(value);
            }
            current = self.prototype_of_value(prototype);
        }
        None
    }

    fn descriptor_accessor_with_protocol_alias(
        &self,
        target: Value,
        key: &str,
        accessor: &str,
    ) -> Option<Value> {
        self.descriptor_accessor(target, key, accessor)
    }

    fn descriptor_data_value_with_protocol_alias(&self, target: Value, key: &str) -> Option<Value> {
        self.descriptor_data_value(target, key)
    }

    fn is_symbol_constructor_value(&self, value: Value) -> bool {
        if value.is_ptr() {
            if let Some(raw_ptr) = unsafe { value.as_ptr::<u8>() } {
                let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };
                if header.type_id() == std::any::TypeId::of::<TypeHandle>() {
                    if let Some(handle_ptr) = unsafe { value.as_ptr::<TypeHandle>() } {
                        let handle_id = unsafe { (*handle_ptr.as_ptr()).handle_id };
                        if let Some(entry) = self.type_handles.read().get(handle_id) {
                            let classes = self.classes.read();
                            if classes
                                .get_class(entry.nominal_type_id as usize)
                                .is_some_and(|class| class.name == "Symbol")
                            {
                                return true;
                            }
                        }
                    }
                }
            }
        }

        self.builtin_global_value("Symbol")
            .is_some_and(|global| global.raw() == value.raw())
    }

    pub(in crate::vm::interpreter) fn well_known_symbol_property_value(
        &mut self,
        object_value: Value,
        property: &str,
        caller_task: &std::sync::Arc<crate::vm::scheduler::Task>,
        caller_module: &crate::compiler::Module,
    ) -> Result<Option<Value>, VmError> {
        let Some(symbol_key) = well_known_symbol_lookup_name(property) else {
            return Ok(None);
        };
        if let Some(getter) =
            self.descriptor_accessor_with_protocol_alias(object_value, symbol_key, "get")
        {
            if !Self::is_callable_value(getter) {
                return Err(VmError::TypeError(format!(
                    "Property '{}' getter is not callable",
                    symbol_key
                )));
            }
            let value = self.invoke_callable_sync_with_this(
                getter,
                Some(object_value),
                &[],
                caller_task,
                caller_module,
            )?;
            return Ok(Some(value));
        }
        if let Some(value) =
            self.descriptor_data_value_with_protocol_alias(object_value, symbol_key)
        {
            return Ok(Some(value));
        }
        if let Some(value) = self
            .property_value_with_protocol_alias(object_value, symbol_key)
            .or_else(|| {
                self.prototype_chain_property_value_with_protocol_alias(
                    object_value,
                    object_value,
                    symbol_key,
                )
            })
        {
            return Ok(Some(value));
        }
        if let Some(value) = self.get_property_value_via_js_semantics_with_context(
            object_value,
            symbol_key,
            caller_task,
            caller_module,
        )? {
            return Ok(Some(value));
        }

        if symbol_key == "Symbol.iterator" {
            let fallback_method = if checked_array_ptr(object_value).is_some() {
                Some((Some("values"), None))
            } else if self.map_handle_from_value(object_value).is_ok() {
                Some((Some("entries"), Some(crate::compiler::native_id::MAP_ENTRIES)))
            } else if self.set_handle_from_value(object_value).is_ok() {
                Some((Some("values"), Some(crate::compiler::native_id::SET_VALUES)))
            } else if object_value.is_ptr() {
                if let Some(ptr) = unsafe { object_value.as_ptr::<u8>() } {
                    let header = unsafe { &*header_ptr_from_value_ptr(ptr.as_ptr()) };
                    let ty = header.type_id();
                    if ty == std::any::TypeId::of::<MapObject>() {
                        Some((Some("entries"), Some(crate::compiler::native_id::MAP_ENTRIES)))
                    } else if ty == std::any::TypeId::of::<SetObject>() {
                        Some((Some("values"), Some(crate::compiler::native_id::SET_VALUES)))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            if let Some((fallback_method_name, fallback_native_id)) = fallback_method {
                if let Some(method_name) = fallback_method_name {
                    if let Some(value) = self.prototype_chain_property_value_with_protocol_alias(
                        object_value,
                        object_value,
                        method_name,
                    ) {
                        return Ok(Some(value));
                    }
                    if let Some(value) = self.get_property_value_via_js_semantics_with_context(
                        object_value,
                        method_name,
                        caller_task,
                        caller_module,
                    )? {
                        return Ok(Some(value));
                    }
                    if let Some(value) =
                        self.property_value_with_protocol_alias(object_value, method_name)
                    {
                        return Ok(Some(value));
                    }
                }
                if let Some(native_id) = fallback_native_id {
                    return Ok(Some(self.alloc_unbound_native_value(native_id)));
                }
            }
        }

        if self.fixed_property_deleted(object_value, symbol_key) {
            return Ok(None);
        }

        if !self.is_symbol_constructor_value(object_value) {
            return Ok(None);
        }

        let static_name = symbol_key.strip_prefix("Symbol.").unwrap_or(symbol_key);
        Ok(self.get_field_value_by_name(object_value, static_name))
    }

    pub(in crate::vm::interpreter) fn property_key_parts(
        &self,
        key_val: Value,
        op_name: &str,
    ) -> Result<(Option<String>, Option<usize>), VmError> {
        if let Ok(parts) = dyn_key_parts(key_val) {
            return Ok(parts);
        }

        for kind in ["Boolean", "Number", "String"] {
            if let Some(primitive) = self.boxed_primitive_internal_value(key_val, kind) {
                if let Ok(parts) = dyn_key_parts(primitive) {
                    return Ok(parts);
                }
            }
        }

        let Some(obj_ptr) = (unsafe { key_val.as_ptr::<Object>() }) else {
            return Err(VmError::TypeError(format!(
                "{op_name} key must be a string, symbol, or non-negative integer"
            )));
        };

        if !self.is_symbol_value(key_val) {
            return Err(VmError::TypeError(format!(
                "{op_name} key must be a string, symbol, or non-negative integer"
            )));
        }

        let Some(key) = self.symbol_property_key_name(key_val) else {
            return Err(VmError::TypeError(format!(
                "{op_name} symbol key is missing its internal string"
            )));
        };
        Ok((Some(key), None))
    }

    pub(in crate::vm::interpreter) fn property_key_parts_with_context(
        &mut self,
        key_val: Value,
        op_name: &str,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<(Option<String>, Option<usize>), VmError> {
        if let Ok(parts) = self.property_key_parts(key_val, op_name) {
            return Ok(parts);
        }

        let hint_ptr = self
            .gc
            .lock()
            .allocate(RayaString::new("string".to_string()));
        let hint_value =
            unsafe { Value::from_ptr(NonNull::new(hint_ptr.as_ptr()).expect("property key hint")) };
        self.ephemeral_gc_roots.write().push(hint_value);

        let cleanup_hint = |roots: &mut Vec<Value>, hint_value: Value| {
            if let Some(index) = roots.iter().rposition(|candidate| *candidate == hint_value) {
                roots.swap_remove(index);
            }
        };

        if let Ok(Some(exotic)) =
            self.well_known_symbol_property_value(key_val, "Symbol.toPrimitive", task, module)
        {
            if !Self::is_callable_value(exotic) {
                let mut roots = self.ephemeral_gc_roots.write();
                cleanup_hint(&mut roots, hint_value);
                return Err(VmError::TypeError(
                    "Cannot convert object to primitive value".to_string(),
                ));
            }
            let result = self.invoke_callable_sync_with_this(
                exotic,
                Some(key_val),
                &[hint_value],
                task,
                module,
            )?;
            if let Ok(parts) = self.property_key_parts(result, op_name) {
                let mut roots = self.ephemeral_gc_roots.write();
                cleanup_hint(&mut roots, hint_value);
                return Ok(parts);
            }
            let mut roots = self.ephemeral_gc_roots.write();
            cleanup_hint(&mut roots, hint_value);
            return Err(VmError::TypeError(
                "Cannot convert object to primitive value".to_string(),
            ));
        }

        for method_name in ["toString", "valueOf"] {
            if let Some(method) = self.get_field_value_by_name(key_val, method_name) {
                if !Self::is_callable_value(method) {
                    continue;
                }
                let result =
                    self.invoke_callable_sync_with_this(method, Some(key_val), &[], task, module)?;
                if let Ok(parts) = self.property_key_parts(result, op_name) {
                    let mut roots = self.ephemeral_gc_roots.write();
                    cleanup_hint(&mut roots, hint_value);
                    return Ok(parts);
                }
            }
        }

        let mut roots = self.ephemeral_gc_roots.write();
        cleanup_hint(&mut roots, hint_value);
        Err(VmError::TypeError(
            "Cannot convert object to primitive value".to_string(),
        ))
    }

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
        self.record_aot_shape_site(crate::aot_profile::AotSiteKind::CastShape, obj.layout_id());
        let Some(adapter) = self.ensure_shape_adapter_for_object(obj, required_shape) else {
            return OpcodeResult::Error(VmError::TypeError(format!(
                "Cannot cast object(layout_id={}) to structural shape @{required_shape:016x}",
                obj.layout_id()
            )));
        };
        let required_names = self
            .structural_shape_names
            .read()
            .get(&required_shape)
            .cloned()
            .unwrap_or_default();
        for slot in 0..adapter.len() {
            if matches!(
                adapter.binding_for_slot(slot),
                crate::vm::interpreter::shared_state::StructuralSlotBinding::Missing
            ) && required_names.get(slot).map_or(true, |name| {
                !matches!(
                    name.as_str(),
                    "constructor"
                        | "equals"
                        | "hashCode"
                        | "hasOwnProperty"
                        | "isPrototypeOf"
                        | "propertyIsEnumerable"
                        | "toLocaleString"
                        | "toString"
                        | "valueOf"
                ) && !self.has_property_via_js_semantics(obj_val, name)
            }) {
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
            .max(obj.dyn_props().map(|dp| dp.len()).unwrap_or(0));
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
                let required_names = self
                    .structural_shape_names
                    .read()
                    .get(&required_shape)
                    .cloned()
                    .unwrap_or_default();
                (0..adapter.len()).all(|slot| {
                    !matches!(
                        adapter.binding_for_slot(slot),
                        crate::vm::interpreter::shared_state::StructuralSlotBinding::Missing
                    ) || required_names.get(slot).is_some_and(|name| {
                        matches!(
                            name.as_str(),
                            "constructor" | "equals" | "hashCode" | "isPrototypeOf" | "valueOf"
                        ) || self.has_property_via_js_semantics(obj_val, name)
                    })
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
                TypeRegistry::new(
                    &type_ctx,
                    crate::compiler::module::BuiltinSurfaceMode::NodeCompat,
                )
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
        let (actual_nominal_type_id, actual_name) =
            if let Some(obj_ptr) = object_ptr_checked(obj_val) {
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

    pub(crate) fn exec_type_ops(
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

                if std::env::var("RAYA_DEBUG_INSTANCEOF").is_ok() {
                    let object_nominal_type_id =
                        object_ptr_checked(obj_val).and_then(|obj_ptr| unsafe {
                            (&*obj_ptr.as_ptr()).nominal_type_id_usize()
                        });
                    eprintln!(
                        "[is-nominal] object={:#x} object_nominal={:?} target_nominal={} result={}",
                        obj_val.raw(),
                        object_nominal_type_id,
                        target_nominal_type_id,
                        result
                    );
                }

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

                let (key_str, array_index) = match self.property_key_parts_with_context(
                    key_val,
                    "DynGetKeyed",
                    task,
                    module,
                ) {
                    Ok(parts) => parts,
                    Err(error) => return OpcodeResult::Error(error),
                };

                // ES spec: TypeError when accessing properties on null or undefined
                if obj_val.is_null() || obj_val.is_undefined() {
                    let type_name = if obj_val.is_null() {
                        "null"
                    } else {
                        "undefined"
                    };
                    let key_display = key_str.as_deref().unwrap_or("<computed>");
                    return OpcodeResult::Error(VmError::TypeError(format!(
                        "Cannot read properties of {} (reading '{}')",
                        type_name, key_display
                    )));
                }

                if let Some(key_str) = key_str.as_deref() {
                    match self.try_proxy_like_get_property(obj_val, key_str, task, module) {
                        Ok(Some(value)) => {
                            if let Err(e) = stack.push(value) {
                                return OpcodeResult::Error(e);
                            }
                            return OpcodeResult::Continue;
                        }
                        Ok(None) => {}
                        Err(error) => return OpcodeResult::Error(error),
                    }
                }

                let result = match js_classify(obj_val) {
                    JSView::Str(ptr) => {
                        let s = unsafe { &*ptr };
                        if let Some(index) = array_index {
                            if let Some(ch) = s.data.chars().nth(index) {
                                let gc_ptr =
                                    self.gc.lock().allocate(RayaString::new(ch.to_string()));
                                unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                    )
                                }
                            } else {
                                Value::undefined()
                            }
                        } else if key_str.as_deref() == Some("length") {
                            Value::i32(s.len() as i32)
                        } else if let Some(key_str) = key_str.as_deref() {
                            if let Some(value) = match self
                                .well_known_symbol_property_value(obj_val, key_str, task, module)
                            {
                                Ok(value) => value,
                                Err(error) => return OpcodeResult::Error(error),
                            } {
                                value
                            } else if let Some(value) =
                                self.get_own_js_property_value_by_name(obj_val, key_str)
                            {
                                value
                            } else
                            if let Some(value) = match self
                                .get_property_value_via_js_semantics_with_context(
                                    obj_val, key_str, task, module,
                                ) {
                                Ok(value) => value,
                                Err(error) => return OpcodeResult::Error(error),
                            } {
                                value
                            } else if let Some(native_id) = builtin_handle_native_method_id(
                                self.pinned_handles,
                                obj_val,
                                key_str,
                            )
                            .or_else(|| {
                                for alias in protocol_alias_names(key_str) {
                                    if let Some(native_id) = builtin_handle_native_method_id(
                                        self.pinned_handles,
                                        obj_val,
                                        alias,
                                    ) {
                                        return Some(native_id);
                                    }
                                }
                                None
                            }) {
                                let method = Object::new_bound_native(obj_val, native_id);
                                let method_ptr = self.gc.lock().allocate(method);
                                unsafe {
                                    Value::from_ptr(
                                        NonNull::new(method_ptr.as_ptr())
                                            .expect("bound native method ptr"),
                                    )
                                }
                            } else {
                                Value::undefined()
                            }
                        } else {
                            Value::undefined()
                        }
                    }
                    JSView::Arr(ptr) => {
                        let arr = unsafe { &*ptr };
                        if let Some(index) = array_index {
                            if let Some(value) = arr.get(index) {
                                value
                            } else if let Some(key_str) = key_str.as_deref() {
                                if let Some(value) = match self
                                    .well_known_symbol_property_value(obj_val, key_str, task, module)
                                {
                                    Ok(value) => value,
                                    Err(error) => return OpcodeResult::Error(error),
                                } {
                                    value
                                } else if let Some(value) =
                                    self.get_own_js_property_value_by_name(obj_val, key_str)
                                {
                                    value
                                } else
                                if let Some(value) = match self
                                    .get_property_value_via_js_semantics_with_context(
                                        obj_val, key_str, task, module,
                                    ) {
                                    Ok(value) => value,
                                    Err(error) => return OpcodeResult::Error(error),
                                } {
                                    value
                                } else {
                                    Value::undefined()
                                }
                            } else {
                                Value::undefined()
                            }
                        } else if let Some(key_str) = key_str.as_deref() {
                            if let Some(value) = match self
                                .well_known_symbol_property_value(obj_val, key_str, task, module)
                            {
                                Ok(value) => value,
                                Err(error) => return OpcodeResult::Error(error),
                            } {
                                value
                            } else if let Some(value) =
                                self.get_own_js_property_value_by_name(obj_val, key_str)
                            {
                                value
                            } else
                            if let Some(value) = match self
                                .get_property_value_via_js_semantics_with_context(
                                    obj_val, key_str, task, module,
                                ) {
                                Ok(value) => value,
                                Err(error) => return OpcodeResult::Error(error),
                            } {
                                value
                            } else if let Some(native_id) = builtin_handle_native_method_id(
                                self.pinned_handles,
                                obj_val,
                                key_str,
                            )
                            .or_else(|| {
                                for alias in protocol_alias_names(key_str) {
                                    if let Some(native_id) = builtin_handle_native_method_id(
                                        self.pinned_handles,
                                        obj_val,
                                        alias,
                                    ) {
                                        return Some(native_id);
                                    }
                                }
                                None
                            }) {
                                let method = Object::new_bound_native(obj_val, native_id);
                                let method_ptr = self.gc.lock().allocate(method);
                                unsafe {
                                    Value::from_ptr(
                                        NonNull::new(method_ptr.as_ptr())
                                            .expect("bound native method ptr"),
                                    )
                                }
                            } else {
                                Value::undefined()
                            }
                        } else {
                            Value::undefined()
                        }
                    }
                    JSView::Struct { ptr, .. } => {
                        let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);
                        let _obj_ptr = unsafe { actual_obj.as_ptr::<Object>() }
                            .expect("JSView::Struct should always be an object");
                        let key_str = key_str
                            .as_deref()
                            .expect("dyn object property access should always have a key string");

                        // Well-known symbol short-circuit
                        if let Some(result) = match self
                            .well_known_symbol_property_value(actual_obj, key_str, task, module)
                        {
                            Ok(value) => value,
                            Err(error) => return OpcodeResult::Error(error),
                        } {
                            if let Err(e) = stack.push(result) {
                                return OpcodeResult::Error(e);
                            }
                            return OpcodeResult::Continue;
                        }

                        match self.get_property_value_via_js_semantics_with_context(
                            actual_obj, key_str, task, module,
                        ) {
                            Ok(Some(value)) => value,
                            Ok(None) => Value::undefined(),
                            Err(error) => return OpcodeResult::Error(error),
                        }
                    }
                    _ => {
                        if let Some(key_str) = key_str.as_deref() {
                            if let Some(value) = match self
                                .well_known_symbol_property_value(obj_val, key_str, task, module)
                            {
                                Ok(value) => value,
                                Err(error) => return OpcodeResult::Error(error),
                            } {
                                value
                            } else {
                                match self.get_property_value_via_js_semantics_with_context(
                                    obj_val, key_str, task, module,
                                ) {
                                    Ok(Some(value)) => value,
                                    Ok(None) => {
                                        if let Some(native_id) = builtin_handle_native_method_id(
                                            self.pinned_handles,
                                            obj_val,
                                            key_str,
                                        ) {
                                            let method =
                                                Object::new_bound_native(obj_val, native_id);
                                            let method_ptr = self.gc.lock().allocate(method);
                                            unsafe {
                                                Value::from_ptr(
                                                    NonNull::new(method_ptr.as_ptr())
                                                        .expect("bound native method ptr"),
                                                )
                                            }
                                        } else {
                                            Value::undefined()
                                        }
                                    }
                                    Err(error) => return OpcodeResult::Error(error),
                                }
                            }
                        } else {
                            Value::undefined()
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

                let (key_str, array_index) = match self.property_key_parts_with_context(
                    key_val,
                    "DynSetKeyed",
                    task,
                    module,
                ) {
                    Ok(parts) => parts,
                    Err(error) => return OpcodeResult::Error(error),
                };

                // ES spec: TypeError when setting properties on null or undefined
                if obj_val.is_null() || obj_val.is_undefined() {
                    let type_name = if obj_val.is_null() {
                        "null"
                    } else {
                        "undefined"
                    };
                    let key_display = key_str.as_deref().unwrap_or("<computed>");
                    return OpcodeResult::Error(VmError::TypeError(format!(
                        "Cannot set properties of {} (setting '{}')",
                        type_name, key_display
                    )));
                }

                if (matches!(js_classify(obj_val), JSView::Arr(_) | JSView::Struct { .. })
                    || Self::is_callable_value(obj_val))
                    && key_str.is_some()
                {
                    let key_name = key_str.as_deref().expect("checked is_some");
                    match self.set_property_value_via_js_semantics(
                        obj_val, key_name, value, obj_val, task, module,
                    ) {
                        Ok(true) => {
                            if let Err(e) = stack.push(value) {
                                return OpcodeResult::Error(e);
                            }
                            return OpcodeResult::Continue;
                        }
                        Ok(false) => {
                            if self
                                .allow_ambient_builtin_global_noop_write(obj_val, key_name, value)
                            {
                                if let Err(e) = stack.push(value) {
                                    return OpcodeResult::Error(e);
                                }
                                return OpcodeResult::Continue;
                            }
                            if self.current_js_code_is_strict(task, module) {
                                return OpcodeResult::Error(VmError::TypeError(format!(
                                    "Cannot assign to non-writable property '{}'",
                                    key_name
                                )));
                            }
                            if let Err(e) = stack.push(value) {
                                return OpcodeResult::Error(e);
                            }
                            return OpcodeResult::Continue;
                        }
                        Err(error) => return OpcodeResult::Error(error),
                    }
                }

                match js_classify(obj_val) {
                    JSView::Arr(ptr) => {
                        if let Some(index) = array_index {
                            let arr = unsafe { &mut *(ptr as *mut Array) };
                            let _ = arr.set(index, value);
                        } else {
                            let Some(key_str) = key_str.as_deref() else {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "DynSetKeyed array property key must be string-like"
                                        .to_string(),
                                ));
                            };
                            if key_str == "length" {
                                match self.set_property_value_via_js_semantics(
                                    obj_val, key_str, value, obj_val, task, module,
                                ) {
                                    Ok(_) => {}
                                    Err(error) => return OpcodeResult::Error(error),
                                }
                                if let Err(e) = stack.push(value) {
                                    return OpcodeResult::Error(e);
                                }
                                return OpcodeResult::Continue;
                            }
                            if let Err(error) = self.define_data_property_on_target(
                                obj_val, key_str, value, true, true, true,
                            ) {
                                return OpcodeResult::Error(error);
                            }
                        }
                    }
                    JSView::Struct { ptr, .. } => {
                        let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);
                        let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() }
                            .expect("JSView::Struct should always be an object");
                        let obj = unsafe { &mut *obj_ptr.as_ptr() };
                        let key_str = key_str
                            .as_deref()
                            .expect("dyn object property access should always have a key string");
                        if self.is_ambient_math_constant_target(actual_obj, key_str) {
                            if self.current_js_code_is_strict(task, module) {
                                return OpcodeResult::Error(VmError::TypeError(format!(
                                    "Cannot assign to non-writable property '{}'",
                                    key_str
                                )));
                            }
                            if let Err(e) = stack.push(value) {
                                return OpcodeResult::Error(e);
                            }
                            return OpcodeResult::Continue;
                        }
                        if self.is_runtime_global_object(actual_obj) {
                            match self.set_property_value_via_js_semantics(
                                actual_obj, key_str, value, actual_obj, task, module,
                            ) {
                                Ok(true) => {
                                    if let Err(e) = stack.push(value) {
                                        return OpcodeResult::Error(e);
                                    }
                                    return OpcodeResult::Continue;
                                }
                                Ok(false) => {
                                    if self.allow_ambient_builtin_global_noop_write(
                                        actual_obj, key_str, value,
                                    ) {
                                        if let Err(e) = stack.push(value) {
                                            return OpcodeResult::Error(e);
                                        }
                                        return OpcodeResult::Continue;
                                    }
                                    if self.current_js_code_is_strict(task, module) {
                                        return OpcodeResult::Error(VmError::TypeError(format!(
                                            "Cannot assign to non-writable property '{}'",
                                            key_str
                                        )));
                                    }
                                    if let Err(e) = stack.push(value) {
                                        return OpcodeResult::Error(e);
                                    }
                                    return OpcodeResult::Continue;
                                }
                                Err(error) => return OpcodeResult::Error(error),
                            }
                        }
                        if Self::is_callable_value(actual_obj) {
                            if let Some((writable, _, _)) =
                                self.callable_virtual_property_descriptor(actual_obj, key_str)
                            {
                                if !writable {
                                    if let Err(e) = stack.push(value) {
                                        return OpcodeResult::Error(e);
                                    }
                                    return OpcodeResult::Continue;
                                }
                            }
                        }
                        let field_index = self.get_field_index_for_value(obj_val, &key_str);
                        if let Some(setter) = self.descriptor_accessor(actual_obj, &key_str, "set")
                        {
                            match self.callable_frame_for_value(
                                setter,
                                stack,
                                &[value],
                                Some(actual_obj),
                                ReturnAction::Discard,
                                module,
                                task,
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
                            if self.current_js_code_is_strict(task, module) {
                                return OpcodeResult::Error(VmError::TypeError(format!(
                                    "Cannot set property '{}' which has only a getter",
                                    key_str
                                )));
                            }
                            if let Err(e) = stack.push(value) {
                                return OpcodeResult::Error(e);
                            }
                            return OpcodeResult::Continue;
                        }
                        if !self.is_field_writable(actual_obj, &key_str) {
                            if self.current_js_code_is_strict(task, module) {
                                return OpcodeResult::Error(VmError::TypeError(format!(
                                    "Cannot assign to non-writable property '{}'",
                                    key_str
                                )));
                            }
                            if let Err(e) = stack.push(value) {
                                return OpcodeResult::Error(e);
                            }
                            return OpcodeResult::Continue;
                        }
                        if let Some(index) = field_index {
                            let _ = obj.set_field(index, value);
                            self.sync_descriptor_value(actual_obj, &key_str, value);
                            self.set_callable_virtual_property_deleted(actual_obj, &key_str, false);
                            self.set_descriptor_field_present(actual_obj, &key_str, true);
                        } else {
                            let key = self.intern_prop_key(&key_str);
                            obj.ensure_dyn_props().insert(key, DynProp::data(value));
                            self.sync_descriptor_value(actual_obj, &key_str, value);
                            self.set_callable_virtual_property_deleted(actual_obj, &key_str, false);
                            self.set_descriptor_field_present(actual_obj, &key_str, true);
                        }
                    }
                    _ => {
                        let Some(key_str) = key_str.as_deref() else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "DynSetKeyed target must be an object".to_string(),
                            ));
                        };
                        if !obj_val.is_ptr() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "DynSetKeyed target must be an object".to_string(),
                            ));
                        }
                        match self.set_property_value_via_js_semantics(
                            obj_val, key_str, value, obj_val, task, module,
                        ) {
                            Ok(true) => {}
                            Ok(false) => {
                                if self.allow_ambient_builtin_global_noop_write(
                                    obj_val, key_str, value,
                                ) {
                                    if let Err(e) = stack.push(value) {
                                        return OpcodeResult::Error(e);
                                    }
                                    return OpcodeResult::Continue;
                                }
                                if self.current_js_code_is_strict(task, module) {
                                    return OpcodeResult::Error(VmError::TypeError(format!(
                                        "Cannot assign to non-writable property '{}'",
                                        key_str
                                    )));
                                }
                                if let Err(e) = stack.push(value) {
                                    return OpcodeResult::Error(e);
                                }
                                return OpcodeResult::Continue;
                            }
                            Err(error) => return OpcodeResult::Error(error),
                        }
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                    }
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
                let js_numeric_semantics = module_uses_js_numeric_semantics(module);

                let type_str = if value.is_null() {
                    "object" // ES spec: typeof null === "object"
                } else if value.is_bool() {
                    "boolean"
                } else if js_numeric_semantics && checked_bigint_ptr(value).is_some() {
                    "bigint"
                } else if value.is_u64() && self.promise_handle_from_value(value).is_some() {
                    // Async JS promises are represented by task handles internally, but
                    // observable JS semantics still require `typeof promise === "object"`.
                    "object"
                } else if value.is_i32()
                    || value.is_f64()
                    || value.is_f32()
                    || value.is_u32()
                    || value.is_i64()
                    || value.is_u64()
                {
                    "number"
                } else if self.callable_function_info(value).is_some() {
                    "function"
                } else if value.is_ptr() {
                    // Pointer values need runtime type header inspection.
                    // `Value::as_ptr<T>` does not verify `T`, so checking
                    // as_ptr::<RayaString>() directly is unsound here.
                    let ptr = unsafe { value.as_ptr::<u8>() }.expect("pointer checked");
                    let header = unsafe { &*header_ptr_from_value_ptr(ptr.as_ptr()) };
                    if header.type_id() == std::any::TypeId::of::<RayaString>() {
                        "string"
                    } else if header.type_id() == std::any::TypeId::of::<TypeHandle>() {
                        "function"
                    } else if header.type_id() == std::any::TypeId::of::<Object>() {
                        if let Some(obj_ptr) = unsafe { value.as_ptr::<Object>() } {
                            let obj = unsafe { &*obj_ptr.as_ptr() };
                            if obj.is_callable() {
                                "function"
                            } else {
                                let layout_is_symbol = self
                                    .layouts
                                    .read()
                                    .get_layout(obj.layout_id())
                                    .and_then(|layout| layout.name.as_deref().map(str::to_string))
                                    .as_deref()
                                    == Some("Symbol");
                                if layout_is_symbol {
                                    "symbol"
                                } else {
                                    let handle_key = self.intern_prop_key("__raya_type_handle__");
                                    if obj
                                        .dyn_props()
                                        .is_some_and(|dp| dp.contains_key(handle_key))
                                    {
                                        "function"
                                    } else {
                                        "object"
                                    }
                                }
                            }
                        } else {
                            "object"
                        }
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
