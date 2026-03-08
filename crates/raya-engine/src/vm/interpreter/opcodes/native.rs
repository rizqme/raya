//! Native call opcode handlers: NativeCall, ModuleNativeCall
//!
//! NativeCall dispatches to built-in operations (channel, buffer, map, set, date, regexp, etc.)
//! and reflect/runtime methods. ModuleNativeCall dispatches through the resolved natives table.

use crate::compiler::native_id::{
    CHANNEL_CAPACITY, CHANNEL_CLOSE, CHANNEL_IS_CLOSED, CHANNEL_LENGTH, CHANNEL_NEW,
    CHANNEL_RECEIVE, CHANNEL_SEND, CHANNEL_TRY_RECEIVE, CHANNEL_TRY_SEND,
};
use crate::compiler::{Module, Opcode};
use crate::vm::builtin::{buffer, date, map, mutex, regexp, set, url};
use crate::vm::gc::GcHeader;
use crate::vm::interpreter::execution::{OpcodeResult, ReturnAction};
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{
    Array, BoundMethod, BoundNativeMethod, Buffer, ChannelObject, Class, Closure, DateObject,
    LayoutId, MapObject, Object, RayaString, RegExpObject, SetObject, TypeHandle,
};
use crate::vm::scheduler::{Task, TaskId, TaskState};
use crate::vm::stack::Stack;
use crate::vm::sync::MutexId;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

const NODE_DESCRIPTOR_METADATA_KEY: &str = "__node_compat_descriptor";
const IMPORTED_CLASS_TYPE_HANDLE_KEY: &str = "__raya_type_handle__";

fn value_as_string(arg: Value) -> Result<String, VmError> {
    if !arg.is_ptr() {
        return Err(VmError::TypeError("Expected string".to_string()));
    }
    let Some(s) = (unsafe { arg.as_ptr::<RayaString>() }) else {
        return Err(VmError::TypeError("Expected string".to_string()));
    };
    Ok(unsafe { &*s.as_ptr() }.data.clone())
}

fn is_uri_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
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
                return Err(VmError::RuntimeError("Malformed percent-encoding".to_string()));
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

impl<'a> Interpreter<'a> {
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

    fn is_callable_value(value: Value) -> bool {
        if !value.is_ptr() {
            return false;
        }
        let header = unsafe {
            let hp = (value.as_ptr::<u8>().unwrap().as_ptr()).sub(std::mem::size_of::<GcHeader>());
            &*(hp as *const GcHeader)
        };
        header.type_id() == std::any::TypeId::of::<Closure>()
            || header.type_id() == std::any::TypeId::of::<BoundMethod>()
            || header.type_id() == std::any::TypeId::of::<BoundNativeMethod>()
    }

