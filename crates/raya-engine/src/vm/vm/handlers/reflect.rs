//! Reflect method handlers
//!
//! Native implementation of Reflect API for metadata, class introspection,
//! field access, method invocation, and object creation.

use parking_lot::{Mutex, RwLock};

use crate::vm::builtin::reflect;
use crate::vm::gc::GarbageCollector as Gc;
use crate::vm::object::{Array, Closure, MapObject, Object, Proxy, RayaString};
use crate::vm::reflect::{ClassMetadataRegistry, MetadataStore};
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use crate::vm::vm::ClassRegistry;

/// Context needed for reflect method execution
pub struct ReflectHandlerContext<'a> {
    pub gc: &'a Mutex<Gc>,
    pub metadata: &'a Mutex<MetadataStore>,
    pub classes: &'a RwLock<ClassRegistry>,
    pub class_metadata: &'a RwLock<ClassMetadataRegistry>,
}

/// Handle built-in Reflect methods
pub fn call_reflect_method(
    ctx: &ReflectHandlerContext,
    stack: &mut std::sync::MutexGuard<'_, Stack>,
    method_id: u16,
    arg_count: usize,
) -> Result<(), VmError> {
    // Pop arguments
    let mut args = Vec::with_capacity(arg_count);
    for _ in 0..arg_count {
        args.push(stack.pop()?);
    }
    args.reverse();

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

            let mut metadata = ctx.metadata.lock();
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

            let mut metadata = ctx.metadata.lock();
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

            let metadata = ctx.metadata.lock();
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

            let metadata = ctx.metadata.lock();
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

            let metadata = ctx.metadata.lock();
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

            let metadata = ctx.metadata.lock();
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

            let metadata = ctx.metadata.lock();
            let keys = metadata.get_metadata_keys(target);

            // Create an array of string keys
            let mut arr = Array::new(0, keys.len());
            for (i, key) in keys.into_iter().enumerate() {
                let s = RayaString::new(key);
                let gc_ptr = ctx.gc.lock().allocate(s);
                let val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };
                arr.set(i, val).ok();
            }
            let arr_gc = ctx.gc.lock().allocate(arr);
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

            let metadata = ctx.metadata.lock();
            let keys = metadata.get_metadata_keys_property(target, &property_key);

            // Create an array of string keys
            let mut arr = Array::new(0, keys.len());
            for (i, key) in keys.into_iter().enumerate() {
                let s = RayaString::new(key);
                let gc_ptr = ctx.gc.lock().allocate(s);
                let val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };
                arr.set(i, val).ok();
            }
            let arr_gc = ctx.gc.lock().allocate(arr);
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

            let mut metadata = ctx.metadata.lock();
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

            let mut metadata = ctx.metadata.lock();
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
            let classes = ctx.classes.read();
            if let Some(class) = classes.get_class_by_name(&name) {
                Value::i32(class.id as i32)
            } else {
                Value::null()
            }
        }

        reflect::GET_ALL_CLASSES => {
            // getAllClasses() -> returns array of class IDs
            let classes = ctx.classes.read();
            let class_ids: Vec<Value> = classes
                .iter()
                .map(|(id, _)| Value::i32(id as i32))
                .collect();

            let mut arr = Array::new(0, class_ids.len());
            for (i, val) in class_ids.into_iter().enumerate() {
                arr.set(i, val).ok();
            }
            let arr_gc = ctx.gc.lock().allocate(arr);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
        }

        reflect::GET_CLASSES_WITH_DECORATOR => {
            // getClassesWithDecorator(decorator) -> returns array of class IDs
            // NOTE: This requires --emit-reflection to work fully
            // For now, returns empty array (decorator metadata not yet stored)
            let arr = Array::new(0, 0);
            let arr_gc = ctx.gc.lock().allocate(arr);
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
                let classes = ctx.classes.read();
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
                let classes = ctx.classes.read();
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
            let gc_ptr = ctx.gc.lock().allocate(s);
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
                let classes = ctx.classes.read();
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
                let arr_gc = ctx.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            } else {
                // Not an object, return empty array
                let arr = Array::new(0, 0);
                let arr_gc = ctx.gc.lock().allocate(arr);
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
            let class_metadata = ctx.class_metadata.read();
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
            let class_metadata = ctx.class_metadata.read();
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
                let class_metadata = ctx.class_metadata.read();
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
                let class_metadata = ctx.class_metadata.read();
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
                    let gc_ptr = ctx.gc.lock().allocate(s);
                    let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    arr.set(i, val).ok();
                }
            }
            let arr_gc = ctx.gc.lock().allocate(arr);
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
                let class_metadata = ctx.class_metadata.read();
                if let Some(meta) = class_metadata.get(class_id) {
                    if let Some(field_info) = meta.get_field_info(&property_key) {
                        // Create a MapObject with field info properties
                        let mut map = MapObject::new();

                        // Add field properties
                        let name_str = RayaString::new(field_info.name.clone());
                        let name_gc = ctx.gc.lock().allocate(name_str);
                        let name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(name_gc.as_ptr()).unwrap()) };

                        let type_str = RayaString::new(field_info.type_info.name.clone());
                        let type_gc = ctx.gc.lock().allocate(type_str);
                        let type_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(type_gc.as_ptr()).unwrap()) };

                        let key_name = RayaString::new("name".to_string());
                        let key_name_gc = ctx.gc.lock().allocate(key_name);
                        let key_name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_name_gc.as_ptr()).unwrap()) };
                        map.set(key_name_val, name_val);

                        let key_type = RayaString::new("type".to_string());
                        let key_type_gc = ctx.gc.lock().allocate(key_type);
                        let key_type_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_type_gc.as_ptr()).unwrap()) };
                        map.set(key_type_val, type_val);

                        let key_index = RayaString::new("index".to_string());
                        let key_index_gc = ctx.gc.lock().allocate(key_index);
                        let key_index_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_index_gc.as_ptr()).unwrap()) };
                        map.set(key_index_val, Value::i32(field_info.field_index as i32));

                        let key_static = RayaString::new("isStatic".to_string());
                        let key_static_gc = ctx.gc.lock().allocate(key_static);
                        let key_static_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_static_gc.as_ptr()).unwrap()) };
                        map.set(key_static_val, Value::bool(field_info.is_static));

                        let key_readonly = RayaString::new("isReadonly".to_string());
                        let key_readonly_gc = ctx.gc.lock().allocate(key_readonly);
                        let key_readonly_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_readonly_gc.as_ptr()).unwrap()) };
                        map.set(key_readonly_val, Value::bool(field_info.is_readonly));

                        let key_class = RayaString::new("declaringClass".to_string());
                        let key_class_gc = ctx.gc.lock().allocate(key_class);
                        let key_class_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_class_gc.as_ptr()).unwrap()) };
                        map.set(key_class_val, Value::i32(field_info.declaring_class_id as i32));

                        let map_gc = ctx.gc.lock().allocate(map);
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
                let arr_gc = ctx.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            } else if let Some(class_id) = crate::vm::reflect::get_class_id(target) {
                let class_metadata = ctx.class_metadata.read();
                if let Some(meta) = class_metadata.get(class_id) {
                    let fields = meta.get_all_field_infos();
                    let mut arr = Array::new(fields.len(), 0);

                    for (i, field_info) in fields.iter().enumerate() {
                        // Create a MapObject for each field
                        let mut map = MapObject::new();

                        let key_name = RayaString::new("name".to_string());
                        let key_name_gc = ctx.gc.lock().allocate(key_name);
                        let key_name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_name_gc.as_ptr()).unwrap()) };

                        let name_str = RayaString::new(field_info.name.clone());
                        let name_gc = ctx.gc.lock().allocate(name_str);
                        let name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(name_gc.as_ptr()).unwrap()) };
                        map.set(key_name_val, name_val);

                        let key_type = RayaString::new("type".to_string());
                        let key_type_gc = ctx.gc.lock().allocate(key_type);
                        let key_type_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_type_gc.as_ptr()).unwrap()) };

                        let type_str = RayaString::new(field_info.type_info.name.clone());
                        let type_gc = ctx.gc.lock().allocate(type_str);
                        let type_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(type_gc.as_ptr()).unwrap()) };
                        map.set(key_type_val, type_val);

                        let key_index = RayaString::new("index".to_string());
                        let key_index_gc = ctx.gc.lock().allocate(key_index);
                        let key_index_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_index_gc.as_ptr()).unwrap()) };
                        map.set(key_index_val, Value::i32(field_info.field_index as i32));

                        let key_static = RayaString::new("isStatic".to_string());
                        let key_static_gc = ctx.gc.lock().allocate(key_static);
                        let key_static_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_static_gc.as_ptr()).unwrap()) };
                        map.set(key_static_val, Value::bool(field_info.is_static));

                        let key_readonly = RayaString::new("isReadonly".to_string());
                        let key_readonly_gc = ctx.gc.lock().allocate(key_readonly);
                        let key_readonly_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_readonly_gc.as_ptr()).unwrap()) };
                        map.set(key_readonly_val, Value::bool(field_info.is_readonly));

                        let key_class = RayaString::new("declaringClass".to_string());
                        let key_class_gc = ctx.gc.lock().allocate(key_class);
                        let key_class_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_class_gc.as_ptr()).unwrap()) };
                        map.set(key_class_val, Value::i32(field_info.declaring_class_id as i32));

                        let map_gc = ctx.gc.lock().allocate(map);
                        let map_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(map_gc.as_ptr()).unwrap()) };
                        arr.set(i, map_val).ok();
                    }

                    let arr_gc = ctx.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                } else {
                    let arr = Array::new(0, 0);
                    let arr_gc = ctx.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                }
            } else {
                let arr = Array::new(0, 0);
                let arr_gc = ctx.gc.lock().allocate(arr);
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

            let class_metadata = ctx.class_metadata.read();
            if let Some(meta) = class_metadata.get(class_id) {
                let names = &meta.static_field_names;
                let mut arr = Array::new(names.len(), 0);
                for (i, name) in names.iter().enumerate() {
                    if !name.is_empty() {
                        let s = RayaString::new(name.clone());
                        let gc_ptr = ctx.gc.lock().allocate(s);
                        let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        arr.set(i, val).ok();
                    }
                }
                let arr_gc = ctx.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            } else {
                let arr = Array::new(0, 0);
                let arr_gc = ctx.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }
        }

        reflect::GET_STATIC_FIELDS => {
            // getStaticFields(classId) -> get static field infos (stub for now)
            // Static field detailed info requires additional metadata
            let arr = Array::new(0, 0);
            let arr_gc = ctx.gc.lock().allocate(arr);
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
                let class_metadata = ctx.class_metadata.read();
                let has_method = class_metadata.get(class_id)
                    .map(|meta| meta.has_method(&method_name))
                    .unwrap_or(false);
                Value::bool(has_method)
            } else {
                Value::bool(false)
            }
        }

        reflect::GET_METHODS => {
            // getMethods(target) -> get all method infos as array of Maps
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "getMethods requires 1 argument (target)".to_string()
                ));
            }
            let target = args[0];

            if !target.is_ptr() {
                let arr = Array::new(0, 0);
                let arr_gc = ctx.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            } else if let Some(class_id) = crate::vm::reflect::get_class_id(target) {
                let class_metadata = ctx.class_metadata.read();
                if let Some(meta) = class_metadata.get(class_id) {
                    let methods = meta.get_all_method_infos();
                    let mut arr = Array::new(methods.len(), 0);

                    for (i, method_info) in methods.iter().enumerate() {
                        // Create a MapObject for each method
                        let mut map = MapObject::new();

                        // name
                        let key_name = RayaString::new("name".to_string());
                        let key_name_gc = ctx.gc.lock().allocate(key_name);
                        let key_name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_name_gc.as_ptr()).unwrap()) };
                        let name_str = RayaString::new(method_info.name.clone());
                        let name_gc = ctx.gc.lock().allocate(name_str);
                        let name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(name_gc.as_ptr()).unwrap()) };
                        map.set(key_name_val, name_val);

                        // parameterCount
                        let key_param_count = RayaString::new("parameterCount".to_string());
                        let key_param_count_gc = ctx.gc.lock().allocate(key_param_count);
                        let key_param_count_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_param_count_gc.as_ptr()).unwrap()) };
                        map.set(key_param_count_val, Value::i32(method_info.parameters.len() as i32));

                        // isStatic
                        let key_static = RayaString::new("isStatic".to_string());
                        let key_static_gc = ctx.gc.lock().allocate(key_static);
                        let key_static_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_static_gc.as_ptr()).unwrap()) };
                        map.set(key_static_val, Value::bool(method_info.is_static));

                        // isAsync
                        let key_async = RayaString::new("isAsync".to_string());
                        let key_async_gc = ctx.gc.lock().allocate(key_async);
                        let key_async_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_async_gc.as_ptr()).unwrap()) };
                        map.set(key_async_val, Value::bool(method_info.is_async));

                        // declaringClass
                        let key_class = RayaString::new("declaringClass".to_string());
                        let key_class_gc = ctx.gc.lock().allocate(key_class);
                        let key_class_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_class_gc.as_ptr()).unwrap()) };
                        map.set(key_class_val, Value::i32(method_info.declaring_class_id as i32));

                        let map_gc = ctx.gc.lock().allocate(map);
                        let map_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(map_gc.as_ptr()).unwrap()) };
                        arr.set(i, map_val).ok();
                    }

                    let arr_gc = ctx.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                } else {
                    let arr = Array::new(0, 0);
                    let arr_gc = ctx.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                }
            } else {
                let arr = Array::new(0, 0);
                let arr_gc = ctx.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }
        }

        reflect::GET_METHOD_INFO => {
            // getMethodInfo(target, methodName) -> get method metadata as Map
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "getMethodInfo requires 2 arguments (target, methodName)".to_string()
                ));
            }
            let target = args[0];
            let method_name = get_string(args[1].clone())?;

            if !target.is_ptr() {
                Value::null()
            } else if let Some(class_id) = crate::vm::reflect::get_class_id(target) {
                let class_metadata = ctx.class_metadata.read();
                if let Some(meta) = class_metadata.get(class_id) {
                    if let Some(method_info) = meta.get_method_info(&method_name) {
                        // Create a MapObject with method info properties
                        let mut map = MapObject::new();

                        // name
                        let key_name = RayaString::new("name".to_string());
                        let key_name_gc = ctx.gc.lock().allocate(key_name);
                        let key_name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_name_gc.as_ptr()).unwrap()) };
                        let name_str = RayaString::new(method_info.name.clone());
                        let name_gc = ctx.gc.lock().allocate(name_str);
                        let name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(name_gc.as_ptr()).unwrap()) };
                        map.set(key_name_val, name_val);

                        // parameterCount
                        let key_param_count = RayaString::new("parameterCount".to_string());
                        let key_param_count_gc = ctx.gc.lock().allocate(key_param_count);
                        let key_param_count_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_param_count_gc.as_ptr()).unwrap()) };
                        map.set(key_param_count_val, Value::i32(method_info.parameters.len() as i32));

                        // isStatic
                        let key_static = RayaString::new("isStatic".to_string());
                        let key_static_gc = ctx.gc.lock().allocate(key_static);
                        let key_static_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_static_gc.as_ptr()).unwrap()) };
                        map.set(key_static_val, Value::bool(method_info.is_static));

                        // isAsync
                        let key_async = RayaString::new("isAsync".to_string());
                        let key_async_gc = ctx.gc.lock().allocate(key_async);
                        let key_async_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_async_gc.as_ptr()).unwrap()) };
                        map.set(key_async_val, Value::bool(method_info.is_async));

                        // declaringClass
                        let key_class = RayaString::new("declaringClass".to_string());
                        let key_class_gc = ctx.gc.lock().allocate(key_class);
                        let key_class_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_class_gc.as_ptr()).unwrap()) };
                        map.set(key_class_val, Value::i32(method_info.declaring_class_id as i32));

                        let map_gc = ctx.gc.lock().allocate(map);
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

        reflect::GET_METHOD => {
            // getMethod(target, methodName) -> get method as function reference
            // Returns the vtable index wrapped as a closure-like value
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "getMethod requires 2 arguments (target, methodName)".to_string()
                ));
            }
            let target = args[0];
            let method_name = get_string(args[1].clone())?;

            if !target.is_ptr() {
                Value::null()
            } else if let Some(class_id) = crate::vm::reflect::get_class_id(target) {
                let class_metadata = ctx.class_metadata.read();
                if let Some(meta) = class_metadata.get(class_id) {
                    if let Some(vtable_idx) = meta.get_method_index(&method_name) {
                        // Return the vtable index as an i32 - this can be used with invokeMethod
                        // TODO: Create a proper callable closure when Closure type is fully integrated
                        Value::i32(vtable_idx as i32)
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

        reflect::GET_STATIC_METHODS => {
            // getStaticMethods(classId) -> get all static method infos
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "getStaticMethods requires 1 argument (classId)".to_string()
                ));
            }
            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("getStaticMethods: classId must be a number".to_string()))?
                as usize;

            let class_metadata = ctx.class_metadata.read();
            if let Some(meta) = class_metadata.get(class_id) {
                // Filter static methods
                let static_methods: Vec<_> = meta.get_all_method_infos()
                    .iter()
                    .filter(|m| m.is_static)
                    .collect();

                let mut arr = Array::new(static_methods.len(), 0);
                for (i, method_info) in static_methods.iter().enumerate() {
                    let mut map = MapObject::new();

                    // name
                    let key_name = RayaString::new("name".to_string());
                    let key_name_gc = ctx.gc.lock().allocate(key_name);
                    let key_name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_name_gc.as_ptr()).unwrap()) };
                    let name_str = RayaString::new(method_info.name.clone());
                    let name_gc = ctx.gc.lock().allocate(name_str);
                    let name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(name_gc.as_ptr()).unwrap()) };
                    map.set(key_name_val, name_val);

                    // parameterCount
                    let key_param_count = RayaString::new("parameterCount".to_string());
                    let key_param_count_gc = ctx.gc.lock().allocate(key_param_count);
                    let key_param_count_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_param_count_gc.as_ptr()).unwrap()) };
                    map.set(key_param_count_val, Value::i32(method_info.parameters.len() as i32));

                    // isStatic
                    let key_static = RayaString::new("isStatic".to_string());
                    let key_static_gc = ctx.gc.lock().allocate(key_static);
                    let key_static_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_static_gc.as_ptr()).unwrap()) };
                    map.set(key_static_val, Value::bool(true));

                    // isAsync
                    let key_async = RayaString::new("isAsync".to_string());
                    let key_async_gc = ctx.gc.lock().allocate(key_async);
                    let key_async_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_async_gc.as_ptr()).unwrap()) };
                    map.set(key_async_val, Value::bool(method_info.is_async));

                    // declaringClass
                    let key_class = RayaString::new("declaringClass".to_string());
                    let key_class_gc = ctx.gc.lock().allocate(key_class);
                    let key_class_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_class_gc.as_ptr()).unwrap()) };
                    map.set(key_class_val, Value::i32(method_info.declaring_class_id as i32));

                    let map_gc = ctx.gc.lock().allocate(map);
                    let map_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(map_gc.as_ptr()).unwrap()) };
                    arr.set(i, map_val).ok();
                }

                let arr_gc = ctx.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            } else {
                let arr = Array::new(0, 0);
                let arr_gc = ctx.gc.lock().allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
            }
        }

        reflect::INVOKE | reflect::INVOKE_ASYNC | reflect::INVOKE_STATIC => {
            // These require VM execution context to call methods
            // TODO: Implement once TaskInterpreter context is available to handlers
            return Err(VmError::RuntimeError(format!(
                "Dynamic method invocation ({}) requires VM execution context - use direct method calls instead",
                match method_id {
                    reflect::INVOKE => "invoke",
                    reflect::INVOKE_ASYNC => "invokeAsync",
                    reflect::INVOKE_STATIC => "invokeStatic",
                    _ => "unknown",
                }
            )));
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

            let classes = ctx.classes.read();
            let class = classes.get_class(class_id)
                .ok_or_else(|| VmError::RuntimeError(format!("Class {} not found", class_id)))?;
            let field_count = class.field_count;
            drop(classes);

            // Allocate new object
            let obj = Object::new(class_id, field_count);
            let gc_ptr = ctx.gc.lock().allocate(obj);
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

            let classes = ctx.classes.read();
            let class = classes.get_class(class_id)
                .ok_or_else(|| VmError::RuntimeError(format!("Class {} not found", class_id)))?;
            let field_count = class.field_count;
            drop(classes);

            // Allocate new object (uninitialized - fields are null)
            let obj = Object::new(class_id, field_count);
            let gc_ptr = ctx.gc.lock().allocate(obj);
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
                let gc_ptr = ctx.gc.lock().allocate(cloned);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
            } else {
                // Unknown pointer type, return as-is
                target
            }
        }

        reflect::DEEP_CLONE => {
            // deepClone(obj) -> deep clone (recursive)
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "deepClone requires 1 argument".to_string()
                ));
            }
            let target = args[0];
            deep_clone_value(ctx, target)?
        }

        reflect::GET_CONSTRUCTOR_INFO => {
            // getConstructorInfo(classId) -> get constructor metadata as Map
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "getConstructorInfo requires 1 argument (classId)".to_string()
                ));
            }
            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("getConstructorInfo: classId must be a number".to_string()))?
                as usize;

            let class_metadata = ctx.class_metadata.read();
            if let Some(meta) = class_metadata.get(class_id) {
                if let Some(ctor) = &meta.constructor {
                    let mut map = MapObject::new();

                    // parameterCount
                    let key_param_count = RayaString::new("parameterCount".to_string());
                    let key_param_count_gc = ctx.gc.lock().allocate(key_param_count);
                    let key_param_count_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_param_count_gc.as_ptr()).unwrap()) };
                    map.set(key_param_count_val, Value::i32(ctor.parameters.len() as i32));

                    // declaringClass
                    let key_class = RayaString::new("declaringClass".to_string());
                    let key_class_gc = ctx.gc.lock().allocate(key_class);
                    let key_class_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_class_gc.as_ptr()).unwrap()) };
                    map.set(key_class_val, Value::i32(ctor.declaring_class_id as i32));

                    // parameterTypes as array
                    let mut param_types_arr = Array::new(ctor.parameters.len(), 0);
                    for (i, param) in ctor.parameters.iter().enumerate() {
                        let type_str = RayaString::new(param.type_info.name.clone());
                        let type_gc = ctx.gc.lock().allocate(type_str);
                        let type_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(type_gc.as_ptr()).unwrap()) };
                        param_types_arr.set(i, type_val).ok();
                    }
                    let param_types_gc = ctx.gc.lock().allocate(param_types_arr);
                    let param_types_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(param_types_gc.as_ptr()).unwrap()) };
                    let key_param_types = RayaString::new("parameterTypes".to_string());
                    let key_param_types_gc = ctx.gc.lock().allocate(key_param_types);
                    let key_param_types_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_param_types_gc.as_ptr()).unwrap()) };
                    map.set(key_param_types_val, param_types_val);

                    let map_gc = ctx.gc.lock().allocate(map);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(map_gc.as_ptr()).unwrap()) }
                } else {
                    Value::null()
                }
            } else {
                Value::null()
            }
        }

        reflect::CONSTRUCT_WITH => {
            // constructWith(classId, params) -> create with named params (for DI)
            // This requires more complex mapping - stub for now
            return Err(VmError::RuntimeError(
                "constructWith requires named parameter mapping - not yet implemented".to_string()
            ));
        }

        // ===== Phase 6: Type Utilities =====

        reflect::IS_STRING => {
            // isString(value) -> check if value is string
            if args.is_empty() {
                return Err(VmError::RuntimeError("isString requires 1 argument".to_string()));
            }
            let target = args[0];
            // Check if it's a pointer to a RayaString
            let is_string = target.is_ptr() && unsafe { target.as_ptr::<RayaString>() }.is_some();
            Value::bool(is_string)
        }

        reflect::IS_NUMBER => {
            // isNumber(value) -> check if value is number (i32 or f64)
            if args.is_empty() {
                return Err(VmError::RuntimeError("isNumber requires 1 argument".to_string()));
            }
            let target = args[0];
            Value::bool(target.as_i32().is_some() || target.as_f64().is_some())
        }

        reflect::IS_BOOLEAN => {
            // isBoolean(value) -> check if value is boolean
            if args.is_empty() {
                return Err(VmError::RuntimeError("isBoolean requires 1 argument".to_string()));
            }
            let target = args[0];
            Value::bool(target.as_bool().is_some())
        }

        reflect::IS_NULL => {
            // isNull(value) -> check if value is null
            if args.is_empty() {
                return Err(VmError::RuntimeError("isNull requires 1 argument".to_string()));
            }
            let target = args[0];
            Value::bool(target.is_null())
        }

        reflect::IS_ARRAY => {
            // isArray(value) -> check if value is array
            if args.is_empty() {
                return Err(VmError::RuntimeError("isArray requires 1 argument".to_string()));
            }
            let target = args[0];
            let is_array = target.is_ptr() && unsafe { target.as_ptr::<Array>() }.is_some();
            Value::bool(is_array)
        }

        reflect::IS_FUNCTION => {
            // isFunction(value) -> check if value is function/closure
            if args.is_empty() {
                return Err(VmError::RuntimeError("isFunction requires 1 argument".to_string()));
            }
            let target = args[0];
            // Check if it's a closure or function reference
            let is_fn = target.is_ptr() && unsafe { target.as_ptr::<Closure>() }.is_some();
            Value::bool(is_fn)
        }

        reflect::IS_OBJECT => {
            // isObject(value) -> check if value is object (class instance)
            if args.is_empty() {
                return Err(VmError::RuntimeError("isObject requires 1 argument".to_string()));
            }
            let target = args[0];
            Value::bool(crate::vm::reflect::get_class_id(target).is_some())
        }

        reflect::TYPE_OF => {
            // typeOf(typeName: string) -> TypeInfo | null
            // Returns a Map representing the TypeInfo for the given type name
            if args.is_empty() {
                return Err(VmError::RuntimeError("typeOf requires 1 argument".to_string()));
            }
            let type_name = get_string(args[0].clone())?;

            // Try to find as primitive type first
            let type_info = match type_name.as_str() {
                "string" | "number" | "boolean" | "null" | "void" | "any" => {
                    Some(crate::vm::reflect::TypeInfo::primitive(&type_name))
                }
                _ => {
                    // Try to find as a class
                    let classes = ctx.classes.read();
                    if let Some(class) = classes.get_class_by_name(&type_name) {
                        Some(crate::vm::reflect::TypeInfo::class(&type_name, class.id))
                    } else {
                        None
                    }
                }
            };

            if let Some(info) = type_info {
                // Create a Map with TypeInfo properties
                let mut map = MapObject::new();

                // kind - convert TypeKind enum to string
                let kind_string = match info.kind {
                    crate::vm::reflect::TypeKind::Primitive => "primitive",
                    crate::vm::reflect::TypeKind::Class => "class",
                    crate::vm::reflect::TypeKind::Interface => "interface",
                    crate::vm::reflect::TypeKind::Union => "union",
                    crate::vm::reflect::TypeKind::Function => "function",
                    crate::vm::reflect::TypeKind::Array => "array",
                    crate::vm::reflect::TypeKind::Generic => "generic",
                };
                let key_kind = RayaString::new("kind".to_string());
                let key_kind_gc = ctx.gc.lock().allocate(key_kind);
                let key_kind_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_kind_gc.as_ptr()).unwrap()) };
                let kind_str = RayaString::new(kind_string.to_string());
                let kind_gc = ctx.gc.lock().allocate(kind_str);
                let kind_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(kind_gc.as_ptr()).unwrap()) };
                map.set(key_kind_val, kind_val);

                // name
                let key_name = RayaString::new("name".to_string());
                let key_name_gc = ctx.gc.lock().allocate(key_name);
                let key_name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_name_gc.as_ptr()).unwrap()) };
                let name_str = RayaString::new(info.name.clone());
                let name_gc = ctx.gc.lock().allocate(name_str);
                let name_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(name_gc.as_ptr()).unwrap()) };
                map.set(key_name_val, name_val);

                // classId (if present)
                if let Some(class_id) = info.class_id {
                    let key_class = RayaString::new("classId".to_string());
                    let key_class_gc = ctx.gc.lock().allocate(key_class);
                    let key_class_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(key_class_gc.as_ptr()).unwrap()) };
                    map.set(key_class_val, Value::i32(class_id as i32));
                }

                let map_gc = ctx.gc.lock().allocate(map);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(map_gc.as_ptr()).unwrap()) }
            } else {
                Value::null()
            }
        }

        reflect::IS_ASSIGNABLE_TO => {
            // isAssignableTo(sourceTypeName: string, targetTypeName: string) -> boolean
            // Check if a value of source type can be assigned to a variable of target type
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "isAssignableTo requires 2 arguments (sourceType, targetType)".to_string()
                ));
            }
            let source_name = get_string(args[0].clone())?;
            let target_name = get_string(args[1].clone())?;

            // Simple type assignability rules:
            // 1. Same type is always assignable
            // 2. any is assignable to/from anything
            // 3. null is assignable to any nullable type
            // 4. Subclass is assignable to superclass

            let is_assignable = if source_name == target_name {
                true
            } else if target_name == "any" || source_name == "any" {
                true
            } else if source_name == "null" {
                // null is assignable to object types (classes)
                let classes = ctx.classes.read();
                classes.get_class_by_name(&target_name).is_some()
            } else {
                // Check class hierarchy
                let classes = ctx.classes.read();
                let source_class = classes.get_class_by_name(&source_name);
                let target_class = classes.get_class_by_name(&target_name);

                match (source_class, target_class) {
                    (Some(src), Some(tgt)) => {
                        // Check if source is subclass of target
                        crate::vm::reflect::is_subclass_of(&classes, src.id, tgt.id)
                    }
                    _ => false,
                }
            };

            Value::bool(is_assignable)
        }

        reflect::CAST => {
            // cast(value, classId) -> T | null
            // Safe cast - returns null if value is not an instance of the class
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "cast requires 2 arguments (value, classId)".to_string()
                ));
            }
            let target = args[0];
            let class_id = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("cast: classId must be a number".to_string()))?
                as usize;

            // Check if target is an instance of the class (or subclass)
            let classes = ctx.classes.read();
            if crate::vm::reflect::is_instance_of(&classes, target, class_id) {
                target
            } else {
                Value::null()
            }
        }

        reflect::CAST_OR_THROW => {
            // castOrThrow(value, classId) -> T
            // Cast that throws if value is not an instance of the class
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "castOrThrow requires 2 arguments (value, classId)".to_string()
                ));
            }
            let target = args[0];
            let class_id = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("castOrThrow: classId must be a number".to_string()))?
                as usize;

            // Check if target is an instance of the class (or subclass)
            let classes = ctx.classes.read();
            if crate::vm::reflect::is_instance_of(&classes, target, class_id) {
                target
            } else {
                // Get class name for error message
                let class_name = classes.get_class(class_id)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| format!("Class#{}", class_id));
                drop(classes);

                return Err(VmError::TypeError(format!(
                    "Cannot cast value to {}: incompatible type",
                    class_name
                )));
            }
        }

        // ===== Phase 7: Interface and Hierarchy Query =====

        reflect::GET_SUPERCLASS => {
            // getSuperclass(classId) -> get parent class ID or null
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "getSuperclass requires 1 argument (classId)".to_string()
                ));
            }
            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("getSuperclass: classId must be a number".to_string()))?
                as usize;

            let classes = ctx.classes.read();
            if let Some(class) = classes.get_class(class_id) {
                if let Some(parent_id) = class.parent_id {
                    Value::i32(parent_id as i32)
                } else {
                    Value::null()
                }
            } else {
                Value::null()
            }
        }

        reflect::GET_SUBCLASSES => {
            // getSubclasses(classId) -> get array of direct subclass IDs
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "getSubclasses requires 1 argument (classId)".to_string()
                ));
            }
            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("getSubclasses: classId must be a number".to_string()))?
                as usize;

            let classes = ctx.classes.read();
            let subclasses: Vec<Value> = classes
                .iter()
                .filter(|(_, class)| class.parent_id == Some(class_id))
                .map(|(id, _)| Value::i32(id as i32))
                .collect();
            drop(classes);

            let mut arr = Array::new(subclasses.len(), 0);
            for (i, val) in subclasses.into_iter().enumerate() {
                arr.set(i, val).ok();
            }
            let arr_gc = ctx.gc.lock().allocate(arr);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
        }

        reflect::IMPLEMENTS => {
            // implements(classId, interfaceName) -> boolean
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "implements requires 2 arguments (classId, interfaceName)".to_string()
                ));
            }
            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("implements: classId must be a number".to_string()))?
                as usize;
            let interface_name = get_string(args[1].clone())?;

            let class_metadata = ctx.class_metadata.read();
            let implements = class_metadata.get(class_id)
                .map(|meta| meta.implements_interface(&interface_name))
                .unwrap_or(false);
            Value::bool(implements)
        }

        reflect::GET_INTERFACES => {
            // getInterfaces(classId) -> string[]
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "getInterfaces requires 1 argument (classId)".to_string()
                ));
            }
            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("getInterfaces: classId must be a number".to_string()))?
                as usize;

            let class_metadata = ctx.class_metadata.read();
            let interfaces = class_metadata.get(class_id)
                .map(|meta| meta.get_interfaces().to_vec())
                .unwrap_or_default();
            drop(class_metadata);

            let mut arr = Array::new(interfaces.len(), 0);
            for (i, iface) in interfaces.into_iter().enumerate() {
                let s = RayaString::new(iface);
                let gc_ptr = ctx.gc.lock().allocate(s);
                let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                arr.set(i, val).ok();
            }
            let arr_gc = ctx.gc.lock().allocate(arr);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
        }

        reflect::GET_IMPLEMENTORS => {
            // getImplementors(interfaceName) -> number[] (class IDs)
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "getImplementors requires 1 argument (interfaceName)".to_string()
                ));
            }
            let interface_name = get_string(args[0].clone())?;

            let class_metadata = ctx.class_metadata.read();
            let implementors = class_metadata.get_implementors(&interface_name);
            drop(class_metadata);

            let mut arr = Array::new(implementors.len(), 0);
            for (i, class_id) in implementors.into_iter().enumerate() {
                arr.set(i, Value::i32(class_id as i32)).ok();
            }
            let arr_gc = ctx.gc.lock().allocate(arr);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
        }

        reflect::IS_STRUCTURALLY_COMPATIBLE => {
            // isStructurallyCompatible(sourceClassId, targetClassId) -> boolean
            // Check if source class has all fields/methods of target class
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "isStructurallyCompatible requires 2 arguments (sourceClassId, targetClassId)".to_string()
                ));
            }
            let source_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("isStructurallyCompatible: sourceClassId must be a number".to_string()))?
                as usize;
            let target_id = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("isStructurallyCompatible: targetClassId must be a number".to_string()))?
                as usize;

            // Same class is always structurally compatible
            if source_id == target_id {
                Value::bool(true)
            } else {
                let class_metadata = ctx.class_metadata.read();
                let source_meta = class_metadata.get(source_id);
                let target_meta = class_metadata.get(target_id);

                let is_compatible = match (source_meta, target_meta) {
                    (Some(src), Some(tgt)) => {
                        // Source must have all fields from target
                        let has_all_fields = tgt.field_names.iter()
                            .filter(|n| !n.is_empty())
                            .all(|name| src.has_field(name));

                        // Source must have all methods from target
                        let has_all_methods = tgt.method_names.iter()
                            .filter(|n| !n.is_empty())
                            .all(|name| src.has_method(name));

                        has_all_fields && has_all_methods
                    }
                    _ => false,
                };

                Value::bool(is_compatible)
            }
        }

        // ===== Phase 8: Object Inspection =====

        reflect::INSPECT => {
            // inspect(obj) -> human-readable string representation
            if args.is_empty() {
                return Err(VmError::RuntimeError("inspect requires 1 argument".to_string()));
            }
            let target = args[0];
            let result = inspect_value(ctx, target, 0)?;
            let s = RayaString::new(result);
            let gc_ptr = ctx.gc.lock().allocate(s);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
        }

        reflect::GET_OBJECT_ID => {
            // getObjectId(obj) -> unique identity number
            if args.is_empty() {
                return Err(VmError::RuntimeError("getObjectId requires 1 argument".to_string()));
            }
            let target = args[0];
            if target.is_ptr() {
                // Use the pointer address as the object ID
                if let Some(ptr) = unsafe { target.as_ptr::<Object>() } {
                    Value::i32((ptr.as_ptr() as usize & 0x7FFFFFFF) as i32)
                } else if let Some(ptr) = unsafe { target.as_ptr::<Array>() } {
                    Value::i32((ptr.as_ptr() as usize & 0x7FFFFFFF) as i32)
                } else if let Some(ptr) = unsafe { target.as_ptr::<RayaString>() } {
                    Value::i32((ptr.as_ptr() as usize & 0x7FFFFFFF) as i32)
                } else {
                    Value::i32(-1)
                }
            } else {
                // Primitives don't have object IDs
                Value::i32(-1)
            }
        }

        reflect::DESCRIBE => {
            // describe(classId) -> detailed class description string
            if args.is_empty() {
                return Err(VmError::RuntimeError("describe requires 1 argument (classId)".to_string()));
            }
            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("describe: classId must be a number".to_string()))?
                as usize;

            let classes = ctx.classes.read();
            let class_metadata = ctx.class_metadata.read();

            let description = if let Some(class) = classes.get_class(class_id) {
                let mut desc = format!("class {} {{\n", class.name);

                // Add parent info
                if let Some(parent_id) = class.parent_id {
                    if let Some(parent) = classes.get_class(parent_id) {
                        desc.push_str(&format!("  extends {}\n", parent.name));
                    }
                }

                // Add field info from metadata
                if let Some(meta) = class_metadata.get(class_id) {
                    // Interfaces
                    if !meta.interfaces.is_empty() {
                        desc.push_str(&format!("  implements {}\n", meta.interfaces.join(", ")));
                    }

                    // Fields
                    for field in meta.get_all_field_infos() {
                        let readonly = if field.is_readonly { "readonly " } else { "" };
                        let static_kw = if field.is_static { "static " } else { "" };
                        desc.push_str(&format!("  {}{}{}:{};\n",
                            static_kw, readonly, field.name, field.type_info.name));
                    }

                    // Methods
                    for method in meta.get_all_method_infos() {
                        let static_kw = if method.is_static { "static " } else { "" };
                        let async_kw = if method.is_async { "async " } else { "" };
                        let params: Vec<String> = method.parameters.iter()
                            .map(|p| format!("{}: {}", p.name, p.type_info.name))
                            .collect();
                        desc.push_str(&format!("  {}{}{}({}): {};\n",
                            static_kw, async_kw, method.name, params.join(", "), method.return_type.name));
                    }
                }

                desc.push_str("}");
                desc
            } else {
                format!("Class #{} not found", class_id)
            };

            let s = RayaString::new(description);
            let gc_ptr = ctx.gc.lock().allocate(s);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
        }

        reflect::SNAPSHOT | reflect::DIFF => {
            // snapshot and diff require more complex state tracking
            return Err(VmError::RuntimeError(format!(
                "{} requires object state tracking - not yet implemented",
                if method_id == reflect::SNAPSHOT { "snapshot" } else { "diff" }
            )));
        }

        // ===== Phase 8: Memory Analysis =====

        reflect::GET_OBJECT_SIZE => {
            // getObjectSize(obj) -> shallow size in bytes
            if args.is_empty() {
                return Err(VmError::RuntimeError("getObjectSize requires 1 argument".to_string()));
            }
            let target = args[0];

            let size = if !target.is_ptr() {
                // Primitives are 8 bytes (Value size)
                8
            } else if let Some(class_id) = crate::vm::reflect::get_class_id(target) {
                // Object: header + fields
                let classes = ctx.classes.read();
                let field_count = classes.get_class(class_id)
                    .map(|c| c.field_count)
                    .unwrap_or(0);
                // Object header (class_id: usize) + field array
                std::mem::size_of::<usize>() + field_count * std::mem::size_of::<Value>()
            } else if unsafe { target.as_ptr::<Array>() }.is_some() {
                let arr_ptr = unsafe { target.as_ptr::<Array>().unwrap() };
                let arr = unsafe { &*arr_ptr.as_ptr() };
                // Array header + elements
                std::mem::size_of::<usize>() * 2 + arr.len() * std::mem::size_of::<Value>()
            } else if let Some(str_ptr) = unsafe { target.as_ptr::<RayaString>() } {
                let s = unsafe { &*str_ptr.as_ptr() };
                // String header + data
                std::mem::size_of::<usize>() + s.data.len()
            } else {
                // Unknown type
                std::mem::size_of::<Value>()
            };

            Value::i32(size as i32)
        }

        reflect::GET_RETAINED_SIZE => {
            // getRetainedSize would need full object graph traversal
            // For now, return same as shallow size
            if args.is_empty() {
                return Err(VmError::RuntimeError("getRetainedSize requires 1 argument".to_string()));
            }
            // TODO: Implement full retained size calculation
            Value::i32(-1) // -1 indicates not implemented
        }

        reflect::GET_REFERENCES => {
            // getReferences(obj) -> array of objects this object references
            if args.is_empty() {
                return Err(VmError::RuntimeError("getReferences requires 1 argument".to_string()));
            }
            let target = args[0];
            let mut references = Vec::new();

            if let Some(class_id) = crate::vm::reflect::get_class_id(target) {
                let obj_ptr = unsafe { target.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                let classes = ctx.classes.read();
                let field_count = classes.get_class(class_id)
                    .map(|c| c.field_count)
                    .unwrap_or(0);
                drop(classes);

                for i in 0..field_count {
                    if let Some(field_val) = obj.get_field(i) {
                        if field_val.is_ptr() && !field_val.is_null() {
                            references.push(field_val);
                        }
                    }
                }
            } else if let Some(arr_ptr) = unsafe { target.as_ptr::<Array>() } {
                let arr = unsafe { &*arr_ptr.as_ptr() };
                for i in 0..arr.len() {
                    if let Some(elem) = arr.get(i) {
                        if elem.is_ptr() && !elem.is_null() {
                            references.push(elem);
                        }
                    }
                }
            }

            let mut arr = Array::new(references.len(), 0);
            for (i, val) in references.into_iter().enumerate() {
                arr.set(i, val).ok();
            }
            let arr_gc = ctx.gc.lock().allocate(arr);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
        }

        reflect::GET_REFERRERS | reflect::GET_HEAP_STATS | reflect::FIND_INSTANCES => {
            // These require GC integration to track all live objects
            return Err(VmError::RuntimeError(format!(
                "{} requires GC integration - not yet implemented",
                match method_id {
                    reflect::GET_REFERRERS => "getReferrers",
                    reflect::GET_HEAP_STATS => "getHeapStats",
                    reflect::FIND_INSTANCES => "findInstances",
                    _ => "unknown",
                }
            )));
        }

        // ===== Phase 8: Stack Introspection =====

        reflect::GET_CALL_STACK | reflect::GET_LOCALS | reflect::GET_SOURCE_LOCATION => {
            // Stack introspection requires access to the interpreter's call stack
            return Err(VmError::RuntimeError(format!(
                "{} requires interpreter context - not yet implemented",
                match method_id {
                    reflect::GET_CALL_STACK => "getCallStack",
                    reflect::GET_LOCALS => "getLocals",
                    reflect::GET_SOURCE_LOCATION => "getSourceLocation",
                    _ => "unknown",
                }
            )));
        }

        // ===== Phase 8: Serialization Helpers =====

        reflect::TO_JSON => {
            // toJSON(obj) -> JSON string representation
            if args.is_empty() {
                return Err(VmError::RuntimeError("toJSON requires 1 argument".to_string()));
            }
            let target = args[0];
            let json = value_to_json(ctx, target, &mut Vec::new())?;
            let s = RayaString::new(json);
            let gc_ptr = ctx.gc.lock().allocate(s);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
        }

        reflect::GET_ENUMERABLE_KEYS => {
            // getEnumerableKeys(obj) -> array of field names
            if args.is_empty() {
                return Err(VmError::RuntimeError("getEnumerableKeys requires 1 argument".to_string()));
            }
            let target = args[0];

            let keys = if let Some(class_id) = crate::vm::reflect::get_class_id(target) {
                let class_metadata = ctx.class_metadata.read();
                class_metadata.get(class_id)
                    .map(|meta| meta.field_names.iter()
                        .filter(|n| !n.is_empty())
                        .cloned()
                        .collect::<Vec<_>>())
                    .unwrap_or_default()
            } else {
                Vec::new()
            };

            let mut arr = Array::new(keys.len(), 0);
            for (i, key) in keys.into_iter().enumerate() {
                let s = RayaString::new(key);
                let gc_ptr = ctx.gc.lock().allocate(s);
                let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                arr.set(i, val).ok();
            }
            let arr_gc = ctx.gc.lock().allocate(arr);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
        }

        reflect::IS_CIRCULAR => {
            // isCircular(obj) -> check for circular references
            if args.is_empty() {
                return Err(VmError::RuntimeError("isCircular requires 1 argument".to_string()));
            }
            let target = args[0];
            let is_circular = check_circular(ctx, target, &mut Vec::new());
            Value::bool(is_circular)
        }

        // ===== Phase 9: Proxy Objects =====

        reflect::CREATE_PROXY => {
            // createProxy(target, handler) -> create a proxy object
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "createProxy requires 2 arguments (target, handler)".to_string()
                ));
            }
            let target = args[0];
            let handler = args[1];

            // Verify target is an object
            if !target.is_ptr() || target.is_null() {
                return Err(VmError::TypeError(
                    "createProxy target must be an object".to_string()
                ));
            }

            // Verify handler is an object
            if !handler.is_ptr() || handler.is_null() {
                return Err(VmError::TypeError(
                    "createProxy handler must be an object".to_string()
                ));
            }

            // Create the proxy
            let proxy = Proxy::new(target, handler);
            let proxy_gc = ctx.gc.lock().allocate(proxy);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(proxy_gc.as_ptr()).unwrap()) }
        }

        reflect::IS_PROXY => {
            // isProxy(obj) -> check if object is a proxy
            if args.is_empty() {
                return Err(VmError::RuntimeError("isProxy requires 1 argument".to_string()));
            }
            let target = args[0];

            // Check if target is a Proxy
            let is_proxy = if target.is_ptr() && !target.is_null() {
                unsafe { target.as_ptr::<Proxy>() }.is_some()
            } else {
                false
            };
            Value::bool(is_proxy)
        }

        reflect::GET_PROXY_TARGET => {
            // getProxyTarget(proxy) -> get the underlying target
            if args.is_empty() {
                return Err(VmError::RuntimeError("getProxyTarget requires 1 argument".to_string()));
            }
            let proxy_val = args[0];

            // Check if it's a proxy
            if !proxy_val.is_ptr() || proxy_val.is_null() {
                return Err(VmError::TypeError(
                    "getProxyTarget expects a proxy object".to_string()
                ));
            }

            if let Some(proxy_ptr) = unsafe { proxy_val.as_ptr::<Proxy>() } {
                let proxy = unsafe { &*proxy_ptr.as_ptr() };
                proxy.get_target()
            } else {
                // Not a proxy, return null
                Value::null()
            }
        }

        reflect::GET_PROXY_HANDLER => {
            // getProxyHandler(proxy) -> get the handler object
            if args.is_empty() {
                return Err(VmError::RuntimeError("getProxyHandler requires 1 argument".to_string()));
            }
            let proxy_val = args[0];

            // Check if it's a proxy
            if !proxy_val.is_ptr() || proxy_val.is_null() {
                return Err(VmError::TypeError(
                    "getProxyHandler expects a proxy object".to_string()
                ));
            }

            if let Some(proxy_ptr) = unsafe { proxy_val.as_ptr::<Proxy>() } {
                let proxy = unsafe { &*proxy_ptr.as_ptr() };
                proxy.get_handler()
            } else {
                // Not a proxy, return null
                Value::null()
            }
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

/// Deep clone a value recursively
fn deep_clone_value(ctx: &ReflectHandlerContext, value: Value) -> Result<Value, VmError> {
    if !value.is_ptr() {
        // Primitives are copied by value
        return Ok(value);
    }

    // Try to clone as Object
    if let Some(class_id) = crate::vm::reflect::get_class_id(value) {
        let obj_ptr = unsafe { value.as_ptr::<Object>() };
        let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };

        let classes = ctx.classes.read();
        let field_count = classes.get_class(class_id)
            .map(|c| c.field_count)
            .unwrap_or(0);
        drop(classes);

        // Create new object and deep clone each field
        let mut new_obj = Object::new(class_id, field_count);
        for i in 0..field_count {
            if let Some(field_val) = obj.get_field(i) {
                let cloned_field = deep_clone_value(ctx, field_val)?;
                new_obj.set_field(i, cloned_field).ok();
            }
        }

        let gc_ptr = ctx.gc.lock().allocate(new_obj);
        return Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) });
    }

    // Try to clone as Array
    if let Some(arr_ptr) = unsafe { value.as_ptr::<Array>() } {
        let arr = unsafe { &*arr_ptr.as_ptr() };
        let len = arr.len();
        let mut new_arr = Array::new(len, 0);

        for i in 0..len {
            if let Some(elem) = arr.get(i) {
                let cloned_elem = deep_clone_value(ctx, elem)?;
                new_arr.set(i, cloned_elem).ok();
            }
        }

        let gc_ptr = ctx.gc.lock().allocate(new_arr);
        return Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) });
    }

    // Try to clone as String (strings are immutable, just return same reference)
    if unsafe { value.as_ptr::<RayaString>() }.is_some() {
        return Ok(value);
    }

    // Unknown pointer type, return as-is
    Ok(value)
}

