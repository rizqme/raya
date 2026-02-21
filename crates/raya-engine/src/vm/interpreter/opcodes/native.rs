//! Native call opcode handlers: NativeCall, ModuleNativeCall
//!
//! NativeCall dispatches to built-in operations (channel, buffer, map, set, date, regexp, etc.)
//! and reflect/runtime methods. ModuleNativeCall dispatches through the resolved natives table.

use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::compiler::native_id::{
    CHANNEL_NEW, CHANNEL_SEND, CHANNEL_RECEIVE, CHANNEL_TRY_SEND, CHANNEL_TRY_RECEIVE,
    CHANNEL_CLOSE, CHANNEL_IS_CLOSED, CHANNEL_LENGTH, CHANNEL_CAPACITY,
};
use crate::vm::builtin::{buffer, map, mutex, set, date, regexp};
use crate::vm::object::{
    Array, Buffer, ChannelObject, DateObject, MapObject, Object, RayaString, RegExpObject,
    SetObject,
};
use crate::vm::scheduler::{Task, TaskId, TaskState};
use crate::vm::stack::Stack;
use crate::vm::sync::MutexId;
use crate::vm::value::Value;
use crate::vm::VmError;
use crate::compiler::{Module, Opcode};
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_native_ops(
        &mut self,
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        module: &Module,
        task: &Arc<Task>,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::NativeCall => {
                let native_id = match Self::read_u16(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u8(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Pop arguments
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    match stack.pop() {
                        Ok(v) => args.push(v),
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }
                args.reverse();

                // Execute native call - handle channel operations specially for suspension
                match native_id {
                    CHANNEL_NEW => {
                        // Create a new channel with given capacity
                        let capacity = args[0].as_i32().unwrap_or(0) as usize;
                        let ch = ChannelObject::new(capacity);
                        let gc_ptr = self.gc.lock().allocate(ch);
                        let handle = gc_ptr.as_ptr() as u64;
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_SEND => {
                        // args: [channel_handle, value]
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_SEND requires 2 arguments".to_string()
                            ));
                        }
                        let handle = args[0].as_u64().unwrap_or(0);
                        let value = args[1];
                        let ch_ptr = handle as *const ChannelObject;
                        if ch_ptr.is_null() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }
                        let channel = unsafe { &*ch_ptr };

                        if channel.is_closed() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Channel closed".to_string()
                            ));
                        }
                        if channel.try_send(value) {
                            if let Err(e) = stack.push(Value::null()) {
                                return OpcodeResult::Error(e);
                            }
                            OpcodeResult::Continue
                        } else {
                            use crate::vm::scheduler::SuspendReason;
                            OpcodeResult::Suspend(SuspendReason::ChannelSend {
                                channel_id: handle,
                                value,
                            })
                        }
                    }

                    CHANNEL_RECEIVE => {
                        // args: [channel_handle]
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_RECEIVE requires 1 argument".to_string()
                            ));
                        }
                        let handle = args[0].as_u64().unwrap_or(0);
                        let ch_ptr = handle as *const ChannelObject;
                        if ch_ptr.is_null() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }
                        let channel = unsafe { &*ch_ptr };

                        if let Some(val) = channel.try_receive() {
                            if let Err(e) = stack.push(val) {
                                return OpcodeResult::Error(e);
                            }
                            OpcodeResult::Continue
                        } else if channel.is_closed() {
                            if let Err(e) = stack.push(Value::null()) {
                                return OpcodeResult::Error(e);
                            }
                            OpcodeResult::Continue
                        } else {
                            use crate::vm::scheduler::SuspendReason;
                            OpcodeResult::Suspend(SuspendReason::ChannelReceive {
                                channel_id: handle,
                            })
                        }
                    }

                    CHANNEL_TRY_SEND => {
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_TRY_SEND requires 2 arguments".to_string()
                            ));
                        }
                        let handle = args[0].as_u64().unwrap_or(0);
                        let value = args[1];
                        let ch_ptr = handle as *const ChannelObject;
                        if ch_ptr.is_null() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }
                        let channel = unsafe { &*ch_ptr };
                        let result = channel.try_send(value);
                        if let Err(e) = stack.push(Value::bool(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_TRY_RECEIVE => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_TRY_RECEIVE requires 1 argument".to_string()
                            ));
                        }
                        let handle = args[0].as_u64().unwrap_or(0);
                        let ch_ptr = handle as *const ChannelObject;
                        if ch_ptr.is_null() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }
                        let channel = unsafe { &*ch_ptr };
                        let result = channel.try_receive().unwrap_or(Value::null());
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_CLOSE => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_CLOSE requires 1 argument".to_string()
                            ));
                        }
                        let handle = args[0].as_u64().unwrap_or(0);
                        let ch_ptr = handle as *const ChannelObject;
                        if ch_ptr.is_null() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }
                        let channel = unsafe { &*ch_ptr };
                        channel.close();
                        // Reactor will wake any waiting tasks on next iteration
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_IS_CLOSED => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_IS_CLOSED requires 1 argument".to_string()
                            ));
                        }
                        let handle = args[0].as_u64().unwrap_or(0);
                        let ch_ptr = handle as *const ChannelObject;
                        if ch_ptr.is_null() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }
                        let channel = unsafe { &*ch_ptr };
                        if let Err(e) = stack.push(Value::bool(channel.is_closed())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_LENGTH => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_LENGTH requires 1 argument".to_string()
                            ));
                        }
                        let handle = args[0].as_u64().unwrap_or(0);
                        let ch_ptr = handle as *const ChannelObject;
                        if ch_ptr.is_null() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }
                        let channel = unsafe { &*ch_ptr };
                        if let Err(e) = stack.push(Value::i32(channel.length() as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_CAPACITY => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_CAPACITY requires 1 argument".to_string()
                            ));
                        }
                        let handle = args[0].as_u64().unwrap_or(0);
                        let ch_ptr = handle as *const ChannelObject;
                        if ch_ptr.is_null() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected channel object".to_string()
                            ));
                        }
                        let channel = unsafe { &*ch_ptr };
                        if let Err(e) = stack.push(Value::i32(channel.capacity() as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    // Buffer native calls
                    id if id == buffer::NEW as u16 => {
                        let size = args[0].as_i32().unwrap_or(0) as usize;
                        let buf = Buffer::new(size);
                        let gc_ptr = self.gc.lock().allocate(buf);
                        let handle = gc_ptr.as_ptr() as u64;
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::LENGTH as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid buffer handle".to_string()));
                        }
                        let buf = unsafe { &*buf_ptr };
                        if let Err(e) = stack.push(Value::i32(buf.length() as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::GET_BYTE as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let index = args[1].as_i32().unwrap_or(0) as usize;
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid buffer handle".to_string()));
                        }
                        let buf = unsafe { &*buf_ptr };
                        let value = buf.get_byte(index).unwrap_or(0);
                        if let Err(e) = stack.push(Value::i32(value as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::SET_BYTE as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let index = args[1].as_i32().unwrap_or(0) as usize;
                        let value = args[2].as_i32().unwrap_or(0) as u8;
                        let buf_ptr = handle as *mut Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid buffer handle".to_string()));
                        }
                        let buf = unsafe { &mut *buf_ptr };
                        if let Err(msg) = buf.set_byte(index, value) {
                            return OpcodeResult::Error(VmError::RuntimeError(msg));
                        }
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::GET_INT32 as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let index = args[1].as_i32().unwrap_or(0) as usize;
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid buffer handle".to_string()));
                        }
                        let buf = unsafe { &*buf_ptr };
                        let value = buf.get_int32(index).unwrap_or(0);
                        if let Err(e) = stack.push(Value::i32(value)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::SET_INT32 as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let index = args[1].as_i32().unwrap_or(0) as usize;
                        let value = args[2].as_i32().unwrap_or(0);
                        let buf_ptr = handle as *mut Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid buffer handle".to_string()));
                        }
                        let buf = unsafe { &mut *buf_ptr };
                        if let Err(msg) = buf.set_int32(index, value) {
                            return OpcodeResult::Error(VmError::RuntimeError(msg));
                        }
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::GET_FLOAT64 as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let index = args[1].as_i32().unwrap_or(0) as usize;
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid buffer handle".to_string()));
                        }
                        let buf = unsafe { &*buf_ptr };
                        let value = buf.get_float64(index).unwrap_or(0.0);
                        if let Err(e) = stack.push(Value::f64(value)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::SET_FLOAT64 as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let index = args[1].as_i32().unwrap_or(0) as usize;
                        let value = args[2].as_f64().unwrap_or(0.0);
                        let buf_ptr = handle as *mut Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid buffer handle".to_string()));
                        }
                        let buf = unsafe { &mut *buf_ptr };
                        if let Err(msg) = buf.set_float64(index, value) {
                            return OpcodeResult::Error(VmError::RuntimeError(msg));
                        }
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::SLICE as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let start = args[1].as_i32().unwrap_or(0) as usize;
                        let end = args[2].as_i32().unwrap_or(0) as usize;
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid buffer handle".to_string()));
                        }
                        let buf = unsafe { &*buf_ptr };
                        let sliced = buf.slice(start, end);
                        let gc_ptr = self.gc.lock().allocate(sliced);
                        let new_handle = gc_ptr.as_ptr() as u64;
                        if let Err(e) = stack.push(Value::u64(new_handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::COPY as u16 => {
                        // copy(srcHandle, targetHandle, targetStart, sourceStart, sourceEnd)
                        let src_handle = args[0].as_u64().unwrap_or(0);
                        let tgt_handle = args[1].as_u64().unwrap_or(0);
                        let tgt_start = args[2].as_i32().unwrap_or(0) as usize;
                        let src_start = args[3].as_i32().unwrap_or(0) as usize;
                        let src_end = args[4].as_i32().unwrap_or(0) as usize;
                        let src_ptr = src_handle as *const Buffer;
                        let tgt_ptr = tgt_handle as *mut Buffer;
                        if src_ptr.is_null() || tgt_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid buffer handle".to_string()));
                        }
                        let src = unsafe { &*src_ptr };
                        let tgt = unsafe { &mut *tgt_ptr };
                        let src_end = src_end.min(src.data.len());
                        let src_start = src_start.min(src_end);
                        let bytes = &src.data[src_start..src_end];
                        let copy_len = bytes.len().min(tgt.data.len().saturating_sub(tgt_start));
                        tgt.data[tgt_start..tgt_start + copy_len].copy_from_slice(&bytes[..copy_len]);
                        if let Err(e) = stack.push(Value::i32(copy_len as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::TO_STRING as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid buffer handle".to_string()));
                        }
                        let buf = unsafe { &*buf_ptr };
                        // encoding argument (args[1]) — currently only utf8/ascii supported
                        let text = String::from_utf8_lossy(&buf.data).into_owned();
                        let s = RayaString::new(text);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        if let Err(e) = stack.push(val) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::FROM_STRING as u16 => {
                        // args[0] = string pointer, args[1] = encoding (ignored, utf8)
                        if !args[0].is_ptr() {
                            return OpcodeResult::Error(VmError::TypeError("Expected string".to_string()));
                        }
                        let str_ptr = unsafe { args[0].as_ptr::<RayaString>() };
                        let s = match str_ptr {
                            Some(p) => unsafe { &*p.as_ptr() },
                            None => return OpcodeResult::Error(VmError::TypeError("Expected string".to_string())),
                        };
                        let bytes = s.data.as_bytes();
                        let mut buf = Buffer::new(bytes.len());
                        buf.data.copy_from_slice(bytes);
                        let gc_ptr = self.gc.lock().allocate(buf);
                        let new_handle = gc_ptr.as_ptr() as u64;
                        if let Err(e) = stack.push(Value::u64(new_handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Mutex native calls
                    id if id == mutex::TRY_LOCK as u16 => {
                        let mutex_id = MutexId::from_u64(args[0].as_i64().unwrap_or(0) as u64);
                        if let Some(mutex) = self.mutex_registry.get(mutex_id) {
                            match mutex.try_lock(task.id()) {
                                Ok(()) => {
                                    task.add_held_mutex(mutex_id);
                                    if let Err(e) = stack.push(Value::bool(true)) {
                                        return OpcodeResult::Error(e);
                                    }
                                }
                                Err(_) => {
                                    if let Err(e) = stack.push(Value::bool(false)) {
                                        return OpcodeResult::Error(e);
                                    }
                                }
                            }
                        } else {
                            return OpcodeResult::Error(VmError::RuntimeError(format!(
                                "Mutex {:?} not found", mutex_id
                            )));
                        }
                        OpcodeResult::Continue
                    }
                    id if id == mutex::IS_LOCKED as u16 => {
                        let mutex_id = MutexId::from_u64(args[0].as_i64().unwrap_or(0) as u64);
                        if let Some(mutex) = self.mutex_registry.get(mutex_id) {
                            let is_locked = mutex.is_locked();
                            if let Err(e) = stack.push(Value::bool(is_locked)) {
                                return OpcodeResult::Error(e);
                            }
                        } else {
                            return OpcodeResult::Error(VmError::RuntimeError(format!(
                                "Mutex {:?} not found", mutex_id
                            )));
                        }
                        OpcodeResult::Continue
                    }
                    // Map native calls
                    id if id == map::NEW as u16 => {
                        let map = MapObject::new();
                        let gc_ptr = self.gc.lock().allocate(map);
                        let handle = gc_ptr.as_ptr() as u64;
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::SIZE as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid map handle".to_string()));
                        }
                        let map = unsafe { &*map_ptr };
                        if let Err(e) = stack.push(Value::i32(map.size() as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::GET as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let key = args[1];
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid map handle".to_string()));
                        }
                        let map = unsafe { &*map_ptr };
                        let value = map.get(key).unwrap_or(Value::null());
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::SET as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let key = args[1];
                        let value = args[2];
                        let map_ptr = handle as *mut MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid map handle".to_string()));
                        }
                        let map = unsafe { &mut *map_ptr };
                        map.set(key, value);
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::HAS as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let key = args[1];
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid map handle".to_string()));
                        }
                        let map = unsafe { &*map_ptr };
                        if let Err(e) = stack.push(Value::bool(map.has(key))) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::DELETE as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let key = args[1];
                        let map_ptr = handle as *mut MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid map handle".to_string()));
                        }
                        let map = unsafe { &mut *map_ptr };
                        let result = map.delete(key);
                        if let Err(e) = stack.push(Value::bool(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::CLEAR as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let map_ptr = handle as *mut MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid map handle".to_string()));
                        }
                        let map = unsafe { &mut *map_ptr };
                        map.clear();
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::KEYS as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid map handle".to_string()));
                        }
                        let map = unsafe { &*map_ptr };
                        let keys = map.keys();
                        let mut arr = Array::new(0, 0);
                        for key in keys {
                            arr.push(key);
                        }
                        let arr_gc = self.gc.lock().allocate(arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::VALUES as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid map handle".to_string()));
                        }
                        let map = unsafe { &*map_ptr };
                        let values = map.values();
                        let mut arr = Array::new(0, 0);
                        for val in values {
                            arr.push(val);
                        }
                        let arr_gc = self.gc.lock().allocate(arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::ENTRIES as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid map handle".to_string()));
                        }
                        let map = unsafe { &*map_ptr };
                        let entries = map.entries();
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
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Set native calls
                    id if id == set::NEW as u16 => {
                        let set_obj = SetObject::new();
                        let gc_ptr = self.gc.lock().allocate(set_obj);
                        let handle = gc_ptr.as_ptr() as u64;
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::SIZE as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let set_ptr = handle as *const SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid set handle".to_string()));
                        }
                        let set_obj = unsafe { &*set_ptr };
                        if let Err(e) = stack.push(Value::i32(set_obj.size() as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::ADD as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let value = args[1];
                        let set_ptr = handle as *mut SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid set handle".to_string()));
                        }
                        let set_obj = unsafe { &mut *set_ptr };
                        set_obj.add(value);
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::HAS as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let value = args[1];
                        let set_ptr = handle as *const SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid set handle".to_string()));
                        }
                        let set_obj = unsafe { &*set_ptr };
                        if let Err(e) = stack.push(Value::bool(set_obj.has(value))) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::DELETE as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let value = args[1];
                        let set_ptr = handle as *mut SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid set handle".to_string()));
                        }
                        let set_obj = unsafe { &mut *set_ptr };
                        let result = set_obj.delete(value);
                        if let Err(e) = stack.push(Value::bool(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::CLEAR as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let set_ptr = handle as *mut SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid set handle".to_string()));
                        }
                        let set_obj = unsafe { &mut *set_ptr };
                        set_obj.clear();
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::VALUES as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let set_ptr = handle as *const SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid set handle".to_string()));
                        }
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
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::UNION as u16 => {
                        let handle_a = args[0].as_u64().unwrap_or(0);
                        let handle_b = args[1].as_u64().unwrap_or(0);
                        let set_a_ptr = handle_a as *const SetObject;
                        let set_b_ptr = handle_b as *const SetObject;
                        if set_a_ptr.is_null() || set_b_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid set handle".to_string()));
                        }
                        let set_a = unsafe { &*set_a_ptr };
                        let set_b = unsafe { &*set_b_ptr };
                        let mut result = SetObject::new();
                        for val in set_a.values() {
                            result.add(val);
                        }
                        for val in set_b.values() {
                            result.add(val);
                        }
                        let gc_ptr = self.gc.lock().allocate(result);
                        let handle = gc_ptr.as_ptr() as u64;
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::INTERSECTION as u16 => {
                        let handle_a = args[0].as_u64().unwrap_or(0);
                        let handle_b = args[1].as_u64().unwrap_or(0);
                        let set_a_ptr = handle_a as *const SetObject;
                        let set_b_ptr = handle_b as *const SetObject;
                        if set_a_ptr.is_null() || set_b_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid set handle".to_string()));
                        }
                        let set_a = unsafe { &*set_a_ptr };
                        let set_b = unsafe { &*set_b_ptr };
                        let mut result = SetObject::new();
                        for val in set_a.values() {
                            if set_b.has(val) {
                                result.add(val);
                            }
                        }
                        let gc_ptr = self.gc.lock().allocate(result);
                        let handle = gc_ptr.as_ptr() as u64;
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::DIFFERENCE as u16 => {
                        let handle_a = args[0].as_u64().unwrap_or(0);
                        let handle_b = args[1].as_u64().unwrap_or(0);
                        let set_a_ptr = handle_a as *const SetObject;
                        let set_b_ptr = handle_b as *const SetObject;
                        if set_a_ptr.is_null() || set_b_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid set handle".to_string()));
                        }
                        let set_a = unsafe { &*set_a_ptr };
                        let set_b = unsafe { &*set_b_ptr };
                        let mut result = SetObject::new();
                        for val in set_a.values() {
                            if !set_b.has(val) {
                                result.add(val);
                            }
                        }
                        let gc_ptr = self.gc.lock().allocate(result);
                        let handle = gc_ptr.as_ptr() as u64;
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Number native calls
                    id if id == 0x0F00u16 => {
                        // NUMBER_TO_FIXED: format number with fixed decimal places
                        // args[0] = number value, args[1] = digits
                        let value = args[0].as_f64()
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0);
                        let digits = args.get(1).and_then(|v| v.as_i32()).unwrap_or(0) as usize;
                        let formatted = format!("{:.prec$}", value, prec = digits);
                        let s = RayaString::new(formatted);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        if let Err(e) = stack.push(val) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == 0x0F01u16 => {
                        // NUMBER_TO_PRECISION: format with N significant digits
                        let value = args[0].as_f64()
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0);
                        let prec = args.get(1).and_then(|v| v.as_i32()).unwrap_or(1).max(1) as usize;
                        let formatted = if value == 0.0 {
                            format!("{:.prec$}", 0.0, prec = prec - 1)
                        } else {
                            let magnitude = value.abs().log10().floor() as i32;
                            let decimal_places = if prec as i32 > magnitude + 1 {
                                (prec as i32 - magnitude - 1) as usize
                            } else {
                                0
                            };
                            format!("{:.prec$}", value, prec = decimal_places)
                        };
                        let s = RayaString::new(formatted);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        if let Err(e) = stack.push(val) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == 0x0F02u16 => {
                        // NUMBER_TO_STRING_RADIX: convert to string with radix
                        let value = args[0].as_f64()
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0);
                        let radix = args.get(1).and_then(|v| v.as_i32()).unwrap_or(10);
                        let formatted = if radix == 10 || radix < 2 || radix > 36 {
                            if value.fract() == 0.0 && value.abs() < i64::MAX as f64 {
                                format!("{}", value as i64)
                            } else {
                                format!("{}", value)
                            }
                        } else {
                            // Integer radix conversion
                            let int_val = value as i64;
                            match radix {
                                2 => format!("{:b}", int_val),
                                8 => format!("{:o}", int_val),
                                16 => format!("{:x}", int_val),
                                _ => {
                                    // General radix conversion
                                    if int_val == 0 { "0".to_string() }
                                    else {
                                        let negative = int_val < 0;
                                        let mut n = int_val.unsigned_abs();
                                        let mut digits = Vec::new();
                                        let radix = radix as u64;
                                        while n > 0 {
                                            let d = (n % radix) as u8;
                                            digits.push(if d < 10 { b'0' + d } else { b'a' + d - 10 });
                                            n /= radix;
                                        }
                                        digits.reverse();
                                        let s = String::from_utf8(digits).unwrap_or_default();
                                        if negative { format!("-{}", s) } else { s }
                                    }
                                }
                            }
                        };
                        let s = RayaString::new(formatted);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let val = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        if let Err(e) = stack.push(val) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == 0x0F03u16 => {
                        // PARSE_INT: parse string to integer
                        let result = if let Some(ptr) = unsafe { args[0].as_ptr::<RayaString>() } {
                            let s = unsafe { &*ptr.as_ptr() }.data.trim();
                            // Parse integer, handling leading whitespace and optional sign
                            s.parse::<i64>().map(|v| v as f64)
                                .or_else(|_| s.parse::<f64>().map(|v| v.trunc()))
                                .unwrap_or(f64::NAN)
                        } else if let Some(n) = args[0].as_f64() {
                            n.trunc()
                        } else if let Some(n) = args[0].as_i32() {
                            n as f64
                        } else {
                            f64::NAN
                        };
                        if result.fract() == 0.0 && result.is_finite() && result.abs() < i32::MAX as f64 {
                            if let Err(e) = stack.push(Value::i32(result as i32)) { return OpcodeResult::Error(e); }
                        } else {
                            if let Err(e) = stack.push(Value::f64(result)) { return OpcodeResult::Error(e); }
                        }
                        OpcodeResult::Continue
                    }
                    id if id == 0x0F04u16 => {
                        // PARSE_FLOAT: parse string to float
                        let result = if let Some(ptr) = unsafe { args[0].as_ptr::<RayaString>() } {
                            let s = unsafe { &*ptr.as_ptr() }.data.trim();
                            s.parse::<f64>().unwrap_or(f64::NAN)
                        } else if let Some(n) = args[0].as_f64() {
                            n
                        } else if let Some(n) = args[0].as_i32() {
                            n as f64
                        } else {
                            f64::NAN
                        };
                        if let Err(e) = stack.push(Value::f64(result)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == 0x0F05u16 => {
                        // IS_NAN: check if value is NaN
                        let is_nan = if let Some(n) = args[0].as_f64() {
                            n.is_nan()
                        } else if args[0].as_i32().is_some() {
                            false // integers are never NaN
                        } else {
                            true // non-numbers are treated as NaN
                        };
                        if let Err(e) = stack.push(Value::bool(is_nan)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == 0x0F06u16 => {
                        // IS_FINITE: check if value is finite
                        let is_finite = if let Some(n) = args[0].as_f64() {
                            n.is_finite()
                        } else if args[0].as_i32().is_some() {
                            true // integers are always finite
                        } else {
                            false
                        };
                        if let Err(e) = stack.push(Value::bool(is_finite)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    // Object native calls
                    id if id == 0x0001u16 => {
                        // OBJECT_TO_STRING: return "[object Object]"
                        let s = RayaString::new("[object Object]".to_string());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        if let Err(e) = stack.push(value) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == 0x0002u16 => {
                        // OBJECT_HASH_CODE: return identity hash from object pointer
                        let hash = if !args.is_empty() {
                            // Use the raw bits of the value as a hash
                            let bits = args[0].as_u64().unwrap_or(0);
                            (bits ^ (bits >> 16)) as i32
                        } else {
                            0
                        };
                        if let Err(e) = stack.push(Value::i32(hash)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == 0x0003u16 => {
                        // OBJECT_EQUAL: reference equality
                        let equal = if args.len() >= 2 {
                            args[0].as_u64() == args[1].as_u64()
                        } else {
                            false
                        };
                        if let Err(e) = stack.push(Value::bool(equal)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    // Task native calls
                    id if id == 0x0500u16 => {
                        // TASK_IS_DONE: check if task completed
                        let task_id = TaskId::from_u64(args[0].as_u64().unwrap_or(0));
                        let tasks = self.tasks.read();
                        let is_done = tasks.get(&task_id)
                            .map(|t| matches!(t.state(), TaskState::Completed | TaskState::Failed))
                            .unwrap_or(true);
                        if let Err(e) = stack.push(Value::bool(is_done)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == 0x0501u16 => {
                        // TASK_IS_CANCELLED: check if task cancelled
                        let task_id = TaskId::from_u64(args[0].as_u64().unwrap_or(0));
                        let tasks = self.tasks.read();
                        let is_cancelled = tasks.get(&task_id)
                            .map(|t| t.is_cancelled())
                            .unwrap_or(false);
                        if let Err(e) = stack.push(Value::bool(is_cancelled)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    // Error native calls
                    id if id == 0x0600u16 => {
                        // ERROR_STACK (0x0600): return stack trace from error object.
                        // Stack traces are populated at throw time in exceptions.rs
                        // (task.build_stack_trace → obj.fields[2]).
                        // Normal e.stack access uses LoadField directly; this native
                        // handler serves as a fallback if called explicitly.
                        let result = if !args.is_empty() {
                            let error_val = args[0];
                            if let Some(obj_ptr) = unsafe { error_val.as_ptr::<Object>() } {
                                let obj = unsafe { &*obj_ptr.as_ptr() };
                                if obj.fields.len() > 2 {
                                    obj.fields[2]
                                } else {
                                    let s = RayaString::new(String::new());
                                    let gc_ptr = self.gc.lock().allocate(s);
                                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
                                }
                            } else {
                                let s = RayaString::new(String::new());
                                let gc_ptr = self.gc.lock().allocate(s);
                                unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
                            }
                        } else {
                            let s = RayaString::new(String::new());
                            let gc_ptr = self.gc.lock().allocate(s);
                            unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
                        };
                        if let Err(e) = stack.push(result) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    // Date native calls
                    id if id == date::NOW as u16 => {
                        use std::time::{SystemTime, UNIX_EPOCH};
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_millis() as f64)
                            .unwrap_or(0.0);
                        if let Err(e) = stack.push(Value::f64(now)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_FULL_YEAR as u16 => {
                        // args[0] is the timestamp in milliseconds (as f64 number)
                        let timestamp = args[0].as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_full_year())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_MONTH as u16 => {
                        let timestamp = args[0].as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_month())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_DATE as u16 => {
                        let timestamp = args[0].as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_date())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_DAY as u16 => {
                        let timestamp = args[0].as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_day())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_HOURS as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_hours())) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_MINUTES as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_minutes())) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_SECONDS as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_seconds())) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_MILLISECONDS as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_milliseconds())) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    // Date setters: args[0]=timestamp, args[1]=new value, returns new timestamp as f64
                    id if id == date::SET_FULL_YEAR as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(0);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_full_year(val) as f64)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_MONTH as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(0);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_month(val) as f64)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_DATE as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(1);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_date(val) as f64)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_HOURS as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(0);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_hours(val) as f64)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_MINUTES as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(0);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_minutes(val) as f64)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_SECONDS as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(0);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_seconds(val) as f64)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_MILLISECONDS as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let val = args[1].as_i32().unwrap_or(0);
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_milliseconds(val) as f64)) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    // Date string formatting: args[0]=timestamp, returns string
                    id if id == date::TO_STRING as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        let s = RayaString::new(date.to_string_repr());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        if let Err(e) = stack.push(value) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::TO_ISO_STRING as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        let s = RayaString::new(date.to_iso_string());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        if let Err(e) = stack.push(value) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::TO_DATE_STRING as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        let s = RayaString::new(date.to_date_string());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        if let Err(e) = stack.push(value) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    id if id == date::TO_TIME_STRING as u16 => {
                        let timestamp = args[0].as_f64().or_else(|| args[0].as_i64().map(|v| v as f64)).or_else(|| args[0].as_i32().map(|v| v as f64)).unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        let s = RayaString::new(date.to_time_string());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                        if let Err(e) = stack.push(value) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    // Date.parse: args[0]=string, returns timestamp f64 (NaN on failure)
                    id if id == date::PARSE as u16 => {
                        let input = if !args.is_empty() && args[0].is_ptr() {
                            if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else { String::new() }
                        } else { String::new() };
                        let result = match DateObject::parse(&input) {
                            Some(ts) => Value::f64(ts as f64),
                            None => Value::f64(f64::NAN),
                        };
                        if let Err(e) = stack.push(result) { return OpcodeResult::Error(e); }
                        OpcodeResult::Continue
                    }
                    // RegExp native calls
                    id if id == regexp::NEW as u16 => {
                        let pattern = if args[0].is_ptr() {
                            if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let flags = if args.len() > 1 && args[1].is_ptr() {
                            if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        match RegExpObject::new(&pattern, &flags) {
                            Ok(re) => {
                                let gc_ptr = self.gc.lock().allocate(re);
                                let handle = gc_ptr.as_ptr() as u64;
                                if let Err(e) = stack.push(Value::u64(handle)) {
                                    return OpcodeResult::Error(e);
                                }
                                OpcodeResult::Continue
                            }
                            Err(e) => {
                                return OpcodeResult::Error(VmError::RuntimeError(format!("Invalid regex: {}", e)));
                            }
                        }
                    }
                    id if id == regexp::TEST as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let input = if args[1].is_ptr() {
                            if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid regexp handle".to_string()));
                        }
                        let re = unsafe { &*re_ptr };
                        if let Err(e) = stack.push(Value::bool(re.test(&input))) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::EXEC as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let input = if args[1].is_ptr() {
                            if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid regexp handle".to_string()));
                        }
                        let re = unsafe { &*re_ptr };
                        match re.exec(&input) {
                            Some((matched, index, groups)) => {
                                let mut arr = Array::new(0, 0);
                                let matched_str = RayaString::new(matched);
                                let gc_ptr = self.gc.lock().allocate(matched_str);
                                let matched_val = unsafe {
                                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                                };
                                arr.push(matched_val);
                                arr.push(Value::i32(index as i32));
                                for group in groups {
                                    let group_str = RayaString::new(group);
                                    let gc_ptr = self.gc.lock().allocate(group_str);
                                    let group_val = unsafe {
                                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                                    };
                                    arr.push(group_val);
                                }
                                let arr_gc = self.gc.lock().allocate(arr);
                                let arr_val = unsafe {
                                    Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                                };
                                if let Err(e) = stack.push(arr_val) {
                                    return OpcodeResult::Error(e);
                                }
                            }
                            None => {
                                if let Err(e) = stack.push(Value::null()) {
                                    return OpcodeResult::Error(e);
                                }
                            }
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::EXEC_ALL as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let input = if args[1].is_ptr() {
                            if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid regexp handle".to_string()));
                        }
                        let re = unsafe { &*re_ptr };
                        let matches = re.exec_all(&input);
                        let mut result_arr = Array::new(0, 0);
                        for (matched, index, groups) in matches {
                            let mut match_arr = Array::new(0, 0);
                            let matched_str = RayaString::new(matched);
                            let gc_ptr = self.gc.lock().allocate(matched_str);
                            let matched_val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            };
                            match_arr.push(matched_val);
                            match_arr.push(Value::i32(index as i32));
                            for group in groups {
                                let group_str = RayaString::new(group);
                                let gc_ptr = self.gc.lock().allocate(group_str);
                                let group_val = unsafe {
                                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                                };
                                match_arr.push(group_val);
                            }
                            let match_arr_gc = self.gc.lock().allocate(match_arr);
                            let match_arr_val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(match_arr_gc.as_ptr()).unwrap())
                            };
                            result_arr.push(match_arr_val);
                        }
                        let arr_gc = self.gc.lock().allocate(result_arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::REPLACE as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let input = if args[1].is_ptr() {
                            if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let replacement = if args[2].is_ptr() {
                            if let Some(s) = unsafe { args[2].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid regexp handle".to_string()));
                        }
                        let re = unsafe { &*re_ptr };
                        let result = re.replace(&input, &replacement);
                        let result_str = RayaString::new(result);
                        let gc_ptr = self.gc.lock().allocate(result_str);
                        let result_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(result_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::SPLIT as u16 => {
                        let handle = args[0].as_u64().unwrap_or(0);
                        let input = if args[1].is_ptr() {
                            if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let limit = if args.len() > 2 {
                            args[2].as_i32().map(|v| v as usize)
                        } else {
                            None
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid regexp handle".to_string()));
                        }
                        let re = unsafe { &*re_ptr };
                        let parts = re.split(&input, limit);
                        let mut arr = Array::new(0, 0);
                        for part in parts {
                            let s = RayaString::new(part);
                            let gc_ptr = self.gc.lock().allocate(s);
                            let val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            };
                            arr.push(val);
                        }
                        let arr_gc = self.gc.lock().allocate(arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::REPLACE_MATCHES as u16 => {
                        // REGEXP_REPLACE_MATCHES: Get match data for replaceWith intrinsic
                        // Args: regexp handle, input string
                        // Returns: array of [matched_text, start_index] arrays, respecting 'g' flag
                        let handle = args[0].as_u64().unwrap_or(0);
                        let input = if args[1].is_ptr() {
                            if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError("Invalid regexp handle".to_string()));
                        }
                        let re = unsafe { &*re_ptr };
                        let is_global = re.flags.contains('g');
                        let mut result_arr = Array::new(0, 0);
                        if is_global {
                            for m in re.compiled.find_iter(&input) {
                                let mut match_arr = Array::new(0, 0);
                                let match_str = RayaString::new(m.as_str().to_string());
                                let gc_ptr = self.gc.lock().allocate(match_str);
                                let match_val = unsafe {
                                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                                };
                                match_arr.push(match_val);
                                match_arr.push(Value::i32(m.start() as i32));
                                let match_arr_gc = self.gc.lock().allocate(match_arr);
                                let match_arr_val = unsafe {
                                    Value::from_ptr(std::ptr::NonNull::new(match_arr_gc.as_ptr()).unwrap())
                                };
                                result_arr.push(match_arr_val);
                            }
                        } else {
                            if let Some(m) = re.compiled.find(&input) {
                                let mut match_arr = Array::new(0, 0);
                                let match_str = RayaString::new(m.as_str().to_string());
                                let gc_ptr = self.gc.lock().allocate(match_str);
                                let match_val = unsafe {
                                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                                };
                                match_arr.push(match_val);
                                match_arr.push(Value::i32(m.start() as i32));
                                let match_arr_gc = self.gc.lock().allocate(match_arr);
                                let match_arr_val = unsafe {
                                    Value::from_ptr(std::ptr::NonNull::new(match_arr_gc.as_ptr()).unwrap())
                                };
                                result_arr.push(match_arr_val);
                            }
                        }
                        let arr_gc = self.gc.lock().allocate(result_arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // JSON.stringify
                    0x0C00 => {
                        use crate::vm::json;

                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "JSON.stringify requires 1 argument".to_string()
                            ));
                        }
                        let value = args[0];

                        // Convert Value to JsonValue
                        let json_value = json::value_to_json(value, &mut self.gc.lock());

                        // Stringify the JsonValue
                        match json::stringify::stringify(&json_value) {
                            Ok(json_str) => {
                                let result_str = RayaString::new(json_str);
                                let gc_ptr = self.gc.lock().allocate(result_str);
                                let result_val = unsafe {
                                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                                };
                                if let Err(e) = stack.push(result_val) {
                                    return OpcodeResult::Error(e);
                                }
                            }
                            Err(e) => {
                                return OpcodeResult::Error(e);
                            }
                        }
                        OpcodeResult::Continue
                    }

                    // JSON.parse
                    0x0C01 => {
                        use crate::vm::json;

                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "JSON.parse requires 1 argument".to_string()
                            ));
                        }
                        let json_str = if args[0].is_ptr() {
                            if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "JSON.parse requires a string argument".to_string()
                                ));
                            }
                        } else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "JSON.parse requires a string argument".to_string()
                            ));
                        };

                        // Parse the JSON string (lock scope ends before json_to_value)
                        let json_value = {
                            let mut gc = self.gc.lock();
                            match json::parser::parse(&json_str, &mut gc) {
                                Ok(v) => v,
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        };

                        // Convert JsonValue to Value (separate lock scope)
                        let result = json::json_to_value(&json_value, &mut self.gc.lock());
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    // JSON.decode<T> - typed decode with field metadata
                    // Args: [json_string, field_count, ...field_keys]
                    0x0C02 => {
                        use crate::vm::json;

                        if args.len() < 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "JSON.decode requires at least 2 arguments".to_string()
                            ));
                        }

                        // Get JSON string
                        let json_str = if args[0].is_ptr() {
                            if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "JSON.decode requires a string argument".to_string()
                                ));
                            }
                        } else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "JSON.decode requires a string argument".to_string()
                            ));
                        };

                        // Get field count
                        let field_count = if let Some(n) = args[1].as_i32() {
                            n as usize
                        } else if let Some(n) = args[1].as_f64() {
                            n as usize
                        } else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "JSON.decode field count must be a number".to_string()
                            ));
                        };

                        // Collect field keys
                        let mut field_keys: Vec<String> = Vec::with_capacity(field_count);
                        for i in 0..field_count {
                            if args.len() <= 2 + i {
                                break;
                            }
                            if args[2 + i].is_ptr() {
                                if let Some(s) = unsafe { args[2 + i].as_ptr::<RayaString>() } {
                                    field_keys.push(unsafe { &*s.as_ptr() }.data.clone());
                                }
                            }
                        }

                        // Parse the JSON string
                        let json_value = {
                            let mut gc = self.gc.lock();
                            match json::parser::parse(&json_str, &mut gc) {
                                Ok(v) => v,
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        };

                        // Create a new object with the specified fields
                        let mut gc = self.gc.lock();
                        let mut obj = Object::new(0, field_keys.len()); // class_id 0 for anonymous

                        // Extract each field from the JSON and store in object
                        for (index, key) in field_keys.iter().enumerate() {
                            let field_value = json_value.get_property(key);
                            let vm_value = json::json_to_value(&field_value, &mut gc);
                            let _ = obj.set_field(index, vm_value);
                        }

                        // Allocate and return the object
                        let obj_ptr = gc.allocate(obj);
                        let result = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap())
                        };
                        drop(gc); // Release lock before push

                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    // JSON.merge(dest, source) - copy all properties from source to dest
                    0x0C03 => {
                        use crate::vm::json::JsonValue;

                        if args.len() < 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "JSON.merge requires 2 arguments (dest, source)".to_string()
                            ));
                        }
                        let dest_val = args[0];
                        let source_val = args[1];

                        // If source is null/non-object, just push dest unchanged
                        if !source_val.is_ptr() {
                            if let Err(e) = stack.push(dest_val) {
                                return OpcodeResult::Error(e);
                            }
                            return OpcodeResult::Continue;
                        }

                        // Get source as JsonValue object
                        let source_ptr = unsafe { source_val.as_ptr::<JsonValue>() };
                        if let Some(source_json_ptr) = source_ptr {
                            let source_json = unsafe { &*source_json_ptr.as_ptr() };
                            if let Some(source_obj_ptr) = source_json.as_object() {
                                // Get dest as JsonValue object
                                if dest_val.is_ptr() {
                                    let dest_ptr = unsafe { dest_val.as_ptr::<JsonValue>() };
                                    if let Some(dest_json_ptr) = dest_ptr {
                                        let dest_json = unsafe { &*dest_json_ptr.as_ptr() };
                                        if let Some(dest_obj_ptr) = dest_json.as_object() {
                                            let source_map = unsafe { &*source_obj_ptr.as_ptr() };
                                            let dest_map = unsafe { &mut *dest_obj_ptr.as_ptr() };
                                            // Copy all properties from source to dest
                                            for (key, value) in source_map.iter() {
                                                dest_map.insert(key.clone(), value.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Push dest back (it's been mutated in place)
                        if let Err(e) = stack.push(dest_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    _ => {
                        // Check if this is a reflect method - pass args directly (don't push/pop)
                        if crate::vm::builtin::is_reflect_method(native_id) {
                            match self.call_reflect_method(task, stack, native_id, args, module) {
                                Ok(()) => return OpcodeResult::Continue,
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        }

                        // Check if this is a runtime method (std:runtime)
                        if crate::vm::builtin::is_runtime_method(native_id) {
                            match self.call_runtime_method(task, stack, native_id, args, module) {
                                Ok(()) => return OpcodeResult::Continue,
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        }


                        // Other native calls not yet implemented
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "NativeCall {:#06x} not yet implemented in Interpreter (args={})",
                            native_id, args.len()
                        )));
                    }
                }
            }

            Opcode::ModuleNativeCall => {
                use crate::vm::abi::{EngineContext, value_to_native, native_to_value};
                use raya_sdk::NativeCallResult;

                let local_idx = match Self::read_u16(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u8(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Pop arguments
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    match stack.pop() {
                        Ok(v) => args.push(v),
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }
                args.reverse();

                // Create EngineContext for handler
                let ctx = EngineContext::new(
                    self.gc,
                    self.classes,
                    task.id(),
                    self.class_metadata,
                );

                // Convert arguments to NativeValue (zero-cost)
                let native_args: Vec<raya_sdk::NativeValue> = args.iter()
                    .map(|v| value_to_native(*v))
                    .collect();

                // Dispatch via resolved natives table (read lock - uncontended, nearly free)
                let resolved = self.resolved_natives.read();
                match resolved.call(local_idx, &ctx, &native_args) {
                    NativeCallResult::Value(val) => {
                        if let Err(e) = stack.push(native_to_value(val)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    NativeCallResult::Suspend(io_request) => {
                        use crate::vm::scheduler::{IoSubmission, SuspendReason};
                        if let Some(tx) = self.io_submit_tx {
                            let _ = tx.send(IoSubmission {
                                task_id: task.id(),
                                request: io_request,
                            });
                        }
                        return OpcodeResult::Suspend(SuspendReason::IoWait);
                    }
                    NativeCallResult::Unhandled => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "ModuleNativeCall index {} unhandled",
                            local_idx
                        )));
                    }
                    NativeCallResult::Error(msg) => {
                        return OpcodeResult::Error(VmError::RuntimeError(msg));
                    }
                }
            }

            _ => OpcodeResult::Error(VmError::RuntimeError(format!(
                "Unexpected opcode in exec_native_ops: {:?}",
                opcode
            ))),
        }
    }
}