    fn raw_type_handle_id(value: Value) -> Option<crate::vm::object::TypeHandleId> {
        if !value.is_ptr() {
            return None;
        }
        let header = unsafe {
            let hp = (value.as_ptr::<u8>().unwrap().as_ptr()).sub(std::mem::size_of::<GcHeader>());
            &*(hp as *const GcHeader)
        };
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

    fn nominal_type_id_from_imported_class_value(
        &self,
        module: &Module,
        value: Value,
    ) -> Option<usize> {
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

        if let Some(object_ptr) = unsafe { value.as_ptr::<Object>() } {
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
        let obj_ptr = unsafe { obj_val.as_ptr::<Object>() }?;
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
            return Some(index);
        }
        // Backstop for builtin values still emitted as generic object literals.
        Self::legacy_object_literal_field_index(field_name, obj.field_count())
    }

    fn get_field_value_by_name(&self, obj_val: Value, field_name: &str) -> Option<Value> {
        let obj_ptr = unsafe { obj_val.as_ptr::<Object>() }?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        if let Some(index) = self.get_field_index_for_value(obj_val, field_name) {
            if let Some(value) = obj.get_field(index) {
                return Some(value);
            }
        }
        let key = self.intern_prop_key(field_name);
        obj.dyn_map().and_then(|map| map.get(&key).copied())
    }

    fn descriptor_flag(&self, descriptor: Value, field_name: &str, default: bool) -> bool {
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

    fn set_descriptor_metadata(&self, target: Value, key: &str, descriptor: Value) {
        let mut metadata = self.metadata.lock();
        metadata.define_metadata_property(
            NODE_DESCRIPTOR_METADATA_KEY.to_string(),
            descriptor,
            target,
            key.to_string(),
        );
    }

    fn get_descriptor_metadata(&self, target: Value, key: &str) -> Option<Value> {
        let metadata = self.metadata.lock();
        metadata.get_metadata_property(NODE_DESCRIPTOR_METADATA_KEY, target, key)
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
        }

        let getter = self.get_field_value_by_name(descriptor, "get");
        let setter = self.get_field_value_by_name(descriptor, "set");
        let has_accessor =
            getter.is_some_and(|v| !v.is_null()) || setter.is_some_and(|v| !v.is_null());
        let value_field = self.get_field_value_by_name(descriptor, "value");
        let has_value = value_field.is_some_and(|v| !v.is_null());

        if let Some(getter_val) = getter.filter(|v| !v.is_null()) {
            if !Self::is_callable_value(getter_val) {
                return Err(VmError::TypeError(format!(
                    "Getter for property '{}' must be callable",
                    key
                )));
            }
        }
        if let Some(setter_val) = setter.filter(|v| !v.is_null()) {
            if !Self::is_callable_value(setter_val) {
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
        if let Some(value) = value_field.filter(|v| !v.is_null()) {
            let obj_ptr = unsafe { target.as_ptr::<Object>() }
                .ok_or_else(|| VmError::TypeError("Expected object".to_string()))?;
            let obj = unsafe { &mut *obj_ptr.as_ptr() };
            if let Some(field_index) = self.get_field_index_for_value(target, key) {
                obj.set_field(field_index, value)
                    .map_err(VmError::RuntimeError)?;
            } else {
                let prop_key = self.intern_prop_key(key);
                obj.ensure_dyn_map().insert(prop_key, value);
            }
        }

        self.set_descriptor_metadata(target, key, descriptor);
        Ok(())
    }

    fn channel_from_handle_arg(&self, value: Value) -> Result<(u64, &ChannelObject), VmError> {
        let Some(handle) = value.as_u64() else {
            return Err(VmError::TypeError(
                "Expected channel handle (u64)".to_string(),
            ));
        };
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

    fn alloc_buffer_object(&self, handle: u64, len: usize) -> Result<Value, VmError> {
        let (buffer_nominal_type_id, buffer_field_count, buffer_layout_id) =
            self.ensure_buffer_class_layout();
        let mut obj =
            Object::new_nominal(
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
        let (object_nominal_type_id, object_field_count, object_layout_id) =
            self.ensure_object_class_layout();
        let mut obj =
            Object::new_nominal(
                object_layout_id,
                object_nominal_type_id as u32,
                object_field_count,
            );
        if object_field_count > 0 {
            obj.set_field(0, Value::null())
                .map_err(VmError::RuntimeError)?;
        }
        if object_field_count > 1 {
            obj.set_field(1, Value::bool(true))
                .map_err(VmError::RuntimeError)?;
        }
        if object_field_count > 2 {
            obj.set_field(2, Value::bool(true))
                .map_err(VmError::RuntimeError)?;
        }
        if object_field_count > 3 {
            obj.set_field(3, Value::bool(true))
                .map_err(VmError::RuntimeError)?;
        }
        if object_field_count > 4 {
            obj.set_field(4, Value::null())
                .map_err(VmError::RuntimeError)?;
        }
        if object_field_count > 5 {
            obj.set_field(5, Value::null())
                .map_err(VmError::RuntimeError)?;
        }
        let obj_ptr = self.gc.lock().allocate(obj);
        Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) })
    }

    fn synthesize_data_property_descriptor(
        &self,
        target: Value,
        key: &str,
    ) -> Result<Option<Value>, VmError> {
        let Some(obj_ptr) = (unsafe { target.as_ptr::<Object>() }) else {
            return Ok(None);
        };
        let obj = unsafe { &*obj_ptr.as_ptr() };

        let fixed_value = self
            .get_field_index_for_value(target, key)
            .and_then(|index| obj.get_field(index));
        let dynamic_value = obj
            .dyn_map()
            .and_then(|dyn_map| dyn_map.get(&self.intern_prop_key(key)).copied());
        let Some(value) = fixed_value.or(dynamic_value) else {
            return Ok(None);
        };

        let descriptor = self.alloc_object_descriptor()?;
        let Some(descriptor_ptr) = (unsafe { descriptor.as_ptr::<Object>() }) else {
            return Ok(None);
        };
        let descriptor_obj = unsafe { &mut *descriptor_ptr.as_ptr() };

        if let Some(value_index) = self.get_field_index_for_value(descriptor, "value") {
            descriptor_obj
                .set_field(value_index, value)
                .map_err(VmError::RuntimeError)?;
        }
        if let Some(writable_index) = self.get_field_index_for_value(descriptor, "writable") {
            descriptor_obj
                .set_field(writable_index, Value::bool(true))
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
                .set_field(enumerable_index, Value::bool(true))
                .map_err(VmError::RuntimeError)?;
        }

        Ok(Some(descriptor))
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

                // Pop arguments
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    match stack.pop() {
                        Ok(v) => args.push(v),
                        Err(e) => return OpcodeResult::Error(e),
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

                    id if id == crate::compiler::native_id::OBJECT_CONSTRUCT_DYNAMIC_CLASS => {
                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "dynamic class construction requires type handle as first argument"
                                    .to_string(),
                            ));
                        }

                        let Some(nominal_type_id) =
                            self.nominal_type_id_from_imported_class_value(module, args[0])
                        else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "dynamic class construction expects imported class value with type handle"
                                    .to_string(),
                            ));
                        };

                        let classes = self.classes.read();
                        let class = match classes.get_class(nominal_type_id) {
                            Some(class) => class,
                            None => {
                                return OpcodeResult::Error(VmError::RuntimeError(format!(
                                    "Invalid nominal type id for dynamic construction: {}",
                                    nominal_type_id
                                )))
                            }
                        };
                        let constructor_id = class.get_constructor();
                        let constructor_module = class.module.clone();
                        let (layout_id, field_count) = match self.nominal_allocation(nominal_type_id)
                        {
                            Some(allocation) => allocation,
                            None => {
                                return OpcodeResult::Error(VmError::RuntimeError(format!(
                                "Invalid nominal type allocation metadata for dynamic construction: {}",
                                nominal_type_id
                            )))
                            }
                        };
                        drop(classes);

                        let obj =
                            Object::new_nominal(layout_id, nominal_type_id as u32, field_count);
                        let gc_ptr = self.gc.lock().allocate(obj);
                        let obj_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };

                        if let Some(constructor_id) = constructor_id {
                            if let Err(error) = stack.push(obj_val) {
                                return OpcodeResult::Error(error);
                            }
                            for arg in args.iter().skip(1).copied() {
                                if let Err(error) = stack.push(arg) {
                                    return OpcodeResult::Error(error);
                                }
                            }
                            return OpcodeResult::PushFrame {
                                func_id: constructor_id,
                                arg_count: args.len(),
                                is_closure: false,
                                closure_val: None,
                                module: constructor_module,
                                return_action: ReturnAction::PushObject(obj_val),
                            };
                        }

                        if let Err(error) = stack.push(obj_val) {
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

                        let Some(nominal_type_id) =
                            self.nominal_type_id_from_imported_class_value(module, args[1])
                        else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "dynamic instanceof expects imported or ambient class value"
                                    .to_string(),
                            ));
                        };

                        let classes = self.classes.read();
                        let result =
                            crate::vm::reflect::is_instance_of(&classes, args[0], nominal_type_id);
                        if let Err(error) = stack.push(Value::bool(result)) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_NEW => {
                        // Create a new channel with given capacity
                        let capacity = args[0].as_i32().unwrap_or(0) as usize;
                        let ch = ChannelObject::new(capacity);
                        let gc_ptr = self.gc.lock().allocate(ch);
                        let handle = gc_ptr.as_ptr() as u64;
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
                            if let Err(e) = stack.push(val) {
                                return OpcodeResult::Error(e);
                            }
                            OpcodeResult::Continue
                        } else if channel.is_closed() {
                            if let Err(e) = stack.push(Value::null()) {
                                return OpcodeResult::Error(e);
                            }
                            OpcodeResult::Continue
                        } else {
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
                        if let Err(e) = stack.push(Value::bool(channel.is_closed())) {
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
                        let gc_ptr = self.gc.lock().allocate(buf);
                        let handle = gc_ptr.as_ptr() as u64;
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
                        let new_handle = {
                            let gc_ptr = self.gc.lock().allocate(sliced);
                            gc_ptr.as_ptr() as u64
                        };

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
                        let gc_ptr = self.gc.lock().allocate(buf);
                        let new_handle = gc_ptr.as_ptr() as u64;
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
                        let gc_ptr = self.gc.lock().allocate(map);
                        let handle = gc_ptr.as_ptr() as u64;
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
                        let gc_ptr = self.gc.lock().allocate(set_obj);
                        let handle = gc_ptr.as_ptr() as u64;
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
                        let gc_ptr = self.gc.lock().allocate(result);
                        let handle = gc_ptr.as_ptr() as u64;
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
                        let gc_ptr = self.gc.lock().allocate(result);
                        let handle = gc_ptr.as_ptr() as u64;
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
                        let gc_ptr = self.gc.lock().allocate(result);
                        let handle = gc_ptr.as_ptr() as u64;
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
                        // OBJECT_TO_STRING: return "[object Object]"
                        let s = RayaString::new("[object Object]".to_string());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
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
                        let key = if let Some(ptr) = unsafe { key_val.as_ptr::<RayaString>() } {
                            unsafe { &*ptr.as_ptr() }.data.clone()
                        } else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.defineProperty key must be a string".to_string(),
                            ));
                        };

                        if let Err(e) = self.apply_descriptor_to_target(target, &key, descriptor) {
                            return OpcodeResult::Error(e);
                        }
                        if let Err(e) = stack.push(target) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0005u16 => {
                        // OBJECT_GET_OWN_PROPERTY_DESCRIPTOR(target, key) -> descriptor | null
                        if args.len() < 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.getOwnPropertyDescriptor requires 2 arguments".to_string(),
                            ));
                        }
                        let target = args[0];
                        let key_val = args[1];
                        if !target.is_ptr() {
                            if let Err(e) = stack.push(Value::null()) {
                                return OpcodeResult::Error(e);
                            }
                            return OpcodeResult::Continue;
                        }
                        let key = if let Some(ptr) = unsafe { key_val.as_ptr::<RayaString>() } {
                            unsafe { &*ptr.as_ptr() }.data.clone()
                        } else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.getOwnPropertyDescriptor key must be a string".to_string(),
                            ));
                        };
                        let value = match self.get_descriptor_metadata(target, &key) {
                            Some(descriptor) => descriptor,
                            None => match self.synthesize_data_property_descriptor(target, &key) {
                                Ok(Some(descriptor)) => descriptor,
                                Ok(None) => Value::null(),
                                Err(error) => return OpcodeResult::Error(error),
                            },
                        };
                        if let Err(e) = stack.push(value) {
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
                                    if let Err(e) = self.apply_descriptor_to_target(
                                        target,
                                        &field_name,
                                        descriptor_val,
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
                                self.get_object_named_field_value(obj, "stack").unwrap_or_else(
                                    || {
                                        let s = RayaString::new(String::new());
                                        let gc_ptr = self.gc.lock().allocate(s);
                                        unsafe {
                                            Value::from_ptr(
                                                std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                            )
                                        }
                                    },
                                )
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
                        let pattern = if args[0].is_ptr() {
                            if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let flags = if args.len() > 1 && args[1].is_ptr() {
                            if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        match RegExpObject::new(&pattern, &flags) {
                            Ok(re) => {
                                let gc_ptr = self.gc.lock().allocate(re);
                                let handle = gc_ptr.as_ptr() as u64;
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
                        if let Err(e) = stack.push(Value::bool(re.test(&input))) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::EXEC => {
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
                        let replacement = if args[2].is_ptr() {
                            if let Some(s) = unsafe { args[2].as_ptr::<RayaString>() } {
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
                        let limit = if args.len() > 2 {
                            let raw_limit = args[2]
                                .as_i32()
                                .or_else(|| args[2].as_i64().map(|v| v as i32))
                                .unwrap_or(0);
                            if raw_limit > 0 {
                                Some(raw_limit as usize)
                            } else {
                                None
                            }
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
