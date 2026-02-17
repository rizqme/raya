use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::stack::Stack;
use crate::compiler::Opcode;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_stack_ops(
        &mut self,
        stack: &mut Stack,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::Nop => OpcodeResult::Continue,

            Opcode::Pop => {
                if let Err(e) = stack.pop() {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Dup => {
                match stack.peek() {
                    Ok(value) => {
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    Err(e) => return OpcodeResult::Error(e),
                }
                OpcodeResult::Continue
            }

            Opcode::Swap => {
                let a = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let b = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(a) {
                    return OpcodeResult::Error(e);
                }
                if let Err(e) = stack.push(b) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            _ => unreachable!("Not a stack opcode: {:?}", opcode),
        }
    }
}
