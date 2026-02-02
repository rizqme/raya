//! String method handlers
//!
//! Native implementation of string methods like charAt, substring, split, etc.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::vm::builtin::string;
use crate::vm::gc::GarbageCollector as Gc;
use crate::vm::object::{Array, Closure, RayaString, RegExpObject};
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use crate::compiler::Module;

/// Context needed for string method execution
pub struct StringHandlerContext<'a> {
    pub gc: &'a Mutex<Gc>,
    pub task: &'a Arc<Task>,
    pub module: &'a Module,
    /// Function to execute nested callbacks (replaceWith, etc.)
    pub execute_nested: &'a dyn Fn(&Arc<Task>, usize, Vec<Value>, &Module) -> Result<Value, VmError>,
}

/// Handle built-in string methods
pub fn call_string_method(
    ctx: &StringHandlerContext,
    stack: &mut std::sync::MutexGuard<'_, Stack>,
    method_id: u16,
    arg_count: usize,
) -> Result<(), VmError> {
    // Pop arguments first (they're on top of the stack)
    let mut args = Vec::with_capacity(arg_count);
    for _ in 0..arg_count {
        args.push(stack.pop()?);
    }
    args.reverse(); // Now args[0] is the first argument

    // Pop the string (receiver)
    let string_val = stack.pop()?;
    if !string_val.is_ptr() {
        return Err(VmError::TypeError("Expected string".to_string()));
    }
    let str_ptr = unsafe { string_val.as_ptr::<RayaString>() };
    let raya_str = unsafe { &*str_ptr.unwrap().as_ptr() };
    let s = &raya_str.data;

    match method_id {
        string::CHAR_AT => {
            if arg_count != 1 {
                return Err(VmError::RuntimeError(format!(
                    "String.charAt expects 1 argument, got {}", arg_count
                )));
            }
            let index = args[0].as_i32().unwrap_or(0) as usize;
            let result = s.chars().nth(index).map(|c| c.to_string()).unwrap_or_default();
            let raya_string = RayaString::new(result);
            let gc_ptr = ctx.gc.lock().allocate(raya_string);
            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
            stack.push(value)?;
            Ok(())
        }
        string::TO_UPPER_CASE => {
            let result = s.to_uppercase();
            let raya_string = RayaString::new(result);
            let gc_ptr = ctx.gc.lock().allocate(raya_string);
            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
            stack.push(value)?;
            Ok(())
        }
        string::TO_LOWER_CASE => {
            let result = s.to_lowercase();
            let raya_string = RayaString::new(result);
            let gc_ptr = ctx.gc.lock().allocate(raya_string);
            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
            stack.push(value)?;
            Ok(())
        }
        string::TRIM => {
            let result = s.trim().to_string();
            let raya_string = RayaString::new(result);
            let gc_ptr = ctx.gc.lock().allocate(raya_string);
            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
            stack.push(value)?;
            Ok(())
        }
        string::INDEX_OF => {
            if arg_count != 1 {
                return Err(VmError::RuntimeError(format!(
                    "String.indexOf expects 1 argument, got {}", arg_count
                )));
            }
            let search_val = args[0];
            let search_str = if let Some(ptr) = unsafe { search_val.as_ptr::<RayaString>() } {
                unsafe { &*ptr.as_ptr() }.data.clone()
            } else {
                String::new()
            };
            let result = s.find(&search_str).map(|i| i as i32).unwrap_or(-1);
            stack.push(Value::i32(result))?;
            Ok(())
        }
        string::INCLUDES => {
            if arg_count != 1 {
                return Err(VmError::RuntimeError(format!(
                    "String.includes expects 1 argument, got {}", arg_count
                )));
            }
            let search_val = args[0];
            let search_str = if let Some(ptr) = unsafe { search_val.as_ptr::<RayaString>() } {
                unsafe { &*ptr.as_ptr() }.data.clone()
            } else {
                String::new()
            };
            let result = s.contains(&search_str);
            stack.push(Value::bool(result))?;
            Ok(())
        }
        string::STARTS_WITH => {
            if arg_count != 1 {
                return Err(VmError::RuntimeError(format!(
                    "String.startsWith expects 1 argument, got {}", arg_count
                )));
            }
            let prefix_val = args[0];
            let prefix_str = if let Some(ptr) = unsafe { prefix_val.as_ptr::<RayaString>() } {
                unsafe { &*ptr.as_ptr() }.data.clone()
            } else {
                String::new()
            };
            let result = s.starts_with(&prefix_str);
            stack.push(Value::bool(result))?;
            Ok(())
        }
        string::ENDS_WITH => {
            if arg_count != 1 {
                return Err(VmError::RuntimeError(format!(
                    "String.endsWith expects 1 argument, got {}", arg_count
                )));
            }
            let suffix_val = args[0];
            let suffix_str = if let Some(ptr) = unsafe { suffix_val.as_ptr::<RayaString>() } {
                unsafe { &*ptr.as_ptr() }.data.clone()
            } else {
                String::new()
            };
            let result = s.ends_with(&suffix_str);
            stack.push(Value::bool(result))?;
            Ok(())
        }
        string::SUBSTRING => {
            // substring(start, end?)
            let start_val = if arg_count >= 1 { args[0] } else { Value::i32(0) };
            let end_val = if arg_count >= 2 { Some(args[1]) } else { None };

            let start = start_val.as_i32().unwrap_or(0).max(0) as usize;
            let end = end_val.and_then(|v| v.as_i32()).map(|e| e.max(0) as usize).unwrap_or(s.len());

            let result: String = s.chars().skip(start).take(end.saturating_sub(start)).collect();
            let raya_string = RayaString::new(result);
            let gc_ptr = ctx.gc.lock().allocate(raya_string);
            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
            stack.push(value)?;
            Ok(())
        }
        string::SPLIT => {
            if arg_count < 1 || arg_count > 2 {
                return Err(VmError::RuntimeError(format!(
                    "String.split expects 1-2 arguments, got {}", arg_count
                )));
            }
            let sep_val = args[0];
            let sep_str = if let Some(ptr) = unsafe { sep_val.as_ptr::<RayaString>() } {
                unsafe { &*ptr.as_ptr() }.data.clone()
            } else {
                String::new()
            };

            // Get optional limit argument (try both i32 and i64)
            // In Raya, limit 0 means "no limit"
            let limit = if arg_count == 2 {
                let raw_limit = args[1].as_i32()
                    .or_else(|| args[1].as_i64().map(|v| v as i32))
                    .unwrap_or(0);
                if raw_limit > 0 { Some(raw_limit as usize) } else { None }
            } else {
                None
            };

            // Split and optionally limit the parts
            let parts: Vec<_> = if sep_str.is_empty() {
                let chars: Vec<_> = s.chars().map(|c| c.to_string()).collect();
                if let Some(limit) = limit {
                    chars.into_iter().take(limit).collect()
                } else {
                    chars
                }
            } else {
                let all_parts: Vec<_> = s.split(&sep_str).map(|p| p.to_string()).collect();
                if let Some(limit) = limit {
                    all_parts.into_iter().take(limit).collect()
                } else {
                    all_parts
                }
            };

            let mut arr = Array::new(0, 0);
            for part in parts {
                let raya_string = RayaString::new(part);
                let gc_ptr = ctx.gc.lock().allocate(raya_string);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                arr.push(value);
            }

            let gc_ptr = ctx.gc.lock().allocate(arr);
            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
            stack.push(value)?;
            Ok(())
        }
        string::CHAR_CODE_AT => {
            if arg_count != 1 {
                return Err(VmError::RuntimeError(format!(
                    "String.charCodeAt expects 1 argument, got {}", arg_count
                )));
            }
            let index = args[0].as_i32().unwrap_or(0) as usize;
            let result = s.chars().nth(index).map(|c| c as i32).unwrap_or(-1);
            stack.push(Value::i32(result))?;
            Ok(())
        }
        string::LAST_INDEX_OF => {
            if arg_count != 1 {
                return Err(VmError::RuntimeError(format!(
                    "String.lastIndexOf expects 1 argument, got {}", arg_count
                )));
            }
            let search_val = args[0];
            let search_str = if let Some(ptr) = unsafe { search_val.as_ptr::<RayaString>() } {
                unsafe { &*ptr.as_ptr() }.data.clone()
            } else {
                String::new()
            };
            let result = s.rfind(&search_str).map(|i| i as i32).unwrap_or(-1);
            stack.push(Value::i32(result))?;
            Ok(())
        }
        string::PAD_START => {
            // padStart(targetLength, padString?)
            if arg_count < 1 {
                return Err(VmError::RuntimeError(format!(
                    "String.padStart expects at least 1 argument, got {}", arg_count
                )));
            }
            let target_length = args[0].as_i32().unwrap_or(0) as usize;
            let pad_str = if arg_count >= 2 {
                if let Some(ptr) = unsafe { args[1].as_ptr::<RayaString>() } {
                    unsafe { &*ptr.as_ptr() }.data.clone()
                } else {
                    " ".to_string()
                }
            } else {
                " ".to_string()
            };

            let result = if s.len() >= target_length {
                s.clone()
            } else {
                let pad_len = target_length - s.len();
                let pad_repeated = pad_str.repeat((pad_len / pad_str.len().max(1)) + 1);
                format!("{}{}", &pad_repeated[..pad_len], s)
            };

            let raya_string = RayaString::new(result);
            let gc_ptr = ctx.gc.lock().allocate(raya_string);
            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
            stack.push(value)?;
            Ok(())
        }
        string::PAD_END => {
            // padEnd(targetLength, padString?)
            if arg_count < 1 {
                return Err(VmError::RuntimeError(format!(
                    "String.padEnd expects at least 1 argument, got {}", arg_count
                )));
            }
            let target_length = args[0].as_i32().unwrap_or(0) as usize;
            let pad_str = if arg_count >= 2 {
                if let Some(ptr) = unsafe { args[1].as_ptr::<RayaString>() } {
                    unsafe { &*ptr.as_ptr() }.data.clone()
                } else {
                    " ".to_string()
                }
            } else {
                " ".to_string()
            };

            let result = if s.len() >= target_length {
                s.clone()
            } else {
                let pad_len = target_length - s.len();
                let pad_repeated = pad_str.repeat((pad_len / pad_str.len().max(1)) + 1);
                format!("{}{}", s, &pad_repeated[..pad_len])
            };

            let raya_string = RayaString::new(result);
            let gc_ptr = ctx.gc.lock().allocate(raya_string);
            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
            stack.push(value)?;
            Ok(())
        }
        string::TRIM_START => {
            let result = s.trim_start().to_string();
            let raya_string = RayaString::new(result);
            let gc_ptr = ctx.gc.lock().allocate(raya_string);
            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
            stack.push(value)?;
            Ok(())
        }
        string::TRIM_END => {
            let result = s.trim_end().to_string();
            let raya_string = RayaString::new(result);
            let gc_ptr = ctx.gc.lock().allocate(raya_string);
            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
            stack.push(value)?;
            Ok(())
        }
        string::MATCH => {
            // match(regexp): returns array of matches or null
            if arg_count != 1 {
                return Err(VmError::RuntimeError(format!(
                    "String.match expects 1 argument, got {}", arg_count
                )));
            }
            let regexp_val = args[0];
            let handle = regexp_val.as_u64().ok_or_else(|| {
                VmError::TypeError("Expected RegExp argument".to_string())
            })?;
            let re_ptr = handle as *const RegExpObject;
            if re_ptr.is_null() {
                return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
            }
            let re = unsafe { &*re_ptr };

            // Check if global flag is set
            let is_global = re.flags.contains('g');

            if is_global {
                // Return all matches
                let matches: Vec<_> = re.compiled.find_iter(s).map(|m| m.as_str().to_string()).collect();
                if matches.is_empty() {
                    stack.push(Value::null())?;
                } else {
                    let mut arr = Array::new(0, 0);
                    for m in matches {
                        let raya_string = RayaString::new(m);
                        let gc_ptr = ctx.gc.lock().allocate(raya_string);
                        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        arr.push(value);
                    }
                    let gc_ptr = ctx.gc.lock().allocate(arr);
                    let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    stack.push(value)?;
                }
            } else {
                // Return first match only
                if let Some(m) = re.compiled.find(s) {
                    let mut arr = Array::new(0, 0);
                    let raya_string = RayaString::new(m.as_str().to_string());
                    let gc_ptr = ctx.gc.lock().allocate(raya_string);
                    let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    arr.push(value);
                    let gc_ptr = ctx.gc.lock().allocate(arr);
                    let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    stack.push(value)?;
                } else {
                    stack.push(Value::null())?;
                }
            }
            Ok(())
        }
        string::MATCH_ALL => {
            // matchAll(regexp): returns array of [match, index] arrays
            if arg_count != 1 {
                return Err(VmError::RuntimeError(format!(
                    "String.matchAll expects 1 argument, got {}", arg_count
                )));
            }
            let regexp_val = args[0];
            let handle = regexp_val.as_u64().ok_or_else(|| {
                VmError::TypeError("Expected RegExp argument".to_string())
            })?;
            let re_ptr = handle as *const RegExpObject;
            if re_ptr.is_null() {
                return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
            }
            let re = unsafe { &*re_ptr };

            // Return all matches as array of [match, index] arrays
            let mut result_arr = Array::new(0, 0);
            for m in re.compiled.find_iter(s) {
                // Create inner array [match_string, index]
                let mut match_arr = Array::new(0, 0);

                // Add match string
                let raya_string = RayaString::new(m.as_str().to_string());
                let gc_ptr = ctx.gc.lock().allocate(raya_string);
                let match_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                match_arr.push(match_val);

                // Add index
                match_arr.push(Value::i32(m.start() as i32));

                let inner_gc_ptr = ctx.gc.lock().allocate(match_arr);
                let inner_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(inner_gc_ptr.as_ptr()).unwrap()) };
                result_arr.push(inner_val);
            }
            let gc_ptr = ctx.gc.lock().allocate(result_arr);
            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
            stack.push(value)?;
            Ok(())
        }
        string::SEARCH => {
            // search(regexp): returns index of first match or -1
            if arg_count != 1 {
                return Err(VmError::RuntimeError(format!(
                    "String.search expects 1 argument, got {}", arg_count
                )));
            }
            let regexp_val = args[0];
            let handle = regexp_val.as_u64().ok_or_else(|| {
                VmError::TypeError("Expected RegExp argument".to_string())
            })?;
            let re_ptr = handle as *const RegExpObject;
            if re_ptr.is_null() {
                return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
            }
            let re = unsafe { &*re_ptr };

            let result = re.compiled.find(s).map(|m| m.start() as i32).unwrap_or(-1);
            stack.push(Value::i32(result))?;
            Ok(())
        }
        string::REPLACE_REGEXP => {
            // replace(regexp, replacement): replace matches with string
            if arg_count != 2 {
                return Err(VmError::RuntimeError(format!(
                    "String.replace expects 2 arguments, got {}", arg_count
                )));
            }
            let regexp_val = args[0];
            let handle = regexp_val.as_u64().ok_or_else(|| {
                VmError::TypeError("Expected RegExp argument".to_string())
            })?;
            let re_ptr = handle as *const RegExpObject;
            if re_ptr.is_null() {
                return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
            }
            let re = unsafe { &*re_ptr };

            let replacement = if let Some(ptr) = unsafe { args[1].as_ptr::<RayaString>() } {
                unsafe { &*ptr.as_ptr() }.data.clone()
            } else {
                String::new()
            };

            let is_global = re.flags.contains('g');
            let result = if is_global {
                re.compiled.replace_all(s, replacement.as_str()).to_string()
            } else {
                re.compiled.replace(s, replacement.as_str()).to_string()
            };

            let raya_string = RayaString::new(result);
            let gc_ptr = ctx.gc.lock().allocate(raya_string);
            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
            stack.push(value)?;
            Ok(())
        }
        string::SPLIT_REGEXP => {
            // split(regexp, limit?): split string by regexp
            if arg_count < 1 || arg_count > 2 {
                return Err(VmError::RuntimeError(format!(
                    "String.split expects 1-2 arguments, got {}", arg_count
                )));
            }
            let regexp_val = args[0];
            let handle = regexp_val.as_u64().ok_or_else(|| {
                VmError::TypeError("Expected RegExp argument".to_string())
            })?;
            let re_ptr = handle as *const RegExpObject;
            if re_ptr.is_null() {
                return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
            }
            let re = unsafe { &*re_ptr };

            // Get optional limit argument (try both i32 and i64)
            // In Raya, limit 0 means "no limit"
            let limit = if arg_count == 2 {
                let raw_limit = args[1].as_i32()
                    .or_else(|| args[1].as_i64().map(|v| v as i32))
                    .unwrap_or(0);
                if raw_limit > 0 { Some(raw_limit as usize) } else { None }
            } else {
                None
            };

            // Split and optionally limit the parts
            let all_parts: Vec<_> = re.compiled.split(s).map(|p| p.to_string()).collect();
            let parts: Vec<_> = if let Some(limit) = limit {
                all_parts.into_iter().take(limit).collect()
            } else {
                all_parts
            };

            let mut arr = Array::new(0, 0);
            for part in parts {
                let raya_string = RayaString::new(part);
                let gc_ptr = ctx.gc.lock().allocate(raya_string);
                let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                arr.push(value);
            }

            let gc_ptr = ctx.gc.lock().allocate(arr);
            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
            stack.push(value)?;
            Ok(())
        }
        string::REPLACE_WITH_REGEXP => {
            // replaceWith(regexp, callback): replace using callback function
            if arg_count != 2 {
                return Err(VmError::RuntimeError(format!(
                    "String.replaceWith expects 2 arguments, got {}", arg_count
                )));
            }
            let regexp_val = args[0];
            let handle = regexp_val.as_u64().ok_or_else(|| {
                VmError::TypeError("Expected RegExp argument".to_string())
            })?;
            let re_ptr = handle as *const RegExpObject;
            if re_ptr.is_null() {
                return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
            }
            let re = unsafe { &*re_ptr };

            // Get callback function
            let callback_val = args[1];
            if !callback_val.is_ptr() {
                return Err(VmError::TypeError("Expected callback function".to_string()));
            }
            let closure_ptr = unsafe { callback_val.as_ptr::<Closure>() };
            let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
            let func_index = closure.func_id();

            let is_global = re.flags.contains('g');

            // Build result string by replacing matches
            let mut result = String::new();
            let mut last_end = 0;

            ctx.task.push_closure(callback_val);

            if is_global {
                // Replace all matches
                for m in re.compiled.find_iter(s) {
                    // Add text before this match
                    result.push_str(&s[last_end..m.start()]);

                    // Create match array argument
                    let mut match_arr = Array::new(0, 0);
                    let match_str = RayaString::new(m.as_str().to_string());
                    let gc_ptr = ctx.gc.lock().allocate(match_str);
                    let match_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    match_arr.push(match_val);
                    let arr_gc_ptr = ctx.gc.lock().allocate(match_arr);
                    let arr_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc_ptr.as_ptr()).unwrap()) };

                    // Call callback with match array
                    let callback_result = (ctx.execute_nested)(ctx.task, func_index, vec![arr_val], ctx.module)?;

                    // Get replacement string from callback result
                    let replacement = if let Some(ptr) = unsafe { callback_result.as_ptr::<RayaString>() } {
                        unsafe { &*ptr.as_ptr() }.data.clone()
                    } else {
                        String::new()
                    };
                    result.push_str(&replacement);
                    last_end = m.end();
                }
            } else {
                // Replace first match only
                if let Some(m) = re.compiled.find(s) {
                    result.push_str(&s[..m.start()]);

                    // Create match array argument
                    let mut match_arr = Array::new(0, 0);
                    let match_str = RayaString::new(m.as_str().to_string());
                    let gc_ptr = ctx.gc.lock().allocate(match_str);
                    let match_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    match_arr.push(match_val);
                    let arr_gc_ptr = ctx.gc.lock().allocate(match_arr);
                    let arr_val = unsafe { Value::from_ptr(std::ptr::NonNull::new(arr_gc_ptr.as_ptr()).unwrap()) };

                    // Call callback
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

            ctx.task.pop_closure();

            // Add remaining text after last match
            result.push_str(&s[last_end..]);

            let raya_string = RayaString::new(result);
            let gc_ptr = ctx.gc.lock().allocate(raya_string);
            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
            stack.push(value)?;
            Ok(())
        }
        _ => Err(VmError::RuntimeError(format!(
            "String method {:#06x} not yet implemented in TaskInterpreter",
            method_id
        ))),
    }
}
