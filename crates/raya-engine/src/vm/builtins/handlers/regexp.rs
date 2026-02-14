//! RegExp method handlers
//!
//! Native implementation of RegExp methods like test, exec, replace, etc.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::compiler::Module;
use crate::vm::builtin::regexp;
use crate::vm::gc::GarbageCollector as Gc;
use crate::vm::object::{Array, Closure, RayaString, RegExpObject};
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;

/// Context needed for regexp method execution
pub struct RegExpHandlerContext<'a> {
    pub gc: &'a Mutex<Gc>,
    pub task: &'a Arc<Task>,
    pub module: &'a Module,
    /// Function to execute nested callbacks (replaceWith)
    pub execute_nested: &'a dyn Fn(&Arc<Task>, usize, Vec<Value>, &Module) -> Result<Value, VmError>,
}

/// Handle built-in regexp methods
pub fn call_regexp_method(
    ctx: &RegExpHandlerContext,
    stack: &mut std::sync::MutexGuard<'_, Stack>,
    method_id: u16,
    arg_count: usize,
) -> Result<(), VmError> {
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
                    let gc_ptr = ctx.gc.lock().allocate(matched_str);
                    let matched_val = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                    };
                    arr.push(matched_val);
                    arr.push(Value::i32(index as i32));
                    for group in groups {
                        let group_str = RayaString::new(group);
                        let gc_ptr = ctx.gc.lock().allocate(group_str);
                        let group_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        arr.push(group_val);
                    }
                    let arr_gc = ctx.gc.lock().allocate(arr);
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
                let gc_ptr = ctx.gc.lock().allocate(matched_str);
                let matched_val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };
                match_arr.push(matched_val);
                match_arr.push(Value::i32(index as i32));
                for group in groups {
                    let group_str = RayaString::new(group);
                    let gc_ptr = ctx.gc.lock().allocate(group_str);
                    let group_val = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                    };
                    match_arr.push(group_val);
                }
                let match_arr_gc = ctx.gc.lock().allocate(match_arr);
                let match_arr_val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(match_arr_gc.as_ptr()).unwrap())
                };
                result_arr.push(match_arr_val);
            }
            let arr_gc = ctx.gc.lock().allocate(result_arr);
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
            let gc_ptr = ctx.gc.lock().allocate(result_str);
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
                let gc_ptr = ctx.gc.lock().allocate(s);
                let val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };
                arr.push(val);
            }
            let arr_gc = ctx.gc.lock().allocate(arr);
            let arr_val = unsafe {
                Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
            };
            stack.push(arr_val)?;
        }
        id if id == regexp::REPLACE_WITH => {
            // replaceWith(str, callback): replace matches using callback function
            // args[0] = input string, args[1] = callback closure
            let input = if !args.is_empty() && args[0].is_ptr() {
                if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                    unsafe { &*s.as_ptr() }.data.clone()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            let callback_val = if args.len() > 1 { args[1] } else {
                return Err(VmError::TypeError("Expected callback function".to_string()));
            };
            if !callback_val.is_ptr() {
                return Err(VmError::TypeError("Expected callback function".to_string()));
            }
            let closure_ptr = unsafe { callback_val.as_ptr::<Closure>() };
            let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
            let func_index = closure.func_id();

            let is_global = re.flags.contains('g');
            let mut result = String::new();
            let mut last_end = 0;

            ctx.task.push_closure(callback_val);

            if is_global {
                for m in re.compiled.find_iter(&input) {
                    result.push_str(&input[last_end..m.start()]);

                    let mut match_arr = Array::new(0, 0);
                    let match_str = RayaString::new(m.as_str().to_string());
                    let gc_ptr = ctx.gc.lock().allocate(match_str);
                    let match_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    match_arr.push(match_val);
                    let arr_gc_ptr = ctx.gc.lock().allocate(match_arr);
                    let arr_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc_ptr.as_ptr()).unwrap()) };

                    let callback_result = (ctx.execute_nested)(ctx.task, func_index, vec![arr_val], ctx.module)?;
                    let replacement = if let Some(ptr) = unsafe { callback_result.as_ptr::<RayaString>() } {
                        unsafe { &*ptr.as_ptr() }.data.clone()
                    } else {
                        String::new()
                    };
                    result.push_str(&replacement);
                    last_end = m.end();
                }
            } else {
                if let Some(m) = re.compiled.find(&input) {
                    result.push_str(&input[..m.start()]);

                    let mut match_arr = Array::new(0, 0);
                    let match_str = RayaString::new(m.as_str().to_string());
                    let gc_ptr = ctx.gc.lock().allocate(match_str);
                    let match_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    match_arr.push(match_val);
                    let arr_gc_ptr = ctx.gc.lock().allocate(match_arr);
                    let arr_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc_ptr.as_ptr()).unwrap()) };

                    let callback_result = (ctx.execute_nested)(ctx.task, func_index, vec![arr_val], ctx.module)?;
                    let replacement = if let Some(ptr) = unsafe { callback_result.as_ptr::<RayaString>() } {
                        unsafe { &*ptr.as_ptr() }.data.clone()
                    } else {
                        String::new()
                    };
                    result.push_str(&replacement);
                    last_end = m.end();
                }
            }

            result.push_str(&input[last_end..]);
            ctx.task.pop_closure();

            let result_str = RayaString::new(result);
            let gc_ptr = ctx.gc.lock().allocate(result_str);
            let result_val = unsafe {
                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
            };
            stack.push(result_val)?;
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
