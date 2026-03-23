//! Closure opcode handlers: MakeClosure, LoadCaptured, StoreCaptured, SetClosureCapture, NewRefCell, LoadRefCell, StoreRefCell

use crate::compiler::{Module, Opcode};
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::Object;
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_closure_ops(
        &mut self,
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        module: &Module,
        task: &Arc<Task>,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::MakeClosure => {
                self.safepoint.poll();
                let func_index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let capture_count = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let mut captures = Vec::with_capacity(capture_count);
                for _ in 0..capture_count {
                    match stack.pop() {
                        Ok(v) => captures.push(v),
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }
                captures.reverse();

                let mut closure =
                    Object::new_closure_with_module(func_index, captures, Arc::new(module.clone()));
                if let Some(env) = self.current_activation_eval_env(task) {
                    let _ = closure.set_callable_direct_eval_env(env);
                    let _ = closure.set_callable_direct_eval_uses_script_global_bindings(
                        task.current_active_direct_eval_uses_script_global_bindings(),
                    );
                }
                let gc_ptr = self.gc.lock().allocate(closure);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadCaptured => {
                let capture_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let closure_val = match task.current_closure() {
                    Some(v) => v,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(
                            "LoadCaptured without active closure".to_string(),
                        ));
                    }
                };

                let closure_ptr = unsafe { closure_val.as_ptr::<Object>() };
                let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                let value = match closure.callable_get_captured(capture_index) {
                    Some(v) => v,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Capture index {} out of bounds",
                            capture_index
                        )));
                    }
                };
                if std::env::var("RAYA_DEBUG_FIELD_TRACE").is_ok() {
                    eprintln!(
                        "[field-trace] LoadCaptured[{}] => {:?} (is_ptr={})",
                        capture_index,
                        value,
                        value.is_ptr()
                    );
                }
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreCaptured => {
                let capture_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let closure_val = match task.current_closure() {
                    Some(v) => v,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(
                            "StoreCaptured without active closure".to_string(),
                        ));
                    }
                };

                let closure_ptr = unsafe { closure_val.as_ptr::<Object>() };
                let closure = unsafe { &mut *closure_ptr.unwrap().as_ptr() };
                if let Err(e) = closure.callable_set_captured(capture_index, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                OpcodeResult::Continue
            }

            Opcode::SetClosureCapture => {
                let capture_index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let closure_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !closure_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected closure".to_string()));
                }

                let closure_ptr = unsafe { closure_val.as_ptr::<Object>() };
                let closure = unsafe { &mut *closure_ptr.unwrap().as_ptr() };
                if let Err(e) = closure.callable_set_captured(capture_index, value) {
                    return OpcodeResult::Error(VmError::RuntimeError(e));
                }
                if let Err(e) = stack.push(closure_val) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::NewRefCell => {
                use crate::vm::object::RefCell;
                let initial_value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let refcell = RefCell::new(initial_value);
                let gc_ptr = self.gc.lock().allocate(refcell);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadRefCell => {
                use crate::vm::object::RefCell;
                let refcell_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !refcell_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected RefCell".to_string()));
                }

                let refcell_ptr = unsafe { refcell_val.as_ptr::<RefCell>() };
                let refcell = unsafe { &*refcell_ptr.unwrap().as_ptr() };
                if let Err(e) = stack.push(refcell.get()) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreRefCell => {
                use crate::vm::object::RefCell;
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let refcell_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !refcell_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected RefCell".to_string()));
                }

                let refcell_ptr = unsafe { refcell_val.as_ptr::<RefCell>() };
                let refcell = unsafe { &mut *refcell_ptr.unwrap().as_ptr() };
                refcell.set(value);
                OpcodeResult::Continue
            }

            _ => OpcodeResult::Error(VmError::RuntimeError(format!(
                "Unexpected opcode in exec_closure_ops: {:?}",
                opcode
            ))),
        }
    }
}
