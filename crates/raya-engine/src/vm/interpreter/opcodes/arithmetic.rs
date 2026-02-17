use crate::vm::interpreter::core::value_to_f64;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use crate::compiler::Opcode;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_arithmetic_ops(
        &mut self,
        stack: &mut Stack,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            // =========================================================
            // Integer Arithmetic
            // =========================================================
            Opcode::Iadd => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a.wrapping_add(b))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Isub => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a.wrapping_sub(b))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Imul => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a.wrapping_mul(b))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Idiv => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if b == 0 {
                    return OpcodeResult::Error(VmError::RuntimeError(
                        "Division by zero".to_string(),
                    ));
                }
                if let Err(e) = stack.push(Value::i32(a / b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Imod => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if b == 0 {
                    return OpcodeResult::Error(VmError::RuntimeError(
                        "Division by zero".to_string(),
                    ));
                }
                if let Err(e) = stack.push(Value::i32(a % b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ineg => {
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(-a)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ipow => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a.pow(b as u32))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Integer Bitwise
            // =========================================================
            Opcode::Ishl => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a << (b & 31))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ishr => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a >> (b & 31))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Iushr => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(((a as u32) >> (b & 31)) as i32)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Iand => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a & b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ior => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a | b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ixor => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a ^ b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Inot => {
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(!a)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Float Arithmetic
            // =========================================================
            Opcode::Fadd => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(a + b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fsub => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(a - b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fmul => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(a * b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fdiv => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(a / b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fneg => {
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(-a)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fpow => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(a.powf(b))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fmod => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(a % b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Generic Number Operations (N = handles both i32 and f64)
            // =========================================================
            Opcode::Nadd | Opcode::Nsub | Opcode::Nmul | Opcode::Ndiv | Opcode::Nmod | Opcode::Nneg | Opcode::Npow => {
                // Helper to convert value to f64
                fn value_to_number(v: Value) -> f64 {
                    if let Some(f) = v.as_f64() {
                        f
                    } else if let Some(i) = v.as_i32() {
                        i as f64
                    } else if let Some(i) = v.as_i64() {
                        i as f64
                    } else {
                        0.0
                    }
                }

                // Helper to check if value is f64
                fn is_float(v: &Value) -> bool {
                    v.is_f64()
                }

                match opcode {
                    Opcode::Nadd => {
                        let b_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let a_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        // If either is float, use float arithmetic
                        let result = if is_float(&a_val) || is_float(&b_val) {
                            Value::f64(value_to_number(a_val) + value_to_number(b_val))
                        } else {
                            let a = a_val.as_i32().unwrap_or(0);
                            let b = b_val.as_i32().unwrap_or(0);
                            Value::i32(a.wrapping_add(b))
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    Opcode::Nsub => {
                        let b_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let a_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let result = if is_float(&a_val) || is_float(&b_val) {
                            Value::f64(value_to_number(a_val) - value_to_number(b_val))
                        } else {
                            let a = a_val.as_i32().unwrap_or(0);
                            let b = b_val.as_i32().unwrap_or(0);
                            Value::i32(a.wrapping_sub(b))
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    Opcode::Nmul => {
                        let b_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let a_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let result = if is_float(&a_val) || is_float(&b_val) {
                            Value::f64(value_to_number(a_val) * value_to_number(b_val))
                        } else {
                            let a = a_val.as_i32().unwrap_or(0);
                            let b = b_val.as_i32().unwrap_or(0);
                            Value::i32(a.wrapping_mul(b))
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    Opcode::Ndiv => {
                        // Division always returns f64
                        let b_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let a_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let a = value_to_number(a_val);
                        let b = value_to_number(b_val);
                        let result = if b != 0.0 { a / b } else { f64::NAN };
                        if let Err(e) = stack.push(Value::f64(result)) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    Opcode::Nmod => {
                        let b_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let a_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let result = if is_float(&a_val) || is_float(&b_val) {
                            let a = value_to_number(a_val);
                            let b = value_to_number(b_val);
                            Value::f64(if b != 0.0 { a % b } else { f64::NAN })
                        } else {
                            let a = a_val.as_i32().unwrap_or(0);
                            let b = b_val.as_i32().unwrap_or(1);
                            Value::i32(if b != 0 { a % b } else { 0 })
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    Opcode::Nneg => {
                        let a_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let result = if is_float(&a_val) {
                            Value::f64(-value_to_number(a_val))
                        } else {
                            Value::i32(-a_val.as_i32().unwrap_or(0))
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    Opcode::Npow => {
                        let b_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let a_val = match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        let result = if is_float(&a_val) || is_float(&b_val) {
                            Value::f64(value_to_number(a_val).powf(value_to_number(b_val)))
                        } else {
                            let a = a_val.as_i32().unwrap_or(0);
                            let b = b_val.as_i32().unwrap_or(0);
                            Value::i32(a.pow(b as u32))
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    _ => unreachable!(),
                }
                OpcodeResult::Continue
            }

            _ => unreachable!("Not an arithmetic opcode: {:?}", opcode),
        }
    }
}
