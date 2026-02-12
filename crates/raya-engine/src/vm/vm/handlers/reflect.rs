//! Reflect method handlers
//!
//! Native implementation of Reflect API for metadata, class introspection,
//! field access, method invocation, and object creation.

use std::sync::LazyLock;

use parking_lot::{Mutex, RwLock};

use crate::vm::builtin::reflect;

// Lazy static registries for Phase 13 and 14 runtime type creation
pub(crate) static CLASS_BUILDER_REGISTRY: LazyLock<Mutex<ClassBuilderRegistry>> =
    LazyLock::new(|| Mutex::new(ClassBuilderRegistry::new()));
static DYNAMIC_FUNCTION_REGISTRY: LazyLock<Mutex<DynamicFunctionRegistry>> =
    LazyLock::new(|| Mutex::new(DynamicFunctionRegistry::new()));
static SPECIALIZATION_CACHE: LazyLock<Mutex<SpecializationCache>> =
    LazyLock::new(|| Mutex::new(SpecializationCache::new()));
static GENERIC_TYPE_REGISTRY: LazyLock<Mutex<GenericTypeRegistry>> =
    LazyLock::new(|| Mutex::new(GenericTypeRegistry::new()));
pub(crate) static BYTECODE_BUILDER_REGISTRY: LazyLock<Mutex<BytecodeBuilderRegistry>> =
    LazyLock::new(|| Mutex::new(BytecodeBuilderRegistry::new()));
static PERMISSION_STORE: LazyLock<Mutex<PermissionStore>> =
    LazyLock::new(|| Mutex::new(PermissionStore::new()));
pub(crate) static DYNAMIC_MODULE_REGISTRY: LazyLock<Mutex<DynamicModuleRegistry>> =
    LazyLock::new(|| Mutex::new(DynamicModuleRegistry::new()));
static BOOTSTRAP_CONTEXT: LazyLock<Mutex<BootstrapContext>> =
    LazyLock::new(|| Mutex::new(BootstrapContext::new()));
static WRAPPER_FUNCTION_REGISTRY: LazyLock<Mutex<WrapperFunctionRegistry>> =
    LazyLock::new(|| Mutex::new(WrapperFunctionRegistry::new()));
static DECORATOR_REGISTRY: LazyLock<Mutex<DecoratorRegistry>> =
    LazyLock::new(|| Mutex::new(DecoratorRegistry::new()));

