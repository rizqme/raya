# lower module

AST to IR lowering - transforms typed AST into three-address code IR.

## Module Structure

```
lower/
├── mod.rs           # Lowerer struct, module lowering
├── stmt.rs          # Statement lowering
├── expr.rs          # Expression lowering
└── control_flow.rs  # Control flow helpers
```

## Key Types

### Lowerer
```rust
pub struct Lowerer<'a> {
    type_ctx: &'a TypeContext,
    interner: &'a Interner,
    current_function: Option<IrFunction>,
    current_block: BasicBlockId,

    // Variable tracking
    local_map: HashMap<Symbol, u16>,      // name -> local slot
    local_registers: HashMap<u16, Register>,

    // Function/class maps
    function_map: HashMap<Symbol, FunctionId>,
    class_map: HashMap<Symbol, ClassId>,

    // Closure support
    captures: Vec<CaptureInfo>,
    refcell_vars: HashSet<Symbol>,  // vars needing RefCell wrapping

    // Loop support
    loop_stack: Vec<LoopContext>,

    // ... more state
}
```

## Lowering Process

### Module Level
```rust
lowerer.lower_module(&ast_module) -> IrModule
```

1. **First pass**: Collect function/class declarations, assign IDs
2. **Second pass**: Lower all declarations
3. **Generate main**: Top-level statements become `main()` function

### Statement Lowering (`stmt.rs`)

```rust
lowerer.lower_stmt(&stmt)
```

- `VariableDecl` → `StoreLocal`
- `If` → Branch with then/else blocks
- `While` → Loop with header/body/exit blocks
- `For` → Similar to while with init/update
- `Return` → `Terminator::Return`
- `Throw` → `Terminator::Throw`
- `Try/Catch` → `SetupTry`, `EndTry`, catch blocks

### Expression Lowering (`expr.rs`)

```rust
lowerer.lower_expr(&expr) -> Register
```

- Literals → `IrConstant`
- Binary ops → `BinaryOp` instruction
- Calls → `Call` or `NativeCall`
- Member access → `LoadField`
- Array index → `LoadElement`
- Arrow functions → `MakeClosure` with captures
- `new` → `NewObject` + constructor call
- `await` → `Await` instruction

## Control Flow

### If Statement
```
    [current block]
         │
         ▼
    Branch(cond)
     /         \
    ▼           ▼
[then_block] [else_block]
     \         /
      ▼       ▼
    [merge_block]
```

### While Loop
```
    [current block]
         │
         ▼
    [header_block] ◄───┐
         │             │
    Branch(cond)       │
     /         \       │
    ▼           ▼      │
[body_block]  [exit]   │
    │                  │
    └──────────────────┘
```

## Closure Capture Analysis

Before lowering a function, the lowerer scans for captured variables:

```rust
// Pre-scan to find variables captured by closures
self.scan_for_captured_vars(&stmts, &locals);

// Variables modified in closures need RefCell wrapping
// for capture-by-reference semantics
if self.refcell_vars.contains(&name) {
    // Emit NewRefCell, LoadRefCell, StoreRefCell
}
```

## For AI Assistants

- Lowering produces IR with explicit control flow (basic blocks)
- Variables become local slots (u16 indices)
- Closures capture variables, possibly via RefCell for mutations
- Async functions are marked, become `Spawn` at call sites
- Loop variables captured by closures get per-iteration RefCells
- Method calls are lowered to `CallMethod` with receiver
