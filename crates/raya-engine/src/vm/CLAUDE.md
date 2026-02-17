# vm module

Raya Virtual Machine runtime: interpreter, scheduler, GC, and runtime support.

## Module Structure

```
vm/
├── mod.rs              # Entry point, VmError, re-exports
├── value.rs            # Value representation (enum: Null, Bool, I32, I64, F64, Object, Array, String, Closure, Task, ...)
├── object.rs           # Object model (Class, Array, RayaString, Closure, Buffer, Map, Set, etc.)
├── stack.rs            # Call frames, operand stack
├── builtin.rs          # Native ID constants (all ranges: 0x01xx-0x6000+)
├── abi.rs              # Internal ABI (NativeContext, NativeValue, allocation/class/task helpers)
├── native_handler.rs   # NativeHandler trait, NativeCallResult, NoopNativeHandler
├── native_registry.rs  # NativeFunctionRegistry, ResolvedNatives (name-based dispatch)
│
├── interpreter/        # Single-executor bytecode interpreter
│   ├── core.rs         # Interpreter struct, main run() loop
│   ├── shared_state.rs # SharedVmState (concurrent state)
│   ├── vm_facade.rs    # Vm public API
│   ├── context.rs      # VmContext, VmOptions, ResourceLimits
│   ├── execution.rs    # ExecutionResult, OpcodeResult, ControlFlow
│   ├── opcodes/        # 15 categorized opcode handler modules
│   ├── handlers/       # Native method handlers (array, string, regexp, reflect)
│   └── ...             # lifecycle, marshal, capabilities, safepoint, etc.
│
├── scheduler/          # Task scheduler (work-stealing)
├── gc/                 # Garbage collector
├── sync/               # Synchronization primitives (Mutex, MutexRegistry)
├── snapshot/           # VM snapshotting
├── module/             # Module loading and linking
├── reflect/            # Reflection API runtime support
│   ├── class_metadata.rs  # ClassMetadataRegistry
│   ├── introspection.rs   # Type info, class hierarchy
│   ├── metadata.rs        # MetadataStore for user metadata
│   ├── snapshot.rs        # ObjectSnapshot, ObjectDiff
│   ├── proxy.rs           # Proxy detection and trap dispatch
│   ├── type_builder.rs    # Dynamic class creation (Phase 10)
│   ├── generic_metadata.rs # Generic type tracking (Phase 13)
│   ├── runtime_builder.rs # Runtime type creation (Phase 14)
│   ├── bytecode_builder.rs # Dynamic bytecode generation (Phase 15)
│   ├── function_builder.rs # FunctionWrapper, DecoratorRegistry (M3.9)
│   ├── permissions.rs     # Security & permissions (Phase 16)
│   ├── dynamic_module.rs  # Dynamic module system (Phase 17)
│   └── bootstrap.rs       # Bootstrap context (Phase 17)
├── builtins/           # Global native handlers
│   └── handlers/
│       ├── runtime.rs  # std:runtime method handlers
│       └── reflect.rs  # Global reflect registries (BytecodeBuilder, ClassBuilder)
├── json/               # JSON parsing and serialization
└── ffi/                # Foreign function interface
```

## Native ID Overview

| Range | Module | Description |
|-------|--------|-------------|
| 0x00xx | Object | hashCode, equals, toString |
| 0x01xx | Array | push, pop, shift, slice, sort, map, filter, etc. |
| 0x02xx | String | charAt, substring, indexOf, split, replace, etc. |
| 0x03xx | Mutex | lock, unlock |
| 0x04xx | Channel | send, receive, close, tryReceive, etc. |
| 0x05xx | Task | isDone, isCancelled |
| 0x07xx | Buffer | alloc, read/write, slice, copy, etc. |
| 0x08xx | Map | get, set, has, delete, keys, values, etc. |
| 0x09xx | Set | add, has, delete, values, etc. |
| 0x0Axx | RegExp | test, exec, match, matchAll, replace, replaceMatches |
| 0x0Bxx | Date | getYear, getMonth, format, parse, etc. |
| 0x0D00-0x0E2F | Reflect | Phases 1-17 (149+ handlers) |
| 0x0Fxx | Number | toFixed, toPrecision, toString(radix) |
| 0x1000-0x1003 | Logger (stdlib) | debug, info, warn, error |
| 0x2000-0x2016 | Math (stdlib) | abs, floor, ceil, sin, cos, sqrt, random, etc. |
| 0x3000-0x30FF | Runtime (stdlib) | Compiler, Bytecode, Vm, Parser, TypeChecker |
| 0x4000-0x400B | Crypto (stdlib) | hash, hmac, randomBytes, toHex, toBase64, etc. |
| 0x5000-0x5004 | Time (stdlib) | now, monotonic, hrtime, elapsed, sleep |
| 0x6000-0x600C | Path (stdlib) | join, normalize, dirname, basename, resolve, etc. |

## Key Types

### Value
```rust
pub enum Value {
    Null, Bool(bool), I32(i32), I64(i64), F64(f64),
    Object(ObjectRef), Array(ArrayRef), String(StringRef),
    Closure(ClosureRef), Task(TaskRef), ...
}
```

## Submodules

### `interpreter/` - Single Executor
See [interpreter/CLAUDE.md](interpreter/CLAUDE.md).
- Single `Interpreter::run()` entry point
- 15 opcode modules, 4 native handler modules
- Frame-based execution (no nesting)

### `scheduler/` - Task Scheduler
See [scheduler/CLAUDE.md](scheduler/CLAUDE.md).
- Work-stealing scheduler
- Task preemption (Go-style async)
- Multi-threaded execution

### `gc/` - Garbage Collector
See [gc/CLAUDE.md](gc/CLAUDE.md).
- Mark-sweep collection
- Object roots tracking

### `reflect/` - Reflection API Runtime
- Phases 1-17 implemented (metadata, introspection, proxies, dynamic code, permissions)
- Native IDs: 0x0D00-0x0E2F (see `builtin.rs`)
- See `vm/reflect/CLAUDE.md` for detailed implementation info

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

## For AI Assistants

- VM is stack-based with local variable slots
- Tasks are green threads, not OS threads
- Scheduler uses work-stealing for parallelism
- Objects have vtables for method dispatch
- Values are tagged (enum), not boxed
- Native calls use `NativeCall` opcode + native ID (dispatched in `interpreter/opcodes/native.rs`)
- Stdlib native calls delegate to `NativeHandler` trait (implemented by `StdNativeHandler` in raya-stdlib)
- `ModuleNativeCall` uses `NativeFunctionRegistry` for name-based dispatch
- Exception handling uses try/catch blocks in bytecode
