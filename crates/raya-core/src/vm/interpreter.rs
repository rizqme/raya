//! Virtual machine interpreter

use raya_bytecode::{Module, Opcode};
use crate::{gc::GarbageCollector, stack::Stack, value::Value, VmError, VmResult};

/// Raya virtual machine
pub struct Vm {
    /// Garbage collector
    gc: GarbageCollector,
    /// Operand stack
    stack: Stack,
    /// Global variables
    globals: rustc_hash::FxHashMap<String, Value>,
}

impl Vm {
    /// Create a new VM
    pub fn new() -> Self {
        Self {
            gc: GarbageCollector::default(),
            stack: Stack::new(),
            globals: rustc_hash::FxHashMap::default(),
        }
    }

    /// Execute a module
    pub fn execute(&mut self, module: &Module) -> VmResult<Value> {
        // Validate module
        module.validate()
            .map_err(|e| VmError::RuntimeError(e))?;

        // Find main function
        let main_fn = module.functions
            .iter()
            .find(|f| f.name == "main")
            .ok_or_else(|| VmError::RuntimeError("No main function".to_string()))?;

        // Execute main function
        self.execute_function(main_fn)
    }

    /// Execute a single function
    fn execute_function(&mut self, function: &raya_bytecode::module::Function) -> VmResult<Value> {
        let mut ip = 0;

        while ip < function.code.len() {
            let opcode_byte = function.code[ip];
            let opcode = Opcode::from_u8(opcode_byte)
                .ok_or(VmError::InvalidOpcode(opcode_byte))?;

            ip += 1;

            match opcode {
                Opcode::Nop => {},
                Opcode::ConstNull => {
                    self.stack.push(Value::null())?;
                }
                Opcode::ConstTrue => {
                    self.stack.push(Value::bool(true))?;
                }
                Opcode::ConstFalse => {
                    self.stack.push(Value::bool(false))?;
                }
                Opcode::Return => {
                    return if self.stack.peek().is_ok() {
                        self.stack.pop()
                    } else {
                        Ok(Value::null())
                    };
                }
                _ => {
                    return Err(VmError::RuntimeError(
                        format!("Unimplemented opcode: {:?}", opcode)
                    ));
                }
            }
        }

        Ok(Value::null())
    }
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_creation() {
        let _vm = Vm::new();
    }
}
