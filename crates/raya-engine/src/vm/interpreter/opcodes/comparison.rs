use crate::compiler::Opcode;
use crate::vm::gc::header_ptr_from_value_ptr;
use crate::vm::interpreter::core::value_to_f64;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::RayaString;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use std::any::TypeId;

impl<'a> Interpreter<'a> {
    #[inline]
    fn ptr_is_raya_string(value: Value) -> bool {
        let Some(ptr) = (unsafe { value.as_ptr::<u8>() }) else {
            return false;
        };
        let header = unsafe { &*header_ptr_from_value_ptr(ptr.as_ptr() as *const u8) };
        header.type_id() == TypeId::of::<RayaString>()
    }

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
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let result = if a.is_f64() || b.is_f64() {
                    let fa = a
                        .as_f64()
                        .unwrap_or(a.as_i32().map(|i| i as f64).unwrap_or(0.0));
                    let fb = b
                        .as_f64()
                        .unwrap_or(b.as_i32().map(|i| i as f64).unwrap_or(0.0));
                    fa == fb
                } else {
                    a.as_i32().unwrap_or(0) == b.as_i32().unwrap_or(0)
                };
                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ine => {
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let result = if a.is_f64() || b.is_f64() {
                    let fa = a
                        .as_f64()
                        .unwrap_or(a.as_i32().map(|i| i as f64).unwrap_or(0.0));
                    let fb = b
                        .as_f64()
                        .unwrap_or(b.as_i32().map(|i| i as f64).unwrap_or(0.0));
                    fa != fb
                } else {
                    a.as_i32().unwrap_or(0) != b.as_i32().unwrap_or(0)
                };
                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ilt => {
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let result = if a.is_f64() || b.is_f64() {
                    let fa = a
                        .as_f64()
                        .unwrap_or(a.as_i32().map(|i| i as f64).unwrap_or(0.0));
                    let fb = b
                        .as_f64()
                        .unwrap_or(b.as_i32().map(|i| i as f64).unwrap_or(0.0));
                    fa < fb
                } else {
                    a.as_i32().unwrap_or(0) < b.as_i32().unwrap_or(0)
                };
                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ile => {
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let result = if a.is_f64() || b.is_f64() {
                    let fa = a
                        .as_f64()
                        .unwrap_or(a.as_i32().map(|i| i as f64).unwrap_or(0.0));
                    let fb = b
                        .as_f64()
                        .unwrap_or(b.as_i32().map(|i| i as f64).unwrap_or(0.0));
                    fa <= fb
                } else {
                    a.as_i32().unwrap_or(0) <= b.as_i32().unwrap_or(0)
                };
                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Igt => {
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let result = if a.is_f64() || b.is_f64() {
                    let fa = a
                        .as_f64()
                        .unwrap_or(a.as_i32().map(|i| i as f64).unwrap_or(0.0));
                    let fb = b
                        .as_f64()
                        .unwrap_or(b.as_i32().map(|i| i as f64).unwrap_or(0.0));
                    fa > fb
                } else {
                    a.as_i32().unwrap_or(0) > b.as_i32().unwrap_or(0)
                };
                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Ige => {
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let result = if a.is_f64() || b.is_f64() {
                    let fa = a
                        .as_f64()
                        .unwrap_or(a.as_i32().map(|i| i as f64).unwrap_or(0.0));
                    let fb = b
                        .as_f64()
                        .unwrap_or(b.as_i32().map(|i| i as f64).unwrap_or(0.0));
                    fa >= fb
                } else {
                    a.as_i32().unwrap_or(0) >= b.as_i32().unwrap_or(0)
                };
                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            // =========================================================
            // Float Comparisons
            // =========================================================
            Opcode::Feq => {
                let b = match stack.pop().and_then(value_to_f64) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(value_to_f64) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a == b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fne => {
                let b = match stack.pop().and_then(value_to_f64) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(value_to_f64) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a != b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Flt => {
                let b = match stack.pop().and_then(value_to_f64) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(value_to_f64) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a < b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fle => {
                let b = match stack.pop().and_then(value_to_f64) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(value_to_f64) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a <= b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fgt => {
                let b = match stack.pop().and_then(value_to_f64) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(value_to_f64) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::bool(a > b)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fge => {
                let b = match stack.pop().and_then(value_to_f64) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop().and_then(value_to_f64) {
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
                // NaN != NaN per IEEE 754 — f64 comparison must use float semantics
                let result = if a.is_f64() || b.is_f64() {
                    let fa = a
                        .as_f64()
                        .unwrap_or(a.as_i32().map(|i| i as f64).unwrap_or(0.0));
                    let fb = b
                        .as_f64()
                        .unwrap_or(b.as_i32().map(|i| i as f64).unwrap_or(0.0));
                    fa == fb
                } else if a.is_ptr()
                    && b.is_ptr()
                    && Self::ptr_is_raya_string(a)
                    && Self::ptr_is_raya_string(b)
                {
                    let a_str = unsafe { a.as_ptr::<RayaString>() };
                    let b_str = unsafe { b.as_ptr::<RayaString>() };
                    if let (Some(a_ptr), Some(b_ptr)) = (a_str, b_str) {
                        let a_ref = unsafe { &*a_ptr.as_ptr() };
                        let b_ref = unsafe { &*b_ptr.as_ptr() };
                        a_ref.data == b_ref.data
                    } else {
                        a == b
                    }
                } else {
                    a == b
                };
                if let Err(e) = stack.push(Value::bool(result)) {
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
                // NaN != NaN per IEEE 754 — f64 comparison must use float semantics
                let result = if a.is_f64() || b.is_f64() {
                    let fa = a
                        .as_f64()
                        .unwrap_or(a.as_i32().map(|i| i as f64).unwrap_or(0.0));
                    let fb = b
                        .as_f64()
                        .unwrap_or(b.as_i32().map(|i| i as f64).unwrap_or(0.0));
                    fa != fb
                } else if a.is_ptr()
                    && b.is_ptr()
                    && Self::ptr_is_raya_string(a)
                    && Self::ptr_is_raya_string(b)
                {
                    let a_str = unsafe { a.as_ptr::<RayaString>() };
                    let b_str = unsafe { b.as_ptr::<RayaString>() };
                    if let (Some(a_ptr), Some(b_ptr)) = (a_str, b_str) {
                        let a_ref = unsafe { &*a_ptr.as_ptr() };
                        let b_ref = unsafe { &*b_ptr.as_ptr() };
                        a_ref.data != b_ref.data
                    } else {
                        a != b
                    }
                } else {
                    a != b
                };
                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            _ => unreachable!("Not a comparison opcode: {:?}", opcode),
        }
    }
}
