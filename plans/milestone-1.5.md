# Milestone 1.5: Basic Bytecode Interpreter

**Status:** ✅ Complete
**Goal:** Execute simple bytecode programs with arithmetic, control flow, and function calls
**Dependencies:** Milestone 1.4 (Stack & Frame Management)

---

## Overview

This milestone implements the core bytecode interpreter that can execute simple Raya programs. The interpreter includes an instruction dispatch loop, arithmetic operations, control flow (jumps, branches), function calls, and local variable access.

**Key Features:**
- **Instruction Dispatch:** Efficient opcode execution loop
- **Arithmetic Operations:** Integer and float math with type-specific opcodes
- **Control Flow:** Unconditional jumps, conditional branches
- **Function Calls:** CALL/RETURN with stack frame management
- **Local Variables:** LOAD_LOCAL/STORE_LOCAL using the stack
- **Error Handling:** Clear error messages with instruction pointer tracking

---

## Architecture

```
┌─────────────────────────────────────┐
│      Bytecode Interpreter           │
├─────────────────────────────────────┤
│ Dispatch Loop                       │
│   ├─ Fetch opcode                  │
│   ├─ Decode operands                │
│   ├─ Execute operation              │
│   └─ Update IP                      │
├─────────────────────────────────────┤
│ Opcode Implementations              │
│   ├─ Stack manipulation             │
│   ├─ Arithmetic (typed)             │
│   ├─ Comparisons                    │
│   ├─ Control flow                   │
│   └─ Function calls                 │
└─────────────────────────────────────┘
```

---

## Task Breakdown

### Task 1: Instruction Dispatch Loop

**File:** `crates/raya-core/src/vm/interpreter.rs`

Implement the core bytecode execution loop with instruction fetching and dispatching.

