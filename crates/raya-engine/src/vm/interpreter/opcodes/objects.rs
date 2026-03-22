//! Object opcode handlers: nominal allocation, field access, structural field access,
//! object literals, and method binding

use super::native::checked_object_ptr;
use crate::compiler::Module;
use crate::compiler::Opcode;
use crate::vm::gc::header_ptr_from_value_ptr;
use crate::vm::interpreter::execution::{OpcodeResult, ReturnAction};
use crate::vm::interpreter::shared_state::{
    ShapeAdapter, StructuralAdapterKey, StructuralSlotBinding,
};
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{Array, CallableKind, DynProp, Object, RayaString};
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

// Property kernel is now the source of truth for descriptors.
// Old metadata key retained only for callable_virtual_property fallback paths.
const NODE_DESCRIPTOR_METADATA_KEY: &str = "__node_compat_descriptor";

impl<'a> Interpreter<'a> {
    fn load_shape_field_on_non_object(
        &mut self,
        receiver: Value,
        shape_id: u64,
        field_offset: usize,
    ) -> Option<Value> {
        use crate::vm::json::view::{js_classify, JSView};

        let member_name = {
            let names = self.structural_shape_names.read();
            names.get(&shape_id)?.get(field_offset)?.clone()
        };

        let bound_native = |this: &mut Self, native_id: u16| {
            let method = Object::new_bound_native(receiver, native_id);
            let method_ptr = this.gc.lock().allocate(method);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(method_ptr.as_ptr()).unwrap()) }
        };

        match js_classify(receiver) {
            JSView::Arr(ptr) => {
                let arr = unsafe { &*ptr };
                if member_name == "length" {
                    let len = arr.len();
                    Some(if len <= i32::MAX as usize {
                        Value::i32(len as i32)
                    } else {
                        Value::f64(len as f64)
                    })
                } else {
                    super::types::builtin_handle_native_method_id(receiver, &member_name)
                        .map(|native_id| bound_native(self, native_id))
                        .or(Some(Value::null()))
                }
            }
            JSView::Str(ptr) => {
                let s = unsafe { &*ptr };
                if member_name == "length" {
                    Some(Value::i32(s.len() as i32))
                } else {
                    super::types::builtin_handle_native_method_id(receiver, &member_name)
                        .map(|native_id| bound_native(self, native_id))
                        .or(Some(Value::null()))
                }
            }
            _ => None,
        }
    }

    pub(in crate::vm::interpreter) fn nominal_method_slot_by_name(
        &self,
        nominal_type_id: usize,
        method_name: &str,
    ) -> Option<usize> {
        let classes = self.classes.read();
        let mut current_id = Some(nominal_type_id);
        while let Some(class_id) = current_id {
            let class = classes.get_class(class_id)?;
            if let Some(module) = class.module.as_ref() {
                for (slot, function_id) in class.vtable.methods.iter().copied().enumerate() {
                    let Some(function) = module.functions.get(function_id) else {
                        continue;
                    };
                    if function.name == method_name
                        || function.name.ends_with(&format!("::{method_name}"))
                    {
                        return Some(slot);
                    }
                }
            }
            current_id = class.parent_id;
        }
        None
    }

    pub(in crate::vm::interpreter) fn bound_method_value_for_slot(
        &self,
        receiver: Value,
        method_slot: usize,
    ) -> Result<Value, VmError> {
        let receiver = Self::ensure_object_receiver(receiver, "method binding")?;
        let obj = unsafe { &*receiver.as_ptr::<Object>().unwrap().as_ptr() };
        let nominal_type_id = obj.nominal_type_id_usize().ok_or_else(|| {
            VmError::TypeError("Cannot bind method on structural object value".to_string())
        })?;
        let classes = self.classes.read();
        let class = classes.get_class(nominal_type_id).ok_or_else(|| {
            VmError::RuntimeError(format!("Invalid nominal type id: {}", nominal_type_id))
        })?;
        let func_id = class.vtable.get_method(method_slot).ok_or_else(|| {
            VmError::RuntimeError(format!(
                "Invalid method slot: {} for class {}",
                method_slot, class.name
            ))
        })?;
        let mut owner_id = Some(nominal_type_id);
        let mut method_module = class.module.clone();
        while let Some(class_id) = owner_id {
            let Some(owner_class) = classes.get_class(class_id) else {
                break;
            };
            if owner_class
                .module
                .as_ref()
                .is_some_and(|module| module.functions.get(func_id).is_some())
            {
                method_module = owner_class.module.clone();
                break;
            }
            owner_id = owner_class.parent_id;
        }
        drop(classes);

        let callable = if method_module
            .as_ref()
            .and_then(|module| module.functions.get(func_id))
            .is_some_and(|function| function.uses_js_this_slot)
        {
            if let Some(module) = method_module {
                Object::new_closure_with_module(func_id, Vec::new(), module)
            } else {
                Object::new_closure(func_id, Vec::new())
            }
        } else {
            Object::new_bound_method(receiver, func_id, method_module)
        };
        let gc_ptr = self.gc.lock().allocate(callable);
        Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) })
    }

    pub(in crate::vm::interpreter) fn callable_frame_for_value(
        &mut self,
        callable: Value,
        stack: &mut Stack,
        args: &[Value],
        explicit_this: Option<Value>,
        return_action: ReturnAction,
        module: &Module,
        task: &Arc<Task>,
    ) -> Result<Option<OpcodeResult>, VmError> {
        if !callable.is_ptr() {
            return Ok(None);
        }
        let header =
            unsafe { &*header_ptr_from_value_ptr(callable.as_ptr::<u8>().unwrap().as_ptr()) };
        if header.type_id() == std::any::TypeId::of::<Object>() {
            let co = unsafe { &*callable.as_ptr::<Object>().unwrap().as_ptr() };
            if let Some(ref callable_data) = co.callable {
                match &callable_data.kind {
                    CallableKind::BoundMethod { func_id, receiver } => {
                        let receiver_value = explicit_this.unwrap_or(*receiver);
                        let receiver_final = if self.callable_uses_js_this_slot(callable) {
                            self.js_this_value_for_callable(callable, Some(receiver_value))?
                        } else {
                            receiver_value
                        };
                        stack.push(receiver_final)?;
                        for arg in args {
                            stack.push(*arg)?;
                        }
                        return Ok(Some(OpcodeResult::PushFrame {
                            func_id: *func_id,
                            arg_count: args.len() + 1,
                            is_closure: false,
                            closure_val: None,
                            module: callable_data.module.clone(),
                            return_action,
                        }));
                    }
                    CallableKind::BoundNative {
                        native_id,
                        receiver,
                    } => {
                        let recv = explicit_this.unwrap_or(*receiver);
                        return Ok(Some(self.exec_bound_native_method_call(
                            stack,
                            recv,
                            *native_id,
                            args.to_vec(),
                            module,
                            task,
                        )));
                    }
                    CallableKind::Bound {
                        target,
                        this_arg,
                        bound_args,
                        rebind_call_helper,
                        ..
                    } => {
                        let mut combined_args = bound_args.clone();
                        combined_args.extend_from_slice(args);

                        if *rebind_call_helper {
                            let target_callable = *this_arg;
                            let this_a =
                                combined_args.first().copied().unwrap_or(Value::undefined());
                            let rest_args = if combined_args.len() > 1 {
                                combined_args[1..].to_vec()
                            } else {
                                Vec::new()
                            };
                            return self.callable_frame_for_value(
                                target_callable,
                                stack,
                                &rest_args,
                                Some(this_a),
                                return_action,
                                module,
                                task,
                            );
                        }

                        return self.callable_frame_for_value(
                            *target,
                            stack,
                            &combined_args,
                            Some(*this_arg),
                            return_action,
                            module,
                            task,
                        );
                    }
                    CallableKind::Closure { func_id } => {
                        let closure_module = co.callable_module();
                        let mut arg_count = args.len();
                        if self.callable_uses_js_this_slot(callable) {
                            stack
                                .push(self.js_this_value_for_callable(callable, explicit_this)?)?;
                            arg_count += 1;
                        }
                        for arg in args {
                            stack.push(*arg)?;
                        }
                        return Ok(Some(OpcodeResult::PushFrame {
                            func_id: *func_id,
                            arg_count,
                            is_closure: true,
                            closure_val: Some(callable),
                            module: closure_module,
                            return_action,
                        }));
                    }
                }
            }
        }
        if let Some(result) =
            self.call_builtin_constructor_as_function(callable, args, task, module)?
        {
            stack.push(result)?;
            return Ok(Some(OpcodeResult::Continue));
        }
        Ok(None)
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
        let nominal_type_id = obj.nominal_type_id_usize();
        let class_metadata = self.class_metadata.read();
        let from_metadata = nominal_type_id.and_then(|nominal_type_id| {
            class_metadata
                .get(nominal_type_id)
                .and_then(|meta| meta.field_names.get(field_offset))
                .cloned()
                .filter(|name| !name.is_empty())
        });
        if from_metadata.is_some() {
            return from_metadata;
        }
        self.structural_field_name_for_object_offset(obj, field_offset)
    }

    fn field_index_for_value(&self, obj_val: Value, field_name: &str) -> Option<usize> {
        let obj_ptr = unsafe { obj_val.as_ptr::<Object>() }?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let nominal_type_id = obj.nominal_type_id_usize();
        let class_metadata = self.class_metadata.read();
        let from_metadata = nominal_type_id
            .and_then(|nominal_type_id| class_metadata.get(nominal_type_id))
            .and_then(|meta| meta.get_field_index(field_name));
        if from_metadata.is_some() {
            return from_metadata;
        }
        let from_legacy = Self::legacy_field_index_for_layout(field_name, obj.field_count());
        if from_legacy.is_some() {
            return from_legacy;
        }
        self.structural_field_slot_index_for_object(obj, field_name)
    }

    pub(in crate::vm::interpreter) fn build_shape_slot_map_for_object(
        &self,
        obj: &Object,
        required_names: &[String],
    ) -> Option<Vec<StructuralSlotBinding>> {
        let dynamic_binding_for = |name: &str| -> Option<StructuralSlotBinding> {
            let key = self.intern_prop_key(name);
            obj.dyn_props().and_then(|dp| {
                dp.contains_key(key)
                    .then_some(StructuralSlotBinding::Dynamic(key))
            })
        };
        let layout_names = self.layout_field_names_for_object(obj);

        if let Some(nominal_type_id) = obj.nominal_type_id_usize() {
            let class_metadata = self.class_metadata.read();
            let class_meta = class_metadata.get(nominal_type_id).cloned();
            drop(class_metadata);
            return Some(
                required_names
                    .iter()
                    .map(|name| {
                        class_meta
                            .as_ref()
                            .and_then(|meta| meta.get_field_index(name))
                            .and_then(|index| {
                                (index < obj.field_count())
                                    .then_some(StructuralSlotBinding::Field(index))
                            })
                            .or_else(|| {
                                Self::legacy_field_index_for_layout(name, obj.field_count())
                                    .map(StructuralSlotBinding::Field)
                            })
                            .or_else(|| {
                                self.structural_field_slot_index_for_object(obj, name)
                                    .map(StructuralSlotBinding::Field)
                            })
                            .or_else(|| {
                                class_meta
                                    .as_ref()
                                    .and_then(|meta| meta.get_method_index(name))
                                    .map(StructuralSlotBinding::Method)
                            })
                            .or_else(|| {
                                self.nominal_method_slot_by_name(nominal_type_id, name)
                                    .map(StructuralSlotBinding::Method)
                            })
                            .or_else(|| dynamic_binding_for(name))
                            .unwrap_or(StructuralSlotBinding::Missing)
                    })
                    .collect(),
            );
        }

        let actual_names = if layout_names
            .as_ref()
            .is_some_and(|names| names.len() == obj.field_count())
        {
            layout_names
        } else {
            None
        };
        Some(
            required_names
                .iter()
                .map(|name| {
                    actual_names
                        .as_ref()
                        .and_then(|names| names.iter().position(|actual| actual == name))
                        .map(StructuralSlotBinding::Field)
                        .or_else(|| dynamic_binding_for(name))
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
        let debug_structural = std::env::var("RAYA_DEBUG_STRUCTURAL_VIEW").is_ok();
        let adapter_key = StructuralAdapterKey {
            provider_layout: obj.layout_id(),
            required_shape,
        };
        let current_epoch = self
            .layouts
            .read()
            .layout_epoch(obj.layout_id())
            .unwrap_or(0);
        if let Some(adapter) = self
            .structural_shape_adapters
            .read()
            .get(&adapter_key)
            .cloned()
        {
            if adapter.epoch == current_epoch {
                return Some(adapter);
            }
        }

        let required_names = self
            .structural_shape_names
            .read()
            .get(&required_shape)
            .cloned();
        let Some(required_names) = required_names else {
            if debug_structural {
                eprintln!(
                    "[structural-shape] missing shape names layout={} shape={}",
                    obj.layout_id(),
                    required_shape
                );
            }
            return None;
        };
        let slot_map = self.build_shape_slot_map_for_object(obj, &required_names);
        let Some(slot_map) = slot_map else {
            if debug_structural {
                eprintln!(
                    "[structural-shape] cannot build slot map layout={} shape={} names=[{}]",
                    obj.layout_id(),
                    required_shape,
                    required_names.join(",")
                );
            }
            return None;
        };
        if debug_structural {
            let rendered = slot_map
                .iter()
                .enumerate()
                .map(|(idx, binding)| match binding {
                    StructuralSlotBinding::Field(slot) => format!("{idx}->f{slot}"),
                    StructuralSlotBinding::Method(slot) => format!("{idx}->m{slot}"),
                    StructuralSlotBinding::Dynamic(key) => format!("{idx}->d{key}"),
                    StructuralSlotBinding::Missing => format!("{idx}->missing"),
                })
                .collect::<Vec<_>>()
                .join(",");
            eprintln!(
                "[structural-shape] build layout={} shape={} names=[{}] map=[{}]",
                obj.layout_id(),
                required_shape,
                required_names.join(","),
                rendered
            );
        }
        let adapter = Arc::new(ShapeAdapter::from_slot_map(
            obj.layout_id(),
            required_shape,
            &slot_map,
            current_epoch,
        ));
        let mut adapters = self.structural_shape_adapters.write();
        Some(
            adapters
                .entry(adapter_key)
                .or_insert_with(|| adapter.clone())
                .clone(),
        )
    }

    pub(in crate::vm::interpreter) fn get_value_field_by_name(
        &self,
        obj_val: Value,
        field_name: &str,
    ) -> Option<Value> {
        let index = self.field_index_for_value(obj_val, field_name)?;
        let obj_ptr = unsafe { obj_val.as_ptr::<Object>() }?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        obj.get_field(index)
    }

    pub(crate) fn is_field_writable(&self, obj_val: Value, field_name: &str) -> bool {
        // Property kernel: check SlotMeta and DynProp first
        if let Some(obj_ptr) = checked_object_ptr(obj_val) {
            let obj = unsafe { &*obj_ptr.as_ptr() };
            // Check fixed slots via shape
            if let Some(slot_idx) = self.shape_resolve_key(obj.header.layout_id, field_name) {
                if let Some(meta) = obj.slot_meta.get(slot_idx) {
                    return meta.writable;
                }
            }
            // Check dyn_props
            let key_id = self.intern_prop_key(field_name);
            if let Some(prop) = obj.dyn_props.as_deref().and_then(|dp| dp.get(key_id)) {
                return prop.writable;
            }
        }
        // Fallback: callable virtual property descriptor
        self.callable_virtual_property_descriptor(obj_val, field_name)
            .map(|(writable, _, _)| writable)
            .unwrap_or(true)
    }

    pub(crate) fn sync_descriptor_value(&self, _obj_val: Value, _field_name: &str, _value: Value) {
        // No-op: the property kernel (DynProp/SlotMeta) is written directly by
        // the caller. No secondary descriptor object needs syncing.
    }

    pub(crate) fn descriptor_data_value(&self, obj_val: Value, field_name: &str) -> Option<Value> {
        // Property kernel: read from SlotMeta/DynProp
        if let Some(obj_ptr) = checked_object_ptr(obj_val) {
            let obj = unsafe { &*obj_ptr.as_ptr() };
            // Check fixed slots
            if let Some(slot_idx) = self.shape_resolve_key(obj.header.layout_id, field_name) {
                if let Some(meta) = obj.slot_meta.get(slot_idx) {
                    if meta.accessor.is_none() {
                        return obj.fields.get(slot_idx).copied();
                    }
                }
            }
            // Check dyn_props
            let key_id = self.intern_prop_key(field_name);
            if let Some(prop) = obj.dyn_props.as_deref().and_then(|dp| dp.get(key_id)) {
                if !prop.is_accessor {
                    return Some(prop.value);
                }
            }
        }
        None
    }

    pub(crate) fn descriptor_accessor(
        &self,
        obj_val: Value,
        field_name: &str,
        accessor_name: &str,
    ) -> Option<Value> {
        // Property kernel: check SlotMeta and DynProp for accessor get/set
        if let Some(obj_ptr) = checked_object_ptr(obj_val) {
            let obj = unsafe { &*obj_ptr.as_ptr() };
            // Check fixed slots
            if let Some(slot_idx) = self.shape_resolve_key(obj.header.layout_id, field_name) {
                if let Some(meta) = obj.slot_meta.get(slot_idx) {
                    if let Some(ref accessor) = meta.accessor {
                        let val = match accessor_name {
                            "get" => accessor.get,
                            "set" => accessor.set,
                            _ => return None,
                        };
                        if !val.is_undefined() {
                            return Some(val);
                        }
                        return None;
                    }
                }
            }
            // Check dyn_props
            let key_id = self.intern_prop_key(field_name);
            if let Some(prop) = obj.dyn_props.as_deref().and_then(|dp| dp.get(key_id)) {
                if prop.is_accessor {
                    let val = match accessor_name {
                        "get" => prop.get,
                        "set" => prop.set,
                        _ => return None,
                    };
                    if !val.is_undefined() {
                        return Some(val);
                    }
                    return None;
                }
            }
        }
        // Fallback: callable virtual accessor
        self.callable_virtual_accessor_value(obj_val, field_name, accessor_name)
    }

    pub(in crate::vm::interpreter) fn ensure_object_receiver(
        value: Value,
        context: &'static str,
    ) -> Result<Value, VmError> {
        if !value.is_ptr() {
            return Err(VmError::TypeError(format!(
                "Expected object for {}",
                context
            )));
        }

        let header = unsafe { &*header_ptr_from_value_ptr(value.as_ptr::<u8>().unwrap().as_ptr()) };
        if header.type_id() == std::any::TypeId::of::<Object>() {
            return Ok(value);
        }

        let kind = if header.type_id() == std::any::TypeId::of::<Array>() {
            "Array"
        } else if header.type_id() == std::any::TypeId::of::<RayaString>() {
            "RayaString"
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
        task: &Arc<Task>,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::NewType => {
                self.safepoint.poll();
                let local_class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let nominal_type_id = match self.resolve_nominal_type_id(module, local_class_index)
                {
                    Ok(id) => id,
                    Err(error) => return OpcodeResult::Error(error),
                };

                let value = match self.alloc_nominal_instance_value(nominal_type_id) {
                    Ok(value) => value,
                    Err(error) => return OpcodeResult::Error(error),
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadFieldExact => {
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
                let slot_binding = StructuralSlotBinding::Field(field_offset);
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
                    StructuralSlotBinding::Method(_)
                    | StructuralSlotBinding::Dynamic(_)
                    | StructuralSlotBinding::Missing => {
                        unreachable!()
                    }
                };
                if let Some(field_name) = self.field_name_for_offset(obj, field_offset) {
                    if let Some(getter) = self.descriptor_accessor(actual_obj, &field_name, "get") {
                        match self.callable_frame_for_value(
                            getter,
                            stack,
                            &[],
                            Some(actual_obj),
                            ReturnAction::PushReturnValue,
                            module,
                            task,
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
                    let value = self
                        .get_field_value_by_name(actual_obj, &field_name)
                        .unwrap_or(Value::null());
                    if let Err(e) = stack.push(value) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }
                // Missing fields resolve to null. This matches object destructuring defaults
                // and allows optional object properties to be absent at runtime.
                let value = obj.get_field(field_offset).unwrap_or(Value::null());
                if std::env::var("RAYA_DEBUG_FIELD_TRACE").is_ok() {
                    let class_debug = obj
                        .nominal_type_id_usize()
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "structural".to_string());
                    eprintln!(
                        "[field-trace] LoadFieldExact[{}] nominal_type_id={} field_count={} => {:?} (is_ptr={})",
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

                if let Some(value) =
                    self.load_shape_field_on_non_object(obj_val, shape_id, field_offset)
                {
                    if let Err(e) = stack.push(value) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }

                let obj_val = match Self::ensure_object_receiver(obj_val, "shape field access") {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);
                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                let member_name = {
                    let names = self.structural_shape_names.read();
                    names
                        .get(&shape_id)
                        .and_then(|names| names.get(field_offset))
                        .cloned()
                };
                self.record_aot_shape_site(
                    crate::aot_profile::AotSiteKind::LoadFieldShape,
                    obj.layout_id(),
                );
                let slot_binding = self.remap_shape_slot_binding(obj, shape_id, field_offset);
                if let StructuralSlotBinding::Missing = slot_binding {
                    if let Err(e) = stack.push(Value::null()) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }
                if let StructuralSlotBinding::Dynamic(key) = slot_binding {
                    let value = obj
                        .dyn_props()
                        .and_then(|dp| dp.get(key).map(|p| p.value))
                        .unwrap_or(Value::null());
                    if let Err(e) = stack.push(value) {
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
                    StructuralSlotBinding::Method(_)
                    | StructuralSlotBinding::Dynamic(_)
                    | StructuralSlotBinding::Missing => {
                        unreachable!()
                    }
                };
                if let Some(ref field_name) = member_name {
                    if let Some(getter) = self.descriptor_accessor(actual_obj, &field_name, "get") {
                        match self.callable_frame_for_value(
                            getter,
                            stack,
                            &[],
                            Some(actual_obj),
                            ReturnAction::PushReturnValue,
                            module,
                            task,
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
                    let value = self
                        .get_field_value_by_name(actual_obj, &field_name)
                        .unwrap_or(Value::null());
                    if let Err(e) = stack.push(value) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }
                let value = obj.get_field(field_offset).unwrap_or(Value::null());
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreFieldExact => {
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

                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() }.unwrap();
                let slot_binding = StructuralSlotBinding::Field(field_offset);
                let field_offset = match slot_binding {
                    StructuralSlotBinding::Field(offset) => offset,
                    StructuralSlotBinding::Dynamic(_) => {
                        return OpcodeResult::Error(VmError::TypeError(
                            "Cannot assign to dynamic binding through fixed field store"
                                .to_string(),
                        ));
                    }
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
                let field_name = {
                    let obj = unsafe { &*obj_ptr.as_ptr() };
                    self.field_name_for_offset(obj, field_offset)
                };
                if let Some(field_name) = field_name {
                    return match self.set_property_value_via_js_semantics(
                        actual_obj,
                        &field_name,
                        value,
                        actual_obj,
                        task,
                        module,
                    ) {
                        Ok(_) => OpcodeResult::Continue,
                        Err(error) => OpcodeResult::Error(error),
                    };
                }
                let obj = unsafe { &mut *obj_ptr.as_ptr() };
                if let Err(e) = obj.set_field(field_offset, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
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
                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() }.unwrap();
                let member_name = {
                    let names = self.structural_shape_names.read();
                    names
                        .get(&shape_id)
                        .and_then(|names| names.get(field_offset))
                        .cloned()
                };
                let obj = unsafe { &mut *obj_ptr.as_ptr() };
                self.record_aot_shape_site(
                    crate::aot_profile::AotSiteKind::StoreFieldShape,
                    obj.layout_id(),
                );
                let slot_binding = self.remap_shape_slot_binding(obj, shape_id, field_offset);
                let field_offset = match slot_binding {
                    StructuralSlotBinding::Field(offset) => offset,
                    StructuralSlotBinding::Dynamic(key) => {
                        obj.ensure_dyn_props().insert(key, DynProp::data(value));
                        return OpcodeResult::Continue;
                    }
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
                if let Some(ref field_name) = member_name {
                    return match self.set_property_value_via_js_semantics(
                        actual_obj, field_name, value, actual_obj, task, module,
                    ) {
                        Ok(_) => OpcodeResult::Continue,
                        Err(error) => OpcodeResult::Error(error),
                    };
                }
                if let Err(e) = obj.set_field(field_offset, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                OpcodeResult::Continue
            }

            Opcode::OptionalFieldExact => {
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
                let slot_binding = StructuralSlotBinding::Field(field_offset);
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
                    StructuralSlotBinding::Method(_)
                    | StructuralSlotBinding::Dynamic(_)
                    | StructuralSlotBinding::Missing => {
                        unreachable!()
                    }
                };
                let value = if let Some(field_name) = self.field_name_for_offset(obj, field_offset)
                {
                    self.get_field_value_by_name(actual_obj, &field_name)
                        .unwrap_or(Value::null())
                } else {
                    obj.get_field(field_offset).unwrap_or(Value::null())
                };
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

                if let Some(value) =
                    self.load_shape_field_on_non_object(obj_val, shape_id, field_offset)
                {
                    if let Err(e) = stack.push(value) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }

                let obj_val =
                    match Self::ensure_object_receiver(obj_val, "optional shape field access") {
                        Ok(v) => v,
                        Err(e) => return OpcodeResult::Error(e),
                    };

                let actual_obj = crate::vm::reflect::unwrap_proxy_target(obj_val);
                let obj_ptr = unsafe { actual_obj.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                let member_name = {
                    let names = self.structural_shape_names.read();
                    names
                        .get(&shape_id)
                        .and_then(|names| names.get(field_offset))
                        .cloned()
                };
                let slot_binding = self.remap_shape_slot_binding(obj, shape_id, field_offset);
                if let StructuralSlotBinding::Missing = slot_binding {
                    if let Err(e) = stack.push(Value::null()) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }
                if let StructuralSlotBinding::Dynamic(key) = slot_binding {
                    let value = obj
                        .dyn_props()
                        .and_then(|dp| dp.get(key).map(|p| p.value))
                        .unwrap_or(Value::null());
                    if let Err(e) = stack.push(value) {
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
                    StructuralSlotBinding::Method(_)
                    | StructuralSlotBinding::Dynamic(_)
                    | StructuralSlotBinding::Missing => {
                        unreachable!()
                    }
                };
                let value = if let Some(field_name) = member_name {
                    self.get_field_value_by_name(actual_obj, &field_name)
                        .unwrap_or(Value::null())
                } else {
                    obj.get_field(field_offset).unwrap_or(Value::null())
                };
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

                let mut obj = Object::new_structural(layout_id, field_count);
                // Set [[Prototype]] to Object.prototype for JS object literals
                if let Some(object_proto) = self
                    .builtin_global_value("Object")
                    .and_then(|ctor| self.object_constructor_prototype_value(ctor))
                {
                    obj.prototype = object_proto;
                }
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
                let nominal_type_id = obj.nominal_type_id_usize().ok_or_else(|| {
                    VmError::TypeError("Cannot bind method on structural object value".to_string())
                });
                let nominal_type_id = match nominal_type_id {
                    Ok(id) => id,
                    Err(error) => return OpcodeResult::Error(error),
                };
                let classes = self.classes.read();
                let class = match classes.get_class(nominal_type_id) {
                    Some(c) => c,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid nominal type id: {}",
                            nominal_type_id
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

                let bm = Object::new_bound_method(obj_val, func_id, method_module);
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
