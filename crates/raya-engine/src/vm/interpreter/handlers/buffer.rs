//! Buffer built-in method handlers

use crate::compiler::Module;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{Buffer, RayaString};
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    /// Handle built-in buffer methods dispatched via CallMethod
    pub(in crate::vm::interpreter) fn call_buffer_method(
        &mut self,
        _task: &Arc<Task>,
        stack: &mut Stack,
        method_id: u16,
        arg_count: usize,
        _module: &Module,
    ) -> Result<(), VmError> {
        use crate::vm::builtin::buffer;

        // Pop arguments in reverse order
        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(stack.pop()?);
        }
        args.reverse();

        // Pop receiver (buffer handle)
        let receiver = stack.pop()?;
        let handle = receiver.as_u64().unwrap_or(0);
        let buf_ptr = handle as *const Buffer;
        if buf_ptr.is_null() {
            return Err(VmError::RuntimeError("Invalid buffer handle".to_string()));
        }

        match method_id {
            buffer::LENGTH => {
                let buf = unsafe { &*buf_ptr };
                stack.push(Value::i32(buf.length() as i32))?;
            }
            buffer::GET_BYTE => {
                let index = args[0].as_i32().unwrap_or(0) as usize;
                let buf = unsafe { &*buf_ptr };
                let value = buf.get_byte(index).unwrap_or(0);
                stack.push(Value::i32(value as i32))?;
            }
            buffer::SET_BYTE => {
                let index = args[0].as_i32().unwrap_or(0) as usize;
                let value = args[1].as_i32().unwrap_or(0) as u8;
                let buf = unsafe { &mut *(buf_ptr as *mut Buffer) };
                if let Err(msg) = buf.set_byte(index, value) {
                    return Err(VmError::RuntimeError(msg));
                }
                stack.push(Value::null())?;
            }
            buffer::GET_INT32 => {
                let index = args[0].as_i32().unwrap_or(0) as usize;
                let buf = unsafe { &*buf_ptr };
                let value = buf.get_int32(index).unwrap_or(0);
                stack.push(Value::i32(value))?;
            }
            buffer::SET_INT32 => {
                let index = args[0].as_i32().unwrap_or(0) as usize;
                let value = args[1].as_i32().unwrap_or(0);
                let buf = unsafe { &mut *(buf_ptr as *mut Buffer) };
                if let Err(msg) = buf.set_int32(index, value) {
                    return Err(VmError::RuntimeError(msg));
                }
                stack.push(Value::null())?;
            }
            buffer::GET_FLOAT64 => {
                let index = args[0].as_i32().unwrap_or(0) as usize;
                let buf = unsafe { &*buf_ptr };
                let value = buf.get_float64(index).unwrap_or(0.0);
                stack.push(Value::f64(value))?;
            }
            buffer::SET_FLOAT64 => {
                let index = args[0].as_i32().unwrap_or(0) as usize;
                let value = args[1].as_f64().unwrap_or(0.0);
                let buf = unsafe { &mut *(buf_ptr as *mut Buffer) };
                if let Err(msg) = buf.set_float64(index, value) {
                    return Err(VmError::RuntimeError(msg));
                }
                stack.push(Value::null())?;
            }
            buffer::SLICE => {
                let start = args[0].as_i32().unwrap_or(0) as usize;
                let end = args[1].as_i32().unwrap_or(0) as usize;
                let buf = unsafe { &*buf_ptr };
                let sliced = buf.slice(start, end);
                let gc_ptr = self.gc.lock().allocate(sliced);
                let new_handle = gc_ptr.as_ptr() as u64;
                stack.push(Value::u64(new_handle))?;
            }
            buffer::COPY => {
                // copy(target, targetStart, sourceStart, sourceEnd)
                let tgt_handle = args[0].as_u64().unwrap_or(0);
                let tgt_start = args[1].as_i32().unwrap_or(0) as usize;
                let src_start = args[2].as_i32().unwrap_or(0) as usize;
                let src_end = args[3].as_i32().unwrap_or(0) as usize;
                let tgt_ptr = tgt_handle as *mut Buffer;
                if tgt_ptr.is_null() {
                    return Err(VmError::RuntimeError("Invalid target buffer handle".to_string()));
                }
                let src = unsafe { &*buf_ptr };
                let tgt = unsafe { &mut *tgt_ptr };
                let src_end = src_end.min(src.data.len());
                let src_start = src_start.min(src_end);
                let bytes = &src.data[src_start..src_end];
                let copy_len = bytes.len().min(tgt.data.len().saturating_sub(tgt_start));
                tgt.data[tgt_start..tgt_start + copy_len].copy_from_slice(&bytes[..copy_len]);
                stack.push(Value::i32(copy_len as i32))?;
            }
            buffer::TO_STRING => {
                let buf = unsafe { &*buf_ptr };
                // encoding argument — currently only utf8 supported
                let text = String::from_utf8_lossy(&buf.data).into_owned();
                let s = RayaString::new(text);
                let gc_ptr = self.gc.lock().allocate(s);
                let val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };
                stack.push(val)?;
            }
            _ => {
                return Err(VmError::RuntimeError(format!(
                    "Buffer method {:#06x} not implemented", method_id
                )));
            }
        }

        Ok(())
    }
}