```rust
impl Vm {
    /// Execute a module
    pub fn execute(&mut self, module: &Module) -> VmResult<Value> {
        // Validate module
        module.validate().map_err(|e| VmError::RuntimeError(e))?;

        // Find main function
        let main_fn = module
            .functions
            .iter()
            .find(|f| f.name == "main")
            .ok_or_else(|| VmError::RuntimeError("No main function".to_string()))?;

        // Execute main function
        self.execute_function(main_fn, module)
    }

    /// Execute a single function
    fn execute_function(
        &mut self,
        function: &Function,
        module: &Module,
    ) -> VmResult<Value> {
        // Push initial frame
        self.stack.push_frame(
            function.id,
            0, // return IP (none for main)
            function.local_count,
            function.param_count,
        )?;

        let mut ip = 0;
        let code = &function.code;

        loop {
            // Bounds check
            if ip >= code.len() {
                return Err(VmError::RuntimeError(
                    "Instruction pointer out of bounds".to_string(),
                ));
            }

            // Fetch opcode
            let opcode_byte = code[ip];
            let opcode = Opcode::from_u8(opcode_byte)
                .ok_or(VmError::InvalidOpcode(opcode_byte))?;

            ip += 1;

            // Dispatch and execute
            match opcode {
                // Stack manipulation
                Opcode::Nop => {},
                Opcode::Pop => self.op_pop()?,
                Opcode::Dup => self.op_dup()?,
                Opcode::Swap => self.op_swap()?,

                // Constants
                Opcode::ConstNull => self.op_const_null()?,
                Opcode::ConstTrue => self.op_const_true()?,
                Opcode::ConstFalse => self.op_const_false()?,
                Opcode::ConstI32 => {
                    let value = self.read_i32(code, &mut ip)?;
                    self.op_const_i32(value)?;
                }
                Opcode::ConstF64 => {
                    let value = self.read_f64(code, &mut ip)?;
                    self.op_const_f64(value)?;
                }

                // Local variables
                Opcode::LoadLocal => {
                    let index = self.read_u8(code, &mut ip)?;
                    self.op_load_local(index as usize)?;
                }
                Opcode::StoreLocal => {
                    let index = self.read_u8(code, &mut ip)?;
                    self.op_store_local(index as usize)?;
                }

                // Arithmetic - Integer
                Opcode::Iadd => self.op_iadd()?,
                Opcode::Isub => self.op_isub()?,
                Opcode::Imul => self.op_imul()?,
                Opcode::Idiv => self.op_idiv()?,
                Opcode::Imod => self.op_imod()?,
                Opcode::Ineg => self.op_ineg()?,

                // Arithmetic - Float
                Opcode::Fadd => self.op_fadd()?,
                Opcode::Fsub => self.op_fsub()?,
                Opcode::Fmul => self.op_fmul()?,
                Opcode::Fdiv => self.op_fdiv()?,
                Opcode::Fneg => self.op_fneg()?,

                // Comparisons - Integer
                Opcode::Ieq => self.op_ieq()?,
                Opcode::Ine => self.op_ine()?,
                Opcode::Ilt => self.op_ilt()?,
                Opcode::Ile => self.op_ile()?,
                Opcode::Igt => self.op_igt()?,
                Opcode::Ige => self.op_ige()?,

                // Control flow
                Opcode::Jmp => {
                    let offset = self.read_i16(code, &mut ip)?;
                    ip = (ip as isize + offset as isize) as usize;
                }
                Opcode::JmpIfTrue => {
                    let offset = self.read_i16(code, &mut ip)?;
                    if self.stack.pop()?.is_truthy() {
                        ip = (ip as isize + offset as isize) as usize;
                    }
                }
                Opcode::JmpIfFalse => {
                    let offset = self.read_i16(code, &mut ip)?;
                    if !self.stack.pop()?.is_truthy() {
                        ip = (ip as isize + offset as isize) as usize;
                    }
                }

                // Function calls
                Opcode::Call => {
                    let func_index = self.read_u16(code, &mut ip)?;
                    let callee = &module.functions[func_index as usize];

                    // Execute callee (recursive call)
                    let result = self.execute_function(callee, module)?;

                    // Push result
                    self.stack.push(result)?;
                }
                Opcode::Return => {
                    // Pop return value (or null if none)
                    let return_value = if self.stack.depth() > 0 {
                        self.stack.pop()?
                    } else {
                        Value::null()
                    };

                    // Pop frame
                    self.stack.pop_frame()?;

                    return Ok(return_value);
                }

                _ => {
                    return Err(VmError::RuntimeError(format!(
                        "Unimplemented opcode: {:?}",
                        opcode
                    )));
                }
            }
        }
    }

    // Helper methods for reading operands
    fn read_u8(&self, code: &[u8], ip: &mut usize) -> VmResult<u8> {
        if *ip >= code.len() {
            return Err(VmError::RuntimeError("Unexpected end of bytecode".to_string()));
        }
        let value = code[*ip];
        *ip += 1;
        Ok(value)
    }

    fn read_u16(&self, code: &[u8], ip: &mut usize) -> VmResult<u16> {
        if *ip + 1 >= code.len() {
            return Err(VmError::RuntimeError("Unexpected end of bytecode".to_string()));
        }
        let value = u16::from_le_bytes([code[*ip], code[*ip + 1]]);
        *ip += 2;
        Ok(value)
    }

    fn read_i16(&self, code: &[u8], ip: &mut usize) -> VmResult<i16> {
        Ok(self.read_u16(code, ip)? as i16)
    }

    fn read_i32(&self, code: &[u8], ip: &mut usize) -> VmResult<i32> {
        if *ip + 3 >= code.len() {
            return Err(VmError::RuntimeError("Unexpected end of bytecode".to_string()));
        }
        let value = i32::from_le_bytes([
            code[*ip],
            code[*ip + 1],
            code[*ip + 2],
            code[*ip + 3],
        ]);
        *ip += 4;
        Ok(value)
    }

    fn read_f64(&self, code: &[u8], ip: &mut usize) -> VmResult<f64> {
        if *ip + 7 >= code.len() {
            return Err(VmError::RuntimeError("Unexpected end of bytecode".to_string()));
        }
        let value = f64::from_le_bytes([
            code[*ip],
            code[*ip + 1],
            code[*ip + 2],
            code[*ip + 3],
            code[*ip + 4],
            code[*ip + 5],
            code[*ip + 6],
            code[*ip + 7],
        ]);
        *ip += 8;
        Ok(value)
    }
}
```

**Tests:**
- [x] Test empty bytecode execution
- [x] Test simple NOP sequence
- [x] Test instruction pointer tracking
- [x] Test invalid opcode detection
- [x] Test bounds checking

---

### Task 2: Stack Manipulation Operations

**File:** `crates/raya-core/src/vm/interpreter.rs` (continued)

Implement basic stack manipulation opcodes.

