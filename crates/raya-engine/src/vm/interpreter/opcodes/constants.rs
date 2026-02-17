use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::RayaString;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use crate::compiler::{Module, Opcode};

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_constant_ops(
        &mut self,
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        module: &Module,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::ConstNull => {
                if let Err(e) = stack.push(Value::null()) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ConstTrue => {
                if let Err(e) = stack.push(Value::bool(true)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ConstFalse => {
                if let Err(e) = stack.push(Value::bool(false)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ConstI32 => {
                let value = match Self::read_i32(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(value)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ConstF64 => {
                let value = match Self::read_f64(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(value)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ConstStr => {
                let index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let s = match module.constants.strings.get(index) {
                    Some(s) => s,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid string constant index: {}",
                            index
                        )));
                    }
                };
                let raya_string = RayaString::new(s.clone());
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            _ => unreachable!("Not a constant opcode: {:?}", opcode),
        }
    }
}
