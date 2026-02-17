//! Array opcode handlers: NewArray, LoadElem, StoreElem, ArrayLen, ArrayPush, ArrayPop, ArrayLiteral, InitArray

use crate::compiler::Opcode;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::Array;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_array_ops(
        &mut self,
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::NewArray => {
                self.safepoint.poll();
                let type_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let len = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0) as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let arr = Array::new(type_index, len);
                let gc_ptr = self.gc.lock().allocate(arr);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadElem => {
                let index = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0) as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arr_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !arr_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected array".to_string()));
                }

                let arr_ptr = unsafe { arr_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                let value = match arr.get(index) {
                    Some(v) => {
                        v
                    }
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Array index {} out of bounds",
                            index
                        )));
                    }
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreElem => {
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let index = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0) as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arr_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !arr_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected array".to_string()));
                }

                let arr_ptr = unsafe { arr_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                if let Err(e) = arr.set(index, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                OpcodeResult::Continue
            }

            Opcode::ArrayLen => {
                let arr_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !arr_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected array".to_string()));
                }

                let arr_ptr = unsafe { arr_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                if let Err(e) = stack.push(Value::i32(arr.len() as i32)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ArrayPush => {
                // Stack: [value, array] -> [] (mutates array in-place)
                let element = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arr_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !arr_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected array".to_string()));
                }

                let arr_ptr = unsafe { arr_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                arr.push(element);
                OpcodeResult::Continue
            }

            Opcode::ArrayPop => {
                // Stack: [array] -> [popped_element]
                let arr_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !arr_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected array".to_string()));
                }

                let arr_ptr = unsafe { arr_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                let value = arr.pop().unwrap_or(Value::null());
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ArrayLiteral => {
                self.safepoint.poll();
                let type_index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let length = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Pop elements from stack in reverse order (last pushed = last element)
                let mut elements = Vec::with_capacity(length);
                for _ in 0..length {
                    match stack.pop() {
                        Ok(v) => elements.push(v),
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }
                // Reverse to get correct order (first pushed = first element)
                elements.reverse();

                // Create array with the elements
                let mut arr = Array::new(type_index, length);
                for (i, elem) in elements.into_iter().enumerate() {
                    if let Err(e) = arr.set(i, elem) {
                        return OpcodeResult::Error(VmError::RuntimeError(e));
                    }
                }

                let gc_ptr = self.gc.lock().allocate(arr);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::InitArray => {
                let index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arr_val = match stack.peek() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !arr_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected array".to_string()));
                }

                let arr_ptr = unsafe { arr_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                if let Err(e) = arr.set(index, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                OpcodeResult::Continue
            }

            _ => OpcodeResult::Error(VmError::RuntimeError(format!(
                "Unexpected opcode in exec_array_ops: {:?}",
                opcode
            ))),
        }
    }
}
