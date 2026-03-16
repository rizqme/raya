use crate::compiler::Module;
use crate::compiler::Opcode;
use crate::vm::gc::header_ptr_from_value_ptr;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::RayaString;
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::any::TypeId;
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    #[inline]
    fn numeric_value(value: Value) -> Option<f64> {
        value.as_f64().or_else(|| value.as_i32().map(|i| i as f64))
    }

    #[inline]
    fn values_equal(a: Value, b: Value) -> bool {
        if let (Some(a_bool), Some(b_bool)) = (a.as_bool(), b.as_bool()) {
            return a_bool == b_bool;
        }

        if let (Some(a_num), Some(b_num)) = (Self::numeric_value(a), Self::numeric_value(b)) {
            return a_num == b_num;
        }

        if a.is_ptr() && b.is_ptr() && Self::ptr_is_raya_string(a) && Self::ptr_is_raya_string(b) {
            let a_str = unsafe { a.as_ptr::<RayaString>() };
            let b_str = unsafe { b.as_ptr::<RayaString>() };
            if let (Some(a_ptr), Some(b_ptr)) = (a_str, b_str) {
                let a_ref = unsafe { &*a_ptr.as_ptr() };
                let b_ref = unsafe { &*b_ptr.as_ptr() };
                return a_ref.data == b_ref.data;
            }
        }

        a == b
    }

    #[inline]
    fn ptr_is_raya_string(value: Value) -> bool {
        let Some(ptr) = (unsafe { value.as_ptr::<u8>() }) else {
            return false;
        };
        let header = unsafe { &*header_ptr_from_value_ptr(ptr.as_ptr() as *const u8) };
        header.type_id() == TypeId::of::<RayaString>()
    }

    fn js_abstract_equality_objectish(&self, value: Value) -> bool {
        self.is_js_object_value(value)
            || Self::is_callable_value(value)
            || self.js_callable_builtin_constructor_name(value).is_some()
    }

    fn js_abstract_equality(
        &mut self,
        a: Value,
        b: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<bool, VmError> {
        if a.is_nullish() && b.is_nullish() {
            return Ok(true);
        }
        if a.is_nullish() || b.is_nullish() {
            return Ok(false);
        }
        if Self::values_equal(a, b) {
            return Ok(true);
        }

        if Self::numeric_value(a).is_some() && Self::ptr_is_raya_string(b) {
            let b_primitive = self.js_to_primitive_number_hint(b, task, module)?;
            let b_number = self.js_to_number_from_primitive(b_primitive)?;
            return Ok(Self::numeric_value(a).unwrap() == b_number);
        }
        if Self::ptr_is_raya_string(a) && Self::numeric_value(b).is_some() {
            let a_primitive = self.js_to_primitive_number_hint(a, task, module)?;
            let a_number = self.js_to_number_from_primitive(a_primitive)?;
            return Ok(a_number == Self::numeric_value(b).unwrap());
        }

        if a.as_bool().is_some() {
            let coerced = Value::f64(self.js_to_number_from_primitive(a)?);
            return self.js_abstract_equality(coerced, b, task, module);
        }
        if b.as_bool().is_some() {
            let coerced = Value::f64(self.js_to_number_from_primitive(b)?);
            return self.js_abstract_equality(a, coerced, task, module);
        }

        if self.js_abstract_equality_objectish(a)
            && (Self::numeric_value(b).is_some()
                || Self::ptr_is_raya_string(b)
                || b.as_bool().is_some())
        {
            let primitive = self.js_to_primitive_with_hint(a, "default", task, module)?;
            return self.js_abstract_equality(primitive, b, task, module);
        }
        if self.js_abstract_equality_objectish(b)
            && (Self::numeric_value(a).is_some()
                || Self::ptr_is_raya_string(a)
                || a.as_bool().is_some())
        {
            let primitive = self.js_to_primitive_with_hint(b, "default", task, module)?;
            return self.js_abstract_equality(a, primitive, task, module);
        }

        Ok(false)
    }

    fn js_abstract_relational_compare(
        &mut self,
        left: Value,
        right: Value,
        left_first: bool,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Option<bool>, VmError> {
        let (left_primitive, right_primitive) = if left_first {
            (
                self.js_to_primitive_number_hint(left, task, module)?,
                self.js_to_primitive_number_hint(right, task, module)?,
            )
        } else {
            let right_primitive = self.js_to_primitive_number_hint(right, task, module)?;
            let left_primitive = self.js_to_primitive_number_hint(left, task, module)?;
            (left_primitive, right_primitive)
        };

        if Self::ptr_is_raya_string(left_primitive) && Self::ptr_is_raya_string(right_primitive) {
            let left_ptr = unsafe { left_primitive.as_ptr::<RayaString>() }.expect("string ptr");
            let right_ptr =
                unsafe { right_primitive.as_ptr::<RayaString>() }.expect("string ptr");
            let left_str = unsafe { &*left_ptr.as_ptr() };
            let right_str = unsafe { &*right_ptr.as_ptr() };
            return Ok(Some(left_str.data < right_str.data));
        }

        let left_number = self.js_to_number_from_primitive(left_primitive)?;
        let right_number = self.js_to_number_from_primitive(right_primitive)?;
        if left_number.is_nan() || right_number.is_nan() {
            return Ok(None);
        }
        Ok(Some(left_number < right_number))
    }

    fn js_less_than(
        &mut self,
        left: Value,
        right: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<bool, VmError> {
        Ok(self
            .js_abstract_relational_compare(left, right, true, task, module)?
            .unwrap_or(false))
    }

    fn js_less_equal(
        &mut self,
        left: Value,
        right: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<bool, VmError> {
        Ok(match self.js_abstract_relational_compare(right, left, false, task, module)? {
            Some(value) => !value,
            None => false,
        })
    }

    fn js_greater_than(
        &mut self,
        left: Value,
        right: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<bool, VmError> {
        Ok(self
            .js_abstract_relational_compare(right, left, false, task, module)?
            .unwrap_or(false))
    }

    fn js_greater_equal(
        &mut self,
        left: Value,
        right: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<bool, VmError> {
        Ok(match self.js_abstract_relational_compare(left, right, true, task, module)? {
            Some(value) => !value,
            None => false,
        })
    }

    pub(in crate::vm::interpreter) fn exec_comparison_ops(
        &mut self,
        stack: &mut Stack,
        module: &Module,
        task: &Arc<Task>,
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
                let result = match self.js_abstract_equality(a, b, task, module) {
                    Ok(value) => value,
                    Err(error) => return OpcodeResult::Error(error),
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
                let result = match self.js_abstract_equality(a, b, task, module) {
                    Ok(value) => !value,
                    Err(error) => return OpcodeResult::Error(error),
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
                let result = match self.js_less_than(a, b, task, module) {
                    Ok(value) => value,
                    Err(error) => return OpcodeResult::Error(error),
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
                let result = match self.js_less_equal(a, b, task, module) {
                    Ok(value) => value,
                    Err(error) => return OpcodeResult::Error(error),
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
                let result = match self.js_greater_than(a, b, task, module) {
                    Ok(value) => value,
                    Err(error) => return OpcodeResult::Error(error),
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
                let result = match self.js_greater_equal(a, b, task, module) {
                    Ok(value) => value,
                    Err(error) => return OpcodeResult::Error(error),
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
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let result = match (Self::numeric_value(a), Self::numeric_value(b)) {
                    (Some(a_num), Some(b_num)) => a_num == b_num,
                    _ => false,
                };
                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fne => {
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let result = match (Self::numeric_value(a), Self::numeric_value(b)) {
                    (Some(a_num), Some(b_num)) => a_num != b_num,
                    _ => true,
                };
                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Flt => {
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let result = match self.js_less_than(a, b, task, module) {
                    Ok(value) => value,
                    Err(error) => return OpcodeResult::Error(error),
                };
                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fle => {
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let result = match self.js_less_equal(a, b, task, module) {
                    Ok(value) => value,
                    Err(error) => return OpcodeResult::Error(error),
                };
                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fgt => {
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let result = match self.js_greater_than(a, b, task, module) {
                    Ok(value) => value,
                    Err(error) => return OpcodeResult::Error(error),
                };
                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Fge => {
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let result = match self.js_greater_equal(a, b, task, module) {
                    Ok(value) => value,
                    Err(error) => return OpcodeResult::Error(error),
                };
                if let Err(e) = stack.push(Value::bool(result)) {
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
                let result = if opcode == Opcode::Eq && a.is_nullish() && b.is_nullish() {
                    true
                } else if opcode == Opcode::StrictEq && a.is_nullish() != b.is_nullish() {
                    false
                } else if opcode == Opcode::Eq {
                    match self.js_abstract_equality(a, b, task, module) {
                        Ok(value) => value,
                        Err(error) => return OpcodeResult::Error(error),
                    }
                } else {
                    Self::values_equal(a, b)
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
                let result = if opcode == Opcode::Ne && a.is_nullish() && b.is_nullish() {
                    false
                } else if opcode == Opcode::StrictNe && a.is_nullish() != b.is_nullish() {
                    true
                } else if opcode == Opcode::Ne {
                    match self.js_abstract_equality(a, b, task, module) {
                        Ok(value) => !value,
                        Err(error) => return OpcodeResult::Error(error),
                    }
                } else {
                    !Self::values_equal(a, b)
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
