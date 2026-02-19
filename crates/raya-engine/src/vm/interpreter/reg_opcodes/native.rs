//! Register-based native call opcode handlers
//!
//! NativeCall bridges to the same native dispatch as the stack-based interpreter.
//! ModuleNativeCall dispatches through the resolved natives table.

use crate::compiler::bytecode::reg_opcode::{RegInstr, RegOpcode};
use crate::compiler::Module;
use crate::vm::interpreter::reg_execution::RegOpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::register_file::RegisterFile;
use crate::vm::scheduler::Task;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_reg_native_ops(
        &mut self,
        task: &Arc<Task>,
        regs: &mut RegisterFile,
        reg_base: usize,
        instr: RegInstr,
        extra: u32,
        module: &Module,
    ) -> RegOpcodeResult {
        let opcode = match instr.opcode() {
            Some(op) => op,
            None => return RegOpcodeResult::error(VmError::InvalidOpcode(instr.opcode_byte())),
        };

        match opcode {
            RegOpcode::NativeCall => {
                // rA = native_id(rB..rB+C-1); extra = native_id
                let dest_reg = instr.a();
                let arg_base = instr.b();
                let arg_count = instr.c() as usize;
                let native_id = extra as u16;

                // Collect arguments from registers
                let mut args = Vec::with_capacity(arg_count);
                for i in 0..arg_count {
                    let val = match regs.get_reg(reg_base, arg_base.wrapping_add(i as u8)) {
                        Ok(v) => v,
                        Err(e) => return RegOpcodeResult::Error(e),
                    };
                    args.push(val);
                }

                // Dispatch native call and get result
                match self.dispatch_reg_native(native_id, &args, task, module) {
                    RegNativeResult::Value(v) => {
                        if let Err(e) = regs.set_reg(reg_base, dest_reg, v) {
                            return RegOpcodeResult::Error(e);
                        }
                        RegOpcodeResult::Continue
                    }
                    RegNativeResult::Suspend(reason) => {
                        task.set_resume_reg_dest(dest_reg);
                        RegOpcodeResult::Suspend(reason)
                    }
                    RegNativeResult::Error(e) => RegOpcodeResult::Error(e),
                }
            }

            RegOpcode::ModuleNativeCall => {
                // rA = module_native(rB..rB+C-1); extra = local_idx
                use crate::vm::abi::{value_to_native, native_to_value, EngineContext};
                use raya_sdk::NativeCallResult;

                let dest_reg = instr.a();
                let arg_base = instr.b();
                let arg_count = instr.c() as usize;
                let local_idx = extra as u16;

                // Collect arguments from registers
                let mut args = Vec::with_capacity(arg_count);
                for i in 0..arg_count {
                    let val = match regs.get_reg(reg_base, arg_base.wrapping_add(i as u8)) {
                        Ok(v) => v,
                        Err(e) => return RegOpcodeResult::Error(e),
                    };
                    args.push(val);
                }

                // Create EngineContext for handler
                let ctx = EngineContext::new(
                    self.gc,
                    self.classes,
                    task.id(),
                    self.class_metadata,
                );

                // Convert arguments to NativeValue
                let native_args: Vec<raya_sdk::NativeValue> = args.iter()
                    .map(|v| value_to_native(*v))
                    .collect();

                // Dispatch via resolved natives table
                let resolved = self.resolved_natives.read();
                match resolved.call(local_idx, &ctx, &native_args) {
                    NativeCallResult::Value(val) => {
                        if let Err(e) = regs.set_reg(reg_base, dest_reg, native_to_value(val)) {
                            return RegOpcodeResult::Error(e);
                        }
                        RegOpcodeResult::Continue
                    }
                    NativeCallResult::Suspend(io_request) => {
                        use crate::vm::scheduler::{IoSubmission, SuspendReason};
                        if let Some(tx) = self.io_submit_tx {
                            let _ = tx.send(IoSubmission {
                                task_id: task.id(),
                                request: io_request,
                            });
                        }
                        task.set_resume_reg_dest(dest_reg);
                        RegOpcodeResult::Suspend(SuspendReason::IoWait)
                    }
                    NativeCallResult::Unhandled => {
                        RegOpcodeResult::runtime_error(format!(
                            "ModuleNativeCall index {} unhandled",
                            local_idx
                        ))
                    }
                    NativeCallResult::Error(msg) => {
                        RegOpcodeResult::Error(VmError::RuntimeError(msg))
                    }
                }
            }

            RegOpcode::Trap => {
                // Trap with error code; Bx = error_code (ABx format)
                let error_code = instr.bx();
                RegOpcodeResult::Error(VmError::RuntimeError(format!(
                    "Trap: error code {}",
                    error_code
                )))
            }

            _ => RegOpcodeResult::runtime_error(format!(
                "Not a native opcode: {:?}",
                opcode
            )),
        }
    }

    /// Dispatch a native call by ID, returning the result value.
    ///
    /// This bridges register-based native calls to the existing native dispatch.
    /// Common operations (channels, buffers, etc.) are handled inline.
    fn dispatch_reg_native(
        &mut self,
        native_id: u16,
        args: &[Value],
        task: &Arc<Task>,
        _module: &Module,
    ) -> RegNativeResult {
        use crate::compiler::native_id::*;
        use crate::vm::object::{ChannelObject, Buffer};
        use crate::vm::builtin::{buffer, map, set, date, regexp};
        use crate::vm::scheduler::SuspendReason;

        match native_id {
            // ============ Channel Operations ============
            CHANNEL_NEW => {
                let capacity = args.get(0).and_then(|v| v.as_i32()).unwrap_or(0) as usize;
                let ch = ChannelObject::new(capacity);
                let gc_ptr = self.gc.lock().allocate(ch);
                let handle = gc_ptr.as_ptr() as u64;
                RegNativeResult::Value(Value::u64(handle))
            }

            CHANNEL_SEND => {
                if args.len() != 2 {
                    return RegNativeResult::error("CHANNEL_SEND requires 2 arguments");
                }
                let handle = args[0].as_u64().unwrap_or(0);
                let value = args[1];
                let ch_ptr = handle as *const ChannelObject;
                if ch_ptr.is_null() {
                    return RegNativeResult::error("Expected channel object");
                }
                let channel = unsafe { &*ch_ptr };
                if channel.is_closed() {
                    return RegNativeResult::error("Channel closed");
                }
                if channel.try_send(value) {
                    RegNativeResult::Value(Value::null())
                } else {
                    RegNativeResult::Suspend(SuspendReason::ChannelSend {
                        channel_id: handle,
                        value,
                    })
                }
            }

            CHANNEL_RECEIVE => {
                if args.len() != 1 {
                    return RegNativeResult::error("CHANNEL_RECEIVE requires 1 argument");
                }
                let handle = args[0].as_u64().unwrap_or(0);
                let ch_ptr = handle as *const ChannelObject;
                if ch_ptr.is_null() {
                    return RegNativeResult::error("Expected channel object");
                }
                let channel = unsafe { &*ch_ptr };
                if let Some(val) = channel.try_receive() {
                    RegNativeResult::Value(val)
                } else if channel.is_closed() {
                    RegNativeResult::Value(Value::null())
                } else {
                    RegNativeResult::Suspend(SuspendReason::ChannelReceive {
                        channel_id: handle,
                    })
                }
            }

            CHANNEL_TRY_SEND => {
                let handle = args.get(0).and_then(|v| v.as_u64()).unwrap_or(0);
                let value = args.get(1).copied().unwrap_or(Value::null());
                let ch_ptr = handle as *const ChannelObject;
                if ch_ptr.is_null() {
                    return RegNativeResult::error("Expected channel object");
                }
                let channel = unsafe { &*ch_ptr };
                RegNativeResult::Value(Value::bool(channel.try_send(value)))
            }

            CHANNEL_TRY_RECEIVE => {
                let handle = args.get(0).and_then(|v| v.as_u64()).unwrap_or(0);
                let ch_ptr = handle as *const ChannelObject;
                if ch_ptr.is_null() {
                    return RegNativeResult::error("Expected channel object");
                }
                let channel = unsafe { &*ch_ptr };
                RegNativeResult::Value(channel.try_receive().unwrap_or(Value::null()))
            }

            CHANNEL_CLOSE => {
                let handle = args.get(0).and_then(|v| v.as_u64()).unwrap_or(0);
                let ch_ptr = handle as *const ChannelObject;
                if !ch_ptr.is_null() {
                    let channel = unsafe { &*ch_ptr };
                    channel.close();
                }
                RegNativeResult::Value(Value::null())
            }

            CHANNEL_IS_CLOSED => {
                let handle = args.get(0).and_then(|v| v.as_u64()).unwrap_or(0);
                let ch_ptr = handle as *const ChannelObject;
                if ch_ptr.is_null() {
                    return RegNativeResult::Value(Value::bool(true));
                }
                let channel = unsafe { &*ch_ptr };
                RegNativeResult::Value(Value::bool(channel.is_closed()))
            }

            CHANNEL_LENGTH => {
                let handle = args.get(0).and_then(|v| v.as_u64()).unwrap_or(0);
                let ch_ptr = handle as *const ChannelObject;
                if ch_ptr.is_null() {
                    return RegNativeResult::Value(Value::i32(0));
                }
                let channel = unsafe { &*ch_ptr };
                RegNativeResult::Value(Value::i32(channel.length() as i32))
            }

            CHANNEL_CAPACITY => {
                let handle = args.get(0).and_then(|v| v.as_u64()).unwrap_or(0);
                let ch_ptr = handle as *const ChannelObject;
                if ch_ptr.is_null() {
                    return RegNativeResult::Value(Value::i32(0));
                }
                let channel = unsafe { &*ch_ptr };
                RegNativeResult::Value(Value::i32(channel.capacity() as i32))
            }

            // ============ Buffer Operations ============
            id if id == buffer::NEW as u16 => {
                let size = args.get(0).and_then(|v| v.as_i32()).unwrap_or(0) as usize;
                let buf = Buffer::new(size);
                let gc_ptr = self.gc.lock().allocate(buf);
                let handle = gc_ptr.as_ptr() as u64;
                RegNativeResult::Value(Value::u64(handle))
            }

            id if id == buffer::LENGTH as u16 => {
                let handle = args.get(0).and_then(|v| v.as_u64()).unwrap_or(0);
                let buf_ptr = handle as *const Buffer;
                if buf_ptr.is_null() {
                    return RegNativeResult::Value(Value::i32(0));
                }
                let buf = unsafe { &*buf_ptr };
                RegNativeResult::Value(Value::i32(buf.length() as i32))
            }

            id if id == buffer::GET_BYTE as u16 => {
                let handle = args.get(0).and_then(|v| v.as_u64()).unwrap_or(0);
                let index = args.get(1).and_then(|v| v.as_i32()).unwrap_or(0) as usize;
                let buf_ptr = handle as *const Buffer;
                if buf_ptr.is_null() {
                    return RegNativeResult::Value(Value::i32(0));
                }
                let buf = unsafe { &*buf_ptr };
                RegNativeResult::Value(Value::i32(buf.get_byte(index).unwrap_or(0) as i32))
            }

            id if id == buffer::SET_BYTE as u16 => {
                let handle = args.get(0).and_then(|v| v.as_u64()).unwrap_or(0);
                let index = args.get(1).and_then(|v| v.as_i32()).unwrap_or(0) as usize;
                let value = args.get(2).and_then(|v| v.as_i32()).unwrap_or(0) as u8;
                let buf_ptr = handle as *mut Buffer;
                if buf_ptr.is_null() {
                    return RegNativeResult::error("Invalid buffer handle");
                }
                let buf = unsafe { &mut *buf_ptr };
                buf.set_byte(index, value);
                RegNativeResult::Value(Value::null())
            }

            // ============ Reflect / Runtime ============
            id if crate::vm::builtin::is_reflect_method(id) => {
                // Reflect methods need stack-based dispatch (complex internal API)
                // For now, return error in register mode
                RegNativeResult::error(format!(
                    "Reflect native {:#06x} not yet available in register mode",
                    id
                ))
            }

            id if crate::vm::builtin::is_runtime_method(id) => {
                RegNativeResult::error(format!(
                    "Runtime native {:#06x} not yet available in register mode",
                    id
                ))
            }

            // ============ Fallback ============
            _ => {
                RegNativeResult::error(format!(
                    "NativeCall {:#06x} not yet implemented in register mode (args={})",
                    native_id, args.len()
                ))
            }
        }
    }
}

/// Result from a native call dispatch
enum RegNativeResult {
    /// Successful result value
    Value(Value),
    /// Suspend the task
    Suspend(crate::vm::scheduler::SuspendReason),
    /// Error
    Error(VmError),
}

impl RegNativeResult {
    fn error(msg: impl Into<String>) -> Self {
        RegNativeResult::Error(VmError::RuntimeError(msg.into()))
    }
}
