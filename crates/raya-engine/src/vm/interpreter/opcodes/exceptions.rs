//! Exception handling opcode handlers: Try, EndTry, Throw, Rethrow

use crate::compiler::Opcode;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{Object, RayaString};
use crate::vm::scheduler::{ExceptionHandler, Task};
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_exception_ops(
        &mut self,
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        task: &Arc<Task>,
        frame_depth: usize,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::Try => {
                let catch_rel = match Self::read_i32(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let catch_abs = if catch_rel >= 0 {
                    (*ip as i32 + catch_rel) as i32
                } else {
                    -1
                };

                let finally_rel = match Self::read_i32(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let finally_abs = if finally_rel > 0 {
                    (*ip as i32 + finally_rel) as i32
                } else {
                    -1
                };

                let handler = ExceptionHandler {
                    catch_offset: catch_abs,
                    finally_offset: finally_abs,
                    stack_size: stack.depth(),
                    frame_count: frame_depth,
                    mutex_count: task.held_mutex_count(),
                };
                task.push_exception_handler(handler);
                OpcodeResult::Continue
            }

            Opcode::EndTry => {
                task.pop_exception_handler();
                OpcodeResult::Continue
            }

            Opcode::Throw => {
                let exception = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // If exception is an Error object, set its stack property
                if exception.is_ptr() {
                    if let Some(obj_ptr) = unsafe { exception.as_ptr::<Object>() } {
                        let obj = unsafe { &mut *obj_ptr.as_ptr() };
                        let classes = self.classes.read();

                        // Check if this is an Error or subclass (Error class has "name" and "stack" fields)
                        // Error fields: 0=message, 1=name, 2=stack
                        if let Some(class) = classes.get_class(obj.class_id) {
                            // Check if class is Error or inherits from Error
                            let is_error = class.name == "Error"
                                || class.name == "TypeError"
                                || class.name == "RangeError"
                                || class.name == "ReferenceError"
                                || class.name == "SyntaxError"
                                || class.name == "ChannelClosedError"
                                || class.name == "AssertionError"
                                || class.parent_id.is_some(); // Subclasses have parent

                            if is_error && obj.fields.len() >= 3 {
                                // Get error name and message
                                let error_name = if let Some(name_ptr) =
                                    unsafe { obj.fields[1].as_ptr::<RayaString>() }
                                {
                                    unsafe { &*name_ptr.as_ptr() }.data.clone()
                                } else {
                                    "Error".to_string()
                                };

                                let error_message = if let Some(msg_ptr) =
                                    unsafe { obj.fields[0].as_ptr::<RayaString>() }
                                {
                                    unsafe { &*msg_ptr.as_ptr() }.data.clone()
                                } else {
                                    String::new()
                                };

                                drop(classes);

                                // Build stack trace
                                let stack_trace =
                                    task.build_stack_trace(&error_name, &error_message);

                                // Allocate stack trace string
                                let raya_string = RayaString::new(stack_trace);
                                let gc_ptr = self.gc.lock().allocate(raya_string);
                                let stack_value = unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                    )
                                };

                                // Set stack field (index 2)
                                obj.fields[2] = stack_value;
                            }
                        }
                    }
                }

                // Extract error message for the VmError if it's an Error object
                let error_msg = if exception.is_ptr() {
                    if let Some(obj_ptr) = unsafe { exception.as_ptr::<Object>() } {
                        let obj = unsafe { &*obj_ptr.as_ptr() };
                        if obj.fields.len() >= 1 {
                            if let Some(msg_ptr) =
                                unsafe { obj.fields[0].as_ptr::<RayaString>() }
                            {
                                let msg = unsafe { &*msg_ptr.as_ptr() }.data.clone();
                                if msg.is_empty() {
                                    "throw".to_string()
                                } else {
                                    msg
                                }
                            } else {
                                "throw".to_string()
                            }
                        } else {
                            "throw".to_string()
                        }
                    } else {
                        "throw".to_string()
                    }
                } else {
                    "throw".to_string()
                };

                task.set_exception(exception);
                OpcodeResult::Error(VmError::RuntimeError(error_msg))
            }

            Opcode::Rethrow => {
                if let Some(exception) = task.caught_exception() {
                    task.set_exception(exception);
                    OpcodeResult::Error(VmError::RuntimeError("rethrow".to_string()))
                } else {
                    OpcodeResult::Error(VmError::RuntimeError(
                        "RETHROW with no active exception".to_string(),
                    ))
                }
            }

            _ => OpcodeResult::Error(VmError::RuntimeError(format!(
                "Unexpected opcode in exec_exception_ops: {:?}",
                opcode
            ))),
        }
    }
}
