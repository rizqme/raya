//! Native call opcode handlers: NativeCall, ModuleNativeCall
//!
//! NativeCall dispatches to built-in operations (channel, buffer, map, set, date, regexp, etc.)
//! and reflect/runtime methods. ModuleNativeCall dispatches through the resolved natives table.

use crate::compiler::native_id::{
    CHANNEL_CAPACITY, CHANNEL_CLOSE, CHANNEL_IS_CLOSED, CHANNEL_LENGTH, CHANNEL_NEW,
    CHANNEL_RECEIVE, CHANNEL_SEND, CHANNEL_TRY_RECEIVE, CHANNEL_TRY_SEND,
};
use crate::compiler::{Compiler, Module, Opcode};
use crate::parser::checker::{Binder, CheckerPolicy, ScopeId, TypeChecker, TypeSystemMode};
use crate::parser::{Parser, TypeContext};
use crate::vm::builtin::{buffer, date, map, mutex, regexp, set, url};
use crate::vm::gc::header_ptr_from_value_ptr;
use crate::vm::interpreter::execution::{OpcodeResult, ReturnAction};
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{
    layout_id_from_ordered_names, Array, BoundFunction, BoundMethod, BoundNativeMethod, Buffer,
    ChannelObject, Class, Closure, DateObject, LayoutId, MapObject, Object, RayaString,
    RegExpObject, SetObject, TypeHandle,
};
use crate::vm::scheduler::{Task, TaskId, TaskState};
use crate::vm::stack::Stack;
use crate::vm::sync::MutexId;
use crate::vm::value::Value;
use crate::vm::VmError;
use rustc_hash::FxHashSet;
use std::any::TypeId;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

const NODE_DESCRIPTOR_METADATA_KEY: &str = "__node_compat_descriptor";
const NON_OBJECT_DYNAMIC_VALUE_METADATA_KEY: &str = "__dynamic_value_property";
const CALLABLE_VIRTUAL_DELETE_METADATA_KEY: &str = "__callable_virtual_deleted";
const CALLABLE_VIRTUAL_VALUE_METADATA_KEY: &str = "__callable_virtual_value";
const FIXED_PROPERTY_DELETE_METADATA_KEY: &str = "__fixed_property_deleted";
const OBJECT_PROTOTYPE_OVERRIDE_METADATA_KEY: &str = "__object_prototype_override__";
const OBJECT_DESCRIPTOR_MARKER_METADATA_KEY: &str = "__object_descriptor_marker__";
const OBJECT_DESCRIPTOR_PRESENT_METADATA_KEY: &str = "__object_descriptor_present__";
const OBJECT_EXTENSIBLE_METADATA_KEY: &str = "__object_extensible__";
const IMPORTED_CLASS_TYPE_HANDLE_KEY: &str = "__raya_type_handle__";
static DYNAMIC_JS_FUNCTION_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy)]
struct JsPropertyDescriptorRecord {
    has_value: bool,
    value: Value,
    has_writable: bool,
    writable: bool,
    has_configurable: bool,
    configurable: bool,
    has_enumerable: bool,
    enumerable: bool,
    has_get: bool,
    get: Value,
    has_set: bool,
    set: Value,
}

impl Default for JsPropertyDescriptorRecord {
    fn default() -> Self {
        Self {
            has_value: false,
            value: Value::undefined(),
            has_writable: false,
            writable: false,
            has_configurable: false,
            configurable: false,
            has_enumerable: false,
            enumerable: false,
            has_get: false,
            get: Value::undefined(),
            has_set: false,
            set: Value::undefined(),
        }
    }
}

fn value_as_string(arg: Value) -> Result<String, VmError> {
    if !arg.is_ptr() {
        return Err(VmError::TypeError("Expected string".to_string()));
    }
    let Some(s) = (unsafe { arg.as_ptr::<RayaString>() }) else {
        return Err(VmError::TypeError("Expected string".to_string()));
    };
    Ok(unsafe { &*s.as_ptr() }.data.clone())
}

fn primitive_to_js_string(value: Value) -> Option<String> {
    if value.is_undefined() {
        return Some("undefined".to_string());
    }
    if value.is_null() {
        return Some("null".to_string());
    }
    if let Some(value) = value.as_bool() {
        return Some(if value { "true" } else { "false" }.to_string());
    }
    if let Some(value) = value.as_i32() {
        return Some(value.to_string());
    }
    if let Some(value) = value.as_f64() {
        return Some(value.to_string());
    }
    let string_ptr = unsafe { value.as_ptr::<RayaString>() }?;
    Some(unsafe { &*string_ptr.as_ptr() }.data.clone())
}

fn boxed_primitive_helper_class_name(class_name: &str) -> Option<&'static str> {
    match class_name {
        "Boolean" => Some("__BooleanPrototype"),
        "Number" => Some("__NumberPrototype"),
        "String" => Some("__StringPrototype"),
        _ => None,
    }
}

fn builtin_error_superclass_name(class_name: &str) -> Option<&'static str> {
    match class_name {
        "AggregateError" | "EvalError" | "RangeError" | "ReferenceError" | "SyntaxError"
        | "TypeError" | "URIError" | "InternalError" | "SuppressedError" | "ChannelClosedError"
        | "AssertionError" => Some("Error"),
        _ => None,
    }
}

pub(in crate::vm::interpreter) fn checked_object_ptr(value: Value) -> Option<NonNull<Object>> {
    if !value.is_ptr() || value.is_null() {
        return None;
    }
    let raw_ptr = unsafe { value.as_ptr::<u8>() }?;
    let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };
    if header.type_id() != TypeId::of::<Object>() {
        return None;
    }
    unsafe { value.as_ptr::<Object>() }
}

pub(in crate::vm::interpreter) fn checked_array_ptr(value: Value) -> Option<NonNull<Array>> {
    if !value.is_ptr() || value.is_null() {
        return None;
    }
    let raw_ptr = unsafe { value.as_ptr::<u8>() }?;
    let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };
    if header.type_id() != TypeId::of::<Array>() {
        return None;
    }
    unsafe { value.as_ptr::<Array>() }
}

pub(in crate::vm::interpreter) fn checked_string_ptr(value: Value) -> Option<NonNull<RayaString>> {
    if !value.is_ptr() || value.is_null() {
        return None;
    }
    let raw_ptr = unsafe { value.as_ptr::<u8>() }?;
    let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };
    if header.type_id() != TypeId::of::<RayaString>() {
        return None;
    }
    unsafe { value.as_ptr::<RayaString>() }
}

pub(in crate::vm::interpreter) fn checked_closure_ptr(value: Value) -> Option<NonNull<Closure>> {
    if !value.is_ptr() || value.is_null() {
        return None;
    }
    let raw_ptr = unsafe { value.as_ptr::<u8>() }?;
    let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };
    if header.type_id() != TypeId::of::<Closure>() {
        return None;
    }
    unsafe { value.as_ptr::<Closure>() }
}

fn value_same_value(a: Value, b: Value) -> bool {
    if a.is_ptr() && b.is_ptr() {
        let a_str = unsafe { a.as_ptr::<RayaString>() };
        let b_str = unsafe { b.as_ptr::<RayaString>() };
        if let (Some(a_ptr), Some(b_ptr)) = (a_str, b_str) {
            let a_ref = unsafe { &*a_ptr.as_ptr() };
            let b_ref = unsafe { &*b_ptr.as_ptr() };
            return a_ref.data == b_ref.data;
        }
        return a.raw() == b.raw();
    }

    let a_num = a.as_f64().or_else(|| a.as_i32().map(|v| v as f64));
    let b_num = b.as_f64().or_else(|| b.as_i32().map(|v| v as f64));
    if let (Some(a_num), Some(b_num)) = (a_num, b_num) {
        if a_num.is_nan() && b_num.is_nan() {
            return true;
        }
        if a_num == 0.0 && b_num == 0.0 {
            let a_bits = a.as_f64().map(f64::to_bits).unwrap_or(0.0f64.to_bits());
            let b_bits = b.as_f64().map(f64::to_bits).unwrap_or(0.0f64.to_bits());
            return a_bits == b_bits;
        }
        return a_num == b_num;
    }

    a.raw() == b.raw()
}

#[inline]
fn native_arg(args: &[Value], index: usize) -> Value {
    args.get(index).copied().unwrap_or(Value::undefined())
}

fn is_uri_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
}

fn parse_js_array_index_name(key: &str) -> Option<usize> {
    if key.is_empty() {
        return None;
    }
    if key != "0" && key.starts_with('0') {
        return None;
    }
    let index = key.parse::<u32>().ok()?;
    if index == u32::MAX || index.to_string() != key {
        return None;
    }
    Some(index as usize)
}

fn percent_encode_uri_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if is_uri_unreserved(byte) {
            out.push(byte as char);
        } else {
            use std::fmt::Write;
            let _ = write!(&mut out, "%{:02X}", byte);
        }
    }
    out
}

fn percent_decode_uri_component(input: &str) -> Result<String, VmError> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(VmError::RuntimeError(
                    "Malformed percent-encoding".to_string(),
                ));
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                .map_err(|_| VmError::RuntimeError("Malformed percent-encoding".to_string()))?;
            let byte = u8::from_str_radix(hex, 16)
                .map_err(|_| VmError::RuntimeError("Malformed percent-encoding".to_string()))?;
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| VmError::RuntimeError("Invalid UTF-8".to_string()))
}

fn object_to_string_tag_from_class_name(class_name: &str) -> &'static str {
    match class_name {
        "Array" => "Array",
        "Function" | "AsyncFunction" | "GeneratorFunction" | "AsyncGeneratorFunction" => "Function",
        "String" => "String",
        "Number" => "Number",
        "Boolean" => "Boolean",
        "Symbol" => "Symbol",
        "Date" => "Date",
        "RegExp" => "RegExp",
        "Error" | "TypeError" | "RangeError" | "ReferenceError" | "SyntaxError" | "URIError"
        | "EvalError" | "InternalError" | "AggregateError" | "SuppressedError"
        | "ChannelClosedError" | "AssertionError" => "Error",
        "Map" => "Map",
        "Set" => "Set",
        "WeakMap" => "WeakMap",
        "WeakSet" => "WeakSet",
        "WeakRef" => "WeakRef",
        "FinalizationRegistry" => "FinalizationRegistry",
        "ArrayBuffer" | "SharedArrayBuffer" => "ArrayBuffer",
        "DataView" => "DataView",
        _ => "Object",
    }
}

