# Dynamic VM Bootstrap Design

## Overview

Phase 17 enables creating and executing code entirely at runtime without any pre-compiled modules. This provides full bootstrap capability - starting with an empty VM and using the Reflect API to create modules, classes, functions, and execute them.

## Goals

1. **Runtime Module Creation** - Create modules dynamically with functions, classes, and globals
2. **Code Execution** - Execute dynamically created functions without pre-existing module context
3. **Bootstrap Context** - Minimal runtime environment for dynamic code execution
4. **Zero Overhead** - No performance impact on statically compiled code

## Architecture

### DynamicModule

A runtime-created module that can hold functions, classes, and global variables.

```rust
pub struct DynamicModule {
    pub id: usize,
    pub name: String,
    pub is_sealed: bool,

    // Functions added to this module (function_id -> CompiledFunction)
    pub functions: HashMap<usize, CompiledFunction>,

    // Classes added to this module (class_id -> DynamicClass info)
    pub classes: HashMap<usize, usize>,  // Maps local class ID to global class ID

    // Global variables (name -> Value)
    pub globals: HashMap<String, Value>,

    // Import mappings (import_name -> resolved module/function)
    pub imports: HashMap<String, ImportResolution>,
}
```

### DynamicModuleRegistry

Manages all dynamically created modules.

```rust
pub struct DynamicModuleRegistry {
    modules: HashMap<usize, DynamicModule>,
    next_module_id: usize,
    next_function_id: usize,  // Dynamic functions use high bit: 0x80000000+
}
```

### BootstrapContext

Provides minimal runtime environment for dynamic code.

```rust
pub struct BootstrapContext {
    pub object_class_id: usize,
    pub array_class_id: usize,
    pub string_class_id: usize,
    pub task_class_id: usize,

    // Native callbacks for basic I/O
    pub print_callback_id: u16,
}
```

## Native Call IDs (0x0E10-0x0E2F)

### Module Creation (0x0E10-0x0E17)

| ID | Method | Description |
|----|--------|-------------|
| 0x0E10 | createModule | Create empty dynamic module |
| 0x0E11 | moduleAddFunction | Add function to module |
| 0x0E12 | moduleAddClass | Add class to module |
| 0x0E13 | moduleAddGlobal | Add global variable |
| 0x0E14 | moduleSeal | Finalize module for execution |
| 0x0E15 | moduleLink | Resolve imports between modules |
| 0x0E16 | getModule | Get module by ID |
| 0x0E17 | getModuleByName | Get module by name |

### Execution (0x0E18-0x0E1F)

| ID | Method | Description |
|----|--------|-------------|
| 0x0E18 | execute | Execute function synchronously |
| 0x0E19 | spawn | Execute function as Task |
| 0x0E1A | eval | Execute raw bytecode |
| 0x0E1B | callDynamic | Call dynamic function by ID |
| 0x0E1C | invokeMethod | Invoke method on dynamic class |

### Bootstrap (0x0E20-0x0E2F)

| ID | Method | Description |
|----|--------|-------------|
| 0x0E20 | bootstrap | Initialize minimal runtime |
| 0x0E21 | getObjectClass | Get core Object class ID |
| 0x0E22 | getArrayClass | Get core Array class ID |
| 0x0E23 | getStringClass | Get core String class ID |
| 0x0E24 | getTaskClass | Get core Task class ID |
| 0x0E25 | print | Print to console |
| 0x0E26 | createArray | Create array from values |
| 0x0E27 | createString | Create string value |

## Function ID Allocation

To avoid conflicts with statically compiled functions:

- **Static functions**: 0x00000000 - 0x7FFFFFFF
- **Dynamic functions**: 0x80000000 - 0xFFFFFFFF

```rust
const DYNAMIC_FUNCTION_BASE: usize = 0x80000000;

impl DynamicModuleRegistry {
    fn allocate_function_id(&mut self) -> usize {
        let id = DYNAMIC_FUNCTION_BASE + self.next_function_id;
        self.next_function_id += 1;
        id
    }
}
```

## Execution Model

### Execute (Synchronous)

