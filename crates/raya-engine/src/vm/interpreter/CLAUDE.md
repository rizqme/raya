# interpreter module

Single-executor bytecode interpreter with suspendable task execution.

## Module Structure

```
interpreter/
├── mod.rs                    # Module index, re-exports
├── core.rs                   # Interpreter struct, main run() loop (~780 lines)
├── shared_state.rs           # SharedVmState (~155 lines)
├── vm_facade.rs              # Vm public API (facade over scheduler)
├── context.rs                # VmContext, ContextRegistry, VmOptions, ResourceLimits
├── execution.rs              # ExecutionResult, OpcodeResult, ControlFlow, ExecutionFrame
├── class_registry.rs         # Runtime class registry
├── lifecycle.rs              # VM lifecycle, InnerVm, VmSnapshot, VmStats
├── marshal.rs                # Value marshaling (MarshalledValue, ForeignHandleManager)
├── capabilities.rs           # Capability registry (HTTP, Log, Read)
├── module_registry.rs        # Module tracking
├── native_module_registry.rs # Native function registration (NativeFn, NativeModule)
├── safepoint.rs              # Safepoint coordination for GC
│
├── opcodes/                  # 15 categorized opcode handler modules
│   ├── mod.rs                # Module index
│   ├── arithmetic.rs         # IAdd/ISub/IMul/IDiv/IMod, FAdd/FSub/FMul/FDiv/FMod, INeg/FNeg
│   ├── arrays.rs             # NewArray, LoadElem, StoreElem, ArrayLen, ArrayPush/Pop, ArrayLiteral, InitArray
│   ├── calls.rs              # Call, CallClosure, CallMethod, CallConstructor, CallSuper
│   ├── closures.rs           # MakeClosure, LoadCaptured, StoreCaptured, SetClosureCapture, NewRefCell, Load/StoreRefCell
│   ├── comparison.rs         # IEq-IGe, FEq-FGe, Eq/Ne/StrictEq/StrictNe, Not/And/Or
│   ├── concurrency.rs        # Spawn, SpawnClosure, Await, WaitAll, Sleep, MutexLock/Unlock, Yield, TaskCancel
│   ├── constants.rs          # ConstNull/True/False, ConstI32/F64/Str
│   ├── control_flow.rs       # Jmp, JmpIfTrue/False/Null/NotNull, Return, ReturnVoid
│   ├── exceptions.rs         # Try, EndTry, Throw, Rethrow
│   ├── native.rs             # NativeCall + ModuleNativeCall dispatch (~1,487 lines)
│   ├── objects.rs            # New, LoadField, StoreField, OptionalField, ObjectLiteral, InitObject
│   ├── stack.rs              # Nop, Pop, Dup, Swap
│   ├── strings.rs            # Sconcat, Slen, Seq/Sne/Slt/Sle/Sgt/Sge, ToString
│   ├── types.rs              # InstanceOf, Cast, Typeof, JsonGet/Set, static field ops
│   └── variables.rs          # LoadLocal/StoreLocal (0/1 variants), LoadGlobal, StoreGlobal
│
└── handlers/                 # Native method handlers (extracted from core.rs)
    ├── mod.rs                # Module index
    ├── array.rs              # Array push/pop/shift/unshift/slice/splice/sort/map/filter/etc.
    ├── string.rs             # String charAt/substring/split/replace/indexOf/includes/etc.
    ├── regexp.rs             # RegExp test/exec/match/matchAll/replace
    └── reflect.rs            # Reflect API Phases 1-17 (~2,749 lines, 149+ handlers)
```

## Execution Model

**Single executor:** `Interpreter::run()` in `core.rs` is the sole bytecode interpreter.

```
Interpreter::run(task) → ExecutionResult
  │
  ├─ fetch opcode → decode → execute_opcode()
  │
  ├─ OpcodeResult::Continue    → advance IP, next instruction
  ├─ OpcodeResult::PushFrame   → push frame, call function/closure/constructor
  ├─ OpcodeResult::Return(v)   → pop frame or complete task
  ├─ OpcodeResult::Suspend(r)  → save state, yield to scheduler
  └─ OpcodeResult::Error(e)    → exception handling or fail
```

- **Frame-based calls:** Function calls, closures, constructors, and callbacks all use `PushFrame` — the main loop pushes an `ExecutionFrame` and continues. No nesting, no separate executor.
- **Suspend/resume:** `Await`, `Sleep`, `MutexLock` suspend the task. The scheduler re-enqueues it when the wait condition is met.
- **Compiler intrinsics:** Array callback methods (map/filter/reduce) and replaceWith are lowered to inline for-loops with `CallClosure` by the compiler — callbacks execute as normal frames.

## Key Types

