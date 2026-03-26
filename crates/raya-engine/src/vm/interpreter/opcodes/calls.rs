//! Call opcode handlers: Call, CallMethodExact, CallConstructor, CallSuper

use crate::compiler::{Module, Opcode};
use crate::vm::builtin::mutex;
use crate::vm::gc::header_ptr_from_value_ptr;
use crate::vm::interpreter::execution::{OpcodeResult, ReturnAction};
use crate::vm::interpreter::opcodes::types::builtin_handle_native_method_id;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{Array, CallableKind, Object, RayaString};
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::sync::MutexId;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_bound_native_method_call(
        &mut self,
        stack: &mut Stack,
        receiver: Value,
        native_id: u16,
        mut args: Vec<Value>,
        module: &Module,
        task: &Arc<Task>,
    ) -> OpcodeResult {
        let mut native_args = Vec::with_capacity(args.len() + 1);
        if self.native_callable_uses_receiver(native_id) {
            let receiver = match self.builtin_native_this_value(receiver, native_id) {
                Ok(value) => value,
                Err(error) => return OpcodeResult::Error(error),
            };
            native_args.push(receiver);
        }
        native_args.append(&mut args);
        for arg in &native_args {
            if let Err(error) = stack.push(*arg) {
                return OpcodeResult::Error(error);
            }
        }
        let arg_count_u8 = match u8::try_from(native_args.len()) {
            Ok(v) => v,
            Err(_) => {
                return OpcodeResult::Error(VmError::RuntimeError(
                    "Too many arguments for bound native method call".to_string(),
                ))
            }
        };
        let code = [
            (native_id & 0x00FF) as u8,
            ((native_id >> 8) & 0x00FF) as u8,
            arg_count_u8,
        ];
        let mut native_ip = 0usize;
        self.exec_native_ops(
            stack,
            &mut native_ip,
            &code,
            module,
            task,
            Opcode::NativeCall,
        )
    }

    pub(crate) fn exec_call_ops(
        &mut self,
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        module: &Module,
        task: &Arc<Task>,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::Call => {
                self.safepoint.poll();
                let func_index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if func_index == 0xFFFFFFFF {
                    // Closure call - extract closure from under the args
                    // Stack layout: [..., closure, arg0, arg1, ..., argN]
                    let mut args_tmp = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        match stack.pop() {
                            Ok(v) => args_tmp.push(v),
                            Err(e) => return OpcodeResult::Error(e),
                        }
                    }
                    let closure_val = match stack.pop() {
                        Ok(v) => v,
                        Err(e) => return OpcodeResult::Error(e),
                    };

                    if !closure_val.is_ptr() {
                        // Compatibility path: some lowered function references may
                        // be represented as direct function IDs.
                        let direct_func_id =
                            closure_val.as_i32().map(|v| v as usize).or_else(|| {
                                closure_val.as_f64().and_then(|v| {
                                    if v.is_finite()
                                        && v.fract() == 0.0
                                        && v >= 0.0
                                        && v <= usize::MAX as f64
                                    {
                                        Some(v as usize)
                                    } else {
                                        None
                                    }
                                })
                            });
                        if let Some(func_id) = direct_func_id {
                            if module.functions.get(func_id).is_some() {
                                if module
                                    .functions
                                    .get(func_id)
                                    .is_some_and(|function| function.is_generator)
                                {
                                    let iterator = match self.create_generator_task_object(
                                        func_id,
                                        task.current_module(),
                                        args_tmp.into_iter().rev().collect(),
                                        None,
                                        task,
                                    ) {
                                        Ok(iterator) => iterator,
                                        Err(error) => return OpcodeResult::Error(error),
                                    };
                                    if let Err(e) = stack.push(iterator) {
                                        return OpcodeResult::Error(e);
                                    }
                                    return OpcodeResult::Continue;
                                }
                                // Push args back for direct function frame call.
                                for arg in args_tmp.into_iter().rev() {
                                    if let Err(e) = stack.push(arg) {
                                        return OpcodeResult::Error(e);
                                    }
                                }
                                return OpcodeResult::PushFrame {
                                    func_id,
                                    arg_count,
                                    is_closure: false,
                                    closure_val: None,
                                    module: None,
                                    return_action: ReturnAction::PushReturnValue,
                                };
                            }
                        };
                        let got = if closure_val.is_null() {
                            "null".to_string()
                        } else if let Some(v) = closure_val.as_bool() {
                            format!("bool({})", v)
                        } else if let Some(v) = closure_val.as_i32() {
                            format!("int({})", v)
                        } else if let Some(v) = closure_val.as_f64() {
                            format!("number({})", v)
                        } else {
                            "non-pointer primitive".to_string()
                        };
                        let current_func_id = task.current_func_id();
                        let current_func_name = module
                            .functions
                            .get(current_func_id)
                            .map(|f| f.name.as_str())
                            .unwrap_or("<unknown>");
                        return OpcodeResult::Error(VmError::TypeError(format!(
                            "Expected closure or bound method, got {} (in {}#{})",
                            got, current_func_name, current_func_id
                        )));
                    }

                    let call_args = args_tmp.into_iter().rev().collect::<Vec<_>>();
                    match self.callable_frame_for_value(
                        closure_val,
                        stack,
                        &call_args,
                        None,
                        ReturnAction::PushReturnValue,
                        module,
                        task,
                    ) {
                        Ok(Some(frame)) => frame,
                        Ok(None) => OpcodeResult::Error(VmError::TypeError(
                            "Value is not callable".to_string(),
                        )),
                        Err(error) => OpcodeResult::Error(error),
                    }
                } else {
                    let uses_js_this_slot = module
                        .functions
                        .get(func_index)
                        .map(|function| function.uses_js_this_slot)
                        .unwrap_or(false);
                    if uses_js_this_slot {
                        let mut args_tmp = Vec::with_capacity(arg_count);
                        for _ in 0..arg_count {
                            match stack.pop() {
                                Ok(v) => args_tmp.push(v),
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        }
                        let implicit_this = if module
                            .functions
                            .get(func_index)
                            .is_some_and(|function| function.is_strict_js)
                        {
                            Value::undefined()
                        } else {
                            self.builtin_global_value("globalThis")
                                .unwrap_or(Value::undefined())
                        };
                        if let Err(e) = stack.push(implicit_this) {
                            return OpcodeResult::Error(e);
                        }
                        for arg in args_tmp.into_iter().rev() {
                            if let Err(e) = stack.push(arg) {
                                return OpcodeResult::Error(e);
                            }
                        }
                    }
                    if std::env::var("RAYA_DEBUG_VM_CALLS").is_ok() {
                        let name = module
                            .functions
                            .get(func_index)
                            .map(|f| f.name.as_str())
                            .unwrap_or("<invalid>");
                        eprintln!(
                            "[vm] Call func_index={} arg_count={} name={}",
                            func_index, arg_count, name
                        );
                    }
                    let total_arg_count = arg_count + usize::from(uses_js_this_slot);
                    if module
                        .functions
                        .get(func_index)
                        .is_some_and(|function| function.is_generator)
                    {
                        let args_start = match stack.depth().checked_sub(total_arg_count) {
                            Some(pos) => pos,
                            None => return OpcodeResult::Error(VmError::StackUnderflow),
                        };
                        let mut frame_args = Vec::with_capacity(total_arg_count);
                        for offset in 0..total_arg_count {
                            match stack.peek_at(args_start + offset) {
                                Ok(value) => frame_args.push(value),
                                Err(error) => return OpcodeResult::Error(error),
                            }
                        }
                        while stack.depth() > args_start {
                            let _ = stack.pop();
                        }
                        let iterator = match self.create_generator_task_object(
                            func_index,
                            task.current_module(),
                            frame_args,
                            None,
                            task,
                        ) {
                            Ok(iterator) => iterator,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        return stack
                            .push(iterator)
                            .map_or_else(OpcodeResult::Error, |_| OpcodeResult::Continue);
                    }
                    // Regular function call - args are already on the stack
                    OpcodeResult::PushFrame {
                        func_id: func_index,
                        arg_count: total_arg_count,
                        is_closure: false,
                        closure_val: None,
                        module: None,
                        return_action: ReturnAction::PushReturnValue,
                    }
                }
            }

            Opcode::CallStatic => {
                self.safepoint.poll();
                let func_index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let uses_js_this_slot = module
                    .functions
                    .get(func_index)
                    .map(|function| function.uses_js_this_slot)
                    .unwrap_or(false);
                if uses_js_this_slot {
                    let mut args_tmp = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        match stack.pop() {
                            Ok(v) => args_tmp.push(v),
                            Err(e) => return OpcodeResult::Error(e),
                        }
                    }
                    let implicit_this = if module
                        .functions
                        .get(func_index)
                        .is_some_and(|function| function.is_strict_js)
                    {
                        Value::undefined()
                    } else {
                        self.builtin_global_value("globalThis")
                            .unwrap_or(Value::undefined())
                    };
                    if let Err(e) = stack.push(implicit_this) {
                        return OpcodeResult::Error(e);
                    }
                    for arg in args_tmp.into_iter().rev() {
                        if let Err(e) = stack.push(arg) {
                            return OpcodeResult::Error(e);
                        }
                    }
                }
                let total_arg_count = arg_count + usize::from(uses_js_this_slot);
                if module
                    .functions
                    .get(func_index)
                    .is_some_and(|function| function.is_generator)
                {
                    let args_start = match stack.depth().checked_sub(total_arg_count) {
                        Some(pos) => pos,
                        None => return OpcodeResult::Error(VmError::StackUnderflow),
                    };
                    let mut frame_args = Vec::with_capacity(total_arg_count);
                    for offset in 0..total_arg_count {
                        match stack.peek_at(args_start + offset) {
                            Ok(value) => frame_args.push(value),
                            Err(error) => return OpcodeResult::Error(error),
                        }
                    }
                    while stack.depth() > args_start {
                        let _ = stack.pop();
                    }
                    let iterator = match self.create_generator_task_object(
                        func_index,
                        task.current_module(),
                        frame_args,
                        None,
                        task,
                    ) {
                        Ok(iterator) => iterator,
                        Err(error) => return OpcodeResult::Error(error),
                    };
                    return stack
                        .push(iterator)
                        .map_or_else(OpcodeResult::Error, |_| OpcodeResult::Continue);
                }

                OpcodeResult::PushFrame {
                    func_id: func_index,
                    arg_count: total_arg_count,
                    is_closure: false,
                    closure_val: None,
                    module: None,
                    return_action: ReturnAction::PushReturnValue,
                }
            }

            Opcode::CallMethodShape | Opcode::OptionalCallMethodShape => {
                self.safepoint.poll();
                let optional = matches!(opcode, Opcode::OptionalCallMethodShape);
                let shape_id = match Self::read_u64(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let method_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let receiver_pos = match stack.depth().checked_sub(arg_count + 1) {
                    Some(pos) => pos,
                    None => return OpcodeResult::Error(VmError::StackUnderflow),
                };
                let receiver_val = match stack.peek_at(receiver_pos) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if optional && receiver_val.is_null() {
                    for _ in 0..(arg_count + 1) {
                        if let Err(e) = stack.pop() {
                            return OpcodeResult::Error(e);
                        }
                    }
                    if let Err(e) = stack.push(Value::null()) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }

                let shape_method_name = self
                    .structural_shape_names
                    .read()
                    .get(&shape_id)
                    .and_then(|names| names.get(method_index).cloned());
                let debug_shape_call = std::env::var("RAYA_DEBUG_SHAPE_CALL").is_ok();

                if let Some(method_name) = shape_method_name.as_deref() {
                    if !receiver_val.is_ptr() {
                        match method_name {
                            "lock" if arg_count == 0 => {
                                match stack.pop() {
                                    Ok(_) => {}
                                    Err(e) => return OpcodeResult::Error(e),
                                }
                                let mutex_id =
                                    MutexId::from_u64(receiver_val.as_i64().unwrap_or(0) as u64);
                                if let Some(mutex) = self.mutex_registry.get(mutex_id) {
                                    return match mutex.try_lock(task.id()) {
                                        Ok(()) => {
                                            task.add_held_mutex(mutex_id);
                                            if let Err(error) = stack.push(Value::null()) {
                                                OpcodeResult::Error(error)
                                            } else {
                                                OpcodeResult::Continue
                                            }
                                        }
                                        Err(_) => {
                                            task.set_resume_value(Value::null());
                                            OpcodeResult::Suspend(
                                                crate::vm::scheduler::SuspendReason::MutexLockCall { mutex_id },
                                            )
                                        }
                                    };
                                }
                                return OpcodeResult::Error(VmError::RuntimeError(format!(
                                    "Mutex {:?} not found",
                                    mutex_id
                                )));
                            }
                            "unlock" if arg_count == 0 => {
                                match stack.pop() {
                                    Ok(_) => {}
                                    Err(e) => return OpcodeResult::Error(e),
                                }
                                let mutex_id =
                                    MutexId::from_u64(receiver_val.as_i64().unwrap_or(0) as u64);
                                if let Some(mutex) = self.mutex_registry.get(mutex_id) {
                                    return match mutex.unlock(task.id()) {
                                        Ok(next_waiter) => {
                                            task.remove_held_mutex(mutex_id);
                                            if let Some(waiter_id) = next_waiter {
                                                let tasks = self.tasks.read();
                                                if let Some(waiter_task) = tasks.get(&waiter_id) {
                                                    // Only wake tasks that have already parked.
                                                    // If ownership is transferred before the waiter
                                                    // finishes suspending, the reactor will resume
                                                    // it when the suspend result is committed.
                                                    if waiter_task.try_resume() {
                                                        waiter_task.add_held_mutex(mutex_id);
                                                        if matches!(
                                                            waiter_task.suspend_reason(),
                                                            Some(crate::vm::scheduler::SuspendReason::MutexLockCall { .. })
                                                        ) {
                                                            waiter_task
                                                                .set_resume_value(Value::null());
                                                        }
                                                        waiter_task.clear_suspend_reason();
                                                        self.injector.push(waiter_task.clone());
                                                    }
                                                }
                                            }
                                            if let Err(error) = stack.push(Value::null()) {
                                                OpcodeResult::Error(error)
                                            } else {
                                                OpcodeResult::Continue
                                            }
                                        }
                                        Err(e) => OpcodeResult::Error(VmError::RuntimeError(
                                            format!("{}", e),
                                        )),
                                    };
                                }
                                return OpcodeResult::Error(VmError::RuntimeError(format!(
                                    "Mutex {:?} not found",
                                    mutex_id
                                )));
                            }
                            "tryLock" => {
                                let mut args = Vec::with_capacity(arg_count);
                                for _ in 0..arg_count {
                                    match stack.pop() {
                                        Ok(v) => args.push(v),
                                        Err(e) => return OpcodeResult::Error(e),
                                    }
                                }
                                match stack.pop() {
                                    Ok(_) => {}
                                    Err(e) => return OpcodeResult::Error(e),
                                }
                                args.reverse();
                                return self.exec_bound_native_method_call(
                                    stack,
                                    receiver_val,
                                    mutex::TRY_LOCK,
                                    args,
                                    module,
                                    task,
                                );
                            }
                            "isLocked" => {
                                let mut args = Vec::with_capacity(arg_count);
                                for _ in 0..arg_count {
                                    match stack.pop() {
                                        Ok(v) => args.push(v),
                                        Err(e) => return OpcodeResult::Error(e),
                                    }
                                }
                                match stack.pop() {
                                    Ok(_) => {}
                                    Err(e) => return OpcodeResult::Error(e),
                                }
                                args.reverse();
                                return self.exec_bound_native_method_call(
                                    stack,
                                    receiver_val,
                                    mutex::IS_LOCKED,
                                    args,
                                    module,
                                    task,
                                );
                            }
                            _ => {}
                        }
                    }

                    if let Some(native_id) = builtin_handle_native_method_id(
                        self.pinned_handles,
                        receiver_val,
                        method_name,
                    )
                    {
                        let mut args = Vec::with_capacity(arg_count);
                        for _ in 0..arg_count {
                            match stack.pop() {
                                Ok(v) => args.push(v),
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        }
                        match stack.pop() {
                            Ok(_) => {}
                            Err(e) => return OpcodeResult::Error(e),
                        }
                        args.reverse();
                        return self.exec_bound_native_method_call(
                            stack,
                            receiver_val,
                            native_id,
                            args,
                            module,
                            task,
                        );
                    }

                    if self.promise_handle_from_value(receiver_val).is_some()
                        && matches!(method_name, "then" | "catch" | "finally")
                    {
                            let mut args = Vec::with_capacity(arg_count);
                            for _ in 0..arg_count {
                                match stack.pop() {
                                    Ok(v) => args.push(v),
                                    Err(e) => return OpcodeResult::Error(e),
                                }
                            }
                            match stack.pop() {
                                Ok(_) => {}
                                Err(e) => return OpcodeResult::Error(e),
                            }
                            args.reverse();

                            let Some(promise_ctor) = self.builtin_global_value("Promise") else {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "Promise constructor is not available".to_string(),
                                ));
                            };
                            let Some(promise_proto) = self.constructor_prototype_value(promise_ctor)
                            else {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "Promise.prototype is not available".to_string(),
                                ));
                            };
                            let method = match self.get_property_value_via_js_semantics_with_context(
                                promise_proto,
                                method_name,
                                task,
                                module,
                            ) {
                                Ok(Some(value)) if Self::is_callable_value(value) => value,
                                Ok(_) => {
                                    return OpcodeResult::Error(VmError::TypeError(format!(
                                        "Promise.prototype.{} is not callable",
                                        method_name
                                    )))
                                }
                                Err(error) => return OpcodeResult::Error(error),
                            };
                            let result = match self.invoke_callable_sync_with_this(
                                method,
                                Some(receiver_val),
                                &args,
                                task,
                                module,
                            ) {
                                Ok(value) => value,
                                Err(error) => return OpcodeResult::Error(error),
                            };
                            if let Err(error) = stack.push(result) {
                                return OpcodeResult::Error(error);
                            }
                            return OpcodeResult::Continue;
                    }
                }

                let receiver_val = match crate::vm::interpreter::Interpreter::ensure_object_receiver(
                    receiver_val,
                    "shape method call",
                ) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let actual_receiver = crate::vm::reflect::unwrap_proxy_target(receiver_val);
                let obj_ptr = unsafe { actual_receiver.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                let slot_binding = self.remap_shape_slot_binding(obj, shape_id, method_index);
                if debug_shape_call {
                    let binding_name = match &slot_binding {
                        crate::vm::interpreter::shared_state::StructuralSlotBinding::Method(_) => {
                            "method"
                        }
                        crate::vm::interpreter::shared_state::StructuralSlotBinding::Field(_) => {
                            "field"
                        }
                        crate::vm::interpreter::shared_state::StructuralSlotBinding::Dynamic(_) => {
                            "dynamic"
                        }
                        crate::vm::interpreter::shared_state::StructuralSlotBinding::Missing => {
                            "missing"
                        }
                    };
                    eprintln!(
                        "[shape-call] shape={:016x} method_index={} name={} binding={} arg_count={} stack_depth={}",
                        shape_id,
                        method_index,
                        shape_method_name.as_deref().unwrap_or("<unknown>"),
                        binding_name,
                        arg_count,
                        stack.depth()
                    );
                }

                match slot_binding {
                    crate::vm::interpreter::shared_state::StructuralSlotBinding::Method(
                        method_slot,
                    ) => {
                        let nominal_type_id = match obj.nominal_type_id_usize() {
                            Some(id) => id,
                            None => {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "Cannot call structural method on non-nominal object value"
                                        .to_string(),
                                ))
                            }
                        };
                        let classes = self.classes.read();
                        let class = match classes.get_class(nominal_type_id) {
                            Some(c) => c,
                            None => {
                                return OpcodeResult::Error(VmError::RuntimeError(format!(
                                    "Invalid nominal type id: {}",
                                    nominal_type_id
                                )))
                            }
                        };
                        let function_id = match class.vtable.get_method(method_slot) {
                            Some(id) => id,
                            None => {
                                return OpcodeResult::Error(VmError::RuntimeError(format!(
                                    "Method index {} not found in vtable for nominal type '{}' (id={}, vtable_size={})",
                                    method_slot,
                                    class.name,
                                    nominal_type_id,
                                    class.vtable.method_count()
                                )))
                            }
                        };
                        let method_module = class.module.clone();
                        drop(classes);

                        let mut args = Vec::with_capacity(arg_count);
                        for _ in 0..arg_count {
                            match stack.pop() {
                                Ok(v) => args.push(v),
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        }
                        match stack.pop() {
                            Ok(_) => {}
                            Err(e) => return OpcodeResult::Error(e),
                        }

                        if let Err(e) = stack.push(actual_receiver) {
                            return OpcodeResult::Error(e);
                        }
                        for arg in args.into_iter().rev() {
                            if let Err(e) = stack.push(arg) {
                                return OpcodeResult::Error(e);
                            }
                        }

                        OpcodeResult::PushFrame {
                            func_id: function_id,
                            arg_count: arg_count + 1,
                            is_closure: false,
                            closure_val: None,
                            module: method_module,
                            return_action: ReturnAction::PushReturnValue,
                        }
                    }
                    crate::vm::interpreter::shared_state::StructuralSlotBinding::Field(
                        field_offset,
                    ) => {
                        let mut args = Vec::with_capacity(arg_count);
                        for _ in 0..arg_count {
                            match stack.pop() {
                                Ok(v) => args.push(v),
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        }
                        match stack.pop() {
                            Ok(_) => {}
                            Err(e) => return OpcodeResult::Error(e),
                        }

                        let callable = obj.get_field(field_offset).unwrap_or(Value::null());
                        match self.callable_frame_for_value(
                            callable,
                            stack,
                            &args.into_iter().rev().collect::<Vec<_>>(),
                            Some(actual_receiver),
                            ReturnAction::PushReturnValue,
                            module,
                            task,
                        ) {
                            Ok(Some(frame)) => frame,
                            Ok(None) => {
                                return OpcodeResult::Error(VmError::TypeError(format!(
                                    "Structural slot {} is not callable",
                                    method_index
                                )));
                            }
                            Err(e) => return OpcodeResult::Error(e),
                        }
                    }
                    crate::vm::interpreter::shared_state::StructuralSlotBinding::Dynamic(key) => {
                        let mut args = Vec::with_capacity(arg_count);
                        for _ in 0..arg_count {
                            match stack.pop() {
                                Ok(v) => args.push(v),
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        }
                        match stack.pop() {
                            Ok(_) => {}
                            Err(e) => return OpcodeResult::Error(e),
                        }

                        let callable = obj
                            .dyn_props()
                            .and_then(|dp| dp.get(key).map(|p| p.value))
                            .unwrap_or(Value::null());
                        match self.callable_frame_for_value(
                            callable,
                            stack,
                            &args.into_iter().rev().collect::<Vec<_>>(),
                            Some(actual_receiver),
                            ReturnAction::PushReturnValue,
                            module,
                            task,
                        ) {
                            Ok(Some(frame)) => frame,
                            Ok(None) => {
                                return OpcodeResult::Error(VmError::TypeError(format!(
                                    "Structural method slot {} is not callable",
                                    method_index
                                )));
                            }
                            Err(e) => return OpcodeResult::Error(e),
                        }
                    }
                    crate::vm::interpreter::shared_state::StructuralSlotBinding::Missing => {
                        if let Some(method_name) = shape_method_name.as_deref() {
                            let mut args = Vec::with_capacity(arg_count);
                            for _ in 0..arg_count {
                                match stack.pop() {
                                    Ok(v) => args.push(v),
                                    Err(e) => return OpcodeResult::Error(e),
                                }
                            }
                            match stack.pop() {
                                Ok(_) => {}
                                Err(e) => return OpcodeResult::Error(e),
                            }

                            let callable = match self
                                .get_property_value_via_js_semantics_with_context(
                                    actual_receiver,
                                    method_name,
                                    task,
                                    module,
                                ) {
                                Ok(Some(value)) => value,
                                Ok(None) => Value::null(),
                                Err(error) => return OpcodeResult::Error(error),
                            };
                            return match self.callable_frame_for_value(
                                callable,
                                stack,
                                &args.into_iter().rev().collect::<Vec<_>>(),
                                Some(actual_receiver),
                                ReturnAction::PushReturnValue,
                                module,
                                task,
                            ) {
                                Ok(Some(frame)) => frame,
                                Ok(None) => {
                                    return OpcodeResult::Error(VmError::TypeError(format!(
                                        "Structural method slot {} is not callable",
                                        method_index
                                    )));
                                }
                                Err(e) => return OpcodeResult::Error(e),
                            };
                        }
                        return OpcodeResult::Error(VmError::TypeError(format!(
                            "Structural method slot {} is not present on receiver layout",
                            method_index
                        )));
                    }
                }
            }

            Opcode::CallMethodExact | Opcode::OptionalCallMethodExact => {
                self.safepoint.poll();
                let optional = matches!(opcode, Opcode::OptionalCallMethodExact);
                let method_index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let method_id = method_index as u16;
                let receiver_pos = match stack.depth().checked_sub(arg_count + 1) {
                    Some(pos) => pos,
                    None => return OpcodeResult::Error(VmError::StackUnderflow),
                };
                let receiver_val = match stack.peek_at(receiver_pos) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Optional chaining semantics: if receiver is null, skip invocation and return null.
                if optional && receiver_val.is_null() {
                    for _ in 0..(arg_count + 1) {
                        if let Err(e) = stack.pop() {
                            return OpcodeResult::Error(e);
                        }
                    }
                    if let Err(e) = stack.push(Value::null()) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }

                // Check for built-in array methods
                if crate::vm::builtin::is_array_method(method_id) {
                    match self.call_array_method(task, stack, method_id, arg_count, module) {
                        Ok(()) => return OpcodeResult::Continue,
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }

                // Check for built-in string methods
                if crate::vm::builtin::is_string_method(method_id) {
                    match self.call_string_method(task, stack, method_id, arg_count, module) {
                        Ok(()) => return OpcodeResult::Continue,
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }

                // Check for built-in regexp methods
                if crate::vm::builtin::is_regexp_method(method_id) {
                    match self.call_regexp_method(task, stack, method_id, arg_count, module) {
                        Ok(()) => return OpcodeResult::Continue,
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }

                // Check for built-in number methods
                if crate::vm::builtin::is_number_method(method_id) {
                    // Pop arguments
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        match stack.pop() {
                            Ok(v) => args.push(v),
                            Err(e) => return OpcodeResult::Error(e),
                        }
                    }
                    args.reverse();
                    // Pop receiver (number value)
                    let receiver = match stack.pop() {
                        Ok(v) => v,
                        Err(e) => return OpcodeResult::Error(e),
                    };
                    // Prepend receiver as args[0] for NativeCall pattern
                    let mut native_args = Vec::with_capacity(args.len() + 1);
                    native_args.push(receiver);
                    native_args.extend(args);

                    // Dispatch based on method ID
                    let value = native_args[0]
                        .as_f64()
                        .or_else(|| native_args[0].as_i32().map(|v| v as f64))
                        .unwrap_or(0.0);

                    let result_str = match method_id {
                        0x0F00 => {
                            // toFixed(digits)
                            let digits =
                                native_args.get(1).and_then(|v| v.as_i32()).unwrap_or(0) as usize;
                            format!("{:.prec$}", value, prec = digits)
                        }
                        0x0F01 => {
                            // toPrecision(prec?)
                            if native_args.get(1).is_none() {
                                // No precision argument: return plain toString()
                                if value.fract() == 0.0 && value.abs() < i64::MAX as f64 {
                                    format!("{}", value as i64)
                                } else {
                                    format!("{}", value)
                                }
                            } else {
                                let prec = native_args
                                    .get(1)
                                    .and_then(|v| v.as_i32())
                                    .unwrap_or(1)
                                    .max(1) as usize;
                                if value == 0.0 {
                                    format!("{:.prec$}", 0.0, prec = prec - 1)
                                } else {
                                    let magnitude = value.abs().log10().floor() as i32;
                                    if prec as i32 <= magnitude + 1 {
                                        let shift = 10f64.powi(magnitude + 1 - prec as i32);
                                        let rounded = (value / shift).round() * shift;
                                        format!("{}", rounded as i64)
                                    } else {
                                        let decimal_places = (prec as i32 - magnitude - 1) as usize;
                                        format!("{:.prec$}", value, prec = decimal_places)
                                    }
                                }
                            }
                        }
                        0x0F02 => {
                            // toString(radix?)
                            let radix = native_args.get(1).and_then(|v| v.as_i32()).unwrap_or(10);
                            if radix == 10 || !(2..=36).contains(&radix) {
                                if value.fract() == 0.0 && value.abs() < i64::MAX as f64 {
                                    format!("{}", value as i64)
                                } else {
                                    format!("{}", value)
                                }
                            } else {
                                let int_val = value as i64;
                                match radix {
                                    2 => format!("{:b}", int_val),
                                    8 => format!("{:o}", int_val),
                                    16 => format!("{:x}", int_val),
                                    _ => {
                                        if int_val == 0 {
                                            "0".to_string()
                                        } else {
                                            let negative = int_val < 0;
                                            let mut n = int_val.unsigned_abs();
                                            let mut digits = Vec::new();
                                            let r = radix as u64;
                                            while n > 0 {
                                                let d = (n % r) as u8;
                                                digits.push(if d < 10 {
                                                    b'0' + d
                                                } else {
                                                    b'a' + d - 10
                                                });
                                                n /= r;
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
                            }
                        }
                        _ => String::new(),
                    };

                    let s = RayaString::new(result_str);
                    let gc_ptr = self.gc.lock().allocate(s);
                    let val = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                    };
                    if let Err(e) = stack.push(val) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                }

                // Check for built-in reflect methods
                if crate::vm::builtin::is_reflect_method(method_id) {
                    // Pop args from stack into a Vec for call_reflect_method
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        match stack.pop() {
                            Ok(v) => args.push(v),
                            Err(e) => return OpcodeResult::Error(e),
                        }
                    }
                    args.reverse();
                    match self.call_reflect_method(task, stack, method_id, args, module) {
                        Ok(()) => return OpcodeResult::Continue,
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }

                // Check for built-in runtime methods (std:runtime)
                if crate::vm::builtin::is_runtime_method(method_id) {
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        match stack.pop() {
                            Ok(v) => args.push(v),
                            Err(e) => return OpcodeResult::Error(e),
                        }
                    }
                    args.reverse();
                    match self.call_runtime_method(task, stack, method_id, args, module) {
                        Ok(()) => return OpcodeResult::Continue,
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }

                // Fall through to vtable dispatch for user-defined methods
                if !receiver_val.is_ptr() {
                    if std::env::var("RAYA_DEBUG_VM_CALLS").is_ok() {
                        eprintln!(
                            "[vm] CallMethodExact fail: receiver not ptr! method_index={} arg_count={} receiver_raw=0x{:016X}",
                            method_index,
                            arg_count,
                            receiver_val.raw()
                        );
                    }
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected object for method call".to_string(),
                    ));
                }

                // Dynamic vtable dispatch only applies to Object receivers.
                // Strings/arrays/closures have dedicated opcode/native paths.
                let receiver_header = unsafe {
                    &*header_ptr_from_value_ptr(receiver_val.as_ptr::<u8>().unwrap().as_ptr())
                };
                if receiver_header.type_id() != std::any::TypeId::of::<Object>() {
                    let receiver_kind = if receiver_header.type_id()
                        == std::any::TypeId::of::<Array>()
                    {
                        "Array"
                    } else if receiver_header.type_id() == std::any::TypeId::of::<RayaString>() {
                        "RayaString"
                    } else {
                        "UnknownGcType"
                    };
                    let current_func_id = task.current_func_id();
                    let current_func_name = module
                        .functions
                        .get(current_func_id)
                        .map(|f| f.name.as_str())
                        .unwrap_or("<unknown>");
                    return OpcodeResult::Error(VmError::TypeError(format!(
                        "Expected Object receiver for method call (method_index={}), got {} (in {}#{})",
                        method_index, receiver_kind, current_func_name, current_func_id
                    )));
                }

                let obj_ptr = unsafe { receiver_val.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                let nominal_type_id = match obj.nominal_type_id_usize() {
                    Some(id) => id,
                    None => {
                        return OpcodeResult::Error(VmError::TypeError(
                            "Cannot call method on structural object value".to_string(),
                        ));
                    }
                };

                let classes = self.classes.read();
                let class = match classes.get_class(nominal_type_id) {
                    Some(c) => c,
                    None => {
                        let receiver_kind = {
                            let header = unsafe {
                                &*header_ptr_from_value_ptr(
                                    receiver_val.as_ptr::<u8>().unwrap().as_ptr(),
                                )
                            };
                            if header.type_id() == std::any::TypeId::of::<Object>() {
                                "Object"
                            } else if header.type_id() == std::any::TypeId::of::<Array>() {
                                "Array"
                            } else if header.type_id() == std::any::TypeId::of::<RayaString>() {
                                "RayaString"
                            } else {
                                "UnknownGcType"
                            }
                        };
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid nominal type id: {} (receiver_kind={})",
                            nominal_type_id, receiver_kind
                        )));
                    }
                };

                let function_id = match class.vtable.get_method(method_index) {
                    Some(id) => id,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Method index {} not found in vtable for nominal type '{}' (id={}, vtable_size={})",
                            method_index, class.name, nominal_type_id, class.vtable.method_count()
                        )));
                    }
                };
                let method_module = class.module.clone();
                drop(classes);

                // Frame-based method call: receiver + args are already on the stack
                OpcodeResult::PushFrame {
                    func_id: function_id,
                    arg_count: arg_count + 1, // +1 for receiver (this)
                    is_closure: false,
                    closure_val: None,
                    module: method_module,
                    return_action: ReturnAction::PushReturnValue,
                }
            }

            Opcode::CallConstructor => {
                self.safepoint.poll();
                let local_class_index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let nominal_type_id = match self.resolve_nominal_type_id(module, local_class_index)
                {
                    Ok(id) => id,
                    Err(error) => return OpcodeResult::Error(error),
                };
                let arg_count = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Pop arguments temporarily
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    match stack.pop() {
                        Ok(v) => args.push(v),
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }
                args.reverse();

                // Look up class and create object
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
                let constructor_id = class.get_constructor();
                let constructor_module = class.module.clone();
                drop(classes);

                // Create the object
                let obj_val = match self.alloc_nominal_instance_value(nominal_type_id) {
                    Ok(value) => value,
                    Err(error) => return OpcodeResult::Error(error),
                };

                // If no constructor, just push the object
                let constructor_id = match constructor_id {
                    Some(id) => id,
                    None => {
                        if let Err(e) = stack.push(obj_val) {
                            return OpcodeResult::Error(e);
                        }
                        return OpcodeResult::Continue;
                    }
                };

                // Push object (receiver) and args back onto stack for frame-based call
                if let Err(e) = stack.push(obj_val) {
                    return OpcodeResult::Error(e);
                }
                for arg in args {
                    if let Err(e) = stack.push(arg) {
                        return OpcodeResult::Error(e);
                    }
                }

                // Frame-based constructor call: push obj on return (not constructor's return value)
                OpcodeResult::PushFrame {
                    func_id: constructor_id,
                    arg_count: arg_count + 1, // +1 for receiver (this)
                    is_closure: false,
                    closure_val: None,
                    module: constructor_module,
                    return_action: ReturnAction::PushConstructResult(obj_val),
                }
            }

            Opcode::ConstructType => {
                self.safepoint.poll();
                let local_nominal_type_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let nominal_type_id =
                    match self.resolve_nominal_type_id(module, local_nominal_type_index) {
                        Ok(id) => id,
                        Err(error) => return OpcodeResult::Error(error),
                    };
                let arg_count = match Self::read_u8(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    match stack.pop() {
                        Ok(v) => args.push(v),
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }
                args.reverse();

                let obj_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let Some(object_ptr) = (unsafe { obj_val.as_ptr::<Object>() }) else {
                    return OpcodeResult::Error(VmError::TypeError(format!(
                        "ConstructType expected object receiver for nominal type {}",
                        nominal_type_id
                    )));
                };
                let obj = unsafe { &*object_ptr.as_ptr() };
                let Some(object_nominal_type_id) = obj.nominal_type_id_usize() else {
                    return OpcodeResult::Error(VmError::TypeError(format!(
                        "ConstructType expected nominal object receiver for nominal type {}",
                        nominal_type_id
                    )));
                };
                if object_nominal_type_id != nominal_type_id {
                    return OpcodeResult::Error(VmError::TypeError(format!(
                        "ConstructType receiver nominal_type_id={} does not match target nominal_type_id={}",
                        object_nominal_type_id, nominal_type_id
                    )));
                }

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
                let constructor_id = class.get_constructor();
                let constructor_module = class.module.clone();
                drop(classes);

                let constructor_id = match constructor_id {
                    Some(id) => id,
                    None => {
                        if let Err(e) = stack.push(obj_val) {
                            return OpcodeResult::Error(e);
                        }
                        return OpcodeResult::Continue;
                    }
                };

                if let Err(e) = stack.push(obj_val) {
                    return OpcodeResult::Error(e);
                }
                for arg in args {
                    if let Err(e) = stack.push(arg) {
                        return OpcodeResult::Error(e);
                    }
                }

                OpcodeResult::PushFrame {
                    func_id: constructor_id,
                    arg_count: arg_count + 1,
                    is_closure: false,
                    closure_val: None,
                    module: constructor_module,
                    return_action: ReturnAction::PushConstructResult(obj_val),
                }
            }

            Opcode::CallSuper => {
                let local_class_index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let nominal_type_id = match self.resolve_nominal_type_id(module, local_class_index)
                {
                    Ok(id) => id,
                    Err(error) => return OpcodeResult::Error(error),
                };
                let arg_count = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
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

                let parent_id = match class.parent_id {
                    Some(id) => id,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(
                            "Class has no parent".to_string(),
                        ));
                    }
                };

                let parent_class = match classes.get_class(parent_id) {
                    Some(c) => c,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid parent class ID: {}",
                            parent_id
                        )));
                    }
                };

                let constructor_id = match parent_class.get_constructor() {
                    Some(id) => id,
                    None => {
                        drop(classes);
                        return OpcodeResult::Continue;
                    }
                };
                let parent_module = parent_class.module.clone();
                drop(classes);

                // Frame-based super call: discard return value (constructor void)
                OpcodeResult::PushFrame {
                    func_id: constructor_id,
                    arg_count: arg_count + 1, // +1 for receiver (this)
                    is_closure: false,
                    closure_val: None,
                    module: parent_module,
                    return_action: ReturnAction::Discard,
                }
            }

            _ => OpcodeResult::Error(VmError::RuntimeError(format!(
                "Unexpected opcode in exec_call_ops: {:?}",
                opcode
            ))),
        }
    }
}