```rust
pub fn execute(
    ctx: &mut ExecutionContext,
    func: &CompiledFunction,
    args: Vec<Value>,
) -> Result<Value, VmError> {
    // 1. Create execution frame
    let frame = create_dynamic_frame(func, args);

    // 2. Push to call stack
    ctx.stack.push_frame(frame);

    // 3. Execute bytecode
    loop {
        match ctx.step()? {
            StepResult::Continue => continue,
            StepResult::Return(value) => return Ok(value),
            StepResult::Yield => return Err(VmError::UnexpectedYield),
        }
    }
}
```

### Spawn (Async)

```rust
pub fn spawn(
    scheduler: &Scheduler,
    func: &CompiledFunction,
    args: Vec<Value>,
) -> TaskId {
    // 1. Create new Task
    let task = Task::new_dynamic(func.function_id, args);

    // 2. Submit to scheduler
    scheduler.submit(task)
}
```

### Eval (Raw Bytecode)

```rust
pub fn eval(
    ctx: &mut ExecutionContext,
    bytecode: &[u8],
) -> Result<Value, VmError> {
    // 1. Validate bytecode
    validate_bytecode(bytecode)?;

    // 2. Create temporary function
    let func = CompiledFunction {
        function_id: EVAL_FUNCTION_ID,
        name: "<eval>".to_string(),
        bytecode: bytecode.to_vec(),
        locals_count: 0,
        max_stack: 16,
        constants: vec![],
    };

    // 3. Execute
    execute(ctx, &func, vec![])
}
```

## Integration Points

### With BytecodeBuilder (Phase 15)

Dynamic functions are created using BytecodeBuilder:

```typescript
const builder = Reflect.newBytecodeBuilder("add", 2, "number");
builder.emitLoadLocal(0);
builder.emitLoadLocal(1);
builder.emit(0x20);  // IADD
builder.emitReturn();

const func = builder.build();
const module = Reflect.createModule("math");
module.addFunction(func);
module.seal();

const result = Reflect.execute(func, [1, 2]);  // 3
```

### With ClassBuilder (Phase 14)

Dynamic classes can be added to modules:

```typescript
const pointClass = Reflect.newClassBuilder("Point")
    .addField("x", "number")
    .addField("y", "number")
    .build();

const module = Reflect.createModule("geometry");
module.addClass(pointClass);
module.seal();

const p = Reflect.construct(pointClass, 10, 20);
```

### With Permissions (Phase 16)

Code generation requires GENERATE_CODE permission:

```rust
fn execute(ctx: &ReflectHandlerContext, ...) -> Result<Value, VmError> {
    // Check permission before execution
    let store = PERMISSION_STORE.lock();
    check_code_generation(&store)?;

    // ... execute
}
```

## Performance Considerations

1. **Same Interpreter Loop** - Dynamic code uses the exact same bytecode interpreter as static code
2. **O(1) Function Lookup** - Dynamic functions stored in same lookup table (just with high bit set)
3. **No Runtime Checks** - No "is_dynamic" checks in hot paths
4. **Lazy Initialization** - Registry only allocated on first use

## Error Handling

```rust
pub enum DynamicExecutionError {
    ModuleNotSealed,
    FunctionNotFound(usize),
    InvalidBytecode(String),
    StackOverflow,
    TypeMismatch(String),
    PermissionDenied,
}
```

## Example: Hello World from Empty VM

```typescript
// Start with nothing
const ctx = Reflect.bootstrap();
const module = Reflect.createModule("main");

// Build hello function
const builder = Reflect.newBytecodeBuilder("hello", 0, "void");
builder.emitPush("Hello from dynamic code!");
builder.emitNativeCall(ctx.printCallbackId);
builder.emitReturnVoid();

const helloFunc = builder.build();
module.addFunction(helloFunc);
module.seal();

// Execute
Reflect.execute(helloFunc);  // Prints: Hello from dynamic code!
```

## Files

- `crates/raya-engine/src/vm/reflect/dynamic_module.rs` - DynamicModule, DynamicModuleRegistry
- `crates/raya-engine/src/vm/reflect/bootstrap.rs` - BootstrapContext
- `crates/raya-engine/src/vm/builtin.rs` - Native call IDs 0x0E10-0x0E2F
- `crates/raya-engine/src/vm/vm/handlers/reflect.rs` - Phase 17 handlers
