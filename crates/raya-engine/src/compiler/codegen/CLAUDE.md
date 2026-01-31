# codegen module

IR to bytecode code generation.

## Module Structure

```
codegen/
├── mod.rs      # Entry point, generate()
├── context.rs  # CodegenContext, state management
├── emit.rs     # Instruction emission helpers
└── control.rs  # Control flow handling
```

## Key Types

### IrCodeGenerator
```rust
pub struct IrCodeGenerator {
    module_builder: ModuleBuilder,
    current_function: Option<FunctionBuilder>,
    label_map: HashMap<BasicBlockId, usize>,  // Block -> bytecode offset
    pending_jumps: Vec<PendingJump>,          // Forward jump fixups
}
```

### generate()
```rust
pub fn generate(ir_module: &IrModule) -> CompileResult<Module>
```

Main entry point that:
1. Creates bytecode module
2. Generates code for each IR function
3. Resolves jump targets
4. Builds final module

## Code Generation Process

### 1. Function Layout
```
For each function:
    1. Create function entry in module
    2. Allocate local slots
    3. Generate code for each block
    4. Resolve jumps
```

### 2. Block Ordering
Blocks are ordered to minimize jumps:
- Entry block first
- Fall-through for conditional branches where possible

### 3. Instruction Emission

```rust
// IR instruction → Bytecode
match instr {
    IrInstr::Assign { dest, value: IrValue::Constant(c) } => {
        emit_constant(writer, c);
        emit_store_local(writer, dest.slot);
    }

    IrInstr::BinaryOp { dest, op, left, right } => {
        emit_load_local(writer, left.slot);
        emit_load_local(writer, right.slot);
        emit_binary_op(writer, op, left.ty);
        emit_store_local(writer, dest.slot);
    }

    IrInstr::Call { dest, func_id, args } => {
        for arg in args {
            emit_load_local(writer, arg.slot);
        }
        emit_call(writer, func_id, args.len());
        if let Some(d) = dest {
            emit_store_local(writer, d.slot);
        }
    }
    // ... etc
}
```

### 4. Jump Resolution

Forward jumps need fixup:
```rust
// Emit placeholder
let jump_offset = writer.offset();
writer.emit_jump(0);  // Placeholder

// Later, when target block is emitted:
let target_offset = label_map[&target_block];
writer.patch_jump(jump_offset, target_offset);
```

## Emission Helpers (`emit.rs`)

```rust
// Constants
emit_const_null(writer)
emit_const_i32(writer, value)
emit_const_f64(writer, value)
emit_const_str(writer, pool_idx)

// Locals
emit_load_local(writer, idx)
emit_store_local(writer, idx)

// Arithmetic (type-specific)
emit_iadd(writer)  // Integer add
emit_fadd(writer)  // Float add
emit_nadd(writer)  // Number add (runtime dispatch)

// Control flow
emit_jump(writer, offset)
emit_jump_if(writer, offset)
emit_return(writer)

// Opcode size
opcode_size(opcode) -> usize
```

## Type-Specific Opcodes

The codegen chooses opcodes based on operand types:

```rust
fn emit_binary_op(writer: &mut BytecodeWriter, op: BinaryOp, ty: TypeId) {
    match (op, ty) {
        (Add, TypeId::INT) => writer.emit_iadd(),
        (Add, TypeId::FLOAT) => writer.emit_fadd(),
        (Add, TypeId::NUMBER) => writer.emit_nadd(),
        (Add, TypeId::STRING) => writer.emit_sconcat(),
        // ...
    }
}
```

## For AI Assistants

- IR registers map to local variable slots
- Jump offsets are relative to instruction start
- Forward jumps use placeholder + fixup
- Type information drives opcode selection
- Stack-based bytecode: load operands, operate, store result
- Methods use vtable indices, not function IDs