/// Generate a human-readable string representation of a value
fn inspect_value(ctx: &ReflectHandlerContext, value: Value, depth: usize) -> Result<String, VmError> {
    const MAX_DEPTH: usize = 5;

    if value.is_null() {
        return Ok("null".to_string());
    }

    if let Some(b) = value.as_bool() {
        return Ok(b.to_string());
    }

    if let Some(i) = value.as_i32() {
        return Ok(i.to_string());
    }

    if let Some(f) = value.as_f64() {
        return Ok(format!("{}", f));
    }

    if !value.is_ptr() {
        return Ok("<unknown primitive>".to_string());
    }

    // Check for string
    if let Some(str_ptr) = unsafe { value.as_ptr::<RayaString>() } {
        let s = unsafe { &*str_ptr.as_ptr() };
        return Ok(format!("\"{}\"", s.data.replace('\\', "\\\\").replace('"', "\\\"")));
    }

    // Check depth limit
    if depth >= MAX_DEPTH {
        return Ok("...".to_string());
    }

    // Check for array
    if let Some(arr_ptr) = unsafe { value.as_ptr::<Array>() } {
        let arr = unsafe { &*arr_ptr.as_ptr() };
        let len = arr.len();
        if len == 0 {
            return Ok("[]".to_string());
        }

        let indent = "  ".repeat(depth + 1);
        let mut result = "[\n".to_string();
        for i in 0..len.min(10) {
            if let Some(elem) = arr.get(i) {
                result.push_str(&indent);
                result.push_str(&inspect_value(ctx, elem, depth + 1)?);
                if i < len - 1 {
                    result.push(',');
                }
                result.push('\n');
            }
        }
        if len > 10 {
            result.push_str(&indent);
            result.push_str(&format!("... ({} more)\n", len - 10));
        }
        result.push_str(&"  ".repeat(depth));
        result.push(']');
        return Ok(result);
    }

    // Check for object
    if let Some(class_id) = crate::vm::reflect::get_class_id(value) {
        let obj_ptr = unsafe { value.as_ptr::<Object>() };
        let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };

        let classes = ctx.classes.read();
        let class_name = classes.get_class(class_id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| format!("Class#{}", class_id));
        let field_count = classes.get_class(class_id)
            .map(|c| c.field_count)
            .unwrap_or(0);
        drop(classes);

        let class_metadata = ctx.class_metadata.read();
        let field_names = class_metadata.get(class_id)
            .map(|m| m.field_names.clone())
            .unwrap_or_default();
        drop(class_metadata);

        let indent = "  ".repeat(depth + 1);
        let mut result = format!("{} {{\n", class_name);
        for i in 0..field_count {
            let field_name = field_names.get(i)
                .filter(|n| !n.is_empty())
                .cloned()
                .unwrap_or_else(|| format!("field{}", i));
            if let Some(field_val) = obj.get_field(i) {
                result.push_str(&indent);
                result.push_str(&format!("{}: {}", field_name, inspect_value(ctx, field_val, depth + 1)?));
                if i < field_count - 1 {
                    result.push(',');
                }
                result.push('\n');
            }
        }
        result.push_str(&"  ".repeat(depth));
        result.push('}');
        return Ok(result);
    }

    Ok("<unknown>".to_string())
}

