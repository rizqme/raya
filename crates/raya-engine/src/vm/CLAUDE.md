# vm module

Raya Virtual Machine runtime: interpreter, scheduler, GC, and runtime support.

## Module Structure

```
vm/
├── mod.rs         # Entry point, VmError, re-exports
├── vm/            # Core VM and interpreter
├── scheduler/     # Task scheduler (work-stealing)
├── gc/            # Garbage collector
├── stack.rs       # Call frames, operand stack
├── value.rs       # Value representation
├── object.rs      # Object model (Class, Array, String)
├── builtin.rs     # Builtin native IDs (0x0Dxx for Reflect)
├── types/         # Runtime type registry
├── sync/          # Synchronization primitives
├── snapshot/      # VM snapshotting
├── json/          # JSON runtime support
├── module/        # Module loading and linking
├── reflect/       # Reflection API runtime support
│   ├── class_metadata.rs  # ClassMetadataRegistry
│   ├── introspection.rs   # Type info, class hierarchy
│   ├── metadata.rs        # MetadataStore for user metadata
│   ├── snapshot.rs        # ObjectSnapshot, ObjectDiff
│   ├── proxy.rs           # Proxy detection and trap dispatch
│   ├── type_builder.rs    # Dynamic class creation (Phase 10)
│   ├── runtime_builder.rs # Runtime type creation (Phase 14)
│   ├── bytecode_builder.rs # Dynamic bytecode generation (Phase 15)
│   ├── permissions.rs     # Security & permissions (Phase 16)
│   ├── dynamic_module.rs  # Dynamic module system (Phase 17)
│   └── bootstrap.rs       # Bootstrap context (Phase 17)
└── ffi/           # Foreign function interface
```

## Key Types

### Vm
```rust
pub struct Vm {
    contexts: Vec<VmContext>,
    scheduler: Scheduler,
    class_registry: ClassRegistry,
    // ...
}

vm.execute(&module) -> VmResult<Value>
vm.spawn_task(func_id, args) -> TaskId
vm.step() -> VmResult<()>  // Single step
```

### VmContext
```rust
pub struct VmContext {
    id: VmContextId,
    stack: Stack,
    module: Arc<Module>,
    // ...
}
```

### Value
```rust
pub enum Value {
    Null,
    Bool(bool),
    I32(i32),
    I64(i64),
    F64(f64),
    Object(ObjectRef),
    Array(ArrayRef),
    String(StringRef),
    Closure(ClosureRef),
    Task(TaskRef),
}
```

## Submodules

### `vm/` - Core Interpreter
See [vm/CLAUDE.md](vm/CLAUDE.md).
- Bytecode interpretation
- Instruction dispatch
- Native call handling

### `scheduler/` - Task Scheduler
See [scheduler/CLAUDE.md](scheduler/CLAUDE.md).
- Work-stealing scheduler
- Task preemption
- Multi-threaded execution

### `gc/` - Garbage Collector
See [gc/CLAUDE.md](gc/CLAUDE.md).
- Mark-sweep collection
- Object roots tracking

### `sync/` - Synchronization
See [sync/CLAUDE.md](sync/CLAUDE.md).
- Mutex implementation
- Task-aware blocking

### `snapshot/` - VM Snapshotting
See [snapshot/CLAUDE.md](snapshot/CLAUDE.md).
- State serialization
- Resume from snapshot

### `ffi/` - Foreign Functions
See [ffi/CLAUDE.md](ffi/CLAUDE.md).
- Native module loading
- Value conversion

### `reflect/` - Reflection API Runtime
- `ClassMetadataRegistry` - stores field names, method info per class
- `MetadataStore` - user-defined metadata via `Reflect.defineMetadata`
- `introspection.rs` - `get_class_id`, `is_instance_of`, `TypeInfo`
- `snapshot.rs` - `ObjectSnapshot`, `ObjectDiff` for state tracking
- `proxy.rs` - Proxy detection and trap helpers (`try_unwrap_proxy`, `unwrap_proxy_target`)
- `type_builder.rs` - Dynamic class creation (`SubclassDefinition`, `FieldDefinition`, `DynamicClassBuilder`)
- `generic_metadata.rs` - Generic type tracking (`GenericTypeInfo`, `GenericParameterInfo`, `GenericTypeRegistry`)
- `runtime_builder.rs` - Runtime type creation (`ClassBuilder`, `DynamicFunction`, `SpecializationCache`)
- `bytecode_builder.rs` - Dynamic bytecode generation (`BytecodeBuilder`, `CompiledFunction`, `Label`)
- Native call handlers use IDs 0x0D00-0x0E2F (see `builtin.rs`)
  - 0x0D00-0x0D0F: Metadata operations
  - 0x0D10-0x0D1F: Class introspection
  - 0x0D20-0x0D2F: Field access
  - 0x0D30-0x0D3F: Method invocation
  - 0x0D40-0x0D4F: Object creation
  - 0x0D50-0x0D5F: Type utilities
  - 0x0D60-0x0D6F: Interface/hierarchy query
  - 0x0D70-0x0D8F: Object inspection/memory
  - 0x0D90-0x0D9F: Stack introspection
  - 0x0DA0-0x0DAF: Serialization helpers
  - 0x0DB0-0x0DBF: Proxy objects (Phase 9)
  - 0x0DC0-0x0DCF: Dynamic subclass creation (Phase 10)
  - 0x0DD0-0x0DDF: Generic type metadata (Phase 13)
  - 0x0DE0-0x0DEF: Runtime type creation (Phase 14)
  - 0x0DF0-0x0DFF: Dynamic bytecode generation (Phase 15)
  - 0x0E00-0x0E0F: Security & permissions (Phase 16)
  - 0x0E10-0x0E2F: Dynamic VM bootstrap (Phase 17)

## Execution Model

### Stack-Based
```
// CONST_I32 42
Stack: [..., 42]

// LOAD_LOCAL 0
Stack: [..., 42, local[0]]

// IADD
Stack: [..., result]  // result = 42 + local[0]
```

### Task-Based Concurrency
```typescript
async function work() { ... }
const task = work();  // Spawns task immediately
const result = await task;  // Suspends current task
```

### Object Model
```
Object Layout:
┌─────────────────┐
│  VTable pointer │
├─────────────────┤
│  Field 0        │
│  Field 1        │
│  ...            │
└─────────────────┘
```

## Error Handling

```rust
pub enum VmError {
    StackOverflow,
    StackUnderflow,
    InvalidOpcode(u8),
    NullPointer,
    TypeError(String),
    RuntimeError(String),
    TaskPreempted,
    Suspended,
}
```

## For AI Assistants

- VM is stack-based with local variable slots
- Tasks are green threads, not OS threads
- Scheduler uses work-stealing for parallelism
- Objects have vtables for method dispatch
- Values are tagged (enum), not boxed
- Native calls use `NATIVE_CALL` opcode + native ID
- Exception handling uses try/catch blocks in bytecode
