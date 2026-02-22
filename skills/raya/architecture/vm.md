# Virtual Machine

The Raya VM executes bytecode with a unified interpreter, work-stealing scheduler, and mark-sweep garbage collector.

## VM Components

```
┌────────────────────────────────────────┐
│             Raya VM                    │
├────────────────────────────────────────┤
│  Interpreter                           │
│  ├─ Bytecode Executor                  │
│  ├─ Opcode Handlers                    │
│  └─ Native Call Dispatch               │
├────────────────────────────────────────┤
│  Scheduler                             │
│  ├─ VM Worker Pool (CPU cores)         │
│  ├─ IO Worker Pool (blocking ops)      │
│  └─ Work-Stealing Deques               │
├────────────────────────────────────────┤
│  Garbage Collector                     │
│  ├─ Mark-Sweep (shared heap)           │
│  └─ Per-Task Nursery (64KB bump)       │
├────────────────────────────────────────┤
│  Object Model                          │
│  ├─ Classes, Arrays, Strings           │
│  ├─ Closures, Buffers                  │
│  └─ Maps, Sets                         │
└────────────────────────────────────────┘
```

## Interpreter

**Location:** `crates/raya-engine/src/vm/interpreter/`

### Unified Executor

Single `Interpreter::run()` method handles all execution:
- Function calls
- Task execution
- Native call returns

**Benefits:**
- No code duplication
- Consistent behavior
- Easier to maintain
- JIT-friendly

### Opcode Modules

Organized into 15 categorized modules:

1. **stack.rs** - Stack manipulation (Push, Pop, Dup)
2. **arithmetic.rs** - Arithmetic ops (IAdd, FAdd, IMul, etc.)
3. **comparison.rs** - Comparison (ILt, FLt, IEq, etc.)
4. **control.rs** - Control flow (Jump, JumpIf, Call, Return)
5. **variables.rs** - Variable access (LoadLocal, StoreLocal)
6. **objects.rs** - Object operations (NewObject, GetField, SetField)
7. **arrays.rs** - Array operations (NewArray, GetIndex, SetIndex)
8. **functions.rs** - Function operations (NewClosure, ClosureCapture)
9. **classes.rs** - Class operations (NewInstance, GetMethod)
10. **tasks.rs** - Concurrency (Spawn, Await, CheckPreemption)
11. **exceptions.rs** - Error handling (Throw, TryCatch)
12. **typeof.rs** - Type checking (TypeOf, InstanceOf)
13. **strings.rs** - String operations
14. **maps_sets.rs** - Map/Set operations
15. **special.rs** - Special opcodes (Nop, Debugger)

### Native Call Handlers

Extracted to 4 modules:

1. **builtin.rs** - Builtin class methods (String, Array, Object)
2. **core.rs** - Reflect API (149+ handlers)
3. **json.rs** - JSON parsing/serialization
4. **sync.rs** - Synchronization primitives (Mutex)

### Execution Loop

```rust
loop {
    let opcode = self.read_opcode()?;
    
    match opcode {
        Opcode::IAdd => {
            let b = self.pop_int()?;
            let a = self.pop_int()?;
            self.push_int(a + b);
        }
        Opcode::Call => {
            // Set up new call frame
            // Jump to function
        }
        Opcode::Await => {
            // Suspend current Task
            return Suspend(AwaitTask(task_id));
        }
        // ... 100+ opcodes
    }
}
```

## Scheduler

**Location:** `crates/raya-engine/src/vm/scheduler/`

### Work-Stealing Architecture

```
┌─────────────────────────────────────────────┐
│            Global Scheduler                 │
├─────────────────────────────────────────────┤
│  Worker 0          Worker 1     Worker N    │
│  ┌──────────┐    ┌──────────┐  ┌──────────┐│
│  │  Deque   │    │  Deque   │  │  Deque   ││
│  │  [T T T] │    │  [T T]   │  │  [T T T T]│
│  └──────────┘    └──────────┘  └──────────┘│
│      │ ↑             │ ↑            │ ↑     │
│      ↓ └─────steal───┘ └────steal───┘      │
│  ┌──────────┐    ┌──────────┐  ┌──────────┐│
│  │ Thread 0 │    │ Thread 1 │  │ Thread N ││
│  └──────────┘    └──────────┘  └──────────┘│
└─────────────────────────────────────────────┘
```

### Task States

- **Ready** - In work-stealing deque, waiting for worker
- **Running** - Currently executing on a worker
- **Suspended** - Waiting for I/O or another Task
- **Complete** - Finished with result

### Scheduling Algorithm

1. Worker pops Task from local deque (LIFO)
2. If empty, try to steal from another worker (FIFO)
3. If no work available, park thread
4. When Task completes, push result to channel
5. When Task suspends, handle suspension reason

### Suspension Reasons

```rust
pub enum Suspend {
    AwaitTask(TaskId),           // Waiting for another Task
    BlockingWork(Box<dyn Fn()>), // Needs I/O pool
    Sleep(Duration),             // Time-based delay
    NativeCall(u16, Vec<Value>), // Native function call
}
```