/// Get a unique ID for a value (for circular reference checking)
fn get_value_id(value: Value) -> Option<usize> {
    if !value.is_ptr() || value.is_null() {
        return None;
    }
    if let Some(ptr) = unsafe { value.as_ptr::<Object>() } {
        Some(ptr.as_ptr() as usize)
    } else if let Some(ptr) = unsafe { value.as_ptr::<Array>() } {
        Some(ptr.as_ptr() as usize)
    } else if let Some(ptr) = unsafe { value.as_ptr::<RayaString>() } {
        Some(ptr.as_ptr() as usize)
    } else {
        None
    }
}

/// Convert a value to JSON string
fn value_to_json(ctx: &ReflectHandlerContext, value: Value, visited: &mut Vec<usize>) -> Result<String, VmError> {
    if value.is_null() {
        return Ok("null".to_string());
    }

    if let Some(b) = value.as_bool() {
        return Ok(b.to_string());
    }

    if let Some(i) = value.as_i32() {
        return Ok(i.to_string());
    }

    if let Some(f) = value.as_f64() {
        if f.is_nan() || f.is_infinite() {
            return Ok("null".to_string()); // JSON doesn't support NaN/Infinity
        }
        return Ok(format!("{}", f));
    }

    if !value.is_ptr() {
        return Ok("null".to_string());
    }

    // Check for circular reference
    let ptr_id = match get_value_id(value) {
        Some(id) => id,
        None => return Ok("null".to_string()),
    };
    if visited.contains(&ptr_id) {
        return Ok("null".to_string()); // Circular reference
    }
    visited.push(ptr_id);

    // Check for string
    if let Some(str_ptr) = unsafe { value.as_ptr::<RayaString>() } {
        let s = unsafe { &*str_ptr.as_ptr() };
        let escaped = s.data
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t");
        visited.pop();
        return Ok(format!("\"{}\"", escaped));
    }

    // Check for array
    if let Some(arr_ptr) = unsafe { value.as_ptr::<Array>() } {
        let arr = unsafe { &*arr_ptr.as_ptr() };
        let len = arr.len();
        if len == 0 {
            visited.pop();
            return Ok("[]".to_string());
        }

        let mut result = "[".to_string();
        for i in 0..len {
            if i > 0 {
                result.push(',');
            }
            if let Some(elem) = arr.get(i) {
                result.push_str(&value_to_json(ctx, elem, visited)?);
            } else {
                result.push_str("null");
            }
        }
        result.push(']');
        visited.pop();
        return Ok(result);
    }

    // Check for object
    if let Some(class_id) = crate::vm::reflect::get_class_id(value) {
        let obj_ptr = unsafe { value.as_ptr::<Object>() };
        let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };

        let classes = ctx.classes.read();
        let field_count = classes.get_class(class_id)
            .map(|c| c.field_count)
            .unwrap_or(0);
        drop(classes);

        let class_metadata = ctx.class_metadata.read();
        let field_names = class_metadata.get(class_id)
            .map(|m| m.field_names.clone())
            .unwrap_or_default();
        drop(class_metadata);

        let mut result = "{".to_string();
        let mut first = true;
        for i in 0..field_count {
            let field_name = field_names.get(i)
                .filter(|n| !n.is_empty())
                .cloned()
                .unwrap_or_else(|| format!("field{}", i));
            if let Some(field_val) = obj.get_field(i) {
                if !first {
                    result.push(',');
                }
                first = false;
                result.push_str(&format!("\"{}\":{}", field_name, value_to_json(ctx, field_val, visited)?));
            }
        }
        result.push('}');
        visited.pop();
        return Ok(result);
    }

    visited.pop();
    Ok("null".to_string())
}

