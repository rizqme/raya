use crate::vm::interpreter::core::value_to_f64;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::compiler::Opcode;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_comparison_ops(
        &mut self,
        stack: &mut Stack,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            // =========================================================
            // Integer Comparisons
            // =========================================================
            Opcode::Ieq => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a == b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ine => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a != b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ilt => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a < b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ile => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a <= b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Igt => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a > b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ige => {
                let b = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v.as_i32().unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a >= b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Float Comparisons
            // =========================================================
            Opcode::Feq => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a == b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fne => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a != b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Flt => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a < b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fle => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a <= b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fgt => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a > b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fge => {
                let b = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(|v| value_to_f64(v)) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a >= b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Boolean Operations
            // =========================================================
            Opcode::Not => {
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(!a.is_truthy())) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::And => {
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a.is_truthy() && b.is_truthy())) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Or => {
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a.is_truthy() || b.is_truthy())) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Generic Equality
            // =========================================================
            Opcode::Eq | Opcode::StrictEq => {
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a == b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ne | Opcode::StrictNe => {
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a != b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            _ => unreachable!("Not a comparison opcode: {:?}", opcode),
        }
    }
}
