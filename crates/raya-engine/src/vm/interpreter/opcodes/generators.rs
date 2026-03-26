//! Generator opcode handlers.

use crate::compiler::Opcode;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::scheduler::SuspendReason;
use crate::vm::stack::Stack;
use crate::vm::VmError;

impl<'a> Interpreter<'a> {
    pub(crate) fn exec_generator_ops(&mut self, stack: &mut Stack, opcode: Opcode) -> OpcodeResult {
        match opcode {
            Opcode::GeneratorInitSuspend => OpcodeResult::Suspend(SuspendReason::JsGeneratorInit),
            Opcode::GeneratorYield => match stack.pop() {
                Ok(value) => OpcodeResult::Suspend(SuspendReason::JsGeneratorYield { value }),
                Err(error) => OpcodeResult::Error(error),
            },
            _ => OpcodeResult::Error(VmError::RuntimeError(format!(
                "Unexpected opcode in exec_generator_ops: {:?}",
                opcode
            ))),
        }
    }
}
