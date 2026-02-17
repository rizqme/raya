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

## Compiler Intrinsics (`expr.rs`)

Certain method calls that take callbacks are lowered to **inline loops** instead of `CallMethod`. This avoids nested execution — callbacks execute as normal frames on the main interpreter stack.

### Array Callback Intrinsics (expr.rs:1028-1040)

Detected by `ARRAY_CALLBACK_METHODS` list, handled by `lower_array_intrinsic()` (expr.rs:1078):

- `map`, `filter`, `reduce`, `forEach`, `find`, `findIndex`, `some`, `every`, `sort`

Pattern: array length → for-loop → `CallClosure(callback, [element, index])` → accumulate result.

### replaceWith Intrinsic (expr.rs:1045-1050)

Detected by `REPLACE_WITH_METHODS` list, handled by `lower_replace_with_intrinsic()` (expr.rs:2139):

- `string.replaceWith(regexp, callback)` (native ID 0x0217)
- `regexp.replaceWith(string, callback)` (native ID 0x0A05)

Pattern: `NativeCall(REGEXP_REPLACE_MATCHES)` → get match array → for-loop → `CallClosure(callback, [match])` → string concatenation.

### Why Intrinsics

These methods take callback closures. Without intrinsics, they would require a nested executor loop (the deleted `execute_nested_function`). By inlining the loop at IR level, `CallClosure` compiles to `OpcodeResult::PushFrame` — the callback runs as a normal frame in `Interpreter::run()`, with full suspend/await/exception support.

## For AI Assistants

- Lowering produces IR with explicit control flow (basic blocks)
- Variables become local slots (u16 indices)
- Closures capture variables, possibly via RefCell for mutations
- Async functions are marked, become `Spawn` at call sites
- Loop variables captured by closures get per-iteration RefCells
- Method calls are lowered to `CallMethod` with receiver
- **Array callback methods** (map/filter/reduce/forEach/find/findIndex/some/every/sort) are compiler intrinsics — NOT runtime CallMethod
- **replaceWith** is also a compiler intrinsic — uses REGEXP_REPLACE_MATCHES + inline loop
- When adding new callback-taking methods, follow the intrinsic pattern in `lower_array_intrinsic()`
