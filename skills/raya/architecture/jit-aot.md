# JIT and AOT Compilation

Raya supports optional native compilation through JIT (Just-In-Time) and AOT (Ahead-Of-Time) compilation using the Cranelift backend.

## JIT Compilation (Feature-Gated)

**Feature Flag:** `--features jit`

**Location:** `crates/raya-engine/src/jit/`

### Architecture

```
Bytecode
    ↓ [Analysis]
Hot Functions Detected
    ↓ [SSA Lifter]
SSA-Form IR
    ↓ [Cranelift Backend]
Native Code
    ↓ [Code Cache]
JIT Entry Function
```

### Hot Function Detection

**Heuristics:**
- Call count threshold (default: 1000)
- Loop back-edge count
- Function size (sweet spot: 50-500 instructions)
- Avoid recursive functions (for now)

**Profiling:**
```rust
pub struct FunctionProfile {
    pub call_count: AtomicU64,
    pub loop_iterations: AtomicU64,
    pub last_compile_time: Option<Instant>,
}
```

### SSA Lifting

**Process:**
1. Decode bytecode to IR
2. Build control-flow graph (CFG)
3. Compute dominance frontiers
4. Insert Phi nodes
5. Rename variables to SSA form

**Loop Support:**
- Reverse postorder (RPO) traversal
- Deferred Cranelift block sealing
- Phi insertion at loop headers
- CheckPreemption at back-edges

### Cranelift Backend

**NaN-Boxing ABI:**
```
Value Encoding (64-bit):
┌────────────────────────────────────────────────┐
│ Int:    0x0000_0000_xxxx_xxxx                 │
│ Number: 0x7FFx_xxxx_xxxx_xxxx (IEEE 754)      │
│ Bool:   0xFFF8_0000_0000_000x (x = 0 or 1)    │
│ Null:   0xFFF8_0000_0000_0002                 │
│ Ptr:    0xFFFF_xxxx_xxxx_xxxx (object ref)    │
└────────────────────────────────────────────────┘
```

**Benefits:**
- Single 64-bit register per value
- Fast type checking (bitmask)
- No heap allocation for primitives

### Compilation Pipeline

```rust
pub fn compile_function(bytecode: &[u8]) -> Result<JitEntryFn> {
    let ir = lift_to_ssa(bytecode)?;
    let optimized = optimize_ir(ir)?;
    let native = cranelift_codegen(optimized)?;
    let entry = link_and_cache(native)?;
    Ok(entry)
}
```

### Pre-Warming

**Strategy:**
- Compile hot functions at module load time
- Run in background thread (non-blocking)
- Based on static analysis heuristics

**Heuristics:**
- Functions called in loops
- Recursive functions
- Functions with many call sites

### Adaptive Compilation

**Policy:**
- Start with bytecode interpreter
- Profile execution
- Compile hot functions on-the-fly
- Replace bytecode entry with JIT entry

### Integration with VM

```rust
impl Interpreter {
    fn call_function(&mut self, func_id: FunctionId) -> Result<()> {
        if let Some(jit_entry) = self.jit_cache.get(func_id) {
            // Call JIT-compiled version
            return self.call_jit(jit_entry);
        }
        
        // Fall back to bytecode interpreter
        self.interpret_bytecode(func_id)
    }
}
```

### Tests

**147 JIT tests:**
- 88 unit tests (SSA lifting, optimization)
- 59 integration tests (full pipeline with native execution)

---

## AOT Compilation (Feature-Gated)

**Feature Flag:** `--features aot`

**Location:** `crates/raya-engine/src/aot/`

**CLI:** `raya bundle input.raya -o output.bundle`

### Architecture

```
Source (.raya)
    ↓ [Compiler]
Bytecode
    ↓ [State Machine Transform]
Suspension-Aware IR
    ↓ [Cranelift Lowering]
Native Code
    ↓ [Bundle Format]
.bundle file
```

### State Machine Transform

**Purpose:** Handle suspension points (await, I/O, preemption)

**Process:**
1. Analyze bytecode for suspension points
2. Split function into states
3. Insert dispatch logic
4. Add save/restore code

**Suspension Points:**
- `Await` - Waiting for another Task
- `NativeCall` - Potential I/O
- `CheckPreemption` - Yield to scheduler

