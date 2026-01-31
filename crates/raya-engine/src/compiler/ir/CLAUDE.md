# ir module

Intermediate Representation for the Raya compiler.

## Module Structure

```
ir/
├── mod.rs       # IrModule, re-exports
├── function.rs  # IrFunction, BasicBlock
├── block.rs     # BasicBlockId, BasicBlock
├── instr.rs     # IrInstr (instructions)
├── value.rs     # IrValue, IrConstant, Register
├── module.rs    # IrModule (collection of functions/classes)
└── pretty.rs    # Pretty printing for debugging
```

## Key Types

### IrModule
```rust
pub struct IrModule {
    pub name: String,
    pub functions: Vec<IrFunction>,
    pub classes: Vec<IrClass>,
    pub type_aliases: Vec<IrTypeAlias>,
}
```

### IrFunction
```rust
pub struct IrFunction {
    pub name: String,
    pub params: Vec<Register>,
    pub return_ty: TypeId,
    pub blocks: Vec<BasicBlock>,
}
```

### BasicBlock
```rust
pub struct BasicBlock {
    pub id: BasicBlockId,
    pub label: Option<String>,
    pub instrs: Vec<IrInstr>,
    pub terminator: Option<Terminator>,
}
```

### Register
```rust
pub struct Register {
    pub id: RegisterId,
    pub ty: TypeId,
}
```

## Instructions (IrInstr)

```rust
pub enum IrInstr {
    // Assignments
    Assign { dest: Register, value: IrValue },

    // Binary operations
    BinaryOp { dest: Register, op: BinaryOp, left: Register, right: Register },

    // Unary operations
    UnaryOp { dest: Register, op: UnaryOp, operand: Register },

    // Memory
    LoadLocal { dest: Register, index: u16 },
    StoreLocal { index: u16, value: Register },
    LoadField { dest: Register, object: Register, field_idx: u16 },
    StoreField { object: Register, field_idx: u16, value: Register },
    LoadElement { dest: Register, array: Register, index: Register },
    StoreElement { array: Register, index: Register, value: Register },

    // Calls
    Call { dest: Option<Register>, func_id: FunctionId, args: Vec<Register> },
    CallMethod { dest: Option<Register>, receiver: Register, method_idx: u16, args: Vec<Register> },
    NativeCall { dest: Option<Register>, native_id: u16, args: Vec<Register> },

    // Objects
    NewObject { dest: Register, class_id: ClassId },
    NewArray { dest: Register, size: Register },
    ArrayLen { dest: Register, array: Register },

    // Closures
    MakeClosure { dest: Register, func_id: FunctionId, captures: Vec<Register> },
    LoadCaptured { dest: Register, capture_idx: u16 },

    // Tasks
    Spawn { dest: Register, func_id: FunctionId, args: Vec<Register> },
    Await { dest: Register, task: Register },

    // Control flow setup
    SetupTry { catch_block: BasicBlockId, finally_block: Option<BasicBlockId> },
    EndTry,

    // ...more
}
```

## Terminators

```rust
pub enum Terminator {
    Return(Option<Register>),
    Jump(BasicBlockId),
    Branch { cond: Register, then_block: BasicBlockId, else_block: BasicBlockId },
    Switch { discriminant: Register, cases: Vec<(i32, BasicBlockId)>, default: BasicBlockId },
    Throw(Register),
    Unreachable,
}
```

## Values

```rust
pub enum IrValue {
    Register(Register),
    Constant(IrConstant),
}

pub enum IrConstant {
    Null,
    Bool(bool),
    I32(i32),
    I64(i64),
    F64(f64),
    String(String),
}
```

## Pretty Printing

```rust
use ir::PrettyPrint;
println!("{}", ir_module.pretty_print());
```

Output example:
```
function main() -> void {
entry:
    r0: number = 42
    r1: number = BinaryOp Add r0, r0
    Return r1
}
```

## For AI Assistants

- IR uses three-address code (dest = op src1, src2)
- Basic blocks end with exactly one terminator
- Registers are typed (`Register { id, ty }`)
- SSA-like: registers assigned once (mostly)
- Function IDs are indices, resolved at codegen
- `PrettyPrint` trait for debugging output
