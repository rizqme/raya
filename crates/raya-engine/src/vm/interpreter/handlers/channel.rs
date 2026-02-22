//! Channel built-in method handlers
//!
//! Unlike other handlers, channel returns `OpcodeResult` directly because
//! `send()` and `receive()` can suspend the current task.

use crate::compiler::Module;
use crate::vm::builtin::channel;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::ChannelObject;
use crate::vm::scheduler::{SuspendReason, Task};
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    /// Handle built-in channel methods dispatched via CallMethod.
    ///
    /// Returns `OpcodeResult` directly (not `Result<(), VmError>`) because
    /// `send()` and `receive()` may suspend the task.
    pub(in crate::vm::interpreter) fn call_channel_method(
        &mut self,
        _task: &Arc<Task>,
        stack: &mut Stack,
        method_id: u16,
        arg_count: usize,
        _module: &Module,
    ) -> OpcodeResult {
        // Pop arguments in reverse order
        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            match stack.pop() {
                Ok(v) => args.push(v),
                Err(e) => return OpcodeResult::Error(e),
            }
        }
        args.reverse();

        // Pop receiver (channel handle)
        let receiver = match stack.pop() {
            Ok(v) => v,
            Err(e) => return OpcodeResult::Error(e),
        };
        let handle = receiver.as_u64().unwrap_or(0);
        let ch_ptr = handle as *const ChannelObject;
        if ch_ptr.is_null() {
            return OpcodeResult::Error(VmError::RuntimeError(
                "Invalid channel handle".to_string(),
            ));
        }

        match method_id {
            channel::SEND => {
                if args.is_empty() {
                    return OpcodeResult::Error(VmError::RuntimeError(
                        "Channel.send() requires 1 argument".to_string(),
                    ));
                }
                let value = args[0];
                let channel = unsafe { &*ch_ptr };

                if channel.is_closed() {
                    return OpcodeResult::Error(VmError::RuntimeError(
                        "Channel closed".to_string(),
                    ));
                }
                if channel.try_send(value) {
                    if let Err(e) = stack.push(Value::null()) {
                        return OpcodeResult::Error(e);
                    }
                    OpcodeResult::Continue
                } else {
                    OpcodeResult::Suspend(SuspendReason::ChannelSend {
                        channel_id: handle,
                        value,
                    })
                }
            }

            channel::RECEIVE => {
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
                    OpcodeResult::Suspend(SuspendReason::ChannelReceive {
                        channel_id: handle,
                    })
                }
            }

            channel::TRY_SEND => {
                if args.is_empty() {
                    return OpcodeResult::Error(VmError::RuntimeError(
                        "Channel.trySend() requires 1 argument".to_string(),
                    ));
                }
                let value = args[0];
                let channel = unsafe { &*ch_ptr };
                let result = channel.try_send(value);
                if let Err(e) = stack.push(Value::bool(result)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            channel::TRY_RECEIVE => {
                let channel = unsafe { &*ch_ptr };
                let result = channel.try_receive().unwrap_or(Value::null());
                if let Err(e) = stack.push(result) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            channel::CLOSE => {
                let channel = unsafe { &*ch_ptr };
                channel.close();
                if let Err(e) = stack.push(Value::null()) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            channel::IS_CLOSED => {
                let channel = unsafe { &*ch_ptr };
                if let Err(e) = stack.push(Value::bool(channel.is_closed())) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            channel::LENGTH => {
                let channel = unsafe { &*ch_ptr };
                if let Err(e) = stack.push(Value::i32(channel.length() as i32)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            channel::CAPACITY => {
                let channel = unsafe { &*ch_ptr };
                if let Err(e) = stack.push(Value::i32(channel.capacity() as i32)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            _ => OpcodeResult::Error(VmError::RuntimeError(format!(
                "Unknown channel method ID: 0x{:04X}",
                method_id
            ))),
        }
    }
}