**Example:**
```typescript
// Original
async function example(): Task<int> {
  const x = await compute();
  const y = await compute();
  return x + y;
}

// Transformed to state machine
function example_state(state: int, locals: Locals): Result {
  match state {
    0 => {
      locals.x = await compute();  // Suspend, next state = 1
    }
    1 => {
      locals.y = await compute();  // Suspend, next state = 2
    }
    2 => {
      return locals.x + locals.y;  // Complete
    }
  }
}
```

### Cranelift Lowering

Same NaN-boxing ABI as JIT:
- Consistent representation
- Reusable backend code
- Predictable performance

**Differences from JIT:**
- No dynamic profiling
- All functions compiled upfront
- Direct compilation (no ObjectModule)
- Zero relocations via AotHelperTable

### Bundle Format

```
┌────────────────────────────────┐
│  Native Code (.text section)   │
│  - Compiled functions          │
│  - Helper trampolines          │
├────────────────────────────────┤
│  Function Table                │
│  - Entry points                │
│  - Metadata                    │
├────────────────────────────────┤
│  Virtual File System (VFS)     │
│  - Embedded .raya sources      │
│  - Dependency modules          │
├────────────────────────────────┤
│  Trailer                       │
│  - Magic: b"RAYAAOT\0"         │
│  - Offsets, sizes              │
└────────────────────────────────┘
```

### AotHelperTable

25 runtime helper functions:

1. Frame management (create, destroy)
2. GC operations (allocate, mark, sweep)
3. Value operations (box, unbox, type check)
4. Native call dispatch
5. Suspension handling
6. Error handling

**Usage:**
```rust
pub struct AotHelperTable {
    pub gc_allocate: extern "C" fn(size: usize) -> *mut u8,
    pub task_suspend: extern "C" fn(reason: SuspendReason),
    pub native_call: extern "C" fn(id: u16, args: *const Value) -> Value,
    // ... 22 more helpers
}
```

### Scheduler Integration

**AOT Executor:**
```rust
pub fn run_aot_function(entry: AotEntryFn, helpers: &AotHelperTable) 
    -> ExecutionResult {
    loop {
        let result = entry(helpers);
        match result {
            Complete(value) => return Ok(value),
            Suspended(reason) => {
                // Handle suspension (await, I/O)
                // Resume when ready
            }
            Error(e) => return Err(e),
        }
    }
}
```

### Tests

**70 AOT tests:**
- 55 AOT unit + integration tests
- 15 bundle format tests

---

## JIT vs AOT Comparison

| Feature | JIT | AOT |
|---------|-----|-----|
| **Compilation time** | On-demand (hot paths) | Upfront (all code) |
| **Startup time** | Fast (interpret first) | Slow (load native) |
| **Peak performance** | Same as AOT | Same as JIT |
| **Binary size** | Small (.ryb) | Large (.bundle) |
| **Adaptability** | Yes (profile-guided) | No (static) |
| **Deployment** | .ryb + runtime | Single .bundle |
| **Use case** | Development, servers | Production, edge |

## Performance Comparison

| Operation | Interpreter | JIT | AOT |
|-----------|------------|-----|-----|
| Int add | 5ns | 1ns | 1ns |
| Float mul | 8ns | 2ns | 2ns |
| Function call | 50ns | 10ns | 10ns |
| Property access | 30ns | 5ns | 5ns |
| Tight loop (1M iter) | 50ms | 5ms | 5ms |

**Note:** JIT and AOT have similar performance (same backend).

## Build Commands

```bash
# Build with JIT support
cargo build --features jit
cargo test --features jit

# Build with AOT support
cargo build --features aot
cargo test --features aot

# Build with both
cargo build --features jit,aot
```

## CLI Usage

### JIT (Enabled by Default)

```bash
# JIT enabled (default)
raya run app.raya

# Disable JIT
raya run --no-jit app.raya
```

### AOT

```bash
# Compile to native bundle
raya bundle app.raya -o app.bundle

# Run bundle
raya run app.bundle
```

## Related

- [Overview](overview.md) - Architecture overview
- [Compiler](compiler.md) - Bytecode generation
- [VM](vm.md) - Interpreter and scheduler