### Interpreter
```rust
pub struct Interpreter<'a> {
    pub gc: &'a Mutex<GarbageCollector>,
    pub classes: &'a RwLock<ClassRegistry>,
    pub mutex_registry: &'a MutexRegistry,
    pub safepoint: &'a SafepointCoordinator,
    pub globals_by_index: &'a RwLock<Vec<Value>>,
    pub tasks: &'a Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,
    pub injector: &'a Arc<Injector<Arc<Task>>>,
    pub metadata: &'a Mutex<MetadataStore>,
    pub class_metadata: &'a RwLock<ClassMetadataRegistry>,
    pub native_handler: &'a Arc<dyn NativeHandler>,
    pub resolved_natives: &'a RwLock<ResolvedNatives>,
}
```

### Execution Types
```rust
enum ExecutionResult { Completed(Value), Suspended(SuspendReason), Failed(VmError) }
enum OpcodeResult { Continue, Return(Value), Suspend(SuspendReason), Error(VmError), PushFrame { ... } }
enum ReturnAction { PushReturnValue, PushObject(Value), Discard }
struct ExecutionFrame { func_id, ip, locals_base, is_closure, return_action }
```

### SharedVmState
```rust
pub struct SharedVmState {
    pub gc: Mutex<GarbageCollector>,
    pub classes: RwLock<ClassRegistry>,
    pub globals: RwLock<FxHashMap<String, Value>>,
    pub globals_by_index: RwLock<Vec<Value>>,
    pub safepoint: Arc<SafepointCoordinator>,
    pub tasks: Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,
    pub injector: Arc<Injector<Arc<Task>>>,
    pub mutex_registry: MutexRegistry,
    pub timer: Arc<TimerThread>,
    pub metadata: Mutex<MetadataStore>,
    pub class_metadata: RwLock<ClassMetadataRegistry>,
    pub native_handler: Arc<dyn NativeHandler>,
    pub resolved_natives: RwLock<ResolvedNatives>,
    pub native_registry: RwLock<NativeFunctionRegistry>,
}
```

## Opcode Dispatch

`core.rs` contains a thin `execute_opcode()` dispatcher — each match arm delegates to a method in the corresponding `opcodes/` module:

```rust
match opcode {
    Opcode::Nop | Opcode::Pop | Opcode::Dup | Opcode::Swap => self.handle_stack_op(opcode, task),
    Opcode::IAdd | Opcode::ISub | ... => self.handle_arithmetic(opcode, task),
    Opcode::Call | Opcode::CallClosure | ... => self.handle_call(opcode, task),
    Opcode::NativeCall => self.handle_native_call(task),
    // ~100 match arms, each a one-liner delegation
}
```

## Native Call Dispatch

`opcodes/native.rs` (~1,487 lines) handles `NativeCall` opcode dispatch by ID range:

| Range | Target | Handler |
|-------|--------|---------|
| 0x01xx | Array methods | `handlers/array.rs` |
| 0x02xx | String methods | `handlers/string.rs` |
| 0x03xx | Mutex ops | inline |
| 0x04xx | Channel ops | inline |
| 0x05xx | Task ops | inline |
| 0x07xx | Buffer ops | inline |
| 0x08xx | Map ops | inline |
| 0x09xx | Set ops | inline |
| 0x0Axx | RegExp methods | `handlers/regexp.rs` |
| 0x0Bxx | Date methods | inline |
| 0x0D00-0x0E2F | Reflect API | `handlers/reflect.rs` |
| 0x0Fxx | Number methods | inline |
| 0x10xx+ | Stdlib | via `NativeHandler` trait |

`ModuleNativeCall` opcode uses `resolved_natives` for name-based dispatch via `NativeFunctionRegistry`.

## Reflect API Handlers (`handlers/reflect.rs`)

Phases 1-17 implemented (~2,749 lines):
- **Phase 1-5**: Metadata, introspection, field access, method invocation, object creation
- **Phase 6-8**: Type utilities, interface queries, object inspection, memory analysis
- **Phase 9**: Proxy objects
- **Phase 10**: Dynamic subclass creation
- **Phase 13-17**: Generic metadata, ClassBuilder, BytecodeBuilder, permissions, dynamic modules

Native IDs: 0x0D00-0x0E2F (see `vm/builtin.rs`)

## For AI Assistants

- **Single executor**: `Interpreter::run()` in `core.rs` is the only bytecode interpreter
- Opcodes are dispatched to 15 categorized modules in `opcodes/`
- Native calls dispatch by ID in `opcodes/native.rs`, delegating to `handlers/` for complex types
- All function calls (including callbacks) use `OpcodeResult::PushFrame` — frame-based, no nesting
- Stdlib native calls (logger, math, crypto, etc.) delegate to `NativeHandler` trait
- `SharedVmState` provides thread-safe concurrent access to all shared VM state
- Task preemption is cooperative (checked at safepoints and backward jumps)
- Reflect handlers are in `handlers/reflect.rs` (Phases 1-17, 149+ methods)
- Array callback methods (map/filter/reduce/forEach) are **compiler intrinsics**, not runtime CallMethod
