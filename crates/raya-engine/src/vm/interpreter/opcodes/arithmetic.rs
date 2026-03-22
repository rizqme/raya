use crate::compiler::{Module, Opcode};
use crate::vm::interpreter::core::value_to_f64;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    /// ES spec ToNumber: calls ToPrimitive then converts the primitive to f64.
    /// Unlike the free `value_to_f64`, this can invoke valueOf/Symbol.toPrimitive
    /// on heap objects.
    #[inline]
    fn to_number(&mut self, v: Value, task: &Arc<Task>, module: &Module) -> Result<f64, VmError> {
        // Fast path: primitives don't need ToPrimitive
        if v.as_f64().is_some()
            || v.as_i32().is_some()
            || v.as_bool().is_some()
            || v.is_null()
            || v.is_undefined()
        {
            return value_to_f64(v);
        }
        // String: parse directly (no valueOf call needed)
        if super::native::checked_string_ptr(v).is_some() {
            return value_to_f64(v);
        }
        // Heap object: call ToPrimitive(value, "number") then convert
        self.js_to_number_with_context(v, task, module)
    }

    pub(in crate::vm::interpreter) fn exec_arithmetic_ops(
        &mut self,
        stack: &mut Stack,
        module: &Module,
        task: &Arc<Task>,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            // =========================================================
            // Integer Arithmetic
            // =========================================================
            Opcode::Iadd => {
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = a_val
                    .as_i32()
                    .or_else(|| a_val.as_f64().map(|f| f as i32))
                    .unwrap_or(0);
                let b = b_val
                    .as_i32()
                    .or_else(|| b_val.as_f64().map(|f| f as i32))
                    .unwrap_or(0);
                if let Err(e) = stack.push(Value::i32(a.wrapping_add(b))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Isub => {
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = a_val
                    .as_i32()
                    .or_else(|| a_val.as_f64().map(|f| f as i32))
                    .unwrap_or(0);
                let b = b_val
                    .as_i32()
                    .or_else(|| b_val.as_f64().map(|f| f as i32))
                    .unwrap_or(0);
                if let Err(e) = stack.push(Value::i32(a.wrapping_sub(b))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Imul => {
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                // Try i32 first, fall back to f64→i32 conversion for values that
                // are f64 at runtime due to type inference gaps (e.g., loop accumulators).
                let a = a_val
                    .as_i32()
                    .or_else(|| a_val.as_f64().map(|f| f as i32))
                    .unwrap_or(0);
                let b = b_val
                    .as_i32()
                    .or_else(|| b_val.as_f64().map(|f| f as i32))
                    .unwrap_or(0);
                if let Err(e) = stack.push(Value::i32(a.wrapping_mul(b))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Idiv => {
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = a_val
                    .as_i32()
                    .or_else(|| a_val.as_f64().map(|f| f as i32))
                    .unwrap_or(0);
                let b = b_val
                    .as_i32()
                    .or_else(|| b_val.as_f64().map(|f| f as i32))
                    .unwrap_or(0);
                if b == 0 {
                    return OpcodeResult::Error(VmError::RuntimeError(
                        "division by zero".to_string(),
                    ));
                }
                if let Err(e) = stack.push(Value::i32(a.wrapping_div(b))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Imod => {
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = a_val
                    .as_i32()
                    .or_else(|| a_val.as_f64().map(|f| f as i32))
                    .unwrap_or(0);
                let b = b_val
                    .as_i32()
                    .or_else(|| b_val.as_f64().map(|f| f as i32))
                    .unwrap_or(0);
                if b == 0 {
                    return OpcodeResult::Error(VmError::RuntimeError(
                        "division by zero".to_string(),
                    ));
                }
                if let Err(e) = stack.push(Value::i32(a.wrapping_rem(b))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ineg => {
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = a_val
                    .as_i32()
                    .or_else(|| a_val.as_f64().map(|f| f as i32))
                    .unwrap_or(0);
                if let Err(e) = stack.push(Value::i32(a.wrapping_neg())) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ipow => {
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = a_val
                    .as_i32()
                    .or_else(|| a_val.as_f64().map(|f| f as i32))
                    .unwrap_or(0);
                let b = b_val
                    .as_i32()
                    .or_else(|| b_val.as_f64().map(|f| f as i32))
                    .unwrap_or(0);
                let result = if b < 0 { 0 } else { a.wrapping_pow(b as u32) };
                if let Err(e) = stack.push(Value::i32(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Integer Bitwise
            // =========================================================
            Opcode::Ishl => {
                let b = match stack.pop() {
                    Ok(v) => v
                        .as_i32()
                        .or_else(|| v.as_f64().map(|f| f as i32))
                        .unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v
                        .as_i32()
                        .or_else(|| v.as_f64().map(|f| f as i32))
                        .unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a << (b & 31))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ishr => {
                let b = match stack.pop() {
                    Ok(v) => v
                        .as_i32()
                        .or_else(|| v.as_f64().map(|f| f as i32))
                        .unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v
                        .as_i32()
                        .or_else(|| v.as_f64().map(|f| f as i32))
                        .unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a >> (b & 31))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Iushr => {
                let b = match stack.pop() {
                    Ok(v) => v
                        .as_i32()
                        .or_else(|| v.as_f64().map(|f| f as i32))
                        .unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v
                        .as_i32()
                        .or_else(|| v.as_f64().map(|f| f as i32))
                        .unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(((a as u32) >> (b & 31)) as i32)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Iand => {
                let b = match stack.pop() {
                    Ok(v) => v
                        .as_i32()
                        .or_else(|| v.as_f64().map(|f| f as i32))
                        .unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v
                        .as_i32()
                        .or_else(|| v.as_f64().map(|f| f as i32))
                        .unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a & b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ior => {
                let b = match stack.pop() {
                    Ok(v) => v
                        .as_i32()
                        .or_else(|| v.as_f64().map(|f| f as i32))
                        .unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v
                        .as_i32()
                        .or_else(|| v.as_f64().map(|f| f as i32))
                        .unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a | b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ixor => {
                let b = match stack.pop() {
                    Ok(v) => v
                        .as_i32()
                        .or_else(|| v.as_f64().map(|f| f as i32))
                        .unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v
                        .as_i32()
                        .or_else(|| v.as_f64().map(|f| f as i32))
                        .unwrap_or(0),
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(a ^ b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Inot => {
                let a = match stack.pop() {
                    Ok(v) => v
                        .as_i32()
                        .or_else(|| v.as_f64().map(|f| f as i32))
                        .unwrap_or(0),
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
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let b = match self.to_number(b_val, task, module) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match self.to_number(a_val, task, module) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(a + b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fsub => {
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let b = match self.to_number(b_val, task, module) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match self.to_number(a_val, task, module) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(a - b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fmul => {
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let b = match self.to_number(b_val, task, module) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match self.to_number(a_val, task, module) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(a * b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fdiv => {
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if b_val.as_i32().is_some() && a_val.as_i32().is_some() && b_val.as_i32() == Some(0)
                {
                    return OpcodeResult::Error(VmError::RuntimeError(
                        "division by zero".to_string(),
                    ));
                }
                let b = match self.to_number(b_val, task, module) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match self.to_number(a_val, task, module) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(a / b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fneg => {
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match self.to_number(a_val, task, module) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(-a)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fpow => {
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let b = match self.to_number(b_val, task, module) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match self.to_number(a_val, task, module) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(a.powf(b))) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fmod => {
                let b_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if b_val.as_i32().is_some() && a_val.as_i32().is_some() && b_val.as_i32() == Some(0)
                {
                    return OpcodeResult::Error(VmError::RuntimeError(
                        "division by zero".to_string(),
                    ));
                }
                let b = match self.to_number(b_val, task, module) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match self.to_number(a_val, task, module) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(a % b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            _ => unreachable!("Not an arithmetic opcode: {:?}", opcode),
        }
    }
}
