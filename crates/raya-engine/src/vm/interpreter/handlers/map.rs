//! Map built-in method handlers

use crate::compiler::Module;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{Array, MapObject};
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    /// Handle built-in map methods dispatched via CallMethod
    pub(in crate::vm::interpreter) fn call_map_method(
        &mut self,
        _task: &Arc<Task>,
        stack: &mut Stack,
        method_id: u16,
        arg_count: usize,
        _module: &Module,
    ) -> Result<(), VmError> {
        use crate::vm::builtin::map;

        // Pop arguments in reverse order
        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(stack.pop()?);
        }
        args.reverse();

        // Pop receiver (map handle)
        let receiver = stack.pop()?;
        let handle = receiver.as_u64().unwrap_or(0);
        let map_ptr = handle as *const MapObject;
        if map_ptr.is_null() {
            return Err(VmError::RuntimeError("Invalid map handle".to_string()));
        }

        match method_id {
            map::SIZE => {
                let map_obj = unsafe { &*map_ptr };
                stack.push(Value::i32(map_obj.size() as i32))?;
            }
            map::GET => {
                let key = args[0];
                let map_obj = unsafe { &*map_ptr };
                let value = map_obj.get(key).unwrap_or(Value::null());
                stack.push(value)?;
            }
            map::SET => {
                let key = args[0];
                let value = args[1];
                let map_obj = unsafe { &mut *(map_ptr as *mut MapObject) };
                map_obj.set(key, value);
                stack.push(Value::null())?;
            }
            map::HAS => {
                let key = args[0];
                let map_obj = unsafe { &*map_ptr };
                stack.push(Value::bool(map_obj.has(key)))?;
            }
            map::DELETE => {
                let key = args[0];
                let map_obj = unsafe { &mut *(map_ptr as *mut MapObject) };
                let result = map_obj.delete(key);
                stack.push(Value::bool(result))?;
            }
            map::CLEAR => {
                let map_obj = unsafe { &mut *(map_ptr as *mut MapObject) };
                map_obj.clear();
                stack.push(Value::null())?;
            }
            map::KEYS => {
                let map_obj = unsafe { &*map_ptr };
                let keys = map_obj.keys();
                let mut arr = Array::new(0, 0);
                for key in keys {
                    arr.push(key);
                }
                let arr_gc = self.gc.lock().allocate(arr);
                let arr_val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                };
                stack.push(arr_val)?;
            }
            map::VALUES => {
                let map_obj = unsafe { &*map_ptr };
                let values = map_obj.values();
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
            map::ENTRIES => {
                let map_obj = unsafe { &*map_ptr };
                let entries = map_obj.entries();
                let mut arr = Array::new(0, 0);
                for (key, val) in entries {
                    let mut entry = Array::new(0, 0);
                    entry.push(key);
                    entry.push(val);
                    let entry_gc = self.gc.lock().allocate(entry);
                    let entry_val = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(entry_gc.as_ptr()).unwrap())
                    };
                    arr.push(entry_val);
                }
                let arr_gc = self.gc.lock().allocate(arr);
                let arr_val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                };
                stack.push(arr_val)?;
            }
            _ => {
                return Err(VmError::RuntimeError(format!(
                    "Map method {:#06x} not implemented", method_id
                )));
            }
        }

        Ok(())
    }
}
