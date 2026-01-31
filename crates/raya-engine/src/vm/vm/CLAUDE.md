# vm/vm module

Core VM implementation: interpreter, context management, and instruction dispatch.

## Module Structure

```
vm/
├── mod.rs              # Vm struct, execution entry points
├── interpreter.rs      # Main instruction dispatch loop
├── class_registry.rs   # Runtime class management
├── shared_state.rs     # Shared state across contexts
└── task_interpreter.rs # Task-specific interpreter
```

## Key Types

### Vm
```rust
pub struct Vm {
    options: VmOptions,
    contexts: ContextRegistry,
    scheduler: Arc<Scheduler>,
    class_registry: ClassRegistry,
    mutex_registry: MutexRegistry,
    global_vars: Vec<Value>,
}

// Execution
vm.execute(&module) -> VmResult<Value>
vm.execute_function(&module, func_idx, args) -> VmResult<Value>

// Task management
vm.spawn_task(func_id, args) -> TaskId
vm.run_tasks() -> VmResult<()>
```

### VmOptions
```rust
pub struct VmOptions {
    pub max_stack_depth: usize,      // Default: 1024
    pub preemption_threshold: u64,   // Ops before preemption
    pub num_workers: usize,          // Scheduler threads
}
```

### VmContext
```rust
pub struct VmContext {
    pub id: VmContextId,
    pub stack: Stack,
    pub module: Arc<Module>,
    pub ip: usize,                   // Instruction pointer
    pub current_function: usize,     // Function index
}
```

## Instruction Dispatch (`interpreter.rs`)

```rust
fn execute_instruction(&mut self, ctx: &mut VmContext) -> VmResult<()> {
    let opcode = ctx.read_opcode()?;

    match opcode {
        Opcode::NOP => {}

        Opcode::CONST_I32 => {
            let value = ctx.read_i32()?;
            ctx.stack.push(Value::I32(value));
        }

        Opcode::IADD => {
            let b = ctx.stack.pop_i32()?;
            let a = ctx.stack.pop_i32()?;
            ctx.stack.push(Value::I32(a + b));
        }

        Opcode::CALL => {
            let func_id = ctx.read_u16()? as usize;
            let arg_count = ctx.read_u16()? as usize;
            self.call_function(ctx, func_id, arg_count)?;
        }

        Opcode::NATIVE_CALL => {
            let native_id = ctx.read_u16()?;
            let arg_count = ctx.read_u16()? as usize;
            self.native_call(ctx, native_id, arg_count)?;
        }

        Opcode::SPAWN => {
            let func_id = ctx.read_u16()? as usize;
            let arg_count = ctx.read_u16()? as usize;
            let task = self.spawn_task(func_id, args)?;
            ctx.stack.push(Value::Task(task));
        }

        Opcode::AWAIT => {
            let task = ctx.stack.pop_task()?;
            if !task.is_complete() {
                return Err(VmError::Suspended);
            }
            ctx.stack.push(task.result()?);
        }

        // ... 100+ more opcodes
    }
    Ok(())
}
```

## Native Call Dispatch

```rust
fn native_call(&mut self, ctx: &mut VmContext, native_id: u16, arg_count: usize) -> VmResult<()> {
    let args = ctx.stack.pop_n(arg_count)?;

    let result = match native_id {
        // Array methods (0x01xx)
        ARRAY_PUSH => self.array_push(&args)?,
        ARRAY_POP => self.array_pop(&args)?,
        ARRAY_LEN => self.array_len(&args)?,

        // String methods (0x02xx)
        STRING_CHAR_AT => self.string_char_at(&args)?,
        STRING_SUBSTRING => self.string_substring(&args)?,

        // Console (0x04xx)
        CONSOLE_LOG => self.console_log(&args)?,

        // ... etc
        _ => return Err(VmError::RuntimeError(format!("Unknown native: {}", native_id))),
    };

    if let Some(r) = result {
        ctx.stack.push(r);
    }
    Ok(())
}
```

## Class Registry (`class_registry.rs`)

```rust
pub struct ClassRegistry {
    classes: Vec<RuntimeClass>,
    vtables: Vec<VTable>,
}

registry.register_class(class_def) -> ClassId
registry.get_class(id) -> &RuntimeClass
registry.get_vtable(id) -> &VTable
registry.create_instance(id) -> ObjectRef
```

## Task Interpreter (`task_interpreter.rs`)

Handles task-specific execution:
- Preemption checking
- Suspension on await
- Result propagation

```rust
pub fn run_task(&mut self, task: &mut Task) -> TaskResult {
    loop {
        match self.step(task) {
            Ok(()) => continue,
            Err(VmError::TaskPreempted) => return TaskResult::Preempted,
            Err(VmError::Suspended) => return TaskResult::Suspended,
            Err(e) => return TaskResult::Error(e),
        }

        if task.is_complete() {
            return TaskResult::Complete(task.result.clone());
        }
    }
}
```

## For AI Assistants

- Main loop is in `interpreter.rs`
- Each opcode has explicit handling (no jump table)
- Native calls dispatch by ID to Rust implementations
- Task preemption is cooperative (checked periodically)
- Class instances are created via ClassRegistry
- Method calls use vtable lookup
- Exception handling uses try/catch bytecode markers
