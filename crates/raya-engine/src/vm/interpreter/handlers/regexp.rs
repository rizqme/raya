//! RegExp built-in method handlers

use crate::compiler::Module;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{Array, RayaString, RegExpObject};
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    /// Handle built-in regexp methods
    pub(in crate::vm::interpreter) fn call_regexp_method(
        &mut self,
        _task: &Arc<Task>,
        stack: &mut Stack,
        method_id: u16,
        arg_count: usize,
        _module: &Module,
    ) -> Result<(), VmError> {
        use crate::vm::builtin::regexp;

        // Pop arguments (excluding receiver)
        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(stack.pop()?);
        }
        args.reverse();

        // Pop receiver (the RegExp handle)
        let receiver = stack.pop()?;
        let handle = receiver.as_u64().ok_or_else(|| {
            VmError::TypeError("Expected RegExp handle".to_string())
        })?;
        let re_ptr = handle as *const RegExpObject;
        if re_ptr.is_null() {
            return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
        }
        let re = unsafe { &*re_ptr };

        match method_id {
            id if id == regexp::TEST => {
                let input = if !args.is_empty() && args[0].is_ptr() {
                    if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                        unsafe { &*s.as_ptr() }.data.clone()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                stack.push(Value::bool(re.test(&input)))?;
            }
            id if id == regexp::EXEC => {
                let input = if !args.is_empty() && args[0].is_ptr() {
                    if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                        unsafe { &*s.as_ptr() }.data.clone()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                match re.exec(&input) {
                    Some((matched, index, groups)) => {
                        let mut arr = Array::new(0, 0);
                        let matched_str = RayaString::new(matched);
                        let gc_ptr = self.gc.lock().allocate(matched_str);
                        let matched_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        arr.push(matched_val);
                        arr.push(Value::i32(index as i32));
                        for group in groups {
                            let group_str = RayaString::new(group);
                            let gc_ptr = self.gc.lock().allocate(group_str);
                            let group_val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            };
                            arr.push(group_val);
                        }
                        let arr_gc = self.gc.lock().allocate(arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        stack.push(arr_val)?;
                    }
                    None => {
                        stack.push(Value::null())?;
                    }
                }
            }
            id if id == regexp::EXEC_ALL => {
                let input = if !args.is_empty() && args[0].is_ptr() {
                    if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                        unsafe { &*s.as_ptr() }.data.clone()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
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
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        match_arr.push(group_val);
                    }
                    let match_arr_gc = self.gc.lock().allocate(match_arr);
                    let match_arr_val = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(match_arr_gc.as_ptr()).unwrap())
                    };
                    result_arr.push(match_arr_val);
                }
                let arr_gc = self.gc.lock().allocate(result_arr);
                let arr_val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                };
                stack.push(arr_val)?;
            }
            id if id == regexp::REPLACE => {
                let input = if !args.is_empty() && args[0].is_ptr() {
                    if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                        unsafe { &*s.as_ptr() }.data.clone()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                let replacement = if args.len() > 1 && args[1].is_ptr() {
                    if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                        unsafe { &*s.as_ptr() }.data.clone()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                let result = re.replace(&input, &replacement);
                let result_str = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(result_str);
                let result_val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };
                stack.push(result_val)?;
            }
            id if id == regexp::SPLIT => {
                let input = if !args.is_empty() && args[0].is_ptr() {
                    if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                        unsafe { &*s.as_ptr() }.data.clone()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                // In Raya, limit 0 means "no limit"
                let limit = if args.len() > 1 {
                    let raw_limit = args[1].as_i32().unwrap_or(0);
                    if raw_limit > 0 { Some(raw_limit as usize) } else { None }
                } else {
                    None
                };
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
                stack.push(arr_val)?;
            }
            id if id == regexp::REPLACE_WITH => {
                // replaceWith is now handled as a compiler intrinsic (inline loop + CallClosure).
                // This path should never be reached.
                return Err(VmError::RuntimeError(
                    "RegExp.replaceWith is handled by compiler intrinsic, should not reach VM handler".to_string()
                ));
            }
            _ => {
                return Err(VmError::RuntimeError(format!(
                    "RegExp method {:#06x} not yet implemented in Interpreter",
                    method_id
                )));
            }
        }
        Ok(())
    }
}