### IO Worker Pool

Separate thread pool for blocking operations:
- File I/O
- Network I/O
- Sleep
- Blocking native calls

**Benefits:**
- VM workers never block
- Better CPU utilization
- Predictable latency

## Garbage Collector

**Location:** `crates/raya-engine/src/vm/gc/`

### Two-Tier GC

**Tier 1: Per-Task Nursery (64KB)**
- Bump allocator (fast)
- Short-lived objects
- No synchronization needed
- Collected when Task completes

**Tier 2: Shared Heap**
- Mark-sweep collector
- Long-lived objects
- Stop-the-world collection
- Triggered by allocation pressure

### GC Algorithm

**Mark Phase:**
1. Pause all Tasks
2. Mark roots (stack, globals)
3. Traverse object graph
4. Mark reachable objects

**Sweep Phase:**
1. Iterate through heap
2. Free unmarked objects
3. Compact memory (optional)
4. Resume Tasks

### Object Layout

```
┌──────────────────────────┐
│ Header (16 bytes)        │
│  - Type tag (8 bytes)    │
│  - Mark bit (1 bit)      │
│  - Size (8 bytes)        │
├──────────────────────────┤
│ Fields / Data            │
│  (variable size)         │
└──────────────────────────┘
```

### Allocation Fast Path

```rust
// Nursery allocation (fast)
if size <= NURSERY_REMAINING {
    let ptr = nursery_ptr;
    nursery_ptr += size;
    return ptr;
}

// Heap allocation (slower)
allocate_from_heap(size)
```

## Object Model

**Location:** `crates/raya-engine/src/vm/object.rs`

### Value Types

```rust
pub enum Value {
    Int(i32),              // Unboxed
    Number(f64),           // Unboxed
    Bool(bool),            // Unboxed
    Null,                  // Unboxed
    Object(ObjectId),      // Boxed (GC-managed)
    String(StringId),      // Boxed (intern pool)
    Array(ArrayId),        // Boxed (GC-managed)
    Closure(ClosureId),    // Boxed (GC-managed)
}
```

### Object Types

- **Class Instance** - User-defined classes
- **Array** - Dynamic arrays
- **String** - Immutable UTF-8 strings (interned)
- **Closure** - Function + captured variables
- **Buffer** - Raw byte arrays
- **Map** - Hash maps
- **Set** - Hash sets
- **Task** - Concurrency primitive

### Class Layout

```rust
pub struct ClassInstance {
    pub class_id: ClassId,
    pub fields: Vec<Value>,
}
```

Fields stored inline, accessed by index.

### Array Layout

```rust
pub struct Array {
    pub elements: Vec<Value>,
    pub capacity: usize,
}
```

Dynamic growth with amortized O(1) push.

## Call Stack

**Location:** `crates/raya-engine/src/vm/stack.rs`

### Call Frame

```rust
pub struct CallFrame {
    pub function_id: FunctionId,
    pub ip: usize,              // Instruction pointer
    pub bp: usize,              // Base pointer (locals start)
    pub locals: Vec<Value>,     // Local variables
}
```

### Stack Layout

```
┌─────────────────────┐  ← Top
│   Operand Stack     │
├─────────────────────┤
│   Call Frame N      │
│   - IP, BP          │
│   - Locals          │
├─────────────────────┤
│   Call Frame N-1    │
│   ...               │
├─────────────────────┤
│   Call Frame 0      │  ← Base
│   (main)            │
└─────────────────────┘
```

## Performance Optimizations

### 1. Stack Pooling

Completed Task stacks are reused:
- Reduces allocation overhead
- Better cache locality
- Amortizes GC cost

### 2. String Interning

Strings are deduplicated:
- Single copy per unique string
- O(1) equality comparison
- Reduced memory usage

### 3. Typed Opcodes

Type-specific instructions:
- `IAdd` for int + int (no type check)
- `FAdd` for number + number
- `NAdd` for generic (boxed)

### 4. Inline Caching (Planned)

Cache property access results:
- Faster object property access
- Faster method calls
- Invalidate on class mutation

## VM Configuration

```rust
pub struct VmConfig {
    pub num_workers: usize,      // VM worker threads (default: CPU cores)
    pub io_pool_size: usize,     // IO worker threads (default: 4)
    pub heap_limit: usize,       // Max heap size (0 = unlimited)
    pub gc_threshold: f64,       // GC trigger (default: 0.75)
    pub stack_size: usize,       // Max stack per Task (default: 1MB)
}
```

## Debugging Support

- **Debugger opcode** - Breakpoint insertion
- **Stack traces** - On error, capture call stack
- **Source maps** - Map bytecode to source locations
- **Profiling hooks** - Per-function execution counts

## Related

- [Overview](overview.md) - Architecture overview
- [Compiler](compiler.md) - Bytecode generation
- [JIT/AOT](jit-aot.md) - Native compilation
