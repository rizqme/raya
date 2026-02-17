//! String opcode handlers: Sconcat, Slen, Seq, Sne, Slt, Sle, Sgt, Sge, ToString

use crate::compiler::Opcode;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::RayaString;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_string_ops(
        &mut self,
        stack: &mut Stack,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::Sconcat => {
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let a_str = if a_val.is_ptr() {
                    let ptr = unsafe { a_val.as_ptr::<RayaString>() };
                    unsafe { &*ptr.unwrap().as_ptr() }.data.clone()
                } else {
                    format!("{:?}", a_val)
                };

                let b_str = if b_val.is_ptr() {
                    let ptr = unsafe { b_val.as_ptr::<RayaString>() };
                    unsafe { &*ptr.unwrap().as_ptr() }.data.clone()
                } else {
                    format!("{:?}", b_val)
                };

                let result = RayaString::new(format!("{}{}", a_str, b_str));
                let gc_ptr = self.gc.lock().allocate(result);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Slen => {
                let s_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !s_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected string".to_string()));
                }

                let str_ptr = unsafe { s_val.as_ptr::<RayaString>() };
                let s = unsafe { &*str_ptr.unwrap().as_ptr() };
                if let Err(e) = stack.push(Value::i32(s.len() as i32)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Seq => {
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let result = if a_val.is_ptr() && b_val.is_ptr() {
                    let a_ptr = unsafe { a_val.as_ptr::<RayaString>() };
                    let b_ptr = unsafe { b_val.as_ptr::<RayaString>() };
                    let a = unsafe { &*a_ptr.unwrap().as_ptr() };
                    let b = unsafe { &*b_ptr.unwrap().as_ptr() };
                    a.data == b.data
                } else {
                    false
                };
                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Sne => {
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let result = if a_val.is_ptr() && b_val.is_ptr() {
                    let a_ptr = unsafe { a_val.as_ptr::<RayaString>() };
                    let b_ptr = unsafe { b_val.as_ptr::<RayaString>() };
                    let a = unsafe { &*a_ptr.unwrap().as_ptr() };
                    let b = unsafe { &*b_ptr.unwrap().as_ptr() };
                    a.data != b.data
                } else {
                    true
                };
                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Slt | Opcode::Sle | Opcode::Sgt | Opcode::Sge => {
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let result = if a_val.is_ptr() && b_val.is_ptr() {
                    let a_ptr = unsafe { a_val.as_ptr::<RayaString>() };
                    let b_ptr = unsafe { b_val.as_ptr::<RayaString>() };
                    let a = unsafe { &*a_ptr.unwrap().as_ptr() };
                    let b = unsafe { &*b_ptr.unwrap().as_ptr() };
                    match opcode {
                        Opcode::Slt => a.data < b.data,
                        Opcode::Sle => a.data <= b.data,
                        Opcode::Sgt => a.data > b.data,
                        Opcode::Sge => a.data >= b.data,
                        _ => unreachable!(),
                    }
                } else {
                    false
                };
                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ToString => {
                let val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                // Convert value to string properly
                let s = if val.is_null() {
                    "null".to_string()
                } else if let Some(b) = val.as_bool() {
                    if b { "true".to_string() } else { "false".to_string() }
                } else if let Some(i) = val.as_i32() {
                    i.to_string()
                } else if let Some(f) = val.as_f64() {
                    // Format float like JavaScript: no trailing zeros, no scientific notation for small numbers
                    if f.fract() == 0.0 && f.abs() < 1e15 {
                        (f as i64).to_string()
                    } else {
                        f.to_string()
                    }
                } else if val.is_ptr() {
                    // Check if it's already a string
                    if let Some(ptr) = unsafe { val.as_ptr::<RayaString>() } {
                        unsafe { &*ptr.as_ptr() }.data.clone()
                    } else {
                        "[object]".to_string()
                    }
                } else {
                    "undefined".to_string()
                };
                let result = RayaString::new(s);
                let gc_ptr = self.gc.lock().allocate(result);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            _ => OpcodeResult::Error(VmError::RuntimeError(format!(
                "Unexpected opcode in exec_string_ops: {:?}",
                opcode
            ))),
        }
    }
}