```rust
impl Vm {
    /// POP - Remove top value from stack
    fn op_pop(&mut self) -> VmResult<()> {
        self.stack.pop()?;
        Ok(())
    }

    /// DUP - Duplicate top stack value
    fn op_dup(&mut self) -> VmResult<()> {
        let value = self.stack.peek()?;
        self.stack.push(value)?;
        Ok(())
    }

    /// SWAP - Swap top two stack values
    fn op_swap(&mut self) -> VmResult<()> {
        let a = self.stack.pop()?;
        let b = self.stack.pop()?;
        self.stack.push(a)?;
        self.stack.push(b)?;
        Ok(())
    }
}
```

**Tests:**
- [x] Test POP removes value
- [x] Test DUP duplicates correctly
- [x] Test SWAP swaps top two values
- [x] Test stack underflow errors

---

### Task 3: Constant Operations

**File:** `crates/raya-core/src/vm/interpreter.rs` (continued)

Implement constant loading opcodes.

```rust
impl Vm {
    /// CONST_NULL - Push null constant
    fn op_const_null(&mut self) -> VmResult<()> {
        self.stack.push(Value::null())
    }

    /// CONST_TRUE - Push true constant
    fn op_const_true(&mut self) -> VmResult<()> {
        self.stack.push(Value::bool(true))
    }

    /// CONST_FALSE - Push false constant
    fn op_const_false(&mut self) -> VmResult<()> {
        self.stack.push(Value::bool(false))
    }

    /// CONST_I32 - Push 32-bit integer constant
    fn op_const_i32(&mut self, value: i32) -> VmResult<()> {
        self.stack.push(Value::i32(value))
    }

    /// CONST_F64 - Push 64-bit float constant (future)
    fn op_const_f64(&mut self, _value: f64) -> VmResult<()> {
        // TODO: Add f64 support to Value
        Err(VmError::RuntimeError("f64 not yet supported".to_string()))
    }
}
```

**Tests:**
- [x] Test CONST_NULL pushes null
- [x] Test CONST_TRUE/FALSE push booleans
- [x] Test CONST_I32 with various values
- [x] Test constant values are correct on stack

---

### Task 4: Local Variable Operations

**File:** `crates/raya-core/src/vm/interpreter.rs` (continued)

Implement local variable access using the stack.

```rust
impl Vm {
    /// LOAD_LOCAL - Push local variable onto stack
    fn op_load_local(&mut self, index: usize) -> VmResult<()> {
        let value = self.stack.load_local(index)?;
        self.stack.push(value)
    }

    /// STORE_LOCAL - Pop stack, store in local variable
    fn op_store_local(&mut self, index: usize) -> VmResult<()> {
        let value = self.stack.pop()?;
        self.stack.store_local(index, value)
    }
}
```

**Tests:**
- [x] Test LOAD_LOCAL reads correct variable
- [x] Test STORE_LOCAL writes correct variable
- [x] Test local variable persistence
- [x] Test invalid index error

---

### Task 5: Integer Arithmetic Operations

**File:** `crates/raya-core/src/vm/interpreter.rs` (continued)

Implement typed integer arithmetic opcodes.

```rust
impl Vm {
    /// IADD - Add two integers
    fn op_iadd(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::i32(a.wrapping_add(b)))
    }

    /// ISUB - Subtract two integers
    fn op_isub(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::i32(a.wrapping_sub(b)))
    }

    /// IMUL - Multiply two integers
    fn op_imul(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::i32(a.wrapping_mul(b)))
    }

    /// IDIV - Divide two integers
    fn op_idiv(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;

        if b == 0 {
            return Err(VmError::RuntimeError("Division by zero".to_string()));
        }

        self.stack.push(Value::i32(a / b))
    }

    /// IMOD - Modulo two integers
    fn op_imod(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;

        if b == 0 {
            return Err(VmError::RuntimeError("Modulo by zero".to_string()));
        }

        self.stack.push(Value::i32(a % b))
    }

    /// INEG - Negate an integer
    fn op_ineg(&mut self) -> VmResult<()> {
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::i32(-a))
    }
}
```

**Tests:**
- [x] Test IADD with positive numbers
- [x] Test IADD with negative numbers
- [x] Test IADD overflow (wrapping)
- [x] Test ISUB
- [x] Test IMUL
- [x] Test IDIV with division by zero error
- [x] Test IMOD with modulo by zero error
- [x] Test INEG

---

### Task 6: Float Arithmetic Operations (Placeholder)

**File:** `crates/raya-core/src/vm/interpreter.rs` (continued)

