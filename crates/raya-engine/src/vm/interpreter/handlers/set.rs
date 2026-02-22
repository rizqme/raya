//! Set built-in method handlers

use crate::compiler::Module;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{Array, SetObject};
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    /// Handle built-in set methods dispatched via CallMethod
    pub(in crate::vm::interpreter) fn call_set_method(
        &mut self,
        _task: &Arc<Task>,
        stack: &mut Stack,
        method_id: u16,
        arg_count: usize,
        _module: &Module,
    ) -> Result<(), VmError> {
        use crate::vm::builtin::set;

        // Pop arguments in reverse order
        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(stack.pop()?);
        }
        args.reverse();

        // Pop receiver (set handle)
        let receiver = stack.pop()?;
        let handle = receiver.as_u64().unwrap_or(0);
        let set_ptr = handle as *const SetObject;
        if set_ptr.is_null() {
            return Err(VmError::RuntimeError("Invalid set handle".to_string()));
        }

        match method_id {
            set::SIZE => {
                let set_obj = unsafe { &*set_ptr };
                stack.push(Value::i32(set_obj.size() as i32))?;
            }
            set::ADD => {
                let value = args[0];
                let set_obj = unsafe { &mut *(set_ptr as *mut SetObject) };
                set_obj.add(value);
                stack.push(Value::null())?;
            }
            set::HAS => {
                let value = args[0];
                let set_obj = unsafe { &*set_ptr };
                stack.push(Value::bool(set_obj.has(value)))?;
            }
            set::DELETE => {
                let value = args[0];
                let set_obj = unsafe { &mut *(set_ptr as *mut SetObject) };
                let result = set_obj.delete(value);
                stack.push(Value::bool(result))?;
            }
            set::CLEAR => {
                let set_obj = unsafe { &mut *(set_ptr as *mut SetObject) };
                set_obj.clear();
                stack.push(Value::null())?;
            }
            set::VALUES => {
                let set_obj = unsafe { &*set_ptr };
                let values = set_obj.values();
                let mut arr = Array::new(0, 0);
                for val in values {
                    arr.push(val);
                }
                let arr_gc = self.gc.lock().allocate(arr);
                let arr_val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                };
                stack.push(arr_val)?;
            }
            set::UNION => {
                let other_handle = args[0].as_u64().unwrap_or(0);
                let other_ptr = other_handle as *const SetObject;
                if other_ptr.is_null() {
                    return Err(VmError::RuntimeError("Invalid set handle".to_string()));
                }
                let set_a = unsafe { &*set_ptr };
                let set_b = unsafe { &*other_ptr };
                let mut result = SetObject::new();
                for val in set_a.values() {
                    result.add(val);
                }
                for val in set_b.values() {
                    result.add(val);
                }
                let gc_ptr = self.gc.lock().allocate(result);
                let result_handle = gc_ptr.as_ptr() as u64;
                stack.push(Value::u64(result_handle))?;
            }
            set::INTERSECTION => {
                let other_handle = args[0].as_u64().unwrap_or(0);
                let other_ptr = other_handle as *const SetObject;
                if other_ptr.is_null() {
                    return Err(VmError::RuntimeError("Invalid set handle".to_string()));
                }
                let set_a = unsafe { &*set_ptr };
                let set_b = unsafe { &*other_ptr };
                let mut result = SetObject::new();
                for val in set_a.values() {
                    if set_b.has(val) {
                        result.add(val);
                    }
                }
                let gc_ptr = self.gc.lock().allocate(result);
                let result_handle = gc_ptr.as_ptr() as u64;
                stack.push(Value::u64(result_handle))?;
            }
            set::DIFFERENCE => {
                let other_handle = args[0].as_u64().unwrap_or(0);
                let other_ptr = other_handle as *const SetObject;
                if other_ptr.is_null() {
                    return Err(VmError::RuntimeError("Invalid set handle".to_string()));
                }
                let set_a = unsafe { &*set_ptr };
                let set_b = unsafe { &*other_ptr };
                let mut result = SetObject::new();
                for val in set_a.values() {
                    if !set_b.has(val) {
                        result.add(val);
                    }
                }
                let gc_ptr = self.gc.lock().allocate(result);
                let result_handle = gc_ptr.as_ptr() as u64;
                stack.push(Value::u64(result_handle))?;
            }
            _ => {
                return Err(VmError::RuntimeError(format!(
                    "Set method {:#06x} not implemented", method_id
                )));
            }
        }

        Ok(())
    }
}
