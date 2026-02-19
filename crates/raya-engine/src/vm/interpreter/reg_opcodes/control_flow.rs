//! Register-based control flow opcode handlers

use crate::compiler::bytecode::reg_opcode::{RegInstr, RegOpcode};
use crate::vm::interpreter::reg_execution::RegOpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::register_file::RegisterFile;
use crate::vm::value::Value;
use crate::vm::VmError;

/// Result of a control flow operation
pub enum RegControlFlow {
    /// Continue to next instruction
    Continue,
    /// Jump to absolute instruction index
    Jump(usize),
    /// Return from function with value
    Return(Value),
}

impl<'a> Interpreter<'a> {
    /// Execute a control flow opcode.
    ///
    /// Returns `RegControlFlow` instead of `RegOpcodeResult` because jumps
    /// modify the IP, which the dispatch loop handles.
    pub(in crate::vm::interpreter) fn exec_reg_control_flow_ops(
        &mut self,
        regs: &mut RegisterFile,
        reg_base: usize,
        instr: RegInstr,
        ip: usize, // current IP (already past this instruction)
    ) -> Result<RegControlFlow, VmError> {
        let opcode = match instr.opcode() {
            Some(op) => op,
            None => return Err(VmError::InvalidOpcode(instr.opcode_byte())),
        };

        match opcode {
            RegOpcode::Jmp => {
                // PC += sBx (relative jump)
                let offset = instr.sbx() as isize;
                let target = (ip as isize + offset) as usize;
                Ok(RegControlFlow::Jump(target))
            }

            RegOpcode::JmpIf => {
                // if rA then PC += sBx
                let val = regs.get_reg(reg_base, instr.a())?;
                if val.is_truthy() {
                    let offset = instr.sbx() as isize;
                    let target = (ip as isize + offset) as usize;
                    Ok(RegControlFlow::Jump(target))
                } else {
                    Ok(RegControlFlow::Continue)
                }
            }

            RegOpcode::JmpIfNot => {
                // if !rA then PC += sBx
                let val = regs.get_reg(reg_base, instr.a())?;
                if !val.is_truthy() {
                    let offset = instr.sbx() as isize;
                    let target = (ip as isize + offset) as usize;
                    Ok(RegControlFlow::Jump(target))
                } else {
                    Ok(RegControlFlow::Continue)
                }
            }

            RegOpcode::JmpIfNull => {
                // if rA == null then PC += sBx
                let val = regs.get_reg(reg_base, instr.a())?;
                if val.is_null() {
                    let offset = instr.sbx() as isize;
                    let target = (ip as isize + offset) as usize;
                    Ok(RegControlFlow::Jump(target))
                } else {
                    Ok(RegControlFlow::Continue)
                }
            }

            RegOpcode::JmpIfNotNull => {
                // if rA != null then PC += sBx
                let val = regs.get_reg(reg_base, instr.a())?;
                if !val.is_null() {
                    let offset = instr.sbx() as isize;
                    let target = (ip as isize + offset) as usize;
                    Ok(RegControlFlow::Jump(target))
                } else {
                    Ok(RegControlFlow::Continue)
                }
            }

            RegOpcode::Return => {
                // return rA
                let val = regs.get_reg(reg_base, instr.a())?;
                Ok(RegControlFlow::Return(val))
            }

            RegOpcode::ReturnVoid => {
                // return null
                Ok(RegControlFlow::Return(Value::null()))
            }

            _ => Err(VmError::RuntimeError(format!(
                "Not a control flow opcode: {:?}",
                opcode
            ))),
        }
    }
}