use crate::vm::gc::GarbageCollector as Gc;
use crate::vm::object::{Array, Closure, MapObject, Object, Proxy, RayaString};
use crate::vm::reflect::{
    BootstrapContext, BytecodeBuilderRegistry, ClassBuilder, ClassBuilderRegistry,
    ClassMetadataRegistry, DecoratorApplication, DecoratorRegistry, DecoratorTargetType,
    DynamicClassBuilder, DynamicClosure, DynamicFunction, DynamicFunctionRegistry,
    DynamicModuleRegistry, FieldDefinition, FunctionWrapper, GenericTypeRegistry,
    MetadataStore, PermissionStore, ReflectionPermission, SpecializationCache, StackType,
    SubclassDefinition, TypeInfo, WrapperFunctionRegistry, core_class_ids, is_bootstrapped,
    mark_bootstrapped,
};
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
            // getClassesWithDecorator(decoratorName) -> returns array of class IDs
            // Queries the DecoratorRegistry for classes with the specified decorator
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "getClassesWithDecorator requires 1 argument (decoratorName)".to_string()
                ));
            }

            let decorator_name = get_string(args[0].clone())?;

            // Query the decorator registry
            let registry = DECORATOR_REGISTRY.lock();
            let class_ids = registry.get_classes_with_decorator(&decorator_name);
            drop(registry);

            // Convert to array of class IDs
            let mut arr = Array::new(0, class_ids.len());
            for (i, class_id) in class_ids.into_iter().enumerate() {
                arr.set(i, Value::i32(class_id as i32)).ok();
            }
            let arr_gc = ctx.gc.lock().allocate(arr);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
        }

        reflect::REGISTER_CLASS_DECORATOR => {
            // registerClassDecorator(classId, decoratorName, argsArray)
            // Called by codegen to register decorator applications
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "registerClassDecorator requires at least 2 arguments (classId, decoratorName)".to_string()
                ));
            }

            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("classId must be a number".to_string()))?
                as usize;
            let decorator_name = get_string(args[1].clone())?;

            // Parse args array if provided
            let decorator_args = if args.len() > 2 {
                parse_value_array(ctx, args[2])?
            } else {
                Vec::new()
            };

            let decorator = DecoratorApplication {
                name: decorator_name,
                args: decorator_args,
                target_type: DecoratorTargetType::Class,
                property_key: None,
                parameter_index: None,
            };

            // Register in decorator registry (primary store for decorator metadata)
            // Query via getClassDecorators(classId), getClassesWithDecorator(name)
            let mut registry = DECORATOR_REGISTRY.lock();
            registry.register_class_decorator(class_id, decorator);

            Value::null()
        }

        reflect::REGISTER_METHOD_DECORATOR => {
            // registerMethodDecorator(classId, methodName, decoratorName, argsArray)
            if args.len() < 3 {
                return Err(VmError::RuntimeError(
                    "registerMethodDecorator requires at least 3 arguments (classId, methodName, decoratorName)".to_string()
                ));
            }

            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("classId must be a number".to_string()))?
                as usize;
            let method_name = get_string(args[1].clone())?;
            let decorator_name = get_string(args[2].clone())?;

            let decorator_args = if args.len() > 3 {
                parse_value_array(ctx, args[3])?
            } else {
                Vec::new()
            };

            let decorator = DecoratorApplication {
                name: decorator_name,
                args: decorator_args,
                target_type: DecoratorTargetType::Method,
                property_key: Some(method_name.clone()),
                parameter_index: None,
            };

            // Register in decorator registry
            // Query via getMethodDecorators(classId, methodName)
            let mut registry = DECORATOR_REGISTRY.lock();
            registry.register_method_decorator(class_id, method_name, decorator);

            Value::null()
        }

        reflect::REGISTER_FIELD_DECORATOR => {
            // registerFieldDecorator(classId, fieldName, decoratorName, argsArray)
            if args.len() < 3 {
                return Err(VmError::RuntimeError(
                    "registerFieldDecorator requires at least 3 arguments (classId, fieldName, decoratorName)".to_string()
                ));
            }

            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("classId must be a number".to_string()))?
                as usize;
            let field_name = get_string(args[1].clone())?;
            let decorator_name = get_string(args[2].clone())?;

            let decorator_args = if args.len() > 3 {
                parse_value_array(ctx, args[3])?
            } else {
                Vec::new()
            };

            let decorator = DecoratorApplication {
                name: decorator_name,
                args: decorator_args,
                target_type: DecoratorTargetType::Field,
                property_key: Some(field_name.clone()),
                parameter_index: None,
            };

            // Register in decorator registry
            // Query via getFieldDecorators(classId, fieldName)
            let mut registry = DECORATOR_REGISTRY.lock();
            registry.register_field_decorator(class_id, field_name, decorator);

            Value::null()
        }

        reflect::REGISTER_PARAMETER_DECORATOR => {
            // registerParameterDecorator(classId, methodName, paramIndex, decoratorName, argsArray)
            if args.len() < 4 {
                return Err(VmError::RuntimeError(
                    "registerParameterDecorator requires at least 4 arguments (classId, methodName, paramIndex, decoratorName)".to_string()
                ));
            }

            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("classId must be a number".to_string()))?
                as usize;
            let method_name = get_string(args[1].clone())?;
            let param_index = args[2].as_i32()
                .ok_or_else(|| VmError::TypeError("paramIndex must be a number".to_string()))?
                as usize;
            let decorator_name = get_string(args[3].clone())?;

            let decorator_args = if args.len() > 4 {
                parse_value_array(ctx, args[4])?
            } else {
                Vec::new()
            };

            let decorator = DecoratorApplication {
                name: decorator_name,
                args: decorator_args,
                target_type: DecoratorTargetType::Parameter,
                property_key: Some(method_name.clone()),
                parameter_index: Some(param_index),
            };

            let mut registry = DECORATOR_REGISTRY.lock();
            registry.register_parameter_decorator(class_id, method_name, param_index, decorator);

            Value::null()
        }

        reflect::GET_CLASS_DECORATORS => {
            // getClassDecorators(classId) -> array of DecoratorInfo objects
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "getClassDecorators requires 1 argument (classId)".to_string()
                ));
            }

            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("classId must be a number".to_string()))?
                as usize;

            let registry = DECORATOR_REGISTRY.lock();
            let decorators = registry.get_class_decorators(class_id);

            // Create array of decorator info objects
            let mut arr = Array::new(0, decorators.len());
            for (i, dec) in decorators.iter().enumerate() {
                // Create a simple object with name and targetType
                let obj = create_decorator_info_object(ctx, dec)?;
                arr.set(i, obj).ok();
            }
            drop(registry);

            let arr_gc = ctx.gc.lock().allocate(arr);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
        }

        reflect::GET_METHOD_DECORATORS => {
            // getMethodDecorators(classId, methodName) -> array of DecoratorInfo objects
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "getMethodDecorators requires 2 arguments (classId, methodName)".to_string()
                ));
            }

            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("classId must be a number".to_string()))?
                as usize;
            let method_name = get_string(args[1].clone())?;

            let registry = DECORATOR_REGISTRY.lock();
            let decorators = registry.get_method_decorators(class_id, &method_name);

            let mut arr = Array::new(0, decorators.len());
            for (i, dec) in decorators.iter().enumerate() {
                let obj = create_decorator_info_object(ctx, dec)?;
                arr.set(i, obj).ok();
            }
            drop(registry);

            let arr_gc = ctx.gc.lock().allocate(arr);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
        }

        reflect::GET_FIELD_DECORATORS => {
            // getFieldDecorators(classId, fieldName) -> array of DecoratorInfo objects
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "getFieldDecorators requires 2 arguments (classId, fieldName)".to_string()
                ));
            }

            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("classId must be a number".to_string()))?
                as usize;
            let field_name = get_string(args[1].clone())?;

            let registry = DECORATOR_REGISTRY.lock();
            let decorators = registry.get_field_decorators(class_id, &field_name);

            let mut arr = Array::new(0, decorators.len());
            for (i, dec) in decorators.iter().enumerate() {
                let obj = create_decorator_info_object(ctx, dec)?;
                arr.set(i, obj).ok();
            }
            drop(registry);

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

        // ===== Phase 10: Dynamic Subclass Creation =====

        reflect::CREATE_SUBCLASS => {
            // createSubclass(superclassId, name, fieldsArray) -> create a new subclass
            // Args: superclassId (i32), name (string), fieldsArray (Array of field definition objects)
            if args.len() < 3 {
                return Err(VmError::RuntimeError(
                    "createSubclass requires 3 arguments (superclassId, name, fieldsArray)".to_string()
                ));
            }

            let superclass_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("createSubclass: superclassId must be a number".to_string()))?
                as usize;
            let name = get_string(args[1].clone())?;

            // Get superclass info
            let classes = ctx.classes.read();
            let superclass = classes.get_class(superclass_id)
                .ok_or_else(|| VmError::RuntimeError(format!("Superclass {} not found", superclass_id)))?
                .clone();
            drop(classes);

            // Get parent metadata
            let class_metadata_guard = ctx.class_metadata.read();
            let parent_metadata = class_metadata_guard.get(superclass_id).cloned();
            drop(class_metadata_guard);

            // Parse fields array to build SubclassDefinition
            let def = parse_fields_array(ctx, args[2])?;

            // Create the subclass
            let mut classes_write = ctx.classes.write();
            let next_id = classes_write.next_class_id();
            let mut builder = DynamicClassBuilder::new(next_id);

            let (new_class, new_metadata) = builder.create_subclass(
                name,
                &superclass,
                parent_metadata.as_ref(),
                &def,
            );

            let new_class_id = new_class.id;
            classes_write.register_class(new_class);
            drop(classes_write);

            // Register metadata
            let mut class_metadata_write = ctx.class_metadata.write();
            class_metadata_write.register(new_class_id, new_metadata);
            drop(class_metadata_write);

            Value::i32(new_class_id as i32)
        }

        reflect::EXTEND_WITH => {
            // extendWith(classId, fieldsArray) -> create extended class with additional fields
            // Args: classId (i32), fieldsArray (Array of field definition objects)
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "extendWith requires 2 arguments (classId, fieldsArray)".to_string()
                ));
            }

            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("extendWith: classId must be a number".to_string()))?
                as usize;

            // Get original class info
            let classes = ctx.classes.read();
            let original_class = classes.get_class(class_id)
                .ok_or_else(|| VmError::RuntimeError(format!("Class {} not found", class_id)))?
                .clone();
            drop(classes);

            // Get original metadata
            let class_metadata_guard = ctx.class_metadata.read();
            let original_metadata = class_metadata_guard.get(class_id).cloned();
            drop(class_metadata_guard);

            // Parse fields array
            let def = parse_fields_array(ctx, args[1])?;

            // Create extended class
            let mut classes_write = ctx.classes.write();
            let next_id = classes_write.next_class_id();
            let mut builder = DynamicClassBuilder::new(next_id);

            let (new_class, new_metadata) = builder.extend_with_fields(
                &original_class,
                original_metadata.as_ref(),
                &def.fields,
            );

            let new_class_id = new_class.id;
            classes_write.register_class(new_class);
            drop(classes_write);

            // Register metadata
            let mut class_metadata_write = ctx.class_metadata.write();
            class_metadata_write.register(new_class_id, new_metadata);
            drop(class_metadata_write);

            Value::i32(new_class_id as i32)
        }

        reflect::DEFINE_CLASS => {
            // defineClass(name, fieldsArray) -> create a new root class
            // Args: name (string), fieldsArray (Array of field definition objects)
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "defineClass requires 2 arguments (name, fieldsArray)".to_string()
                ));
            }

            let name = get_string(args[0].clone())?;

            // Parse fields array to build SubclassDefinition
            let def = parse_fields_array(ctx, args[1])?;

            // Create the class
            let mut classes_write = ctx.classes.write();
            let next_id = classes_write.next_class_id();
            let mut builder = DynamicClassBuilder::new(next_id);

            let (new_class, new_metadata) = builder.create_root_class(name, &def);

            let new_class_id = new_class.id;
            classes_write.register_class(new_class);
            drop(classes_write);

            // Register metadata
            let mut class_metadata_write = ctx.class_metadata.write();
            class_metadata_write.register(new_class_id, new_metadata);
            drop(class_metadata_write);

            Value::i32(new_class_id as i32)
        }

        reflect::ADD_METHOD => {
            // addMethod(classId, name, functionId) -> add method to class vtable
            // Args: classId (i32), name (string), functionId (i32)
            if args.len() < 3 {
                return Err(VmError::RuntimeError(
                    "addMethod requires 3 arguments (classId, name, functionId)".to_string()
                ));
            }

            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("addMethod: classId must be a number".to_string()))?
                as usize;
            let method_name = get_string(args[1].clone())?;
            let function_id = args[2].as_i32()
                .ok_or_else(|| VmError::TypeError("addMethod: functionId must be a number".to_string()))?
                as usize;

            // Add method to class vtable
            let mut classes = ctx.classes.write();
            let class = classes.get_class_mut(class_id)
                .ok_or_else(|| VmError::RuntimeError(format!("Class {} not found", class_id)))?;
            class.add_method(function_id);
            let method_index = class.vtable.method_count() - 1;
            drop(classes);

            // Update metadata
            let mut class_metadata = ctx.class_metadata.write();
            let meta = class_metadata.get_or_create(class_id);
            meta.add_method(method_name, method_index);
            drop(class_metadata);

            Value::i32(method_index as i32)
        }

        reflect::SET_CONSTRUCTOR => {
            // setConstructor(classId, functionId) -> set constructor for class
            // Args: classId (i32), functionId (i32)
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "setConstructor requires 2 arguments (classId, functionId)".to_string()
                ));
            }

            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("setConstructor: classId must be a number".to_string()))?
                as usize;
            let function_id = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("setConstructor: functionId must be a number".to_string()))?
                as usize;

            // Set constructor
            let mut classes = ctx.classes.write();
            let class = classes.get_class_mut(class_id)
                .ok_or_else(|| VmError::RuntimeError(format!("Class {} not found", class_id)))?;
            class.set_constructor(function_id);
            drop(classes);

            Value::null()
        }

        // ===== Phase 13: Generic Type Metadata =====

        reflect::GET_GENERIC_ORIGIN => {
            // getGenericOrigin(classId) -> string | null
            // Returns the original generic name (e.g., "Box" for Box_number)
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "getGenericOrigin requires 1 argument (classId)".to_string()
                ));
            }
            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("classId must be a number".to_string()))?
                as usize;

            let registry = GENERIC_TYPE_REGISTRY.lock();
            match registry.get_generic_origin(class_id) {
                Some(name) => {
                    let s = RayaString::new(name.to_string());
                    let gc_ptr = ctx.gc.lock().allocate(s);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
                }
                None => Value::null()
            }
        }

        reflect::GET_TYPE_PARAMETERS => {
            // getTypeParameters(classId) -> GenericParameterInfo[]
            // Returns type parameter info for a generic class
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "getTypeParameters requires 1 argument (classId)".to_string()
                ));
            }
            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("classId must be a number".to_string()))?
                as usize;

            // First check if this is a specialized class, get its generic name
            let registry = GENERIC_TYPE_REGISTRY.lock();
            let generic_name = registry.get_generic_origin(class_id)
                .map(|s| s.to_string());

            let params = if let Some(name) = generic_name {
                registry.get_type_parameters(&name)
            } else {
                None
            };

            match params {
                Some(params) => {
                    // Create an array of parameter info objects
                    // Each element is a Map with name, index, constraint
                    let mut arr = Array::new(0, params.len());
                    for (i, param) in params.iter().enumerate() {
                        let mut map = MapObject::new();

                        // Set name
                        let name_key = RayaString::new("name".to_string());
                        let name_key_gc = ctx.gc.lock().allocate(name_key);
                        let name_val = RayaString::new(param.name.clone());
                        let name_val_gc = ctx.gc.lock().allocate(name_val);
                        let name_key_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(name_key_gc.as_ptr()).unwrap()) };
                        let name_val_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(name_val_gc.as_ptr()).unwrap()) };
                        map.set(name_key_v, name_val_v);

                        // Set index
                        let index_key = RayaString::new("index".to_string());
                        let index_key_gc = ctx.gc.lock().allocate(index_key);
                        let index_key_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(index_key_gc.as_ptr()).unwrap()) };
                        map.set(index_key_v, Value::i32(param.index as i32));

                        // Set constraint (null if none)
                        let constraint_key = RayaString::new("constraint".to_string());
                        let constraint_key_gc = ctx.gc.lock().allocate(constraint_key);
                        let constraint_key_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(constraint_key_gc.as_ptr()).unwrap()) };
                        let constraint_val = if let Some(ref c) = param.constraint {
                            let c_name = RayaString::new(c.name.clone());
                            let c_gc = ctx.gc.lock().allocate(c_name);
                            unsafe { Value::from_ptr(std::ptr::NonNull::new(c_gc.as_ptr()).unwrap()) }
                        } else {
                            Value::null()
                        };
                        map.set(constraint_key_v, constraint_val);

                        let map_gc = ctx.gc.lock().allocate(map);
                        let map_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(map_gc.as_ptr()).unwrap()) };
                        arr.set(i, map_val).ok();
                    }
                    let arr_gc = ctx.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                }
                None => {
                    // Return empty array
                    let arr = Array::new(0, 0);
                    let arr_gc = ctx.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                }
            }
        }

        reflect::GET_TYPE_ARGUMENTS => {
            // getTypeArguments(classId) -> TypeInfo[]
            // Returns actual type arguments for a monomorphized class
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "getTypeArguments requires 1 argument (classId)".to_string()
                ));
            }
            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("classId must be a number".to_string()))?
                as usize;

            let registry = GENERIC_TYPE_REGISTRY.lock();
            match registry.get_type_arguments(class_id) {
                Some(type_args) => {
                    // Create array of TypeInfo representations (as Maps)
                    let mut arr = Array::new(0, type_args.len());
                    for (i, type_info) in type_args.iter().enumerate() {
                        let mut map = MapObject::new();

                        // Set name
                        let name_key = RayaString::new("name".to_string());
                        let name_key_gc = ctx.gc.lock().allocate(name_key);
                        let name_val = RayaString::new(type_info.name.clone());
                        let name_val_gc = ctx.gc.lock().allocate(name_val);
                        let name_key_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(name_key_gc.as_ptr()).unwrap()) };
                        let name_val_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(name_val_gc.as_ptr()).unwrap()) };
                        map.set(name_key_v, name_val_v);

                        // Set kind
                        let kind_key = RayaString::new("kind".to_string());
                        let kind_key_gc = ctx.gc.lock().allocate(kind_key);
                        let kind_val = RayaString::new(format!("{:?}", type_info.kind));
                        let kind_val_gc = ctx.gc.lock().allocate(kind_val);
                        let kind_key_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(kind_key_gc.as_ptr()).unwrap()) };
                        let kind_val_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(kind_val_gc.as_ptr()).unwrap()) };
                        map.set(kind_key_v, kind_val_v);

                        // Set classId if present
                        if let Some(cid) = type_info.class_id {
                            let cid_key = RayaString::new("classId".to_string());
                            let cid_key_gc = ctx.gc.lock().allocate(cid_key);
                            let cid_key_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(cid_key_gc.as_ptr()).unwrap()) };
                            map.set(cid_key_v, Value::i32(cid as i32));
                        }

                        let map_gc = ctx.gc.lock().allocate(map);
                        let map_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(map_gc.as_ptr()).unwrap()) };
                        arr.set(i, map_val).ok();
                    }
                    let arr_gc = ctx.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                }
                None => {
                    // Return empty array
                    let arr = Array::new(0, 0);
                    let arr_gc = ctx.gc.lock().allocate(arr);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
                }
            }
        }

        reflect::IS_GENERIC_INSTANCE => {
            // isGenericInstance(classId) -> boolean
            // Returns true if the class is a monomorphized generic
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "isGenericInstance requires 1 argument (classId)".to_string()
                ));
            }
            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("classId must be a number".to_string()))?
                as usize;

            let registry = GENERIC_TYPE_REGISTRY.lock();
            Value::bool(registry.is_generic_instance(class_id))
        }

        reflect::GET_GENERIC_BASE => {
            // getGenericBase(genericName) -> number | null
            // Returns the base generic class ID for a generic definition
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "getGenericBase requires 1 argument (genericName)".to_string()
                ));
            }
            let generic_name = get_string(args[0].clone())?;

            let registry = GENERIC_TYPE_REGISTRY.lock();
            match registry.get_generic_base(&generic_name) {
                Some(class_id) => Value::i32(class_id as i32),
                None => Value::null()
            }
        }

        reflect::FIND_SPECIALIZATIONS => {
            // findSpecializations(genericName) -> number[]
            // Returns array of class IDs for all monomorphizations of a generic
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "findSpecializations requires 1 argument (genericName)".to_string()
                ));
            }
            let generic_name = get_string(args[0].clone())?;

            let registry = GENERIC_TYPE_REGISTRY.lock();
            let specializations = registry.find_specializations(&generic_name);

            let mut arr = Array::new(0, specializations.len());
            for (i, class_id) in specializations.iter().enumerate() {
                arr.set(i, Value::i32(*class_id as i32)).ok();
            }
            let arr_gc = ctx.gc.lock().allocate(arr);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
        }

        // ===== Phase 14: Runtime Type Creation =====

        reflect::NEW_CLASS_BUILDER => {
            // newClassBuilder(name) -> create a new ClassBuilder, returns builder ID
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "newClassBuilder requires 1 argument (name)".to_string()
                ));
            }
            let name = get_string(args[0].clone())?;

            let mut registry = CLASS_BUILDER_REGISTRY.lock();
            let builder_id = registry.create_builder(name);

            Value::i32(builder_id as i32)
        }

        reflect::BUILDER_ADD_FIELD => {
            // builderAddField(builderId, name, typeName, isStatic, isReadonly) -> add field
            if args.len() < 5 {
                return Err(VmError::RuntimeError(
                    "builderAddField requires 5 arguments (builderId, name, typeName, isStatic, isReadonly)".to_string()
                ));
            }

            let builder_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                as usize;
            let name = get_string(args[1].clone())?;
            let type_name = get_string(args[2].clone())?;
            let is_static = args[3].as_bool().unwrap_or(false);
            let is_readonly = args[4].as_bool().unwrap_or(false);

            let mut registry = CLASS_BUILDER_REGISTRY.lock();
            let builder = registry.get_mut(builder_id)
                .ok_or_else(|| VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id)))?;

            builder.add_field(name, &type_name, is_static, is_readonly)?;

            Value::null()
        }

        reflect::BUILDER_ADD_METHOD => {
            // builderAddMethod(builderId, name, functionId, isStatic, isAsync) -> add method
            if args.len() < 5 {
                return Err(VmError::RuntimeError(
                    "builderAddMethod requires 5 arguments (builderId, name, functionId, isStatic, isAsync)".to_string()
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

            let mut registry = CLASS_BUILDER_REGISTRY.lock();
            let builder = registry.get_mut(builder_id)
                .ok_or_else(|| VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id)))?;

            builder.add_method(name, function_id, is_static, is_async)?;

            Value::null()
        }

        reflect::BUILDER_SET_CONSTRUCTOR => {
            // builderSetConstructor(builderId, functionId) -> set constructor
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "builderSetConstructor requires 2 arguments (builderId, functionId)".to_string()
                ));
            }

            let builder_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                as usize;
            let function_id = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("functionId must be a number".to_string()))?
                as usize;

            let mut registry = CLASS_BUILDER_REGISTRY.lock();
            let builder = registry.get_mut(builder_id)
                .ok_or_else(|| VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id)))?;

            builder.set_constructor(function_id)?;

            Value::null()
        }

        reflect::BUILDER_SET_PARENT => {
            // builderSetParent(builderId, parentClassId) -> set parent class
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "builderSetParent requires 2 arguments (builderId, parentClassId)".to_string()
                ));
            }

            let builder_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                as usize;
            let parent_id = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("parentClassId must be a number".to_string()))?
                as usize;

            let mut registry = CLASS_BUILDER_REGISTRY.lock();
            let builder = registry.get_mut(builder_id)
                .ok_or_else(|| VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id)))?;

            builder.set_parent(parent_id)?;

            Value::null()
        }

        reflect::BUILDER_ADD_INTERFACE => {
            // builderAddInterface(builderId, interfaceName) -> add interface
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "builderAddInterface requires 2 arguments (builderId, interfaceName)".to_string()
                ));
            }

            let builder_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                as usize;
            let interface_name = get_string(args[1].clone())?;

            let mut registry = CLASS_BUILDER_REGISTRY.lock();
            let builder = registry.get_mut(builder_id)
                .ok_or_else(|| VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id)))?;

            builder.add_interface(interface_name)?;

            Value::null()
        }

        reflect::BUILDER_BUILD => {
            // builderBuild(builderId) -> finalize and register class, returns class ID
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "builderBuild requires 1 argument (builderId)".to_string()
                ));
            }

            let builder_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                as usize;

            // Get and remove the builder
            let builder = {
                let mut registry = CLASS_BUILDER_REGISTRY.lock();
                registry.remove(builder_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("ClassBuilder {} not found", builder_id)))?
            };

            // Convert to definition
            let def = builder.to_definition();

            // Build the class
            let mut classes_write = ctx.classes.write();
            let next_id = classes_write.next_class_id();
            let mut dyn_builder = DynamicClassBuilder::new(next_id);

            let (new_class, new_metadata) = if let Some(parent_id) = builder.parent_id {
                // Get parent info
                let parent = classes_write.get_class(parent_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("Parent class {} not found", parent_id)))?
                    .clone();
                drop(classes_write);

                let class_metadata_guard = ctx.class_metadata.read();
                let parent_metadata = class_metadata_guard.get(parent_id).cloned();
                drop(class_metadata_guard);

                let result = dyn_builder.create_subclass(
                    builder.name,
                    &parent,
                    parent_metadata.as_ref(),
                    &def,
                );

                // Re-acquire write lock
                classes_write = ctx.classes.write();
                result
            } else {
                dyn_builder.create_root_class(builder.name, &def)
            };

            let new_class_id = new_class.id;
            classes_write.register_class(new_class);
            drop(classes_write);

            // Register metadata
            let mut class_metadata_write = ctx.class_metadata.write();
            class_metadata_write.register(new_class_id, new_metadata);
            drop(class_metadata_write);

            Value::i32(new_class_id as i32)
        }

        reflect::CREATE_FUNCTION => {
            // createFunction(name, paramCount, bytecodeArray) -> create function, returns function ID
            if args.len() < 3 {
                return Err(VmError::RuntimeError(
                    "createFunction requires 3 arguments (name, paramCount, bytecodeArray)".to_string()
                ));
            }

            let name = get_string(args[0].clone())?;
            let param_count = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("paramCount must be a number".to_string()))?
                as usize;

            // Parse bytecode array
            let bytecode = parse_bytecode_array(args[2])?;

            let func = DynamicFunction::new(name, param_count, bytecode);
            let func_id = func.id;

            let mut registry = DYNAMIC_FUNCTION_REGISTRY.lock();
            registry.register(func);

            Value::i32(func_id as i32)
        }

        reflect::CREATE_ASYNC_FUNCTION => {
            // createAsyncFunction(name, paramCount, bytecodeArray) -> create async function
            if args.len() < 3 {
                return Err(VmError::RuntimeError(
                    "createAsyncFunction requires 3 arguments (name, paramCount, bytecodeArray)".to_string()
                ));
            }

            let name = get_string(args[0].clone())?;
            let param_count = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("paramCount must be a number".to_string()))?
                as usize;

            let bytecode = parse_bytecode_array(args[2])?;

            let func = DynamicFunction::new_async(name, param_count, bytecode);
            let func_id = func.id;

            let mut registry = DYNAMIC_FUNCTION_REGISTRY.lock();
            registry.register(func);

            Value::i32(func_id as i32)
        }

        reflect::CREATE_CLOSURE => {
            // createClosure(functionId, capturesArray) -> create closure with captures
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "createClosure requires 2 arguments (functionId, capturesArray)".to_string()
                ));
            }

            let function_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("functionId must be a number".to_string()))?
                as usize;

            // Parse captures array
            let captures = parse_captures_array(args[1])?;

            // Create Closure object
            let closure = Closure::new(function_id, captures);
            let gc_ptr = ctx.gc.lock().allocate(closure);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
        }

        reflect::CREATE_NATIVE_CALLBACK => {
            // createNativeCallback(callbackId) -> register a native callback (stub)
            // In a real implementation, this would register a callback function pointer
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "createNativeCallback requires 1 argument (callbackId)".to_string()
                ));
            }

            let callback_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("callbackId must be a number".to_string()))?;

            // For now, just return the callback ID (would need FFI integration for real usage)
            Value::i32(callback_id)
        }

        reflect::SPECIALIZE => {
            // specialize(genericName, typeArgsArray) -> create/lookup specialization
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "specialize requires 2 arguments (genericName, typeArgsArray)".to_string()
                ));
            }

            let generic_name = get_string(args[0].clone())?;
            let type_args = parse_string_array(ctx, args[1])?;

            // Check cache first
            let mut cache = SPECIALIZATION_CACHE.lock();
            if let Some(class_id) = cache.get(&generic_name, &type_args) {
                return stack.push(Value::i32(class_id as i32)).map(|_| ());
            }

            // For now, specialization creation is a stub - would need compiler integration
            // to actually generate specialized bytecode from generic template
            return Err(VmError::RuntimeError(format!(
                "Runtime specialization of '{}' not yet implemented - requires compiler integration",
                generic_name
            )));
        }

        reflect::GET_SPECIALIZATION_CACHE => {
            // getSpecializationCache() -> get all cached specializations as array
            let cache = SPECIALIZATION_CACHE.lock();
            let entries: Vec<_> = cache.entries().collect();

            // Create array of [key, classId] pairs
            let mut arr = Array::new(0, entries.len());
            for (i, (key, &class_id)) in entries.iter().enumerate() {
                // Create a simple representation - just return class IDs for now
                arr.set(i, Value::i32(class_id as i32)).ok();
            }

            let arr_gc = ctx.gc.lock().allocate(arr);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap()) }
        }

        // ===== High-level Function Builder (for Decorators) =====

        reflect::CREATE_WRAPPER => {
            // createWrapper(method, hooks) -> wrapped function
            // High-level API for method decorators
            // hooks: { before?, after?, around?, onError? }
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "createWrapper requires 2 arguments (method, hooks)".to_string()
                ));
            }

            let method = args[0];
            let hooks_obj = args[1];

            // Get method function ID (if it's a closure or function reference)
            let method_func_id = if method.is_ptr() {
                // Try to extract function ID from closure
                if let Some(closure_ptr) = unsafe { method.as_ptr::<Closure>() } {
                    let closure = unsafe { &*closure_ptr.as_ptr() };
                    closure.func_id
                } else {
                    // Use a placeholder ID for non-closure functions
                    0
                }
            } else if let Some(func_id) = method.as_i32() {
                func_id as usize
            } else {
                // Unknown method type - return as-is
                return stack.push(method).map(|_| ());
            };

            // Build a simple wrapper that stores the original method and hooks
            let wrapper = FunctionWrapper::new(method_func_id, 0)
                .build()
                .map_err(|e| VmError::RuntimeError(format!("Failed to build wrapper: {:?}", e)))?;
            let wrapper_id = wrapper.id;

            // Register the wrapper
            let mut registry = WRAPPER_FUNCTION_REGISTRY.lock();
            registry.register(wrapper);
            drop(registry);

            // Create a closure that captures: [original_method, wrapper_id_value, hooks_obj]
            // The interpreter can use these captures to execute the wrapper logic
            let captures = vec![method, Value::i32(wrapper_id as i32), hooks_obj];
            let closure = Closure::new(wrapper_id, captures);

            // Allocate and return the closure
            let closure_gc = ctx.gc.lock().allocate(closure);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(closure_gc.as_ptr()).unwrap()) }
        }

        reflect::CREATE_METHOD_WRAPPER => {
            // createMethodWrapper(method, wrapper) -> wrapped function
            // Simple wrapper where wrapper function controls execution
            // The wrapper function receives (method, ...args)
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "createMethodWrapper requires 2 arguments (method, wrapperFn)".to_string()
                ));
            }

            let method = args[0];
            let wrapper_fn = args[1];

            // Get method function ID
            let method_func_id = if method.is_ptr() {
                if let Some(closure_ptr) = unsafe { method.as_ptr::<Closure>() } {
                    let closure = unsafe { &*closure_ptr.as_ptr() };
                    closure.func_id
                } else {
                    0
                }
            } else if let Some(func_id) = method.as_i32() {
                func_id as usize
            } else {
                return stack.push(method).map(|_| ());
            };

            // Create a wrapper
            let wrapper = FunctionWrapper::new(method_func_id, 0)
                .with_hook_closure(crate::vm::reflect::HookType::Around, wrapper_fn)
                .build()
                .map_err(|e| VmError::RuntimeError(format!("Failed to build wrapper: {:?}", e)))?;
            let wrapper_id = wrapper.id;

            // Register the wrapper
            let mut registry = WRAPPER_FUNCTION_REGISTRY.lock();
            registry.register(wrapper);
            drop(registry);

            // Create a closure that captures: [original_method, wrapper_id, wrapper_fn]
            let captures = vec![method, Value::i32(wrapper_id as i32), wrapper_fn];
            let closure = Closure::new(wrapper_id, captures);

            // Allocate and return the closure
            let closure_gc = ctx.gc.lock().allocate(closure);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(closure_gc.as_ptr()).unwrap()) }
        }

        // ===== Phase 15: Dynamic Bytecode Generation =====

        reflect::NEW_BYTECODE_BUILDER => {
            // newBytecodeBuilder(name, paramCount, returnType) -> builderId
            if args.len() < 3 {
                return Err(VmError::RuntimeError(
                    "newBytecodeBuilder requires 3 arguments (name, paramCount, returnType)".to_string()
                ));
            }
            let name = get_string(args[0].clone())?;
            let param_count = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("paramCount must be a number".to_string()))?
                as usize;
            let return_type = get_string(args[2].clone())?;

            let mut registry = BYTECODE_BUILDER_REGISTRY.lock();
            let builder_id = registry.create_builder(name, param_count, return_type);

            Value::i32(builder_id as i32)
        }

        reflect::BUILDER_EMIT => {
            // builderEmit(builderId, opcode, ...operands) -> emit raw instruction
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "builderEmit requires at least 2 arguments (builderId, opcode)".to_string()
                ));
            }
            let builder_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                as usize;
            let opcode = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("opcode must be a number".to_string()))?
                as u8;

            // Collect operands
            let operands: Vec<u8> = args[2..].iter()
                .filter_map(|v| v.as_i32().map(|n| n as u8))
                .collect();

            let mut registry = BYTECODE_BUILDER_REGISTRY.lock();
            let builder = registry.get_mut(builder_id)
                .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;

            builder.emit(opcode, &operands)?;
            Value::null()
        }

        reflect::BUILDER_EMIT_PUSH => {
            // builderEmitPush(builderId, value) -> emit push constant
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "builderEmitPush requires 2 arguments (builderId, value)".to_string()
                ));
            }
            let builder_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                as usize;
            let value = args[1];

            let mut registry = BYTECODE_BUILDER_REGISTRY.lock();
            let builder = registry.get_mut(builder_id)
                .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;

            // Emit appropriate push based on value type
            if value.is_null() {
                builder.emit_push_null()?;
            } else if let Some(b) = value.as_bool() {
                builder.emit_push_bool(b)?;
            } else if let Some(i) = value.as_i32() {
                builder.emit_push_i32(i)?;
            } else if let Some(f) = value.as_f64() {
                builder.emit_push_f64(f)?;
            } else if value.is_ptr() {
                // Try as string
                if let Ok(s) = get_string(value) {
                    builder.emit_push_string(s)?;
                } else {
                    return Err(VmError::TypeError("Unsupported value type for push".to_string()));
                }
            }

            Value::null()
        }

        reflect::BUILDER_DEFINE_LABEL => {
            // builderDefineLabel(builderId) -> labelId
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "builderDefineLabel requires 1 argument (builderId)".to_string()
                ));
            }
            let builder_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                as usize;

            let mut registry = BYTECODE_BUILDER_REGISTRY.lock();
            let builder = registry.get_mut(builder_id)
                .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;

            let label = builder.define_label();
            Value::i32(label.id as i32)
        }

        reflect::BUILDER_MARK_LABEL => {
            // builderMarkLabel(builderId, labelId) -> mark label position
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "builderMarkLabel requires 2 arguments (builderId, labelId)".to_string()
                ));
            }
            let builder_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                as usize;
            let label_id = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("labelId must be a number".to_string()))?
                as usize;

            let mut registry = BYTECODE_BUILDER_REGISTRY.lock();
            let builder = registry.get_mut(builder_id)
                .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;

            builder.mark_label(crate::vm::reflect::Label { id: label_id })?;
            Value::null()
        }

        reflect::BUILDER_EMIT_JUMP => {
            // builderEmitJump(builderId, labelId) -> emit unconditional jump
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "builderEmitJump requires 2 arguments (builderId, labelId)".to_string()
                ));
            }
            let builder_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                as usize;
            let label_id = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("labelId must be a number".to_string()))?
                as usize;

            let mut registry = BYTECODE_BUILDER_REGISTRY.lock();
            let builder = registry.get_mut(builder_id)
                .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;

            builder.emit_jump(crate::vm::reflect::Label { id: label_id })?;
            Value::null()
        }

        reflect::BUILDER_EMIT_JUMP_IF => {
            // builderEmitJumpIf(builderId, labelId, ifTrue) -> emit conditional jump
            if args.len() < 3 {
                return Err(VmError::RuntimeError(
                    "builderEmitJumpIf requires 3 arguments (builderId, labelId, ifTrue)".to_string()
                ));
            }
            let builder_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                as usize;
            let label_id = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("labelId must be a number".to_string()))?
                as usize;
            let if_true = args[2].as_bool().unwrap_or(false);

            let mut registry = BYTECODE_BUILDER_REGISTRY.lock();
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
            // builderDeclareLocal(builderId, typeName) -> localIndex
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "builderDeclareLocal requires 2 arguments (builderId, typeName)".to_string()
                ));
            }
            let builder_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                as usize;
            let type_name = get_string(args[1].clone())?;

            // Map type name to StackType
            let stack_type = match type_name.as_str() {
                "number" | "i32" | "i64" | "int" => StackType::Integer,
                "f64" | "float" => StackType::Float,
                "boolean" | "bool" => StackType::Boolean,
                "string" => StackType::String,
                "null" => StackType::Null,
                _ => StackType::Object,
            };

            let mut registry = BYTECODE_BUILDER_REGISTRY.lock();
            let builder = registry.get_mut(builder_id)
                .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;

            let index = builder.declare_local(None, stack_type)?;
            Value::i32(index as i32)
        }

        reflect::BUILDER_EMIT_LOAD_LOCAL => {
            // builderEmitLoadLocal(builderId, index) -> emit load local
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "builderEmitLoadLocal requires 2 arguments (builderId, index)".to_string()
                ));
            }
            let builder_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                as usize;
            let index = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("index must be a number".to_string()))?
                as usize;

            let mut registry = BYTECODE_BUILDER_REGISTRY.lock();
            let builder = registry.get_mut(builder_id)
                .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;

            builder.emit_load_local(index)?;
            Value::null()
        }

        reflect::BUILDER_EMIT_STORE_LOCAL => {
            // builderEmitStoreLocal(builderId, index) -> emit store local
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "builderEmitStoreLocal requires 2 arguments (builderId, index)".to_string()
                ));
            }
            let builder_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                as usize;
            let index = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("index must be a number".to_string()))?
                as usize;

            let mut registry = BYTECODE_BUILDER_REGISTRY.lock();
            let builder = registry.get_mut(builder_id)
                .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;

            builder.emit_store_local(index)?;
            Value::null()
        }

        reflect::BUILDER_EMIT_CALL => {
            // builderEmitCall(builderId, functionId, argCount) -> emit function call
            if args.len() < 3 {
                return Err(VmError::RuntimeError(
                    "builderEmitCall requires 3 arguments (builderId, functionId, argCount)".to_string()
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

            let mut registry = BYTECODE_BUILDER_REGISTRY.lock();
            let builder = registry.get_mut(builder_id)
                .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;

            builder.emit_call(function_id, arg_count)?;
            Value::null()
        }

        reflect::BUILDER_EMIT_RETURN => {
            // builderEmitReturn(builderId, hasValue) -> emit return
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "builderEmitReturn requires at least 1 argument (builderId)".to_string()
                ));
            }
            let builder_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                as usize;
            let has_value = args.get(1).and_then(|v| v.as_bool()).unwrap_or(true);

            let mut registry = BYTECODE_BUILDER_REGISTRY.lock();
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
            // builderValidate(builderId) -> validation result object
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "builderValidate requires 1 argument (builderId)".to_string()
                ));
            }
            let builder_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                as usize;

            let mut registry = BYTECODE_BUILDER_REGISTRY.lock();
            let builder = registry.get_mut(builder_id)
                .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;

            let result = builder.validate();

            // Create result object with isValid and errors
            let mut map = MapObject::new();

            let valid_key = RayaString::new("isValid".to_string());
            let valid_key_gc = ctx.gc.lock().allocate(valid_key);
            let valid_key_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(valid_key_gc.as_ptr()).unwrap()) };
            map.set(valid_key_v, Value::bool(result.is_valid));

            let errors_key = RayaString::new("errors".to_string());
            let errors_key_gc = ctx.gc.lock().allocate(errors_key);
            let errors_key_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(errors_key_gc.as_ptr()).unwrap()) };

            let mut errors_arr = Array::new(0, result.errors.len());
            for (i, err) in result.errors.iter().enumerate() {
                let s = RayaString::new(err.clone());
                let s_gc = ctx.gc.lock().allocate(s);
                let s_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(s_gc.as_ptr()).unwrap()) };
                errors_arr.set(i, s_v).ok();
            }
            let errors_gc = ctx.gc.lock().allocate(errors_arr);
            let errors_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(errors_gc.as_ptr()).unwrap()) };
            map.set(errors_key_v, errors_v);

            let map_gc = ctx.gc.lock().allocate(map);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(map_gc.as_ptr()).unwrap()) }
        }

        reflect::BUILDER_BUILD_FUNCTION => {
            // builderBuildFunction(builderId) -> functionId
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "builderBuildFunction requires 1 argument (builderId)".to_string()
                ));
            }
            let builder_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("builderId must be a number".to_string()))?
                as usize;

            let mut registry = BYTECODE_BUILDER_REGISTRY.lock();
            let builder = registry.get_mut(builder_id)
                .ok_or_else(|| VmError::RuntimeError(format!("BytecodeBuilder {} not found", builder_id)))?;

            let func = builder.build()?;
            let func_id = func.function_id;
            registry.register_function(func);

            Value::i32(func_id as i32)
        }

        reflect::EXTEND_MODULE => {
            // extendModule(moduleName, additions) -> extend module with dynamic code
            // For now, this is a stub that returns null
            // Full implementation requires module registry integration
            Value::null()
        }

        // ===== Phase 16: Reflection Security & Permissions =====

        reflect::SET_PERMISSIONS => {
            // setPermissions(target, permissions) -> set object-level permissions
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "setPermissions requires 2 arguments (target, permissions)".to_string()
                ));
            }
            let target = args[0];
            let perms_val = args[1];

            // Get object ID from target pointer
            let object_id = get_object_identity(target)
                .ok_or_else(|| VmError::TypeError("setPermissions: target must be an object".to_string()))?;

            // Parse permissions (can be number or string)
            let perms = if let Some(bits) = perms_val.as_i32() {
                ReflectionPermission::from_bits(bits as u8)
            } else if perms_val.is_ptr() {
                let s = get_string(perms_val)?;
                ReflectionPermission::from_combined_str(&s)
                    .ok_or_else(|| VmError::TypeError(format!("Invalid permission: {}", s)))?
            } else {
                return Err(VmError::TypeError("permissions must be a number or string".to_string()));
            };

            let mut store = PERMISSION_STORE.lock();
            store.set_object(object_id, perms)?;
            Value::null()
        }

        reflect::GET_PERMISSIONS => {
            // getPermissions(target) -> get resolved permissions for target
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "getPermissions requires 1 argument (target)".to_string()
                ));
            }
            let target = args[0];

            // Get object ID and class ID
            let object_id = get_object_identity(target);
            let class_id = crate::vm::reflect::get_class_id(target);

            let store = PERMISSION_STORE.lock();
            let perms = store.resolve(object_id, class_id, None);
            Value::i32(perms.bits() as i32)
        }

        reflect::HAS_PERMISSION => {
            // hasPermission(target, permission) -> check specific permission flag
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "hasPermission requires 2 arguments (target, permission)".to_string()
                ));
            }
            let target = args[0];
            let perm_val = args[1];

            // Get object ID and class ID
            let object_id = get_object_identity(target);
            let class_id = crate::vm::reflect::get_class_id(target);

            // Parse permission
            let required = if let Some(bits) = perm_val.as_i32() {
                ReflectionPermission::from_bits(bits as u8)
            } else if perm_val.is_ptr() {
                let s = get_string(perm_val)?;
                ReflectionPermission::from_combined_str(&s)
                    .ok_or_else(|| VmError::TypeError(format!("Invalid permission: {}", s)))?
            } else {
                return Err(VmError::TypeError("permission must be a number or string".to_string()));
            };

            let store = PERMISSION_STORE.lock();
            let has = store.check_permission(object_id, class_id, None, required);
            Value::bool(has)
        }

        reflect::CLEAR_PERMISSIONS => {
            // clearPermissions(target) -> clear object-level permissions
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "clearPermissions requires 1 argument (target)".to_string()
                ));
            }
            let target = args[0];

            let object_id = get_object_identity(target)
                .ok_or_else(|| VmError::TypeError("clearPermissions: target must be an object".to_string()))?;

            let mut store = PERMISSION_STORE.lock();
            store.clear_object(object_id)?;
            Value::null()
        }

        reflect::SET_CLASS_PERMISSIONS => {
            // setClassPermissions(classId, permissions) -> set class-level permissions
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "setClassPermissions requires 2 arguments (classId, permissions)".to_string()
                ));
            }
            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("classId must be a number".to_string()))?
                as usize;
            let perms_val = args[1];

            let perms = if let Some(bits) = perms_val.as_i32() {
                ReflectionPermission::from_bits(bits as u8)
            } else if perms_val.is_ptr() {
                let s = get_string(perms_val)?;
                ReflectionPermission::from_combined_str(&s)
                    .ok_or_else(|| VmError::TypeError(format!("Invalid permission: {}", s)))?
            } else {
                return Err(VmError::TypeError("permissions must be a number or string".to_string()));
            };

            let mut store = PERMISSION_STORE.lock();
            store.set_class(class_id, perms)?;
            Value::null()
        }

        reflect::GET_CLASS_PERMISSIONS => {
            // getClassPermissions(classId) -> get class-level permissions
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "getClassPermissions requires 1 argument (classId)".to_string()
                ));
            }
            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("classId must be a number".to_string()))?
                as usize;

            let store = PERMISSION_STORE.lock();
            match store.get_class(class_id) {
                Some(perms) => Value::i32(perms.bits() as i32),
                None => Value::null()
            }
        }

        reflect::CLEAR_CLASS_PERMISSIONS => {
            // clearClassPermissions(classId) -> clear class-level permissions
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "clearClassPermissions requires 1 argument (classId)".to_string()
                ));
            }
            let class_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("classId must be a number".to_string()))?
                as usize;

            let mut store = PERMISSION_STORE.lock();
            store.clear_class(class_id)?;
            Value::null()
        }

        reflect::SET_MODULE_PERMISSIONS => {
            // setModulePermissions(moduleName, permissions) -> set module-level permissions
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "setModulePermissions requires 2 arguments (moduleName, permissions)".to_string()
                ));
            }
            let module_name = get_string(args[0].clone())?;
            let perms_val = args[1];

            let perms = if let Some(bits) = perms_val.as_i32() {
                ReflectionPermission::from_bits(bits as u8)
            } else if perms_val.is_ptr() {
                let s = get_string(perms_val)?;
                ReflectionPermission::from_combined_str(&s)
                    .ok_or_else(|| VmError::TypeError(format!("Invalid permission: {}", s)))?
            } else {
                return Err(VmError::TypeError("permissions must be a number or string".to_string()));
            };

            let mut store = PERMISSION_STORE.lock();
            store.set_module(&module_name, perms);
            Value::null()
        }

        reflect::GET_MODULE_PERMISSIONS => {
            // getModulePermissions(moduleName) -> get module-level permissions
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "getModulePermissions requires 1 argument (moduleName)".to_string()
                ));
            }
            let module_name = get_string(args[0].clone())?;

            let store = PERMISSION_STORE.lock();
            match store.get_module_resolved(&module_name) {
                Some(perms) => Value::i32(perms.bits() as i32),
                None => Value::null()
            }
        }

        reflect::CLEAR_MODULE_PERMISSIONS => {
            // clearModulePermissions(moduleName) -> clear module-level permissions
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "clearModulePermissions requires 1 argument (moduleName)".to_string()
                ));
            }
            let module_name = get_string(args[0].clone())?;

            let mut store = PERMISSION_STORE.lock();
            store.clear_module(&module_name);
            Value::null()
        }

        reflect::SET_GLOBAL_PERMISSIONS => {
            // setGlobalPermissions(permissions) -> set global default permissions
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "setGlobalPermissions requires 1 argument (permissions)".to_string()
                ));
            }
            let perms_val = args[0];

            let perms = if let Some(bits) = perms_val.as_i32() {
                ReflectionPermission::from_bits(bits as u8)
            } else if perms_val.is_ptr() {
                let s = get_string(perms_val)?;
                ReflectionPermission::from_combined_str(&s)
                    .ok_or_else(|| VmError::TypeError(format!("Invalid permission: {}", s)))?
            } else {
                return Err(VmError::TypeError("permissions must be a number or string".to_string()));
            };

            let mut store = PERMISSION_STORE.lock();
            store.set_global(perms);
            Value::null()
        }

        reflect::GET_GLOBAL_PERMISSIONS => {
            // getGlobalPermissions() -> get global default permissions
            let store = PERMISSION_STORE.lock();
            Value::i32(store.get_global().bits() as i32)
        }

        reflect::SEAL_PERMISSIONS => {
            // sealPermissions(target) -> make permissions immutable
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "sealPermissions requires 1 argument (target)".to_string()
                ));
            }
            let target = args[0];

            // Can seal objects or classes (by class_id)
            if let Some(class_id) = target.as_i32() {
                // Seal class by ID
                let mut store = PERMISSION_STORE.lock();
                store.seal_class(class_id as usize);
            } else if let Some(object_id) = get_object_identity(target) {
                // Seal object
                let mut store = PERMISSION_STORE.lock();
                store.seal_object(object_id);
            } else {
                return Err(VmError::TypeError("sealPermissions: target must be an object or classId".to_string()));
            }
            Value::null()
        }

        reflect::IS_PERMISSIONS_SEALED => {
            // isPermissionsSealed(target) -> check if permissions are sealed
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "isPermissionsSealed requires 1 argument (target)".to_string()
                ));
            }
            let target = args[0];

            let is_sealed = if let Some(class_id) = target.as_i32() {
                let store = PERMISSION_STORE.lock();
                store.is_class_sealed(class_id as usize)
            } else if let Some(object_id) = get_object_identity(target) {
                let store = PERMISSION_STORE.lock();
                store.is_object_sealed(object_id)
            } else {
                false
            };
            Value::bool(is_sealed)
        }

        // ===== Phase 17: Dynamic VM Bootstrap =====

        // ----- Module Creation (0x0E10-0x0E17) -----

        reflect::CREATE_MODULE => {
            // createModule(name) -> create empty dynamic module
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "createModule requires 1 argument (name)".to_string()
                ));
            }
            let name = get_string(args[0].clone())?;

            let mut registry = DYNAMIC_MODULE_REGISTRY.lock();
            let module_id = registry.create_module(name)?;
            Value::i32(module_id as i32)
        }

        reflect::MODULE_ADD_FUNCTION => {
            // moduleAddFunction(moduleId, functionId) -> add function to module
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "moduleAddFunction requires 2 arguments (moduleId, functionId)".to_string()
                ));
            }
            let module_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("moduleId must be a number".to_string()))?
                as usize;
            let function_id = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("functionId must be a number".to_string()))?
                as usize;

            // Get the compiled function from BytecodeBuilderRegistry
            let bytecode_registry = BYTECODE_BUILDER_REGISTRY.lock();
            let func = bytecode_registry.get_function(function_id)
                .ok_or_else(|| VmError::RuntimeError(format!("Function {} not found", function_id)))?
                .clone();
            drop(bytecode_registry);

            let mut registry = DYNAMIC_MODULE_REGISTRY.lock();
            let module = registry.get_mut(module_id)
                .ok_or_else(|| VmError::RuntimeError(format!("Module {} not found", module_id)))?;
            module.add_function(func)?;

            Value::null()
        }

        reflect::MODULE_ADD_CLASS => {
            // moduleAddClass(moduleId, classId, name) -> add class to module
            if args.len() < 3 {
                return Err(VmError::RuntimeError(
                    "moduleAddClass requires 3 arguments (moduleId, classId, name)".to_string()
                ));
            }
            let module_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("moduleId must be a number".to_string()))?
                as usize;
            let class_id = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("classId must be a number".to_string()))?
                as usize;
            let name = get_string(args[2].clone())?;

            let mut registry = DYNAMIC_MODULE_REGISTRY.lock();
            let module = registry.get_mut(module_id)
                .ok_or_else(|| VmError::RuntimeError(format!("Module {} not found", module_id)))?;

            // Use class_id as both local and global ID for now
            module.add_class(class_id, class_id, name)?;

            Value::null()
        }

        reflect::MODULE_ADD_GLOBAL => {
            // moduleAddGlobal(moduleId, name, value) -> add global variable
            if args.len() < 3 {
                return Err(VmError::RuntimeError(
                    "moduleAddGlobal requires 3 arguments (moduleId, name, value)".to_string()
                ));
            }
            let module_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("moduleId must be a number".to_string()))?
                as usize;
            let name = get_string(args[1].clone())?;
            let value = args[2];

            let mut registry = DYNAMIC_MODULE_REGISTRY.lock();
            let module = registry.get_mut(module_id)
                .ok_or_else(|| VmError::RuntimeError(format!("Module {} not found", module_id)))?;
            module.add_global(name, value)?;

            Value::null()
        }

        reflect::MODULE_SEAL => {
            // moduleSeal(moduleId) -> finalize module for execution
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "moduleSeal requires 1 argument (moduleId)".to_string()
                ));
            }
            let module_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("moduleId must be a number".to_string()))?
                as usize;

            let mut registry = DYNAMIC_MODULE_REGISTRY.lock();
            let module = registry.get_mut(module_id)
                .ok_or_else(|| VmError::RuntimeError(format!("Module {} not found", module_id)))?;
            module.seal()?;

            Value::null()
        }

        reflect::MODULE_LINK => {
            // moduleLink(moduleId, imports) -> resolve imports
            // For now, this is a stub - full import resolution requires more infrastructure
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "moduleLink requires 1 argument (moduleId)".to_string()
                ));
            }
            // Stub: just return success
            Value::null()
        }

        reflect::GET_MODULE => {
            // getModule(moduleId) -> get module info by ID
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "getModule requires 1 argument (moduleId)".to_string()
                ));
            }
            let module_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("moduleId must be a number".to_string()))?
                as usize;

            let registry = DYNAMIC_MODULE_REGISTRY.lock();
            match registry.get(module_id) {
                Some(module) => {
                    // Create info object
                    let info = module.get_info();
                    let mut map = MapObject::new();

                    // Add id
                    let id_key = RayaString::new("id".to_string());
                    let id_key_gc = ctx.gc.lock().allocate(id_key);
                    let id_key_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(id_key_gc.as_ptr()).unwrap()) };
                    map.set(id_key_v, Value::i32(info.id as i32));

                    // Add name
                    let name_key = RayaString::new("name".to_string());
                    let name_key_gc = ctx.gc.lock().allocate(name_key);
                    let name_key_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(name_key_gc.as_ptr()).unwrap()) };
                    let name_val = RayaString::new(info.name);
                    let name_val_gc = ctx.gc.lock().allocate(name_val);
                    let name_val_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(name_val_gc.as_ptr()).unwrap()) };
                    map.set(name_key_v, name_val_v);

                    // Add isSealed
                    let sealed_key = RayaString::new("isSealed".to_string());
                    let sealed_key_gc = ctx.gc.lock().allocate(sealed_key);
                    let sealed_key_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(sealed_key_gc.as_ptr()).unwrap()) };
                    map.set(sealed_key_v, Value::bool(info.is_sealed));

                    // Add function count
                    let fc_key = RayaString::new("functionCount".to_string());
                    let fc_key_gc = ctx.gc.lock().allocate(fc_key);
                    let fc_key_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(fc_key_gc.as_ptr()).unwrap()) };
                    map.set(fc_key_v, Value::i32(info.function_count as i32));

                    let map_gc = ctx.gc.lock().allocate(map);
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(map_gc.as_ptr()).unwrap()) }
                }
                None => Value::null()
            }
        }

        reflect::GET_MODULE_BY_NAME => {
            // getModuleByName(name) -> get module ID by name
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "getModuleByName requires 1 argument (name)".to_string()
                ));
            }
            let name = get_string(args[0].clone())?;

            let registry = DYNAMIC_MODULE_REGISTRY.lock();
            match registry.get_by_name(&name) {
                Some(module) => Value::i32(module.id as i32),
                None => Value::null()
            }
        }

        // ----- Execution (0x0E18-0x0E1F) -----

        reflect::EXECUTE => {
            // execute(functionId, argsArray) -> execute function synchronously
            // Note: Full execution requires VM context which we don't have here
            // This is a stub that returns null for now
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "execute requires at least 1 argument (functionId)".to_string()
                ));
            }
            let function_id = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("functionId must be a number".to_string()))?
                as usize;

            // Verify function exists
            let registry = DYNAMIC_MODULE_REGISTRY.lock();
            if registry.get_function(function_id).is_none() {
                // Also check bytecode builder registry
                let bc_registry = BYTECODE_BUILDER_REGISTRY.lock();
                if bc_registry.get_function(function_id).is_none() {
                    return Err(VmError::RuntimeError(
                        format!("Function {} not found", function_id)
                    ));
                }
            }

            // Stub: execution requires VM context passed through
            // Return null to indicate stub behavior
            Value::null()
        }

        reflect::SPAWN => {
            // spawn(functionId, argsArray) -> execute function as Task
            // This is a stub - spawning tasks requires scheduler access
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "spawn requires at least 1 argument (functionId)".to_string()
                ));
            }
            // Stub: return -1 to indicate not implemented
            Value::i32(-1)
        }

        reflect::EVAL => {
            // eval(bytecodeArray) -> execute raw bytecode
            // This is a stub - direct bytecode execution requires VM context
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "eval requires 1 argument (bytecodeArray)".to_string()
                ));
            }
            // Stub: return null
            Value::null()
        }

        reflect::CALL_DYNAMIC => {
            // callDynamic(functionId, argsArray) -> call dynamic function
            // Similar to execute but specifically for dynamic functions
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "callDynamic requires at least 1 argument (functionId)".to_string()
                ));
            }
            // Stub: return null
            Value::null()
        }

        reflect::INVOKE_DYNAMIC_METHOD => {
            // invokeDynamicMethod(target, methodIndex, argsArray) -> invoke method
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "invokeDynamicMethod requires at least 2 arguments (target, methodIndex)".to_string()
                ));
            }
            // Stub: return null
            Value::null()
        }

        // ----- Bootstrap (0x0E20-0x0E2F) -----

        reflect::BOOTSTRAP => {
            // bootstrap() -> initialize minimal runtime environment
            let mut ctx_guard = BOOTSTRAP_CONTEXT.lock();
            if ctx_guard.is_initialized() {
                return Err(VmError::RuntimeError(
                    "Bootstrap context already initialized".to_string()
                ));
            }
            ctx_guard.initialize()?;
            mark_bootstrapped();

            // Return bootstrap info as an object
            let info = ctx_guard.get_info();
            drop(ctx_guard);

            let mut map = MapObject::new();

            // Add objectClassId
            let obj_key = RayaString::new("objectClassId".to_string());
            let obj_key_gc = ctx.gc.lock().allocate(obj_key);
            let obj_key_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_key_gc.as_ptr()).unwrap()) };
            map.set(obj_key_v, Value::i32(info.object_class_id as i32));

            // Add arrayClassId
            let arr_key = RayaString::new("arrayClassId".to_string());
            let arr_key_gc = ctx.gc.lock().allocate(arr_key);
            let arr_key_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_key_gc.as_ptr()).unwrap()) };
            map.set(arr_key_v, Value::i32(info.array_class_id as i32));

            // Add stringClassId
            let str_key = RayaString::new("stringClassId".to_string());
            let str_key_gc = ctx.gc.lock().allocate(str_key);
            let str_key_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(str_key_gc.as_ptr()).unwrap()) };
            map.set(str_key_v, Value::i32(info.string_class_id as i32));

            // Add printNativeId
            let print_key = RayaString::new("printNativeId".to_string());
            let print_key_gc = ctx.gc.lock().allocate(print_key);
            let print_key_v = unsafe { Value::from_ptr(std::ptr::NonNull::new(print_key_gc.as_ptr()).unwrap()) };
            map.set(print_key_v, Value::i32(info.print_native_id as i32));

            let map_gc = ctx.gc.lock().allocate(map);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(map_gc.as_ptr()).unwrap()) }
        }

        reflect::GET_OBJECT_CLASS => {
            // getObjectClass() -> get core Object class ID
            Value::i32(core_class_ids::OBJECT as i32)
        }

        reflect::GET_ARRAY_CLASS => {
            // getArrayClass() -> get core Array class ID
            Value::i32(core_class_ids::ARRAY as i32)
        }

        reflect::GET_STRING_CLASS => {
            // getStringClass() -> get core String class ID
            Value::i32(core_class_ids::STRING as i32)
        }

        reflect::GET_TASK_CLASS => {
            // getTaskClass() -> get core Task class ID
            Value::i32(core_class_ids::TASK as i32)
        }

        reflect::DYNAMIC_PRINT => {
            // dynamicPrint(message) -> print to console
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "dynamicPrint requires 1 argument (message)".to_string()
                ));
            }
            let message = get_string(args[0].clone())?;
            println!("{}", message);
            Value::null()
        }

        reflect::CREATE_DYNAMIC_ARRAY => {
            // createDynamicArray(elements...) -> create array from values
            let len = args.len();
            let mut arr = Array::new(len, 0);
            for (i, val) in args.into_iter().enumerate() {
                arr.set(i, val).ok();
            }
            let gc_ptr = ctx.gc.lock().allocate(arr);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
        }

        reflect::CREATE_DYNAMIC_STRING => {
            // createDynamicString(value) -> create string value
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "createDynamicString requires 1 argument (value)".to_string()
                ));
            }
            let s = if args[0].is_ptr() {
                get_string(args[0].clone())?
            } else if let Some(i) = args[0].as_i32() {
                i.to_string()
            } else if let Some(f) = args[0].as_f64() {
                f.to_string()
            } else if let Some(b) = args[0].as_bool() {
                b.to_string()
            } else if args[0].is_null() {
                "null".to_string()
            } else {
                "[object]".to_string()
            };

            let str_obj = RayaString::new(s);
            let gc_ptr = ctx.gc.lock().allocate(str_obj);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
        }

        reflect::IS_BOOTSTRAPPED => {
            // isBootstrapped() -> check if bootstrap context exists
            Value::bool(is_bootstrapped())
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

/// Get a unique identity for an object based on its pointer address
fn get_object_identity(value: Value) -> Option<usize> {
    if !value.is_ptr() || value.is_null() {
        return None;
    }

    // Use pointer address as identity (same approach as getObjectId)
    if let Some(ptr) = unsafe { value.as_ptr::<Object>() } {
        Some(ptr.as_ptr() as usize)
    } else if let Some(ptr) = unsafe { value.as_ptr::<Array>() } {
        Some(ptr.as_ptr() as usize)
    } else if let Some(ptr) = unsafe { value.as_ptr::<RayaString>() } {
        Some(ptr.as_ptr() as usize)
    } else if let Some(ptr) = unsafe { value.as_ptr::<Closure>() } {
        Some(ptr.as_ptr() as usize)
    } else if let Some(ptr) = unsafe { value.as_ptr::<Proxy>() } {
        Some(ptr.as_ptr() as usize)
    } else if let Some(ptr) = unsafe { value.as_ptr::<MapObject>() } {
        Some(ptr.as_ptr() as usize)
    } else {
        None
    }
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

/// Parse an array of field definition objects into a SubclassDefinition
///
/// Each element of the array should be an object with fields:
/// - name: string (required)
/// - type: string (optional, defaults to "any")
/// - isStatic: boolean (optional, defaults to false)
/// - isReadonly: boolean (optional, defaults to false)
fn parse_fields_array(ctx: &ReflectHandlerContext, value: Value) -> Result<SubclassDefinition, VmError> {
    if !value.is_ptr() || value.is_null() {
        // Empty array - return empty definition
        return Ok(SubclassDefinition::new());
    }

    // Helper to extract string from Value
    let extract_string = |v: Value| -> Option<String> {
        if v.is_ptr() && !v.is_null() {
            if let Some(str_ptr) = unsafe { v.as_ptr::<RayaString>() } {
                let s = unsafe { &*str_ptr.as_ptr() };
                return Some(s.data.clone());
            }
        }
        None
    };

    // Try to interpret as array
    if let Some(arr_ptr) = unsafe { value.as_ptr::<Array>() } {
        let arr = unsafe { &*arr_ptr.as_ptr() };
        let len = arr.len();

        let mut def = SubclassDefinition::new();

        for i in 0..len {
            if let Some(elem) = arr.get(i) {
                // Each element should be an object with field definition
                if elem.is_ptr() && !elem.is_null() {
                    // Try to read as an Object with specific fields
                    // We look for: name, type, isStatic, isReadonly
                    if let Some(class_id) = crate::vm::reflect::get_class_id(elem) {
                        let obj_ptr = unsafe { elem.as_ptr::<Object>() };
                        let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };

                        // Get field metadata to look up fields by name
                        let class_metadata = ctx.class_metadata.read();
                        let meta = class_metadata.get(class_id);

                        let mut field_name: Option<String> = None;
                        let mut field_type = "any".to_string();
                        let mut is_static = false;
                        let mut is_readonly = false;

                        if let Some(m) = meta {
                            // Look up "name" field
                            if let Some(idx) = m.get_field_index("name") {
                                if let Some(val) = obj.get_field(idx) {
                                    field_name = extract_string(val);
                                }
                            }
                            // Look up "type" field
                            if let Some(idx) = m.get_field_index("type") {
                                if let Some(val) = obj.get_field(idx) {
                                    if let Some(t) = extract_string(val) {
                                        field_type = t;
                                    }
                                }
                            }
                            // Look up "isStatic" field
                            if let Some(idx) = m.get_field_index("isStatic") {
                                if let Some(val) = obj.get_field(idx) {
                                    if let Some(b) = val.as_bool() {
                                        is_static = b;
                                    }
                                }
                            }
                            // Look up "isReadonly" field
                            if let Some(idx) = m.get_field_index("isReadonly") {
                                if let Some(val) = obj.get_field(idx) {
                                    if let Some(b) = val.as_bool() {
                                        is_readonly = b;
                                    }
                                }
                            }
                        }
                        drop(class_metadata);

                        if let Some(name) = field_name {
                            let mut field_def = FieldDefinition::new(name, &field_type);
                            if is_static {
                                field_def = field_def.as_static();
                            }
                            if is_readonly {
                                field_def = field_def.as_readonly();
                            }
                            def = def.add_field(field_def);
                        }
                    }
                }
            }
        }

        return Ok(def);
    }

    // Not an array, return empty definition
    Ok(SubclassDefinition::new())
}

/// Parse a bytecode array (array of numbers) into a Vec<u8>
fn parse_bytecode_array(value: Value) -> Result<Vec<u8>, VmError> {
    if !value.is_ptr() || value.is_null() {
        return Ok(Vec::new());
    }

    if let Some(arr_ptr) = unsafe { value.as_ptr::<Array>() } {
        let arr = unsafe { &*arr_ptr.as_ptr() };
        let mut bytecode = Vec::with_capacity(arr.len());

        for i in 0..arr.len() {
            if let Some(elem) = arr.get(i) {
                if let Some(n) = elem.as_i32() {
                    if n < 0 || n > 255 {
                        return Err(VmError::RuntimeError(format!(
                            "Bytecode value {} at index {} out of range (0-255)",
                            n, i
                        )));
                    }
                    bytecode.push(n as u8);
                } else {
                    return Err(VmError::TypeError(format!(
                        "Bytecode array element at index {} must be a number",
                        i
                    )));
                }
            }
        }

        Ok(bytecode)
    } else {
        Err(VmError::TypeError("bytecodeArray must be an array".to_string()))
    }
}

/// Parse a captures array (array of values) into a Vec<Value>
fn parse_captures_array(value: Value) -> Result<Vec<Value>, VmError> {
    if !value.is_ptr() || value.is_null() {
        return Ok(Vec::new());
    }

    if let Some(arr_ptr) = unsafe { value.as_ptr::<Array>() } {
        let arr = unsafe { &*arr_ptr.as_ptr() };
        let mut captures = Vec::with_capacity(arr.len());

        for i in 0..arr.len() {
            if let Some(elem) = arr.get(i) {
                captures.push(elem);
            } else {
                captures.push(Value::null());
            }
        }

        Ok(captures)
    } else {
        Err(VmError::TypeError("capturesArray must be an array".to_string()))
    }
}

/// Parse an array of strings into a Vec<String>
fn parse_string_array(ctx: &ReflectHandlerContext, value: Value) -> Result<Vec<String>, VmError> {
    if !value.is_ptr() || value.is_null() {
        return Ok(Vec::new());
    }

    if let Some(arr_ptr) = unsafe { value.as_ptr::<Array>() } {
        let arr = unsafe { &*arr_ptr.as_ptr() };
        let mut strings = Vec::with_capacity(arr.len());

        for i in 0..arr.len() {
            if let Some(elem) = arr.get(i) {
                if elem.is_ptr() && !elem.is_null() {
                    if let Some(str_ptr) = unsafe { elem.as_ptr::<RayaString>() } {
                        let s = unsafe { &*str_ptr.as_ptr() };
                        strings.push(s.data.clone());
                    } else {
                        return Err(VmError::TypeError(format!(
                            "Array element at index {} must be a string",
                            i
                        )));
                    }
                } else {
                    return Err(VmError::TypeError(format!(
                        "Array element at index {} must be a string",
                        i
                    )));
                }
            }
        }

        Ok(strings)
    } else {
        Err(VmError::TypeError("typeArgsArray must be an array".to_string()))
    }
}

/// Parse an array of values into a Vec<Value>
fn parse_value_array(ctx: &ReflectHandlerContext, value: Value) -> Result<Vec<Value>, VmError> {
    if !value.is_ptr() || value.is_null() {
        return Ok(Vec::new());
    }

    if let Some(arr_ptr) = unsafe { value.as_ptr::<Array>() } {
        let arr = unsafe { &*arr_ptr.as_ptr() };
        let mut values = Vec::with_capacity(arr.len());

        for i in 0..arr.len() {
            if let Some(elem) = arr.get(i) {
                values.push(elem);
            }
        }

        Ok(values)
    } else {
        Err(VmError::TypeError("Expected an array".to_string()))
    }
}

/// Create a DecoratorInfo object from a DecoratorApplication
/// Returns a MapObject with keys: name, args, targetType, propertyKey, parameterIndex
fn create_decorator_info_object(
    ctx: &ReflectHandlerContext,
    decorator: &DecoratorApplication,
) -> Result<Value, VmError> {
    // Use MapObject for dynamic key-value storage
    let mut map = MapObject::new();

    // Helper to create string key
    let create_string_key = |s: &str| -> Value {
        let str_obj = RayaString::new(s.to_string());
        let str_gc = ctx.gc.lock().allocate(str_obj);
        unsafe { Value::from_ptr(std::ptr::NonNull::new(str_gc.as_ptr()).unwrap()) }
    };

    // Set name field
    let name_key = create_string_key("name");
    let name_val = create_string_key(&decorator.name);
    map.set(name_key, name_val);

    // Set targetType field
    let target_type_key = create_string_key("targetType");
    let target_type_val = create_string_key(decorator.target_type.as_str());
    map.set(target_type_key, target_type_val);

    // Set args field (as array)
    let args_key = create_string_key("args");
    let mut args_arr = Array::new(0, decorator.args.len());
    for (i, arg) in decorator.args.iter().enumerate() {
        args_arr.set(i, *arg).ok();
    }
    let args_gc = ctx.gc.lock().allocate(args_arr);
    let args_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(args_gc.as_ptr()).unwrap()) };
    map.set(args_key, args_val);

    // Set propertyKey field (optional)
    let prop_key_key = create_string_key("propertyKey");
    if let Some(ref key) = decorator.property_key {
        let key_val = create_string_key(key);
        map.set(prop_key_key, key_val);
    } else {
        map.set(prop_key_key, Value::null());
    }

    // Set parameterIndex field (optional)
    let param_idx_key = create_string_key("parameterIndex");
    if let Some(idx) = decorator.parameter_index {
        map.set(param_idx_key, Value::i32(idx as i32));
    } else {
        map.set(param_idx_key, Value::null());
    }

    // Allocate the map
    let map_gc = ctx.gc.lock().allocate(map);
    Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new(map_gc.as_ptr()).unwrap()) })
}
