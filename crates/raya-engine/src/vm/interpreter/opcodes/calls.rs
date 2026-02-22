//! Call opcode handlers: Call, CallMethod, CallConstructor, CallSuper

use crate::vm::interpreter::execution::{OpcodeResult, ReturnAction};
use crate::vm::interpreter::Interpreter;
use crate::vm::gc::GcHeader;
use crate::vm::object::{BoundMethod, Closure, Object, RayaString};
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use crate::compiler::{Module, Opcode};
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_call_ops(
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
                        return OpcodeResult::Error(VmError::TypeError(
                            "Expected closure or bound method".to_string(),
                        ));
                    }

                    // Check GcHeader to distinguish BoundMethod from Closure
                    let header = unsafe {
                        let hp = (closure_val.as_ptr::<u8>().unwrap().as_ptr())
                            .sub(std::mem::size_of::<GcHeader>());
                        &*(hp as *const GcHeader)
                    };

                    if header.type_id() == std::any::TypeId::of::<BoundMethod>() {
                        // BoundMethod call — prepend receiver as `this` (locals[0])
                        let bm = unsafe { &*closure_val.as_ptr::<BoundMethod>().unwrap().as_ptr() };
                        // Push receiver first (becomes this = locals[0])
                        if let Err(e) = stack.push(bm.receiver) {
                            return OpcodeResult::Error(e);
                        }
                        // Push args on top of receiver
                        for arg in args_tmp.into_iter().rev() {
                            if let Err(e) = stack.push(arg) {
                                return OpcodeResult::Error(e);
                            }
                        }
                        OpcodeResult::PushFrame {
                            func_id: bm.func_id,
                            arg_count: arg_count + 1, // +1 for receiver
                            is_closure: false,
                            closure_val: None,
                            return_action: ReturnAction::PushReturnValue,
                        }
                    } else {
                        // Closure call - push args back (they become the callee's locals)
                        for arg in args_tmp.into_iter().rev() {
                            if let Err(e) = stack.push(arg) {
                                return OpcodeResult::Error(e);
                            }
                        }

                        let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
                        let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                        let closure_func_id = closure.func_id();

                        OpcodeResult::PushFrame {
                            func_id: closure_func_id,
                            arg_count,
                            is_closure: true,
                            closure_val: Some(closure_val),
                            return_action: ReturnAction::PushReturnValue,
                        }
                    }
                } else {
                    // Regular function call - args are already on the stack
                    OpcodeResult::PushFrame {
                        func_id: func_index,
                        arg_count,
                        is_closure: false,
                        closure_val: None,
                        return_action: ReturnAction::PushReturnValue,
                    }
                }
            }

            Opcode::CallMethod => {
                self.safepoint.poll();
                let method_index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let method_id = method_index as u16;

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
                    let value = native_args[0].as_f64()
                        .or_else(|| native_args[0].as_i32().map(|v| v as f64))
                        .unwrap_or(0.0);

                    let result_str = match method_id {
                        0x0F00 => {
                            // toFixed(digits)
                            let digits = native_args.get(1).and_then(|v| v.as_i32()).unwrap_or(0) as usize;
                            format!("{:.prec$}", value, prec = digits)
                        }
                        0x0F01 => {
                            // toPrecision(prec)
                            let prec = native_args.get(1).and_then(|v| v.as_i32()).unwrap_or(1).max(1) as usize;
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
                                        if int_val == 0 { "0".to_string() }
                                        else {
                                            let negative = int_val < 0;
                                            let mut n = int_val.unsigned_abs();
                                            let mut digits = Vec::new();
                                            let r = radix as u64;
                                            while n > 0 {
                                                let d = (n % r) as u8;
                                                digits.push(if d < 10 { b'0' + d } else { b'a' + d - 10 });
                                                n /= r;
                                            }
                                            digits.reverse();
                                            let s = String::from_utf8(digits).unwrap_or_default();
                                            if negative { format!("-{}", s) } else { s }
                                        }
                                    }
                                }
                            }
                        }
                        _ => String::new(),
                    };

                    let s = RayaString::new(result_str);
                    let gc_ptr = self.gc.lock().allocate(s);
                    let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    if let Err(e) = stack.push(val) { return OpcodeResult::Error(e); }
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

                // Check for built-in map methods
                if crate::vm::builtin::is_map_method(method_id) {
                    match self.call_map_method(task, stack, method_id, arg_count, module) {
                        Ok(()) => return OpcodeResult::Continue,
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }

                // Check for built-in set methods
                if crate::vm::builtin::is_set_method(method_id) {
                    match self.call_set_method(task, stack, method_id, arg_count, module) {
                        Ok(()) => return OpcodeResult::Continue,
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }

                // Check for built-in buffer methods
                if crate::vm::builtin::is_buffer_method(method_id) {
                    match self.call_buffer_method(task, stack, method_id, arg_count, module) {
                        Ok(()) => return OpcodeResult::Continue,
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }

                // Check for built-in channel methods (returns OpcodeResult directly
                // because send/receive can suspend)
                if crate::vm::builtin::is_channel_method(method_id) {
                    return self.call_channel_method(task, stack, method_id, arg_count, module);
                }

                // Fall through to vtable dispatch for user-defined methods
                let receiver_pos = match stack.depth().checked_sub(arg_count + 1) {
                    Some(pos) => pos,
                    None => return OpcodeResult::Error(VmError::StackUnderflow),
                };

                let receiver_val = match stack.peek_at(receiver_pos) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !receiver_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected object for method call".to_string(),
                    ));
                }

                let obj_ptr = unsafe { receiver_val.as_ptr::<Object>() };
                let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };

                let classes = self.classes.read();
                let class = match classes.get_class(obj.class_id) {
                    Some(c) => c,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid class ID: {}",
                            obj.class_id
                        )));
                    }
                };

                let function_id = match class.vtable.get_method(method_index) {
                    Some(id) => id,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Method index {} not found in vtable for class '{}' (id={}, vtable_size={})",
                            method_index, class.name, obj.class_id, class.vtable.method_count()
                        )));
                    }
                };
                drop(classes);

                // Frame-based method call: receiver + args are already on the stack
                OpcodeResult::PushFrame {
                    func_id: function_id,
                    arg_count: arg_count + 1, // +1 for receiver (this)
                    is_closure: false,
                    closure_val: None,
                    return_action: ReturnAction::PushReturnValue,
                }
            }

            Opcode::CallConstructor => {
                self.safepoint.poll();
                let class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u8(code, ip) {
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
                let constructor_id = class.get_constructor();
                drop(classes);

                // Create the object
                let obj = Object::new(class_index, field_count);
                let gc_ptr = self.gc.lock().allocate(obj);
                let obj_val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
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
                    return_action: ReturnAction::PushObject(obj_val),
                }
            }

            Opcode::CallSuper => {
                let class_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u8(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
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
                drop(classes);

                // Frame-based super call: discard return value (constructor void)
                OpcodeResult::PushFrame {
                    func_id: constructor_id,
                    arg_count: arg_count + 1, // +1 for receiver (this)
                    is_closure: false,
                    closure_val: None,
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
