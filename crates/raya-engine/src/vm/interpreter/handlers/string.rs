//! String built-in method handlers

use crate::compiler::Module;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{Array, RayaString, RegExpObject};
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

#[inline]
fn to_i32_arg(value: Value, default: i32) -> i32 {
    value
        .as_i32()
        .or_else(|| value.as_f64().map(|f| f as i32))
        .or_else(|| value.as_i64().map(|v| v as i32))
        .unwrap_or(default)
}

impl<'a> Interpreter<'a> {
    /// Handle built-in string methods
    pub(in crate::vm::interpreter) fn call_string_method(
        &mut self,
        task: &Arc<Task>,
        stack: &mut Stack,
        method_id: u16,
        arg_count: usize,
        module: &Module,
    ) -> Result<(), VmError> {
        use crate::vm::builtin::string;

        // Pop arguments first (they're on top of the stack)
        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(stack.pop()?);
        }
        args.reverse(); // Now args[0] is the first argument

        // Pop the string (receiver)
        let string_val = stack.pop()?;
        let s = self.js_function_argument_to_string(string_val, task, module)?;
        let s = s.as_str();

        match method_id {
            string::CHAR_AT => {
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "String.charAt expects 1 argument, got {}",
                        arg_count
                    )));
                }
                let index = to_i32_arg(args[0], 0) as usize;
                let result = s
                    .chars()
                    .nth(index)
                    .map(|c| c.to_string())
                    .unwrap_or_default();
                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::TO_UPPER_CASE => {
                let result = s.to_uppercase();
                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::TO_LOWER_CASE => {
                let result = s.to_lowercase();
                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::TRIM => {
                let result = s.trim().to_string();
                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::INDEX_OF => {
                if !(1..=2).contains(&arg_count) {
                    return Err(VmError::RuntimeError(format!(
                        "String.indexOf expects 1-2 arguments, got {}",
                        arg_count
                    )));
                }
                let search_str = self.js_function_argument_to_string(args[0], task, module)?;
                let from_index = if arg_count == 2 {
                    to_i32_arg(args[1], 0).max(0) as usize
                } else {
                    0
                };
                let result = if from_index >= s.len() {
                    -1
                } else {
                    s[from_index..]
                        .find(&search_str)
                        .map(|i| (i + from_index) as i32)
                        .unwrap_or(-1)
                };
                stack.push(Value::i32(result))?;
                Ok(())
            }
            string::INCLUDES => {
                if !(1..=2).contains(&arg_count) {
                    return Err(VmError::RuntimeError(format!(
                        "String.includes expects 1 or 2 arguments, got {}",
                        arg_count
                    )));
                }
                let search_str = self.js_function_argument_to_string(args[0], task, module)?;
                let start = if arg_count >= 2 {
                    to_i32_arg(args[1], 0).max(0) as usize
                } else {
                    0
                };
                let result = s
                    .get(start.min(s.len())..)
                    .is_some_and(|slice| slice.contains(&search_str));
                stack.push(Value::bool(result))?;
                Ok(())
            }
            string::STARTS_WITH => {
                if !(1..=2).contains(&arg_count) {
                    return Err(VmError::RuntimeError(format!(
                        "String.startsWith expects 1 or 2 arguments, got {}",
                        arg_count
                    )));
                }
                let prefix_str = self.js_function_argument_to_string(args[0], task, module)?;
                let start = if arg_count >= 2 {
                    to_i32_arg(args[1], 0).max(0) as usize
                } else {
                    0
                };
                let result = s
                    .get(start.min(s.len())..)
                    .is_some_and(|slice| slice.starts_with(&prefix_str));
                stack.push(Value::bool(result))?;
                Ok(())
            }
            string::ENDS_WITH => {
                if !(1..=2).contains(&arg_count) {
                    return Err(VmError::RuntimeError(format!(
                        "String.endsWith expects 1 or 2 arguments, got {}",
                        arg_count
                    )));
                }
                let suffix_str = self.js_function_argument_to_string(args[0], task, module)?;
                let end = if arg_count >= 2 {
                    to_i32_arg(args[1], s.len() as i32).max(0) as usize
                } else {
                    s.len()
                };
                let result = s
                    .get(..end.min(s.len()))
                    .is_some_and(|slice| slice.ends_with(&suffix_str));
                stack.push(Value::bool(result))?;
                Ok(())
            }
            string::SUBSTRING => {
                // substring(start, end?)
                let start_val = if arg_count >= 1 {
                    args[0]
                } else {
                    Value::i32(0)
                };
                let end_val = if arg_count >= 2 { Some(args[1]) } else { None };

                let start = to_i32_arg(start_val, 0).max(0) as usize;
                let end = end_val
                    .map(|v| to_i32_arg(v, 0))
                    .map(|e| e.max(0) as usize)
                    .unwrap_or(s.len());

                let result: String = s
                    .chars()
                    .skip(start)
                    .take(end.saturating_sub(start))
                    .collect();
                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::SPLIT => {
                if !(1..=2).contains(&arg_count) {
                    return Err(VmError::RuntimeError(format!(
                        "String.split expects 1-2 arguments, got {}",
                        arg_count
                    )));
                }
                let sep_str = self.js_function_argument_to_string(args[0], task, module)?;

                // Get optional limit argument (try both i32 and i64)
                // In Raya, limit 0 means "no limit"
                let limit = if arg_count == 2 {
                    let raw_limit = to_i32_arg(args[1], 0);
                    if raw_limit > 0 {
                        Some(raw_limit as usize)
                    } else {
                        None
                    }
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
                    let gc_ptr = self.gc.lock().allocate(raya_string);
                    let value = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                    };
                    arr.push(value);
                }

                let gc_ptr = self.gc.lock().allocate(arr);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::CHAR_CODE_AT => {
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "String.charCodeAt expects 1 argument, got {}",
                        arg_count
                    )));
                }
                let index = to_i32_arg(args[0], 0) as usize;
                let result = s.chars().nth(index).map(|c| c as i32).unwrap_or(-1);
                stack.push(Value::i32(result))?;
                Ok(())
            }
            string::LAST_INDEX_OF => {
                if !(1..=2).contains(&arg_count) {
                    return Err(VmError::RuntimeError(format!(
                        "String.lastIndexOf expects 1-2 arguments, got {}",
                        arg_count
                    )));
                }
                let search_str = self.js_function_argument_to_string(args[0], task, module)?;
                let result = if arg_count == 2 {
                    let end_index = to_i32_arg(args[1], s.len() as i32).max(0) as usize;
                    let end = (end_index + search_str.len()).min(s.len());
                    s[..end].rfind(&search_str).map(|i| i as i32).unwrap_or(-1)
                } else {
                    s.rfind(&search_str).map(|i| i as i32).unwrap_or(-1)
                };
                stack.push(Value::i32(result))?;
                Ok(())
            }
            string::PAD_START => {
                // padStart(targetLength, padString?)
                if arg_count < 1 {
                    return Err(VmError::RuntimeError(format!(
                        "String.padStart expects at least 1 argument, got {}",
                        arg_count
                    )));
                }
                let target_length = to_i32_arg(args[0], 0) as usize;
                let pad_str = if arg_count >= 2 {
                    self.js_function_argument_to_string(args[1], task, module)?
                } else {
                    " ".to_string()
                };

                let result = if s.len() >= target_length {
                    s.to_string()
                } else {
                    let pad_len = target_length - s.len();
                    let pad_repeated = pad_str.repeat((pad_len / pad_str.len().max(1)) + 1);
                    format!("{}{}", &pad_repeated[..pad_len], s)
                };

                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::PAD_END => {
                // padEnd(targetLength, padString?)
                if arg_count < 1 {
                    return Err(VmError::RuntimeError(format!(
                        "String.padEnd expects at least 1 argument, got {}",
                        arg_count
                    )));
                }
                let target_length = to_i32_arg(args[0], 0) as usize;
                let pad_str = if arg_count >= 2 {
                    self.js_function_argument_to_string(args[1], task, module)?
                } else {
                    " ".to_string()
                };

                let result = if s.len() >= target_length {
                    s.to_string()
                } else {
                    let pad_len = target_length - s.len();
                    let pad_repeated = pad_str.repeat((pad_len / pad_str.len().max(1)) + 1);
                    format!("{}{}", s, &pad_repeated[..pad_len])
                };

                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::TRIM_START => {
                let result = s.trim_start().to_string();
                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::TRIM_END => {
                let result = s.trim_end().to_string();
                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::MATCH => {
                // match(regexp): returns array of matches or null
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "String.match expects 1 argument, got {}",
                        arg_count
                    )));
                }
                let regexp_val = args[0];
                let handle = self.regexp_handle_from_value(regexp_val)?;
                let re_ptr = handle as *const RegExpObject;
                if re_ptr.is_null() {
                    return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
                }
                let re = unsafe { &*re_ptr };

                // Check if global flag is set
                let is_global = re.flags.contains('g');

                if is_global {
                    // Return all matches
                    let matches: Vec<_> = re
                        .compiled
                        .find_iter(s)
                        .map(|m| m.as_str().to_string())
                        .collect();
                    if matches.is_empty() {
                        stack.push(Value::null())?;
                    } else {
                        let mut arr = Array::new(0, 0);
                        for m in matches {
                            let raya_string = RayaString::new(m);
                            let gc_ptr = self.gc.lock().allocate(raya_string);
                            let value = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            };
                            arr.push(value);
                        }
                        let gc_ptr = self.gc.lock().allocate(arr);
                        let value = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        stack.push(value)?;
                    }
                } else {
                    // Return first match only
                    if let Some(m) = re.compiled.find(s) {
                        let mut arr = Array::new(0, 0);
                        let raya_string = RayaString::new(m.as_str().to_string());
                        let gc_ptr = self.gc.lock().allocate(raya_string);
                        let value = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        arr.push(value);
                        let gc_ptr = self.gc.lock().allocate(arr);
                        let value = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
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
                        "String.matchAll expects 1 argument, got {}",
                        arg_count
                    )));
                }
                let regexp_val = args[0];
                let handle = self.regexp_handle_from_value(regexp_val)?;
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
                    let gc_ptr = self.gc.lock().allocate(raya_string);
                    let match_val = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                    };
                    match_arr.push(match_val);

                    // Add index
                    match_arr.push(Value::i32(m.start() as i32));

                    let inner_gc_ptr = self.gc.lock().allocate(match_arr);
                    let inner_val = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(inner_gc_ptr.as_ptr()).unwrap())
                    };
                    result_arr.push(inner_val);
                }
                let gc_ptr = self.gc.lock().allocate(result_arr);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::SEARCH => {
                // search(regexp): returns index of first match or -1
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "String.search expects 1 argument, got {}",
                        arg_count
                    )));
                }
                let regexp_val = args[0];
                let handle = self.regexp_handle_from_value(regexp_val)?;
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
                        "String.replace expects 2 arguments, got {}",
                        arg_count
                    )));
                }
                let regexp_val = args[0];
                let handle = self.regexp_handle_from_value(regexp_val)?;
                let re_ptr = handle as *const RegExpObject;
                if re_ptr.is_null() {
                    return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
                }
                let re = unsafe { &*re_ptr };

                let replacement = self.js_function_argument_to_string(args[1], task, module)?;

                let is_global = re.flags.contains('g');
                let result = if is_global {
                    re.compiled.replace_all(s, replacement.as_str()).to_string()
                } else {
                    re.compiled.replace(s, replacement.as_str()).to_string()
                };

                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::SPLIT_REGEXP => {
                // split(regexp, limit?): split string by regexp
                if !(1..=2).contains(&arg_count) {
                    return Err(VmError::RuntimeError(format!(
                        "String.split expects 1-2 arguments, got {}",
                        arg_count
                    )));
                }
                let regexp_val = args[0];
                let handle = self.regexp_handle_from_value(regexp_val)?;
                let re_ptr = handle as *const RegExpObject;
                if re_ptr.is_null() {
                    return Err(VmError::RuntimeError("Invalid regexp handle".to_string()));
                }
                let re = unsafe { &*re_ptr };

                // Get optional limit argument (try both i32 and i64)
                // In Raya, limit 0 means "no limit"
                let limit = if arg_count == 2 {
                    let raw_limit = to_i32_arg(args[1], 0);
                    if raw_limit > 0 {
                        Some(raw_limit as usize)
                    } else {
                        None
                    }
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
                    let gc_ptr = self.gc.lock().allocate(raya_string);
                    let value = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                    };
                    arr.push(value);
                }

                let gc_ptr = self.gc.lock().allocate(arr);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::REPLACE_WITH_REGEXP => {
                // replaceWith is now handled as a compiler intrinsic (inline loop + CallClosure).
                // This path should never be reached.
                Err(VmError::RuntimeError(
                    "String.replaceWith is handled by compiler intrinsic, should not reach VM handler".to_string()
                ))
            }
            string::SLICE => {
                // slice(start, end?)
                // Supports negative indices: -1 = last char, -2 = second-to-last, etc.
                let start_val = if arg_count >= 1 {
                    args[0]
                } else {
                    Value::i32(0)
                };
                let end_val = if arg_count >= 2 { Some(args[1]) } else { None };

                let len = s.len();

                // Normalize negative indices
                let start_raw = to_i32_arg(start_val, 0);
                let start = if start_raw < 0 {
                    ((len as i32 + start_raw).max(0) as usize).min(len)
                } else {
                    (start_raw as usize).min(len)
                };

                let end = end_val
                    .map(|v| to_i32_arg(v, 0))
                    .map(|e| {
                        if e < 0 {
                            ((len as i32 + e).max(0) as usize).min(len)
                        } else {
                            (e as usize).min(len)
                        }
                    })
                    .unwrap_or(len);

                let result: String = s
                    .chars()
                    .skip(start)
                    .take(end.saturating_sub(start))
                    .collect();
                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::REPLACE => {
                // replace(search: string, replacement: string): string
                if arg_count != 2 {
                    return Err(VmError::RuntimeError(format!(
                        "String.replace expects 2 arguments, got {}",
                        arg_count
                    )));
                }
                let search_str = self.js_function_argument_to_string(args[0], task, module)?;
                let replacement_str = self.js_function_argument_to_string(args[1], task, module)?;
                let result = s.replacen(&search_str, &replacement_str, 1);
                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            string::REPEAT => {
                // repeat(count: number = 1): string
                if arg_count > 1 {
                    return Err(VmError::RuntimeError(format!(
                        "String.repeat expects at most 1 argument, got {}",
                        arg_count
                    )));
                }
                let count = if arg_count == 0 {
                    1
                } else {
                    to_i32_arg(args[0], 0).max(0) as usize
                };
                let result = s.repeat(count);
                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            _ => Err(VmError::RuntimeError(format!(
                "String method {:#06x} not yet implemented in Interpreter",
                method_id
            ))),
        }
    }
}
