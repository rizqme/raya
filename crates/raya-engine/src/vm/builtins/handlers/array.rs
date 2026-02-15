//! Array method handlers
//!
//! Native implementation of non-callback array methods (push, pop, slice, etc.).
//! Callback methods (map, filter, reduce, forEach, find, findIndex, some, every, sort)
//! are compiled as inline loops by the compiler and never reach this handler.

use parking_lot::Mutex;

use crate::vm::builtin::array;
use crate::vm::gc::GarbageCollector as Gc;
use crate::vm::object::{Array, RayaString};
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;

/// Context needed for array method execution
pub struct ArrayHandlerContext<'a> {
    pub gc: &'a Mutex<Gc>,
}

/// Handle built-in array methods
pub fn call_array_method(
    ctx: &ArrayHandlerContext,
    stack: &mut std::sync::MutexGuard<'_, Stack>,
    method_id: u16,
    arg_count: usize,
) -> Result<(), VmError> {
    match method_id {
        array::PUSH => {
            if arg_count != 1 {
                return Err(VmError::RuntimeError(format!(
                    "Array.push expects 1 argument, got {}", arg_count
                )));
            }
            let value = stack.pop()?;
            let array_val = stack.pop()?;
            if !array_val.is_ptr() {
                return Err(VmError::TypeError("Expected array".to_string()));
            }
            let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
            let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
            let new_len = arr.push(value);
            stack.push(Value::i32(new_len as i32))?;
            Ok(())
        }
        array::POP => {
            if arg_count != 0 {
                return Err(VmError::RuntimeError(format!(
                    "Array.pop expects 0 arguments, got {}", arg_count
                )));
            }
            let array_val = stack.pop()?;
            if !array_val.is_ptr() {
                return Err(VmError::TypeError("Expected array".to_string()));
            }
            let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
            let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
            let result = arr.pop().unwrap_or(Value::null());
            stack.push(result)?;
            Ok(())
        }
        array::SHIFT => {
            if arg_count != 0 {
                return Err(VmError::RuntimeError(format!(
                    "Array.shift expects 0 arguments, got {}", arg_count
                )));
            }
            let array_val = stack.pop()?;
            if !array_val.is_ptr() {
                return Err(VmError::TypeError("Expected array".to_string()));
            }
            let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
            let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
            let result = arr.shift().unwrap_or(Value::null());
            stack.push(result)?;
            Ok(())
        }
        array::UNSHIFT => {
            if arg_count != 1 {
                return Err(VmError::RuntimeError(format!(
                    "Array.unshift expects 1 argument, got {}", arg_count
                )));
            }
            let value = stack.pop()?;
            let array_val = stack.pop()?;
            if !array_val.is_ptr() {
                return Err(VmError::TypeError("Expected array".to_string()));
            }
            let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
            let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
            let new_len = arr.unshift(value);
            stack.push(Value::i32(new_len as i32))?;
            Ok(())
        }
        array::INDEX_OF => {
            if arg_count != 1 {
                return Err(VmError::RuntimeError(format!(
                    "Array.indexOf expects 1 argument, got {}", arg_count
                )));
            }
            let value = stack.pop()?;
            let array_val = stack.pop()?;
            if !array_val.is_ptr() {
                return Err(VmError::TypeError("Expected array".to_string()));
            }
            let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
            let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
            let result = arr.index_of(value);
            stack.push(Value::i32(result))?;
            Ok(())
        }
        array::INCLUDES => {
            if arg_count != 1 {
                return Err(VmError::RuntimeError(format!(
                    "Array.includes expects 1 argument, got {}", arg_count
                )));
            }
            let value = stack.pop()?;
            let array_val = stack.pop()?;
            if !array_val.is_ptr() {
                return Err(VmError::TypeError("Expected array".to_string()));
            }
            let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
            let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
            let result = arr.includes(value);
            stack.push(Value::bool(result))?;
            Ok(())
        }
        array::SLICE => {
            // slice(start, end?) - arg_count is 1 or 2
            let end_val = if arg_count >= 2 { Some(stack.pop()?) } else { None };
            let start_val = if arg_count >= 1 { stack.pop()? } else { Value::i32(0) };
            let array_val = stack.pop()?;

            if !array_val.is_ptr() {
                return Err(VmError::TypeError("Expected array".to_string()));
            }
            let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
            let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };

            let len = arr.len();
            let start = start_val.as_i32().unwrap_or(0) as usize;
            let end = end_val.and_then(|v| v.as_i32()).map(|e| e as usize).unwrap_or(len);
            let start = start.min(len);
            let end = end.min(len);

            let mut new_arr = Array::new(arr.type_id, 0);
            if start < end {
                for i in start..end {
                    if let Some(v) = arr.get(i) {
                        new_arr.push(v);
                    }
                }
            }
            let gc_ptr = ctx.gc.lock().allocate(new_arr);
            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
            stack.push(value)?;
            Ok(())
        }
        array::REVERSE => {
            if arg_count != 0 {
                return Err(VmError::RuntimeError(format!(
                    "Array.reverse expects 0 arguments, got {}", arg_count
                )));
            }
            let array_val = stack.pop()?;
            if !array_val.is_ptr() {
                return Err(VmError::TypeError("Expected array".to_string()));
            }
            let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
            let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
            arr.elements.reverse();
            stack.push(array_val)?;
            Ok(())
        }
        array::CONCAT => {
            // concat(other): merge two arrays
            if arg_count != 1 {
                return Err(VmError::RuntimeError(format!(
                    "Array.concat expects 1 argument, got {}", arg_count
                )));
            }
            let other_val = stack.pop()?;
            let array_val = stack.pop()?;

            if !array_val.is_ptr() || !other_val.is_ptr() {
                return Err(VmError::TypeError("Expected array".to_string()));
            }

            let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
            let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
            let other_ptr = unsafe { other_val.as_ptr::<Array>() };
            let other = unsafe { &*other_ptr.unwrap().as_ptr() };

            let mut new_arr = Array::new(0, 0);
            for elem in arr.elements.iter() {
                new_arr.push(*elem);
            }
            for elem in other.elements.iter() {
                new_arr.push(*elem);
            }

            let gc_ptr = ctx.gc.lock().allocate(new_arr);
            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
            stack.push(value)?;
            Ok(())
        }
        array::LAST_INDEX_OF => {
            // lastIndexOf(value): find last occurrence
            if arg_count != 1 {
                return Err(VmError::RuntimeError(format!(
                    "Array.lastIndexOf expects 1 argument, got {}", arg_count
                )));
            }
            let search_val = stack.pop()?;
            let array_val = stack.pop()?;

            if !array_val.is_ptr() {
                return Err(VmError::TypeError("Expected array".to_string()));
            }

            let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
            let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };

            let mut found_index: i32 = -1;
            for (i, elem) in arr.elements.iter().enumerate().rev() {
                // Compare values
                let matches = if let (Some(a), Some(b)) = (elem.as_i32(), search_val.as_i32()) {
                    a == b
                } else if let (Some(a), Some(b)) = (elem.as_f64(), search_val.as_f64()) {
                    a == b
                } else if let (Some(a), Some(b)) = (elem.as_bool(), search_val.as_bool()) {
                    a == b
                } else if elem.is_null() && search_val.is_null() {
                    true
                } else {
                    false
                };
                if matches {
                    found_index = i as i32;
                    break;
                }
            }

            stack.push(Value::i32(found_index))?;
            Ok(())
        }
        array::FILL => {
            // fill(value, start?, end?): fill with value
            if arg_count < 1 || arg_count > 3 {
                return Err(VmError::RuntimeError(format!(
                    "Array.fill expects 1-3 arguments, got {}", arg_count
                )));
            }

            // Pop arguments in reverse order
            let mut args = Vec::with_capacity(arg_count);
            for _ in 0..arg_count {
                args.push(stack.pop()?);
            }
            args.reverse();

            let array_val = stack.pop()?;
            if !array_val.is_ptr() {
                return Err(VmError::TypeError("Expected array".to_string()));
            }

            let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
            let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };

            let fill_value = args[0];
            let start = if arg_count >= 2 { args[1].as_i32().unwrap_or(0).max(0) as usize } else { 0 };
            let end = if arg_count >= 3 { args[2].as_i32().unwrap_or(arr.len() as i32).max(0) as usize } else { arr.len() };

            for i in start..end.min(arr.len()) {
                arr.elements[i] = fill_value;
            }

            stack.push(array_val)?;
            Ok(())
        }
        array::FLAT => {
            // flat(depth?): flatten nested arrays
            let depth = if arg_count >= 1 {
                let d = stack.pop()?.as_i32().unwrap_or(1);
                d.max(0) as usize
            } else {
                1
            };
            let array_val = stack.pop()?;

            if !array_val.is_ptr() {
                return Err(VmError::TypeError("Expected array".to_string()));
            }

            let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
            let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };

            fn flatten(arr: &Array, depth: usize) -> Array {
                let mut result = Array::new(0, 0);
                for elem in arr.elements.iter() {
                    if depth > 0 && elem.is_ptr() {
                        if let Some(ptr) = unsafe { elem.as_ptr::<Array>() } {
                            let inner = unsafe { &*ptr.as_ptr() };
                            let flattened = flatten(inner, depth - 1);
                            for inner_elem in flattened.elements {
                                result.push(inner_elem);
                            }
                            continue;
                        }
                    }
                    result.push(*elem);
                }
                result
            }

            let result = flatten(arr, depth);
            let gc_ptr = ctx.gc.lock().allocate(result);
            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
            stack.push(value)?;
            Ok(())
        }
        array::JOIN => {
            // join(separator?) - arg_count is 0 or 1
            let sep = if arg_count >= 1 {
                let sep_val = stack.pop()?;
                if let Some(ptr) = unsafe { sep_val.as_ptr::<RayaString>() } {
                    let s = unsafe { &*ptr.as_ptr() };
                    s.data.clone()
                } else {
                    ",".to_string()
                }
            } else {
                ",".to_string()
            };
            let array_val = stack.pop()?;

            if !array_val.is_ptr() {
                return Err(VmError::TypeError("Expected array".to_string()));
            }
            let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
            let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };

            // Convert elements to strings and join
            let parts: Vec<String> = arr.elements.iter().map(|v| {
                if let Some(ptr) = unsafe { v.as_ptr::<RayaString>() } {
                    unsafe { &*ptr.as_ptr() }.data.clone()
                } else if let Some(i) = v.as_i32() {
                    i.to_string()
                } else if let Some(f) = v.as_f64() {
                    f.to_string()
                } else if v.is_null() {
                    String::new()
                } else if let Some(b) = v.as_bool() {
                    b.to_string()
                } else {
                    String::new()
                }
            }).collect();
            let result = parts.join(&sep);
            let raya_string = RayaString::new(result);
            let gc_ptr = ctx.gc.lock().allocate(raya_string);
            let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
            stack.push(value)?;
            Ok(())
        }
        _ => Err(VmError::RuntimeError(format!(
            "Array method {:#06x} not implemented",
            method_id
        ))),
    }
}

