use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::compiler::Opcode;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_variable_ops(
        &mut self,
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        locals_base: usize,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::LoadLocal => {
                let index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.peek_at(locals_base + index) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreLocal => {
                let index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.set_at(locals_base + index, value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadLocal0 => {
                let value = match stack.peek_at(locals_base) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadLocal1 => {
                let value = match stack.peek_at(locals_base + 1) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreLocal0 => {
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.set_at(locals_base, value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreLocal1 => {
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.set_at(locals_base + 1, value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadGlobal => {
                let index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let globals = self.globals_by_index.read();
                let value = globals.get(index).copied().unwrap_or(Value::null());
                drop(globals);
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreGlobal => {
                let index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let mut globals = self.globals_by_index.write();
                if index >= globals.len() {
                    globals.resize(index + 1, Value::null());
                }
                globals[index] = value;
                OpcodeResult::Continue
            }

            _ => unreachable!("Not a variable opcode: {:?}", opcode),
        }
    }
}