Add placeholder implementations for float operations (to be completed when f64 is added to Value).

```rust
impl Vm {
    /// FADD - Add two floats (TODO: implement when f64 added to Value)
    fn op_fadd(&mut self) -> VmResult<()> {
        Err(VmError::RuntimeError("Float operations not yet supported".to_string()))
    }

    /// FSUB - Subtract two floats
    fn op_fsub(&mut self) -> VmResult<()> {
        Err(VmError::RuntimeError("Float operations not yet supported".to_string()))
    }

    /// FMUL - Multiply two floats
    fn op_fmul(&mut self) -> VmResult<()> {
        Err(VmError::RuntimeError("Float operations not yet supported".to_string()))
    }

    /// FDIV - Divide two floats
    fn op_fdiv(&mut self) -> VmResult<()> {
        Err(VmError::RuntimeError("Float operations not yet supported".to_string()))
    }

    /// FNEG - Negate a float
    fn op_fneg(&mut self) -> VmResult<()> {
        Err(VmError::RuntimeError("Float operations not yet supported".to_string()))
    }
}
```

---

### Task 7: Comparison Operations

**File:** `crates/raya-core/src/vm/interpreter.rs` (continued)

Implement integer comparison opcodes.

```rust
impl Vm {
    /// IEQ - Integer equality
    fn op_ieq(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::bool(a == b))
    }

    /// INE - Integer inequality
    fn op_ine(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::bool(a != b))
    }

    /// ILT - Integer less than
    fn op_ilt(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::bool(a < b))
    }

    /// ILE - Integer less or equal
    fn op_ile(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::bool(a <= b))
    }

    /// IGT - Integer greater than
    fn op_igt(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::bool(a > b))
    }

    /// IGE - Integer greater or equal
    fn op_ige(&mut self) -> VmResult<()> {
        let b = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        let a = self.stack.pop()?.as_i32()
            .ok_or_else(|| VmError::TypeError("Expected i32".to_string()))?;
        self.stack.push(Value::bool(a >= b))
    }
}
```

**Tests:**
- [x] Test IEQ with equal values
- [x] Test IEQ with different values
- [x] Test INE
- [x] Test ILT with various combinations
- [x] Test ILE
- [x] Test IGT
- [x] Test IGE

---

### Task 8: Control Flow Operations

**File:** `crates/raya-core/src/vm/interpreter.rs` (continued)

Control flow is already implemented in the main dispatch loop (Task 1).

**Tests:**
- [x] Test JMP forward
- [x] Test JMP backward (loop)
- [x] Test JMP_IF_TRUE when true
- [x] Test JMP_IF_TRUE when false
- [x] Test JMP_IF_FALSE when true
- [x] Test JMP_IF_FALSE when false
- [x] Test jump to invalid address

---

### Task 9: Function Call Operations

**File:** `crates/raya-core/src/vm/interpreter.rs` (continued)

Function calls are already implemented in the main dispatch loop (Task 1).

**Tests:**
- [x] Test simple function call
- [x] Test function with parameters
- [x] Test function return value
- [x] Test nested function calls
- [x] Test recursive function calls
- [x] Test return without value

---

### Task 10: Global Variable Operations

**File:** `crates/raya-core/src/vm/interpreter.rs` (continued)

Implement global variable access (future enhancement).

```rust
impl Vm {
    /// LOAD_GLOBAL - Load global variable
    fn op_load_global(&mut self, name: &str) -> VmResult<()> {
        let value = self.globals
            .get(name)
            .copied()
            .ok_or_else(|| VmError::RuntimeError(format!("Undefined global: {}", name)))?;
        self.stack.push(value)
    }

    /// STORE_GLOBAL - Store global variable
    fn op_store_global(&mut self, name: String) -> VmResult<()> {
        let value = self.stack.pop()?;
        self.globals.insert(name, value);
        Ok(())
    }
}
```

---

## Integration Tests

**File:** `tests/interpreter_integration.rs`

Create comprehensive integration tests for the interpreter.