/// Check if an object contains circular references
fn check_circular(ctx: &ReflectHandlerContext, value: Value, visited: &mut Vec<usize>) -> bool {
    if !value.is_ptr() || value.is_null() {
        return false;
    }

    let ptr_id = match get_value_id(value) {
        Some(id) => id,
        None => return false,
    };
    if visited.contains(&ptr_id) {
        return true; // Found circular reference
    }
    visited.push(ptr_id);

    // Check array elements
    if let Some(arr_ptr) = unsafe { value.as_ptr::<Array>() } {
        let arr = unsafe { &*arr_ptr.as_ptr() };
        for i in 0..arr.len() {
            if let Some(elem) = arr.get(i) {
                if check_circular(ctx, elem, visited) {
                    return true;
                }
            }
        }
    }

    // Check object fields
    if let Some(class_id) = crate::vm::reflect::get_class_id(value) {
        let obj_ptr = unsafe { value.as_ptr::<Object>() };
        let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };

        let classes = ctx.classes.read();
        let field_count = classes.get_class(class_id)
            .map(|c| c.field_count)
            .unwrap_or(0);
        drop(classes);

        for i in 0..field_count {
            if let Some(field_val) = obj.get_field(i) {
                if check_circular(ctx, field_val, visited) {
                    return true;
                }
            }
        }
    }

    visited.pop();
    false
}