impl<'a> Interpreter<'a> {
    fn alloc_string_value(&self, value: impl Into<String>) -> Value {
        let gc_ptr = self.gc.lock().allocate(RayaString::new(value.into()));
        unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).expect("string ptr")) }
    }

    pub(in crate::vm::interpreter) fn alloc_builtin_error_value(
        &self,
        class_name: &str,
        message: &str,
    ) -> Value {
        let member_names = vec!["message".to_string(), "name".to_string()];
        let layout_id = layout_id_from_ordered_names(&member_names);
        let object_ptr = self
            .gc
            .lock()
            .allocate(Object::new_dynamic(layout_id, member_names.len()));
        let object_value = unsafe {
            Value::from_ptr(std::ptr::NonNull::new(object_ptr.as_ptr()).expect("error object ptr"))
        };
        if let Some(constructor) = self.builtin_global_value(class_name) {
            self.set_constructed_object_prototype_from_constructor(object_value, constructor);
        }
        let _ = self.define_data_property_on_target(
            object_value,
            "name",
            self.alloc_string_value(class_name),
            true,
            false,
            true,
        );
        let _ = self.define_data_property_on_target(
            object_value,
            "message",
            self.alloc_string_value(message),
            true,
            false,
            true,
        );
        object_value
    }

    fn construct_ordinary_callable(
        &mut self,
        constructor: Value,
        new_target: Value,
        args: &[Value],
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        let member_names: Vec<String> = Vec::new();
        let layout_id = layout_id_from_ordered_names(&member_names);
        let object_ptr = self.gc.lock().allocate(Object::new_dynamic(layout_id, 0));
        let object_value =
            unsafe { Value::from_ptr(NonNull::new(object_ptr.as_ptr()).expect("object ptr")) };
        self.set_constructed_object_prototype_from_constructor(object_value, new_target);
        let returned = self.invoke_callable_sync_with_this(
            constructor,
            Some(object_value),
            args,
            task,
            module,
        )?;
        if self.is_js_object_value(returned) || self.callable_function_info(returned).is_some() {
            Ok(returned)
        } else {
            Ok(object_value)
        }
    }

    fn construct_builtin_object(&mut self, new_target: Value) -> Value {
        let member_names: Vec<String> = Vec::new();
        let layout_id = layout_id_from_ordered_names(&member_names);
        let object_ptr = self.gc.lock().allocate(Object::new_dynamic(layout_id, 0));
        let object_value =
            unsafe { Value::from_ptr(NonNull::new(object_ptr.as_ptr()).expect("object ptr")) };
        self.set_constructed_object_prototype_from_constructor(object_value, new_target);
        object_value
    }

    fn set_constructed_value_prototype_from_constructor(&self, value: Value, constructor: Value) {
        if let Some(prototype) = self.constructed_object_prototype_from_constructor(constructor) {
            self.set_explicit_object_prototype(value, prototype);
        }
    }

    fn construct_builtin_array(
        &mut self,
        new_target: Value,
        args: &[Value],
    ) -> Result<Value, VmError> {
        let array_ptr = self.gc.lock().allocate(Array::new(0, 0));
        let array_value =
            unsafe { Value::from_ptr(NonNull::new(array_ptr.as_ptr()).expect("array ptr")) };
        self.set_constructed_value_prototype_from_constructor(array_value, new_target);

        let array = unsafe { &mut *array_ptr.as_ptr() };
        if args.len() == 1 {
            if let Some(len) = self.js_array_constructor_length_from_value(args[0])? {
                array.resize_holey(len);
            } else {
                array.set(0, args[0]).map_err(VmError::RuntimeError)?;
            }
            return Ok(array_value);
        }

        for (index, value) in args.iter().copied().enumerate() {
            if index >= array.elements.len() {
                array.resize_holey(index + 1);
            }
            array.set(index, value).map_err(VmError::RuntimeError)?;
        }

        Ok(array_value)
    }

    pub(in crate::vm::interpreter) fn construct_value_with_new_target(
        &mut self,
        constructor: Value,
        new_target: Value,
        args: &[Value],
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        if !self.callable_is_constructible(constructor) {
            return Err(VmError::TypeError("Value is not a constructor".to_string()));
        }
        if !self.callable_is_constructible(new_target) {
            return Err(VmError::TypeError("Value is not a constructor".to_string()));
        }

        if let Some(value) = self.try_construct_boxed_primitive(constructor, args, task, module)? {
            return Ok(value);
        }

        if self
            .builtin_global_value("Array")
            .is_some_and(|builtin| builtin.raw() == constructor.raw())
        {
            return self.construct_builtin_array(new_target, args);
        }

        if self
            .builtin_global_value("Object")
            .is_some_and(|builtin| builtin.raw() == constructor.raw())
        {
            return Ok(self.construct_builtin_object(new_target));
        }

        if let Some(nominal_type_id) = self
            .constructor_nominal_type_id(constructor)
            .or_else(|| self.nominal_type_id_from_imported_class_value(module, constructor))
        {
            let (layout_id, field_count) =
                self.nominal_allocation(nominal_type_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("Class {} not found", nominal_type_id))
                })?;

            let obj = Object::new_nominal(layout_id, nominal_type_id as u32, field_count);
            let gc_ptr = self.gc.lock().allocate(obj);
            let obj_val =
                unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
            self.ephemeral_gc_roots.write().push(obj_val);

            let prototype =
                match self.try_proxy_like_get_property(new_target, "prototype", task, module)? {
                    Some(prototype) if self.is_js_object_value(prototype) => Some(prototype),
                    _ => self.constructor_prototype_value(constructor),
                };
            if let Some(prototype) = prototype {
                self.set_constructed_object_prototype_from_value(obj_val, prototype);
            }

            let (constructor_id, constructor_module) = {
                let classes = self.classes.read();
                let class = classes.get_class(nominal_type_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("Class {} not found", nominal_type_id))
                })?;
                (class.get_constructor(), class.module.clone())
            };

            if let Some(constructor_id) = constructor_id {
                let closure = if let Some(module) = constructor_module {
                    Closure::with_module(constructor_id, Vec::new(), module)
                } else {
                    Closure::new(constructor_id, Vec::new())
                };
                let closure_ptr = self.gc.lock().allocate(closure);
                let closure_val = unsafe {
                    Value::from_ptr(
                        std::ptr::NonNull::new(closure_ptr.as_ptr())
                            .expect("constructor closure ptr"),
                    )
                };
                self.ephemeral_gc_roots.write().push(closure_val);

                let mut invoke_args = Vec::with_capacity(args.len() + 1);
                invoke_args.push(obj_val);
                invoke_args.extend_from_slice(args);
                let invoke_result =
                    self.invoke_callable_sync(closure_val, &invoke_args, task, module);
                {
                    let mut ephemeral = self.ephemeral_gc_roots.write();
                    if let Some(index) = ephemeral
                        .iter()
                        .rposition(|candidate| *candidate == closure_val)
                    {
                        ephemeral.swap_remove(index);
                    }
                }
                invoke_result?;
            }

            {
                let mut ephemeral = self.ephemeral_gc_roots.write();
                if let Some(index) = ephemeral
                    .iter()
                    .rposition(|candidate| *candidate == obj_val)
                {
                    ephemeral.swap_remove(index);
                }
            }

            return Ok(obj_val);
        }

        if self.callable_function_info(constructor).is_some()
            && self
                .constructor_nominal_type_id(constructor)
                .or_else(|| self.nominal_type_id_from_imported_class_value(module, constructor))
                .is_none()
        {
            return self.construct_ordinary_callable(constructor, new_target, args, task, module);
        }

        Err(VmError::TypeError(
            "Value is not a supported constructor".to_string(),
        ))
    }

    fn object_to_string_tag(&self, value: Value) -> &'static str {
        if value.is_undefined() {
            return "Undefined";
        }
        if value.is_null() {
            return "Null";
        }
        if value.as_bool().is_some() {
            return "Boolean";
        }
        if value.as_i32().is_some() || value.as_f64().is_some() {
            return "Number";
        }
        if checked_string_ptr(value).is_some() {
            return "String";
        }
        if checked_array_ptr(value).is_some() {
            return "Array";
        }
        if self.callable_function_info(value).is_some() {
            return "Function";
        }
        if let Some(class_name) = self.nominal_class_name_for_value(value) {
            return object_to_string_tag_from_class_name(&class_name);
        }
        "Object"
    }

    fn seed_builtin_error_prototype_properties(
        &self,
        prototype_val: Value,
        class_name: &str,
    ) -> Option<()> {
        let name = match class_name {
            "Error" | "AggregateError" | "EvalError" | "RangeError" | "ReferenceError"
            | "SyntaxError" | "TypeError" | "URIError" => class_name,
            _ => return Some(()),
        };

        self.define_data_property_on_target(
            prototype_val,
            "name",
            self.alloc_string_value(name),
            true,
            false,
            true,
        )
        .ok()?;

        self.define_data_property_on_target(
            prototype_val,
            "message",
            self.alloc_string_value(String::new()),
            true,
            false,
            true,
        )
        .ok()?;

        Some(())
    }

    fn normalize_dynamic_value(&self, value: Value) -> Value {
        use crate::vm::json::view::{js_classify, JSView};

        match js_classify(value) {
            JSView::Arr(ptr) => {
                let (type_id, elements) = unsafe { ((*ptr).type_id, (*ptr).elements.clone()) };
                let mut array = Array::new(type_id, elements.len());
                for (index, element) in elements.into_iter().enumerate() {
                    let normalized = self.normalize_dynamic_value(element);
                    let _ = array.set(index, normalized);
                }
                let gc_ptr = self.gc.lock().allocate(array);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
            }
            _ => value,
        }
    }

    fn collect_dynamic_entries(&self, value: Value) -> Vec<(String, Value)> {
        use crate::vm::json::view::{js_classify, JSView};

        match js_classify(value) {
            JSView::Struct { ptr, .. } => {
                let obj = unsafe { &*ptr };
                let mut entries = Vec::new();
                let mut fixed_entries_added = false;

                if let Some(nominal_type_id) = obj.nominal_type_id_usize() {
                    let class_metadata = self.class_metadata.read();
                    if let Some(meta) = class_metadata.get(nominal_type_id) {
                        for (index, name) in meta.field_names.iter().enumerate() {
                            if name.is_empty() || index >= obj.field_count() {
                                continue;
                            }
                            if let Some(value) = obj.get_field(index) {
                                entries.push((name.clone(), self.normalize_dynamic_value(value)));
                            }
                        }
                        fixed_entries_added = true;
                    }
                }
                if !fixed_entries_added {
                    if let Some(layout_names) = self.layout_field_names_for_object(obj) {
                        for (index, name) in layout_names.into_iter().enumerate() {
                            if index >= obj.field_count() {
                                break;
                            }
                            if let Some(value) = obj.get_field(index) {
                                entries.push((name, self.normalize_dynamic_value(value)));
                            }
                        }
                    }
                }

                if let Some(dyn_map) = obj.dyn_map() {
                    for (key, value) in dyn_map {
                        let Some(name) = self.prop_key_name(*key) else {
                            continue;
                        };
                        if entries.iter().any(|(existing, _)| existing == &name) {
                            continue;
                        }
                        entries.push((name, self.normalize_dynamic_value(*value)));
                    }
                }

                entries
            }
            _ => Vec::new(),
        }
    }

    fn merge_dynamic_entries_into(&self, target: Value, entries: &[(String, Value)]) {
        use crate::vm::json::view::{js_classify, JSView};

        match js_classify(target) {
            JSView::Struct { ptr, .. } => {
                let obj = unsafe { &mut *(ptr as *mut Object) };
                for (key, value) in entries {
                    if let Some(index) = self.get_field_index_for_value(target, key) {
                        let _ = obj.set_field(index, *value);
                    } else {
                        obj.ensure_dyn_map()
                            .insert(self.intern_prop_key(key), *value);
                    }
                }
            }
            _ => {}
        }
    }

    fn legacy_object_literal_field_index(field_name: &str, field_count: usize) -> Option<usize> {
        let idx = match field_name {
            // Error-like object literal layout: [message, name, stack, cause, ...]
            "message" => 0,
            "name" => 1,
            "stack" => 2,
            "cause" => 3,
            "code" => 4,
            "errno" => 5,
            "syscall" => 6,
            "path" => 7,
            "errors" => 8,
            // Node-compat descriptor Object layout: [value, writable, configurable, enumerable, get, set]
            "value" => 0,
            "writable" => 1,
            "configurable" => 2,
            "enumerable" => 3,
            "get" => 4,
            "set" => 5,
            _ => return None,
        };
        (idx < field_count).then_some(idx)
    }

    pub(in crate::vm::interpreter) fn is_callable_value(value: Value) -> bool {
        if !value.is_ptr() {
            return false;
        }
        let header = unsafe { &*header_ptr_from_value_ptr(value.as_ptr::<u8>().unwrap().as_ptr()) };
        header.type_id() == std::any::TypeId::of::<Closure>()
            || header.type_id() == std::any::TypeId::of::<BoundMethod>()
            || header.type_id() == std::any::TypeId::of::<BoundNativeMethod>()
            || header.type_id() == std::any::TypeId::of::<BoundFunction>()
    }

    pub(in crate::vm::interpreter) fn proxy_wrapper_proxy_value(
        &self,
        value: Value,
    ) -> Option<Value> {
        let object_ptr = checked_object_ptr(value)?;
        let object = unsafe { &*object_ptr.as_ptr() };
        let proxy_value = self.get_object_named_field_value(object, "_proxy")?;
        crate::vm::reflect::try_unwrap_proxy(proxy_value)?;
        Some(proxy_value)
    }

    pub(in crate::vm::interpreter) fn unwrapped_proxy_like(
        &self,
        value: Value,
    ) -> Option<crate::vm::reflect::UnwrappedProxy> {
        crate::vm::reflect::try_unwrap_proxy(value).or_else(|| {
            let proxy_value = self.proxy_wrapper_proxy_value(value)?;
            crate::vm::reflect::try_unwrap_proxy(proxy_value)
        })
    }

    pub(in crate::vm::interpreter) fn explicit_object_prototype(
        &self,
        value: Value,
    ) -> Option<Value> {
        self.metadata
            .lock()
            .get_metadata(OBJECT_PROTOTYPE_OVERRIDE_METADATA_KEY, value)
    }

    pub(in crate::vm::interpreter) fn set_explicit_object_prototype(
        &self,
        value: Value,
        prototype: Value,
    ) {
        self.metadata.lock().define_metadata(
            OBJECT_PROTOTYPE_OVERRIDE_METADATA_KEY.to_string(),
            prototype,
            value,
        );
    }

    pub(in crate::vm::interpreter) fn is_js_object_value(&self, value: Value) -> bool {
        if checked_array_ptr(value).is_some() || self.callable_function_info(value).is_some() {
            return true;
        }
        checked_object_ptr(value).is_some()
            && self.nominal_class_name_for_value(value).as_deref() != Some("Symbol")
    }

    pub(in crate::vm::interpreter) fn is_array_value(&self, value: Value) -> Result<bool, VmError> {
        if let Some(proxy) = self.unwrapped_proxy_like(value) {
            if proxy.handler.is_null() {
                return Err(VmError::TypeError("Proxy has been revoked".to_string()));
            }
            return self.is_array_value(proxy.target);
        }

        Ok(checked_array_ptr(value).is_some())
    }

    fn js_value_supports_extensibility(&self, value: Value) -> bool {
        if checked_array_ptr(value).is_some() || checked_object_ptr(value).is_some() {
            return self.nominal_class_name_for_value(value).as_deref() != Some("Symbol");
        }
        self.callable_function_info(value).is_some()
    }

    fn is_js_value_extensible(&self, value: Value) -> bool {
        if !self.js_value_supports_extensibility(value) {
            return false;
        }
        self.metadata
            .lock()
            .get_metadata(OBJECT_EXTENSIBLE_METADATA_KEY, value)
            .and_then(|flag| flag.as_bool())
            .unwrap_or(true)
    }

    fn set_js_value_extensible(&self, value: Value, extensible: bool) {
        if !self.js_value_supports_extensibility(value) {
            return;
        }
        let mut metadata = self.metadata.lock();
        if extensible {
            metadata.delete_metadata(OBJECT_EXTENSIBLE_METADATA_KEY, value);
        } else {
            metadata.define_metadata(
                OBJECT_EXTENSIBLE_METADATA_KEY.to_string(),
                Value::bool(false),
                value,
            );
        }
    }

    fn has_own_js_property(&self, target: Value, key: &str) -> bool {
        self.get_descriptor_metadata(target, key).is_some()
            || self
                .get_own_js_property_value_by_name(target, key)
                .is_some()
            || self
                .callable_virtual_property_descriptor(target, key)
                .is_some()
    }

    fn raw_type_handle_id(value: Value) -> Option<crate::vm::object::TypeHandleId> {
        if !value.is_ptr() {
            return None;
        }
        let header = unsafe { &*header_ptr_from_value_ptr(value.as_ptr::<u8>().unwrap().as_ptr()) };
        if header.type_id() != std::any::TypeId::of::<TypeHandle>() {
            return None;
        }
        let handle_ptr = unsafe { value.as_ptr::<TypeHandle>() }?;
        Some(unsafe { (*handle_ptr.as_ptr()).handle_id })
    }

    fn type_handle_nominal_id(&self, value: Value) -> Option<crate::vm::object::NominalTypeId> {
        let handle_id = Self::raw_type_handle_id(value)?;
        self.type_handles
            .read()
            .get(handle_id)
            .map(|entry| entry.nominal_type_id)
    }

    fn constructor_value_for_nominal_type(&self, nominal_type_id: usize) -> Option<Value> {
        let class_name = {
            let classes = self.classes.read();
            classes.get_class(nominal_type_id)?.name.clone()
        };
        if let Some(global) = self.builtin_global_value(&class_name) {
            return Some(global);
        }

        if let Some(&slot) = self.class_value_slots.read().get(&nominal_type_id) {
            if let Some(value) = self.globals_by_index.read().get(slot).copied() {
                return Some(value);
            }
        }

        let (layout_id, _) = self.nominal_allocation(nominal_type_id)?;
        let mut class_value_slots = self.class_value_slots.write();
        if let Some(&slot) = class_value_slots.get(&nominal_type_id) {
            if let Some(value) = self.globals_by_index.read().get(slot).copied() {
                return Some(value);
            }
        }

        let handle_id = self
            .type_handles
            .write()
            .register(nominal_type_id as u32, layout_id, None);
        let gc_ptr = self.gc.lock().allocate(TypeHandle {
            handle_id,
            shape_id: None,
        });
        let value = unsafe {
            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).expect("type handle ptr"))
        };
        let mut globals = self.globals_by_index.write();
        let slot = globals.len();
        globals.push(value);
        class_value_slots.insert(nominal_type_id, slot);
        Some(value)
    }

    pub(in crate::vm::interpreter) fn constructor_nominal_type_id(
        &self,
        value: Value,
    ) -> Option<usize> {
        let value = self
            .unwrapped_proxy_like(value)
            .map(|proxy| proxy.target)
            .unwrap_or(value);

        let debug_ctor_resolve = std::env::var("RAYA_DEBUG_CTOR_RESOLVE").is_ok();

        if let Some(global_name) = self.builtin_global_name_for_value(value) {
            let classes = self.classes.read();
            if let Some(class) = classes.get_class_by_name(&global_name) {
                if debug_ctor_resolve {
                    eprintln!(
                        "[ctor-resolve] value={:#x} builtin_global='{}' -> nominal_type_id={} class='{}'",
                        value.raw(),
                        global_name,
                        class.id,
                        class.name
                    );
                }
                return Some(class.id);
            }
        }

        if let Some(nominal_id) = self.type_handle_nominal_id(value) {
            if debug_ctor_resolve {
                eprintln!(
                    "[ctor-resolve] value={:#x} type_handle_nominal_id={}",
                    value.raw(),
                    nominal_id
                );
            }
            return Some(nominal_id as usize);
        }

        let object_ptr = checked_object_ptr(value)?;
        let object = unsafe { &*object_ptr.as_ptr() };
        let handle_key = self.intern_prop_key(IMPORTED_CLASS_TYPE_HANDLE_KEY);
        let handle_val = object
            .dyn_map()
            .and_then(|dyn_map| dyn_map.get(&handle_key).copied())?;
        let resolved = self
            .type_handle_nominal_id(handle_val)
            .map(|id| id as usize);
        if debug_ctor_resolve {
            eprintln!(
                "[ctor-resolve] value={:#x} dyn_handle={:#x} -> {:?}",
                value.raw(),
                handle_val.raw(),
                resolved
            );
        }
        resolved
    }

    pub(in crate::vm::interpreter) fn materialize_constructor_static_method(
        &self,
        constructor: Value,
        key: &str,
    ) -> Option<Value> {
        let debug_static_method = std::env::var("RAYA_DEBUG_STATIC_METHOD").is_ok();
        if matches!(key, "prototype" | "name" | "length") {
            return None;
        }

        let origin_nominal_type_id = self.constructor_nominal_type_id(constructor)?;
        let mut current_nominal_type_id = Some(origin_nominal_type_id);

        while let Some(nominal_type_id) = current_nominal_type_id {
            let (class_name, class_module, parent_id) = {
                let classes = self.classes.read();
                let class = classes.get_class(nominal_type_id)?;
                (class.name.clone(), class.module.clone(), class.parent_id)
            };
            if debug_static_method {
                eprintln!(
                    "[static-method] ctor={:#x} key={} nominal_type_id={} class={} has_module={}",
                    constructor.raw(),
                    key,
                    nominal_type_id,
                    class_name,
                    class_module.is_some()
                );
            }

            let Some(module) = class_module else {
                current_nominal_type_id = parent_id;
                continue;
            };

            let static_method_name = format!("{}::static::{}", class_name, key);
            if debug_static_method {
                let sample = module
                    .functions
                    .iter()
                    .filter(|function| {
                        function
                            .name
                            .starts_with(&format!("{}::static::", class_name))
                    })
                    .take(8)
                    .map(|function| function.name.clone())
                    .collect::<Vec<_>>();
                eprintln!(
                    "[static-method] seek={} module={} sample={:?}",
                    static_method_name, module.metadata.name, sample
                );
            }
            let Some(func_id) = module
                .functions
                .iter()
                .position(|function| function.name == static_method_name)
            else {
                current_nominal_type_id = parent_id;
                continue;
            };

            let closure = Closure::with_module(func_id, Vec::new(), module.clone());
            let closure_ptr = self.gc.lock().allocate(closure);
            let closure_value = unsafe {
                Value::from_ptr(
                    std::ptr::NonNull::new(closure_ptr.as_ptr())
                        .expect("constructor static method ptr"),
                )
            };
            let property_target = if nominal_type_id == origin_nominal_type_id {
                constructor
            } else {
                self.constructor_value_for_nominal_type(nominal_type_id)?
            };
            let _ = self.define_data_property_on_target(
                property_target,
                key,
                closure_value,
                true,
                false,
                true,
            );
            return Some(closure_value);
        }

        None
    }

    fn has_constructor_static_method(&self, constructor: Value, key: &str) -> bool {
        if matches!(key, "prototype" | "name" | "length") {
            return false;
        }

        let origin_nominal_type_id = match self.constructor_nominal_type_id(constructor) {
            Some(id) => id,
            None => return false,
        };
        let mut current_nominal_type_id = Some(origin_nominal_type_id);

        while let Some(nominal_type_id) = current_nominal_type_id {
            let (class_name, class_module, parent_id) = {
                let classes = self.classes.read();
                let Some(class) = classes.get_class(nominal_type_id) else {
                    return false;
                };
                (class.name.clone(), class.module.clone(), class.parent_id)
            };

            let Some(module) = class_module else {
                current_nominal_type_id = parent_id;
                continue;
            };

            let static_method_name = format!("{}::static::{}", class_name, key);
            if module
                .functions
                .iter()
                .any(|function| function.name == static_method_name)
            {
                return true;
            }

            current_nominal_type_id = parent_id;
        }

        false
    }

    pub(in crate::vm::interpreter) fn callable_virtual_property_deleted(
        &self,
        target: Value,
        key: &str,
    ) -> bool {
        self.metadata
            .lock()
            .get_metadata_property(CALLABLE_VIRTUAL_DELETE_METADATA_KEY, target, key)
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }

    fn cached_callable_virtual_property_value(&self, target: Value, key: &str) -> Option<Value> {
        self.metadata
            .lock()
            .get_metadata_property(CALLABLE_VIRTUAL_VALUE_METADATA_KEY, target, key)
    }

    fn set_cached_callable_virtual_property_value(&self, target: Value, key: &str, value: Value) {
        self.metadata.lock().define_metadata_property(
            CALLABLE_VIRTUAL_VALUE_METADATA_KEY.to_string(),
            value,
            target,
            key.to_string(),
        );
    }

    pub(in crate::vm::interpreter) fn is_callable_virtual_property(
        &self,
        target: Value,
        key: &str,
    ) -> bool {
        self.callable_virtual_property_value(target, key).is_some()
            || self
                .callable_virtual_accessor_value(target, key, "get")
                .is_some()
            || self
                .callable_virtual_accessor_value(target, key, "set")
                .is_some()
    }

    pub(in crate::vm::interpreter) fn set_callable_virtual_property_deleted(
        &self,
        target: Value,
        key: &str,
        deleted: bool,
    ) {
        let mut metadata = self.metadata.lock();
        if deleted {
            metadata.define_metadata_property(
                CALLABLE_VIRTUAL_DELETE_METADATA_KEY.to_string(),
                Value::bool(true),
                target,
                key.to_string(),
            );
        } else {
            let _ = metadata.delete_metadata_property(
                CALLABLE_VIRTUAL_DELETE_METADATA_KEY,
                target,
                key,
            );
        }
    }

    pub(in crate::vm::interpreter) fn fixed_property_deleted(
        &self,
        target: Value,
        key: &str,
    ) -> bool {
        self.metadata
            .lock()
            .get_metadata_property(FIXED_PROPERTY_DELETE_METADATA_KEY, target, key)
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }

    fn set_fixed_property_deleted(&self, target: Value, key: &str, deleted: bool) {
        let mut metadata = self.metadata.lock();
        if deleted {
            metadata.define_metadata_property(
                FIXED_PROPERTY_DELETE_METADATA_KEY.to_string(),
                Value::bool(true),
                target,
                key.to_string(),
            );
        } else {
            let _ =
                metadata.delete_metadata_property(FIXED_PROPERTY_DELETE_METADATA_KEY, target, key);
        }
    }

    pub(in crate::vm::interpreter) fn is_runtime_global_object(&self, target: Value) -> bool {
        self.builtin_global_value("globalThis")
            .is_some_and(|global_obj| global_obj.raw() == target.raw())
    }

    pub(in crate::vm::interpreter) fn builtin_global_property_value(
        &self,
        target: Value,
        key: &str,
    ) -> Option<Value> {
        if !self.is_runtime_global_object(target) || self.fixed_property_deleted(target, key) {
            return None;
        }
        self.builtin_global_value(key)
    }

    pub(in crate::vm::interpreter) fn set_builtin_global_property(
        &self,
        target: Value,
        key: &str,
        value: Value,
    ) -> bool {
        if !self.is_runtime_global_object(target) {
            return false;
        }
        if !self.builtin_global_slots.read().contains_key(key) {
            return false;
        }
        let slot = match self.builtin_global_slots.read().get(key).copied() {
            Some(slot) => slot,
            None => return false,
        };
        let mut globals = self.globals_by_index.write();
        if slot >= globals.len() {
            globals.resize(slot + 1, Value::undefined());
        }
        globals[slot] = value;
        self.set_fixed_property_deleted(target, key, false);
        true
    }

    fn bind_script_global_property(
        &mut self,
        key: &str,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<(), VmError> {
        let Some(global_this) = self.builtin_global_value("globalThis") else {
            return Ok(());
        };

        let has_concrete_own_property = self.get_descriptor_metadata(global_this, key).is_some()
            || self
                .get_own_js_property_value_by_name(global_this, key)
                .is_some()
            || self.own_js_property_flags(global_this, key).is_some()
            || (self.is_runtime_global_object(global_this)
                && self.builtin_global_slots.read().contains_key(key));

        if has_concrete_own_property {
            return match self.set_property_value_via_js_semantics(
                global_this,
                key,
                value,
                global_this,
                caller_task,
                caller_module,
            )? {
                true => Ok(()),
                false => Err(VmError::TypeError(format!(
                    "Cannot assign to non-writable property '{}'",
                    key
                ))),
            };
        }

        if self.is_js_value_extensible(global_this) {
            self.define_data_property_on_target(global_this, key, value, true, true, false)?;
        }

        Ok(())
    }

    fn visible_function_name(raw_name: &str) -> String {
        let visible = raw_name.rsplit("::").next().unwrap_or(raw_name);
        match visible {
            "__speciesGetter" => "get [Symbol.species]".to_string(),
            _ => visible.to_string(),
        }
    }

    fn function_native_alias_id(raw_name: &str) -> Option<u16> {
        if raw_name.ends_with("Function::call") {
            Some(crate::compiler::native_id::FUNCTION_CALL_HELPER)
        } else if raw_name.ends_with("Function::apply") {
            Some(crate::compiler::native_id::FUNCTION_APPLY_HELPER)
        } else if raw_name.ends_with("Function::bind") {
            Some(crate::compiler::native_id::FUNCTION_BIND_HELPER)
        } else {
            None
        }
    }

    pub(in crate::vm::interpreter) fn native_callable_uses_receiver(&self, native_id: u16) -> bool {
        !matches!(
            native_id,
            crate::compiler::native_id::OBJECT_DEFINE_PROPERTY
                | crate::compiler::native_id::OBJECT_GET_OWN_PROPERTY_DESCRIPTOR
                | crate::compiler::native_id::OBJECT_DEFINE_PROPERTIES
                | crate::compiler::native_id::OBJECT_DELETE_PROPERTY
                | crate::compiler::native_id::OBJECT_GET_PROTOTYPE_OF
                | crate::compiler::native_id::OBJECT_GET_AMBIENT_GLOBAL
        )
    }

    pub(in crate::vm::interpreter) fn callable_function_info(
        &self,
        target: Value,
    ) -> Option<(String, usize)> {
        let target = self
            .unwrapped_proxy_like(target)
            .map(|proxy| proxy.target)
            .unwrap_or(target);

        let raw_ptr = unsafe { target.as_ptr::<u8>() }?;
        let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };

        if header.type_id() == std::any::TypeId::of::<Closure>() {
            let closure = unsafe { &*target.as_ptr::<Closure>()?.as_ptr() };
            let module = closure.module()?;
            if std::env::var("RAYA_DEBUG_DYNAMIC_FUNCTION").is_ok() {
                eprintln!(
                    "[callable-info] closure target={:#x} func_id={} module={}",
                    target.raw(),
                    closure.func_id,
                    module.metadata.name
                );
            }
            let function = module.functions.get(closure.func_id)?;
            if module.metadata.name.starts_with("__dynamic_function__/") {
                return Some(("anonymous".to_string(), function.visible_length));
            }
            return Some((
                Self::visible_function_name(&function.name),
                function.visible_length,
            ));
        }

        if header.type_id() == std::any::TypeId::of::<BoundMethod>() {
            let method = unsafe { &*target.as_ptr::<BoundMethod>()?.as_ptr() };
            let module = method.module.clone()?;
            let function = module.functions.get(method.func_id)?;
            return Some((
                Self::visible_function_name(&function.name),
                function.visible_length,
            ));
        }

        if header.type_id() == std::any::TypeId::of::<BoundNativeMethod>() {
            let method = unsafe { &*target.as_ptr::<BoundNativeMethod>()?.as_ptr() };
            let raw_name = crate::compiler::native_id::native_name(method.native_id);
            let visible_name = raw_name.rsplit('.').next().unwrap_or(raw_name).to_string();
            let arity = match method.native_id {
                crate::compiler::native_id::FUNCTION_CALL_HELPER => 1,
                crate::compiler::native_id::FUNCTION_APPLY_HELPER => 2,
                crate::compiler::native_id::FUNCTION_BIND_HELPER => 1,
                _ => 0,
            };
            return Some((visible_name, arity));
        }

        if header.type_id() == std::any::TypeId::of::<BoundFunction>() {
            let bound = unsafe { &*target.as_ptr::<BoundFunction>()?.as_ptr() };
            let (name, length) = self.callable_function_info(bound.target)?;
            return Some((
                format!("bound {}", name),
                length.saturating_sub(bound.bound_args.len()),
            ));
        }

        if let Some(nominal_type_id) = self.constructor_nominal_type_id(target) {
            let classes = self.classes.read();
            let class = classes.get_class(nominal_type_id)?;
            let visible_name = class.name.clone();
            let builtin_arity = crate::vm::builtins::get_all_signatures()
                .iter()
                .flat_map(|sig| sig.classes.iter())
                .find(|sig| sig.name == visible_name)
                .and_then(|sig| sig.constructor.map(|ctor| ctor.len()));
            let runtime_arity = class
                .get_constructor()
                .and_then(|constructor_id| {
                    class
                        .module
                        .as_ref()
                        .and_then(|module| module.functions.get(constructor_id))
                        .map(|function| function.visible_length)
                })
                .unwrap_or(0);
            let arity = builtin_arity.unwrap_or(runtime_arity);
            return Some((visible_name, arity));
        }

        None
    }

    pub(in crate::vm::interpreter) fn callable_is_constructible(&self, target: Value) -> bool {
        let target = self
            .unwrapped_proxy_like(target)
            .map(|proxy| proxy.target)
            .unwrap_or(target);

        let Some(raw_ptr) = (unsafe { target.as_ptr::<u8>() }) else {
            return false;
        };
        let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };

        if header.type_id() == std::any::TypeId::of::<Closure>() {
            let Some(closure_ptr) = (unsafe { target.as_ptr::<Closure>() }) else {
                return false;
            };
            let closure = unsafe { &*closure_ptr.as_ptr() };
            let Some(module) = closure.module() else {
                return false;
            };
            return module
                .functions
                .get(closure.func_id)
                .is_some_and(|function| function.is_constructible);
        }

        if header.type_id() == std::any::TypeId::of::<BoundFunction>() {
            let Some(bound_ptr) = (unsafe { target.as_ptr::<BoundFunction>() }) else {
                return false;
            };
            return self.callable_is_constructible(unsafe { &*bound_ptr.as_ptr() }.target);
        }

        self.constructor_nominal_type_id(target).is_some()
            || self.builtin_global_name_for_value(target).is_some()
    }

    fn callable_exposes_default_prototype(&self, target: Value) -> bool {
        let target = self
            .unwrapped_proxy_like(target)
            .map(|proxy| proxy.target)
            .unwrap_or(target);

        let Some(raw_ptr) = (unsafe { target.as_ptr::<u8>() }) else {
            return false;
        };
        let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };

        if header.type_id() == std::any::TypeId::of::<Closure>() {
            let Some(closure_ptr) = (unsafe { target.as_ptr::<Closure>() }) else {
                return false;
            };
            let closure = unsafe { &*closure_ptr.as_ptr() };
            let Some(module) = closure.module() else {
                return false;
            };
            return module
                .functions
                .get(closure.func_id)
                .is_some_and(|function| function.is_constructible || function.is_generator);
        }

        if header.type_id() == std::any::TypeId::of::<BoundFunction>() {
            let Some(bound_ptr) = (unsafe { target.as_ptr::<BoundFunction>() }) else {
                return false;
            };
            return self.callable_exposes_default_prototype(unsafe { &*bound_ptr.as_ptr() }.target);
        }

        self.constructor_nominal_type_id(target).is_some()
            || self.builtin_global_name_for_value(target).is_some()
    }

    pub(in crate::vm::interpreter) fn callable_is_strict_js(&self, target: Value) -> bool {
        let target = self
            .unwrapped_proxy_like(target)
            .map(|proxy| proxy.target)
            .unwrap_or(target);

        let Some(raw_ptr) = (unsafe { target.as_ptr::<u8>() }) else {
            return false;
        };
        let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };

        if header.type_id() == std::any::TypeId::of::<Closure>() {
            let Some(closure_ptr) = (unsafe { target.as_ptr::<Closure>() }) else {
                return false;
            };
            let closure = unsafe { &*closure_ptr.as_ptr() };
            let Some(module) = closure.module() else {
                return false;
            };
            return module
                .functions
                .get(closure.func_id)
                .is_some_and(|function| function.is_strict_js);
        }

        if header.type_id() == std::any::TypeId::of::<BoundFunction>() {
            let Some(bound_ptr) = (unsafe { target.as_ptr::<BoundFunction>() }) else {
                return false;
            };
            return self.callable_is_strict_js(unsafe { &*bound_ptr.as_ptr() }.target);
        }

        if header.type_id() == std::any::TypeId::of::<BoundMethod>() {
            return true;
        }

        false
    }

    pub(in crate::vm::interpreter) fn js_this_value_for_callable(
        &self,
        callable: Value,
        explicit_this: Option<Value>,
    ) -> Value {
        let this_value = explicit_this.unwrap_or(Value::undefined());
        if self.callable_is_strict_js(callable) {
            return this_value;
        }
        if this_value.is_null() || this_value.is_undefined() {
            return self
                .builtin_global_value("globalThis")
                .unwrap_or(Value::undefined());
        }
        this_value
    }

    fn callable_native_alias_id(&self, callable: Value) -> Option<u16> {
        let raw_ptr = unsafe { callable.as_ptr::<u8>() }?;
        let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };

        if header.type_id() == std::any::TypeId::of::<Closure>() {
            let closure = unsafe { &*callable.as_ptr::<Closure>()?.as_ptr() };
            let module = closure.module()?;
            let function = module.functions.get(closure.func_id())?;
            return Self::function_native_alias_id(&function.name);
        }

        if header.type_id() == std::any::TypeId::of::<BoundMethod>() {
            let method = unsafe { &*callable.as_ptr::<BoundMethod>()?.as_ptr() };
            let module = method.module.clone()?;
            let function = module.functions.get(method.func_id)?;
            return Self::function_native_alias_id(&function.name);
        }

        if header.type_id() == std::any::TypeId::of::<BoundNativeMethod>() {
            let method = unsafe { &*callable.as_ptr::<BoundNativeMethod>()?.as_ptr() };
            return Some(method.native_id);
        }

        if header.type_id() == std::any::TypeId::of::<BoundFunction>() {
            let bound = unsafe { &*callable.as_ptr::<BoundFunction>()?.as_ptr() };
            return self.callable_native_alias_id(bound.target);
        }

        None
    }

    pub(in crate::vm::interpreter) fn callable_uses_js_this_slot(&self, callable: Value) -> bool {
        let Some(raw_ptr) = (unsafe { callable.as_ptr::<u8>() }) else {
            return false;
        };
        let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };
        if header.type_id() == std::any::TypeId::of::<Closure>() {
            let closure = unsafe { &*callable.as_ptr::<Closure>().unwrap().as_ptr() };
            if let Some(module) = closure.module() {
                return module
                    .functions
                    .get(closure.func_id())
                    .map(|function| function.uses_js_this_slot)
                    .unwrap_or(false);
            }
            return false;
        }
        if header.type_id() == std::any::TypeId::of::<BoundMethod>() {
            return true;
        }
        if header.type_id() == std::any::TypeId::of::<BoundNativeMethod>() {
            let method = unsafe { &*callable.as_ptr::<BoundNativeMethod>().unwrap().as_ptr() };
            return self.native_callable_uses_receiver(method.native_id);
        }
        if header.type_id() == std::any::TypeId::of::<BoundFunction>() {
            let bound = unsafe { &*callable.as_ptr::<BoundFunction>().unwrap().as_ptr() };
            return self.callable_uses_js_this_slot(bound.target);
        }
        false
    }

    fn alloc_bound_function(
        &self,
        target: Value,
        this_arg: Value,
        bound_args: Vec<Value>,
    ) -> Result<Value, VmError> {
        let rebind_call_helper = self.callable_native_alias_id(target)
            == Some(crate::compiler::native_id::FUNCTION_CALL_HELPER);
        let bound = BoundFunction {
            target,
            this_arg,
            bound_args,
            rebind_call_helper,
        };
        let bound_ptr = self.gc.lock().allocate(bound);
        Ok(unsafe {
            Value::from_ptr(std::ptr::NonNull::new(bound_ptr.as_ptr()).expect("bound function ptr"))
        })
    }

    pub(in crate::vm::interpreter) fn dispatch_call_with_explicit_this(
        &mut self,
        stack: &mut Stack,
        target_callable: Value,
        this_arg: Value,
        rest_args: Vec<Value>,
        module: &Module,
        task: &Arc<Task>,
        non_callable_message: &'static str,
    ) -> OpcodeResult {
        if self.callable_native_alias_id(target_callable)
            == Some(crate::compiler::native_id::FUNCTION_CALL_HELPER)
        {
            let rebound_target = this_arg;
            let rebound_this = rest_args.first().copied().unwrap_or(Value::undefined());
            let rebound_rest = if rest_args.len() > 1 {
                rest_args[1..].to_vec()
            } else {
                Vec::new()
            };
            return self.dispatch_call_with_explicit_this(
                stack,
                rebound_target,
                rebound_this,
                rebound_rest,
                module,
                task,
                non_callable_message,
            );
        }

        if let Some(target_ptr) = unsafe { target_callable.as_ptr::<u8>() } {
            let header = unsafe { &*header_ptr_from_value_ptr(target_ptr.as_ptr()) };
            if header.type_id() == std::any::TypeId::of::<BoundNativeMethod>() {
                let method = unsafe {
                    &*target_callable
                        .as_ptr::<BoundNativeMethod>()
                        .expect("bound native target")
                        .as_ptr()
                };
                return self.exec_bound_native_method_call(
                    stack,
                    this_arg,
                    method.native_id,
                    rest_args,
                    module,
                    task,
                );
            } else if header.type_id() == std::any::TypeId::of::<BoundMethod>() {
                let method = unsafe {
                    &*target_callable
                        .as_ptr::<BoundMethod>()
                        .expect("bound method target")
                        .as_ptr()
                };
                if let Err(e) = stack.push(this_arg) {
                    return OpcodeResult::Error(e);
                }
                for arg in &rest_args {
                    if let Err(e) = stack.push(*arg) {
                        return OpcodeResult::Error(e);
                    }
                }
                return OpcodeResult::PushFrame {
                    func_id: method.func_id,
                    arg_count: rest_args.len() + 1,
                    is_closure: false,
                    closure_val: None,
                    module: method.module.clone(),
                    return_action: ReturnAction::PushReturnValue,
                };
            }
        }

        match self.callable_frame_for_value(
            target_callable,
            stack,
            &rest_args,
            Some(this_arg),
            ReturnAction::PushReturnValue,
        ) {
            Ok(Some(frame)) => frame,
            Ok(None) => OpcodeResult::Error(VmError::TypeError(non_callable_message.to_string())),
            Err(error) => OpcodeResult::Error(error),
        }
    }

    fn delete_property_from_target(
        &mut self,
        target: Value,
        key: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<bool, VmError> {
        let (key_str, _) =
            self.property_key_parts_with_context(key, "Object.deleteProperty", task, module)?;
        let Some(key_name) = key_str else {
            return Ok(true);
        };

        if !target.is_ptr() {
            return Ok(true);
        }

        let has_runtime_global_source = self.is_runtime_global_object(target)
            && self.builtin_global_slots.read().contains_key(&key_name);
        let has_callable_virtual_source = self.is_callable_virtual_property(target, &key_name);
        let has_constructor_static_source = self.has_constructor_static_method(target, &key_name);
        let has_fixed_field_source = self.get_field_index_for_value(target, &key_name).is_some();

        if let Some(existing) = self.get_descriptor_metadata(target, &key_name) {
            if !self.descriptor_flag(existing, "configurable", true) {
                return Ok(false);
            }
        } else if let Some((_, configurable, _)) =
            self.callable_virtual_property_descriptor(target, &key_name)
        {
            if !configurable {
                return Ok(false);
            }
        }

        let mut removed = false;
        if let Some(obj_ptr) = checked_object_ptr(target) {
            let obj = unsafe { &mut *obj_ptr.as_ptr() };
            if let Some(dyn_map) = obj.dyn_map_mut() {
                removed = dyn_map.remove(&self.intern_prop_key(&key_name)).is_some();
            }
        }

        let (metadata_removed, dynamic_value_removed) = {
            let mut metadata = self.metadata.lock();
            (
                metadata.delete_metadata_property(NODE_DESCRIPTOR_METADATA_KEY, target, &key_name),
                metadata.delete_metadata_property(
                    NON_OBJECT_DYNAMIC_VALUE_METADATA_KEY,
                    target,
                    &key_name,
                ),
            )
        };

        if removed || metadata_removed || dynamic_value_removed {
            self.set_callable_virtual_property_deleted(
                target,
                &key_name,
                has_callable_virtual_source,
            );
            self.set_fixed_property_deleted(
                target,
                &key_name,
                has_runtime_global_source
                    || has_constructor_static_source
                    || has_fixed_field_source,
            );
            return Ok(true);
        }

        if has_runtime_global_source {
            self.set_fixed_property_deleted(target, &key_name, true);
            return Ok(true);
        }

        if has_callable_virtual_source {
            self.set_callable_virtual_property_deleted(target, &key_name, true);
            return Ok(true);
        }

        if let Some(index) = self.get_field_index_for_value(target, &key_name) {
            if let Some(obj_ptr) = checked_object_ptr(target) {
                let obj = unsafe { &mut *obj_ptr.as_ptr() };
                let _ = obj.set_field(index, Value::undefined());
            }
            self.set_fixed_property_deleted(target, &key_name, true);
            return Ok(true);
        }

        Ok(false)
    }

    pub(in crate::vm::interpreter) fn builtin_global_value(&self, name: &str) -> Option<Value> {
        let slot = self.builtin_global_slots.read().get(name).copied()?;
        self.globals_by_index.read().get(slot).copied()
    }

    pub(in crate::vm::interpreter) fn builtin_global_name_for_value(
        &self,
        value: Value,
    ) -> Option<String> {
        let globals = self.globals_by_index.read();
        self.builtin_global_slots
            .read()
            .iter()
            .find_map(|(name, &slot)| {
                globals
                    .get(slot)
                    .copied()
                    .filter(|candidate| candidate.raw() == value.raw())
                    .map(|_| name.clone())
            })
    }

    fn prototype_object_for_nominal_type(
        &self,
        nominal_type_id: usize,
        constructor_value: Value,
    ) -> Option<Value> {
        if let Some(existing) =
            self.cached_callable_virtual_property_value(constructor_value, "prototype")
        {
            return Some(existing);
        }
        let (class_name, class_module, method_ids, mut method_names, parent_id) = {
            let classes = self.classes.read();
            let class = classes.get_class(nominal_type_id)?;
            let class_name = class.name.clone();
            let class_module = class.module.clone();
            let method_ids = class.vtable.methods.clone();
            let parent_id = class.parent_id;
            drop(classes);

            let method_names = self
                .class_metadata
                .read()
                .get(nominal_type_id)
                .map(|meta| meta.method_names.clone())
                .unwrap_or_default();

            (
                class_name,
                class_module,
                method_ids,
                method_names,
                parent_id,
            )
        };
        if let Some(module) = class_module.as_ref() {
            if method_names.len() < method_ids.len() {
                method_names.resize(method_ids.len(), String::new());
            }
            for (slot, func_id) in method_ids.iter().copied().enumerate() {
                if func_id == usize::MAX
                    || !method_names.get(slot).is_some_and(|name| name.is_empty())
                {
                    continue;
                }
                if let Some(function) = module.functions.get(func_id) {
                    if let Some(name) = function.name.rsplit("::").next() {
                        method_names[slot] = name.to_string();
                    }
                }
            }
        }

        let mut member_names = method_names
            .iter()
            .filter(|name| !name.is_empty())
            .cloned()
            .collect::<Vec<_>>();
        member_names.push("constructor".to_string());
        member_names.sort_unstable();
        member_names.dedup();

        let prototype_val = if class_name == "Array" {
            let prototype_ptr = self.gc.lock().allocate(Array::new(0, 0));
            unsafe {
                Value::from_ptr(
                    std::ptr::NonNull::new(prototype_ptr.as_ptr()).expect("prototype array ptr"),
                )
            }
        } else {
            let layout_id = layout_id_from_ordered_names(&member_names);
            let prototype_ptr = self
                .gc
                .lock()
                .allocate(Object::new_dynamic(layout_id, member_names.len()));
            unsafe {
                Value::from_ptr(
                    std::ptr::NonNull::new(prototype_ptr.as_ptr()).expect("prototype object ptr"),
                )
            }
        };
        self.set_cached_callable_virtual_property_value(
            constructor_value,
            "prototype",
            prototype_val,
        );

        self.define_data_property_on_target(
            prototype_val,
            "constructor",
            constructor_value,
            true,
            false,
            true,
        )
        .ok()?;

        if class_name == "Array" {
            self.define_data_property_on_target(
                prototype_val,
                "length",
                Value::i32(0),
                true,
                false,
                false,
            )
            .ok()?;
        }

        if let Some(parent_id) = parent_id {
            if let Some(parent_ctor) = self.constructor_value_for_nominal_type(parent_id) {
                if let Some(parent_proto) = self.constructor_prototype_value(parent_ctor) {
                    self.set_constructed_object_prototype_from_value(prototype_val, parent_proto);
                }
            }
        } else if let Some(object_ctor) = self.builtin_global_value("Object") {
            if let Some(object_proto) = self.object_constructor_prototype_value(object_ctor) {
                self.set_constructed_object_prototype_from_value(prototype_val, object_proto);
            }
        }

        self.seed_builtin_error_prototype_properties(prototype_val, &class_name)?;

        for (slot, method_name) in method_names.iter().enumerate() {
            if method_name.is_empty() {
                continue;
            }
            let Some(&func_id) = method_ids.get(slot) else {
                continue;
            };
            let closure = if let Some(module) = class_module.clone() {
                Closure::with_module(func_id, Vec::new(), module)
            } else {
                Closure::new(func_id, Vec::new())
            };
            let closure_ptr = self.gc.lock().allocate(closure);
            let closure_val = unsafe {
                Value::from_ptr(
                    std::ptr::NonNull::new(closure_ptr.as_ptr()).expect("prototype method ptr"),
                )
            };
            self.define_data_property_on_target(
                prototype_val,
                method_name,
                closure_val,
                true,
                false,
                true,
            )
            .ok()?;
        }

        Some(prototype_val)
    }

    pub(in crate::vm::interpreter) fn nominal_instance_prototype_value(
        &self,
        value: Value,
    ) -> Option<Value> {
        let debug_proto_resolve = std::env::var("RAYA_DEBUG_PROTO_RESOLVE").is_ok();
        let object_ptr = checked_object_ptr(value)?;
        let object = unsafe { &*object_ptr.as_ptr() };
        let nominal_type_id = object.nominal_type_id_usize()?;
        let class_name = {
            let classes = self.classes.read();
            classes.get_class(nominal_type_id)?.name.clone()
        };
        let constructor_value = self.constructor_value_for_nominal_type(nominal_type_id)?;
        if debug_proto_resolve {
            eprintln!(
                "[proto-resolve] instance={:#x} nominal_type_id={} class='{}' ctor={:#x}",
                value.raw(),
                nominal_type_id,
                class_name,
                constructor_value.raw()
            );
        }
        self.prototype_object_for_nominal_type(nominal_type_id, constructor_value)
    }

    fn ordinary_object_prototype_value(&self) -> Option<Value> {
        let constructor_value = self.builtin_global_value("Object")?;
        self.object_constructor_prototype_value(constructor_value)
    }

    pub(in crate::vm::interpreter) fn prototype_of_value(&self, value: Value) -> Option<Value> {
        let debug_proto_resolve = std::env::var("RAYA_DEBUG_PROTO_RESOLVE").is_ok();
        if let Some(prototype) = self.explicit_object_prototype(value) {
            if debug_proto_resolve {
                eprintln!(
                    "[proto-of] value={:#x} explicit={:#x}",
                    value.raw(),
                    prototype.raw()
                );
            }
            return Some(prototype);
        }

        if let Some(nominal_type_id) = self.constructor_nominal_type_id(value) {
            let parent_id = {
                let classes = self.classes.read();
                classes
                    .get_class(nominal_type_id)
                    .and_then(|class| class.parent_id)
            };
            if let Some(parent_id) = parent_id {
                let prototype = self.constructor_value_for_nominal_type(parent_id);
                if debug_proto_resolve {
                    eprintln!(
                        "[proto-of] value={:#x} class-parent={} -> {:?}",
                        value.raw(),
                        parent_id,
                        prototype.map(|v| format!("{:#x}", v.raw()))
                    );
                }
                return prototype;
            }
        }

        if self.callable_function_info(value).is_some() {
            if let Some(parent_name) = self
                .builtin_global_name_for_value(value)
                .as_deref()
                .and_then(builtin_error_superclass_name)
            {
                let prototype = self.builtin_global_value(parent_name);
                if debug_proto_resolve {
                    eprintln!(
                        "[proto-of] value={:#x} builtin-error-super='{}' -> {:?}",
                        value.raw(),
                        parent_name,
                        prototype.map(|v| format!("{:#x}", v.raw()))
                    );
                }
                return prototype;
            }
            let prototype = self
                .builtin_global_value("Function")
                .and_then(|ctor| self.function_constructor_prototype_value(ctor));
            if debug_proto_resolve {
                eprintln!(
                    "[proto-of] value={:#x} callable -> {:?}",
                    value.raw(),
                    prototype.map(|v| format!("{:#x}", v.raw()))
                );
            }
            return prototype;
        }

        if checked_object_ptr(value).is_some() {
            if let Some(prototype) = self.nominal_instance_prototype_value(value) {
                if debug_proto_resolve {
                    eprintln!(
                        "[proto-of] value={:#x} nominal -> {:#x}",
                        value.raw(),
                        prototype.raw()
                    );
                }
                return Some(prototype);
            }
            if debug_proto_resolve {
                eprintln!("[proto-of] value={:#x} ordinary-object", value.raw());
            }
            return self.ordinary_object_prototype_value();
        }

        if checked_array_ptr(value).is_some() {
            return self
                .builtin_global_value("Array")
                .and_then(|ctor| self.array_constructor_prototype_value(ctor));
        }

        if checked_string_ptr(value).is_some() {
            return self
                .builtin_global_value("String")
                .and_then(|ctor| self.string_constructor_prototype_value(ctor));
        }

        None
    }

    pub(in crate::vm::interpreter) fn constructor_prototype_value(
        &self,
        constructor: Value,
    ) -> Option<Value> {
        if let Some(existing) = self.metadata_data_property_value(constructor, "prototype") {
            if std::env::var("RAYA_DEBUG_PROTO_RESOLVE").is_ok() {
                eprintln!(
                    "[ctor-proto] ctor={:#x} metadata={:#x}",
                    constructor.raw(),
                    existing.raw()
                );
            }
            return Some(existing);
        }
        if let Some(existing) =
            self.cached_callable_virtual_property_value(constructor, "prototype")
        {
            if std::env::var("RAYA_DEBUG_PROTO_RESOLVE").is_ok() {
                eprintln!(
                    "[ctor-proto] ctor={:#x} cached={:#x}",
                    constructor.raw(),
                    existing.raw()
                );
            }
            return Some(existing);
        }
        if let Some(obj_ptr) = checked_object_ptr(constructor) {
            let obj = unsafe { &*obj_ptr.as_ptr() };
            if let Some(existing) = obj
                .dyn_map()
                .and_then(|dyn_map| dyn_map.get(&self.intern_prop_key("prototype")).copied())
            {
                return Some(existing);
            }
        }

        if let Some(nominal_type_id) = self.constructor_nominal_type_id(constructor) {
            let prototype = self.prototype_object_for_nominal_type(nominal_type_id, constructor);
            if std::env::var("RAYA_DEBUG_PROTO_RESOLVE").is_ok() {
                eprintln!(
                    "[ctor-proto] ctor={:#x} nominal_type_id={} -> {:?}",
                    constructor.raw(),
                    nominal_type_id,
                    prototype.map(|value| format!("{:#x}", value.raw()))
                );
            }
            return prototype;
        }

        let (visible_name, _) = self.callable_function_info(constructor)?;
        let prototype = self
            .prototype_object_for_class_name(&visible_name, constructor)
            .or_else(|| self.generic_function_prototype_value(constructor));
        if std::env::var("RAYA_DEBUG_PROTO_RESOLVE").is_ok() {
            eprintln!(
                "[ctor-proto] ctor={:#x} class='{}' -> {:?}",
                constructor.raw(),
                visible_name,
                prototype.map(|value| format!("{:#x}", value.raw()))
            );
        }
        prototype
    }

    fn constructed_object_prototype_from_constructor(&self, constructor: Value) -> Option<Value> {
        if let Some(prototype) = self.constructor_prototype_value(constructor) {
            if self.is_js_object_value(prototype) {
                return Some(prototype);
            }
        }

        self.builtin_global_value("Object")
            .and_then(|ctor| self.object_constructor_prototype_value(ctor))
    }

    pub(in crate::vm::interpreter) fn set_constructed_object_prototype_from_value(
        &self,
        object: Value,
        prototype: Value,
    ) {
        if !self.js_value_supports_extensibility(object) {
            return;
        }
        if !self.is_js_object_value(prototype) {
            return;
        }
        self.set_explicit_object_prototype(object, prototype);
    }

    pub(in crate::vm::interpreter) fn set_constructed_object_prototype_from_constructor(
        &self,
        object: Value,
        constructor: Value,
    ) {
        if let Some(prototype) = self.constructed_object_prototype_from_constructor(constructor) {
            if std::env::var("RAYA_DEBUG_PROTO_RESOLVE").is_ok() {
                eprintln!(
                    "[set-ctor-proto] object={:#x} ctor={:#x} proto={:#x}",
                    object.raw(),
                    constructor.raw(),
                    prototype.raw()
                );
            }
            self.set_constructed_object_prototype_from_value(object, prototype);
        }
    }

    pub(in crate::vm::interpreter) fn set_array_length_value(
        &self,
        target: Value,
        length_value: Value,
    ) -> Result<(), VmError> {
        let Some(array_ptr) = (unsafe { target.as_ptr::<Array>() }) else {
            return Err(VmError::TypeError(
                "Array length target must be an array".to_string(),
            ));
        };
        let new_len = self.js_array_length_from_property_value_without_context(length_value)?;
        let array = unsafe { &mut *array_ptr.as_ptr() };
        array.resize_holey(new_len);
        Ok(())
    }

    pub(in crate::vm::interpreter) fn set_array_length_value_with_context(
        &mut self,
        target: Value,
        length_value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<(), VmError> {
        let Some(array_ptr) = (unsafe { target.as_ptr::<Array>() }) else {
            return Err(VmError::TypeError(
                "Array length target must be an array".to_string(),
            ));
        };
        let new_len = self.js_array_length_from_property_value_with_context(
            length_value,
            caller_task,
            caller_module,
        )?;
        let array = unsafe { &mut *array_ptr.as_ptr() };
        array.resize_holey(new_len);
        Ok(())
    }

    fn set_property_value_on_receiver(
        &mut self,
        receiver: Value,
        key: &str,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<bool, VmError> {
        if let Some(array_ptr) = checked_array_ptr(receiver) {
            if key == "length" {
                return self.set_array_length_via_array_set_length(
                    receiver,
                    value,
                    caller_task,
                    caller_module,
                );
            }
            if let Ok(index) = key.parse::<usize>() {
                let array = unsafe { &mut *array_ptr.as_ptr() };
                if !self.is_js_value_extensible(receiver) && array.get(index).is_none() {
                    return Ok(false);
                }
                if index >= array.elements.len() {
                    array.resize_holey(index + 1);
                }
                array.set(index, value).map_err(VmError::RuntimeError)?;
                return Ok(true);
            }
        }

        if self.set_builtin_global_property(receiver, key, value) {
            self.sync_descriptor_value(receiver, key, value);
            return Ok(true);
        }

        if self.get_descriptor_metadata(receiver, key).is_some()
            && checked_object_ptr(receiver).is_none()
        {
            self.metadata.lock().define_metadata_property(
                NON_OBJECT_DYNAMIC_VALUE_METADATA_KEY.to_string(),
                value,
                receiver,
                key.to_string(),
            );
            self.sync_descriptor_value(receiver, key, value);
            self.set_callable_virtual_property_deleted(receiver, key, false);
            self.set_fixed_property_deleted(receiver, key, false);
            return Ok(true);
        }

        if self.callable_function_info(receiver).is_some()
            && self.get_descriptor_metadata(receiver, key).is_none()
        {
            if let Some((writable, _, _)) = self.callable_virtual_property_descriptor(receiver, key)
            {
                if !writable {
                    return Ok(false);
                }
                self.set_cached_callable_virtual_property_value(receiver, key, value);
                self.sync_descriptor_value(receiver, key, value);
                self.set_callable_virtual_property_deleted(receiver, key, false);
                self.set_fixed_property_deleted(receiver, key, false);
                return Ok(true);
            }
        }

        if let Some(obj_ptr) = checked_object_ptr(receiver) {
            let obj = unsafe { &mut *obj_ptr.as_ptr() };
            if let Some(index) = self.get_field_index_for_value(receiver, key) {
                obj.set_field(index, value).map_err(VmError::RuntimeError)?;
            } else {
                if !self.is_js_value_extensible(receiver) {
                    return Ok(false);
                }
                obj.ensure_dyn_map()
                    .insert(self.intern_prop_key(key), value);
            }
            self.sync_descriptor_value(receiver, key, value);
            self.set_callable_virtual_property_deleted(receiver, key, false);
            self.set_fixed_property_deleted(receiver, key, false);
            return Ok(true);
        }

        if receiver.is_ptr() || self.callable_function_info(receiver).is_some() {
            self.define_data_property_on_target(receiver, key, value, true, true, true)?;
            return Ok(true);
        }

        Ok(false)
    }

    pub(in crate::vm::interpreter) fn set_property_value_via_js_semantics(
        &mut self,
        target: Value,
        key: &str,
        value: Value,
        receiver: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<bool, VmError> {
        if target.raw() == receiver.raw() {
            if let Some(array_ptr) = checked_array_ptr(target) {
                if key == "length" {
                    return self.set_array_length_via_array_set_length(
                        target,
                        value,
                        caller_task,
                        caller_module,
                    );
                }
                if let Ok(index) = key.parse::<usize>() {
                    let array = unsafe { &mut *array_ptr.as_ptr() };
                    if index >= array.elements.len() {
                        array.resize_holey(index + 1);
                    }
                    array.set(index, value).map_err(VmError::RuntimeError)?;
                    return Ok(true);
                }
            }

            if self.set_builtin_global_property(target, key, value) {
                self.sync_descriptor_value(target, key, value);
                return Ok(true);
            }
        }

        if let Some(setter) = self.descriptor_accessor(target, key, "set") {
            let _ = self.invoke_callable_sync_with_this(
                setter,
                Some(receiver),
                &[value],
                caller_task,
                caller_module,
            )?;
            return Ok(true);
        }

        let has_getter_only = self.descriptor_accessor(target, key, "get").is_some()
            && !self.is_field_writable(target, key);
        if has_getter_only {
            return Ok(false);
        }

        let has_own_property = self.get_descriptor_metadata(target, key).is_some()
            || self.get_own_field_value_by_name(target, key).is_some()
            || self
                .callable_virtual_property_descriptor(target, key)
                .is_some();
        if has_own_property {
            if checked_array_ptr(target).is_some()
                && key == "length"
                && target.raw() == receiver.raw()
            {
                return self.set_array_length_via_array_set_length(
                    target,
                    value,
                    caller_task,
                    caller_module,
                );
            }
            if !self.is_field_writable(target, key) {
                return Ok(false);
            }
            return self.set_property_value_on_receiver(
                receiver,
                key,
                value,
                caller_task,
                caller_module,
            );
        }

        if let Some(prototype) = self.prototype_of_value(target) {
            if prototype != target {
                return self.set_property_value_via_js_semantics(
                    prototype,
                    key,
                    value,
                    receiver,
                    caller_task,
                    caller_module,
                );
            }
        }

        self.set_property_value_on_receiver(receiver, key, value, caller_task, caller_module)
    }

    pub(in crate::vm::interpreter) fn try_proxy_like_get_property(
        &mut self,
        value: Value,
        key: &str,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<Value>, VmError> {
        let Some(proxy) = self.unwrapped_proxy_like(value) else {
            return Ok(None);
        };

        if let Some(getter) = self.get_field_value_by_name(proxy.handler, "get") {
            let key_ptr = self.gc.lock().allocate(RayaString::new(key.to_string()));
            let key_value = unsafe {
                Value::from_ptr(std::ptr::NonNull::new(key_ptr.as_ptr()).expect("proxy key ptr"))
            };
            self.ephemeral_gc_roots.write().push(key_value);
            let trap_args = [proxy.target, key_value];
            let result = self.invoke_callable_sync_with_this(
                getter,
                Some(proxy.handler),
                &trap_args,
                caller_task,
                caller_module,
            );
            let mut ephemeral = self.ephemeral_gc_roots.write();
            if let Some(index) = ephemeral
                .iter()
                .rposition(|candidate| *candidate == key_value)
            {
                ephemeral.swap_remove(index);
            }
            let result = result?;
            return Ok(Some(result));
        }

        if let Some(getter) = self.descriptor_accessor(proxy.target, key, "get") {
            let result = self.invoke_callable_sync(getter, &[], caller_task, caller_module)?;
            return Ok(Some(result));
        }

        if let Some(value) = self.descriptor_data_value(proxy.target, key) {
            return Ok(Some(value));
        }

        if let Some(value) = self.get_field_value_by_name(proxy.target, key) {
            return Ok(Some(value));
        }

        if key == "prototype" {
            if let Some(value) = self.constructor_prototype_value(proxy.target) {
                return Ok(Some(value));
            }
        }

        if let Some(value) = self.callable_property_value(proxy.target, key) {
            return Ok(Some(value));
        }

        Ok(Some(Value::null()))
    }

    fn prototype_object_for_class_name(
        &self,
        class_name: &str,
        class_value: Value,
    ) -> Option<Value> {
        if let Some(existing) =
            self.cached_callable_virtual_property_value(class_value, "prototype")
        {
            self.ensure_intrinsic_prototype_parent(class_name, existing);
            return Some(existing);
        }
        if let Some(class_obj_ptr) = checked_object_ptr(class_value) {
            let class_obj = unsafe { &mut *class_obj_ptr.as_ptr() };
            if let Some(existing) = class_obj
                .dyn_map()
                .and_then(|dyn_map| dyn_map.get(&self.intern_prop_key("prototype")).copied())
            {
                self.ensure_intrinsic_prototype_parent(class_name, existing);
                return Some(existing);
            }
        }

        let lookup_class_name = {
            let classes = self.classes.read();
            if classes.get_class_by_name(class_name).is_some() {
                class_name.to_string()
            } else {
                boxed_primitive_helper_class_name(class_name)?.to_string()
            }
        };

        let (class_module, method_ids, mut method_names) = {
            let classes = self.classes.read();
            let class = classes.get_class_by_name(&lookup_class_name)?;
            let class_module = class.module.clone();
            let method_ids = class.vtable.methods.clone();
            drop(classes);
            let nominal_type_id = self
                .classes
                .read()
                .get_class_by_name(&lookup_class_name)
                .map(|class| class.id)?;
            let method_names = self
                .class_metadata
                .read()
                .get(nominal_type_id)
                .map(|meta| meta.method_names.clone())
                .unwrap_or_default();
            (class_module, method_ids, method_names)
        };
        if let Some(module) = class_module.as_ref() {
            if method_names.len() < method_ids.len() {
                method_names.resize(method_ids.len(), String::new());
            }
            for (slot, func_id) in method_ids.iter().copied().enumerate() {
                if func_id == usize::MAX
                    || !method_names.get(slot).is_some_and(|name| name.is_empty())
                {
                    continue;
                }
                if let Some(function) = module.functions.get(func_id) {
                    if let Some(name) = function.name.rsplit("::").next() {
                        method_names[slot] = name.to_string();
                    }
                }
            }
        }

        let mut member_names = method_names
            .iter()
            .filter(|name| !name.is_empty())
            .cloned()
            .collect::<Vec<_>>();
        member_names.push("constructor".to_string());
        member_names.sort_unstable();
        member_names.dedup();

        let prototype_val = if class_name == "Array" {
            let prototype_ptr = self.gc.lock().allocate(Array::new(0, 0));
            unsafe {
                Value::from_ptr(
                    std::ptr::NonNull::new(prototype_ptr.as_ptr()).expect("prototype array ptr"),
                )
            }
        } else {
            let layout_id = layout_id_from_ordered_names(&member_names);
            let prototype_ptr = self
                .gc
                .lock()
                .allocate(Object::new_dynamic(layout_id, member_names.len()));
            unsafe {
                Value::from_ptr(
                    std::ptr::NonNull::new(prototype_ptr.as_ptr()).expect("prototype object ptr"),
                )
            }
        };
        self.set_cached_callable_virtual_property_value(class_value, "prototype", prototype_val);

        self.define_data_property_on_target(
            prototype_val,
            "constructor",
            class_value,
            true,
            false,
            true,
        )
        .ok()?;

        if class_name == "Array" {
            self.define_data_property_on_target(
                prototype_val,
                "length",
                Value::i32(0),
                true,
                false,
                false,
            )
            .ok()?;
        }

        self.ensure_intrinsic_prototype_parent(class_name, prototype_val);

        self.seed_builtin_error_prototype_properties(prototype_val, class_name)?;

        for (slot, method_name) in method_names.iter().enumerate() {
            if method_name.is_empty() {
                continue;
            }
            let Some(&func_id) = method_ids.get(slot) else {
                continue;
            };
            let closure = if let Some(module) = class_module.clone() {
                Closure::with_module(func_id, Vec::new(), module)
            } else {
                Closure::new(func_id, Vec::new())
            };
            let closure_ptr = self.gc.lock().allocate(closure);
            let closure_val = unsafe {
                Value::from_ptr(
                    std::ptr::NonNull::new(closure_ptr.as_ptr()).expect("prototype method ptr"),
                )
            };
            self.define_data_property_on_target(
                prototype_val,
                method_name,
                closure_val,
                true,
                false,
                true,
            )
            .ok()?;
        }

        if let Some(class_obj_ptr) = checked_object_ptr(class_value) {
            let class_obj = unsafe { &mut *class_obj_ptr.as_ptr() };
            class_obj
                .ensure_dyn_map()
                .insert(self.intern_prop_key("prototype"), prototype_val);
        }
        Some(prototype_val)
    }

    fn ensure_intrinsic_prototype_parent(&self, class_name: &str, prototype_val: Value) {
        if self.explicit_object_prototype(prototype_val).is_some() {
            return;
        }

        if class_name == "Object" {
            self.set_explicit_object_prototype(prototype_val, Value::null());
            return;
        }

        if let Some(parent_name) = builtin_error_superclass_name(class_name) {
            if let Some(parent_ctor) = self.builtin_global_value(parent_name) {
                if let Some(parent_proto) = self.constructor_prototype_value(parent_ctor) {
                    self.set_constructed_object_prototype_from_value(prototype_val, parent_proto);
                }
            }
            return;
        }

        if let Some(object_ctor) = self.builtin_global_value("Object") {
            if let Some(object_proto) = self.object_constructor_prototype_value(object_ctor) {
                self.set_constructed_object_prototype_from_value(prototype_val, object_proto);
            }
        }
    }

    fn generic_function_prototype_value(&self, class_value: Value) -> Option<Value> {
        let debug_dynamic_function = std::env::var("RAYA_DEBUG_DYNAMIC_FUNCTION").is_ok();
        if let Some(existing) =
            self.cached_callable_virtual_property_value(class_value, "prototype")
        {
            if debug_dynamic_function {
                eprintln!(
                    "[generic-fn-proto] target={:#x} cached={:#x}",
                    class_value.raw(),
                    existing.raw()
                );
            }
            return Some(existing);
        }
        if !self.callable_exposes_default_prototype(class_value) {
            if debug_dynamic_function {
                eprintln!(
                    "[generic-fn-proto] target={:#x} no-default-prototype",
                    class_value.raw()
                );
            }
            return None;
        }
        if debug_dynamic_function {
            eprintln!(
                "[generic-fn-proto] target={:#x} alloc:start",
                class_value.raw()
            );
        }

        let layout_id = layout_id_from_ordered_names(&["constructor".to_string()]);
        let prototype_ptr = self.gc.lock().allocate(Object::new_dynamic(layout_id, 1));
        let prototype_val = unsafe {
            Value::from_ptr(
                std::ptr::NonNull::new(prototype_ptr.as_ptr()).expect("prototype object ptr"),
            )
        };
        if debug_dynamic_function {
            eprintln!(
                "[generic-fn-proto] target={:#x} alloc:prototype={:#x}",
                class_value.raw(),
                prototype_val.raw()
            );
        }
        self.set_cached_callable_virtual_property_value(class_value, "prototype", prototype_val);
        if debug_dynamic_function {
            eprintln!(
                "[generic-fn-proto] target={:#x} cache:set",
                class_value.raw()
            );
        }

        self.define_data_property_on_target(
            prototype_val,
            "constructor",
            class_value,
            true,
            false,
            true,
        )
        .ok()?;
        if debug_dynamic_function {
            eprintln!(
                "[generic-fn-proto] target={:#x} constructor:set",
                class_value.raw()
            );
        }

        if let Some(object_ctor) = self.builtin_global_value("Object") {
            if let Some(object_proto) = self.object_constructor_prototype_value(object_ctor) {
                self.set_constructed_object_prototype_from_value(prototype_val, object_proto);
                if debug_dynamic_function {
                    eprintln!(
                        "[generic-fn-proto] target={:#x} object-proto:set",
                        class_value.raw()
                    );
                }
            }
        }

        if let Some(class_obj_ptr) = checked_object_ptr(class_value) {
            let class_obj = unsafe { &mut *class_obj_ptr.as_ptr() };
            class_obj
                .ensure_dyn_map()
                .insert(self.intern_prop_key("prototype"), prototype_val);
        }
        if debug_dynamic_function {
            eprintln!("[generic-fn-proto] target={:#x} done", class_value.raw());
        }
        Some(prototype_val)
    }

    pub(in crate::vm::interpreter) fn boxed_primitive_internal_value(
        &self,
        value: Value,
        kind: &str,
    ) -> Option<Value> {
        let kind_value = self.get_own_field_value_by_name(value, "__boxedPrimitiveKind")?;
        let actual_kind = primitive_to_js_string(kind_value)?;
        if actual_kind != kind {
            return None;
        }
        self.get_own_field_value_by_name(value, "__primitiveValue")
    }

    fn alloc_boxed_primitive_object(
        &mut self,
        constructor: Value,
        kind: &str,
        primitive_value: Value,
    ) -> Result<Value, VmError> {
        let member_names = vec![
            "__boxedPrimitiveKind".to_string(),
            "__primitiveValue".to_string(),
        ];
        let layout_id = layout_id_from_ordered_names(&member_names);
        let object_ptr = self
            .gc
            .lock()
            .allocate(Object::new_dynamic(layout_id, member_names.len()));
        let object_value = unsafe {
            Value::from_ptr(
                std::ptr::NonNull::new(object_ptr.as_ptr()).expect("boxed primitive object ptr"),
            )
        };
        self.set_constructed_object_prototype_from_constructor(object_value, constructor);
        let kind_ptr = self.gc.lock().allocate(RayaString::new(kind.to_string()));
        let kind_value = unsafe {
            Value::from_ptr(
                std::ptr::NonNull::new(kind_ptr.as_ptr()).expect("boxed primitive kind ptr"),
            )
        };
        self.define_data_property_on_target(
            object_value,
            "__boxedPrimitiveKind",
            kind_value,
            true,
            false,
            false,
        )?;
        self.define_data_property_on_target(
            object_value,
            "__primitiveValue",
            primitive_value,
            true,
            false,
            false,
        )?;
        Ok(object_value)
    }

    pub(in crate::vm::interpreter) fn try_construct_boxed_primitive(
        &mut self,
        constructor: Value,
        args: &[Value],
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Option<Value>, VmError> {
        let Some(global_name) = self.builtin_global_name_for_value(constructor) else {
            return Ok(None);
        };
        if !matches!(global_name.as_str(), "Boolean" | "Number" | "String") {
            return Ok(None);
        }
        let primitive_value = self.invoke_callable_sync(constructor, args, task, module)?;
        self.alloc_boxed_primitive_object(constructor, &global_name, primitive_value)
            .map(Some)
    }

    fn js_array_length_from_number(&self, numeric: f64) -> Result<usize, VmError> {
        if !numeric.is_finite()
            || numeric < 0.0
            || numeric > u32::MAX as f64
            || numeric.fract() != 0.0
        {
            return Err(VmError::RangeError("Invalid array length".to_string()));
        }

        Ok(numeric as usize)
    }

    fn js_array_constructor_length_from_value(
        &self,
        value: Value,
    ) -> Result<Option<usize>, VmError> {
        let Some(numeric) = value.as_i32().map(|v| v as f64).or_else(|| value.as_f64()) else {
            return Ok(None);
        };
        self.js_array_length_from_number(numeric).map(Some)
    }

    fn is_js_primitive_value(&self, value: Value) -> bool {
        value.is_undefined()
            || value.is_null()
            || value.as_bool().is_some()
            || value.as_i32().is_some()
            || value.as_f64().is_some()
            || checked_string_ptr(value).is_some()
    }

    fn js_to_primitive_number_hint(
        &mut self,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Value, VmError> {
        if self.is_js_primitive_value(value) {
            return Ok(value);
        }
        for kind in ["Boolean", "Number", "String"] {
            if let Some(primitive) = self.boxed_primitive_internal_value(value, kind) {
                return Ok(primitive);
            }
        }
        for method_name in ["valueOf", "toString"] {
            let Some(method) = self.get_field_value_by_name(value, method_name) else {
                continue;
            };
            if !Self::is_callable_value(method) {
                continue;
            }
            let primitive = self.invoke_callable_sync_with_this(
                method,
                Some(value),
                &[],
                caller_task,
                caller_module,
            )?;
            if self.is_js_primitive_value(primitive) {
                return Ok(primitive);
            }
        }
        Err(VmError::TypeError(
            "Cannot convert object to primitive value".to_string(),
        ))
    }

    fn js_to_number_from_primitive(&self, value: Value) -> Result<f64, VmError> {
        if value.is_undefined() {
            return Ok(f64::NAN);
        }
        if value.is_null() {
            return Ok(0.0);
        }
        if let Some(value) = value.as_bool() {
            return Ok(if value { 1.0 } else { 0.0 });
        }
        if let Some(value) = value.as_i32() {
            return Ok(value as f64);
        }
        if let Some(value) = value.as_f64() {
            return Ok(value);
        }
        if let Some(ptr) = checked_string_ptr(value) {
            let text = unsafe { &*ptr.as_ptr() }.data.trim().to_string();
            if text.is_empty() {
                return Ok(0.0);
            }
            if text == "Infinity" || text == "+Infinity" {
                return Ok(f64::INFINITY);
            }
            if text == "-Infinity" {
                return Ok(f64::NEG_INFINITY);
            }
            if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
                return Ok(u64::from_str_radix(hex, 16)
                    .map(|value| value as f64)
                    .unwrap_or(f64::NAN));
            }
            if let Some(bin) = text.strip_prefix("0b").or_else(|| text.strip_prefix("0B")) {
                return Ok(u64::from_str_radix(bin, 2)
                    .map(|value| value as f64)
                    .unwrap_or(f64::NAN));
            }
            if let Some(oct) = text.strip_prefix("0o").or_else(|| text.strip_prefix("0O")) {
                return Ok(u64::from_str_radix(oct, 8)
                    .map(|value| value as f64)
                    .unwrap_or(f64::NAN));
            }
            return Ok(text.parse::<f64>().unwrap_or(f64::NAN));
        }
        Err(VmError::TypeError(
            "Cannot convert value to number".to_string(),
        ))
    }

    fn js_to_number_with_context(
        &mut self,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<f64, VmError> {
        let primitive = self.js_to_primitive_number_hint(value, caller_task, caller_module)?;
        self.js_to_number_from_primitive(primitive)
    }

    fn js_to_uint32(number: f64) -> u32 {
        if !number.is_finite() || number == 0.0 {
            return 0;
        }
        let integer = number.signum() * number.abs().floor();
        integer.rem_euclid(4_294_967_296.0) as u32
    }

    fn js_array_length_from_property_value_without_context(
        &self,
        value: Value,
    ) -> Result<usize, VmError> {
        let primitive = if self.is_js_primitive_value(value) {
            value
        } else {
            let mut boxed_primitive = None;
            for kind in ["Boolean", "Number", "String"] {
                if let Some(primitive) = self.boxed_primitive_internal_value(value, kind) {
                    boxed_primitive = Some(primitive);
                    break;
                }
            }
            match boxed_primitive {
                Some(primitive) => primitive,
                None => {
                    return Err(VmError::TypeError(
                        "Cannot convert object to primitive value".to_string(),
                    ))
                }
            }
        };
        let numeric = self.js_to_number_from_primitive(primitive)?;
        self.js_array_length_from_number(numeric)
    }

    fn js_array_length_from_property_value_with_context(
        &mut self,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<usize, VmError> {
        let primitive = self.js_to_primitive_number_hint(value, caller_task, caller_module)?;
        let numeric = self.js_to_number_from_primitive(primitive)?;
        self.js_array_length_from_number(numeric)
    }

    fn js_array_set_length_from_property_value_with_context(
        &mut self,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<usize, VmError> {
        if std::env::var("RAYA_DEBUG_REFLECT_SET").is_ok() {
            eprintln!("[array-set-length] coercing value={:#x}", value.raw());
        }
        let new_len = Self::js_to_uint32(self.js_to_number_with_context(
            value,
            caller_task,
            caller_module,
        )?);
        let number_len = self.js_to_number_with_context(value, caller_task, caller_module)?;
        if std::env::var("RAYA_DEBUG_REFLECT_SET").is_ok() {
            eprintln!(
                "[array-set-length] numeric-coercions uint32={} number={}",
                new_len, number_len
            );
        }
        if new_len as f64 != number_len {
            return Err(VmError::RangeError("Invalid array length".to_string()));
        }
        Ok(new_len as usize)
    }

    fn array_length_value(len: usize) -> Value {
        if len <= i32::MAX as usize {
            Value::i32(len as i32)
        } else {
            Value::f64(len as f64)
        }
    }

    fn store_array_length_descriptor(
        &self,
        target: Value,
        len: usize,
        writable: bool,
    ) -> Result<(), VmError> {
        let descriptor = self.alloc_object_descriptor()?;
        let Some(descriptor_ptr) = (unsafe { descriptor.as_ptr::<Object>() }) else {
            return Err(VmError::RuntimeError(
                "Failed to allocate array length descriptor".to_string(),
            ));
        };
        let descriptor_obj = unsafe { &mut *descriptor_ptr.as_ptr() };
        for (field_name, field_value) in [
            ("value", Self::array_length_value(len)),
            ("writable", Value::bool(writable)),
            ("enumerable", Value::bool(false)),
            ("configurable", Value::bool(false)),
        ] {
            if let Some(field_index) = self.get_field_index_for_value(descriptor, field_name) {
                descriptor_obj
                    .set_field(field_index, field_value)
                    .map_err(VmError::RuntimeError)?;
            }
            self.set_descriptor_field_present(descriptor, field_name, true);
        }
        self.set_descriptor_metadata(target, "length", descriptor);
        self.set_callable_virtual_property_deleted(target, "length", false);
        self.set_fixed_property_deleted(target, "length", false);
        Ok(())
    }

    fn set_array_length_via_array_set_length(
        &mut self,
        target: Value,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<bool, VmError> {
        if std::env::var("RAYA_DEBUG_REFLECT_SET").is_ok() {
            eprintln!(
                "[array-set-length] start target={:#x} value={:#x} writable={}",
                target.raw(),
                value.raw(),
                self.is_field_writable(target, "length")
            );
        }
        let new_len = self.js_array_set_length_from_property_value_with_context(
            value,
            caller_task,
            caller_module,
        )?;
        if std::env::var("RAYA_DEBUG_REFLECT_SET").is_ok() {
            eprintln!("[array-set-length] coerced new_len={}", new_len);
        }
        if !self.is_field_writable(target, "length") {
            if std::env::var("RAYA_DEBUG_REFLECT_SET").is_ok() {
                eprintln!("[array-set-length] target became non-writable");
            }
            return Ok(false);
        }
        let Some(array_ptr) = checked_array_ptr(target) else {
            return Err(VmError::TypeError(
                "Array length target must be an array".to_string(),
            ));
        };
        let array = unsafe { &mut *array_ptr.as_ptr() };
        array.resize_holey(new_len);
        self.store_array_length_descriptor(target, new_len, true)?;
        Ok(true)
    }

    fn apply_array_length_descriptor_with_context(
        &mut self,
        target: Value,
        descriptor: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<(), VmError> {
        let Some(array_ptr) = checked_array_ptr(target) else {
            return Err(VmError::TypeError(
                "Array length target must be an array".to_string(),
            ));
        };

        let requested_len = if self.descriptor_field_present(descriptor, "value") {
            let value = self
                .get_field_value_by_name(descriptor, "value")
                .unwrap_or(Value::undefined());
            Some(self.js_array_set_length_from_property_value_with_context(
                value,
                caller_task,
                caller_module,
            )?)
        } else {
            None
        };

        if self.descriptor_field_present(descriptor, "get")
            || self.descriptor_field_present(descriptor, "set")
        {
            return Err(VmError::TypeError(
                "Cannot redefine non-configurable property 'length'".to_string(),
            ));
        }
        if self.descriptor_field_present(descriptor, "configurable")
            && self.descriptor_flag(descriptor, "configurable", false)
        {
            return Err(VmError::TypeError(
                "Cannot redefine non-configurable property 'length'".to_string(),
            ));
        }
        if self.descriptor_field_present(descriptor, "enumerable")
            && self.descriptor_flag(descriptor, "enumerable", false)
        {
            return Err(VmError::TypeError(
                "Cannot redefine non-configurable property 'length'".to_string(),
            ));
        }

        let old_len = unsafe { &*array_ptr.as_ptr() }.len();
        let current_writable = self.is_field_writable(target, "length");
        let requested_writable = self
            .descriptor_field_present(descriptor, "writable")
            .then(|| self.descriptor_flag(descriptor, "writable", false));
        if !current_writable && requested_writable == Some(true) {
            return Err(VmError::TypeError(
                "Cannot redefine non-configurable property 'length'".to_string(),
            ));
        }

        let mut final_len = old_len;
        if let Some(new_len) = requested_len {
            if new_len != old_len && !current_writable {
                return Err(VmError::TypeError(
                    "Cannot assign to non-writable property 'length'".to_string(),
                ));
            }
            if new_len != old_len {
                let array = unsafe { &mut *array_ptr.as_ptr() };
                array.resize_holey(new_len);
            }
            final_len = new_len;
        }

        self.store_array_length_descriptor(
            target,
            final_len,
            requested_writable.unwrap_or(current_writable),
        )
    }

    pub(in crate::vm::interpreter) fn object_constructor_prototype_value(
        &self,
        class_value: Value,
    ) -> Option<Value> {
        self.prototype_object_for_class_name("Object", class_value)
    }

    pub(in crate::vm::interpreter) fn array_constructor_prototype_value(
        &self,
        class_value: Value,
    ) -> Option<Value> {
        self.prototype_object_for_class_name("Array", class_value)
    }

    pub(in crate::vm::interpreter) fn string_constructor_prototype_value(
        &self,
        class_value: Value,
    ) -> Option<Value> {
        self.prototype_object_for_class_name("String", class_value)
    }

    pub(in crate::vm::interpreter) fn function_constructor_prototype_value(
        &self,
        class_value: Value,
    ) -> Option<Value> {
        self.prototype_object_for_class_name("Function", class_value)
    }

    fn species_accessor_getter_for_constructor(&self, class_value: Value) -> Option<Value> {
        let builtin = self.builtin_global_value("Array")?;
        if builtin.raw() != class_value.raw() {
            return None;
        }
        self.get_own_field_value_by_name(class_value, "__speciesGetter")
    }

    pub(in crate::vm::interpreter) fn callable_virtual_accessor_value(
        &self,
        target: Value,
        key: &str,
        accessor_name: &str,
    ) -> Option<Value> {
        if self.callable_virtual_property_deleted(target, key) {
            return None;
        }
        match (key, accessor_name) {
            ("Symbol.species", "get") => self.species_accessor_getter_for_constructor(target),
            _ => None,
        }
    }

    pub(in crate::vm::interpreter) fn callable_virtual_property_value(
        &self,
        target: Value,
        key: &str,
    ) -> Option<Value> {
        if self.callable_virtual_property_deleted(target, key) {
            return None;
        }
        if let Some(value) = self.metadata_data_property_value(target, key) {
            return Some(value);
        }
        if let Some(value) = self.cached_callable_virtual_property_value(target, key) {
            return Some(value);
        }
        match key {
            "prototype" => self.constructor_prototype_value(target),
            "name" | "length" => self.callable_property_value(target, key),
            _ => None,
        }
    }

    pub(in crate::vm::interpreter) fn callable_virtual_property_descriptor(
        &self,
        target: Value,
        key: &str,
    ) -> Option<(bool, bool, bool)> {
        if self.callable_virtual_property_deleted(target, key) {
            return None;
        }
        match key {
            "prototype" if self.constructor_prototype_value(target).is_some() => {
                Some((true, false, false))
            }
            "name" | "length" if self.callable_property_value(target, key).is_some() => {
                Some((false, true, false))
            }
            "Symbol.species"
                if self
                    .species_accessor_getter_for_constructor(target)
                    .is_some() =>
            {
                Some((false, true, false))
            }
            _ => None,
        }
    }

    pub(in crate::vm::interpreter) fn callable_property_value(
        &self,
        target: Value,
        key: &str,
    ) -> Option<Value> {
        if self.callable_virtual_property_deleted(target, key) {
            return None;
        }
        let (name, length) = self.callable_function_info(target)?;
        if std::env::var("RAYA_DEBUG_DYNAMIC_FUNCTION").is_ok() {
            eprintln!(
                "[callable-prop] target={:#x} key={} name={} length={}",
                target.raw(),
                key,
                name,
                length
            );
        }
        match key {
            "name" => {
                let ptr = self.gc.lock().allocate(RayaString::new(name));
                if std::env::var("RAYA_DEBUG_DYNAMIC_FUNCTION").is_ok() {
                    eprintln!("[callable-prop] name:allocated");
                }
                Some(unsafe { Value::from_ptr(std::ptr::NonNull::new(ptr.as_ptr()).unwrap()) })
            }
            "length" => Some(Value::i32(length as i32)),
            _ => None,
        }
    }

    fn nominal_class_name_for_value(&self, value: Value) -> Option<String> {
        let obj_ptr = checked_object_ptr(value)?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let nominal_type_id = obj.nominal_type_id_usize()?;
        let classes = self.classes.read();
        classes
            .get_class(nominal_type_id)
            .map(|class| class.name.clone())
    }

    fn js_function_argument_to_string(
        &mut self,
        value: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<String, VmError> {
        if let Some(text) = primitive_to_js_string(value) {
            return Ok(text);
        }

        if self
            .nominal_class_name_for_value(value)
            .as_deref()
            .is_some_and(|name| name == "Symbol")
        {
            return Err(VmError::TypeError(
                "Cannot convert a Symbol value to a string".to_string(),
            ));
        }

        if let Ok(Some(exotic)) =
            self.well_known_symbol_property_value(value, "Symbol.toPrimitive", task, module)
        {
            if !Self::is_callable_value(exotic) {
                return Err(VmError::TypeError(
                    "Cannot convert object to primitive value".to_string(),
                ));
            }
            let hint_ptr = self
                .gc
                .lock()
                .allocate(RayaString::new("string".to_string()));
            let hint_value = unsafe {
                Value::from_ptr(std::ptr::NonNull::new(hint_ptr.as_ptr()).expect("string hint ptr"))
            };
            self.ephemeral_gc_roots.write().push(hint_value);
            let result = self.invoke_callable_sync_with_this(
                exotic,
                Some(value),
                &[hint_value],
                task,
                module,
            );
            let mut ephemeral = self.ephemeral_gc_roots.write();
            if let Some(index) = ephemeral
                .iter()
                .rposition(|candidate| *candidate == hint_value)
            {
                ephemeral.swap_remove(index);
            }
            let primitive = result?;
            if let Some(text) = primitive_to_js_string(primitive) {
                return Ok(text);
            }
            return Err(VmError::TypeError(
                "Cannot convert object to primitive value".to_string(),
            ));
        }

        for method_name in ["toString", "valueOf"] {
            if let Some(method) = self.get_field_value_by_name(value, method_name) {
                if !Self::is_callable_value(method) {
                    continue;
                }
                let primitive =
                    self.invoke_callable_sync_with_this(method, Some(value), &[], task, module)?;
                if let Some(text) = primitive_to_js_string(primitive) {
                    return Ok(text);
                }
            }
        }

        Err(VmError::TypeError(
            "Cannot convert object to primitive value".to_string(),
        ))
    }

    fn compile_dynamic_js_function_module(
        &self,
        params_source: &str,
        body_source: &str,
    ) -> Result<Arc<Module>, VmError> {
        let debug_dynamic_function = std::env::var("RAYA_DEBUG_DYNAMIC_FUNCTION").is_ok();
        if debug_dynamic_function {
            eprintln!(
                "[dynamic-fn] compile:start params={:?} body={:?}",
                params_source, body_source
            );
        }
        let source = format!("function __dynamic_fn__({params_source}) {{\n{body_source}\n}}\n");
        let parser = Parser::new(&source).map_err(|error| {
            VmError::RuntimeError(format!("Dynamic Function lexer error: {:?}", error))
        })?;
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] compile:parsed-lexer");
        }
        let (ast, interner) = parser.parse().map_err(|error| {
            VmError::RuntimeError(format!("Dynamic Function parse error: {:?}", error))
        })?;
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] compile:parsed-ast");
        }

        let mut type_ctx = TypeContext::new();
        let policy = CheckerPolicy::for_mode(TypeSystemMode::Js);
        let mut binder = Binder::new(&mut type_ctx, &interner)
            .with_mode(TypeSystemMode::Js)
            .with_policy(policy);
        let builtin_sigs = crate::builtins::to_checker_signatures();
        binder.register_builtins(&builtin_sigs);
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] compile:builtin-sigs");
        }

        let mut symbols = binder.bind_module(&ast).map_err(|error| {
            VmError::RuntimeError(format!("Dynamic Function bind error: {:?}", error))
        })?;
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] compile:bound");
        }

        let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner)
            .with_mode(TypeSystemMode::Js)
            .with_policy(policy);
        let check_result = checker.check_module(&ast).map_err(|error| {
            VmError::RuntimeError(format!("Dynamic Function type error: {:?}", error))
        })?;
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] compile:checked");
        }

        for ((scope_id, name), ty) in check_result.inferred_types {
            symbols.update_type(ScopeId(scope_id), &name, ty);
        }

        let ambient_builtin_globals: FxHashSet<String> =
            self.builtin_global_slots.read().keys().cloned().collect();
        let module_identity = format!(
            "__dynamic_function__/{}",
            DYNAMIC_JS_FUNCTION_COUNTER.fetch_add(1, Ordering::Relaxed)
        );

        let compiler = Compiler::new(type_ctx, &interner)
            .with_expr_types(check_result.expr_types)
            .with_type_annotation_types(check_result.type_annotation_types)
            .with_module_identity(module_identity)
            .with_js_this_binding_compat(true)
            .with_allow_unresolved_runtime_fallback(true)
            .with_ambient_builtin_globals(ambient_builtin_globals)
            .with_source_text(source);
        let module = compiler.compile_via_ir(&ast).map_err(|error| {
            VmError::RuntimeError(format!("Dynamic Function compile error: {}", error))
        })?;
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] compile:done");
        }
        Ok(Arc::new(module))
    }

    fn alloc_dynamic_js_function(
        &mut self,
        args: &[Value],
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        let debug_dynamic_function = std::env::var("RAYA_DEBUG_DYNAMIC_FUNCTION").is_ok();
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] alloc:start argc={}", args.len());
        }
        let mut parts = Vec::with_capacity(args.len());
        for arg in args {
            if debug_dynamic_function {
                eprintln!(
                    "[dynamic-fn] alloc:arg-to-string:start value={:#x}",
                    arg.raw()
                );
            }
            parts.push(self.js_function_argument_to_string(*arg, task, module)?);
            if debug_dynamic_function {
                eprintln!("[dynamic-fn] alloc:arg-to-string:done");
            }
        }
        let body_source = parts.pop().unwrap_or_default();
        let params_source = parts.join(",");
        if debug_dynamic_function {
            eprintln!(
                "[dynamic-fn] alloc:sources params={:?} body={:?}",
                params_source, body_source
            );
        }
        let function_module =
            self.compile_dynamic_js_function_module(&params_source, &body_source)?;
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] alloc:compiled-module");
        }
        let func_id = function_module
            .functions
            .iter()
            .position(|function| function.name == "__dynamic_fn__")
            .ok_or_else(|| {
                VmError::RuntimeError(
                    "Dynamic Function compile did not produce __dynamic_fn__".to_string(),
                )
            })?;
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] alloc:func-id={}", func_id);
        }
        let closure = Closure::with_module(func_id, Vec::new(), function_module);
        let closure_ptr = self.gc.lock().allocate(closure);
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] alloc:done");
        }
        Ok(unsafe {
            Value::from_ptr(
                std::ptr::NonNull::new(closure_ptr.as_ptr()).expect("dynamic function ptr"),
            )
        })
    }

    pub(in crate::vm::interpreter) fn collect_apply_arguments(
        &self,
        arg_list: Value,
    ) -> Result<Vec<Value>, VmError> {
        use crate::vm::json::view::{js_classify, JSView};

        if arg_list.is_null() {
            return Ok(Vec::new());
        }

        match js_classify(arg_list) {
            JSView::Arr(ptr) => Ok(unsafe { &*ptr }.elements.clone()),
            JSView::Struct { .. } => {
                let length_value = self
                    .get_field_value_by_name(arg_list, "length")
                    .unwrap_or(Value::null());
                let length = crate::vm::interpreter::core::value_to_f64(length_value)?
                    .max(0.0)
                    .floor() as usize;
                let mut values = Vec::with_capacity(length);
                for index in 0..length {
                    values.push(
                        self.get_field_value_by_name(arg_list, &index.to_string())
                            .unwrap_or(Value::null()),
                    );
                }
                Ok(values)
            }
            _ => Err(VmError::TypeError(
                "Function.prototype.apply expects an array-like argument list".to_string(),
            )),
        }
    }

    fn nominal_type_id_from_imported_class_value(
        &self,
        module: &Module,
        value: Value,
    ) -> Option<usize> {
        if let Some(global_name) = self.builtin_global_name_for_value(value) {
            let classes = self.classes.read();
            if let Some(class) = classes.get_class_by_name(&global_name) {
                return Some(class.id);
            }
        }

        if let Some(nominal_id) = self.type_handle_nominal_id(value) {
            return Some(nominal_id as usize);
        }

        if let Some(local_nominal_type_id) = value.as_i32() {
            return self
                .resolve_nominal_type_id(module, local_nominal_type_id as usize)
                .ok();
        }
        if let Some(local_nominal_type_id) = value.as_u32() {
            return self
                .resolve_nominal_type_id(module, local_nominal_type_id as usize)
                .ok();
        }
        if let Some(local_nominal_type_id) = value.as_u64() {
            return self
                .resolve_nominal_type_id(module, local_nominal_type_id as usize)
                .ok();
        }

        if let Some(object_ptr) = checked_object_ptr(value) {
            let object = unsafe { &*object_ptr.as_ptr() };
            let handle_key = self.intern_prop_key(IMPORTED_CLASS_TYPE_HANDLE_KEY);
            if let Some(handle_val) = object
                .dyn_map()
                .and_then(|dyn_map| dyn_map.get(&handle_key).copied())
            {
                return self
                    .type_handle_nominal_id(handle_val)
                    .map(|id| id as usize);
            }
        }
        None
    }

    pub(in crate::vm::interpreter) fn get_field_index_for_value(
        &self,
        obj_val: Value,
        field_name: &str,
    ) -> Option<usize> {
        let obj_ptr = checked_object_ptr(obj_val)?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let nominal_type_id = obj.nominal_type_id_usize();
        let class_metadata = self.class_metadata.read();
        let metadata_index = nominal_type_id
            .and_then(|nominal_type_id| class_metadata.get(nominal_type_id))
            .and_then(|meta| meta.get_field_index(field_name));
        if metadata_index.is_some() {
            return metadata_index;
        }
        if let Some(index) = self
            .layout_field_names_for_object(obj)
            .and_then(|names| names.iter().position(|name| name == field_name))
        {
            if index < obj.field_count() {
                return Some(index);
            }
        }
        if nominal_type_id.is_some() {
            return None;
        }
        None
    }

    fn get_own_field_value_by_name(&self, obj_val: Value, field_name: &str) -> Option<Value> {
        if self.fixed_property_deleted(obj_val, field_name) {
            return None;
        }
        if let Some(value) = self.metadata_data_property_value(obj_val, field_name) {
            return Some(value);
        }
        if let Some(value) = self.callable_virtual_property_value(obj_val, field_name) {
            return Some(value);
        }
        let obj_ptr = checked_object_ptr(obj_val)?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let debug_field_lookup = std::env::var("RAYA_DEBUG_FIELD_LOOKUP").is_ok();
        if debug_field_lookup {
            eprintln!(
                "[field.lookup] target={:#x} key={} layout={} nominal={:?} dyn_map={} field_count={}",
                obj_val.raw(),
                field_name,
                obj.layout_id(),
                obj.nominal_type_id(),
                obj.dyn_map().is_some(),
                obj.field_count()
            );
        }
        if let Some(index) = self.get_field_index_for_value(obj_val, field_name) {
            if let Some(value) = obj.get_field(index) {
                if !value.is_null()
                    || self
                        .callable_virtual_property_value(obj_val, field_name)
                        .is_none()
                {
                    return Some(value);
                }
            }
        }
        let key = self.intern_prop_key(field_name);
        if debug_field_lookup {
            eprintln!("[field.lookup] target={:#x} dyn-key={}", obj_val.raw(), key);
        }
        obj.dyn_map().and_then(|map| map.get(&key).copied())
    }

    fn get_own_js_property_value_by_name(&self, target: Value, key: &str) -> Option<Value> {
        if let Some(array_ptr) = checked_array_ptr(target) {
            let array = unsafe { &*array_ptr.as_ptr() };
            if key == "length" {
                let len = array.len();
                return Some(if len <= i32::MAX as usize {
                    Value::i32(len as i32)
                } else {
                    Value::f64(len as f64)
                });
            }
            if let Some(index) = parse_js_array_index_name(key) {
                return array.get(index);
            }
        }

        self.get_own_field_value_by_name(target, key)
    }

    fn own_js_property_flags(&self, target: Value, key: &str) -> Option<(bool, bool, bool)> {
        if checked_array_ptr(target).is_some() {
            if key == "length" {
                return Some((true, false, false));
            }
            if parse_js_array_index_name(key).is_some() {
                return Some((true, true, true));
            }
        }
        None
    }

    fn get_field_value_on_target_by_name(&self, obj_val: Value, field_name: &str) -> Option<Value> {
        if let Some(value) = self.get_own_js_property_value_by_name(obj_val, field_name) {
            return Some(value);
        }

        if let Some(value) = self.materialize_constructor_static_method(obj_val, field_name) {
            return Some(value);
        }

        if let Some(obj_ptr) = checked_object_ptr(obj_val) {
            let obj = unsafe { &*obj_ptr.as_ptr() };
            if let Some(nominal_type_id) = obj.nominal_type_id_usize() {
                if let Some(method_slot) =
                    self.nominal_method_slot_by_name(nominal_type_id, field_name)
                {
                    if let Ok(bound) = self.bound_method_value_for_slot(obj_val, method_slot) {
                        return Some(bound);
                    }
                }
            }
        }

        None
    }

    pub(in crate::vm::interpreter) fn get_field_value_by_name(
        &self,
        obj_val: Value,
        field_name: &str,
    ) -> Option<Value> {
        let mut current = Some(obj_val);
        let mut seen = vec![obj_val.raw()];

        while let Some(target) = current {
            if let Some(value) = self.get_field_value_on_target_by_name(target, field_name) {
                return Some(value);
            }

            let Some(prototype) = self.prototype_of_value(target) else {
                break;
            };
            if prototype.raw() == target.raw() || seen.contains(&prototype.raw()) {
                break;
            }
            seen.push(prototype.raw());
            current = Some(prototype);
        }

        None
    }

    fn has_own_property_via_js_semantics(&self, target: Value, key: &str) -> bool {
        self.get_descriptor_metadata(target, key).is_some()
            || self.builtin_global_property_value(target, key).is_some()
            || self
                .get_own_js_property_value_by_name(target, key)
                .is_some()
            || self
                .callable_virtual_property_descriptor(target, key)
                .is_some()
            || self
                .materialize_constructor_static_method(target, key)
                .is_some()
    }

    fn has_property_via_js_semantics(&self, target: Value, key: &str) -> bool {
        let mut current = Some(target);
        let mut seen = vec![target.raw()];

        while let Some(candidate) = current {
            if self.has_own_property_via_js_semantics(candidate, key) {
                return true;
            }

            let Some(prototype) = self.prototype_of_value(candidate) else {
                break;
            };
            if prototype.raw() == candidate.raw() || seen.contains(&prototype.raw()) {
                break;
            }
            seen.push(prototype.raw());
            current = Some(prototype);
        }

        false
    }

    fn get_own_property_value_via_js_semantics_with_context(
        &mut self,
        target: Value,
        key: &str,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<Value>, VmError> {
        if let Some(getter) = self.descriptor_accessor(target, key, "get") {
            let value = self.invoke_callable_sync_with_this(
                getter,
                Some(target),
                &[],
                caller_task,
                caller_module,
            )?;
            return Ok(Some(value));
        }

        if let Some(value) = self.descriptor_data_value(target, key) {
            return Ok(Some(value));
        }

        if let Some(value) = self.builtin_global_property_value(target, key) {
            return Ok(Some(value));
        }

        if let Some(value) = self.get_own_js_property_value_by_name(target, key) {
            return Ok(Some(value));
        }

        if key == "prototype" {
            if let Some(value) = self.constructor_prototype_value(target) {
                return Ok(Some(value));
            }
        }

        if let Some(value) = self.callable_virtual_property_value(target, key) {
            return Ok(Some(value));
        }

        if let Some(value) = self.materialize_constructor_static_method(target, key) {
            return Ok(Some(value));
        }

        Ok(None)
    }

    pub(in crate::vm::interpreter) fn get_property_value_via_js_semantics_with_context(
        &mut self,
        target: Value,
        key: &str,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<Value>, VmError> {
        let mut current = Some(target);
        let mut seen = vec![target.raw()];

        while let Some(candidate) = current {
            if let Some(value) = self.get_own_property_value_via_js_semantics_with_context(
                candidate,
                key,
                caller_task,
                caller_module,
            )? {
                return Ok(Some(value));
            }

            let Some(prototype) = self.prototype_of_value(candidate) else {
                break;
            };
            if prototype.raw() == candidate.raw() || seen.contains(&prototype.raw()) {
                break;
            }
            seen.push(prototype.raw());
            current = Some(prototype);
        }

        Ok(None)
    }

    fn descriptor_flag(&self, descriptor: Value, field_name: &str, default: bool) -> bool {
        if !self.descriptor_field_present(descriptor, field_name) {
            return default;
        }
        let Some(value) = self.get_field_value_by_name(descriptor, field_name) else {
            return default;
        };
        if let Some(b) = value.as_bool() {
            b
        } else if let Some(i) = value.as_i32() {
            i != 0
        } else {
            default
        }
    }

    fn set_internal_descriptor_field(
        &self,
        descriptor: Value,
        field_name: &str,
        value: Value,
    ) -> Result<(), VmError> {
        let Some(descriptor_ptr) = (unsafe { descriptor.as_ptr::<Object>() }) else {
            return Err(VmError::RuntimeError(
                "Failed to access property descriptor object".to_string(),
            ));
        };
        let descriptor_obj = unsafe { &mut *descriptor_ptr.as_ptr() };
        if let Some(field_index) = self.get_field_index_for_value(descriptor, field_name) {
            descriptor_obj
                .set_field(field_index, value)
                .map_err(VmError::RuntimeError)?;
        }
        self.set_descriptor_field_present(descriptor, field_name, true);
        Ok(())
    }

    fn normalize_property_descriptor_with_context(
        &mut self,
        descriptor: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Value, VmError> {
        if self.is_descriptor_object(descriptor) {
            return Ok(descriptor);
        }
        if !descriptor.is_ptr() {
            return Err(VmError::TypeError(
                "Object property descriptor must be an object".to_string(),
            ));
        }

        let mut record = JsPropertyDescriptorRecord::default();

        for field_name in [
            "enumerable",
            "configurable",
            "value",
            "writable",
            "get",
            "set",
        ] {
            if !self.has_property_via_js_semantics(descriptor, field_name) {
                continue;
            }

            let value = self
                .get_property_value_via_js_semantics_with_context(
                    descriptor,
                    field_name,
                    caller_task,
                    caller_module,
                )?
                .unwrap_or(Value::undefined());

            match field_name {
                "enumerable" => {
                    record.has_enumerable = true;
                    record.enumerable = value.is_truthy();
                }
                "configurable" => {
                    record.has_configurable = true;
                    record.configurable = value.is_truthy();
                }
                "value" => {
                    record.has_value = true;
                    record.value = value;
                }
                "writable" => {
                    record.has_writable = true;
                    record.writable = value.is_truthy();
                }
                "get" => {
                    if !value.is_undefined() && !Self::is_callable_value(value) {
                        return Err(VmError::TypeError(
                            "Getter for property descriptor must be callable".to_string(),
                        ));
                    }
                    record.has_get = true;
                    record.get = value;
                }
                "set" => {
                    if !value.is_undefined() && !Self::is_callable_value(value) {
                        return Err(VmError::TypeError(
                            "Setter for property descriptor must be callable".to_string(),
                        ));
                    }
                    record.has_set = true;
                    record.set = value;
                }
                _ => {}
            }
        }

        if (record.has_get || record.has_set) && (record.has_value || record.has_writable) {
            return Err(VmError::TypeError(
                "Invalid property descriptor: cannot mix accessors and value".to_string(),
            ));
        }

        let normalized = self.alloc_object_descriptor()?;
        if record.has_value {
            self.set_internal_descriptor_field(normalized, "value", record.value)?;
        }
        if record.has_writable {
            self.set_internal_descriptor_field(
                normalized,
                "writable",
                Value::bool(record.writable),
            )?;
        }
        if record.has_configurable {
            self.set_internal_descriptor_field(
                normalized,
                "configurable",
                Value::bool(record.configurable),
            )?;
        }
        if record.has_enumerable {
            self.set_internal_descriptor_field(
                normalized,
                "enumerable",
                Value::bool(record.enumerable),
            )?;
        }
        if record.has_get {
            self.set_internal_descriptor_field(normalized, "get", record.get)?;
        }
        if record.has_set {
            self.set_internal_descriptor_field(normalized, "set", record.set)?;
        }

        Ok(normalized)
    }

    fn set_descriptor_metadata(&self, target: Value, key: &str, descriptor: Value) {
        let mut metadata = self.metadata.lock();
        metadata.define_metadata_property(
            NODE_DESCRIPTOR_METADATA_KEY.to_string(),
            descriptor,
            target,
            key.to_string(),
        );
    }

    pub(in crate::vm::interpreter) fn define_data_property_on_target(
        &self,
        target: Value,
        key: &str,
        value: Value,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) -> Result<(), VmError> {
        let debug_array_prop = std::env::var("RAYA_DEBUG_ARRAY_PROP").is_ok();
        if debug_array_prop {
            eprintln!(
                "[defineData] target={:#x} is_object={} key={} value={:#x} attrs=({}, {}, {})",
                target.raw(),
                checked_object_ptr(target).is_some(),
                key,
                value.raw(),
                writable,
                enumerable,
                configurable
            );
        }
        let descriptor = self.alloc_object_descriptor()?;
        let Some(descriptor_ptr) = (unsafe { descriptor.as_ptr::<Object>() }) else {
            return Err(VmError::RuntimeError(
                "Failed to allocate property descriptor object".to_string(),
            ));
        };
        let descriptor_obj = unsafe { &mut *descriptor_ptr.as_ptr() };
        for (field_name, field_value) in [
            ("value", value),
            ("writable", Value::bool(writable)),
            ("enumerable", Value::bool(enumerable)),
            ("configurable", Value::bool(configurable)),
        ] {
            if let Some(field_index) = self.get_field_index_for_value(descriptor, field_name) {
                descriptor_obj
                    .set_field(field_index, field_value)
                    .map_err(VmError::RuntimeError)?;
            }
            self.set_descriptor_field_present(descriptor, field_name, true);
        }
        let result = self.apply_descriptor_to_target(target, key, descriptor);
        if debug_array_prop {
            eprintln!("[defineData] done result={:?}", result);
        }
        result
    }

    fn get_descriptor_metadata(&self, target: Value, key: &str) -> Option<Value> {
        if self.fixed_property_deleted(target, key) {
            return None;
        }
        let metadata = self.metadata.lock();
        metadata.get_metadata_property(NODE_DESCRIPTOR_METADATA_KEY, target, key)
    }

    pub(in crate::vm::interpreter) fn metadata_data_property_value(
        &self,
        target: Value,
        key: &str,
    ) -> Option<Value> {
        if self.fixed_property_deleted(target, key) {
            return None;
        }
        let metadata = self.metadata.lock();
        metadata.get_metadata_property(NON_OBJECT_DYNAMIC_VALUE_METADATA_KEY, target, key)
    }

    fn is_descriptor_object(&self, value: Value) -> bool {
        self.metadata
            .lock()
            .get_metadata(OBJECT_DESCRIPTOR_MARKER_METADATA_KEY, value)
            .and_then(|marker| marker.as_bool())
            .unwrap_or(false)
    }

    pub(in crate::vm::interpreter) fn descriptor_field_present(
        &self,
        descriptor: Value,
        field_name: &str,
    ) -> bool {
        if self.is_descriptor_object(descriptor) {
            return self
                .metadata
                .lock()
                .get_metadata_property(
                    OBJECT_DESCRIPTOR_PRESENT_METADATA_KEY,
                    descriptor,
                    field_name,
                )
                .and_then(|present| present.as_bool())
                .unwrap_or(false);
        }
        self.get_own_field_value_by_name(descriptor, field_name)
            .is_some()
    }

    pub(in crate::vm::interpreter) fn set_descriptor_field_present(
        &self,
        descriptor: Value,
        field_name: &str,
        present: bool,
    ) {
        if !self.is_descriptor_object(descriptor) {
            return;
        }
        let mut metadata = self.metadata.lock();
        if present {
            metadata.define_metadata_property(
                OBJECT_DESCRIPTOR_PRESENT_METADATA_KEY.to_string(),
                Value::bool(true),
                descriptor,
                field_name.to_string(),
            );
        } else {
            let _ = metadata.delete_metadata_property(
                OBJECT_DESCRIPTOR_PRESENT_METADATA_KEY,
                descriptor,
                field_name,
            );
        }
    }

    pub(in crate::vm::interpreter) fn is_property_enumerable(
        &self,
        target: Value,
        key: &str,
    ) -> bool {
        if let Some(descriptor) = self.get_descriptor_metadata(target, key) {
            return self.descriptor_flag(descriptor, "enumerable", false);
        }

        match self.synthesize_data_property_descriptor(target, key) {
            Ok(Some(descriptor)) => self.descriptor_flag(descriptor, "enumerable", false),
            _ => false,
        }
    }

    fn apply_descriptor_to_target(
        &self,
        target: Value,
        key: &str,
        descriptor: Value,
    ) -> Result<(), VmError> {
        if !descriptor.is_ptr() {
            return Err(VmError::TypeError(
                "Object property descriptor must be an object".to_string(),
            ));
        }

        // Block redefinition if previous descriptor was marked non-configurable.
        if let Some(existing) = self.get_descriptor_metadata(target, key) {
            if !self.descriptor_flag(existing, "configurable", true) {
                return Err(VmError::TypeError(format!(
                    "Cannot redefine non-configurable property '{}'",
                    key
                )));
            }
        } else if !self.has_own_js_property(target, key) && !self.is_js_value_extensible(target) {
            return Err(VmError::TypeError(format!(
                "Cannot define property '{}': object is not extensible",
                key
            )));
        }

        let has_getter = self.descriptor_field_present(descriptor, "get");
        let getter = if has_getter {
            self.get_field_value_by_name(descriptor, "get")
        } else {
            None
        };
        let has_setter = self.descriptor_field_present(descriptor, "set");
        let setter = if has_setter {
            self.get_field_value_by_name(descriptor, "set")
        } else {
            None
        };
        let has_accessor = has_getter || has_setter;
        let has_value = self.descriptor_field_present(descriptor, "value");
        let value_field = if has_value {
            self.get_field_value_by_name(descriptor, "value")
        } else {
            None
        };

        if has_getter {
            let getter_val = getter.unwrap_or(Value::undefined());
            if !getter_val.is_undefined() && !Self::is_callable_value(getter_val) {
                return Err(VmError::TypeError(format!(
                    "Getter for property '{}' must be callable",
                    key
                )));
            }
        }
        if has_setter {
            let setter_val = setter.unwrap_or(Value::undefined());
            if !setter_val.is_undefined() && !Self::is_callable_value(setter_val) {
                return Err(VmError::TypeError(format!(
                    "Setter for property '{}' must be callable",
                    key
                )));
            }
        }
        if has_accessor && has_value {
            return Err(VmError::TypeError(format!(
                "Invalid property descriptor for '{}': cannot mix accessors and value",
                key
            )));
        }

        // Apply data descriptor value directly to the target field if provided.
        if let Some(value) = value_field {
            if self.set_builtin_global_property(target, key, value) {
                self.set_descriptor_metadata(target, key, descriptor);
                if self
                    .callable_virtual_property_descriptor(target, key)
                    .is_some()
                {
                    self.set_cached_callable_virtual_property_value(target, key, value);
                }
                self.set_callable_virtual_property_deleted(target, key, false);
                self.set_fixed_property_deleted(target, key, false);
                return Ok(());
            }
            if let Some(array_ptr) = checked_array_ptr(target) {
                if key == "length" {
                    self.set_array_length_value(target, value)?;
                } else if let Ok(index) = key.parse::<usize>() {
                    let array = unsafe { &mut *array_ptr.as_ptr() };
                    if !self.is_js_value_extensible(target) && array.get(index).is_none() {
                        return Err(VmError::TypeError(format!(
                            "Cannot define property '{}': object is not extensible",
                            key
                        )));
                    }
                    if index >= array.elements.len() {
                        array.resize_holey(index + 1);
                    }
                    array.set(index, value).map_err(VmError::RuntimeError)?;
                } else {
                    self.metadata.lock().define_metadata_property(
                        NON_OBJECT_DYNAMIC_VALUE_METADATA_KEY.to_string(),
                        value,
                        target,
                        key.to_string(),
                    );
                }
            } else if let Some(obj_ptr) = checked_object_ptr(target) {
                let obj = unsafe { &mut *obj_ptr.as_ptr() };
                if let Some(field_index) = self.get_field_index_for_value(target, key) {
                    obj.set_field(field_index, value)
                        .map_err(VmError::RuntimeError)?;
                } else {
                    let prop_key = self.intern_prop_key(key);
                    obj.ensure_dyn_map().insert(prop_key, value);
                }
            } else {
                self.metadata.lock().define_metadata_property(
                    NON_OBJECT_DYNAMIC_VALUE_METADATA_KEY.to_string(),
                    value,
                    target,
                    key.to_string(),
                );
            }
            if self
                .callable_virtual_property_descriptor(target, key)
                .is_some()
            {
                self.set_cached_callable_virtual_property_value(target, key, value);
            }
        }

        self.set_descriptor_metadata(target, key, descriptor);
        self.set_callable_virtual_property_deleted(target, key, false);
        self.set_fixed_property_deleted(target, key, false);
        Ok(())
    }

    fn apply_descriptor_to_target_with_context(
        &mut self,
        target: Value,
        key: &str,
        descriptor: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<(), VmError> {
        if !descriptor.is_ptr() {
            return Err(VmError::TypeError(
                "Object property descriptor must be an object".to_string(),
            ));
        }

        let descriptor = self.normalize_property_descriptor_with_context(
            descriptor,
            caller_task,
            caller_module,
        )?;

        if checked_array_ptr(target).is_some() && key == "length" {
            return self.apply_array_length_descriptor_with_context(
                target,
                descriptor,
                caller_task,
                caller_module,
            );
        }

        if let Some(existing) = self.get_descriptor_metadata(target, key) {
            if !self.descriptor_flag(existing, "configurable", true) {
                return Err(VmError::TypeError(format!(
                    "Cannot redefine non-configurable property '{}'",
                    key
                )));
            }
        } else if !self.has_own_js_property(target, key) && !self.is_js_value_extensible(target) {
            return Err(VmError::TypeError(format!(
                "Cannot define property '{}': object is not extensible",
                key
            )));
        }

        let has_getter = self.descriptor_field_present(descriptor, "get");
        let getter = if has_getter {
            self.get_field_value_by_name(descriptor, "get")
        } else {
            None
        };
        let has_setter = self.descriptor_field_present(descriptor, "set");
        let setter = if has_setter {
            self.get_field_value_by_name(descriptor, "set")
        } else {
            None
        };
        let has_accessor = has_getter || has_setter;
        let has_value = self.descriptor_field_present(descriptor, "value");
        let value_field = if has_value {
            self.get_field_value_by_name(descriptor, "value")
        } else {
            None
        };

        if has_getter {
            let getter_val = getter.unwrap_or(Value::undefined());
            if !getter_val.is_undefined() && !Self::is_callable_value(getter_val) {
                return Err(VmError::TypeError(format!(
                    "Getter for property '{}' must be callable",
                    key
                )));
            }
        }
        if has_setter {
            let setter_val = setter.unwrap_or(Value::undefined());
            if !setter_val.is_undefined() && !Self::is_callable_value(setter_val) {
                return Err(VmError::TypeError(format!(
                    "Setter for property '{}' must be callable",
                    key
                )));
            }
        }
        if has_accessor && has_value {
            return Err(VmError::TypeError(format!(
                "Invalid property descriptor for '{}': cannot mix accessors and value",
                key
            )));
        }

        if let Some(value) = value_field {
            if self.set_builtin_global_property(target, key, value) {
                self.set_descriptor_metadata(target, key, descriptor);
                if self
                    .callable_virtual_property_descriptor(target, key)
                    .is_some()
                {
                    self.set_cached_callable_virtual_property_value(target, key, value);
                }
                self.set_callable_virtual_property_deleted(target, key, false);
                self.set_fixed_property_deleted(target, key, false);
                return Ok(());
            }
            if let Some(array_ptr) = checked_array_ptr(target) {
                if key == "length" {
                    self.set_array_length_value_with_context(
                        target,
                        value,
                        caller_task,
                        caller_module,
                    )?;
                } else if let Ok(index) = key.parse::<usize>() {
                    let array = unsafe { &mut *array_ptr.as_ptr() };
                    if !self.is_js_value_extensible(target) && array.get(index).is_none() {
                        return Err(VmError::TypeError(format!(
                            "Cannot define property '{}': object is not extensible",
                            key
                        )));
                    }
                    if index >= array.elements.len() {
                        array.resize_holey(index + 1);
                    }
                    array.set(index, value).map_err(VmError::RuntimeError)?;
                } else {
                    self.metadata.lock().define_metadata_property(
                        NON_OBJECT_DYNAMIC_VALUE_METADATA_KEY.to_string(),
                        value,
                        target,
                        key.to_string(),
                    );
                }
            } else if let Some(obj_ptr) = checked_object_ptr(target) {
                let obj = unsafe { &mut *obj_ptr.as_ptr() };
                if let Some(field_index) = self.get_field_index_for_value(target, key) {
                    obj.set_field(field_index, value)
                        .map_err(VmError::RuntimeError)?;
                } else {
                    let prop_key = self.intern_prop_key(key);
                    obj.ensure_dyn_map().insert(prop_key, value);
                }
            } else {
                self.metadata.lock().define_metadata_property(
                    NON_OBJECT_DYNAMIC_VALUE_METADATA_KEY.to_string(),
                    value,
                    target,
                    key.to_string(),
                );
            }
            if self
                .callable_virtual_property_descriptor(target, key)
                .is_some()
            {
                self.set_cached_callable_virtual_property_value(target, key, value);
            }
        }

        self.set_descriptor_metadata(target, key, descriptor);
        self.set_callable_virtual_property_deleted(target, key, false);
        self.set_fixed_property_deleted(target, key, false);
        Ok(())
    }

    fn channel_from_handle_arg(&self, value: Value) -> Result<(u64, &ChannelObject), VmError> {
        let Some(handle) = value.as_u64() else {
            return Err(VmError::TypeError(
                "Expected channel handle (u64)".to_string(),
            ));
        };
        if !self.pinned_handles.read().contains(&handle) {
            return Err(VmError::TypeError(
                "Expected channel handle (u64)".to_string(),
            ));
        }
        let ch_ptr = handle as *const ChannelObject;
        if ch_ptr.is_null() {
            return Err(VmError::TypeError(
                "Expected channel handle (u64)".to_string(),
            ));
        }
        Ok((handle, unsafe { &*ch_ptr }))
    }

    fn buffer_handle_from_value(&self, value: Value) -> Result<u64, VmError> {
        let obj_ptr = unsafe { value.as_ptr::<Object>() }
            .ok_or_else(|| VmError::TypeError("Expected Buffer object".to_string()))?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let nominal_type_id = obj
            .nominal_type_id_usize()
            .ok_or_else(|| VmError::TypeError("Expected Buffer object".to_string()))?;
        let classes = self.classes.read();
        let class = classes
            .get_class(nominal_type_id)
            .ok_or_else(|| VmError::RuntimeError("Buffer class metadata missing".to_string()))?;
        if class.name != "Buffer" {
            return Err(VmError::TypeError("Expected Buffer object".to_string()));
        }
        drop(classes);

        let field_index = self
            .get_field_index_for_value(value, "bufferPtr")
            .ok_or_else(|| {
                VmError::RuntimeError("Buffer field 'bufferPtr' not found".to_string())
            })?;
        let handle = obj
            .get_field(field_index)
            .and_then(|f| f.as_u64())
            .ok_or_else(|| {
                VmError::RuntimeError("Buffer.bufferPtr is not a valid handle".to_string())
            })?;
        Ok(handle)
    }

    fn decode_u64_handle(value: Value) -> Option<u64> {
        if let Some(h) = value.as_u64() {
            return Some(h);
        }
        if let Some(i) = value.as_i64() {
            if i >= 0 {
                return Some(i as u64);
            }
        }
        if let Some(i) = value.as_i32() {
            if i >= 0 {
                return Some(i as u64);
            }
        }
        if let Some(f) = value.as_f64() {
            if f.is_finite() && f >= 0.0 && f.fract() == 0.0 && f <= u64::MAX as f64 {
                return Some(f as u64);
            }
        }
        None
    }

    fn map_handle_from_value(&self, value: Value) -> Result<u64, VmError> {
        if let Some(handle) = Self::decode_u64_handle(value) {
            return Ok(handle);
        }
        let obj_ptr = unsafe { value.as_ptr::<Object>() }
            .ok_or_else(|| VmError::TypeError("Expected Map object or map handle".to_string()))?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let field_index = self
            .get_field_index_for_value(value, "mapPtr")
            .ok_or_else(|| VmError::RuntimeError("Map field 'mapPtr' not found".to_string()))?;
        let raw = obj
            .get_field(field_index)
            .ok_or_else(|| VmError::RuntimeError("Map.mapPtr is missing".to_string()))?;
        Self::decode_u64_handle(raw)
            .ok_or_else(|| VmError::RuntimeError("Map.mapPtr is not a valid handle".to_string()))
    }

    fn set_handle_from_value(&self, value: Value) -> Result<u64, VmError> {
        if let Some(handle) = Self::decode_u64_handle(value) {
            return Ok(handle);
        }
        let obj_ptr = unsafe { value.as_ptr::<Object>() }
            .ok_or_else(|| VmError::TypeError("Expected Set object or set handle".to_string()))?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let field_index = self
            .get_field_index_for_value(value, "setPtr")
            .ok_or_else(|| VmError::RuntimeError("Set field 'setPtr' not found".to_string()))?;
        let raw = obj
            .get_field(field_index)
            .ok_or_else(|| VmError::RuntimeError("Set.setPtr is missing".to_string()))?;
        Self::decode_u64_handle(raw)
            .ok_or_else(|| VmError::RuntimeError("Set.setPtr is not a valid handle".to_string()))
    }

    pub(in crate::vm::interpreter) fn regexp_handle_from_value(
        &self,
        value: Value,
    ) -> Result<u64, VmError> {
        if let Some(handle) = Self::decode_u64_handle(value) {
            return Ok(handle);
        }
        let obj_ptr = unsafe { value.as_ptr::<Object>() }.ok_or_else(|| {
            VmError::TypeError("Expected RegExp object or regexp handle".to_string())
        })?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let field_index = self
            .get_field_index_for_value(value, "regexpPtr")
            .ok_or_else(|| {
                VmError::RuntimeError("RegExp field 'regexpPtr' not found".to_string())
            })?;
        let raw = obj
            .get_field(field_index)
            .ok_or_else(|| VmError::RuntimeError("RegExp.regexpPtr is missing".to_string()))?;
        Self::decode_u64_handle(raw).ok_or_else(|| {
            VmError::RuntimeError("RegExp.regexpPtr is not a valid handle".to_string())
        })
    }

    fn ensure_buffer_class_layout(&self) -> (usize, usize, LayoutId) {
        let mut classes = self.classes.write();
        if let Some(id) = classes.get_class_by_name("Buffer").map(|class| class.id) {
            let (layout_id, field_count) = self
                .nominal_allocation(id)
                .expect("registered Buffer allocation");
            (id, field_count.max(2), layout_id)
        } else {
            drop(classes);
            let id = self.register_runtime_class_with_layout_names(
                Class::new(0, "Buffer".to_string(), 2),
                Some(crate::vm::object::BUFFER_LAYOUT_FIELDS),
            );
            let (layout_id, field_count) = self
                .nominal_allocation(id)
                .expect("registered Buffer allocation");
            (id, field_count.max(2), layout_id)
        }
    }

    fn ensure_object_class_layout(&self) -> (usize, usize, LayoutId) {
        let mut classes = self.classes.write();
        if let Some(id) = classes.get_class_by_name("Object").map(|class| class.id) {
            let (_, mut field_count) = self
                .nominal_allocation(id)
                .expect("registered Object allocation");
            if field_count < 6 {
                drop(classes);
                self.set_nominal_field_count(id, 6);
                field_count = 6;
                classes = self.classes.write();
            }
            let (layout_id, field_count) = self
                .nominal_allocation(id)
                .expect("registered Object allocation");
            (id, field_count.max(6), layout_id)
        } else {
            drop(classes);
            let id = self.register_runtime_class_with_layout_names(
                Class::new(0, "Object".to_string(), 6),
                Some(crate::vm::object::OBJECT_DESCRIPTOR_LAYOUT_FIELDS),
            );
            let (layout_id, field_count) = self
                .nominal_allocation(id)
                .expect("registered Object allocation");
            (id, field_count.max(6), layout_id)
        }
    }

    fn ensure_symbol_class_layout(&self) -> (usize, usize, LayoutId) {
        let mut classes = self.classes.write();
        if let Some(id) = classes.get_class_by_name("Symbol").map(|class| class.id) {
            let (_, mut field_count) = self
                .nominal_allocation(id)
                .expect("registered Symbol allocation");
            if field_count < 1 {
                drop(classes);
                self.set_nominal_field_count(id, 1);
                field_count = 1;
                classes = self.classes.write();
            }
            let (layout_id, field_count) = self
                .nominal_allocation(id)
                .expect("registered Symbol allocation");
            (id, field_count.max(1), layout_id)
        } else {
            drop(classes);
            const SYMBOL_LAYOUT_FIELDS: &[&str] = &["key"];
            let id = self.register_runtime_class_with_layout_names(
                Class::new(0, "Symbol".to_string(), 1),
                Some(SYMBOL_LAYOUT_FIELDS),
            );
            let (layout_id, field_count) = self
                .nominal_allocation(id)
                .expect("registered Symbol allocation");
            (id, field_count.max(1), layout_id)
        }
    }

    fn alloc_buffer_object(&self, handle: u64, len: usize) -> Result<Value, VmError> {
        let (buffer_nominal_type_id, buffer_field_count, buffer_layout_id) =
            self.ensure_buffer_class_layout();
        let mut obj = Object::new_nominal(
            buffer_layout_id,
            buffer_nominal_type_id as u32,
            buffer_field_count,
        );
        obj.set_field(0, Value::u64(handle))
            .map_err(VmError::RuntimeError)?;
        if buffer_field_count > 1 {
            obj.set_field(1, Value::i32(len as i32))
                .map_err(VmError::RuntimeError)?;
        }
        let obj_ptr = self.gc.lock().allocate(obj);
        Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) })
    }

    fn alloc_object_descriptor(&self) -> Result<Value, VmError> {
        let field_names = crate::vm::object::OBJECT_DESCRIPTOR_LAYOUT_FIELDS
            .iter()
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>();
        let object_layout_id = layout_id_from_ordered_names(&field_names);
        self.register_structural_layout_shape(object_layout_id, &field_names);
        let object_field_count = field_names.len();
        let mut obj = Object::new_structural(object_layout_id, object_field_count);
        if object_field_count > 0 {
            obj.set_field(0, Value::undefined())
                .map_err(VmError::RuntimeError)?;
        }
        if object_field_count > 1 {
            obj.set_field(1, Value::bool(false))
                .map_err(VmError::RuntimeError)?;
        }
        if object_field_count > 2 {
            obj.set_field(2, Value::bool(false))
                .map_err(VmError::RuntimeError)?;
        }
        if object_field_count > 3 {
            obj.set_field(3, Value::bool(false))
                .map_err(VmError::RuntimeError)?;
        }
        if object_field_count > 4 {
            obj.set_field(4, Value::undefined())
                .map_err(VmError::RuntimeError)?;
        }
        if object_field_count > 5 {
            obj.set_field(5, Value::undefined())
                .map_err(VmError::RuntimeError)?;
        }
        let obj_ptr = self.gc.lock().allocate(obj);
        let descriptor =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) };
        self.metadata.lock().define_metadata(
            OBJECT_DESCRIPTOR_MARKER_METADATA_KEY.to_string(),
            Value::bool(true),
            descriptor,
        );
        Ok(descriptor)
    }

    fn alloc_symbol_object(&self, key: &str) -> Result<Value, VmError> {
        let (symbol_nominal_type_id, symbol_field_count, symbol_layout_id) =
            self.ensure_symbol_class_layout();
        let mut obj = Object::new_nominal(
            symbol_layout_id,
            symbol_nominal_type_id as u32,
            symbol_field_count,
        );
        let key_ptr = self.gc.lock().allocate(RayaString::new(key.to_string()));
        let key_value =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(key_ptr.as_ptr()).unwrap()) };
        obj.set_field(0, key_value).map_err(VmError::RuntimeError)?;
        let obj_ptr = self.gc.lock().allocate(obj);
        Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) })
    }

    fn synthesize_data_property_descriptor(
        &self,
        target: Value,
        key: &str,
    ) -> Result<Option<Value>, VmError> {
        if self.fixed_property_deleted(target, key) {
            return Ok(None);
        }
        let exotic_value = self.get_own_js_property_value_by_name(target, key);
        let callable_value = self
            .callable_virtual_property_value(target, key)
            .or_else(|| self.materialize_constructor_static_method(target, key));
        let builtin_global_value = self.builtin_global_property_value(target, key);
        let object_value = checked_object_ptr(target).map(|obj_ptr| {
            let obj = unsafe { &*obj_ptr.as_ptr() };
            let fixed_value = self
                .get_field_index_for_value(target, key)
                .and_then(|index| obj.get_field(index));
            let fixed_value =
                if fixed_value.is_some_and(|value| value.is_null()) && callable_value.is_some() {
                    None
                } else {
                    fixed_value
                };
            let dynamic_value = obj
                .dyn_map()
                .and_then(|dyn_map| dyn_map.get(&self.intern_prop_key(key)).copied());
            fixed_value.or(dynamic_value)
        });
        let metadata_value = self.metadata_data_property_value(target, key);
        let Some(value) = exotic_value
            .or(object_value.flatten())
            .or(metadata_value)
            .or(builtin_global_value)
            .or(callable_value)
        else {
            return Ok(None);
        };
        let own_flags = self.own_js_property_flags(target, key);
        let object_backed_value = object_value
            .flatten()
            .or(metadata_value)
            .or(builtin_global_value);

        let descriptor = self.alloc_object_descriptor()?;
        let Some(descriptor_ptr) = (unsafe { descriptor.as_ptr::<Object>() }) else {
            return Ok(None);
        };
        let descriptor_obj = unsafe { &mut *descriptor_ptr.as_ptr() };

        let legacy_error_descriptor = self.legacy_error_field_descriptor(target, key);
        let callable_virtual_descriptor = self.callable_virtual_property_descriptor(target, key);
        let writable_flag = callable_virtual_descriptor
            .or(legacy_error_descriptor)
            .map(|(writable, _, _)| writable)
            .or(own_flags.map(|(writable, _, _)| writable))
            .unwrap_or(object_backed_value.is_some());
        let configurable_flag = callable_virtual_descriptor
            .or(legacy_error_descriptor)
            .map(|(_, configurable, _)| configurable)
            .or(own_flags.map(|(_, configurable, _)| configurable))
            .unwrap_or(true);
        let callable_data_property = callable_virtual_descriptor.is_none()
            && object_backed_value.is_some()
            && self.callable_function_info(value).is_some()
            && self.callable_function_info(target).is_some();
        let enumerable_flag = callable_virtual_descriptor
            .or(legacy_error_descriptor)
            .map(|(_, _, enumerable)| enumerable)
            .or(own_flags.map(|(_, _, enumerable)| enumerable))
            .unwrap_or_else(|| {
                if callable_data_property {
                    false
                } else {
                    object_backed_value.is_some()
                }
            });

        if let Some(value_index) = self.get_field_index_for_value(descriptor, "value") {
            descriptor_obj
                .set_field(value_index, value)
                .map_err(VmError::RuntimeError)?;
        }
        if let Some(writable_index) = self.get_field_index_for_value(descriptor, "writable") {
            descriptor_obj
                .set_field(writable_index, Value::bool(writable_flag))
                .map_err(VmError::RuntimeError)?;
        }
        if let Some(configurable_index) = self.get_field_index_for_value(descriptor, "configurable")
        {
            descriptor_obj
                .set_field(configurable_index, Value::bool(configurable_flag))
                .map_err(VmError::RuntimeError)?;
        }
        if let Some(enumerable_index) = self.get_field_index_for_value(descriptor, "enumerable") {
            descriptor_obj
                .set_field(enumerable_index, Value::bool(enumerable_flag))
                .map_err(VmError::RuntimeError)?;
        }

        Ok(Some(descriptor))
    }

    fn synthesize_accessor_property_descriptor(
        &self,
        target: Value,
        key: &str,
    ) -> Result<Option<Value>, VmError> {
        let getter = self.callable_virtual_accessor_value(target, key, "get");
        let setter = self.callable_virtual_accessor_value(target, key, "set");
        if getter.is_none() && setter.is_none() {
            return Ok(None);
        }

        let descriptor = self.alloc_object_descriptor()?;
        let Some(descriptor_ptr) = (unsafe { descriptor.as_ptr::<Object>() }) else {
            return Ok(None);
        };
        let descriptor_obj = unsafe { &mut *descriptor_ptr.as_ptr() };

        if let Some(value_index) = self.get_field_index_for_value(descriptor, "value") {
            descriptor_obj
                .set_field(value_index, Value::undefined())
                .map_err(VmError::RuntimeError)?;
        }
        if let Some(writable_index) = self.get_field_index_for_value(descriptor, "writable") {
            descriptor_obj
                .set_field(writable_index, Value::undefined())
                .map_err(VmError::RuntimeError)?;
        }
        if let Some(configurable_index) = self.get_field_index_for_value(descriptor, "configurable")
        {
            descriptor_obj
                .set_field(configurable_index, Value::bool(true))
                .map_err(VmError::RuntimeError)?;
        }
        if let Some(enumerable_index) = self.get_field_index_for_value(descriptor, "enumerable") {
            descriptor_obj
                .set_field(enumerable_index, Value::bool(false))
                .map_err(VmError::RuntimeError)?;
        }
        if let Some(get_index) = self.get_field_index_for_value(descriptor, "get") {
            descriptor_obj
                .set_field(get_index, getter.unwrap_or(Value::undefined()))
                .map_err(VmError::RuntimeError)?;
        }
        if let Some(set_index) = self.get_field_index_for_value(descriptor, "set") {
            descriptor_obj
                .set_field(set_index, setter.unwrap_or(Value::undefined()))
                .map_err(VmError::RuntimeError)?;
        }

        Ok(Some(descriptor))
    }

    fn legacy_error_field_descriptor(
        &self,
        target: Value,
        key: &str,
    ) -> Option<(bool, bool, bool)> {
        let obj_ptr = checked_object_ptr(target)?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let nominal_class_name = obj.nominal_type_id_usize().and_then(|nominal_type_id| {
            let classes = self.classes.read();
            classes
                .get_class(nominal_type_id)
                .map(|class| class.name.clone())
        });
        let field_names = self.layout_field_names_for_object(obj).unwrap_or_default();
        let is_error_like = nominal_class_name.as_deref().is_some_and(|name| {
            matches!(
                name,
                "Error"
                    | "TypeError"
                    | "RangeError"
                    | "ReferenceError"
                    | "SyntaxError"
                    | "URIError"
                    | "EvalError"
                    | "InternalError"
                    | "AggregateError"
                    | "SuppressedError"
                    | "ChannelClosedError"
                    | "AssertionError"
            )
        }) || (field_names.iter().any(|name| name == "message")
            && field_names.iter().any(|name| name == "name"));
        if !is_error_like {
            return None;
        }

        match key {
            "message" | "name" | "stack" | "cause" | "code" | "errno" | "syscall" | "path" => {
                Some((true, true, false))
            }
            "errors"
                if nominal_class_name.as_deref() == Some("AggregateError")
                    || field_names.iter().any(|name| name == "errors") =>
            {
                Some((true, true, false))
            }
            _ => None,
        }
    }

    pub(in crate::vm::interpreter) fn exec_native_ops(
        &mut self,
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        module: &Module,
        task: &Arc<Task>,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::NativeCall => {
                let native_id = match Self::read_u16(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u8(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let debug_native_stack = std::env::var("RAYA_DEBUG_NATIVE_STACK").is_ok();
                if debug_native_stack {
                    let func_id = task.current_func_id();
                    let func_name = module
                        .functions
                        .get(func_id)
                        .map(|f| f.name.as_str())
                        .unwrap_or("<unknown>");
                    eprintln!(
                        "[native] enter {}#{} native_id={} arg_count={} stack_depth={}",
                        func_name,
                        func_id,
                        native_id,
                        arg_count,
                        stack.depth()
                    );
                }

                // Pop arguments
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    match stack.pop() {
                        Ok(v) => args.push(v),
                        Err(e) => {
                            if debug_native_stack {
                                let func_id = task.current_func_id();
                                let func_name = module
                                    .functions
                                    .get(func_id)
                                    .map(|f| f.name.as_str())
                                    .unwrap_or("<unknown>");
                                eprintln!(
                                    "[native] pop-underflow {}#{} native_id={} arg_count={} stack_depth={}",
                                    func_name,
                                    func_id,
                                    native_id,
                                    arg_count,
                                    stack.depth()
                                );
                            }
                            return OpcodeResult::Error(e);
                        }
                    }
                }
                args.reverse();

                // Route builtin array native IDs through shared array handler.
                // Native array calls use args = [receiver, ...methodArgs].
                if crate::vm::builtin::is_array_method(native_id) {
                    if args.is_empty() {
                        return OpcodeResult::Error(VmError::RuntimeError(
                            "Array native call requires receiver".to_string(),
                        ));
                    }
                    for arg in &args {
                        if let Err(e) = stack.push(*arg) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    let method_arg_count = args.len().saturating_sub(1);
                    return match self.call_array_method(
                        task,
                        stack,
                        native_id,
                        method_arg_count,
                        module,
                    ) {
                        Ok(()) => OpcodeResult::Continue,
                        Err(e) => OpcodeResult::Error(e),
                    };
                }

                // Route builtin string native IDs through shared string handler.
                if crate::vm::builtin::is_string_method(native_id) {
                    if args.is_empty() {
                        return OpcodeResult::Error(VmError::RuntimeError(
                            "String native call requires receiver".to_string(),
                        ));
                    }
                    for arg in &args {
                        if let Err(e) = stack.push(*arg) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    let method_arg_count = args.len().saturating_sub(1);
                    return match self.call_string_method(
                        task,
                        stack,
                        native_id,
                        method_arg_count,
                        module,
                    ) {
                        Ok(()) => OpcodeResult::Continue,
                        Err(e) => OpcodeResult::Error(e),
                    };
                }

                // Execute native call - handle channel operations specially for suspension
                match native_id {
                    id if id == crate::compiler::native_id::OBJECT_NEW => {
                        let value = match self.alloc_object_descriptor() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_WELL_KNOWN_SYMBOL => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.wellKnownSymbol requires 1 argument".to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.wellKnownSymbol expects a string key".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() }.data.clone();
                        let value = match self.alloc_symbol_object(&name) {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_GET_AMBIENT_GLOBAL => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "ambient global lookup expects exactly one string argument"
                                    .to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "ambient global lookup expects a string name".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() };
                        let Some(slot) = self
                            .builtin_global_slots
                            .read()
                            .get(name.data.as_str())
                            .copied()
                        else {
                            return OpcodeResult::Error(VmError::RuntimeError(format!(
                                "ambient builtin global '{}' is not initialized",
                                name.data
                            )));
                        };
                        let value = self
                            .globals_by_index
                            .read()
                            .get(slot)
                            .copied()
                            .unwrap_or(Value::null());
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_BIND_SCRIPT_GLOBAL => {
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "script global binding expects name and value".to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "script global binding expects a string name".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() };
                        if let Err(error) =
                            self.bind_script_global_property(&name.data, args[1], task, module)
                        {
                            return OpcodeResult::Error(error);
                        }
                        if let Err(e) = stack.push(args[1]) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_CALL_CONSTRUCTOR_BY_NAME => {
                        if args.len() < 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "parent constructor helper expects `this`, class name, and optional args"
                                    .to_string(),
                            ));
                        }
                        let this_arg = args[0];
                        let Some(name_ptr) = (unsafe { args[1].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "parent constructor helper expects a string class name".to_string(),
                            ));
                        };
                        let class_name = unsafe { &*name_ptr.as_ptr() }.data.clone();
                        let (constructor_id, constructor_module) = {
                            let classes = self.classes.read();
                            let Some(class) = classes.get_class_by_name(&class_name) else {
                                return OpcodeResult::Error(VmError::RuntimeError(format!(
                                    "Parent class '{}' not found",
                                    class_name
                                )));
                            };
                            (class.get_constructor(), class.module.clone())
                        };
                        if let Some(constructor_id) = constructor_id {
                            let closure = if let Some(module) = constructor_module {
                                Closure::with_module(constructor_id, Vec::new(), module)
                            } else {
                                Closure::new(constructor_id, Vec::new())
                            };
                            let closure_ptr = self.gc.lock().allocate(closure);
                            let closure_val = unsafe {
                                Value::from_ptr(
                                    std::ptr::NonNull::new(closure_ptr.as_ptr())
                                        .expect("parent constructor closure ptr"),
                                )
                            };
                            self.ephemeral_gc_roots.write().push(closure_val);
                            let invoke_args = args[2..].to_vec();
                            let invoke_result = self.invoke_callable_sync_with_this(
                                closure_val,
                                Some(this_arg),
                                &invoke_args,
                                task,
                                module,
                            );
                            {
                                let mut ephemeral = self.ephemeral_gc_roots.write();
                                if let Some(index) = ephemeral
                                    .iter()
                                    .rposition(|candidate| *candidate == closure_val)
                                {
                                    ephemeral.swap_remove(index);
                                }
                            }
                            if let Err(error) = invoke_result {
                                return OpcodeResult::Error(error);
                            }
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::FUNCTION_CONSTRUCTOR_HELPER => {
                        let value = match self.alloc_dynamic_js_function(&args, task, module) {
                            Ok(value) => value,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(value) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::FUNCTION_CALL_HELPER => {
                        if args.len() < 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Function.prototype.call requires a target function and thisArg"
                                    .to_string(),
                            ));
                        }
                        let target_callable = args[0];
                        let this_arg = args[1];
                        let rest_args = if args.len() >= 3 {
                            match self.collect_apply_arguments(args[2]) {
                                Ok(values) => values,
                                Err(error) => return OpcodeResult::Error(error),
                            }
                        } else {
                            Vec::new()
                        };
                        self.dispatch_call_with_explicit_this(
                            stack,
                            target_callable,
                            this_arg,
                            rest_args,
                            module,
                            task,
                            "Function.prototype.call target is not callable",
                        )
                    }

                    id if id == crate::compiler::native_id::FUNCTION_APPLY_HELPER => {
                        if args.len() < 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Function.prototype.apply requires a target function and thisArg"
                                    .to_string(),
                            ));
                        }
                        let target_callable = args[0];
                        let this_arg = args[1];
                        let apply_args = if args.len() >= 3 {
                            match self.collect_apply_arguments(args[2]) {
                                Ok(values) => values,
                                Err(error) => return OpcodeResult::Error(error),
                            }
                        } else {
                            Vec::new()
                        };

                        if let Some(target_ptr) = unsafe { target_callable.as_ptr::<u8>() } {
                            let header =
                                unsafe { &*header_ptr_from_value_ptr(target_ptr.as_ptr()) };
                            if header.type_id() == std::any::TypeId::of::<BoundNativeMethod>() {
                                let method = unsafe {
                                    &*target_callable
                                        .as_ptr::<BoundNativeMethod>()
                                        .expect("bound native target")
                                        .as_ptr()
                                };
                                return self.exec_bound_native_method_call(
                                    stack,
                                    this_arg,
                                    method.native_id,
                                    apply_args,
                                    module,
                                    task,
                                );
                            } else if header.type_id() == std::any::TypeId::of::<BoundMethod>() {
                                let method = unsafe {
                                    &*target_callable
                                        .as_ptr::<BoundMethod>()
                                        .expect("bound method target")
                                        .as_ptr()
                                };
                                if let Err(e) = stack.push(this_arg) {
                                    return OpcodeResult::Error(e);
                                }
                                for arg in &apply_args {
                                    if let Err(e) = stack.push(*arg) {
                                        return OpcodeResult::Error(e);
                                    }
                                }
                                return OpcodeResult::PushFrame {
                                    func_id: method.func_id,
                                    arg_count: apply_args.len() + 1,
                                    is_closure: false,
                                    closure_val: None,
                                    module: method.module.clone(),
                                    return_action: ReturnAction::PushReturnValue,
                                };
                            }
                        }

                        match self.callable_frame_for_value(
                            target_callable,
                            stack,
                            &apply_args,
                            Some(this_arg),
                            ReturnAction::PushReturnValue,
                        ) {
                            Ok(Some(frame)) => frame,
                            Ok(None) => OpcodeResult::Error(VmError::TypeError(
                                "Function.prototype.apply target is not callable".to_string(),
                            )),
                            Err(error) => OpcodeResult::Error(error),
                        }
                    }

                    id if id == crate::compiler::native_id::FUNCTION_BIND_HELPER => {
                        if args.len() < 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Function.prototype.bind requires a target function and thisArg"
                                    .to_string(),
                            ));
                        }
                        let target_callable = args[0];
                        if !Self::is_callable_value(target_callable) {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Function.prototype.bind target is not callable".to_string(),
                            ));
                        }
                        let this_arg = args[1];
                        let bound_args = if args.len() > 2 {
                            args[2..].to_vec()
                        } else {
                            Vec::new()
                        };
                        let bound = match self.alloc_bound_function(
                            target_callable,
                            this_arg,
                            bound_args,
                        ) {
                            Ok(value) => value,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(bound) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_CONSTRUCT_DYNAMIC_CLASS => {
                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "dynamic class construction requires type handle as first argument"
                                    .to_string(),
                            ));
                        }

                        if self.callable_native_alias_id(args[0])
                            == Some(crate::compiler::native_id::FUNCTION_CONSTRUCTOR_HELPER)
                        {
                            let value =
                                match self.alloc_dynamic_js_function(&args[1..], task, module) {
                                    Ok(value) => value,
                                    Err(error) => return OpcodeResult::Error(error),
                                };
                            if let Err(error) = stack.push(value) {
                                return OpcodeResult::Error(error);
                            }
                            return OpcodeResult::Continue;
                        }

                        let value = match self.construct_value_with_new_target(
                            args[0],
                            args[0],
                            &args[1..],
                            task,
                            module,
                        ) {
                            Ok(value) => value,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(value) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_INSTANCE_OF_DYNAMIC_CLASS => {
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "dynamic instanceof requires (object, classValue)".to_string(),
                            ));
                        }

                        let mut result = false;

                        if let Some(constructor_prototype) =
                            self.constructor_prototype_value(args[1])
                        {
                            let mut current = self.prototype_of_value(args[0]);
                            let mut seen = vec![args[0].raw()];
                            while let Some(prototype) = current {
                                if seen.contains(&prototype.raw()) {
                                    break;
                                }
                                seen.push(prototype.raw());
                                if prototype == constructor_prototype {
                                    result = true;
                                    break;
                                }
                                let next = self.prototype_of_value(prototype);
                                if next == current {
                                    break;
                                }
                                current = next;
                            }
                        }

                        if !result {
                            let Some(nominal_type_id) =
                                self.nominal_type_id_from_imported_class_value(module, args[1])
                            else {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "dynamic instanceof expects imported or ambient class value"
                                        .to_string(),
                                ));
                            };

                            let classes = self.classes.read();
                            result = crate::vm::reflect::is_instance_of(
                                &classes,
                                args[0],
                                nominal_type_id,
                            );
                            if std::env::var("RAYA_DEBUG_INSTANCEOF").is_ok() {
                                eprintln!(
                                    "[instanceof-dynamic] object={:#x} class_value={:#x} nominal_type_id={} result={}",
                                    args[0].raw(),
                                    args[1].raw(),
                                    nominal_type_id,
                                    result
                                );
                            }
                        }
                        if let Err(error) = stack.push(Value::bool(result)) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_NEW => {
                        // Create a new channel with given capacity
                        let capacity = args[0].as_i32().unwrap_or(0) as usize;
                        let ch = ChannelObject::new(capacity);
                        let handle = self.allocate_pinned_handle(ch);
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_SEND => {
                        // args: [channel_handle, value]
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_SEND requires 2 arguments".to_string(),
                            ));
                        }
                        let value = args[1];
                        let (handle, channel) = match self.channel_from_handle_arg(args[0]) {
                            Ok(tuple) => tuple,
                            Err(err) => return OpcodeResult::Error(err),
                        };

                        if channel.is_closed() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Channel closed".to_string(),
                            ));
                        }
                        if channel.try_send(value) {
                            if let Err(e) = stack.push(Value::null()) {
                                return OpcodeResult::Error(e);
                            }
                            OpcodeResult::Continue
                        } else {
                            use crate::vm::scheduler::SuspendReason;
                            OpcodeResult::Suspend(SuspendReason::ChannelSend {
                                channel_id: handle,
                                value,
                            })
                        }
                    }

                    CHANNEL_RECEIVE => {
                        // args: [channel_handle]
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_RECEIVE requires 1 argument".to_string(),
                            ));
                        }
                        let (handle, channel) = match self.channel_from_handle_arg(args[0]) {
                            Ok(tuple) => tuple,
                            Err(err) => return OpcodeResult::Error(err),
                        };

                        if let Some(val) = channel.try_receive() {
                            if debug_native_stack {
                                eprintln!("[native] CHANNEL_RECEIVE immediate value");
                            }
                            if let Err(e) = stack.push(val) {
                                return OpcodeResult::Error(e);
                            }
                            OpcodeResult::Continue
                        } else if channel.is_closed() {
                            if debug_native_stack {
                                eprintln!("[native] CHANNEL_RECEIVE closed->null");
                            }
                            if let Err(e) = stack.push(Value::null()) {
                                return OpcodeResult::Error(e);
                            }
                            OpcodeResult::Continue
                        } else {
                            if debug_native_stack {
                                eprintln!("[native] CHANNEL_RECEIVE suspend");
                            }
                            use crate::vm::scheduler::SuspendReason;
                            OpcodeResult::Suspend(SuspendReason::ChannelReceive {
                                channel_id: handle,
                            })
                        }
                    }

                    CHANNEL_TRY_SEND => {
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_TRY_SEND requires 2 arguments".to_string(),
                            ));
                        }
                        let value = args[1];
                        let (_, channel) = match self.channel_from_handle_arg(args[0]) {
                            Ok(tuple) => tuple,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let result = channel.try_send(value);
                        if let Err(e) = stack.push(Value::bool(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_TRY_RECEIVE => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_TRY_RECEIVE requires 1 argument".to_string(),
                            ));
                        }
                        let (_, channel) = match self.channel_from_handle_arg(args[0]) {
                            Ok(tuple) => tuple,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let result = channel.try_receive().unwrap_or(Value::null());
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_CLOSE => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_CLOSE requires 1 argument".to_string(),
                            ));
                        }
                        let (_, channel) = match self.channel_from_handle_arg(args[0]) {
                            Ok(tuple) => tuple,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        channel.close();
                        // Reactor will wake any waiting tasks on next iteration
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_IS_CLOSED => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_IS_CLOSED requires 1 argument".to_string(),
                            ));
                        }
                        let (_, channel) = match self.channel_from_handle_arg(args[0]) {
                            Ok(tuple) => tuple,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let closed = channel.is_closed();
                        if debug_native_stack {
                            eprintln!("[native] CHANNEL_IS_CLOSED -> {}", closed);
                        }
                        if let Err(e) = stack.push(Value::bool(closed)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_LENGTH => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_LENGTH requires 1 argument".to_string(),
                            ));
                        }
                        let (_, channel) = match self.channel_from_handle_arg(args[0]) {
                            Ok(tuple) => tuple,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        if let Err(e) = stack.push(Value::i32(channel.length() as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_CAPACITY => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_CAPACITY requires 1 argument".to_string(),
                            ));
                        }
                        let (_, channel) = match self.channel_from_handle_arg(args[0]) {
                            Ok(tuple) => tuple,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        if let Err(e) = stack.push(Value::i32(channel.capacity() as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    // Buffer native calls
                    id if id == buffer::NEW => {
                        let size = args[0].as_i32().unwrap_or(0) as usize;
                        let buf = Buffer::new(size);
                        let handle = self.allocate_pinned_handle(buf);
                        let wrapped = match self.alloc_buffer_object(handle, size) {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        if let Err(e) = stack.push(wrapped) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::LENGTH => {
                        let handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let buf = unsafe { &*buf_ptr };
                        if let Err(e) = stack.push(Value::i32(buf.length() as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::GET_BYTE => {
                        let handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let index = args[1].as_i32().unwrap_or(0) as usize;
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let buf = unsafe { &*buf_ptr };
                        let value = buf.get_byte(index).unwrap_or(0);
                        if let Err(e) = stack.push(Value::i32(value as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::SET_BYTE => {
                        let handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let index = args[1].as_i32().unwrap_or(0) as usize;
                        let value = args[2].as_i32().unwrap_or(0) as u8;
                        let buf_ptr = handle as *mut Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let buf = unsafe { &mut *buf_ptr };
                        if let Err(msg) = buf.set_byte(index, value) {
                            return OpcodeResult::Error(VmError::RuntimeError(msg));
                        }
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::GET_INT32 => {
                        let handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let index = args[1].as_i32().unwrap_or(0) as usize;
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let buf = unsafe { &*buf_ptr };
                        let value = buf.get_int32(index).unwrap_or(0);
                        if let Err(e) = stack.push(Value::i32(value)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::SET_INT32 => {
                        let handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let index = args[1].as_i32().unwrap_or(0) as usize;
                        let value = args[2].as_i32().unwrap_or(0);
                        let buf_ptr = handle as *mut Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let buf = unsafe { &mut *buf_ptr };
                        if let Err(msg) = buf.set_int32(index, value) {
                            return OpcodeResult::Error(VmError::RuntimeError(msg));
                        }
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::GET_FLOAT64 => {
                        let handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let index = args[1].as_i32().unwrap_or(0) as usize;
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let buf = unsafe { &*buf_ptr };
                        let value = buf.get_float64(index).unwrap_or(0.0);
                        if let Err(e) = stack.push(Value::f64(value)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::SET_FLOAT64 => {
                        let handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let index = args[1].as_i32().unwrap_or(0) as usize;
                        let value = args[2].as_f64().unwrap_or(0.0);
                        let buf_ptr = handle as *mut Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let buf = unsafe { &mut *buf_ptr };
                        if let Err(msg) = buf.set_float64(index, value) {
                            return OpcodeResult::Error(VmError::RuntimeError(msg));
                        }
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::SLICE => {
                        let handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let start = args[1].as_i32().unwrap_or(0) as usize;
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let buf = unsafe { &*buf_ptr };
                        // end is optional - if not provided, use buffer length
                        let end = if arg_count >= 3 {
                            args[2].as_i32().unwrap_or(buf.length() as i32) as usize
                        } else {
                            buf.length()
                        };
                        let sliced = buf.slice(start, end);
                        let sliced_len = sliced.length() as i32;
                        let new_handle = self.allocate_pinned_handle(sliced);

                        let value = match self.alloc_buffer_object(new_handle, sliced_len as usize)
                        {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };

                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::COPY => {
                        // copy(srcHandle, targetHandle, targetStart?, sourceStart?, sourceEnd?)
                        let src_handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let tgt_handle = match self.buffer_handle_from_value(args[1]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let src_ptr = src_handle as *const Buffer;
                        let tgt_ptr = tgt_handle as *mut Buffer;
                        if src_ptr.is_null() || tgt_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let src = unsafe { &*src_ptr };
                        let tgt = unsafe { &mut *tgt_ptr };

                        // Optional parameters with defaults
                        let tgt_start = if arg_count >= 3 {
                            args[2].as_i32().unwrap_or(0) as usize
                        } else {
                            0
                        };
                        let src_start = if arg_count >= 4 {
                            args[3].as_i32().unwrap_or(0) as usize
                        } else {
                            0
                        };
                        let src_end = if arg_count >= 5 {
                            args[4].as_i32().unwrap_or(src.data.len() as i32) as usize
                        } else {
                            src.data.len()
                        };

                        let src_end = src_end.min(src.data.len());
                        let src_start = src_start.min(src_end);
                        let bytes = &src.data[src_start..src_end];
                        let copy_len = bytes.len().min(tgt.data.len().saturating_sub(tgt_start));
                        tgt.data[tgt_start..tgt_start + copy_len]
                            .copy_from_slice(&bytes[..copy_len]);
                        if let Err(e) = stack.push(Value::i32(copy_len as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::TO_STRING => {
                        let handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let buf = unsafe { &*buf_ptr };
                        // encoding argument (args[1]) — currently only utf8/ascii supported
                        let text = String::from_utf8_lossy(&buf.data).into_owned();
                        let s = RayaString::new(text);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::FROM_STRING => {
                        // args[0] = string pointer, args[1] = encoding (ignored, utf8)
                        if !args[0].is_ptr() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected string".to_string(),
                            ));
                        }
                        let str_ptr = unsafe { args[0].as_ptr::<RayaString>() };
                        let s = match str_ptr {
                            Some(p) => unsafe { &*p.as_ptr() },
                            None => {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "Expected string".to_string(),
                                ))
                            }
                        };
                        let bytes = s.data.as_bytes();
                        let mut buf = Buffer::new(bytes.len());
                        buf.data.copy_from_slice(bytes);
                        let new_handle = self.allocate_pinned_handle(buf);
                        let value = match self.alloc_buffer_object(new_handle, bytes.len()) {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };

                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Mutex native calls
                    id if id == mutex::TRY_LOCK => {
                        let mutex_id = MutexId::from_u64(args[0].as_i64().unwrap_or(0) as u64);
                        if let Some(mutex) = self.mutex_registry.get(mutex_id) {
                            match mutex.try_lock(task.id()) {
                                Ok(()) => {
                                    task.add_held_mutex(mutex_id);
                                    if let Err(e) = stack.push(Value::bool(true)) {
                                        return OpcodeResult::Error(e);
                                    }
                                }
                                Err(_) => {
                                    if let Err(e) = stack.push(Value::bool(false)) {
                                        return OpcodeResult::Error(e);
                                    }
                                }
                            }
                        } else {
                            return OpcodeResult::Error(VmError::RuntimeError(format!(
                                "Mutex {:?} not found",
                                mutex_id
                            )));
                        }
                        OpcodeResult::Continue
                    }
                    id if id == mutex::IS_LOCKED => {
                        let mutex_id = MutexId::from_u64(args[0].as_i64().unwrap_or(0) as u64);
                        if let Some(mutex) = self.mutex_registry.get(mutex_id) {
                            let is_locked = mutex.is_locked();
                            if let Err(e) = stack.push(Value::bool(is_locked)) {
                                return OpcodeResult::Error(e);
                            }
                        } else {
                            return OpcodeResult::Error(VmError::RuntimeError(format!(
                                "Mutex {:?} not found",
                                mutex_id
                            )));
                        }
                        OpcodeResult::Continue
                    }
                    id if id == url::ENCODE => {
                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "encodeURI requires 1 argument".to_string(),
                            ));
                        }
                        let encoded = match value_as_string(args[0]) {
                            Ok(input) => percent_encode_uri_component(&input),
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let s = RayaString::new(encoded);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let result = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == url::DECODE => {
                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "decodeURI requires 1 argument".to_string(),
                            ));
                        }
                        let decoded = match value_as_string(args[0])
                            .and_then(|input| percent_decode_uri_component(&input))
                        {
                            Ok(decoded) => decoded,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let s = RayaString::new(decoded);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let result = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Map native calls
                    id if id == map::NEW => {
                        let map = MapObject::new();
                        let handle = self.allocate_pinned_handle(map);
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::SIZE => {
                        let handle = match self.map_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid map handle".to_string(),
                            ));
                        }
                        let map = unsafe { &*map_ptr };
                        if let Err(e) = stack.push(Value::i32(map.size() as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::GET => {
                        let handle = match self.map_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let key = args[1];
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid map handle".to_string(),
                            ));
                        }
                        let map = unsafe { &*map_ptr };
                        let value = map.get(key).unwrap_or(Value::null());
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::SET => {
                        let handle = match self.map_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let key = args[1];
                        let value = args[2];
                        let map_ptr = handle as *mut MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid map handle".to_string(),
                            ));
                        }
                        let map = unsafe { &mut *map_ptr };
                        map.set(key, value);
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::HAS => {
                        let handle = match self.map_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let key = args[1];
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid map handle".to_string(),
                            ));
                        }
                        let map = unsafe { &*map_ptr };
                        if let Err(e) = stack.push(Value::bool(map.has(key))) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::DELETE => {
                        let handle = match self.map_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let key = args[1];
                        let map_ptr = handle as *mut MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid map handle".to_string(),
                            ));
                        }
                        let map = unsafe { &mut *map_ptr };
                        let result = map.delete(key);
                        if let Err(e) = stack.push(Value::bool(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::CLEAR => {
                        let handle = match self.map_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let map_ptr = handle as *mut MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid map handle".to_string(),
                            ));
                        }
                        let map = unsafe { &mut *map_ptr };
                        map.clear();
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::KEYS => {
                        let handle = match self.map_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid map handle".to_string(),
                            ));
                        }
                        let map = unsafe { &*map_ptr };
                        let keys = map.keys();
                        let mut arr = Array::new(0, 0);
                        for key in keys {
                            arr.push(key);
                        }
                        let arr_gc = self.gc.lock().allocate(arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::VALUES => {
                        let handle = match self.map_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid map handle".to_string(),
                            ));
                        }
                        let map = unsafe { &*map_ptr };
                        let values = map.values();
                        let mut arr = Array::new(0, 0);
                        for val in values {
                            arr.push(val);
                        }
                        let arr_gc = self.gc.lock().allocate(arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::ENTRIES => {
                        let handle = match self.map_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid map handle".to_string(),
                            ));
                        }
                        let map = unsafe { &*map_ptr };
                        let entries = map.entries();
                        let mut arr = Array::new(0, 0);
                        for (key, val) in entries {
                            let mut entry = Array::new(0, 0);
                            entry.push(key);
                            entry.push(val);
                            let entry_gc = self.gc.lock().allocate(entry);
                            let entry_val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(entry_gc.as_ptr()).unwrap())
                            };
                            arr.push(entry_val);
                        }
                        let arr_gc = self.gc.lock().allocate(arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Set native calls
                    id if id == set::NEW => {
                        let set_obj = SetObject::new();
                        let handle = self.allocate_pinned_handle(set_obj);
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::SIZE => {
                        let handle = match self.set_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let set_ptr = handle as *const SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid set handle".to_string(),
                            ));
                        }
                        let set_obj = unsafe { &*set_ptr };
                        if let Err(e) = stack.push(Value::i32(set_obj.size() as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::ADD => {
                        let handle = match self.set_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let value = args[1];
                        let set_ptr = handle as *mut SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid set handle".to_string(),
                            ));
                        }
                        let set_obj = unsafe { &mut *set_ptr };
                        set_obj.add(value);
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::HAS => {
                        let handle = match self.set_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let value = args[1];
                        let set_ptr = handle as *const SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid set handle".to_string(),
                            ));
                        }
                        let set_obj = unsafe { &*set_ptr };
                        if let Err(e) = stack.push(Value::bool(set_obj.has(value))) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::DELETE => {
                        let handle = match self.set_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let value = args[1];
                        let set_ptr = handle as *mut SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid set handle".to_string(),
                            ));
                        }
                        let set_obj = unsafe { &mut *set_ptr };
                        let result = set_obj.delete(value);
                        if let Err(e) = stack.push(Value::bool(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::CLEAR => {
                        let handle = match self.set_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let set_ptr = handle as *mut SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid set handle".to_string(),
                            ));
                        }
                        let set_obj = unsafe { &mut *set_ptr };
                        set_obj.clear();
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::VALUES => {
                        let handle = match self.set_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let set_ptr = handle as *const SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid set handle".to_string(),
                            ));
                        }
                        let set_obj = unsafe { &*set_ptr };
                        let values = set_obj.values();
                        let mut arr = Array::new(0, 0);
                        for val in values {
                            arr.push(val);
                        }
                        let arr_gc = self.gc.lock().allocate(arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::UNION => {
                        let handle_a = match self.set_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let handle_b = match self.set_handle_from_value(args[1]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let set_a_ptr = handle_a as *const SetObject;
                        let set_b_ptr = handle_b as *const SetObject;
                        if set_a_ptr.is_null() || set_b_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid set handle".to_string(),
                            ));
                        }
                        let set_a = unsafe { &*set_a_ptr };
                        let set_b = unsafe { &*set_b_ptr };
                        let mut result = SetObject::new();
                        for val in set_a.values() {
                            result.add(val);
                        }
                        for val in set_b.values() {
                            result.add(val);
                        }
                        let handle = self.allocate_pinned_handle(result);
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::INTERSECTION => {
                        let handle_a = match self.set_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let handle_b = match self.set_handle_from_value(args[1]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let set_a_ptr = handle_a as *const SetObject;
                        let set_b_ptr = handle_b as *const SetObject;
                        if set_a_ptr.is_null() || set_b_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid set handle".to_string(),
                            ));
                        }
                        let set_a = unsafe { &*set_a_ptr };
                        let set_b = unsafe { &*set_b_ptr };
                        let mut result = SetObject::new();
                        for val in set_a.values() {
                            if set_b.has(val) {
                                result.add(val);
                            }
                        }
                        let handle = self.allocate_pinned_handle(result);
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::DIFFERENCE => {
                        let handle_a = match self.set_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let handle_b = match self.set_handle_from_value(args[1]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let set_a_ptr = handle_a as *const SetObject;
                        let set_b_ptr = handle_b as *const SetObject;
                        if set_a_ptr.is_null() || set_b_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid set handle".to_string(),
                            ));
                        }
                        let set_a = unsafe { &*set_a_ptr };
                        let set_b = unsafe { &*set_b_ptr };
                        let mut result = SetObject::new();
                        for val in set_a.values() {
                            if !set_b.has(val) {
                                result.add(val);
                            }
                        }
                        let handle = self.allocate_pinned_handle(result);
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Number native calls
                    0x0F00u16 => {
                        // NUMBER_TO_FIXED: format number with fixed decimal places
                        // args[0] = number value, args[1] = digits
                        let value = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0);
                        let digits = args.get(1).and_then(|v| v.as_i32()).unwrap_or(0) as usize;
                        let formatted = format!("{:.prec$}", value, prec = digits);
                        let s = RayaString::new(formatted);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0F01u16 => {
                        // NUMBER_TO_PRECISION: format with N significant digits (or plain if no arg)
                        let value = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0);
                        let formatted = if args.get(1).is_none() {
                            // No precision argument: return plain toString()
                            if value.fract() == 0.0 && value.abs() < i64::MAX as f64 {
                                format!("{}", value as i64)
                            } else {
                                format!("{}", value)
                            }
                        } else {
                            let precision =
                                args.get(1).and_then(|v| v.as_i32()).unwrap_or(1).max(1) as usize;
                            if !value.is_finite() {
                                format!("{}", value)
                            } else if value == 0.0 {
                                if precision == 1 {
                                    "0".to_string()
                                } else {
                                    format!("0.{}", "0".repeat(precision - 1))
                                }
                            } else {
                                let magnitude = value.abs().log10().floor() as i32;
                                let scale_pow = magnitude - precision as i32 + 1;
                                let scale = 10f64.powi(scale_pow);
                                let rounded = (value / scale).round() * scale;
                                let decimal_places =
                                    (precision as i32 - magnitude - 1).max(0) as usize;
                                let mut text = format!("{:.prec$}", rounded, prec = decimal_places);
                                if decimal_places > 0 {
                                    while text.ends_with('0') {
                                        text.pop();
                                    }
                                    if text.ends_with('.') {
                                        text.pop();
                                    }
                                }
                                if text == "-0" {
                                    "0".to_string()
                                } else {
                                    text
                                }
                            }
                        };
                        let s = RayaString::new(formatted);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0F02u16 => {
                        // NUMBER_TO_STRING_RADIX: convert to string with radix
                        let value = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0);
                        let radix = args.get(1).and_then(|v| v.as_i32()).unwrap_or(10);
                        let formatted = if radix == 10 || !(2..=36).contains(&radix) {
                            if value.fract() == 0.0 && value.abs() < i64::MAX as f64 {
                                format!("{}", value as i64)
                            } else {
                                format!("{}", value)
                            }
                        } else {
                            // Integer radix conversion
                            let int_val = value as i64;
                            match radix {
                                2 => format!("{:b}", int_val),
                                8 => format!("{:o}", int_val),
                                16 => format!("{:x}", int_val),
                                _ => {
                                    // General radix conversion
                                    if int_val == 0 {
                                        "0".to_string()
                                    } else {
                                        let negative = int_val < 0;
                                        let mut n = int_val.unsigned_abs();
                                        let mut digits = Vec::new();
                                        let radix = radix as u64;
                                        while n > 0 {
                                            let d = (n % radix) as u8;
                                            digits.push(if d < 10 {
                                                b'0' + d
                                            } else {
                                                b'a' + d - 10
                                            });
                                            n /= radix;
                                        }
                                        digits.reverse();
                                        let s = String::from_utf8(digits).unwrap_or_default();
                                        if negative {
                                            format!("-{}", s)
                                        } else {
                                            s
                                        }
                                    }
                                }
                            }
                        };
                        let s = RayaString::new(formatted);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0F03u16 => {
                        // PARSE_INT: parse string to integer
                        let result = if let Some(ptr) = unsafe { args[0].as_ptr::<RayaString>() } {
                            let s = unsafe { &*ptr.as_ptr() }.data.trim();
                            // Parse integer, handling leading whitespace and optional sign
                            s.parse::<i64>()
                                .map(|v| v as f64)
                                .or_else(|_| s.parse::<f64>().map(|v| v.trunc()))
                                .unwrap_or(f64::NAN)
                        } else if let Some(n) = args[0].as_f64() {
                            n.trunc()
                        } else if let Some(n) = args[0].as_i32() {
                            n as f64
                        } else {
                            f64::NAN
                        };
                        if result.fract() == 0.0
                            && result.is_finite()
                            && result.abs() < i32::MAX as f64
                        {
                            if let Err(e) = stack.push(Value::i32(result as i32)) {
                                return OpcodeResult::Error(e);
                            }
                        } else if let Err(e) = stack.push(Value::f64(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0F04u16 => {
                        // PARSE_FLOAT: parse string to float
                        let result = if let Some(ptr) = unsafe { args[0].as_ptr::<RayaString>() } {
                            let s = unsafe { &*ptr.as_ptr() }.data.trim();
                            s.parse::<f64>().unwrap_or(f64::NAN)
                        } else if let Some(n) = args[0].as_f64() {
                            n
                        } else if let Some(n) = args[0].as_i32() {
                            n as f64
                        } else {
                            f64::NAN
                        };
                        if let Err(e) = stack.push(Value::f64(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0F05u16 => {
                        // IS_NAN: check if value is NaN
                        let is_nan = if let Some(n) = args[0].as_f64() {
                            n.is_nan()
                        } else if args[0].as_i32().is_some() {
                            false // integers are never NaN
                        } else {
                            true // non-numbers are treated as NaN
                        };
                        if let Err(e) = stack.push(Value::bool(is_nan)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0F06u16 => {
                        // IS_FINITE: check if value is finite
                        let is_finite = if let Some(n) = args[0].as_f64() {
                            n.is_finite()
                        } else if args[0].as_i32().is_some() {
                            true // integers are always finite
                        } else {
                            false
                        };
                        if let Err(e) = stack.push(Value::bool(is_finite)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Object native calls
                    0x0001u16 => {
                        let target = args.first().copied().unwrap_or(Value::undefined());
                        let value = self.alloc_string_value(format!(
                            "[object {}]",
                            self.object_to_string_tag(target)
                        ));
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0002u16 => {
                        // OBJECT_HASH_CODE: return identity hash from object pointer
                        let hash = if !args.is_empty() {
                            // Use the raw bits of the value as a hash
                            let bits = args[0].as_u64().unwrap_or(0);
                            (bits ^ (bits >> 16)) as i32
                        } else {
                            0
                        };
                        if let Err(e) = stack.push(Value::i32(hash)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0003u16 => {
                        // OBJECT_EQUAL: reference equality
                        let equal = if args.len() >= 2 {
                            args[0].as_u64() == args[1].as_u64()
                        } else {
                            false
                        };
                        if let Err(e) = stack.push(Value::bool(equal)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0008u16 => {
                        let same = if args.len() >= 2 {
                            value_same_value(args[0], args[1])
                        } else {
                            false
                        };
                        if let Err(e) = stack.push(Value::bool(same)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0004u16 => {
                        // OBJECT_DEFINE_PROPERTY(target, key, descriptor) -> target
                        if args.len() < 3 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.defineProperty requires 3 arguments".to_string(),
                            ));
                        }
                        let target = args[0];
                        let key_val = args[1];
                        let descriptor = args[2];

                        if !target.is_ptr() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.defineProperty target must be an object".to_string(),
                            ));
                        }
                        let (Some(key), _) = (match self.property_key_parts_with_context(
                            key_val,
                            "Object.defineProperty",
                            task,
                            module,
                        ) {
                            Ok(parts) => parts,
                            Err(error) => return OpcodeResult::Error(error),
                        }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.defineProperty key must be a string or symbol".to_string(),
                            ));
                        };

                        if let Err(e) = self.apply_descriptor_to_target_with_context(
                            target, &key, descriptor, task, module,
                        ) {
                            return OpcodeResult::Error(e);
                        }
                        if let Err(e) = stack.push(target) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0005u16 => {
                        // OBJECT_GET_OWN_PROPERTY_DESCRIPTOR(target, key) -> descriptor | undefined
                        if args.len() < 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.getOwnPropertyDescriptor requires 2 arguments".to_string(),
                            ));
                        }
                        let target = args[0];
                        let key_val = args[1];
                        if !target.is_ptr() {
                            if let Err(e) = stack.push(Value::undefined()) {
                                return OpcodeResult::Error(e);
                            }
                            return OpcodeResult::Continue;
                        }
                        let (Some(key), _) = (match self.property_key_parts_with_context(
                            key_val,
                            "Object.getOwnPropertyDescriptor",
                            task,
                            module,
                        ) {
                            Ok(parts) => parts,
                            Err(error) => return OpcodeResult::Error(error),
                        }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.getOwnPropertyDescriptor key must be a string or symbol"
                                    .to_string(),
                            ));
                        };
                        let value = match self.get_descriptor_metadata(target, &key) {
                            Some(descriptor) => descriptor,
                            None => {
                                match self.synthesize_accessor_property_descriptor(target, &key) {
                                    Ok(Some(descriptor)) => descriptor,
                                    Ok(None) => match self
                                        .synthesize_data_property_descriptor(target, &key)
                                    {
                                        Ok(Some(descriptor)) => descriptor,
                                        Ok(None) => Value::undefined(),
                                        Err(error) => return OpcodeResult::Error(error),
                                    },
                                    Err(error) => return OpcodeResult::Error(error),
                                }
                            }
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0011u16 => {
                        // OBJECT_GET_PROTOTYPE_OF(target) -> prototype | null
                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.getPrototypeOf requires 1 argument".to_string(),
                            ));
                        }
                        let target = args[0];
                        if std::env::var("RAYA_DEBUG_PROTO_RESOLVE").is_ok() {
                            eprintln!(
                                "[get-proto-native] target={:#x} is_object={} callable={} explicit={}",
                                target.raw(),
                                checked_object_ptr(target).is_some(),
                                self.callable_function_info(target).is_some(),
                                self.explicit_object_prototype(target)
                                    .map(|value| format!("{:#x}", value.raw()))
                                    .unwrap_or_else(|| "None".to_string())
                            );
                        }
                        if target.is_null() || target.is_undefined() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Cannot convert undefined or null to object".to_string(),
                            ));
                        }
                        let prototype = self.prototype_of_value(target).unwrap_or(Value::null());
                        if let Err(e) = stack.push(prototype) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_GET_CLASS_VALUE => {
                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.getClassValue requires 1 argument".to_string(),
                            ));
                        }
                        let Some(local_nominal_type_id) =
                            args[0].as_i32().filter(|id| *id >= 0).map(|id| id as usize)
                        else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.getClassValue expects a non-negative nominal type id"
                                    .to_string(),
                            ));
                        };
                        let nominal_type_id =
                            match self.resolve_nominal_type_id(module, local_nominal_type_id) {
                                Ok(id) => id,
                                Err(error) => return OpcodeResult::Error(error),
                            };
                        let Some(value) = self.constructor_value_for_nominal_type(nominal_type_id)
                        else {
                            return OpcodeResult::Error(VmError::RuntimeError(format!(
                                "Object.getClassValue could not resolve nominal type {}",
                                nominal_type_id
                            )));
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_IS_EXTENSIBLE => {
                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.isExtensible requires 1 argument".to_string(),
                            ));
                        }
                        if let Err(e) =
                            stack.push(Value::bool(self.is_js_value_extensible(args[0])))
                        {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_PREVENT_EXTENSIONS => {
                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.preventExtensions requires 1 argument".to_string(),
                            ));
                        }
                        let target = args[0];
                        self.set_js_value_extensible(target, false);
                        if let Err(e) = stack.push(target) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0006u16 => {
                        // OBJECT_DEFINE_PROPERTIES(target, descriptors) -> target
                        if args.len() < 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.defineProperties requires 2 arguments".to_string(),
                            ));
                        }
                        let target = args[0];
                        let descriptors_obj = args[1];
                        if !target.is_ptr() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.defineProperties target must be an object".to_string(),
                            ));
                        }
                        if let Some(desc_ptr) = unsafe { descriptors_obj.as_ptr::<Object>() } {
                            let desc_obj = unsafe { &*desc_ptr.as_ptr() };
                            let field_names = desc_obj
                                .nominal_type_id_usize()
                                .and_then(|nominal_type_id| {
                                    let metadata = self.class_metadata.read();
                                    metadata
                                        .get(nominal_type_id)
                                        .map(|m| m.field_names.clone())
                                        .filter(|names| !names.is_empty())
                                })
                                .or_else(|| self.layout_field_names_for_object(desc_obj))
                                .unwrap_or_default();
                            for (idx, field_name) in field_names.into_iter().enumerate() {
                                if field_name.is_empty() {
                                    continue;
                                }
                                if let Some(descriptor_val) = desc_obj.get_field(idx) {
                                    if let Err(e) = self.apply_descriptor_to_target_with_context(
                                        target,
                                        &field_name,
                                        descriptor_val,
                                        task,
                                        module,
                                    ) {
                                        return OpcodeResult::Error(e);
                                    }
                                }
                            }
                        } else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.defineProperties descriptors must be an object".to_string(),
                            ));
                        }
                        if let Err(e) = stack.push(target) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x000Cu16 => {
                        // OBJECT_DELETE_PROPERTY(target, key) -> bool
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.deleteProperty requires 2 arguments".to_string(),
                            ));
                        }
                        let deleted = match self
                            .delete_property_from_target(args[0], args[1], task, module)
                        {
                            Ok(result) => result,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(Value::bool(deleted)) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    // Task native calls
                    0x0500u16 => {
                        // TASK_IS_DONE: check if task completed
                        let task_id = TaskId::from_u64(args[0].as_u64().unwrap_or(0));
                        let tasks = self.tasks.read();
                        let is_done = tasks
                            .get(&task_id)
                            .map(|t| matches!(t.state(), TaskState::Completed | TaskState::Failed))
                            .unwrap_or(true);
                        if let Err(e) = stack.push(Value::bool(is_done)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0501u16 => {
                        // TASK_IS_CANCELLED: check if task cancelled
                        let task_id = TaskId::from_u64(args[0].as_u64().unwrap_or(0));
                        let tasks = self.tasks.read();
                        let is_cancelled = tasks
                            .get(&task_id)
                            .map(|t| t.is_cancelled())
                            .unwrap_or(false);
                        if let Err(e) = stack.push(Value::bool(is_cancelled)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0502u16 => {
                        // TASK_IS_FAILED: check if task failed
                        let task_id = TaskId::from_u64(args[0].as_u64().unwrap_or(0));
                        let tasks = self.tasks.read();
                        let is_failed = tasks
                            .get(&task_id)
                            .map(|t| t.state() == TaskState::Failed)
                            .unwrap_or(false);
                        if let Err(e) = stack.push(Value::bool(is_failed)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0503u16 => {
                        // TASK_GET_ERROR: retrieve rejection reason and mark it observed
                        let task_id = TaskId::from_u64(args[0].as_u64().unwrap_or(0));
                        let tasks = self.tasks.read();
                        let reason = tasks
                            .get(&task_id)
                            .and_then(|t| {
                                t.mark_rejection_observed();
                                t.current_exception()
                            })
                            .unwrap_or(Value::null());
                        if let Err(e) = stack.push(reason) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0504u16 => {
                        // TASK_MARK_OBSERVED: mark rejection as handled
                        let task_id = TaskId::from_u64(args[0].as_u64().unwrap_or(0));
                        let tasks = self.tasks.read();
                        if let Some(task) = tasks.get(&task_id) {
                            task.mark_rejection_observed();
                        }
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Error native calls
                    0x0600u16 => {
                        // ERROR_STACK (0x0600): return stack trace from error object.
                        // Stack traces are populated at throw time in exceptions.rs
                        // using the structural `stack` field surface.
                        // Normal e.stack access uses LoadFieldExact directly; this native
                        // handler serves as a fallback if called explicitly.
                        let result = if !args.is_empty() {
                            let error_val = args[0];
                            if let Some(obj_ptr) = unsafe { error_val.as_ptr::<Object>() } {
                                let obj = unsafe { &*obj_ptr.as_ptr() };
                                self.get_object_named_field_value(obj, "stack")
                                    .unwrap_or_else(|| {
                                        let s = RayaString::new(String::new());
                                        let gc_ptr = self.gc.lock().allocate(s);
                                        unsafe {
                                            Value::from_ptr(
                                                std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                            )
                                        }
                                    })
                            } else {
                                let s = RayaString::new(String::new());
                                let gc_ptr = self.gc.lock().allocate(s);
                                unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                    )
                                }
                            }
                        } else {
                            let s = RayaString::new(String::new());
                            let gc_ptr = self.gc.lock().allocate(s);
                            unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            }
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::ERROR_TO_STRING => {
                        let error_val = args.first().copied().unwrap_or(Value::null());
                        let name_val = self
                            .get_field_value_by_name(error_val, "name")
                            .unwrap_or(Value::null());
                        let message_val = self
                            .get_field_value_by_name(error_val, "message")
                            .unwrap_or(Value::null());

                        let to_string = |value: Value| -> String {
                            if value.is_null() {
                                return String::new();
                            }
                            if let Some(ptr) = unsafe { value.as_ptr::<RayaString>() } {
                                return unsafe { &*ptr.as_ptr() }.data.clone();
                            }
                            if let Some(i) = value.as_i32() {
                                return i.to_string();
                            }
                            if let Some(f) = value.as_f64() {
                                if f.fract() == 0.0 {
                                    return format!("{}", f as i64);
                                }
                                return f.to_string();
                            }
                            if let Some(b) = value.as_bool() {
                                return b.to_string();
                            }
                            String::new()
                        };

                        let mut name = to_string(name_val);
                        if name.is_empty() {
                            name = "Error".to_string();
                        }
                        let message = to_string(message_val);
                        let rendered = if message.is_empty() {
                            name
                        } else {
                            format!("{}: {}", name, message)
                        };
                        let s = RayaString::new(rendered);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Date native calls
                    id if id == date::NOW => {
                        use std::time::{SystemTime, UNIX_EPOCH};
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_millis() as f64)
                            .unwrap_or(0.0);
                        if let Err(e) = stack.push(Value::f64(now)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_FULL_YEAR => {
                        // args[0] is the timestamp in milliseconds (as f64 number)
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_full_year())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_MONTH => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_month())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_DATE => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_date())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_DAY => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_day())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_HOURS => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_hours())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_MINUTES => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_minutes())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_SECONDS => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_seconds())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_MILLISECONDS => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_milliseconds())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Date setters: args[0]=timestamp, args[1]=new value, returns new timestamp as f64
                    id if id == date::SET_FULL_YEAR => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(0);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_full_year(val) as f64)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_MONTH => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(0);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_month(val) as f64)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_DATE => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(1);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_date(val) as f64)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_HOURS => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(0);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_hours(val) as f64)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_MINUTES => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(0);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_minutes(val) as f64)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_SECONDS => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(0);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_seconds(val) as f64)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_MILLISECONDS => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(0);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_milliseconds(val) as f64)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Date string formatting: args[0]=timestamp, returns string
                    id if id == date::TO_STRING => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        let s = RayaString::new(date.to_string_repr());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::TO_ISO_STRING => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        let s = RayaString::new(date.to_iso_string());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::TO_DATE_STRING => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        let s = RayaString::new(date.to_date_string());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::TO_TIME_STRING => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        let s = RayaString::new(date.to_time_string());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Date.parse: args[0]=string, returns timestamp f64 (NaN on failure)
                    id if id == date::PARSE => {
                        let input = if !args.is_empty() && args[0].is_ptr() {
                            if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let result = match DateObject::parse(&input) {
                            Some(ts) => Value::f64(ts as f64),
                            None => Value::f64(f64::NAN),
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // RegExp native calls
                    id if id == regexp::NEW => {
                        let pattern_arg = native_arg(&args, 0);
                        let flags_arg = native_arg(&args, 1);
                        let pattern = if pattern_arg.is_ptr() {
                            if let Some(s) = unsafe { pattern_arg.as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let flags = if flags_arg.is_ptr() {
                            if let Some(s) = unsafe { flags_arg.as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        match RegExpObject::new(&pattern, &flags) {
                            Ok(re) => {
                                let handle = self.allocate_pinned_handle(re);
                                if let Err(e) = stack.push(Value::u64(handle)) {
                                    return OpcodeResult::Error(e);
                                }
                                OpcodeResult::Continue
                            }
                            Err(e) => OpcodeResult::Error(VmError::RuntimeError(format!(
                                "Invalid regex: {}",
                                e
                            ))),
                        }
                    }
                    id if id == regexp::TEST => {
                        let regexp_arg = native_arg(&args, 0);
                        let input_arg = native_arg(&args, 1);
                        let handle = match self.regexp_handle_from_value(regexp_arg) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let input = if input_arg.is_ptr() {
                            if let Some(s) = unsafe { input_arg.as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid regexp handle".to_string(),
                            ));
                        }
                        let re = unsafe { &*re_ptr };
                        if let Err(e) = stack.push(Value::bool(re.test(&input))) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::EXEC => {
                        let regexp_arg = native_arg(&args, 0);
                        let input_arg = native_arg(&args, 1);
                        let handle = match self.regexp_handle_from_value(regexp_arg) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let input = if input_arg.is_ptr() {
                            if let Some(s) = unsafe { input_arg.as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid regexp handle".to_string(),
                            ));
                        }
                        let re = unsafe { &*re_ptr };
                        match re.exec(&input) {
                            Some((matched, index, groups)) => {
                                let mut arr = Array::new(0, 0);
                                let matched_str = RayaString::new(matched);
                                let gc_ptr = self.gc.lock().allocate(matched_str);
                                let matched_val = unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                    )
                                };
                                arr.push(matched_val);
                                arr.push(Value::i32(index as i32));
                                for group in groups {
                                    let group_str = RayaString::new(group);
                                    let gc_ptr = self.gc.lock().allocate(group_str);
                                    let group_val = unsafe {
                                        Value::from_ptr(
                                            std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                        )
                                    };
                                    arr.push(group_val);
                                }
                                let arr_gc = self.gc.lock().allocate(arr);
                                let arr_val = unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap(),
                                    )
                                };
                                if let Err(e) = stack.push(arr_val) {
                                    return OpcodeResult::Error(e);
                                }
                            }
                            None => {
                                if let Err(e) = stack.push(Value::null()) {
                                    return OpcodeResult::Error(e);
                                }
                            }
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::EXEC_ALL => {
                        let regexp_arg = native_arg(&args, 0);
                        let input_arg = native_arg(&args, 1);
                        let handle = match self.regexp_handle_from_value(regexp_arg) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let input = if input_arg.is_ptr() {
                            if let Some(s) = unsafe { input_arg.as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid regexp handle".to_string(),
                            ));
                        }
                        let re = unsafe { &*re_ptr };
                        let matches = re.exec_all(&input);
                        let mut result_arr = Array::new(0, 0);
                        for (matched, index, groups) in matches {
                            let mut match_arr = Array::new(0, 0);
                            let matched_str = RayaString::new(matched);
                            let gc_ptr = self.gc.lock().allocate(matched_str);
                            let matched_val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            };
                            match_arr.push(matched_val);
                            match_arr.push(Value::i32(index as i32));
                            for group in groups {
                                let group_str = RayaString::new(group);
                                let gc_ptr = self.gc.lock().allocate(group_str);
                                let group_val = unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                    )
                                };
                                match_arr.push(group_val);
                            }
                            let match_arr_gc = self.gc.lock().allocate(match_arr);
                            let match_arr_val = unsafe {
                                Value::from_ptr(
                                    std::ptr::NonNull::new(match_arr_gc.as_ptr()).unwrap(),
                                )
                            };
                            result_arr.push(match_arr_val);
                        }
                        let arr_gc = self.gc.lock().allocate(result_arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::REPLACE => {
                        let regexp_arg = native_arg(&args, 0);
                        let input_arg = native_arg(&args, 1);
                        let replacement_arg = native_arg(&args, 2);
                        let handle = match self.regexp_handle_from_value(regexp_arg) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let input = if input_arg.is_ptr() {
                            if let Some(s) = unsafe { input_arg.as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let replacement = if replacement_arg.is_ptr() {
                            if let Some(s) = unsafe { replacement_arg.as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid regexp handle".to_string(),
                            ));
                        }
                        let re = unsafe { &*re_ptr };
                        let result = re.replace(&input, &replacement);
                        let result_str = RayaString::new(result);
                        let gc_ptr = self.gc.lock().allocate(result_str);
                        let result_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(result_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::SPLIT => {
                        let regexp_arg = native_arg(&args, 0);
                        let input_arg = native_arg(&args, 1);
                        let limit_arg = native_arg(&args, 2);
                        let handle = match self.regexp_handle_from_value(regexp_arg) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let input = if input_arg.is_ptr() {
                            if let Some(s) = unsafe { input_arg.as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let raw_limit = limit_arg
                            .as_i32()
                            .or_else(|| limit_arg.as_i64().map(|v| v as i32))
                            .unwrap_or(0);
                        let limit = if raw_limit > 0 {
                            Some(raw_limit as usize)
                        } else {
                            None
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid regexp handle".to_string(),
                            ));
                        }
                        let re = unsafe { &*re_ptr };
                        let parts = re.split(&input, limit);
                        let mut arr = Array::new(0, 0);
                        for part in parts {
                            let s = RayaString::new(part);
                            let gc_ptr = self.gc.lock().allocate(s);
                            let val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            };
                            arr.push(val);
                        }
                        let arr_gc = self.gc.lock().allocate(arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::REPLACE_MATCHES => {
                        // REGEXP_REPLACE_MATCHES: Get match data for replaceWith intrinsic
                        // Args: regexp handle, input string
                        // Returns: array of [matched_text, start_index] arrays, respecting 'g' flag
                        let handle = match self.regexp_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let input = if args[1].is_ptr() {
                            if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid regexp handle".to_string(),
                            ));
                        }
                        let re = unsafe { &*re_ptr };
                        let is_global = re.flags.contains('g');
                        let mut result_arr = Array::new(0, 0);
                        if is_global {
                            for m in re.compiled.find_iter(&input) {
                                let mut match_arr = Array::new(0, 0);
                                let match_str = RayaString::new(m.as_str().to_string());
                                let gc_ptr = self.gc.lock().allocate(match_str);
                                let match_val = unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                    )
                                };
                                match_arr.push(match_val);
                                match_arr.push(Value::i32(m.start() as i32));
                                let match_arr_gc = self.gc.lock().allocate(match_arr);
                                let match_arr_val = unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(match_arr_gc.as_ptr()).unwrap(),
                                    )
                                };
                                result_arr.push(match_arr_val);
                            }
                        } else if let Some(m) = re.compiled.find(&input) {
                            let mut match_arr = Array::new(0, 0);
                            let match_str = RayaString::new(m.as_str().to_string());
                            let gc_ptr = self.gc.lock().allocate(match_str);
                            let match_val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            };
                            match_arr.push(match_val);
                            match_arr.push(Value::i32(m.start() as i32));
                            let match_arr_gc = self.gc.lock().allocate(match_arr);
                            let match_arr_val = unsafe {
                                Value::from_ptr(
                                    std::ptr::NonNull::new(match_arr_gc.as_ptr()).unwrap(),
                                )
                            };
                            result_arr.push(match_arr_val);
                        }
                        let arr_gc = self.gc.lock().allocate(result_arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // JSON.stringify
                    0x0C00 => {
                        use crate::vm::json;

                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "JSON.stringify requires 1 argument".to_string(),
                            ));
                        }
                        let value = args[0];

                        // Stringify the Value using js_classify() dispatch plus the
                        // runtime property-key registry for dynamic object lanes.
                        match json::stringify::stringify_with_runtime_metadata(
                            value,
                            |key| self.prop_key_name(key),
                            |layout_id| self.structural_layout_names(layout_id),
                        ) {
                            Ok(json_str) => {
                                let result_str = RayaString::new(json_str);
                                let gc_ptr = self.gc.lock().allocate(result_str);
                                let result_val = unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                    )
                                };
                                if let Err(e) = stack.push(result_val) {
                                    return OpcodeResult::Error(e);
                                }
                            }
                            Err(e) => {
                                return OpcodeResult::Error(e);
                            }
                        }
                        OpcodeResult::Continue
                    }

                    // JSON.parse
                    0x0C01 => {
                        use crate::vm::json;

                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "JSON.parse requires 1 argument".to_string(),
                            ));
                        }
                        let json_str = if args[0].is_ptr() {
                            if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "JSON.parse requires a string argument".to_string(),
                                ));
                            }
                        } else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "JSON.parse requires a string argument".to_string(),
                            ));
                        };

                        // Parse JSON directly into the unified Object + dyn_map carrier
                        // used by the interpreter.
                        let result = {
                            let mut gc = self.gc.lock();
                            let mut prop_keys = self.prop_keys.write();
                            match json::parser::parse_with_prop_key_interner(
                                &json_str,
                                &mut gc,
                                &mut |name| prop_keys.intern(name),
                            ) {
                                Ok(v) => v,
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        };

                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    // JSON.merge(dest, source) - copy all properties from source to dest
                    0x0C03 => {
                        if args.len() < 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "JSON.merge requires 2 arguments (dest, source)".to_string(),
                            ));
                        }
                        let dest_val = args[0];
                        let source_val = args[1];

                        // If source is null/non-object, just push dest unchanged
                        if !source_val.is_ptr() {
                            if let Err(e) = stack.push(dest_val) {
                                return OpcodeResult::Error(e);
                            }
                            return OpcodeResult::Continue;
                        }

                        let pairs = self.collect_dynamic_entries(source_val);
                        if !pairs.is_empty() && dest_val.is_ptr() {
                            self.merge_dynamic_entries_into(dest_val, &pairs);
                        }

                        // Push dest back (it's been mutated in place)
                        if let Err(e) = stack.push(dest_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    _ => {
                        // Check if this is a reflect method - pass args directly (don't push/pop)
                        if crate::vm::builtin::is_reflect_method(native_id) {
                            match self.call_reflect_method(task, stack, native_id, args, module) {
                                Ok(()) => return OpcodeResult::Continue,
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        }

                        // Check if this is a runtime method (std:runtime)
                        if crate::vm::builtin::is_runtime_method(native_id) {
                            match self.call_runtime_method(task, stack, native_id, args, module) {
                                Ok(()) => return OpcodeResult::Continue,
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        }

                        // Other native calls not yet implemented
                        OpcodeResult::Error(VmError::RuntimeError(format!(
                            "NativeCall {:#06x} not yet implemented in Interpreter (args={})",
                            native_id,
                            args.len()
                        )))
                    }
                }
            }

            Opcode::ModuleNativeCall => {
                use crate::vm::abi::{native_to_value, value_to_native, EngineContext};
                use raya_sdk::NativeCallResult;

                let local_idx = match Self::read_u16(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u8(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Pop arguments
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    match stack.pop() {
                        Ok(v) => args.push(v),
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }
                args.reverse();

                // Create EngineContext for handler
                let ctx = EngineContext::new(
                    self.gc,
                    self.classes,
                    self.layouts,
                    task.id(),
                    self.class_metadata,
                );

                // Convert arguments to NativeValue (zero-cost)
                let native_args: Vec<raya_sdk::NativeValue> =
                    args.iter().map(|v| value_to_native(*v)).collect();

                // Dispatch via module-local resolved native table.
                let resolved = self.module_resolved_natives(module);
                match resolved.call(local_idx, &ctx, &native_args) {
                    NativeCallResult::Value(val) => {
                        if let Err(e) = stack.push(native_to_value(val)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    NativeCallResult::Suspend(io_request) => {
                        use crate::vm::scheduler::{IoSubmission, SuspendReason};
                        if let Some(tx) = self.io_submit_tx {
                            let _ = tx.send(IoSubmission {
                                task_id: task.id(),
                                request: io_request,
                            });
                        }
                        OpcodeResult::Suspend(SuspendReason::IoWait)
                    }
                    NativeCallResult::Unhandled => OpcodeResult::Error(VmError::RuntimeError(
                        format!("ModuleNativeCall index {} unhandled", local_idx),
                    )),
                    NativeCallResult::Error(msg) => OpcodeResult::Error(VmError::RuntimeError(msg)),
                }
            }

            _ => OpcodeResult::Error(VmError::RuntimeError(format!(
                "Unexpected opcode in exec_native_ops: {:?}",
                opcode
            ))),
        }
    }
}
