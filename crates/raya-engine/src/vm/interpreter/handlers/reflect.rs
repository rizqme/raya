//! Reflect built-in method handlers and helpers

use crate::compiler::Module;
use crate::vm::gc::header_ptr_from_value_ptr;
use crate::vm::interpreter::core::value_to_f64;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{
    layout_id_from_ordered_names, Array, BoundMethod, BoundNativeMethod, Closure, Object, Proxy,
    RayaString,
};
use crate::vm::reflect::{ObjectDiff, ObjectSnapshot, SnapshotContext, SnapshotValue};
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::ptr::NonNull;
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    fn reflect_array_index(property_key: &str) -> Option<usize> {
        if property_key.is_empty() {
            return None;
        }
        if property_key != "0" && property_key.starts_with('0') {
            return None;
        }
        let index = property_key.parse::<u32>().ok()?;
        if index == u32::MAX {
            return None;
        }
        if index.to_string() != property_key {
            return None;
        }
        Some(index as usize)
    }

    fn reflect_object_ptr(value: Value) -> Option<NonNull<Object>> {
        if !value.is_ptr() {
            return None;
        }
        let raw_ptr = unsafe { value.as_ptr::<u8>() }?;
        let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };
        if header.type_id() != std::any::TypeId::of::<Object>() {
            return None;
        }
        unsafe { value.as_ptr::<Object>() }
    }

    fn reflect_object_field_names(&self, obj: &Object) -> Vec<String> {
        let mut field_names = if let Some(nominal_type_id) = obj.nominal_type_id_usize() {
            let class_metadata = self.class_metadata.read();
            class_metadata
                .get(nominal_type_id)
                .map(|meta| {
                    meta.field_names
                        .iter()
                        .enumerate()
                        .map(|(index, name)| {
                            if name.is_empty() {
                                format!("field_{}", index)
                            } else {
                                name.clone()
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .filter(|names| !names.is_empty())
                .unwrap_or_else(|| {
                    self.layout_field_names_for_object(obj).unwrap_or_else(|| {
                        (0..obj.field_count())
                            .map(|index| format!("field_{}", index))
                            .collect::<Vec<_>>()
                    })
                })
        } else {
            self.layout_field_names_for_object(obj).unwrap_or_else(|| {
                (0..obj.field_count())
                    .map(|index| format!("field_{}", index))
                    .collect::<Vec<_>>()
            })
        };

        if let Some(dyn_map) = obj.dyn_map() {
            for key in dyn_map.keys() {
                let Some(name) = self.prop_key_name(*key) else {
                    continue;
                };
                if !field_names.iter().any(|existing| existing == &name) {
                    field_names.push(name);
                }
            }
        }

        if let Some(global_obj) = self.builtin_global_value("globalThis") {
            let obj_value = unsafe {
                Value::from_ptr(
                    NonNull::new(obj as *const Object as *mut Object).expect("global object ptr"),
                )
            };
            if global_obj.raw() == obj_value.raw() {
                for name in self.builtin_global_slots.read().keys() {
                    if self.fixed_property_deleted(obj_value, name) {
                        continue;
                    }
                    if !field_names.iter().any(|existing| existing == name) {
                        field_names.push(name.clone());
                    }
                }
            }
        }

        field_names
    }

    fn reflect_object_class_name(&self, obj: &Object) -> String {
        if let Some(nominal_type_id) = obj.nominal_type_id_usize() {
            return self
                .classes
                .read()
                .get_class(nominal_type_id)
                .map(|class| class.name.clone())
                .unwrap_or_else(|| format!("Class{}", nominal_type_id));
        }
        format!("Layout{}", obj.layout_id())
    }

    fn reflect_property_value(&self, target: Value, property_key: &str) -> Option<Value> {
        if let Some(value) = self.builtin_global_property_value(target, property_key) {
            return Some(value);
        }
        let callable_like = self.reflect_is_callable_value(target);
        if let Some(value) = self.get_field_value_by_name(target, property_key) {
            return Some(value);
        }
        if Self::reflect_object_ptr(target).is_some() {
            if !callable_like {
                return None;
            }
        }
        if !callable_like {
            return None;
        }
        self.descriptor_data_value(target, property_key)
            .or_else(|| self.callable_property_value(target, property_key))
    }

    fn reflect_set_property_value(
        &mut self,
        target: Value,
        property_key: &str,
        value: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<bool, VmError> {
        if std::env::var("RAYA_DEBUG_REFLECT_SET").is_ok() {
            eprintln!(
                "[reflect.set] target={:#x} key={} value={:#x}",
                target.raw(),
                property_key,
                value.raw()
            );
        }
        self.set_property_value_via_js_semantics(target, property_key, value, target, task, module)
    }

    fn reflect_method_slot_for_object(&self, obj: &Object, property_key: &str) -> Option<usize> {
        let nominal_type_id = obj.nominal_type_id_usize()?;
        let class_metadata = self.class_metadata.read();
        class_metadata
            .get(nominal_type_id)
            .and_then(|meta| meta.get_method_index(property_key))
    }

    fn reflect_is_callable_value(&self, value: Value) -> bool {
        if self.callable_function_info(value).is_some() {
            return true;
        }
        if !value.is_ptr() {
            return false;
        }
        let header = unsafe { &*header_ptr_from_value_ptr(value.as_ptr::<u8>().unwrap().as_ptr()) };
        header.type_id() == std::any::TypeId::of::<Closure>()
            || header.type_id() == std::any::TypeId::of::<BoundMethod>()
            || header.type_id() == std::any::TypeId::of::<BoundNativeMethod>()
    }

    fn reflect_has_property(&self, target: Value, property_key: &str) -> bool {
        let callable_like = self.reflect_is_callable_value(target);
        if let Some(obj_ptr) = Self::reflect_object_ptr(target) {
            let obj = unsafe { obj_ptr.as_ref() };
            if self.get_field_value_by_name(target, property_key).is_some() {
                return true;
            }
            let key = self.intern_prop_key(property_key);
            if obj.dyn_map().is_some_and(|map| map.contains_key(&key)) {
                return true;
            }
            if self
                .reflect_method_slot_for_object(obj, property_key)
                .is_some()
            {
                return true;
            }
            if !callable_like {
                return false;
            }
        }
        if !callable_like {
            return false;
        }
        self.descriptor_data_value(target, property_key).is_some()
            || self.callable_property_value(target, property_key).is_some()
    }

    fn reflect_alloc_string_value(&self, value: impl Into<String>) -> Value {
        let gc_ptr = self.gc.lock().allocate(RayaString::new(value.into()));
        unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
    }

    fn reflect_alloc_structural_object(&self, entries: &[(&str, Value)]) -> Value {
        let ordered_names = entries
            .iter()
            .map(|(name, _)| (*name).to_string())
            .collect::<Vec<_>>();
        let layout_id = layout_id_from_ordered_names(&ordered_names);
        self.register_structural_layout_shape(layout_id, &ordered_names);
        let mut obj = Object::new_structural(layout_id, entries.len());
        for (index, (_, value)) in entries.iter().enumerate() {
            let _ = obj.set_field(index, *value);
        }
        let gc_ptr = self.gc.lock().allocate(obj);
        unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
    }

    fn reflect_nominal_type_id_from_value(&self, value: Value) -> Option<usize> {
        if let Some(id) = value.as_i32() {
            return (id >= 0).then_some(id as usize);
        }
        let obj_ptr = Self::reflect_object_ptr(value)?;
        let obj = unsafe { obj_ptr.as_ref() };
        let id_value = self.get_object_named_field_value(obj, "nominalTypeId")?;
        let id = value_to_f64(id_value).ok()? as isize;
        (id >= 0).then_some(id as usize)
    }

    fn reflect_require_nominal_type_id(
        &self,
        value: Value,
        context: &str,
    ) -> Result<usize, VmError> {
        self.reflect_nominal_type_id_from_value(value)
            .ok_or_else(|| {
                VmError::TypeError(format!(
                    "{context}: expected NominalTypeRef or nominal type id"
                ))
            })
    }

    fn reflect_alloc_nominal_type_ref(&self, nominal_type_id: usize) -> Value {
        let class_name = {
            let classes = self.classes.read();
            let Some(class) = classes.get_class(nominal_type_id) else {
                return Value::null();
            };
            class.name.clone()
        };
        let Some(layout_id) = self.nominal_layout_id(nominal_type_id) else {
            return Value::null();
        };

        self.reflect_alloc_structural_object(&[
            ("nominalTypeId", Value::i32(nominal_type_id as i32)),
            ("name", self.reflect_alloc_string_value(class_name)),
            ("layoutId", Value::i32(layout_id as i32)),
        ])
    }

    fn reflect_alloc_nominal_type_ref_array(
        &self,
        nominal_type_ids: impl IntoIterator<Item = usize>,
    ) -> Value {
        let refs = nominal_type_ids
            .into_iter()
            .map(|nominal_type_id| self.reflect_alloc_nominal_type_ref(nominal_type_id))
            .filter(|value| !value.is_null())
            .collect::<Vec<_>>();
        let mut arr = Array::new(0, refs.len());
        for (index, value) in refs.into_iter().enumerate() {
            arr.set(index, value).ok();
        }
        let gc_ptr = self.gc.lock().allocate(arr);
        unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
    }

    fn reflect_alloc_field_info_map(
        &self,
        name: &str,
        type_name: &str,
        field_index: Option<usize>,
        is_static: bool,
        is_readonly: bool,
        declaring_nominal_type_id: Option<usize>,
    ) -> Value {
        self.reflect_alloc_structural_object(&[
            ("name", self.reflect_alloc_string_value(name)),
            ("type", self.reflect_alloc_string_value(type_name)),
            (
                "index",
                field_index
                    .map(|index| Value::i32(index as i32))
                    .unwrap_or(Value::null()),
            ),
            ("isStatic", Value::bool(is_static)),
            ("isReadonly", Value::bool(is_readonly)),
            (
                "declaringType",
                declaring_nominal_type_id
                    .map(|id| self.reflect_alloc_nominal_type_ref(id))
                    .unwrap_or(Value::null()),
            ),
        ])
    }

    fn reflect_object_snapshot_descriptor(&self, value: Value) -> (String, Vec<String>) {
        let Some(ptr) = Self::reflect_object_ptr(value) else {
            return ("unknown".to_string(), Vec::new());
        };
        let obj = unsafe { ptr.as_ref() };
        (
            self.reflect_object_class_name(obj),
            self.reflect_object_field_names(obj),
        )
    }

    /// Handle built-in Reflect methods
    pub(in crate::vm::interpreter) fn call_reflect_method(
        &mut self,
        task: &Arc<Task>,
        stack: &mut Stack,
        method_id: u16,
        args: Vec<Value>,
        module: &Module,
    ) -> Result<(), VmError> {
        use crate::vm::builtin::reflect;

        // Helper to get string from Value
        let get_string = |v: Value| -> Result<String, VmError> {
            if !v.is_ptr() {
                return Err(VmError::TypeError("Expected string".to_string()));
            }
            let s_ptr = unsafe { v.as_ptr::<RayaString>() };
            let s = unsafe { &*s_ptr.unwrap().as_ptr() };
            Ok(s.data.clone())
        };
        let mut get_property_key = |v: Value, op_name: &str| -> Result<String, VmError> {
            let (key, _) = self.property_key_parts_with_context(v, op_name, task, module)?;
            key.ok_or_else(|| VmError::TypeError(format!("{op_name}: expected property key")))
        };

        let result = match method_id {
            reflect::DEFINE_METADATA => {
                // defineMetadata(key, value, target)
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "defineMetadata requires 3 arguments".to_string(),
                    ));
                }
                let key = get_string(args[0])?;
                let value = args[1];
                let target = args[2];

                let mut metadata = self.metadata.lock();
                metadata.define_metadata(key, value, target);
                Value::null()
            }

            reflect::DEFINE_METADATA_PROP => {
                // defineMetadata(key, value, target, propertyKey)
                if args.len() < 4 {
                    return Err(VmError::RuntimeError(
                        "defineMetadata with property requires 4 arguments".to_string(),
                    ));
                }
                let key = get_string(args[0])?;
                let value = args[1];
                let target = args[2];
                let property_key = get_string(args[3])?;

                let mut metadata = self.metadata.lock();
                metadata.define_metadata_property(key, value, target, property_key);
                Value::null()
            }

            reflect::GET_METADATA => {
                // getMetadata(key, target)
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "getMetadata requires 2 arguments".to_string(),
                    ));
                }
                let key = get_string(args[0])?;
                let target = args[1];

                let metadata = self.metadata.lock();
                metadata.get_metadata(&key, target).unwrap_or_default()
            }

            reflect::GET_METADATA_PROP => {
                // getMetadata(key, target, propertyKey)
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "getMetadata with property requires 3 arguments".to_string(),
                    ));
                }
                let key = get_string(args[0])?;
                let target = args[1];
                let property_key = get_string(args[2])?;

                let metadata = self.metadata.lock();
                metadata
                    .get_metadata_property(&key, target, &property_key)
                    .unwrap_or_default()
            }

            reflect::HAS_METADATA => {
                // hasMetadata(key, target)
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "hasMetadata requires 2 arguments".to_string(),
                    ));
                }
                let key = get_string(args[0])?;
                let target = args[1];

                let metadata = self.metadata.lock();
                Value::bool(metadata.has_metadata(&key, target))
            }

            reflect::HAS_METADATA_PROP => {
                // hasMetadata(key, target, propertyKey)
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "hasMetadata with property requires 3 arguments".to_string(),
                    ));
                }
                let key = get_string(args[0])?;
                let target = args[1];
                let property_key = get_string(args[2])?;

                let metadata = self.metadata.lock();
                Value::bool(metadata.has_metadata_property(&key, target, &property_key))
            }

            reflect::GET_METADATA_KEYS => {
                // getMetadataKeys(target)
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getMetadataKeys requires 1 argument".to_string(),
                    ));
                }
                let target = args[0];

                let metadata = self.metadata.lock();
                let keys = metadata.get_metadata_keys(target);

                // Create an array of string keys
                let mut arr = Array::new(0, keys.len());
                for (i, key) in keys.into_iter().enumerate() {
                    let s = RayaString::new(key);
                    let gc_ptr = self.gc.lock().allocate(s);
                    let val = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                    };
                    arr.set(i, val).ok();
                }
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            reflect::GET_METADATA_KEYS_PROP => {
                // getMetadataKeys(target, propertyKey)
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "getMetadataKeys with property requires 2 arguments".to_string(),
                    ));
                }
                let target = args[0];
                let property_key = get_property_key(args[1], "Reflect.get")?;

                let metadata = self.metadata.lock();
                let keys = metadata.get_metadata_keys_property(target, &property_key);

                // Create an array of string keys
                let mut arr = Array::new(0, keys.len());
                for (i, key) in keys.into_iter().enumerate() {
                    let s = RayaString::new(key);
                    let gc_ptr = self.gc.lock().allocate(s);
                    let val = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                    };
                    arr.set(i, val).ok();
                }
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            reflect::DELETE_METADATA => {
                // deleteMetadata(key, target)
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "deleteMetadata requires 2 arguments".to_string(),
                    ));
                }
                let key = get_string(args[0])?;
                let target = args[1];

                let mut metadata = self.metadata.lock();
                Value::bool(metadata.delete_metadata(&key, target))
            }

            reflect::DELETE_METADATA_PROP => {
                // deleteMetadata(key, target, propertyKey)
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "deleteMetadata with property requires 3 arguments".to_string(),
                    ));
                }
                let key = get_string(args[0])?;
                let target = args[1];
                let property_key = get_string(args[2])?;

                let mut metadata = self.metadata.lock();
                Value::bool(metadata.delete_metadata_property(&key, target, &property_key))
            }

            // ===== Phase 2: Class Introspection =====
            reflect::GET_CLASS => {
                // getClass(obj) -> returns nominal type ref, or null
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getClass requires 1 argument".to_string(),
                    ));
                }
                let obj = args[0];
                if let Some(nominal_type_id) = crate::vm::reflect::get_nominal_type_id(obj) {
                    self.reflect_alloc_nominal_type_ref(nominal_type_id)
                } else {
                    Value::null()
                }
            }

            reflect::GET_CLASS_BY_NAME => {
                // getClassByName(name) -> returns nominal type ref, or null if not found
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getClassByName requires 1 argument".to_string(),
                    ));
                }
                let name = get_string(args[0])?;
                let classes = self.classes.read();
                if let Some(class) = classes.get_class_by_name(&name) {
                    let nominal_type_id = class.id;
                    drop(classes);
                    self.reflect_alloc_nominal_type_ref(nominal_type_id)
                } else {
                    Value::null()
                }
            }

            reflect::GET_ALL_CLASSES => {
                // getAllClasses() -> returns array of nominal type refs
                let classes = self.classes.read();
                let class_ids = classes.iter().map(|(id, _)| id).collect::<Vec<_>>();
                drop(classes);
                self.reflect_alloc_nominal_type_ref_array(class_ids)
            }

            reflect::GET_CLASSES_WITH_DECORATOR => {
                // getClassesWithDecorator(decorator) -> returns array of nominal type refs
                // NOTE: This requires --emit-reflection to work fully
                // For now, returns empty array (decorator metadata not yet stored)
                let arr = Array::new(0, 0);
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            reflect::IS_SUBCLASS_OF => {
                // isSubclassOf(subTypeRef, superTypeRef) -> boolean
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "isSubclassOf requires 2 arguments".to_string(),
                    ));
                }
                match (
                    self.reflect_nominal_type_id_from_value(args[0]),
                    self.reflect_nominal_type_id_from_value(args[1]),
                ) {
                    (Some(sub_id), Some(super_id)) if sub_id != 0 && super_id != 0 => {
                        let classes = self.classes.read();
                        Value::bool(crate::vm::reflect::is_subclass_of(
                            &classes, sub_id, super_id,
                        ))
                    }
                    _ => Value::bool(false),
                }
            }

            reflect::IS_INSTANCE_OF => {
                // isInstanceOf(obj, typeRef) -> boolean
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "isInstanceOf requires 2 arguments".to_string(),
                    ));
                }
                let obj = args[0];
                match self.reflect_nominal_type_id_from_value(args[1]) {
                    Some(nominal_type_id) => {
                        let classes = self.classes.read();
                        Value::bool(crate::vm::reflect::is_instance_of(
                            &classes,
                            obj,
                            nominal_type_id,
                        ))
                    }
                    None => Value::bool(false),
                }
            }

            reflect::GET_TYPE_INFO => {
                // getTypeInfo(target) -> returns type kind as string
                // NOTE: Full TypeInfo requires --emit-reflection
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getTypeInfo requires 1 argument".to_string(),
                    ));
                }
                let target = args[0];
                let type_info = crate::vm::reflect::get_type_info_for_value(target);

                // Return the type name as a string for now
                let s = RayaString::new(type_info.name);
                let gc_ptr = self.gc.lock().allocate(s);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
            }

            reflect::GET_CLASS_HIERARCHY => {
                // getClassHierarchy(obj) -> returns nominal type refs to the root
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getClassHierarchy requires 1 argument".to_string(),
                    ));
                }
                let obj = args[0];

                if let Some(nominal_type_id) = crate::vm::reflect::get_nominal_type_id(obj) {
                    let classes = self.classes.read();
                    let hierarchy =
                        crate::vm::reflect::get_class_hierarchy(&classes, nominal_type_id);
                    let class_ids = hierarchy.iter().map(|c| c.id).collect::<Vec<_>>();
                    drop(classes);
                    self.reflect_alloc_nominal_type_ref_array(class_ids)
                } else {
                    // Not an object, return empty array
                    let arr = Array::new(0, 0);
                    let arr_gc = self.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                }
            }

            // ===== Phase 3: Field Access =====
            reflect::GET => {
                // get(target, propertyKey) -> get field value by name
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "get requires 2 arguments (target, propertyKey)".to_string(),
                    ));
                }
                let target = args[0];
                let property_key = get_property_key(args[1], "Reflect.set")?;

                if !target.is_ptr() {
                    return Err(VmError::TypeError(
                        "get: target must be an object".to_string(),
                    ));
                }

                if let Some(value) = self.reflect_property_value(target, &property_key) {
                    value
                } else {
                    let obj_ptr = Self::reflect_object_ptr(target).ok_or_else(|| {
                        VmError::TypeError("get: target must be an object".to_string())
                    })?;
                    let obj = unsafe { obj_ptr.as_ref() };
                    if let Some(slot) = self.reflect_method_slot_for_object(obj, &property_key) {
                        let Some(runtime_nominal_type_id) = obj.nominal_type_id_usize() else {
                            return Err(VmError::RuntimeError(
                                "Method fallback requires nominal runtime type".to_string(),
                            ));
                        };
                        let classes = self.classes.read();
                        let class = match classes.get_class(runtime_nominal_type_id) {
                            Some(c) => c,
                            None => {
                                return Err(VmError::RuntimeError(format!(
                                    "Invalid nominal type id: {}",
                                    runtime_nominal_type_id
                                )));
                            }
                        };
                        let func_id = match class.vtable.get_method(slot) {
                            Some(fid) => fid,
                            None => {
                                return Err(VmError::RuntimeError(format!(
                                    "Invalid method slot: {} for class {}",
                                    slot, class.name
                                )));
                            }
                        };
                        let method_module = class.module.clone();
                        drop(classes);

                        let bm = BoundMethod {
                            receiver: target,
                            func_id,
                            module: method_module,
                        };
                        let gc_ptr = self.gc.lock().allocate(bm);
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
                    } else {
                        Value::null()
                    }
                }
            }

            reflect::SET => {
                // set(target, propertyKey, value) -> set field value by name
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "set requires 3 arguments (target, propertyKey, value)".to_string(),
                    ));
                }
                let target = args[0];
                let property_key = get_property_key(args[1], "Reflect.has")?;
                let value = args[2];

                if !target.is_ptr() {
                    return Err(VmError::TypeError(
                        "set: target must be an object".to_string(),
                    ));
                }

                Value::bool(self.reflect_set_property_value(
                    target,
                    &property_key,
                    value,
                    task,
                    module,
                )?)
            }

            reflect::HAS => {
                // has(target, propertyKey) -> check if field exists
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "has requires 2 arguments (target, propertyKey)".to_string(),
                    ));
                }
                let target = args[0];
                let property_key = get_property_key(args[1], "Reflect.getFieldInfo")?;

                Value::bool(self.reflect_has_property(target, &property_key))
            }

            reflect::GET_FIELD_NAMES => {
                // getFieldNames(target) -> list all field names
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getFieldNames requires 1 argument".to_string(),
                    ));
                }
                let target = args[0];

                let field_names = Self::reflect_object_ptr(target)
                    .map(|ptr| {
                        let obj = unsafe { ptr.as_ref() };
                        self.reflect_object_field_names(obj)
                    })
                    .unwrap_or_default();

                // Create array of strings
                let mut arr = Array::new(0, field_names.len());
                for (i, name) in field_names.into_iter().enumerate() {
                    if !name.is_empty() {
                        let s = RayaString::new(name);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        arr.set(i, val).ok();
                    }
                }
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            reflect::GET_FIELD_INFO => {
                // getFieldInfo(target, propertyKey) -> get field metadata as Map
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "getFieldInfo requires 2 arguments (target, propertyKey)".to_string(),
                    ));
                }
                let target = args[0];
                let property_key = get_string(args[1])?;

                if let Some(obj_ptr) = Self::reflect_object_ptr(target) {
                    let obj = unsafe { obj_ptr.as_ref() };

                    if let Some(nominal_type_id) = obj.nominal_type_id_usize() {
                        let class_metadata = self.class_metadata.read();
                        if let Some(field_info) = class_metadata
                            .get(nominal_type_id)
                            .and_then(|meta| meta.get_field_info(&property_key))
                        {
                            self.reflect_alloc_field_info_map(
                                &field_info.name,
                                &field_info.type_info.name,
                                Some(field_info.field_index),
                                field_info.is_static,
                                field_info.is_readonly,
                                Some(field_info.declaring_nominal_type_id),
                            )
                        } else if let Some(value) =
                            self.reflect_property_value(target, &property_key)
                        {
                            self.reflect_alloc_field_info_map(
                                &property_key,
                                &crate::vm::reflect::get_type_info_for_value(value).name,
                                self.get_field_index_for_value(target, &property_key),
                                false,
                                false,
                                Some(nominal_type_id),
                            )
                        } else {
                            Value::null()
                        }
                    } else if let Some(value) = self.reflect_property_value(target, &property_key) {
                        self.reflect_alloc_field_info_map(
                            &property_key,
                            &crate::vm::reflect::get_type_info_for_value(value).name,
                            self.get_field_index_for_value(target, &property_key),
                            false,
                            false,
                            obj.nominal_type_id_usize(),
                        )
                    } else {
                        Value::null()
                    }
                } else {
                    Value::null()
                }
            }

            reflect::GET_FIELDS => {
                // getFields(target) -> get all field infos as array of Maps
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getFields requires 1 argument (target)".to_string(),
                    ));
                }
                let target = args[0];

                if let Some(obj_ptr) = Self::reflect_object_ptr(target) {
                    let obj = unsafe { obj_ptr.as_ref() };

                    if let Some(nominal_type_id) = obj.nominal_type_id_usize() {
                        let class_metadata = self.class_metadata.read();
                        if let Some(meta) = class_metadata.get(nominal_type_id) {
                            let fields = meta.get_all_field_infos();
                            if !fields.is_empty() {
                                let mut arr = Array::new(0, fields.len());
                                for (i, field_info) in fields.iter().enumerate() {
                                    let map_val = self.reflect_alloc_field_info_map(
                                        &field_info.name,
                                        &field_info.type_info.name,
                                        Some(field_info.field_index),
                                        field_info.is_static,
                                        field_info.is_readonly,
                                        Some(field_info.declaring_nominal_type_id),
                                    );
                                    arr.set(i, map_val).ok();
                                }

                                let arr_gc = self.gc.lock().allocate(arr);
                                unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap(),
                                    )
                                }
                            } else {
                                let field_values = self
                                    .reflect_object_field_names(obj)
                                    .into_iter()
                                    .filter_map(|name| {
                                        self.reflect_property_value(target, &name).map(|value| {
                                            self.reflect_alloc_field_info_map(
                                                &name,
                                                &crate::vm::reflect::get_type_info_for_value(value)
                                                    .name,
                                                self.get_field_index_for_value(target, &name),
                                                false,
                                                false,
                                                Some(nominal_type_id),
                                            )
                                        })
                                    })
                                    .collect::<Vec<_>>();
                                let mut arr = Array::new(0, field_values.len());
                                for (i, value) in field_values.into_iter().enumerate() {
                                    arr.set(i, value).ok();
                                }
                                let arr_gc = self.gc.lock().allocate(arr);
                                unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap(),
                                    )
                                }
                            }
                        } else {
                            let field_values = self
                                .reflect_object_field_names(obj)
                                .into_iter()
                                .filter_map(|name| {
                                    self.reflect_property_value(target, &name).map(|value| {
                                        self.reflect_alloc_field_info_map(
                                            &name,
                                            &crate::vm::reflect::get_type_info_for_value(value)
                                                .name,
                                            self.get_field_index_for_value(target, &name),
                                            false,
                                            false,
                                            Some(nominal_type_id),
                                        )
                                    })
                                })
                                .collect::<Vec<_>>();
                            let mut arr = Array::new(0, field_values.len());
                            for (i, value) in field_values.into_iter().enumerate() {
                                arr.set(i, value).ok();
                            }
                            let arr_gc = self.gc.lock().allocate(arr);
                            unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                            }
                        }
                    } else {
                        let field_values = self
                            .reflect_object_field_names(obj)
                            .into_iter()
                            .filter_map(|name| {
                                self.reflect_property_value(target, &name).map(|value| {
                                    self.reflect_alloc_field_info_map(
                                        &name,
                                        &crate::vm::reflect::get_type_info_for_value(value).name,
                                        self.get_field_index_for_value(target, &name),
                                        false,
                                        false,
                                        obj.nominal_type_id_usize(),
                                    )
                                })
                            })
                            .collect::<Vec<_>>();
                        let mut arr = Array::new(0, field_values.len());
                        for (i, value) in field_values.into_iter().enumerate() {
                            arr.set(i, value).ok();
                        }
                        let arr_gc = self.gc.lock().allocate(arr);
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                    }
                } else {
                    let arr = Array::new(0, 0);
                    let arr_gc = self.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                }
            }

            reflect::GET_STATIC_FIELD_NAMES => {
                // getStaticFieldNames(typeRef) -> get static field names as array
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getStaticFieldNames requires 1 argument (typeRef)".to_string(),
                    ));
                }
                let nominal_type_id =
                    self.reflect_require_nominal_type_id(args[0], "getStaticFieldNames")?;

                let class_metadata = self.class_metadata.read();
                if let Some(meta) = class_metadata.get(nominal_type_id) {
                    let names = &meta.static_field_names;
                    let mut arr = Array::new(0, names.len());
                    for (i, name) in names.iter().enumerate() {
                        if !name.is_empty() {
                            let s = RayaString::new(name.clone());
                            let gc_ptr = self.gc.lock().allocate(s);
                            let val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            };
                            arr.set(i, val).ok();
                        }
                    }
                    let arr_gc = self.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                } else {
                    let arr = Array::new(0, 0);
                    let arr_gc = self.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                }
            }

            reflect::GET_STATIC_FIELDS => {
                // getStaticFields(typeRef) -> get static field infos (stub for now)
                // Static field detailed info requires additional metadata
                let arr = Array::new(0, 0);
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            // ===== Phase 4: Method Invocation =====
            reflect::HAS_METHOD => {
                // hasMethod(target, methodName) -> check if method exists
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "hasMethod requires 2 arguments (target, methodName)".to_string(),
                    ));
                }
                let target = args[0];
                let method_name = get_string(args[1])?;

                let has_method = Self::reflect_object_ptr(target)
                    .map(|ptr| {
                        let obj = unsafe { ptr.as_ref() };
                        self.reflect_method_slot_for_object(obj, &method_name)
                            .is_some()
                            || self
                                .reflect_property_value(target, &method_name)
                                .is_some_and(|value| self.reflect_is_callable_value(value))
                    })
                    .unwrap_or(false);
                Value::bool(has_method)
            }

            reflect::GET_METHODS
            | reflect::GET_METHOD
            | reflect::GET_METHOD_INFO
            | reflect::INVOKE
            | reflect::INVOKE_ASYNC
            | reflect::INVOKE_STATIC
            | reflect::GET_STATIC_METHODS => {
                // These require full --emit-reflection metadata and dynamic dispatch
                // Return null/empty for now
                match method_id {
                    reflect::INVOKE | reflect::INVOKE_ASYNC | reflect::INVOKE_STATIC => {
                        return Err(VmError::RuntimeError(
                            "Dynamic method invocation requires --emit-reflection".to_string(),
                        ));
                    }
                    reflect::GET_METHOD | reflect::GET_METHOD_INFO => Value::null(),
                    _ => {
                        let arr = Array::new(0, 0);
                        let arr_gc = self.gc.lock().allocate(arr);
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                    }
                }
            }

            // ===== Phase 5: Object Creation =====
            reflect::CONSTRUCT => {
                // construct(typeRef, ...args) -> create instance
                // JS mode also routes Reflect.construct(target, args, newTarget) here.
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "construct requires at least 1 argument (typeRef)".to_string(),
                    ));
                }
                if self.callable_function_info(args[0]).is_some()
                    || self.unwrapped_proxy_like(args[0]).is_some()
                {
                    let target = args[0];
                    let arg_list = args.get(1).copied().unwrap_or(Value::null());
                    let new_target = args
                        .get(2)
                        .copied()
                        .filter(|value| !value.is_null() && !value.is_undefined())
                        .unwrap_or(target);

                    if !self.callable_is_constructible(new_target) {
                        return Err(VmError::TypeError(
                            "Reflect.construct newTarget must be a constructor".to_string(),
                        ));
                    }
                    let ctor_args = self.collect_apply_arguments(arg_list)?;
                    self.construct_value_with_new_target(
                        target, new_target, &ctor_args, task, module,
                    )?
                } else {
                    let nominal_type_id =
                        self.reflect_require_nominal_type_id(args[0], "construct")?;

                    let (layout_id, field_count) =
                        self.nominal_allocation(nominal_type_id).ok_or_else(|| {
                            VmError::RuntimeError(format!("Class {} not found", nominal_type_id))
                        })?;

                    let obj = Object::new_nominal(layout_id, nominal_type_id as u32, field_count);
                    let gc_ptr = self.gc.lock().allocate(obj);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
                }
            }

            reflect::ALLOCATE => {
                // allocate(typeRef) -> allocate uninitialized instance
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "allocate requires 1 argument (typeRef)".to_string(),
                    ));
                }
                let nominal_type_id = self.reflect_require_nominal_type_id(args[0], "allocate")?;

                let (layout_id, field_count) =
                    self.nominal_allocation(nominal_type_id).ok_or_else(|| {
                        VmError::RuntimeError(format!("Class {} not found", nominal_type_id))
                    })?;

                // Allocate new object (uninitialized - fields are null)
                let obj = Object::new_nominal(layout_id, nominal_type_id as u32, field_count);
                let gc_ptr = self.gc.lock().allocate(obj);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
            }

            reflect::CLONE => {
                // clone(obj) -> shallow clone
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "clone requires 1 argument".to_string(),
                    ));
                }
                let target = args[0];

                if !target.is_ptr() {
                    // Primitives are copied by value
                    target
                } else if let Some(obj_ptr) = Self::reflect_object_ptr(target) {
                    // Clone any unified runtime object, nominal or structural.
                    let obj = unsafe { obj_ptr.as_ref() };
                    let cloned = obj.clone();
                    let gc_ptr = self.gc.lock().allocate(cloned);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
                } else {
                    // Unknown pointer type, return as-is
                    target
                }
            }

            reflect::CONSTRUCT_WITH | reflect::DEEP_CLONE | reflect::GET_CONSTRUCTOR_INFO => {
                // These require more complex implementation
                match method_id {
                    reflect::CONSTRUCT_WITH => {
                        return Err(VmError::RuntimeError(
                            "constructWith requires --emit-reflection".to_string(),
                        ));
                    }
                    reflect::DEEP_CLONE => {
                        return Err(VmError::RuntimeError(
                            "deepClone not yet implemented".to_string(),
                        ));
                    }
                    _ => Value::null(),
                }
            }

            // ===== Phase 6: Type Utilities =====
            reflect::IS_STRING => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "isString requires 1 argument".to_string(),
                    ));
                }
                let value = args[0];
                let is_string =
                    crate::vm::interpreter::opcodes::native::checked_string_ptr(value).is_some();
                Value::bool(is_string)
            }

            reflect::IS_NUMBER => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "isNumber requires 1 argument".to_string(),
                    ));
                }
                let value = args[0];
                let is_number = value.as_f64().is_some() || value.as_i32().is_some();
                Value::bool(is_number)
            }

            reflect::IS_BOOLEAN => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "isBoolean requires 1 argument".to_string(),
                    ));
                }
                let value = args[0];
                let is_bool = value.as_bool().is_some();
                Value::bool(is_bool)
            }

            reflect::IS_NULL => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "isNull requires 1 argument".to_string(),
                    ));
                }
                let value = args[0];
                Value::bool(value.is_null())
            }

            reflect::IS_ARRAY => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "isArray requires 1 argument".to_string(),
                    ));
                }
                let is_array = self.is_array_value(args[0])?;
                Value::bool(is_array)
            }

            reflect::IS_FUNCTION => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "isFunction requires 1 argument".to_string(),
                    ));
                }
                let value = args[0];
                let is_func =
                    crate::vm::interpreter::opcodes::native::checked_closure_ptr(value).is_some();
                Value::bool(is_func)
            }

            reflect::IS_OBJECT => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "isObject requires 1 argument".to_string(),
                    ));
                }
                let value = args[0];
                let is_obj =
                    crate::vm::interpreter::opcodes::native::checked_object_ptr(value).is_some();
                Value::bool(is_obj)
            }

            reflect::TYPE_OF => {
                // typeOf(typeName) - get TypeInfo from string
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "typeOf requires 1 argument".to_string(),
                    ));
                }
                let type_name = get_string(args[0])?;

                // Check primitive types
                let (kind, nominal_type_id) = match type_name.as_str() {
                    "string" | "number" | "boolean" | "null" | "void" | "any" => {
                        ("primitive".to_string(), None)
                    }
                    _ => {
                        // Check if it's a class name
                        let classes = self.classes.read();
                        if let Some(class) = classes.get_class_by_name(&type_name) {
                            ("class".to_string(), Some(class.id))
                        } else {
                            // Unknown type
                            return stack.push(Value::null());
                        }
                    }
                };

                let mut entries = vec![
                    ("kind", self.reflect_alloc_string_value(kind)),
                    ("name", self.reflect_alloc_string_value(type_name)),
                ];
                if let Some(id) = nominal_type_id {
                    entries.push(("nominalType", self.reflect_alloc_nominal_type_ref(id)));
                }
                self.reflect_alloc_structural_object(&entries)
            }

            reflect::IS_ASSIGNABLE_TO => {
                // isAssignableTo(sourceType, targetType) - check type compatibility
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "isAssignableTo requires 2 arguments".to_string(),
                    ));
                }
                let source = get_string(args[0])?;
                let target = get_string(args[1])?;

                // Same type is always assignable
                if source == target {
                    Value::bool(true)
                } else if target == "any" {
                    // Everything is assignable to any
                    Value::bool(true)
                } else {
                    // Check class hierarchy
                    let classes = self.classes.read();
                    let source_class = classes.get_class_by_name(&source);
                    let target_class = classes.get_class_by_name(&target);

                    if let (Some(src), Some(tgt)) = (source_class, target_class) {
                        let is_subclass =
                            crate::vm::reflect::is_subclass_of(&classes, src.id, tgt.id);
                        Value::bool(is_subclass)
                    } else {
                        Value::bool(false)
                    }
                }
            }

            reflect::CAST => {
                // cast(value, typeRef) - safe cast, returns null if incompatible
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "cast requires 2 arguments".to_string(),
                    ));
                }
                let value = args[0];
                let nominal_type_id = self.reflect_require_nominal_type_id(args[1], "cast")?;

                let classes = self.classes.read();
                if crate::vm::reflect::is_instance_of(&classes, value, nominal_type_id) {
                    value
                } else {
                    Value::null()
                }
            }

            reflect::CAST_OR_THROW => {
                // castOrThrow(value, typeRef) - cast or throw error
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "castOrThrow requires 2 arguments".to_string(),
                    ));
                }
                let value = args[0];
                let nominal_type_id =
                    self.reflect_require_nominal_type_id(args[1], "castOrThrow")?;

                let classes = self.classes.read();
                if crate::vm::reflect::is_instance_of(&classes, value, nominal_type_id) {
                    value
                } else {
                    return Err(VmError::TypeError(format!(
                        "Cannot cast value to nominal type {}",
                        nominal_type_id
                    )));
                }
            }

            // ===== Phase 7: Interface and Hierarchy Query =====
            reflect::IMPLEMENTS => {
                // implements(typeRef, interfaceName) - check if class implements interface
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "implements requires 2 arguments".to_string(),
                    ));
                }
                let nominal_type_id =
                    self.reflect_require_nominal_type_id(args[0], "implements")?;
                let interface_name = get_string(args[1])?;

                let class_metadata = self.class_metadata.read();
                if let Some(meta) = class_metadata.get(nominal_type_id) {
                    Value::bool(meta.implements_interface(&interface_name))
                } else {
                    Value::bool(false)
                }
            }

            reflect::GET_INTERFACES => {
                // getInterfaces(typeRef) - get interfaces implemented by class
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getInterfaces requires 1 argument".to_string(),
                    ));
                }
                let nominal_type_id =
                    self.reflect_require_nominal_type_id(args[0], "getInterfaces")?;

                let class_metadata = self.class_metadata.read();
                let interfaces: Vec<String> =
                    if let Some(meta) = class_metadata.get(nominal_type_id) {
                        meta.get_interfaces().to_vec()
                    } else {
                        Vec::new()
                    };
                drop(class_metadata);

                // Build array of interface names
                let mut arr = Array::new(0, 0);
                for iface in interfaces {
                    let s = RayaString::new(iface);
                    let s_ptr = self.gc.lock().allocate(s);
                    arr.push(unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(s_ptr.as_ptr()).unwrap())
                    });
                }
                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }

            reflect::GET_SUPERCLASS => {
                // getSuperclass(typeRef) - get parent nominal type ref
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getSuperclass requires 1 argument".to_string(),
                    ));
                }
                let nominal_type_id =
                    self.reflect_require_nominal_type_id(args[0], "getSuperclass")?;

                let classes = self.classes.read();
                if let Some(class) = classes.get_class(nominal_type_id) {
                    if let Some(parent) = class.parent_id {
                        drop(classes);
                        self.reflect_alloc_nominal_type_ref(parent)
                    } else {
                        Value::null()
                    }
                } else {
                    Value::null()
                }
            }

            reflect::GET_SUBCLASSES => {
                // getSubclasses(typeRef) - get direct subclass nominal type refs
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getSubclasses requires 1 argument".to_string(),
                    ));
                }
                let nominal_type_id =
                    self.reflect_require_nominal_type_id(args[0], "getSubclasses")?;

                let classes = self.classes.read();
                let mut subclasses = Vec::new();
                for (id, class) in classes.iter() {
                    if class.parent_id == Some(nominal_type_id) {
                        subclasses.push(id);
                    }
                }
                drop(classes);
                self.reflect_alloc_nominal_type_ref_array(subclasses)
            }

            reflect::GET_IMPLEMENTORS => {
                // getImplementors(interfaceName) - get all classes implementing interface
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getImplementors requires 1 argument".to_string(),
                    ));
                }
                let interface_name = get_string(args[0])?;

                let class_metadata = self.class_metadata.read();
                let implementors = class_metadata.get_implementors(&interface_name);
                drop(class_metadata);
                self.reflect_alloc_nominal_type_ref_array(implementors)
            }

            reflect::IS_STRUCTURALLY_COMPATIBLE => {
                // isStructurallyCompatible(sourceTypeRef, targetTypeRef) - check structural compatibility
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "isStructurallyCompatible requires 2 arguments".to_string(),
                    ));
                }
                let source_id =
                    self.reflect_require_nominal_type_id(args[0], "isStructurallyCompatible")?;
                let target_id =
                    self.reflect_require_nominal_type_id(args[1], "isStructurallyCompatible")?;

                let class_metadata = self.class_metadata.read();
                let source_meta = class_metadata.get(source_id);
                let target_meta = class_metadata.get(target_id);

                if let (Some(source), Some(target)) = (source_meta, target_meta) {
                    // Check if source has all fields of target
                    let fields_ok = target.field_names.iter().all(|name| source.has_field(name));
                    // Check if source has all methods of target
                    let methods_ok = target
                        .method_names
                        .iter()
                        .all(|name| name.is_empty() || source.has_method(name));
                    Value::bool(fields_ok && methods_ok)
                } else {
                    Value::bool(false)
                }
            }

            // ===== Phase 8: Object Inspection =====
            reflect::INSPECT => {
                // inspect(obj, depth?) - human-readable representation
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "inspect requires 1 argument".to_string(),
                    ));
                }
                let target = args[0];
                let max_depth = if args.len() > 1 {
                    value_to_f64(args[1])? as usize
                } else {
                    2
                };

                let result = self.inspect_value(target, 0, max_depth)?;
                let s = RayaString::new(result);
                let s_ptr = self.gc.lock().allocate(s);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(s_ptr.as_ptr()).unwrap()) }
            }

            reflect::GET_OBJECT_ID => {
                // getObjectId(obj) - unique object identifier
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getObjectId requires 1 argument".to_string(),
                    ));
                }
                let value = args[0];

                if !value.is_ptr() || value.is_null() {
                    Value::i32(0)
                } else if let Some(ptr) = unsafe { value.as_ptr::<Object>() } {
                    Value::i32((ptr.as_ptr() as usize & 0x7FFFFFFF) as i32)
                } else if let Some(ptr) = unsafe { value.as_ptr::<Array>() } {
                    Value::i32((ptr.as_ptr() as usize & 0x7FFFFFFF) as i32)
                } else if let Some(ptr) = unsafe { value.as_ptr::<RayaString>() } {
                    Value::i32((ptr.as_ptr() as usize & 0x7FFFFFFF) as i32)
                } else {
                    Value::i32(0)
                }
            }

            reflect::DESCRIBE => {
                // describe(typeRef) - detailed class description
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "describe requires 1 argument".to_string(),
                    ));
                }
                let nominal_type_id = self.reflect_require_nominal_type_id(args[0], "describe")?;

                let classes = self.classes.read();
                let class = classes.get_class(nominal_type_id);
                let class_metadata = self.class_metadata.read();
                let meta = class_metadata.get(nominal_type_id);

                let description = if let Some(class) = class {
                    let mut desc = format!("class {} {{\n", class.name);

                    if let Some(m) = meta {
                        // Fields
                        for name in &m.field_names {
                            desc.push_str(&format!("  {}: any;\n", name));
                        }
                        // Methods
                        for name in &m.method_names {
                            if !name.is_empty() {
                                desc.push_str(&format!("  {}(): any;\n", name));
                            }
                        }
                    } else {
                        desc.push_str(&format!("  // {} fields\n", class.field_count));
                    }

                    desc.push('}');
                    desc
                } else {
                    format!("Unknown class {}", nominal_type_id)
                };

                let s = RayaString::new(description);
                let s_ptr = self.gc.lock().allocate(s);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(s_ptr.as_ptr()).unwrap()) }
            }

            reflect::SNAPSHOT => {
                // snapshot(obj) - Capture object state as a snapshot
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "snapshot requires 1 argument".to_string(),
                    ));
                }
                let target = args[0];

                // Create snapshot context with max depth of 10
                let mut ctx = SnapshotContext::new(10);

                // Get class name if it's an object
                let (class_name, field_names) = self.reflect_object_snapshot_descriptor(target);

                // Capture the snapshot
                let snapshot = ctx.capture_object_with_names(target, &field_names, &class_name);

                // Convert snapshot to a Raya Object
                self.snapshot_to_value(&snapshot)
            }

            reflect::DIFF => {
                // diff(a, b) - Compare two objects and return differences
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "diff requires 2 arguments".to_string(),
                    ));
                }
                let obj_a = args[0];
                let obj_b = args[1];

                // Capture both objects as snapshots
                let mut ctx = SnapshotContext::new(10);

                let (class_name_a, field_names_a) = self.reflect_object_snapshot_descriptor(obj_a);
                let (class_name_b, field_names_b) = self.reflect_object_snapshot_descriptor(obj_b);

                let snapshot_a =
                    ctx.capture_object_with_names(obj_a, &field_names_a, &class_name_a);
                let snapshot_b =
                    ctx.capture_object_with_names(obj_b, &field_names_b, &class_name_b);

                // Compute the diff
                let diff = ObjectDiff::compute(&snapshot_a, &snapshot_b);

                // Convert diff to a Raya Object
                self.diff_to_value(&diff)
            }

            // ===== Phase 8: Memory Analysis =====
            reflect::GET_OBJECT_SIZE => {
                // getObjectSize(obj) - shallow memory size
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getObjectSize requires 1 argument".to_string(),
                    ));
                }
                let value = args[0];

                let size = if !value.is_ptr() || value.is_null() {
                    8 // primitive size
                } else if let Some(ptr) = unsafe { value.as_ptr::<Object>() } {
                    let obj = unsafe { &*ptr.as_ptr() };
                    std::mem::size_of::<Object>() + obj.fields.len() * 8
                } else if let Some(ptr) = unsafe { value.as_ptr::<Array>() } {
                    let arr = unsafe { &*ptr.as_ptr() };
                    std::mem::size_of::<Array>() + arr.len() * 8
                } else if let Some(ptr) = unsafe { value.as_ptr::<RayaString>() } {
                    let s = unsafe { &*ptr.as_ptr() };
                    std::mem::size_of::<RayaString>() + s.data.len()
                } else {
                    8
                };

                Value::i32(size as i32)
            }

            reflect::GET_RETAINED_SIZE => {
                // getRetainedSize(obj) - size including referenced objects
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getRetainedSize requires 1 argument".to_string(),
                    ));
                }
                let target = args[0];

                let mut visited = std::collections::HashSet::new();
                let size = self.calculate_retained_size(target, &mut visited);
                Value::i32(size as i32)
            }

            reflect::GET_REFERENCES => {
                // getReferences(obj) - objects referenced by this object
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getReferences requires 1 argument".to_string(),
                    ));
                }
                let target = args[0];

                let mut refs = Vec::new();
                self.collect_references(target, &mut refs);

                let mut arr = Array::new(0, 0);
                for r in refs {
                    arr.push(r);
                }
                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }

            reflect::GET_REFERRERS => {
                // getReferrers(obj) - objects that reference this object
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getReferrers requires 1 argument".to_string(),
                    ));
                }
                let target = args[0];

                // Get target's identity
                let target_id = if let Some(ptr) = unsafe { target.as_ptr::<u8>() } {
                    ptr.as_ptr() as usize
                } else {
                    return stack.push(Value::null());
                };

                // Scan all allocations for references to target
                let gc = self.gc.lock();
                let mut referrers = Vec::new();

                for header_ptr in gc.heap().iter_allocations() {
                    let header = unsafe { &*header_ptr };
                    // Get the object pointer (after header)
                    let obj_ptr = unsafe { header_ptr.add(1) as *const u8 };

                    // Check if this object references the target
                    // This is a simplified check - just look at Object types
                    if header.type_id() == std::any::TypeId::of::<Object>() {
                        let obj = unsafe { &*(obj_ptr as *const Object) };
                        for field in &obj.fields {
                            if let Some(ptr) = unsafe { field.as_ptr::<u8>() } {
                                if ptr.as_ptr() as usize == target_id {
                                    let value = unsafe {
                                        Value::from_ptr(
                                            std::ptr::NonNull::new(obj_ptr as *mut Object).unwrap(),
                                        )
                                    };
                                    referrers.push(value);
                                    break;
                                }
                            }
                        }
                    }
                }
                drop(gc);

                let mut arr = Array::new(0, 0);
                for r in referrers {
                    arr.push(r);
                }
                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }

            reflect::GET_HEAP_STATS => {
                // getHeapStats() - heap statistics
                let gc = self.gc.lock();
                let stats = gc.heap_stats();
                drop(gc);

                self.reflect_alloc_structural_object(&[
                    ("totalObjects", Value::i32(stats.allocation_count as i32)),
                    ("totalBytes", Value::i32(stats.allocated_bytes as i32)),
                ])
            }

            reflect::FIND_INSTANCES => {
                // findInstances(typeRef) - find all live instances of a class
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "findInstances requires 1 argument".to_string(),
                    ));
                }
                let nominal_type_id =
                    self.reflect_require_nominal_type_id(args[0], "findInstances")?;

                let gc = self.gc.lock();
                let mut instances = Vec::new();

                for header_ptr in gc.heap().iter_allocations() {
                    let header = unsafe { &*header_ptr };
                    // Check if this is an Object with matching nominal_type_id
                    if header.type_id() == std::any::TypeId::of::<Object>() {
                        let obj_ptr = unsafe { header_ptr.add(1) as *const Object };
                        let obj = unsafe { &*obj_ptr };
                        if obj.nominal_type_id_usize() == Some(nominal_type_id) {
                            let value = unsafe {
                                Value::from_ptr(
                                    std::ptr::NonNull::new(obj_ptr as *mut Object).unwrap(),
                                )
                            };
                            instances.push(value);
                        }
                    }
                }
                drop(gc);

                let mut arr = Array::new(0, 0);
                for inst in instances {
                    arr.push(inst);
                }
                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }

            // ===== Phase 8: Stack Introspection =====
            reflect::GET_CALL_STACK => {
                // getCallStack() - get current call frames
                let call_stack = task.get_call_stack();
                let stack_frames: Vec<_> = stack.frames().collect();

                let mut arr = Array::new(0, 0);

                for (i, &func_id) in call_stack.iter().enumerate() {
                    // Function name
                    let func_name = module
                        .functions
                        .get(func_id)
                        .map(|f| f.name.clone())
                        .unwrap_or_else(|| format!("<function_{}>", func_id));
                    let function_name = self.reflect_alloc_string_value(func_name);
                    let mut entries = vec![
                        ("functionName", function_name),
                        ("frameIndex", Value::i32(i as i32)),
                    ];

                    // Add frame info if available
                    if let Some(frame) = stack_frames.get(i) {
                        entries.push(("localCount", Value::i32(frame.local_count as i32)));
                        entries.push(("argCount", Value::i32(frame.arg_count as i32)));
                    }
                    arr.push(self.reflect_alloc_structural_object(&entries));
                }

                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }

            reflect::GET_LOCALS => {
                // getLocals(frameIndex?) - get local variables
                let frame_index = if !args.is_empty() {
                    value_to_f64(args[0])? as usize
                } else {
                    0
                };

                let frames: Vec<_> = stack.frames().collect();
                if let Some(frame) = frames.get(frame_index) {
                    let mut locals_arr = Array::new(0, 0);

                    for i in 0..frame.local_count {
                        if let Ok(local) = stack.load_local(i) {
                            locals_arr.push(local);
                        } else {
                            locals_arr.push(Value::null());
                        }
                    }

                    let arr_ptr = self.gc.lock().allocate(locals_arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
                } else {
                    Value::null()
                }
            }

            reflect::GET_SOURCE_LOCATION => {
                // getSourceLocation(typeRef, methodName) - source location
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "getSourceLocation requires 2 arguments: typeRef, methodName".to_string(),
                    ));
                }

                let nominal_type_id =
                    self.reflect_require_nominal_type_id(args[0], "getSourceLocation")?;

                let method_name = if let Some(ptr) = unsafe { args[1].as_ptr::<RayaString>() } {
                    let s = unsafe { &*ptr.as_ptr() };
                    s.data.clone()
                } else {
                    return Err(VmError::RuntimeError(
                        "getSourceLocation: methodName must be a string".to_string(),
                    ));
                };

                // Check if module has debug info
                if !module.has_debug_info() {
                    // Return null if no debug info available
                    Value::null()
                } else if let Some(ref debug_info) = module.debug_info {
                    // Find the class and method
                    if let Some(class_def) = module.classes.get(nominal_type_id) {
                        // Find the method by name
                        let method = class_def.methods.iter().find(|m| m.name == method_name);

                        if let Some(method) = method {
                            let function_id = method.function_id;

                            // Get function debug info
                            if let Some(func_debug) = debug_info.functions.get(function_id) {
                                // Get source file path
                                let source_file = debug_info
                                    .get_source_file(func_debug.source_file_index)
                                    .unwrap_or("unknown");

                                // Create a SourceLocation object with: file, line, column
                                let mut result_obj = Object::new_structural(
                                    crate::vm::object::layout_id_from_ordered_names(&[
                                        "file".to_string(),
                                        "line".to_string(),
                                        "column".to_string(),
                                    ]),
                                    3,
                                );

                                // Set file
                                let file_str = RayaString::new(source_file.to_string());
                                let file_ptr = self.gc.lock().allocate(file_str);
                                let _ = result_obj.set_field(0, unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(file_ptr.as_ptr()).unwrap(),
                                    )
                                });

                                // Set line (1-indexed)
                                let _ = result_obj
                                    .set_field(1, Value::i32(func_debug.start_line as i32));

                                // Set column (1-indexed)
                                let _ = result_obj
                                    .set_field(2, Value::i32(func_debug.start_column as i32));

                                let result_ptr = self.gc.lock().allocate(result_obj);
                                unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(result_ptr.as_ptr()).unwrap(),
                                    )
                                }
                            } else {
                                Value::null()
                            }
                        } else {
                            // Method not found
                            Value::null()
                        }
                    } else {
                        Value::null()
                    }
                } else {
                    Value::null()
                }
            }

            // ===== Phase 8: Serialization Helpers =====
            reflect::TO_JSON => {
                // toJSON(obj) - JSON string representation
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "toJSON requires 1 argument".to_string(),
                    ));
                }
                let target = args[0];
                let mut visited = Vec::new();
                let json = self.value_to_json(target, &mut visited)?;
                let s = RayaString::new(json);
                let s_ptr = self.gc.lock().allocate(s);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(s_ptr.as_ptr()).unwrap()) }
            }

            reflect::GET_ENUMERABLE_KEYS => {
                // getEnumerableKeys(obj) - get field names
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getEnumerableKeys requires 1 argument".to_string(),
                    ));
                }
                let target = args[0];

                let mut arr = Array::new(0, 0);

                if let Some(obj_ptr) = Self::reflect_object_ptr(target) {
                    let obj = unsafe { obj_ptr.as_ref() };
                    for name in self.reflect_object_field_names(obj) {
                        if !self.is_property_enumerable(target, &name) {
                            continue;
                        }
                        let s = RayaString::new(name);
                        let s_ptr = self.gc.lock().allocate(s);
                        arr.push(unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(s_ptr.as_ptr()).unwrap())
                        });
                    }
                }

                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }

            reflect::IS_CIRCULAR => {
                // isCircular(obj) - check for circular references
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "isCircular requires 1 argument".to_string(),
                    ));
                }
                let target = args[0];
                let mut visited = Vec::new();
                let is_circular = self.check_circular(target, &mut visited);
                Value::bool(is_circular)
            }

            // ===== Phase 9: Proxy Objects =====
            reflect::CREATE_PROXY => {
                // createProxy(target, handler)
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "createProxy requires 2 arguments (target, handler)".to_string(),
                    ));
                }
                let target = args[0];
                let handler = args[1];

                if !target.is_ptr() || !handler.is_ptr() {
                    return Err(VmError::TypeError(
                        "createProxy: target and handler must be objects".to_string(),
                    ));
                }

                let proxy = Proxy::new(target, handler);
                let proxy_gc = self.gc.lock().allocate(proxy);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(proxy_gc.as_ptr()).unwrap()) }
            }

            reflect::IS_PROXY => {
                // isProxy(obj)
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "isProxy requires 1 argument".to_string(),
                    ));
                }
                Value::bool(crate::vm::reflect::is_proxy(args[0]))
            }

            reflect::GET_PROXY_TARGET => {
                // getProxyTarget(proxy)
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getProxyTarget requires 1 argument".to_string(),
                    ));
                }
                if let Some(unwrapped) = crate::vm::reflect::try_unwrap_proxy(args[0]) {
                    unwrapped.target
                } else {
                    Value::null()
                }
            }

            reflect::GET_PROXY_HANDLER => {
                // getProxyHandler(proxy)
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getProxyHandler requires 1 argument".to_string(),
                    ));
                }
                if let Some(unwrapped) = crate::vm::reflect::try_unwrap_proxy(args[0]) {
                    unwrapped.handler
                } else {
                    Value::null()
                }
            }

            reflect::REVOKE_PROXY => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "revokeProxy requires 1 argument".to_string(),
                    ));
                }
                let Some(raw_ptr) = (unsafe { args[0].as_ptr::<u8>() }) else {
                    return Err(VmError::TypeError(
                        "revokeProxy expects a proxy".to_string(),
                    ));
                };
                let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };
                if header.type_id() != std::any::TypeId::of::<Proxy>() {
                    return Err(VmError::TypeError(
                        "revokeProxy expects a proxy".to_string(),
                    ));
                }
                let proxy = unsafe { &mut *(raw_ptr.as_ptr() as *mut Proxy) };
                proxy.handler = Value::null();
                Value::null()
            }

            // ===== Decorator Registration (Phase 3/4 codegen) =====
            reflect::REGISTER_CLASS_DECORATOR => {
                // registerClassDecorator(typeRef, decoratorName)
                // Metadata registration - currently a no-op, decorator function does the work
                // The DecoratorRegistry is populated by the codegen emitted registration calls
                // which use global state. For now, we just acknowledge the call.
                Value::null()
            }

            reflect::REGISTER_METHOD_DECORATOR => {
                // registerMethodDecorator(typeRef, methodName, decoratorName)
                // Metadata registration - currently a no-op
                Value::null()
            }

            reflect::REGISTER_FIELD_DECORATOR => {
                // registerFieldDecorator(typeRef, fieldName, decoratorName)
                // Metadata registration - currently a no-op
                Value::null()
            }

            reflect::REGISTER_PARAMETER_DECORATOR => {
                // registerParameterDecorator(typeRef, methodName, paramIndex, decoratorName)
                // Metadata registration - currently a no-op
                Value::null()
            }

            reflect::GET_CLASS_DECORATORS => {
                // getClassDecorators(typeRef) -> get decorators applied to class
                // Returns empty array for now - full implementation uses DecoratorRegistry
                let arr = Array::new(0, 0);
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            reflect::GET_METHOD_DECORATORS => {
                // getMethodDecorators(typeRef, methodName) -> get decorators on method
                let arr = Array::new(0, 0);
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            reflect::GET_FIELD_DECORATORS => {
                // getFieldDecorators(typeRef, fieldName) -> get decorators on field
                let arr = Array::new(0, 0);
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            // ── BytecodeBuilder (Phase 15, delegated from std:runtime Phase 6) ──
            reflect::NEW_BYTECODE_BUILDER => {
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "BytecodeBuilder requires 3 arguments (name, paramCount, returnType)"
                            .to_string(),
                    ));
                }
                let name = get_string(args[0])?;
                let param_count = args[1]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("paramCount must be a number".to_string()))?
                    as usize;
                let return_type = get_string(args[2])?;
                let mut registry =
                    crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder_id = registry.create_builder(name, param_count, return_type);
                Value::i32(builder_id as i32)
            }

            reflect::BUILDER_EMIT => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "emit requires at least 2 arguments (builderId, opcode)".to_string(),
                    ));
                }
                let builder_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let opcode = args[1]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("opcode must be a number".to_string()))?
                    as u8;
                let operands: Vec<u8> = args[2..]
                    .iter()
                    .filter_map(|v| v.as_i32().map(|n| n as u8))
                    .collect();
                let mut registry =
                    crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id))
                })?;
                builder.emit(opcode, &operands)?;
                Value::null()
            }

            reflect::BUILDER_EMIT_PUSH => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "emitPush requires 2 arguments (builderId, value)".to_string(),
                    ));
                }
                let builder_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let value = args[1];
                let mut registry =
                    crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id))
                })?;
                if value.is_null() {
                    builder.emit_push_null()?;
                } else if let Some(b) = value.as_bool() {
                    builder.emit_push_bool(b)?;
                } else if let Some(i) = value.as_i32() {
                    builder.emit_push_i32(i)?;
                } else if let Some(f) = value.as_f64() {
                    builder.emit_push_f64(f)?;
                } else {
                    builder.emit_push_i32(0)?;
                }
                Value::null()
            }

            reflect::BUILDER_DEFINE_LABEL => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "defineLabel requires 1 argument (builderId)".to_string(),
                    ));
                }
                let builder_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let mut registry =
                    crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id))
                })?;
                let label = builder.define_label();
                Value::i32(label.id as i32)
            }

            reflect::BUILDER_MARK_LABEL => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "markLabel requires 2 arguments (builderId, labelId)".to_string(),
                    ));
                }
                let builder_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let label_id = args[1]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("labelId must be a number".to_string()))?
                    as usize;
                let mut registry =
                    crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id))
                })?;
                builder.mark_label(crate::vm::reflect::Label { id: label_id })?;
                Value::null()
            }

            reflect::BUILDER_EMIT_JUMP => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "emitJump requires 2 arguments (builderId, labelId)".to_string(),
                    ));
                }
                let builder_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let label_id = args[1]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("labelId must be a number".to_string()))?
                    as usize;
                let mut registry =
                    crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id))
                })?;
                builder.emit_jump(crate::vm::reflect::Label { id: label_id })?;
                Value::null()
            }

            reflect::BUILDER_EMIT_JUMP_IF => {
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "emitJumpIf requires 3 arguments (builderId, labelId, ifTrue)".to_string(),
                    ));
                }
                let builder_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let label_id = args[1]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("labelId must be a number".to_string()))?
                    as usize;
                let if_true = args[2].as_bool().unwrap_or(false);
                let mut registry =
                    crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id))
                })?;
                if if_true {
                    builder.emit_jump_if_true(crate::vm::reflect::Label { id: label_id })?;
                } else {
                    builder.emit_jump_if_false(crate::vm::reflect::Label { id: label_id })?;
                }
                Value::null()
            }

            reflect::BUILDER_DECLARE_LOCAL => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "declareLocal requires 2 arguments (builderId, typeName)".to_string(),
                    ));
                }
                let builder_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let type_name = get_string(args[1])?;
                let stack_type = match type_name.as_str() {
                    "number" | "i32" | "i64" | "int" => crate::vm::reflect::StackType::Integer,
                    "f64" | "float" => crate::vm::reflect::StackType::Float,
                    "boolean" | "bool" => crate::vm::reflect::StackType::Boolean,
                    "string" => crate::vm::reflect::StackType::String,
                    "null" => crate::vm::reflect::StackType::Null,
                    _ => crate::vm::reflect::StackType::Object,
                };
                let mut registry =
                    crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id))
                })?;
                let index = builder.declare_local(None, stack_type)?;
                Value::i32(index as i32)
            }

            reflect::BUILDER_EMIT_LOAD_LOCAL => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "emitLoadLocal requires 2 arguments (builderId, index)".to_string(),
                    ));
                }
                let builder_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let index = args[1]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("index must be a number".to_string()))?
                    as usize;
                let mut registry =
                    crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id))
                })?;
                builder.emit_load_local(index)?;
                Value::null()
            }

            reflect::BUILDER_EMIT_STORE_LOCAL => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "emitStoreLocal requires 2 arguments (builderId, index)".to_string(),
                    ));
                }
                let builder_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let index = args[1]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("index must be a number".to_string()))?
                    as usize;
                let mut registry =
                    crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id))
                })?;
                builder.emit_store_local(index)?;
                Value::null()
            }

            reflect::BUILDER_EMIT_CALL => {
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "emitCall requires 3 arguments (builderId, functionId, argCount)"
                            .to_string(),
                    ));
                }
                let builder_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let function_id = args[1]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("functionId must be a number".to_string()))?
                    as u32;
                let arg_count = args[2]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("argCount must be a number".to_string()))?
                    as u16;
                let mut registry =
                    crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id))
                })?;
                builder.emit_call(function_id, arg_count)?;
                Value::null()
            }

            reflect::BUILDER_EMIT_RETURN => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "emitReturn requires at least 1 argument (builderId)".to_string(),
                    ));
                }
                let builder_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let has_value = args.get(1).and_then(|v| v.as_bool()).unwrap_or(true);
                let mut registry =
                    crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id))
                })?;
                if has_value {
                    builder.emit_return()?;
                } else {
                    builder.emit_return_void()?;
                }
                Value::null()
            }

            reflect::BUILDER_VALIDATE => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "validate requires 1 argument (builderId)".to_string(),
                    ));
                }
                let builder_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let mut registry =
                    crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id))
                })?;
                let result = builder.validate();
                Value::bool(result.is_valid)
            }

            reflect::BUILDER_BUILD_FUNCTION => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "build requires 1 argument (builderId)".to_string(),
                    ));
                }
                let builder_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let mut registry =
                    crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id))
                })?;
                let func = builder.build()?;
                let func_id = func.function_id;
                registry.register_function(func);
                Value::i32(func_id as i32)
            }

            // ===== Phase 14: ClassBuilder (0x0DE0-0x0DE6) =====
            reflect::NEW_CLASS_BUILDER => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "newClassBuilder requires 1 argument (name)".to_string(),
                    ));
                }
                let name = get_string(args[0])?;
                let mut registry =
                    crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                let builder_id = registry.create_builder(name);
                Value::i32(builder_id as i32)
            }

            reflect::BUILDER_ADD_FIELD => {
                if args.len() < 5 {
                    return Err(VmError::RuntimeError(
                        "addField requires 5 arguments (builderId, name, typeName, isStatic, isReadonly)".to_string()
                    ));
                }
                let builder_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let name = get_string(args[1])?;
                let type_name = get_string(args[2])?;
                let is_static = args[3].as_bool().unwrap_or(false);
                let is_readonly = args[4].as_bool().unwrap_or(false);
                let mut registry =
                    crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id))
                })?;
                builder.add_field(name, &type_name, is_static, is_readonly)?;
                Value::null()
            }

            reflect::BUILDER_ADD_METHOD => {
                if args.len() < 5 {
                    return Err(VmError::RuntimeError(
                        "addMethod requires 5 arguments (builderId, name, functionId, isStatic, isAsync)".to_string()
                    ));
                }
                let builder_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let name = get_string(args[1])?;
                let function_id = args[2]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("functionId must be a number".to_string()))?
                    as usize;
                let is_static = args[3].as_bool().unwrap_or(false);
                let is_async = args[4].as_bool().unwrap_or(false);
                let mut registry =
                    crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id))
                })?;
                builder.add_method(name, function_id, is_static, is_async)?;
                Value::null()
            }

            reflect::BUILDER_SET_CONSTRUCTOR => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "setConstructor requires 2 arguments (builderId, functionId)".to_string(),
                    ));
                }
                let builder_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let function_id = args[1]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("functionId must be a number".to_string()))?
                    as usize;
                let mut registry =
                    crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id))
                })?;
                builder.set_constructor(function_id)?;
                Value::null()
            }

            reflect::BUILDER_SET_PARENT => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "setParent requires 2 arguments (builderId, parentTypeRef)".to_string(),
                    ));
                }
                let builder_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let parent_id = self.reflect_require_nominal_type_id(args[1], "setParent")?;
                let mut registry =
                    crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id))
                })?;
                builder.set_parent(parent_id)?;
                Value::null()
            }

            reflect::BUILDER_ADD_INTERFACE => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "addInterface requires 2 arguments (builderId, interfaceName)".to_string(),
                    ));
                }
                let builder_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let interface_name = get_string(args[1])?;
                let mut registry =
                    crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id))
                })?;
                builder.add_interface(interface_name)?;
                Value::null()
            }

            reflect::BUILDER_BUILD => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "build requires 1 argument (builderId)".to_string(),
                    ));
                }
                let builder_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;

                let builder = {
                    let mut registry =
                        crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                    registry.remove(builder_id).ok_or_else(|| {
                        VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id))
                    })?
                };

                let def = builder.to_definition();
                let mut classes_write = self.classes.write();
                let next_id = classes_write.allocate_nominal_type_id();
                let dyn_builder = crate::vm::reflect::DynamicClassBuilder::new();

                let (new_class, new_metadata) = if let Some(parent_id) = builder.parent_id {
                    let parent = classes_write
                        .get_class(parent_id)
                        .ok_or_else(|| {
                            VmError::RuntimeError(format!("Parent class {} not found", parent_id))
                        })?
                        .clone();
                    drop(classes_write);

                    let class_metadata_guard = self.class_metadata.read();
                    let parent_metadata = class_metadata_guard.get(parent_id).cloned();
                    drop(class_metadata_guard);

                    let result = dyn_builder.create_subclass(
                        next_id,
                        builder.name,
                        &parent,
                        parent_metadata.as_ref(),
                        &def,
                    );
                    classes_write = self.classes.write();
                    result
                } else {
                    dyn_builder.create_root_class(next_id, builder.name, &def)
                };

                drop(classes_write);
                let new_nominal_type_id = self.register_runtime_class(new_class);

                let mut class_metadata_write = self.class_metadata.write();
                class_metadata_write.register(new_nominal_type_id, new_metadata);
                drop(class_metadata_write);

                self.reflect_alloc_nominal_type_ref(new_nominal_type_id)
            }

            // ===== Phase 17: DynamicModule (0x0E10-0x0E15) =====
            reflect::CREATE_MODULE => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "createModule requires 1 argument (name)".to_string(),
                    ));
                }
                let name = get_string(args[0])?;
                let mut registry =
                    crate::vm::builtins::handlers::reflect::DYNAMIC_MODULE_REGISTRY.lock();
                let module_id = registry.create_module(name)?;
                Value::i32(module_id as i32)
            }

            reflect::MODULE_ADD_FUNCTION => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "addFunction requires 2 arguments (moduleId, functionId)".to_string(),
                    ));
                }
                let module_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("moduleId must be a number".to_string()))?
                    as usize;
                // Cast i32 → u32 → usize to preserve bit pattern (function IDs start at 0x8000_0000)
                let function_id = args[1]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("functionId must be a number".to_string()))?
                    as u32 as usize;

                let bytecode_registry =
                    crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let func = bytecode_registry
                    .get_function(function_id)
                    .ok_or_else(|| {
                        VmError::RuntimeError(format!("Function {} not found", function_id))
                    })?
                    .clone();
                drop(bytecode_registry);

                let mut registry =
                    crate::vm::builtins::handlers::reflect::DYNAMIC_MODULE_REGISTRY.lock();
                let module = registry.get_mut(module_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("Module {} not found", module_id))
                })?;
                module.add_function(func)?;
                Value::null()
            }

            reflect::MODULE_ADD_CLASS => {
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "addClass requires 3 arguments (moduleId, typeRef, name)".to_string(),
                    ));
                }
                let module_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("moduleId must be a number".to_string()))?
                    as usize;
                let nominal_type_id = self.reflect_require_nominal_type_id(args[1], "addClass")?;
                let name = get_string(args[2])?;
                let mut registry =
                    crate::vm::builtins::handlers::reflect::DYNAMIC_MODULE_REGISTRY.lock();
                let module = registry.get_mut(module_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("Module {} not found", module_id))
                })?;
                module.add_class(nominal_type_id, nominal_type_id, name)?;
                Value::null()
            }

            reflect::MODULE_ADD_GLOBAL => {
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "addGlobal requires 3 arguments (moduleId, name, value)".to_string(),
                    ));
                }
                let module_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("moduleId must be a number".to_string()))?
                    as usize;
                let name = get_string(args[1])?;
                let value = args[2];
                let mut registry =
                    crate::vm::builtins::handlers::reflect::DYNAMIC_MODULE_REGISTRY.lock();
                let module = registry.get_mut(module_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("Module {} not found", module_id))
                })?;
                module.add_global(name, value)?;
                Value::null()
            }

            reflect::MODULE_SEAL => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "seal requires 1 argument (moduleId)".to_string(),
                    ));
                }
                let module_id = args[0]
                    .as_i32()
                    .ok_or_else(|| VmError::TypeError("moduleId must be a number".to_string()))?
                    as usize;
                let mut registry =
                    crate::vm::builtins::handlers::reflect::DYNAMIC_MODULE_REGISTRY.lock();
                let module = registry.get_mut(module_id).ok_or_else(|| {
                    VmError::RuntimeError(format!("Module {} not found", module_id))
                })?;
                module.seal()?;
                Value::null()
            }

            reflect::MODULE_LINK => {
                // Stub: full import resolution not yet implemented
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "link requires 1 argument (moduleId)".to_string(),
                    ));
                }
                Value::null()
            }

            _ => {
                return Err(VmError::RuntimeError(format!(
                    "Reflect method {:#06x} not yet implemented",
                    method_id
                )));
            }
        };

        stack.push(result)?;
        Ok(())
    }

    /// Helper: Inspect a value recursively with depth limit
    fn inspect_value(
        &self,
        value: Value,
        depth: usize,
        max_depth: usize,
    ) -> Result<String, VmError> {
        if depth > max_depth {
            return Ok("...".to_string());
        }

        if value.is_null() {
            return Ok("null".to_string());
        }

        if let Some(b) = value.as_bool() {
            return Ok(if b { "true" } else { "false" }.to_string());
        }

        if let Some(i) = value.as_i32() {
            return Ok(i.to_string());
        }

        if let Some(f) = value.as_f64() {
            return Ok(f.to_string());
        }

        if !value.is_ptr() {
            return Ok("<unknown>".to_string());
        }

        // String
        if let Some(ptr) = unsafe { value.as_ptr::<RayaString>() } {
            let s = unsafe { &*ptr.as_ptr() };
            return Ok(format!(
                "\"{}\"",
                s.data.replace('\\', "\\\\").replace('"', "\\\"")
            ));
        }

        // Array
        if let Some(ptr) = unsafe { value.as_ptr::<Array>() } {
            let arr = unsafe { &*ptr.as_ptr() };
            if depth >= max_depth {
                return Ok(format!("[Array({})]", arr.len()));
            }
            let mut items = Vec::new();
            for i in 0..arr.len().min(10) {
                items.push(self.inspect_value(
                    arr.get(i).unwrap_or(Value::null()),
                    depth + 1,
                    max_depth,
                )?);
            }
            if arr.len() > 10 {
                items.push(format!("... {} more", arr.len() - 10));
            }
            return Ok(format!("[{}]", items.join(", ")));
        }

        // Object
        if let Some(ptr) = Self::reflect_object_ptr(value) {
            let obj = unsafe { ptr.as_ref() };
            let class_name = self.reflect_object_class_name(obj);

            if depth >= max_depth {
                return Ok(format!("{} {{}}", class_name));
            }

            let mut fields = Vec::new();
            for name in self.reflect_object_field_names(obj) {
                if let Some(field_val) = self.reflect_property_value(value, &name) {
                    let val_str = self.inspect_value(field_val, depth + 1, max_depth)?;
                    fields.push(format!("{}: {}", name, val_str));
                }
            }
            return Ok(format!("{} {{ {} }}", class_name, fields.join(", ")));
        }

        Ok("<ptr>".to_string())
    }

    /// Helper: Calculate retained size by traversing references
    fn calculate_retained_size(
        &self,
        value: Value,
        visited: &mut std::collections::HashSet<usize>,
    ) -> usize {
        if !value.is_ptr() || value.is_null() {
            return 8; // primitive size
        }

        // Get object ID for cycle detection
        let obj_id = if let Some(ptr) = unsafe { value.as_ptr::<u8>() } {
            ptr.as_ptr() as usize
        } else {
            return 8;
        };

        // Already visited - don't count again
        if visited.contains(&obj_id) {
            return 0;
        }
        visited.insert(obj_id);

        // Calculate size based on type
        if let Some(ptr) = unsafe { value.as_ptr::<Object>() } {
            let obj = unsafe { &*ptr.as_ptr() };
            let mut size = std::mem::size_of::<Object>() + obj.fields.len() * 8;
            // Add retained size of referenced objects
            for &field in &obj.fields {
                size += self.calculate_retained_size(field, visited);
            }
            return size;
        }

        if let Some(ptr) = unsafe { value.as_ptr::<Array>() } {
            let arr = unsafe { &*ptr.as_ptr() };
            let mut size = std::mem::size_of::<Array>() + arr.len() * 8;
            // Add retained size of elements
            for i in 0..arr.len() {
                if let Some(elem) = arr.get(i) {
                    size += self.calculate_retained_size(elem, visited);
                }
            }
            return size;
        }

        if let Some(ptr) = unsafe { value.as_ptr::<RayaString>() } {
            let s = unsafe { &*ptr.as_ptr() };
            return std::mem::size_of::<RayaString>() + s.data.len();
        }

        8 // default
    }

    /// Helper: Collect direct references from an object
    fn collect_references(&self, value: Value, refs: &mut Vec<Value>) {
        if !value.is_ptr() || value.is_null() {
            return;
        }

        if let Some(ptr) = unsafe { value.as_ptr::<Object>() } {
            let obj = unsafe { &*ptr.as_ptr() };
            for &field in &obj.fields {
                if field.is_ptr() && !field.is_null() {
                    refs.push(field);
                }
            }
        } else if let Some(ptr) = unsafe { value.as_ptr::<Array>() } {
            let arr = unsafe { &*ptr.as_ptr() };
            for i in 0..arr.len() {
                if let Some(elem) = arr.get(i) {
                    if elem.is_ptr() && !elem.is_null() {
                        refs.push(elem);
                    }
                }
            }
        }
    }

    /// Helper: Convert value to JSON string
    fn value_to_json(&self, value: Value, visited: &mut Vec<usize>) -> Result<String, VmError> {
        if value.is_null() {
            return Ok("null".to_string());
        }

        if let Some(b) = value.as_bool() {
            return Ok(if b { "true" } else { "false" }.to_string());
        }

        if let Some(i) = value.as_i32() {
            return Ok(i.to_string());
        }

        if let Some(f) = value.as_f64() {
            if f.is_nan() || f.is_infinite() {
                return Ok("null".to_string());
            }
            return Ok(f.to_string());
        }

        if !value.is_ptr() {
            return Ok("null".to_string());
        }

        // Check for circular reference
        let obj_id = if let Some(ptr) = unsafe { value.as_ptr::<u8>() } {
            ptr.as_ptr() as usize
        } else {
            0
        };

        if obj_id != 0 && visited.contains(&obj_id) {
            return Ok("\"[Circular]\"".to_string());
        }
        visited.push(obj_id);

        // String
        if let Some(ptr) = unsafe { value.as_ptr::<RayaString>() } {
            let s = unsafe { &*ptr.as_ptr() };
            visited.pop();
            return Ok(format!(
                "\"{}\"",
                s.data
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
                    .replace('\r', "\\r")
                    .replace('\t', "\\t")
            ));
        }

        // Array
        if let Some(ptr) = unsafe { value.as_ptr::<Array>() } {
            let arr = unsafe { &*ptr.as_ptr() };
            let mut items = Vec::new();
            for i in 0..arr.len() {
                if let Some(elem) = arr.get(i) {
                    items.push(self.value_to_json(elem, visited)?);
                }
            }
            visited.pop();
            return Ok(format!("[{}]", items.join(",")));
        }

        // Object
        if let Some(ptr) = Self::reflect_object_ptr(value) {
            let obj = unsafe { ptr.as_ref() };
            let mut fields = Vec::new();
            for name in self.reflect_object_field_names(obj) {
                if let Some(field_val) = self.reflect_property_value(value, &name) {
                    let val_json = self.value_to_json(field_val, visited)?;
                    fields.push(format!("\"{}\":{}", name, val_json));
                }
            }
            visited.pop();
            return Ok(format!("{{{}}}", fields.join(",")));
        }

        visited.pop();
        Ok("null".to_string())
    }

    /// Helper: Check for circular references
    fn check_circular(&self, value: Value, visited: &mut Vec<usize>) -> bool {
        if !value.is_ptr() || value.is_null() {
            return false;
        }

        let obj_id = if let Some(ptr) = unsafe { value.as_ptr::<u8>() } {
            ptr.as_ptr() as usize
        } else {
            return false;
        };

        // Found a cycle
        if visited.contains(&obj_id) {
            return true;
        }
        visited.push(obj_id);

        // Check Object fields
        if let Some(ptr) = unsafe { value.as_ptr::<Object>() } {
            let obj = unsafe { &*ptr.as_ptr() };
            for &field in &obj.fields {
                if self.check_circular(field, visited) {
                    return true;
                }
            }
        }

        // Check Array elements
        if let Some(ptr) = unsafe { value.as_ptr::<Array>() } {
            let arr = unsafe { &*ptr.as_ptr() };
            for i in 0..arr.len() {
                if let Some(elem) = arr.get(i) {
                    if self.check_circular(elem, visited) {
                        return true;
                    }
                }
            }
        }

        visited.pop();
        false
    }

    /// Helper: Convert ObjectSnapshot to a Raya Value (Object)
    #[allow(unused_must_use)]
    fn snapshot_to_value(&self, snapshot: &ObjectSnapshot) -> Value {
        // Create an object with snapshot fields:
        // - class_name: string
        // - identity: number
        // - timestamp: number
        // - fields: object mapping field names to values
        let mut obj = Object::new_structural(
            crate::vm::object::layout_id_from_ordered_names(&[
                "class_name".to_string(),
                "identity".to_string(),
                "timestamp".to_string(),
                "fields".to_string(),
            ]),
            4,
        );

        // Store class_name
        let class_name_str = RayaString::new(snapshot.class_name.clone());
        let class_name_ptr = self.gc.lock().allocate(class_name_str);
        let class_name_val =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(class_name_ptr.as_ptr()).unwrap()) };
        obj.set_field(0, class_name_val);

        // Store identity
        obj.set_field(1, Value::i32(snapshot.identity as i32));

        // Store timestamp
        obj.set_field(2, Value::i32(snapshot.timestamp as i32));

        // Create fields object
        let fields_obj = self.snapshot_fields_to_value(&snapshot.fields);
        obj.set_field(3, fields_obj);

        let obj_ptr = self.gc.lock().allocate(obj);
        unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) }
    }

    /// Helper: Convert snapshot fields HashMap to a Raya Value (Object)
    #[allow(unused_must_use)]
    fn snapshot_fields_to_value(
        &self,
        fields: &std::collections::HashMap<String, crate::vm::reflect::FieldSnapshot>,
    ) -> Value {
        // Create an object with field count matching the number of fields
        let field_count = fields.len();
        // Sort fields by name for consistent ordering
        let mut field_names: Vec<_> = fields.keys().collect();
        field_names.sort();
        let ordered_names: Vec<String> = field_names.iter().map(|name| (*name).clone()).collect();
        let mut obj = Object::new_structural(
            crate::vm::object::layout_id_from_ordered_names(&ordered_names),
            field_count,
        );

        for (i, name) in field_names.iter().enumerate() {
            if let Some(field) = fields.get(*name) {
                // Create a field info object with: name, value, type_name
                let mut field_obj = Object::new_structural(
                    crate::vm::object::layout_id_from_ordered_names(&[
                        "name".to_string(),
                        "value".to_string(),
                        "type_name".to_string(),
                    ]),
                    3,
                );

                // Field name
                let name_str = RayaString::new(field.name.clone());
                let name_ptr = self.gc.lock().allocate(name_str);
                let name_val =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(name_ptr.as_ptr()).unwrap()) };
                field_obj.set_field(0, name_val);

                // Field value (converted from SnapshotValue)
                let val = self.snapshot_value_to_value(&field.value);
                field_obj.set_field(1, val);

                // Type name
                let type_str = RayaString::new(field.type_name.clone());
                let type_ptr = self.gc.lock().allocate(type_str);
                let type_val =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(type_ptr.as_ptr()).unwrap()) };
                field_obj.set_field(2, type_val);

                let field_ptr = self.gc.lock().allocate(field_obj);
                obj.set_field(i, unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(field_ptr.as_ptr()).unwrap())
                });
            }
        }

        let obj_ptr = self.gc.lock().allocate(obj);
        unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) }
    }

    /// Helper: Convert SnapshotValue to a Raya Value
    fn snapshot_value_to_value(&self, snapshot_val: &SnapshotValue) -> Value {
        match snapshot_val {
            SnapshotValue::Null => Value::null(),
            SnapshotValue::Boolean(b) => Value::bool(*b),
            SnapshotValue::Integer(i) => Value::i32(*i),
            SnapshotValue::Float(f) => Value::f64(*f),
            SnapshotValue::String(s) => {
                let raya_str = RayaString::new(s.clone());
                let str_ptr = self.gc.lock().allocate(raya_str);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(str_ptr.as_ptr()).unwrap()) }
            }
            SnapshotValue::ObjectRef(id) => {
                // Return the object ID as an integer for reference tracking
                Value::i32(*id as i32)
            }
            SnapshotValue::Array(elements) => {
                let mut arr = Array::new(0, elements.len());
                for elem in elements {
                    arr.push(self.snapshot_value_to_value(elem));
                }
                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }
            SnapshotValue::Object(nested_snapshot) => {
                // Recursively convert nested snapshot
                self.snapshot_to_value(nested_snapshot)
            }
        }
    }

    /// Helper: Convert ObjectDiff to a Raya Value (Object)
    #[allow(unused_must_use)]
    fn diff_to_value(&self, diff: &ObjectDiff) -> Value {
        // Create an object with diff fields:
        // - added: string[] (field names added)
        // - removed: string[] (field names removed)
        // - changed: object mapping field name to { old, new }
        let mut obj = Object::new_structural(
            crate::vm::object::layout_id_from_ordered_names(&[
                "added".to_string(),
                "removed".to_string(),
                "changed".to_string(),
            ]),
            3,
        );

        // Create added array
        let mut added_arr = Array::new(0, diff.added.len());
        for name in &diff.added {
            let name_str = RayaString::new(name.clone());
            let name_ptr = self.gc.lock().allocate(name_str);
            added_arr.push(unsafe {
                Value::from_ptr(std::ptr::NonNull::new(name_ptr.as_ptr()).unwrap())
            });
        }
        let added_ptr = self.gc.lock().allocate(added_arr);
        obj.set_field(0, unsafe {
            Value::from_ptr(std::ptr::NonNull::new(added_ptr.as_ptr()).unwrap())
        });

        // Create removed array
        let mut removed_arr = Array::new(0, diff.removed.len());
        for name in &diff.removed {
            let name_str = RayaString::new(name.clone());
            let name_ptr = self.gc.lock().allocate(name_str);
            removed_arr.push(unsafe {
                Value::from_ptr(std::ptr::NonNull::new(name_ptr.as_ptr()).unwrap())
            });
        }
        let removed_ptr = self.gc.lock().allocate(removed_arr);
        obj.set_field(1, unsafe {
            Value::from_ptr(std::ptr::NonNull::new(removed_ptr.as_ptr()).unwrap())
        });

        // Create changed object
        let changed_obj = self.diff_changes_to_value(&diff.changed);
        obj.set_field(2, changed_obj);

        let obj_ptr = self.gc.lock().allocate(obj);
        unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) }
    }

    /// Helper: Convert diff changes HashMap to a Raya Value (Object)
    #[allow(unused_must_use)]
    fn diff_changes_to_value(
        &self,
        changes: &std::collections::HashMap<String, crate::vm::reflect::ValueChange>,
    ) -> Value {
        let change_count = changes.len();
        // Sort changes by name for consistent ordering
        let mut change_names: Vec<_> = changes.keys().collect();
        change_names.sort();
        let ordered_names: Vec<String> = change_names.iter().map(|name| (*name).clone()).collect();
        let mut obj = Object::new_structural(
            crate::vm::object::layout_id_from_ordered_names(&ordered_names),
            change_count,
        );

        for (i, name) in change_names.iter().enumerate() {
            if let Some(change) = changes.get(*name) {
                // Create a change object with: fieldName, old, new
                let mut change_obj = Object::new_structural(
                    crate::vm::object::layout_id_from_ordered_names(&[
                        "fieldName".to_string(),
                        "old".to_string(),
                        "new".to_string(),
                    ]),
                    3,
                );

                // Field name
                let name_str = RayaString::new((*name).clone());
                let name_ptr = self.gc.lock().allocate(name_str);
                change_obj.set_field(0, unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(name_ptr.as_ptr()).unwrap())
                });

                // Old value
                let old_val = self.snapshot_value_to_value(&change.old);
                change_obj.set_field(1, old_val);

                // New value
                let new_val = self.snapshot_value_to_value(&change.new);
                change_obj.set_field(2, new_val);

                let change_ptr = self.gc.lock().allocate(change_obj);
                obj.set_field(i, unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(change_ptr.as_ptr()).unwrap())
                });
            }
        }

        let obj_ptr = self.gc.lock().allocate(obj);
        unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) }
    }
}