```rust
use raya_core::{Vm, Value};
use raya_bytecode::{Module, Function, Opcode};

#[test]
fn test_simple_arithmetic() {
    // Bytecode: 10 + 20
    // CONST_I32 10
    // CONST_I32 20
    // IADD
    // RETURN

    let mut module = Module::new();
    let main_fn = Function {
        id: 0,
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            Opcode::ConstI32 as u8, 10, 0, 0, 0,
            Opcode::ConstI32 as u8, 20, 0, 0, 0,
            Opcode::Iadd as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();

    assert_eq!(result, Value::i32(30));
}

#[test]
fn test_local_variables() {
    // Bytecode:
    // local x = 42
    // local y = 10
    // return x + y

    let mut module = Module::new();
    let main_fn = Function {
        id: 0,
        name: "main".to_string(),
        param_count: 0,
        local_count: 2,
        code: vec![
            Opcode::ConstI32 as u8, 42, 0, 0, 0,
            Opcode::StoreLocal as u8, 0,
            Opcode::ConstI32 as u8, 10, 0, 0, 0,
            Opcode::StoreLocal as u8, 1,
            Opcode::LoadLocal as u8, 0,
            Opcode::LoadLocal as u8, 1,
            Opcode::Iadd as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();

    assert_eq!(result, Value::i32(52));
}

#[test]
fn test_conditional_branch() {
    // Bytecode: if (10 > 5) { return 1 } else { return 0 }

    let mut module = Module::new();
    let main_fn = Function {
        id: 0,
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            Opcode::ConstI32 as u8, 10, 0, 0, 0,
            Opcode::ConstI32 as u8, 5, 0, 0, 0,
            Opcode::Igt as u8,
            Opcode::JmpIfFalse as u8, 8, 0,  // Skip to else branch
            Opcode::ConstI32 as u8, 1, 0, 0, 0,
            Opcode::Return as u8,
            // Else branch
            Opcode::ConstI32 as u8, 0, 0, 0, 0,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();

    assert_eq!(result, Value::i32(1));
}

#[test]
fn test_function_call() {
    // Bytecode:
    // function add(a, b) { return a + b }
    // function main() { return add(10, 20) }

    let mut module = Module::new();

    // Function 0: add(a, b)
    let add_fn = Function {
        id: 0,
        name: "add".to_string(),
        param_count: 2,
        local_count: 2,
        code: vec![
            Opcode::LoadLocal as u8, 0,  // Load a
            Opcode::LoadLocal as u8, 1,  // Load b
            Opcode::Iadd as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(add_fn);

    // Function 1: main()
    let main_fn = Function {
        id: 1,
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            Opcode::ConstI32 as u8, 10, 0, 0, 0,
            Opcode::ConstI32 as u8, 20, 0, 0, 0,
            Opcode::Call as u8, 0, 0,  // Call function 0
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    // Need to execute main (index 1)
    let result = vm.execute(&module).unwrap();

    assert_eq!(result, Value::i32(30));
}
```

---

## Acceptance Criteria

- [x] Interpreter can execute simple bytecode programs
- [x] All basic arithmetic operations work correctly
- [x] Conditional branches work (JMP_IF_TRUE, JMP_IF_FALSE)
- [x] Function calls and returns work
- [x] Local variables can be read and written
- [x] Stack manipulation works correctly
- [x] Error handling provides clear messages
- [x] Division by zero is detected
- [x] Type errors are caught (wrong type for operation)
- [x] All unit tests pass
- [x] All integration tests pass
- [x] Code coverage >85% for interpreter module

---

## Reference Documentation

- **OPCODE.md Section 3:** Complete instruction set
- **OPCODE.md Section 7:** Example bytecode programs
- **ARCHITECTURE.md Section 3:** Stack and execution model
- **MAPPING.md:** Language-to-bytecode examples

---

## Next Steps

After completing this milestone:

1. **Milestone 1.6:** Object Model - heap-allocated objects and arrays
2. **Milestone 1.7:** Complete GC with precise marking
3. **Milestone 1.9:** Task Scheduler for concurrency

---

## Notes

### Implementation Order

1. Start with dispatch loop and operand reading
2. Implement stack manipulation (simplest)
3. Add constants and local variables
4. Implement integer arithmetic
5. Add comparisons
6. Implement control flow (already in dispatch)
7. Test function calls thoroughly
8. Add comprehensive integration tests

### Performance Considerations

- Use inline functions for opcode implementations
- Minimize bounds checking in hot loop
- Cache frequently used values (e.g., function references)
- Profile interpreter to find bottlenecks

### Future Enhancements

- Add f64 support to Value type
- Implement optimized local variable opcodes (LOAD_LOCAL_0, etc.)
- Add string operations
- Implement array operations
- Add exception handling (try/catch)
