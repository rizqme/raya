//! Object opcode handlers: New, LoadField, StoreField, OptionalField, ObjectLiteral, InitObject, BindMethod

use crate::compiler::Module;
use crate::compiler::Opcode;
use crate::vm::gc::GcHeader;
use crate::vm::interpreter::execution::{OpcodeResult, ReturnAction};
use crate::vm::interpreter::shared_state::{
    ShapeAdapter, StructuralAdapterKey, StructuralSlotBinding,
};
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{Array, BoundMethod, Closure, Object, RayaString};
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

const NODE_DESCRIPTOR_METADATA_KEY: &str = "__node_compat_descriptor";

impl<'a> Interpreter<'a> {
    fn bound_method_value_for_slot(
        &mut self,
        receiver: Value,
        method_slot: usize,
    ) -> Result<Value, VmError> {
        let receiver = Self::ensure_object_receiver(receiver, "method binding")?;
        let obj = unsafe { &*receiver.as_ptr::<Object>().unwrap().as_ptr() };
        let class_id = obj.nominal_class_id().ok_or_else(|| {
            VmError::TypeError("Cannot bind method on structural object value".to_string())
        })?;
        let classes = self.classes.read();
        let class = classes
            .get_class(class_id)
            .ok_or_else(|| VmError::RuntimeError(format!("Invalid class index: {}", class_id)))?;
        let func_id = class.vtable.get_method(method_slot).ok_or_else(|| {
            VmError::RuntimeError(format!(
                "Invalid method slot: {} for class {}",
                method_slot, class.name
            ))
        })?;
        let method_module = class.module.clone();
        drop(classes);

        let bm = BoundMethod {
            receiver,
            func_id,
            module: method_module,
        };
        let gc_ptr = self.gc.lock().allocate(bm);
        Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) })
    }

    fn callable_frame_for_value(
        &self,
        callable: Value,
        stack: &mut Stack,
        args: &[Value],
        return_action: ReturnAction,
    ) -> Result<Option<OpcodeResult>, VmError> {
        if !callable.is_ptr() {
            return Ok(None);
        }
        let header = unsafe {
            let hp =
                (callable.as_ptr::<u8>().unwrap().as_ptr()).sub(std::mem::size_of::<GcHeader>());
            &*(hp as *const GcHeader)
        };
        if header.type_id() == std::any::TypeId::of::<BoundMethod>() {
            let bm = unsafe { &*callable.as_ptr::<BoundMethod>().unwrap().as_ptr() };
            stack.push(bm.receiver)?;
            for arg in args {
                stack.push(*arg)?;
            }
            return Ok(Some(OpcodeResult::PushFrame {
                func_id: bm.func_id,
                arg_count: args.len() + 1,
                is_closure: false,
                closure_val: None,
                module: bm.module.clone(),
                return_action,
            }));
        }
        if header.type_id() == std::any::TypeId::of::<Closure>() {
            let closure_module =
                unsafe { &*callable.as_ptr::<Closure>().unwrap().as_ptr() }.module();
            for arg in args {
                stack.push(*arg)?;
            }
            return Ok(Some(OpcodeResult::PushFrame {
                func_id: unsafe { &*callable.as_ptr::<Closure>().unwrap().as_ptr() }.func_id(),
                arg_count: args.len(),
                is_closure: true,
                closure_val: Some(callable),
                module: closure_module,
                return_action,
            }));
        }
        Ok(None)
    }

    fn builtin_field_name_for_class_name(class_name: &str, field_offset: usize) -> Option<String> {
        let name = match class_name {
            "Object" => match field_offset {
                0 => "value",
                1 => "writable",
                2 => "configurable",
                3 => "enumerable",
                4 => "get",
                5 => "set",
                _ => return None,
            },
            "Map" => match field_offset {
                0 => "mapPtr",
                1 => "size",
                _ => return None,
            },
            "Set" => match field_offset {
                0 => "setPtr",
                1 => "size",
                _ => return None,
            },
            "Buffer" => match field_offset {
                0 => "bufferPtr",
                1 => "length",
                _ => return None,
            },
            "AggregateError" => match field_offset {
                0 => "message",
                1 => "name",
                2 => "stack",
                3 => "cause",
                4 => "code",
                5 => "errno",
                6 => "syscall",
                7 => "path",
                8 => "errors",
                _ => return None,
            },
            "Error" | "TypeError" | "RangeError" | "ReferenceError" | "SyntaxError"
            | "URIError" | "EvalError" | "ChannelClosedError" | "AssertionError" => match field_offset {
                0 => "message",
                1 => "name",
                2 => "stack",
                3 => "cause",
                4 => "code",
                5 => "errno",
                6 => "syscall",
                7 => "path",
                _ => return None,
            },
            _ => return None,
        };
        Some(name.to_string())
    }

    fn builtin_field_index_for_class_name(class_name: &str, field_name: &str) -> Option<usize> {
        match class_name {
            "Object" => match field_name {
                "value" => Some(0),
                "writable" => Some(1),
                "configurable" => Some(2),
                "enumerable" => Some(3),
                "get" => Some(4),
                "set" => Some(5),
                _ => None,
            },
            "Map" => match field_name {
                "mapPtr" => Some(0),
                "size" => Some(1),
                _ => None,
            },
            "Set" => match field_name {
                "setPtr" => Some(0),
                "size" => Some(1),
                _ => None,
            },
            "Buffer" => match field_name {
                "bufferPtr" => Some(0),
                "length" => Some(1),
                _ => None,
            },
            "AggregateError" => match field_name {
                "message" => Some(0),
                "name" => Some(1),
                "stack" => Some(2),
                "cause" => Some(3),
                "code" => Some(4),
                "errno" => Some(5),
                "syscall" => Some(6),
                "path" => Some(7),
                "errors" => Some(8),
                _ => None,
            },
            "Error" | "TypeError" | "RangeError" | "ReferenceError" | "SyntaxError"
            | "URIError" | "EvalError" | "ChannelClosedError" | "AssertionError" => match field_name {
                "message" => Some(0),
                "name" => Some(1),
                "stack" => Some(2),
                "cause" => Some(3),
                "code" => Some(4),
                "errno" => Some(5),
                "syscall" => Some(6),
                "path" => Some(7),
                _ => None,
            },
            _ => None,
        }
    }

    fn legacy_field_name_for_layout(field_offset: usize, field_count: usize) -> Option<String> {
        let name = match field_offset {
            0 => "message",
            1 => "name",
            2 => "stack",
            3 => "cause",
            4 => "code",
            5 => "errno",
            6 => "syscall",
            7 => "path",
            8 => "errors",
            _ => return None,
        };
        (field_offset < field_count).then(|| name.to_string())
    }

    fn legacy_field_index_for_layout(field_name: &str, field_count: usize) -> Option<usize> {
        let idx = match field_name {
            "message" => 0,
            "name" => 1,
            "stack" => 2,
            "cause" => 3,
            "code" => 4,
            "errno" => 5,
            "syscall" => 6,
            "path" => 7,
            "errors" => 8,
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

    fn field_name_for_offset(&self, obj: &Object, field_offset: usize) -> Option<String> {
        let nominal_class_id = obj.nominal_class_id();
        let class_metadata = self.class_metadata.read();
        let from_metadata = nominal_class_id.and_then(|class_id| {
            class_metadata
                .get(class_id)
                .and_then(|meta| meta.field_names.get(field_offset))
                .cloned()
                .filter(|name| !name.is_empty())
        });
        if from_metadata.is_some() {
            return from_metadata;
        }
        if obj.is_structural() {
            if let Some(name) = self
                .structural_object_shapes
                .read()
                .get(&obj.layout_id())
                .and_then(|names| names.get(field_offset))
                .cloned()
            {
                return Some(name);
            }
        }
        let class_id = nominal_class_id?;
        let classes = self.classes.read();
        let class_name = classes.get_class(class_id)?.name.as_str();
        if class_name == "Object" && obj.field_count() <= 4 {
            if let Some(name) = Self::legacy_field_name_for_layout(field_offset, obj.field_count())
            {
                return Some(name);
            }
        }
        if let Some(name) = Self::builtin_field_name_for_class_name(class_name, field_offset) {
            return Some(name);
        }
        Self::legacy_field_name_for_layout(field_offset, obj.field_count())
    }

    fn field_index_for_value(&self, obj_val: Value, field_name: &str) -> Option<usize> {
        let obj_ptr = unsafe { obj_val.as_ptr::<Object>() }?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let nominal_class_id = obj.nominal_class_id();
        let class_metadata = self.class_metadata.read();
        let from_metadata = nominal_class_id
            .and_then(|class_id| class_metadata.get(class_id))
            .and_then(|meta| meta.get_field_index(field_name));
        if from_metadata.is_some() {
            return from_metadata;
        }
        if obj.is_structural() {
            if let Some(index) = self
                .structural_object_shapes
                .read()
                .get(&obj.layout_id())
                .and_then(|names| names.iter().position(|name| name == field_name))
            {
                return Some(index);
            }
        }
        let class_id = nominal_class_id?;
        let classes = self.classes.read();
        let class_name = classes.get_class(class_id)?.name.as_str();
        if class_name == "Object" && obj.field_count() <= 4 {
            if let Some(index) = Self::legacy_field_index_for_layout(field_name, obj.field_count())
            {
                return Some(index);
            }
        }
        if let Some(index) = Self::builtin_field_index_for_class_name(class_name, field_name) {
            return Some(index);
        }
        Self::legacy_field_index_for_layout(field_name, obj.field_count())
    }

    pub(in crate::vm::interpreter) fn build_shape_slot_map_for_object(
        &self,
        obj: &Object,
        required_names: &[String],
    ) -> Option<Vec<StructuralSlotBinding>> {
        if let Some(class_id) = obj.nominal_class_id() {
            let class_metadata = self.class_metadata.read();
            let class_meta = class_metadata.get(class_id).cloned();
            drop(class_metadata);
            let class_name = {
                let classes = self.classes.read();
                classes.get_class(class_id).map(|class| class.name.clone())
            };
            return Some(
                required_names
                    .iter()
                    .map(|name| {
                        class_meta
                            .as_ref()
                            .and_then(|meta| meta.get_field_index(name))
                            .and_then(|index| {
                                (index < obj.field_count()).then_some(StructuralSlotBinding::Field(index))
                            })
                            .or_else(|| {
                                class_meta
                                    .as_ref()
                                    .and_then(|meta| meta.get_method_index(name))
                                    .map(StructuralSlotBinding::Method)
                            })
                            .or_else(|| {
                                class_name.as_ref().and_then(|class_name| {
                                    Self::builtin_field_index_for_class_name(class_name, name)
                                        .map(StructuralSlotBinding::Field)
                                })
                            })
                            .unwrap_or(StructuralSlotBinding::Missing)
                    })
                    .collect(),
            );
        }

        let actual_names = self
            .structural_object_shapes
            .read()
            .get(&obj.layout_id())
            .cloned()?;
        Some(
            required_names
                .iter()
                .map(|name| {
                    actual_names
                        .iter()
                        .position(|actual| actual == name)
                        .map(StructuralSlotBinding::Field)
                        .unwrap_or(StructuralSlotBinding::Missing)
                })
                .collect(),
        )
    }

    pub(in crate::vm::interpreter) fn ensure_shape_adapter_for_object(
        &self,
        obj: &Object,
        required_shape: crate::vm::object::ShapeId,
    ) -> Option<Arc<ShapeAdapter>> {
        let adapter_key = StructuralAdapterKey {
            provider_layout: obj.layout_id(),
            required_shape,
        };
        if let Some(adapter) = self.structural_shape_adapters.read().get(&adapter_key).cloned() {
            return Some(adapter);
        }

        let required_names = self
            .structural_shape_names
            .read()
            .get(&required_shape)
            .cloned()?;
        let slot_map = self.build_shape_slot_map_for_object(obj, &required_names)?;
        let adapter = Arc::new(ShapeAdapter::from_slot_map(
            obj.layout_id(),
            required_shape,
            &slot_map,
        ));
        let mut adapters = self.structural_shape_adapters.write();
        Some(
            adapters
                .entry(adapter_key)
                .or_insert_with(|| adapter.clone())
                .clone(),
        )
    }

    fn get_value_field_by_name(&self, obj_val: Value, field_name: &str) -> Option<Value> {
        let index = self.field_index_for_value(obj_val, field_name)?;
        let obj_ptr = unsafe { obj_val.as_ptr::<Object>() }?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        obj.get_field(index)
    }

    fn is_field_writable(&self, obj_val: Value, field_name: &str) -> bool {
        let metadata = self.metadata.lock();
        let Some(descriptor) =
            metadata.get_metadata_property(NODE_DESCRIPTOR_METADATA_KEY, obj_val, field_name)
        else {
            return true;
        };
        let Some(writable) = self.get_value_field_by_name(descriptor, "writable") else {
            return true;
        };
        if let Some(b) = writable.as_bool() {
            b
        } else if let Some(i) = writable.as_i32() {
            i != 0
        } else {
            true
        }
    }

    fn sync_descriptor_value(&self, obj_val: Value, field_name: &str, value: Value) {
        let descriptor = {
            let metadata = self.metadata.lock();
            metadata.get_metadata_property(NODE_DESCRIPTOR_METADATA_KEY, obj_val, field_name)
        };
        let Some(descriptor) = descriptor else {
            return;
        };
        let Some(value_index) = self.field_index_for_value(descriptor, "value") else {
            return;
        };
        if let Some(desc_ptr) = unsafe { descriptor.as_ptr::<Object>() } {
            let desc = unsafe { &mut *desc_ptr.as_ptr() };
            let _ = desc.set_field(value_index, value);
        }
    }

    fn descriptor_accessor(
        &self,
        obj_val: Value,
        field_name: &str,
        accessor_name: &str,
    ) -> Option<Value> {
        let descriptor = {
            let metadata = self.metadata.lock();
            metadata.get_metadata_property(NODE_DESCRIPTOR_METADATA_KEY, obj_val, field_name)
        }?;
        let accessor = self.get_value_field_by_name(descriptor, accessor_name)?;
        if accessor.is_null() {
            return None;
        }
        Some(accessor)
    }

    fn ensure_object_receiver(value: Value, context: &'static str) -> Result<Value, VmError> {
        if !value.is_ptr() {
            return Err(VmError::TypeError(format!(
                "Expected object for {}",
                context
            )));
        }

        let header = unsafe {
            let hp = (value.as_ptr::<u8>().unwrap().as_ptr()).sub(std::mem::size_of::<GcHeader>());
            &*(hp as *const GcHeader)
        };
        if header.type_id() == std::any::TypeId::of::<Object>() {
            return Ok(value);
        }

        let kind = if header.type_id() == std::any::TypeId::of::<Array>() {
            "Array"
        } else if header.type_id() == std::any::TypeId::of::<RayaString>() {
            "RayaString"
        } else if header.type_id() == std::any::TypeId::of::<Closure>() {
            "Closure"
        } else if header.type_id() == std::any::TypeId::of::<BoundMethod>() {
            "BoundMethod"
        } else {
            "UnknownGcType"
        };

        Err(VmError::TypeError(format!(
            "Expected Object receiver for {}, got {}",
            context, kind
        )))
    }

    pub(in crate::vm::interpreter) fn exec_object_ops(
        &mut self,
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        module: &Module,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::New => {
                self.safepoint.poll();
                let local_class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let class_index = match self.resolve_nominal_type_id(module, local_class_index) {
                    Ok(id) => id,
                    Err(error) => return OpcodeResult::Error(error),
                };

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
                let field_count = class.field_count;
                let layout_id = class.layout_id;
                drop(classes);

                let obj = Object::new_nominal(layout_id, class_index as u32, field_count);
                let gc_ptr = self.gc.lock().allocate(obj);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadField => {
                let field_offset = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let obj_val = match Self::ensure_object_receiver(obj_val, "field access") {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Check if the object is a proxy - if so, unwrap to target
                // TODO: Full trap support would call handler.get(target, fieldName)
                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                let slot_binding = self.remap_structural_slot_binding(module, obj, field_offset);
                if let StructuralSlotBinding::Missing = slot_binding {
                    if let Err(e) = stack.push(Value::null()) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }
                if let StructuralSlotBinding::Method(method_slot) = slot_binding {
                    let bound = match self.bound_method_value_for_slot(actual_obj, method_slot) {
                        Ok(value) => value,
                        Err(error) => return OpcodeResult::Error(error),
                    };
                    if let Err(e) = stack.push(bound) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }
                let field_offset = match slot_binding {
                    StructuralSlotBinding::Field(offset) => offset,
                    StructuralSlotBinding::Method(_) | StructuralSlotBinding::Missing => {
                        unreachable!()
                    }
                };
                if let Some(field_name) = self.field_name_for_offset(obj, field_offset) {
                    if let Some(getter) = self.descriptor_accessor(actual_obj, &field_name, "get") {
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
                                    field_name
                                )));
                            }
                            Err(e) => return OpcodeResult::Error(e),
                        }
                    }
                }
                // Missing fields resolve to null. This matches object destructuring defaults
                // and allows optional object properties to be absent at runtime.
                let value = obj.get_field(field_offset).unwrap_or(Value::null());
                if std::env::var("RAYA_DEBUG_FIELD_TRACE").is_ok() {
                    let class_debug = obj
                        .nominal_class_id()
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "structural".to_string());
                    eprintln!(
                        "[field-trace] LoadField[{}] class_id={} field_count={} => {:?} (is_ptr={})",
                        field_offset,
                        class_debug,
                        obj.field_count(),
                        value,
                        value.is_ptr()
                    );
                }
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadFieldShape => {
                let shape_id = match Self::read_u64(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let field_offset = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let obj_val = match Self::ensure_object_receiver(obj_val, "shape field access") {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);
                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                let slot_binding = self.remap_shape_slot_binding(obj, shape_id, field_offset);
                if let StructuralSlotBinding::Missing = slot_binding {
                    if let Err(e) = stack.push(Value::null()) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }
                if let StructuralSlotBinding::Method(method_slot) = slot_binding {
                    let bound = match self.bound_method_value_for_slot(actual_obj, method_slot) {
                        Ok(value) => value,
                        Err(error) => return OpcodeResult::Error(error),
                    };
                    if let Err(e) = stack.push(bound) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }
                let field_offset = match slot_binding {
                    StructuralSlotBinding::Field(offset) => offset,
                    StructuralSlotBinding::Method(_) | StructuralSlotBinding::Missing => {
                        unreachable!()
                    }
                };
                if let Some(field_name) = self.field_name_for_offset(obj, field_offset) {
                    if let Some(getter) = self.descriptor_accessor(actual_obj, &field_name, "get") {
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
                                    field_name
                                )));
                            }
                            Err(e) => return OpcodeResult::Error(e),
                        }
                    }
                }
                let value = obj.get_field(field_offset).unwrap_or(Value::null());
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreField => {
                let field_offset = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let obj_val = match Self::ensure_object_receiver(obj_val, "field access") {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Check if the object is a proxy - if so, unwrap to target
                // TODO: Full trap support would call handler.set(target, fieldName, value)
                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
                let obj = unsafe { &mut *obj_ptr.unwrap().as_ptr() };
                let slot_binding = self.remap_structural_slot_binding(module, obj, field_offset);
                let field_offset = match slot_binding {
                    StructuralSlotBinding::Field(offset) => offset,
                    StructuralSlotBinding::Method(_) => {
                        return OpcodeResult::Error(VmError::TypeError(
                            "Cannot assign to structural method slot".to_string(),
                        ));
                    }
                    StructuralSlotBinding::Missing => {
                        return OpcodeResult::Error(VmError::TypeError(
                            "Cannot write field not present in structural slot view".to_string(),
                        ));
                    }
                };
                if let Some(field_name) = self.field_name_for_offset(obj, field_offset) {
                    if let Some(setter) = self.descriptor_accessor(actual_obj, &field_name, "set") {
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
                                    field_name
                                )));
                            }
                            Err(e) => return OpcodeResult::Error(e),
                        }
                    }
                    if self
                        .descriptor_accessor(actual_obj, &field_name, "get")
                        .is_some()
                        && !self.is_field_writable(actual_obj, &field_name)
                    {
                        return OpcodeResult::Error(VmError::TypeError(format!(
                            "Cannot set property '{}' which has only a getter",
                            field_name
                        )));
                    }
                    if !self.is_field_writable(actual_obj, &field_name) {
                        return OpcodeResult::Error(VmError::TypeError(format!(
                            "Cannot assign to non-writable property '{}'",
                            field_name
                        )));
                    }
                }
                if let Err(e) = obj.set_field(field_offset, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                if let Some(field_name) = self.field_name_for_offset(obj, field_offset) {
                    self.sync_descriptor_value(actual_obj, &field_name, value);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreFieldShape => {
                let shape_id = match Self::read_u64(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let field_offset = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let obj_val = match Self::ensure_object_receiver(obj_val, "shape field access") {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);
                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
                let obj = unsafe { &mut *obj_ptr.unwrap().as_ptr() };
                let slot_binding = self.remap_shape_slot_binding(obj, shape_id, field_offset);
                let field_offset = match slot_binding {
                    StructuralSlotBinding::Field(offset) => offset,
                    StructuralSlotBinding::Method(_) => {
                        return OpcodeResult::Error(VmError::TypeError(
                            "Cannot assign to structural method slot".to_string(),
                        ));
                    }
                    StructuralSlotBinding::Missing => {
                        return OpcodeResult::Error(VmError::TypeError(
                            "Cannot write field not present in structural shape view".to_string(),
                        ));
                    }
                };
                if let Some(field_name) = self.field_name_for_offset(obj, field_offset) {
                    if let Some(setter) = self.descriptor_accessor(actual_obj, &field_name, "set") {
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
                                    field_name
                                )));
                            }
                            Err(e) => return OpcodeResult::Error(e),
                        }
                    }
                    if self
                        .descriptor_accessor(actual_obj, &field_name, "get")
                        .is_some()
                        && !self.is_field_writable(actual_obj, &field_name)
                    {
                        return OpcodeResult::Error(VmError::TypeError(format!(
                            "Cannot set property '{}' which has only a getter",
                            field_name
                        )));
                    }
                    if !self.is_field_writable(actual_obj, &field_name) {
                        return OpcodeResult::Error(VmError::TypeError(format!(
                            "Cannot assign to non-writable property '{}'",
                            field_name
                        )));
                    }
                }
                if let Err(e) = obj.set_field(field_offset, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                if let Some(field_name) = self.field_name_for_offset(obj, field_offset) {
                    self.sync_descriptor_value(actual_obj, &field_name, value);
                }
                OpcodeResult::Continue
            }

            Opcode::OptionalField => {
                let field_offset = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // If null, return null (optional chaining semantics)
                if obj_val.is_null() {
                    if let Err(e) = stack.push(Value::null()) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }

                let obj_val = match Self::ensure_object_receiver(obj_val, "optional field access") {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Check if the object is a proxy - if so, unwrap to target
                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);

                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                let slot_binding = self.remap_structural_slot_binding(module, obj, field_offset);
                if let StructuralSlotBinding::Missing = slot_binding {
                    if let Err(e) = stack.push(Value::null()) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }
                if let StructuralSlotBinding::Method(method_slot) = slot_binding {
                    let bound = match self.bound_method_value_for_slot(actual_obj, method_slot) {
                        Ok(value) => value,
                        Err(error) => return OpcodeResult::Error(error),
                    };
                    if let Err(e) = stack.push(bound) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }
                let field_offset = match slot_binding {
                    StructuralSlotBinding::Field(offset) => offset,
                    StructuralSlotBinding::Method(_) | StructuralSlotBinding::Missing => {
                        unreachable!()
                    }
                };
                let value = obj.get_field(field_offset).unwrap_or(Value::null());
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::OptionalFieldShape => {
                let shape_id = match Self::read_u64(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let field_offset = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if obj_val.is_null() {
                    if let Err(e) = stack.push(Value::null()) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }

                let obj_val = match Self::ensure_object_receiver(obj_val, "optional shape field access")
                {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);
                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                let slot_binding = self.remap_shape_slot_binding(obj, shape_id, field_offset);
                if let StructuralSlotBinding::Missing = slot_binding {
                    if let Err(e) = stack.push(Value::null()) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }
                if let StructuralSlotBinding::Method(method_slot) = slot_binding {
                    let bound = match self.bound_method_value_for_slot(actual_obj, method_slot) {
                        Ok(value) => value,
                        Err(error) => return OpcodeResult::Error(error),
                    };
                    if let Err(e) = stack.push(bound) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }
                let field_offset = match slot_binding {
                    StructuralSlotBinding::Field(offset) => offset,
                    StructuralSlotBinding::Method(_) | StructuralSlotBinding::Missing => {
                        unreachable!()
                    }
                };
                let value = obj.get_field(field_offset).unwrap_or(Value::null());
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ObjectLiteral => {
                self.safepoint.poll();
                let layout_id = match Self::read_u32(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let field_count = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if layout_id == 0 {
                    return OpcodeResult::Error(VmError::RuntimeError(
                        "object literal is missing structural layout id".to_string(),
                    ));
                }

                let obj = Object::new_structural(layout_id, field_count);
                let gc_ptr = self.gc.lock().allocate(obj);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::InitObject => {
                let field_offset = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.peek() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let obj_val = match Self::ensure_object_receiver(obj_val, "field initialization") {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
                let obj = unsafe { &mut *obj_ptr.unwrap().as_ptr() };
                if let Err(e) = obj.set_field(field_offset, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                OpcodeResult::Continue
            }

            Opcode::BindMethod => {
                let method_slot = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let obj_val = match Self::ensure_object_receiver(obj_val, "method binding") {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let obj = unsafe { &*obj_val.as_ptr::<Object>().unwrap().as_ptr() };
                let class_id = obj.nominal_class_id().ok_or_else(|| {
                    VmError::TypeError("Cannot bind method on structural object value".to_string())
                });
                let class_id = match class_id {
                    Ok(id) => id,
                    Err(error) => return OpcodeResult::Error(error),
                };
                let classes = self.classes.read();
                let class = match classes.get_class(class_id) {
                    Some(c) => c,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid class index: {}",
                            class_id
                        )));
                    }
                };
                let func_id = match class.vtable.get_method(method_slot) {
                    Some(fid) => fid,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid method slot: {} for class {}",
                            method_slot, class.name
                        )));
                    }
                };
                let method_module = class.module.clone();
                drop(classes);

                let bm = BoundMethod {
                    receiver: obj_val,
                    func_id,
                    module: method_module,
                };
                let gc_ptr = self.gc.lock().allocate(bm);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            _ => OpcodeResult::Error(VmError::RuntimeError(format!(
                "Unexpected opcode in exec_object_ops: {:?}",
                opcode
            ))),
        }
    }
}
