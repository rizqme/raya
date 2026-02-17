//! Control flow opcode handlers: Jmp, JmpIfTrue, JmpIfFalse, JmpIfNull, JmpIfNotNull, Return, ReturnVoid

use crate::compiler::Opcode;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::stack::Stack;
use crate::vm::value::Value;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_control_flow_ops(
        &mut self,
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::Jmp => {
                let offset = match Self::read_i16(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if offset < 0 {
                    self.safepoint.poll();
                }
                *ip = (*ip as isize + offset as isize) as usize;
                OpcodeResult::Continue
            }

            Opcode::JmpIfTrue => {
                let offset = match Self::read_i16(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let cond = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if cond.is_truthy() {
                    *ip = (*ip as isize + offset as isize) as usize;
                }
                OpcodeResult::Continue
            }

            Opcode::JmpIfFalse => {
                let offset = match Self::read_i16(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let cond = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if !cond.is_truthy() {
                    *ip = (*ip as isize + offset as isize) as usize;
                }
                OpcodeResult::Continue
            }

            Opcode::JmpIfNull => {
                let offset = match Self::read_i16(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if value.is_null() {
                    *ip = (*ip as isize + offset as isize) as usize;
                }
                OpcodeResult::Continue
            }

            Opcode::JmpIfNotNull => {
                let offset = match Self::read_i16(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if !value.is_null() {
                    *ip = (*ip as isize + offset as isize) as usize;
                }
                OpcodeResult::Continue
            }

            Opcode::Return => {
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(_) => Value::null(),
                };
                OpcodeResult::Return(value)
            }

            Opcode::ReturnVoid => OpcodeResult::Return(Value::null()),

            _ => OpcodeResult::Error(crate::vm::VmError::RuntimeError(format!(
                "Unexpected opcode in exec_control_flow_ops: {:?}",
                opcode
            ))),
        }
    }
}
