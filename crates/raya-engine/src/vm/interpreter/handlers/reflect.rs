//! Reflect built-in method handlers and helpers

use crate::compiler::Module;
use crate::vm::interpreter::Interpreter;
use crate::vm::interpreter::core::value_to_f64;
use crate::vm::object::{Array, Closure, MapObject, Object, RayaString};
use crate::vm::reflect::{ObjectDiff, ObjectSnapshot, SnapshotContext, SnapshotValue};
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

impl<'a> Interpreter<'a> {
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

        let result = match method_id {
            reflect::DEFINE_METADATA => {
                // defineMetadata(key, value, target)
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "defineMetadata requires 3 arguments".to_string()
                    ));
                }
                let key = get_string(args[0].clone())?;
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
                        "defineMetadata with property requires 4 arguments".to_string()
                    ));
                }
                let key = get_string(args[0].clone())?;
                let value = args[1];
                let target = args[2];
                let property_key = get_string(args[3].clone())?;

                let mut metadata = self.metadata.lock();
                metadata.define_metadata_property(key, value, target, property_key);
                Value::null()
            }

            reflect::GET_METADATA => {
                // getMetadata(key, target)
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "getMetadata requires 2 arguments".to_string()
                    ));
                }
                let key = get_string(args[0].clone())?;
                let target = args[1];

                let metadata = self.metadata.lock();
                metadata.get_metadata(&key, target).unwrap_or(Value::null())
            }

            reflect::GET_METADATA_PROP => {
                // getMetadata(key, target, propertyKey)
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "getMetadata with property requires 3 arguments".to_string()
                    ));
                }
                let key = get_string(args[0].clone())?;
                let target = args[1];
                let property_key = get_string(args[2].clone())?;

                let metadata = self.metadata.lock();
                metadata.get_metadata_property(&key, target, &property_key).unwrap_or(Value::null())
            }

            reflect::HAS_METADATA => {
                // hasMetadata(key, target)
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "hasMetadata requires 2 arguments".to_string()
                    ));
                }
                let key = get_string(args[0].clone())?;
                let target = args[1];

                let metadata = self.metadata.lock();
                Value::bool(metadata.has_metadata(&key, target))
            }

            reflect::HAS_METADATA_PROP => {
                // hasMetadata(key, target, propertyKey)
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "hasMetadata with property requires 3 arguments".to_string()
                    ));
                }
                let key = get_string(args[0].clone())?;
                let target = args[1];
                let property_key = get_string(args[2].clone())?;

                let metadata = self.metadata.lock();
                Value::bool(metadata.has_metadata_property(&key, target, &property_key))
            }

            reflect::GET_METADATA_KEYS => {
                // getMetadataKeys(target)
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getMetadataKeys requires 1 argument".to_string()
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
                        "getMetadataKeys with property requires 2 arguments".to_string()
                    ));
                }
                let target = args[0];
                let property_key = get_string(args[1].clone())?;

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
                        "deleteMetadata requires 2 arguments".to_string()
                    ));
                }
                let key = get_string(args[0].clone())?;
                let target = args[1];

                let mut metadata = self.metadata.lock();
                Value::bool(metadata.delete_metadata(&key, target))
            }

            reflect::DELETE_METADATA_PROP => {
                // deleteMetadata(key, target, propertyKey)
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "deleteMetadata with property requires 3 arguments".to_string()
                    ));
                }
                let key = get_string(args[0].clone())?;
                let target = args[1];
                let property_key = get_string(args[2].clone())?;

                let mut metadata = self.metadata.lock();
                Value::bool(metadata.delete_metadata_property(&key, target, &property_key))
            }

            // ===== Phase 2: Class Introspection =====

            reflect::GET_CLASS => {
                // getClass(obj) -> returns class ID as i32, or null if not an object
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getClass requires 1 argument".to_string()
                    ));
                }
                let obj = args[0];
                if let Some(class_id) = crate::vm::reflect::get_class_id(obj) {
                    Value::i32(class_id as i32)
                } else {
                    Value::null()
                }
            }

            reflect::GET_CLASS_BY_NAME => {
                // getClassByName(name) -> returns class ID as i32, or null if not found
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getClassByName requires 1 argument".to_string()
                    ));
                }
                let name = get_string(args[0].clone())?;
                let classes = self.classes.read();
                if let Some(class) = classes.get_class_by_name(&name) {
                    Value::i32(class.id as i32)
                } else {
                    Value::null()
                }
            }

            reflect::GET_ALL_CLASSES => {
                // getAllClasses() -> returns array of class IDs
                let classes = self.classes.read();
                let class_ids: Vec<Value> = classes
                    .iter()
                    .map(|(id, _)| Value::i32(id as i32))
                    .collect();

                let mut arr = Array::new(0, class_ids.len());
                for (i, val) in class_ids.into_iter().enumerate() {
                    arr.set(i, val).ok();
                }
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            reflect::GET_CLASSES_WITH_DECORATOR => {
                // getClassesWithDecorator(decorator) -> returns array of class IDs
                // NOTE: This requires --emit-reflection to work fully
                // For now, returns empty array (decorator metadata not yet stored)
                let arr = Array::new(0, 0);
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            reflect::IS_SUBCLASS_OF => {
                // isSubclassOf(subClassId, superClassId) -> boolean
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "isSubclassOf requires 2 arguments".to_string()
                    ));
                }
                let sub_id = args[0].as_i32().unwrap_or(-1);
                let super_id = args[1].as_i32().unwrap_or(-1);

                if sub_id < 0 || super_id < 0 {
                    Value::bool(false)
                } else {
                    let classes = self.classes.read();
                    Value::bool(crate::vm::reflect::is_subclass_of(
                        &classes,
                        sub_id as usize,
                        super_id as usize,
                    ))
                }
            }

            reflect::IS_INSTANCE_OF => {
                // isInstanceOf(obj, classId) -> boolean
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "isInstanceOf requires 2 arguments".to_string()
                    ));
                }
                let obj = args[0];
                let class_id = args[1].as_i32().unwrap_or(-1);

                if class_id < 0 {
                    Value::bool(false)
                } else {
                    let classes = self.classes.read();
                    Value::bool(crate::vm::reflect::is_instance_of(
                        &classes,
                        obj,
                        class_id as usize,
                    ))
                }
            }

            reflect::GET_TYPE_INFO => {
                // getTypeInfo(target) -> returns type kind as string
                // NOTE: Full TypeInfo requires --emit-reflection
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getTypeInfo requires 1 argument".to_string()
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
                // getClassHierarchy(obj) -> returns array of class IDs from obj's class to root
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getClassHierarchy requires 1 argument".to_string()
                    ));
                }
                let obj = args[0];

                if let Some(class_id) = crate::vm::reflect::get_class_id(obj) {
                    let classes = self.classes.read();
                    let hierarchy = crate::vm::reflect::get_class_hierarchy(&classes, class_id);

                    let class_ids: Vec<Value> = hierarchy
                        .iter()
                        .map(|c| Value::i32(c.id as i32))
                        .collect();

                    drop(classes);

                    let mut arr = Array::new(0, class_ids.len());
                    for (i, val) in class_ids.into_iter().enumerate() {
                        arr.set(i, val).ok();
                    }
                    let arr_gc = self.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
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
                        "get requires 2 arguments (target, propertyKey)".to_string()
                    ));
                }
                let target = args[0];
                let property_key = get_string(args[1].clone())?;

                if !target.is_ptr() {
                    return Err(VmError::TypeError("get: target must be an object".to_string()));
                }

                // Get class ID from object
                let class_id = crate::vm::reflect::get_class_id(target)
                    .ok_or_else(|| VmError::TypeError("get: target is not a class instance".to_string()))?;

                // Look up field index from class metadata
                let class_metadata = self.class_metadata.read();
                let field_index = class_metadata.get(class_id)
                    .and_then(|meta| meta.get_field_index(&property_key));
                drop(class_metadata);

                if let Some(index) = field_index {
                    let obj_ptr = unsafe { target.as_ptr::<Object>() };
                    let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                    obj.get_field(index).unwrap_or(Value::null())
                } else {
                    // Field not found in metadata - return null
                    Value::null()
                }
            }

            reflect::SET => {
                // set(target, propertyKey, value) -> set field value by name
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "set requires 3 arguments (target, propertyKey, value)".to_string()
                    ));
                }
                let target = args[0];
                let property_key = get_string(args[1].clone())?;
                let value = args[2];

                if !target.is_ptr() {
                    return Err(VmError::TypeError("set: target must be an object".to_string()));
                }

                // Get class ID from object
                let class_id = crate::vm::reflect::get_class_id(target)
                    .ok_or_else(|| VmError::TypeError("set: target is not a class instance".to_string()))?;

                // Look up field index from class metadata
                let class_metadata = self.class_metadata.read();
                let field_index = class_metadata.get(class_id)
                    .and_then(|meta| meta.get_field_index(&property_key));
                drop(class_metadata);

                if let Some(index) = field_index {
                    let obj_ptr = unsafe { target.as_ptr::<Object>() };
                    let obj = unsafe { &mut *obj_ptr.unwrap().as_ptr() };
                    match obj.set_field(index, value) {
                        Ok(()) => Value::bool(true),
                        Err(_) => Value::bool(false),
                    }
                } else {
                    // Field not found in metadata
                    Value::bool(false)
                }
            }

            reflect::HAS => {
                // has(target, propertyKey) -> check if field exists
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "has requires 2 arguments (target, propertyKey)".to_string()
                    ));
                }
                let target = args[0];
                let property_key = get_string(args[1].clone())?;

                if !target.is_ptr() {
                    Value::bool(false)
                } else if let Some(class_id) = crate::vm::reflect::get_class_id(target) {
                    let class_metadata = self.class_metadata.read();
                    let has_field = class_metadata.get(class_id)
                        .map(|meta| meta.has_field(&property_key))
                        .unwrap_or(false);
                    Value::bool(has_field)
                } else {
                    Value::bool(false)
                }
            }

            reflect::GET_FIELD_NAMES => {
                // getFieldNames(target) -> list all field names
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getFieldNames requires 1 argument".to_string()
                    ));
                }
                let target = args[0];

                let field_names = if let Some(class_id) = crate::vm::reflect::get_class_id(target) {
                    let class_metadata = self.class_metadata.read();
                    class_metadata.get(class_id)
                        .map(|meta| meta.field_names.clone())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };

                // Create array of strings
                let mut arr = Array::new(0, field_names.len());
                for (i, name) in field_names.into_iter().enumerate() {
                    if !name.is_empty() {
                        let s = RayaString::new(name);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
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
                        "getFieldInfo requires 2 arguments (target, propertyKey)".to_string()
                    ));
                }
                let target = args[0];
                let property_key = get_string(args[1].clone())?;

                if !target.is_ptr() {
                    Value::null()
                } else if let Some(class_id) = crate::vm::reflect::get_class_id(target) {
                    let class_metadata = self.class_metadata.read();
                    if let Some(meta) = class_metadata.get(class_id) {
                        if let Some(field_info) = meta.get_field_info(&property_key) {
                            // Create a MapObject with field info properties
                            let mut map = MapObject::new();

                            // Add field properties
                            let name_str = RayaString::new(field_info.name.clone());
                            let name_gc = self.gc.lock().allocate(name_str);
                            let name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(name_gc.as_ptr()).unwrap()) };

                            let type_str = RayaString::new(field_info.type_info.name.clone());
                            let type_gc = self.gc.lock().allocate(type_str);
                            let type_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(type_gc.as_ptr()).unwrap()) };

                            let key_name = RayaString::new("name".to_string());
                            let key_name_gc = self.gc.lock().allocate(key_name);
                            let key_name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_name_gc.as_ptr()).unwrap()) };
                            map.set(key_name_val, name_val);

                            let key_type = RayaString::new("type".to_string());
                            let key_type_gc = self.gc.lock().allocate(key_type);
                            let key_type_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_type_gc.as_ptr()).unwrap()) };
                            map.set(key_type_val, type_val);

                            let key_index = RayaString::new("index".to_string());
                            let key_index_gc = self.gc.lock().allocate(key_index);
                            let key_index_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_index_gc.as_ptr()).unwrap()) };
                            map.set(key_index_val, Value::i32(field_info.field_index as i32));

                            let key_static = RayaString::new("isStatic".to_string());
                            let key_static_gc = self.gc.lock().allocate(key_static);
                            let key_static_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_static_gc.as_ptr()).unwrap()) };
                            map.set(key_static_val, Value::bool(field_info.is_static));

                            let key_readonly = RayaString::new("isReadonly".to_string());
                            let key_readonly_gc = self.gc.lock().allocate(key_readonly);
                            let key_readonly_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_readonly_gc.as_ptr()).unwrap()) };
                            map.set(key_readonly_val, Value::bool(field_info.is_readonly));

                            let key_class = RayaString::new("declaringClass".to_string());
                            let key_class_gc = self.gc.lock().allocate(key_class);
                            let key_class_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_class_gc.as_ptr()).unwrap()) };
                            map.set(key_class_val, Value::i32(field_info.declaring_class_id as i32));

                            let map_gc = self.gc.lock().allocate(map);
                            unsafe { Value::from_ptr(std::ptr::NonNull::new(map_gc.as_ptr()).unwrap()) }
                        } else {
                            Value::null()
                        }
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
                        "getFields requires 1 argument (target)".to_string()
                    ));
                }
                let target = args[0];

                if !target.is_ptr() {
                    let arr = Array::new(0, 0);
                    let arr_gc = self.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                } else if let Some(class_id) = crate::vm::reflect::get_class_id(target) {
                    let class_metadata = self.class_metadata.read();
                    if let Some(meta) = class_metadata.get(class_id) {
                        let fields = meta.get_all_field_infos();
                        let mut arr = Array::new(fields.len(), 0);

                        for (i, field_info) in fields.iter().enumerate() {
                            // Create a MapObject for each field
                            let mut map = MapObject::new();

                            let key_name = RayaString::new("name".to_string());
                            let key_name_gc = self.gc.lock().allocate(key_name);
                            let key_name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_name_gc.as_ptr()).unwrap()) };

                            let name_str = RayaString::new(field_info.name.clone());
                            let name_gc = self.gc.lock().allocate(name_str);
                            let name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(name_gc.as_ptr()).unwrap()) };
                            map.set(key_name_val, name_val);

                            let key_type = RayaString::new("type".to_string());
                            let key_type_gc = self.gc.lock().allocate(key_type);
                            let key_type_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_type_gc.as_ptr()).unwrap()) };

                            let type_str = RayaString::new(field_info.type_info.name.clone());
                            let type_gc = self.gc.lock().allocate(type_str);
                            let type_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(type_gc.as_ptr()).unwrap()) };
                            map.set(key_type_val, type_val);

                            let key_index = RayaString::new("index".to_string());
                            let key_index_gc = self.gc.lock().allocate(key_index);
                            let key_index_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_index_gc.as_ptr()).unwrap()) };
                            map.set(key_index_val, Value::i32(field_info.field_index as i32));

                            let key_static = RayaString::new("isStatic".to_string());
                            let key_static_gc = self.gc.lock().allocate(key_static);
                            let key_static_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_static_gc.as_ptr()).unwrap()) };
                            map.set(key_static_val, Value::bool(field_info.is_static));

                            let key_readonly = RayaString::new("isReadonly".to_string());
                            let key_readonly_gc = self.gc.lock().allocate(key_readonly);
                            let key_readonly_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_readonly_gc.as_ptr()).unwrap()) };
                            map.set(key_readonly_val, Value::bool(field_info.is_readonly));

                            let key_class = RayaString::new("declaringClass".to_string());
                            let key_class_gc = self.gc.lock().allocate(key_class);
                            let key_class_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_class_gc.as_ptr()).unwrap()) };
                            map.set(key_class_val, Value::i32(field_info.declaring_class_id as i32));

                            let map_gc = self.gc.lock().allocate(map);
                            let map_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(map_gc.as_ptr()).unwrap()) };
                            arr.set(i, map_val).ok();
                        }

                        let arr_gc = self.gc.lock().allocate(arr);
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                    } else {
                        let arr = Array::new(0, 0);
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
                // getStaticFieldNames(classId) -> get static field names as array
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "getStaticFieldNames requires 1 argument (classId)".to_string()
                    ));
                }
                let class_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("getStaticFieldNames: classId must be a number".to_string()))?
                    as usize;

                let class_metadata = self.class_metadata.read();
                if let Some(meta) = class_metadata.get(class_id) {
                    let names = &meta.static_field_names;
                    let mut arr = Array::new(names.len(), 0);
                    for (i, name) in names.iter().enumerate() {
                        if !name.is_empty() {
                            let s = RayaString::new(name.clone());
                            let gc_ptr = self.gc.lock().allocate(s);
                            let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
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
                // getStaticFields(classId) -> get static field infos (stub for now)
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
                        "hasMethod requires 2 arguments (target, methodName)".to_string()
                    ));
                }
                let target = args[0];
                let method_name = get_string(args[1].clone())?;

                if !target.is_ptr() {
                    Value::bool(false)
                } else if let Some(class_id) = crate::vm::reflect::get_class_id(target) {
                    let class_metadata = self.class_metadata.read();
                    let has_method = class_metadata.get(class_id)
                        .map(|meta| meta.has_method(&method_name))
                        .unwrap_or(false);
                    Value::bool(has_method)
                } else {
                    Value::bool(false)
                }
            }

            reflect::GET_METHODS | reflect::GET_METHOD | reflect::GET_METHOD_INFO |
            reflect::INVOKE | reflect::INVOKE_ASYNC | reflect::INVOKE_STATIC |
            reflect::GET_STATIC_METHODS => {
                // These require full --emit-reflection metadata and dynamic dispatch
                // Return null/empty for now
                match method_id {
                    reflect::INVOKE | reflect::INVOKE_ASYNC | reflect::INVOKE_STATIC => {
                        return Err(VmError::RuntimeError(
                            "Dynamic method invocation requires --emit-reflection".to_string()
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
                // construct(classId, ...args) -> create instance
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "construct requires at least 1 argument (classId)".to_string()
                    ));
                }
                let class_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("construct: classId must be a number".to_string()))?
                    as usize;

                let classes = self.classes.read();
                let class = classes.get_class(class_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("Class {} not found", class_id)))?;
                let field_count = class.field_count;
                drop(classes);

                // Allocate new object
                let obj = Object::new(class_id, field_count);
                let gc_ptr = self.gc.lock().allocate(obj);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }

                // Note: Constructor call with args requires more work (call constructor function)
            }

            reflect::ALLOCATE => {
                // allocate(classId) -> allocate uninitialized instance
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "allocate requires 1 argument (classId)".to_string()
                    ));
                }
                let class_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("allocate: classId must be a number".to_string()))?
                    as usize;

                let classes = self.classes.read();
                let class = classes.get_class(class_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("Class {} not found", class_id)))?;
                let field_count = class.field_count;
                drop(classes);

                // Allocate new object (uninitialized - fields are null)
                let obj = Object::new(class_id, field_count);
                let gc_ptr = self.gc.lock().allocate(obj);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
            }

            reflect::CLONE => {
                // clone(obj) -> shallow clone
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "clone requires 1 argument".to_string()
                    ));
                }
                let target = args[0];

                if !target.is_ptr() {
                    // Primitives are copied by value
                    target
                } else if let Some(_class_id) = crate::vm::reflect::get_class_id(target) {
                    // Clone object
                    let obj_ptr = unsafe { target.as_ptr::<Object>() };
                    let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
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
                            "constructWith requires --emit-reflection".to_string()
                        ));
                    }
                    reflect::DEEP_CLONE => {
                        return Err(VmError::RuntimeError(
                            "deepClone not yet implemented".to_string()
                        ));
                    }
                    _ => Value::null()
                }
            }

            // ===== Phase 6: Type Utilities =====

            reflect::IS_STRING => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError("isString requires 1 argument".to_string()));
                }
                let value = args[0];
                let is_string = value.is_ptr() && unsafe { value.as_ptr::<RayaString>().is_some() };
                Value::bool(is_string)
            }

            reflect::IS_NUMBER => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError("isNumber requires 1 argument".to_string()));
                }
                let value = args[0];
                let is_number = value.as_f64().is_some() || value.as_i32().is_some();
                Value::bool(is_number)
            }

            reflect::IS_BOOLEAN => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError("isBoolean requires 1 argument".to_string()));
                }
                let value = args[0];
                let is_bool = value.as_bool().is_some();
                Value::bool(is_bool)
            }

            reflect::IS_NULL => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError("isNull requires 1 argument".to_string()));
                }
                let value = args[0];
                Value::bool(value.is_null())
            }

            reflect::IS_ARRAY => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError("isArray requires 1 argument".to_string()));
                }
                let value = args[0];
                let is_array = value.is_ptr() && unsafe { value.as_ptr::<Array>().is_some() };
                Value::bool(is_array)
            }

            reflect::IS_FUNCTION => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError("isFunction requires 1 argument".to_string()));
                }
                let value = args[0];
                let is_func = value.is_ptr() && unsafe { value.as_ptr::<Closure>().is_some() };
                Value::bool(is_func)
            }

            reflect::IS_OBJECT => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError("isObject requires 1 argument".to_string()));
                }
                let value = args[0];
                let is_obj = value.is_ptr() && unsafe { value.as_ptr::<Object>().is_some() };
                Value::bool(is_obj)
            }

            reflect::TYPE_OF => {
                // typeOf(typeName) - get TypeInfo from string
                if args.is_empty() {
                    return Err(VmError::RuntimeError("typeOf requires 1 argument".to_string()));
                }
                let type_name = get_string(args[0].clone())?;

                // Check primitive types
                let (kind, class_id) = match type_name.as_str() {
                    "string" | "number" | "boolean" | "null" | "void" | "any" =>
                        ("primitive".to_string(), None),
                    _ => {
                        // Check if it's a class name
                        let classes = self.classes.read();
                        if let Some(class) = classes.get_class_by_name(&type_name) {
                            ("class".to_string(), Some(class.id))
                        } else {
                            // Unknown type
                            return Ok(stack.push(Value::null())?);
                        }
                    }
                };

                // Return TypeInfo as a Map
                let mut map = MapObject::new();
                let kind_str = RayaString::new(kind);
                let kind_ptr = self.gc.lock().allocate(kind_str);
                let kind_key = RayaString::new("kind".to_string());
                let kind_key_ptr = self.gc.lock().allocate(kind_key);
                map.set(unsafe { Value::from_ptr(std::ptr::NonNull::new(kind_key_ptr.as_ptr()).unwrap()) },
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(kind_ptr.as_ptr()).unwrap()) });

                let name_str = RayaString::new(type_name);
                let name_ptr = self.gc.lock().allocate(name_str);
                let name_key = RayaString::new("name".to_string());
                let name_key_ptr = self.gc.lock().allocate(name_key);
                map.set(unsafe { Value::from_ptr(std::ptr::NonNull::new(name_key_ptr.as_ptr()).unwrap()) },
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(name_ptr.as_ptr()).unwrap()) });

                if let Some(id) = class_id {
                    let id_key = RayaString::new("classId".to_string());
                    let id_key_ptr = self.gc.lock().allocate(id_key);
                    map.set(unsafe { Value::from_ptr(std::ptr::NonNull::new(id_key_ptr.as_ptr()).unwrap()) },
                            Value::i32(id as i32));
                }

                let map_ptr = self.gc.lock().allocate(map);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(map_ptr.as_ptr()).unwrap()) }
            }

            reflect::IS_ASSIGNABLE_TO => {
                // isAssignableTo(sourceType, targetType) - check type compatibility
                if args.len() < 2 {
                    return Err(VmError::RuntimeError("isAssignableTo requires 2 arguments".to_string()));
                }
                let source = get_string(args[0].clone())?;
                let target = get_string(args[1].clone())?;

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
                        let is_subclass = crate::vm::reflect::is_subclass_of(&classes, src.id, tgt.id);
                        Value::bool(is_subclass)
                    } else {
                        Value::bool(false)
                    }
                }
            }

            reflect::CAST => {
                // cast(value, classId) - safe cast, returns null if incompatible
                if args.len() < 2 {
                    return Err(VmError::RuntimeError("cast requires 2 arguments".to_string()));
                }
                let value = args[0];
                let class_id = value_to_f64(args[1])? as usize;

                let classes = self.classes.read();
                if crate::vm::reflect::is_instance_of(&classes, value, class_id) {
                    value
                } else {
                    Value::null()
                }
            }

            reflect::CAST_OR_THROW => {
                // castOrThrow(value, classId) - cast or throw error
                if args.len() < 2 {
                    return Err(VmError::RuntimeError("castOrThrow requires 2 arguments".to_string()));
                }
                let value = args[0];
                let class_id = value_to_f64(args[1])? as usize;

                let classes = self.classes.read();
                if crate::vm::reflect::is_instance_of(&classes, value, class_id) {
                    value
                } else {
                    return Err(VmError::TypeError(format!(
                        "Cannot cast value to class {}",
                        class_id
                    )));
                }
            }

            // ===== Phase 7: Interface and Hierarchy Query =====

            reflect::IMPLEMENTS => {
                // implements(classId, interfaceName) - check if class implements interface
                if args.len() < 2 {
                    return Err(VmError::RuntimeError("implements requires 2 arguments".to_string()));
                }
                let class_id = value_to_f64(args[0])? as usize;
                let interface_name = get_string(args[1].clone())?;

                let class_metadata = self.class_metadata.read();
                if let Some(meta) = class_metadata.get(class_id) {
                    Value::bool(meta.implements_interface(&interface_name))
                } else {
                    Value::bool(false)
                }
            }

            reflect::GET_INTERFACES => {
                // getInterfaces(classId) - get interfaces implemented by class
                if args.is_empty() {
                    return Err(VmError::RuntimeError("getInterfaces requires 1 argument".to_string()));
                }
                let class_id = value_to_f64(args[0])? as usize;

                let class_metadata = self.class_metadata.read();
                let interfaces: Vec<String> = if let Some(meta) = class_metadata.get(class_id) {
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
                    arr.push(unsafe { Value::from_ptr(std::ptr::NonNull::new(s_ptr.as_ptr()).unwrap()) });
                }
                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }

            reflect::GET_SUPERCLASS => {
                // getSuperclass(classId) - get parent class
                if args.is_empty() {
                    return Err(VmError::RuntimeError("getSuperclass requires 1 argument".to_string()));
                }
                let class_id = value_to_f64(args[0])? as usize;

                let classes = self.classes.read();
                if let Some(class) = classes.get_class(class_id) {
                    if let Some(parent) = class.parent_id {
                        Value::i32(parent as i32)
                    } else {
                        Value::null()
                    }
                } else {
                    Value::null()
                }
            }

            reflect::GET_SUBCLASSES => {
                // getSubclasses(classId) - get direct subclasses
                if args.is_empty() {
                    return Err(VmError::RuntimeError("getSubclasses requires 1 argument".to_string()));
                }
                let class_id = value_to_f64(args[0])? as usize;

                let classes = self.classes.read();
                let mut subclasses = Vec::new();
                for (id, class) in classes.iter() {
                    if class.parent_id == Some(class_id) {
                        subclasses.push(id);
                    }
                }
                drop(classes);

                let mut arr = Array::new(0, 0);
                for id in subclasses {
                    arr.push(Value::i32(id as i32));
                }
                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }

            reflect::GET_IMPLEMENTORS => {
                // getImplementors(interfaceName) - get all classes implementing interface
                if args.is_empty() {
                    return Err(VmError::RuntimeError("getImplementors requires 1 argument".to_string()));
                }
                let interface_name = get_string(args[0].clone())?;

                let class_metadata = self.class_metadata.read();
                let implementors = class_metadata.get_implementors(&interface_name);
                drop(class_metadata);

                let mut arr = Array::new(0, 0);
                for id in implementors {
                    arr.push(Value::i32(id as i32));
                }
                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }

            reflect::IS_STRUCTURALLY_COMPATIBLE => {
                // isStructurallyCompatible(sourceClassId, targetClassId) - check structural compatibility
                if args.len() < 2 {
                    return Err(VmError::RuntimeError("isStructurallyCompatible requires 2 arguments".to_string()));
                }
                let source_id = value_to_f64(args[0])? as usize;
                let target_id = value_to_f64(args[1])? as usize;

                let class_metadata = self.class_metadata.read();
                let source_meta = class_metadata.get(source_id);
                let target_meta = class_metadata.get(target_id);

                if let (Some(source), Some(target)) = (source_meta, target_meta) {
                    // Check if source has all fields of target
                    let fields_ok = target.field_names.iter().all(|name| source.has_field(name));
                    // Check if source has all methods of target
                    let methods_ok = target.method_names.iter().all(|name|
                        name.is_empty() || source.has_method(name)
                    );
                    Value::bool(fields_ok && methods_ok)
                } else {
                    Value::bool(false)
                }
            }

            // ===== Phase 8: Object Inspection =====

            reflect::INSPECT => {
                // inspect(obj, depth?) - human-readable representation
                if args.is_empty() {
                    return Err(VmError::RuntimeError("inspect requires 1 argument".to_string()));
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
                    return Err(VmError::RuntimeError("getObjectId requires 1 argument".to_string()));
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
                // describe(classId) - detailed class description
                if args.is_empty() {
                    return Err(VmError::RuntimeError("describe requires 1 argument".to_string()));
                }
                let class_id = value_to_f64(args[0])? as usize;

                let classes = self.classes.read();
                let class = classes.get_class(class_id);
                let class_metadata = self.class_metadata.read();
                let meta = class_metadata.get(class_id);

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

                    desc.push_str("}");
                    desc
                } else {
                    format!("Unknown class {}", class_id)
                };

                let s = RayaString::new(description);
                let s_ptr = self.gc.lock().allocate(s);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(s_ptr.as_ptr()).unwrap()) }
            }

            reflect::SNAPSHOT => {
                // snapshot(obj) - Capture object state as a snapshot
                if args.is_empty() {
                    return Err(VmError::RuntimeError("snapshot requires 1 argument".to_string()));
                }
                let target = args[0];

                // Create snapshot context with max depth of 10
                let mut ctx = SnapshotContext::new(10);

                // Get class name if it's an object
                let (class_name, field_names) = if let Some(ptr) = unsafe { target.as_ptr::<Object>() } {
                    let obj = unsafe { &*ptr.as_ptr() };
                    let class_registry = self.classes.read();
                    if let Some(class) = class_registry.get_class(obj.class_id) {
                        let names: Vec<String> = (0..class.field_count)
                            .map(|i| format!("field_{}", i))
                            .collect();
                        (class.name.clone(), names)
                    } else {
                        (format!("Class{}", obj.class_id), Vec::new())
                    }
                } else {
                    ("unknown".to_string(), Vec::new())
                };

                // Capture the snapshot
                let snapshot = ctx.capture_object_with_names(target, &field_names, &class_name);

                // Convert snapshot to a Raya Object
                self.snapshot_to_value(&snapshot)
            }

            reflect::DIFF => {
                // diff(a, b) - Compare two objects and return differences
                if args.len() < 2 {
                    return Err(VmError::RuntimeError("diff requires 2 arguments".to_string()));
                }
                let obj_a = args[0];
                let obj_b = args[1];

                // Capture both objects as snapshots
                let mut ctx = SnapshotContext::new(10);

                let (class_name_a, field_names_a) = if let Some(ptr) = unsafe { obj_a.as_ptr::<Object>() } {
                    let obj = unsafe { &*ptr.as_ptr() };
                    let class_registry = self.classes.read();
                    if let Some(class) = class_registry.get_class(obj.class_id) {
                        let names: Vec<String> = (0..class.field_count)
                            .map(|i| format!("field_{}", i))
                            .collect();
                        (class.name.clone(), names)
                    } else {
                        (format!("Class{}", obj.class_id), Vec::new())
                    }
                } else {
                    ("unknown".to_string(), Vec::new())
                };

                let (class_name_b, field_names_b) = if let Some(ptr) = unsafe { obj_b.as_ptr::<Object>() } {
                    let obj = unsafe { &*ptr.as_ptr() };
                    let class_registry = self.classes.read();
                    if let Some(class) = class_registry.get_class(obj.class_id) {
                        let names: Vec<String> = (0..class.field_count)
                            .map(|i| format!("field_{}", i))
                            .collect();
                        (class.name.clone(), names)
                    } else {
                        (format!("Class{}", obj.class_id), Vec::new())
                    }
                } else {
                    ("unknown".to_string(), Vec::new())
                };

                let snapshot_a = ctx.capture_object_with_names(obj_a, &field_names_a, &class_name_a);
                let snapshot_b = ctx.capture_object_with_names(obj_b, &field_names_b, &class_name_b);

                // Compute the diff
                let diff = ObjectDiff::compute(&snapshot_a, &snapshot_b);

                // Convert diff to a Raya Object
                self.diff_to_value(&diff)
            }

            // ===== Phase 8: Memory Analysis =====

            reflect::GET_OBJECT_SIZE => {
                // getObjectSize(obj) - shallow memory size
                if args.is_empty() {
                    return Err(VmError::RuntimeError("getObjectSize requires 1 argument".to_string()));
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
                    return Err(VmError::RuntimeError("getRetainedSize requires 1 argument".to_string()));
                }
                let target = args[0];

                let mut visited = std::collections::HashSet::new();
                let size = self.calculate_retained_size(target, &mut visited);
                Value::i32(size as i32)
            }

            reflect::GET_REFERENCES => {
                // getReferences(obj) - objects referenced by this object
                if args.is_empty() {
                    return Err(VmError::RuntimeError("getReferences requires 1 argument".to_string()));
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
                    return Err(VmError::RuntimeError("getReferrers requires 1 argument".to_string()));
                }
                let target = args[0];

                // Get target's identity
                let target_id = if let Some(ptr) = unsafe { target.as_ptr::<u8>() } {
                    ptr.as_ptr() as usize
                } else {
                    return Ok(stack.push(Value::null())?);
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
                                        Value::from_ptr(std::ptr::NonNull::new(obj_ptr as *mut Object).unwrap())
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

                let mut map = MapObject::new();

                // totalObjects
                let key = RayaString::new("totalObjects".to_string());
                let key_ptr = self.gc.lock().allocate(key);
                map.set(unsafe { Value::from_ptr(std::ptr::NonNull::new(key_ptr.as_ptr()).unwrap()) },
                        Value::i32(stats.allocation_count as i32));

                // totalBytes
                let key = RayaString::new("totalBytes".to_string());
                let key_ptr = self.gc.lock().allocate(key);
                map.set(unsafe { Value::from_ptr(std::ptr::NonNull::new(key_ptr.as_ptr()).unwrap()) },
                        Value::i32(stats.allocated_bytes as i32));

                let map_ptr = self.gc.lock().allocate(map);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(map_ptr.as_ptr()).unwrap()) }
            }

            reflect::FIND_INSTANCES => {
                // findInstances(classId) - find all live instances of a class
                if args.is_empty() {
                    return Err(VmError::RuntimeError("findInstances requires 1 argument".to_string()));
                }
                let class_id = value_to_f64(args[0])? as usize;

                let gc = self.gc.lock();
                let mut instances = Vec::new();

                for header_ptr in gc.heap().iter_allocations() {
                    let header = unsafe { &*header_ptr };
                    // Check if this is an Object with matching class_id
                    if header.type_id() == std::any::TypeId::of::<Object>() {
                        let obj_ptr = unsafe { header_ptr.add(1) as *const Object };
                        let obj = unsafe { &*obj_ptr };
                        if obj.class_id == class_id {
                            let value = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(obj_ptr as *mut Object).unwrap())
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
                    let mut frame_map = MapObject::new();

                    // Function name
                    let func_name = module.functions.get(func_id)
                        .map(|f| f.name.clone())
                        .unwrap_or_else(|| format!("<function_{}>", func_id));

                    let name_key = RayaString::new("functionName".to_string());
                    let name_key_ptr = self.gc.lock().allocate(name_key);
                    let name_val = RayaString::new(func_name);
                    let name_val_ptr = self.gc.lock().allocate(name_val);
                    frame_map.set(
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(name_key_ptr.as_ptr()).unwrap()) },
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(name_val_ptr.as_ptr()).unwrap()) }
                    );

                    // Frame index
                    let idx_key = RayaString::new("frameIndex".to_string());
                    let idx_key_ptr = self.gc.lock().allocate(idx_key);
                    frame_map.set(
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(idx_key_ptr.as_ptr()).unwrap()) },
                        Value::i32(i as i32)
                    );

                    // Add frame info if available
                    if let Some(frame) = stack_frames.get(i) {
                        let locals_key = RayaString::new("localCount".to_string());
                        let locals_key_ptr = self.gc.lock().allocate(locals_key);
                        frame_map.set(
                            unsafe { Value::from_ptr(std::ptr::NonNull::new(locals_key_ptr.as_ptr()).unwrap()) },
                            Value::i32(frame.local_count as i32)
                        );

                        let args_key = RayaString::new("argCount".to_string());
                        let args_key_ptr = self.gc.lock().allocate(args_key);
                        frame_map.set(
                            unsafe { Value::from_ptr(std::ptr::NonNull::new(args_key_ptr.as_ptr()).unwrap()) },
                            Value::i32(frame.arg_count as i32)
                        );
                    }

                    let frame_ptr = self.gc.lock().allocate(frame_map);
                    arr.push(unsafe { Value::from_ptr(std::ptr::NonNull::new(frame_ptr.as_ptr()).unwrap()) });
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
                // getSourceLocation(classId, methodName) - source location
                // Args: classId (number), methodName (string)
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "getSourceLocation requires 2 arguments: classId, methodName".to_string()
                    ));
                }

                let class_id = args[0].as_i32().ok_or_else(|| {
                    VmError::RuntimeError("getSourceLocation: classId must be a number".to_string())
                })? as usize;

                let method_name = if let Some(ptr) = unsafe { args[1].as_ptr::<RayaString>() } {
                    let s = unsafe { &*ptr.as_ptr() };
                    s.data.clone()
                } else {
                    return Err(VmError::RuntimeError(
                        "getSourceLocation: methodName must be a string".to_string()
                    ));
                };

                // Check if module has debug info
                if !module.has_debug_info() {
                    // Return null if no debug info available
                    Value::null()
                } else if let Some(ref debug_info) = module.debug_info {
                    // Find the class and method
                    let class_def = module.classes.get(class_id);
                    if class_def.is_none() {
                        Value::null()
                    } else {
                        let class_def = class_def.unwrap();
                        // Find the method by name
                        let method = class_def.methods.iter()
                            .find(|m| m.name == method_name);

                        if let Some(method) = method {
                            let function_id = method.function_id;

                            // Get function debug info
                            if let Some(func_debug) = debug_info.functions.get(function_id) {
                                // Get source file path
                                let source_file = debug_info
                                    .get_source_file(func_debug.source_file_index)
                                    .unwrap_or("unknown");

                                // Create a SourceLocation object with: file, line, column
                                let mut result_obj = Object::new(0, 3);

                                // Set file
                                let file_str = RayaString::new(source_file.to_string());
                                let file_ptr = self.gc.lock().allocate(file_str);
                                let _ = result_obj.set_field(0, unsafe {
                                    Value::from_ptr(std::ptr::NonNull::new(file_ptr.as_ptr()).unwrap())
                                });

                                // Set line (1-indexed)
                                let _ = result_obj.set_field(1, Value::i32(func_debug.start_line as i32));

                                // Set column (1-indexed)
                                let _ = result_obj.set_field(2, Value::i32(func_debug.start_column as i32));

                                let result_ptr = self.gc.lock().allocate(result_obj);
                                unsafe { Value::from_ptr(std::ptr::NonNull::new(result_ptr.as_ptr()).unwrap()) }
                            } else {
                                Value::null()
                            }
                        } else {
                            // Method not found
                            Value::null()
                        }
                    }
                } else {
                    Value::null()
                }
            }

            // ===== Phase 8: Serialization Helpers =====

            reflect::TO_JSON => {
                // toJSON(obj) - JSON string representation
                if args.is_empty() {
                    return Err(VmError::RuntimeError("toJSON requires 1 argument".to_string()));
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
                    return Err(VmError::RuntimeError("getEnumerableKeys requires 1 argument".to_string()));
                }
                let target = args[0];

                let mut arr = Array::new(0, 0);

                if let Some(class_id) = crate::vm::reflect::get_class_id(target) {
                    let class_metadata = self.class_metadata.read();
                    if let Some(meta) = class_metadata.get(class_id) {
                        for name in &meta.field_names {
                            let s = RayaString::new(name.clone());
                            let s_ptr = self.gc.lock().allocate(s);
                            arr.push(unsafe { Value::from_ptr(std::ptr::NonNull::new(s_ptr.as_ptr()).unwrap()) });
                        }
                    }
                }

                let arr_ptr = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap()) }
            }

            reflect::IS_CIRCULAR => {
                // isCircular(obj) - check for circular references
                if args.is_empty() {
                    return Err(VmError::RuntimeError("isCircular requires 1 argument".to_string()));
                }
                let target = args[0];
                let mut visited = Vec::new();
                let is_circular = self.check_circular(target, &mut visited);
                Value::bool(is_circular)
            }

            // ===== Decorator Registration (Phase 3/4 codegen) =====

            reflect::REGISTER_CLASS_DECORATOR => {
                // registerClassDecorator(classId, decoratorName)
                // Metadata registration - currently a no-op, decorator function does the work
                // The DecoratorRegistry is populated by the codegen emitted registration calls
                // which use global state. For now, we just acknowledge the call.
                Value::null()
            }

            reflect::REGISTER_METHOD_DECORATOR => {
                // registerMethodDecorator(classId, methodName, decoratorName)
                // Metadata registration - currently a no-op
                Value::null()
            }

            reflect::REGISTER_FIELD_DECORATOR => {
                // registerFieldDecorator(classId, fieldName, decoratorName)
                // Metadata registration - currently a no-op
                Value::null()
            }

            reflect::REGISTER_PARAMETER_DECORATOR => {
                // registerParameterDecorator(classId, methodName, paramIndex, decoratorName)
                // Metadata registration - currently a no-op
                Value::null()
            }

            reflect::GET_CLASS_DECORATORS => {
                // getClassDecorators(classId) -> get decorators applied to class
                // Returns empty array for now - full implementation uses DecoratorRegistry
                let arr = Array::new(0, 0);
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            reflect::GET_METHOD_DECORATORS => {
                // getMethodDecorators(classId, methodName) -> get decorators on method
                let arr = Array::new(0, 0);
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            reflect::GET_FIELD_DECORATORS => {
                // getFieldDecorators(classId, fieldName) -> get decorators on field
                let arr = Array::new(0, 0);
                let arr_gc = self.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }

            // ── BytecodeBuilder (Phase 15, delegated from std:runtime Phase 6) ──

            reflect::NEW_BYTECODE_BUILDER => {
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "BytecodeBuilder requires 3 arguments (name, paramCount, returnType)".to_string()
                    ));
                }
                let name = get_string(args[0].clone())?;
                let param_count = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("paramCount must be a number".to_string()))?
                    as usize;
                let return_type = get_string(args[2].clone())?;
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder_id = registry.create_builder(name, param_count, return_type);
                Value::i32(builder_id as i32)
            }

            reflect::BUILDER_EMIT => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "emit requires at least 2 arguments (builderId, opcode)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let opcode = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("opcode must be a number".to_string()))?
                    as u8;
                let operands: Vec<u8> = args[2..].iter()
                    .filter_map(|v| v.as_i32().map(|n| n as u8))
                    .collect();
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                builder.emit(opcode, &operands)?;
                Value::null()
            }

            reflect::BUILDER_EMIT_PUSH => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "emitPush requires 2 arguments (builderId, value)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let value = args[1];
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
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
                        "defineLabel requires 1 argument (builderId)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                let label = builder.define_label();
                Value::i32(label.id as i32)
            }

            reflect::BUILDER_MARK_LABEL => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "markLabel requires 2 arguments (builderId, labelId)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let label_id = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("labelId must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                builder.mark_label(crate::vm::reflect::Label { id: label_id })?;
                Value::null()
            }

            reflect::BUILDER_EMIT_JUMP => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "emitJump requires 2 arguments (builderId, labelId)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let label_id = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("labelId must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                builder.emit_jump(crate::vm::reflect::Label { id: label_id })?;
                Value::null()
            }

            reflect::BUILDER_EMIT_JUMP_IF => {
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "emitJumpIf requires 3 arguments (builderId, labelId, ifTrue)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let label_id = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("labelId must be a number".to_string()))?
                    as usize;
                let if_true = args[2].as_bool().unwrap_or(false);
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
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
                        "declareLocal requires 2 arguments (builderId, typeName)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let type_name = get_string(args[1].clone())?;
                let stack_type = match type_name.as_str() {
                    "number" | "i32" | "i64" | "int" => crate::vm::reflect::StackType::Integer,
                    "f64" | "float" => crate::vm::reflect::StackType::Float,
                    "boolean" | "bool" => crate::vm::reflect::StackType::Boolean,
                    "string" => crate::vm::reflect::StackType::String,
                    "null" => crate::vm::reflect::StackType::Null,
                    _ => crate::vm::reflect::StackType::Object,
                };
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                let index = builder.declare_local(None, stack_type)?;
                Value::i32(index as i32)
            }

            reflect::BUILDER_EMIT_LOAD_LOCAL => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "emitLoadLocal requires 2 arguments (builderId, index)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let index = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("index must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                builder.emit_load_local(index)?;
                Value::null()
            }

            reflect::BUILDER_EMIT_STORE_LOCAL => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "emitStoreLocal requires 2 arguments (builderId, index)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let index = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("index must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                builder.emit_store_local(index)?;
                Value::null()
            }

            reflect::BUILDER_EMIT_CALL => {
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "emitCall requires 3 arguments (builderId, functionId, argCount)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let function_id = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("functionId must be a number".to_string()))?
                    as u32;
                let arg_count = args[2].as_i32()
                    .ok_or_else(|| VmError::TypeError("argCount must be a number".to_string()))?
                    as u16;
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                builder.emit_call(function_id, arg_count)?;
                Value::null()
            }

            reflect::BUILDER_EMIT_RETURN => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "emitReturn requires at least 1 argument (builderId)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let has_value = args.get(1).and_then(|v| v.as_bool()).unwrap_or(true);
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
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
                        "validate requires 1 argument (builderId)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                let result = builder.validate();
                Value::bool(result.is_valid)
            }

            reflect::BUILDER_BUILD_FUNCTION => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "build requires 1 argument (builderId)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;
                let func = builder.build()?;
                let func_id = func.function_id;
                registry.register_function(func);
                Value::i32(func_id as i32)
            }

            // ===== Phase 14: ClassBuilder (0x0DE0-0x0DE6) =====

            reflect::NEW_CLASS_BUILDER => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "newClassBuilder requires 1 argument (name)".to_string()
                    ));
                }
                let name = get_string(args[0].clone())?;
                let mut registry = crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                let builder_id = registry.create_builder(name);
                Value::i32(builder_id as i32)
            }

            reflect::BUILDER_ADD_FIELD => {
                if args.len() < 5 {
                    return Err(VmError::RuntimeError(
                        "addField requires 5 arguments (builderId, name, typeName, isStatic, isReadonly)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let name = get_string(args[1].clone())?;
                let type_name = get_string(args[2].clone())?;
                let is_static = args[3].as_bool().unwrap_or(false);
                let is_readonly = args[4].as_bool().unwrap_or(false);
                let mut registry = crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id)))?;
                builder.add_field(name, &type_name, is_static, is_readonly)?;
                Value::null()
            }

            reflect::BUILDER_ADD_METHOD => {
                if args.len() < 5 {
                    return Err(VmError::RuntimeError(
                        "addMethod requires 5 arguments (builderId, name, functionId, isStatic, isAsync)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let name = get_string(args[1].clone())?;
                let function_id = args[2].as_i32()
                    .ok_or_else(|| VmError::TypeError("functionId must be a number".to_string()))?
                    as usize;
                let is_static = args[3].as_bool().unwrap_or(false);
                let is_async = args[4].as_bool().unwrap_or(false);
                let mut registry = crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id)))?;
                builder.add_method(name, function_id, is_static, is_async)?;
                Value::null()
            }

            reflect::BUILDER_SET_CONSTRUCTOR => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "setConstructor requires 2 arguments (builderId, functionId)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let function_id = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("functionId must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id)))?;
                builder.set_constructor(function_id)?;
                Value::null()
            }

            reflect::BUILDER_SET_PARENT => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "setParent requires 2 arguments (builderId, parentClassId)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let parent_id = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("parentClassId must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id)))?;
                builder.set_parent(parent_id)?;
                Value::null()
            }

            reflect::BUILDER_ADD_INTERFACE => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "addInterface requires 2 arguments (builderId, interfaceName)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;
                let interface_name = get_string(args[1].clone())?;
                let mut registry = crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                let builder = registry.get_mut(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id)))?;
                builder.add_interface(interface_name)?;
                Value::null()
            }

            reflect::BUILDER_BUILD => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "build requires 1 argument (builderId)".to_string()
                    ));
                }
                let builder_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                    as usize;

                let builder = {
                    let mut registry = crate::vm::builtins::handlers::reflect::CLASS_BUILDER_REGISTRY.lock();
                    registry.remove(builder_id)
                        .ok_or_else(|| VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id)))?
                };

                let def = builder.to_definition();
                let mut classes_write = self.classes.write();
                let next_id = classes_write.next_class_id();
                let mut dyn_builder = crate::vm::reflect::DynamicClassBuilder::new(next_id);

                let (new_class, new_metadata) = if let Some(parent_id) = builder.parent_id {
                    let parent = classes_write.get_class(parent_id)
                        .ok_or_else(|| VmError::RuntimeError(format!("Parent class {} not found", parent_id)))?
                        .clone();
                    drop(classes_write);

                    let class_metadata_guard = self.class_metadata.read();
                    let parent_metadata = class_metadata_guard.get(parent_id).cloned();
                    drop(class_metadata_guard);

                    let result = dyn_builder.create_subclass(
                        builder.name,
                        &parent,
                        parent_metadata.as_ref(),
                        &def,
                    );
                    classes_write = self.classes.write();
                    result
                } else {
                    dyn_builder.create_root_class(builder.name, &def)
                };

                let new_class_id = new_class.id;
                classes_write.register_class(new_class);
                drop(classes_write);

                let mut class_metadata_write = self.class_metadata.write();
                class_metadata_write.register(new_class_id, new_metadata);
                drop(class_metadata_write);

                Value::i32(new_class_id as i32)
            }

            // ===== Phase 17: DynamicModule (0x0E10-0x0E15) =====

            reflect::CREATE_MODULE => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "createModule requires 1 argument (name)".to_string()
                    ));
                }
                let name = get_string(args[0].clone())?;
                let mut registry = crate::vm::builtins::handlers::reflect::DYNAMIC_MODULE_REGISTRY.lock();
                let module_id = registry.create_module(name)?;
                Value::i32(module_id as i32)
            }

            reflect::MODULE_ADD_FUNCTION => {
                if args.len() < 2 {
                    return Err(VmError::RuntimeError(
                        "addFunction requires 2 arguments (moduleId, functionId)".to_string()
                    ));
                }
                let module_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("moduleId must be a number".to_string()))?
                    as usize;
                // Cast i32 → u32 → usize to preserve bit pattern (function IDs start at 0x8000_0000)
                let function_id = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("functionId must be a number".to_string()))?
                    as u32 as usize;

                let bytecode_registry = crate::vm::builtins::handlers::reflect::BYTECODE_BUILDER_REGISTRY.lock();
                let func = bytecode_registry.get_function(function_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("Function {} not found", function_id)))?
                    .clone();
                drop(bytecode_registry);

                let mut registry = crate::vm::builtins::handlers::reflect::DYNAMIC_MODULE_REGISTRY.lock();
                let module = registry.get_mut(module_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("Module {} not found", module_id)))?;
                module.add_function(func)?;
                Value::null()
            }

            reflect::MODULE_ADD_CLASS => {
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "addClass requires 3 arguments (moduleId, classId, name)".to_string()
                    ));
                }
                let module_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("moduleId must be a number".to_string()))?
                    as usize;
                let class_id = args[1].as_i32()
                    .ok_or_else(|| VmError::TypeError("classId must be a number".to_string()))?
                    as usize;
                let name = get_string(args[2].clone())?;
                let mut registry = crate::vm::builtins::handlers::reflect::DYNAMIC_MODULE_REGISTRY.lock();
                let module = registry.get_mut(module_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("Module {} not found", module_id)))?;
                module.add_class(class_id, class_id, name)?;
                Value::null()
            }

            reflect::MODULE_ADD_GLOBAL => {
                if args.len() < 3 {
                    return Err(VmError::RuntimeError(
                        "addGlobal requires 3 arguments (moduleId, name, value)".to_string()
                    ));
                }
                let module_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("moduleId must be a number".to_string()))?
                    as usize;
                let name = get_string(args[1].clone())?;
                let value = args[2];
                let mut registry = crate::vm::builtins::handlers::reflect::DYNAMIC_MODULE_REGISTRY.lock();
                let module = registry.get_mut(module_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("Module {} not found", module_id)))?;
                module.add_global(name, value)?;
                Value::null()
            }

            reflect::MODULE_SEAL => {
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "seal requires 1 argument (moduleId)".to_string()
                    ));
                }
                let module_id = args[0].as_i32()
                    .ok_or_else(|| VmError::TypeError("moduleId must be a number".to_string()))?
                    as usize;
                let mut registry = crate::vm::builtins::handlers::reflect::DYNAMIC_MODULE_REGISTRY.lock();
                let module = registry.get_mut(module_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("Module {} not found", module_id)))?;
                module.seal()?;
                Value::null()
            }

            reflect::MODULE_LINK => {
                // Stub: full import resolution not yet implemented
                if args.is_empty() {
                    return Err(VmError::RuntimeError(
                        "link requires 1 argument (moduleId)".to_string()
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
    fn inspect_value(&self, value: Value, depth: usize, max_depth: usize) -> Result<String, VmError> {
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
            return Ok(format!("\"{}\"", s.data.replace('\\', "\\\\").replace('"', "\\\"")));
        }

        // Array
        if let Some(ptr) = unsafe { value.as_ptr::<Array>() } {
            let arr = unsafe { &*ptr.as_ptr() };
            if depth >= max_depth {
                return Ok(format!("[Array({})]", arr.len()));
            }
            let mut items = Vec::new();
            for i in 0..arr.len().min(10) {
                items.push(self.inspect_value(arr.get(i).unwrap_or(Value::null()), depth + 1, max_depth)?);
            }
            if arr.len() > 10 {
                items.push(format!("... {} more", arr.len() - 10));
            }
            return Ok(format!("[{}]", items.join(", ")));
        }

        // Object
        if let Some(class_id) = crate::vm::reflect::get_class_id(value) {
            let classes = self.classes.read();
            let class_name = classes.get_class(class_id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| format!("Class{}", class_id));
            drop(classes);

            if depth >= max_depth {
                return Ok(format!("{} {{}}", class_name));
            }

            let class_metadata = self.class_metadata.read();
            if let Some(meta) = class_metadata.get(class_id) {
                let obj_ptr = unsafe { value.as_ptr::<Object>() };
                if let Some(ptr) = obj_ptr {
                    let obj = unsafe { &*ptr.as_ptr() };
                    let mut fields = Vec::new();
                    for (i, name) in meta.field_names.iter().enumerate() {
                        if let Some(&field_val) = obj.fields.get(i) {
                            let val_str = self.inspect_value(field_val, depth + 1, max_depth)?;
                            fields.push(format!("{}: {}", name, val_str));
                        }
                    }
                    return Ok(format!("{} {{ {} }}", class_name, fields.join(", ")));
                }
            }
            return Ok(format!("{} {{ ... }}", class_name));
        }

        Ok("<ptr>".to_string())
    }

    /// Helper: Calculate retained size by traversing references
    fn calculate_retained_size(&self, value: Value, visited: &mut std::collections::HashSet<usize>) -> usize {
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
            return Ok(format!("\"{}\"",
                s.data.replace('\\', "\\\\")
                      .replace('"', "\\\"")
                      .replace('\n', "\\n")
                      .replace('\r', "\\r")
                      .replace('\t', "\\t")));
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
        if let Some(class_id) = crate::vm::reflect::get_class_id(value) {
            let class_metadata = self.class_metadata.read();
            if let Some(meta) = class_metadata.get(class_id) {
                let obj_ptr = unsafe { value.as_ptr::<Object>() };
                if let Some(ptr) = obj_ptr {
                    let obj = unsafe { &*ptr.as_ptr() };
                    let mut fields = Vec::new();
                    for (i, name) in meta.field_names.iter().enumerate() {
                        if let Some(&field_val) = obj.fields.get(i) {
                            let val_json = self.value_to_json(field_val, visited)?;
                            fields.push(format!("\"{}\":{}", name, val_json));
                        }
                    }
                    visited.pop();
                    return Ok(format!("{{{}}}", fields.join(",")));
                }
            }
            visited.pop();
            return Ok("{}".to_string());
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
        let mut obj = Object::new(0, 4); // class_id 0 for dynamic object, 4 fields

        // Store class_name
        let class_name_str = RayaString::new(snapshot.class_name.clone());
        let class_name_ptr = self.gc.lock().allocate(class_name_str);
        let class_name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(class_name_ptr.as_ptr()).unwrap()) };
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
    fn snapshot_fields_to_value(&self, fields: &std::collections::HashMap<String, crate::vm::reflect::FieldSnapshot>) -> Value {
        // Create an object with field count matching the number of fields
        let field_count = fields.len();
        let mut obj = Object::new(0, field_count);

        // Sort fields by name for consistent ordering
        let mut field_names: Vec<_> = fields.keys().collect();
        field_names.sort();

        for (i, name) in field_names.iter().enumerate() {
            if let Some(field) = fields.get(*name) {
                // Create a field info object with: name, value, type_name
                let mut field_obj = Object::new(0, 3);

                // Field name
                let name_str = RayaString::new(field.name.clone());
                let name_ptr = self.gc.lock().allocate(name_str);
                let name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(name_ptr.as_ptr()).unwrap()) };
                field_obj.set_field(0, name_val);

                // Field value (converted from SnapshotValue)
                let val = self.snapshot_value_to_value(&field.value);
                field_obj.set_field(1, val);

                // Type name
                let type_str = RayaString::new(field.type_name.clone());
                let type_ptr = self.gc.lock().allocate(type_str);
                let type_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(type_ptr.as_ptr()).unwrap()) };
                field_obj.set_field(2, type_val);

                let field_ptr = self.gc.lock().allocate(field_obj);
                obj.set_field(i, unsafe { Value::from_ptr(std::ptr::NonNull::new(field_ptr.as_ptr()).unwrap()) });
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
        let mut obj = Object::new(0, 3);

        // Create added array
        let mut added_arr = Array::new(0, diff.added.len());
        for name in &diff.added {
            let name_str = RayaString::new(name.clone());
            let name_ptr = self.gc.lock().allocate(name_str);
            added_arr.push(unsafe { Value::from_ptr(std::ptr::NonNull::new(name_ptr.as_ptr()).unwrap()) });
        }
        let added_ptr = self.gc.lock().allocate(added_arr);
        obj.set_field(0, unsafe { Value::from_ptr(std::ptr::NonNull::new(added_ptr.as_ptr()).unwrap()) });

        // Create removed array
        let mut removed_arr = Array::new(0, diff.removed.len());
        for name in &diff.removed {
            let name_str = RayaString::new(name.clone());
            let name_ptr = self.gc.lock().allocate(name_str);
            removed_arr.push(unsafe { Value::from_ptr(std::ptr::NonNull::new(name_ptr.as_ptr()).unwrap()) });
        }
        let removed_ptr = self.gc.lock().allocate(removed_arr);
        obj.set_field(1, unsafe { Value::from_ptr(std::ptr::NonNull::new(removed_ptr.as_ptr()).unwrap()) });

        // Create changed object
        let changed_obj = self.diff_changes_to_value(&diff.changed);
        obj.set_field(2, changed_obj);

        let obj_ptr = self.gc.lock().allocate(obj);
        unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) }
    }

    /// Helper: Convert diff changes HashMap to a Raya Value (Object)
    #[allow(unused_must_use)]
    fn diff_changes_to_value(&self, changes: &std::collections::HashMap<String, crate::vm::reflect::ValueChange>) -> Value {
        let change_count = changes.len();
        let mut obj = Object::new(0, change_count);

        // Sort changes by name for consistent ordering
        let mut change_names: Vec<_> = changes.keys().collect();
        change_names.sort();

        for (i, name) in change_names.iter().enumerate() {
            if let Some(change) = changes.get(*name) {
                // Create a change object with: fieldName, old, new
                let mut change_obj = Object::new(0, 3);

                // Field name
                let name_str = RayaString::new((*name).clone());
                let name_ptr = self.gc.lock().allocate(name_str);
                change_obj.set_field(0, unsafe { Value::from_ptr(std::ptr::NonNull::new(name_ptr.as_ptr()).unwrap()) });

                // Old value
                let old_val = self.snapshot_value_to_value(&change.old);
                change_obj.set_field(1, old_val);

                // New value
                let new_val = self.snapshot_value_to_value(&change.new);
                change_obj.set_field(2, new_val);

                let change_ptr = self.gc.lock().allocate(change_obj);
                obj.set_field(i, unsafe { Value::from_ptr(std::ptr::NonNull::new(change_ptr.as_ptr()).unwrap()) });
            }
        }

        let obj_ptr = self.gc.lock().allocate(obj);
        unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) }
    }
}
