# Raya Implementation Plan

This document outlines the complete implementation roadmap for the Raya programming language and virtual machine, written in Rust.

---

## Table of Contents

1. [Overview](#overview)
2. [Phase 1: VM Core](#phase-1-vm-core)
3. [Phase 2: Parser & Type Checker](#phase-2-parser--type-checker)
4. [Phase 3: Compiler & Code Generation](#phase-3-compiler--code-generation)
5. [Phase 4: Standard Library](#phase-4-standard-library)
6. [Phase 5: Package Manager](#phase-5-package-manager)
7. [Phase 6: Testing System](#phase-6-testing-system)
8. [Phase 7: Tooling & Developer Experience](#phase-7-tooling--developer-experience)
9. [Milestones](#milestones)

---

## Overview

**Technology Stack:**
- **Language**: Rust (stable)
- **Target**: Native binary with embedded VM
- **Architecture**: Interpreter-based VM with future JIT support

**Project Structure:**
```
rayavm/
├── crates/
│   ├── raya-core/        # VM runtime
│   ├── raya-bytecode/    # Bytecode definitions
│   ├── raya-parser/      # Lexer & Parser
│   ├── raya-types/       # Type system
│   ├── raya-compiler/    # Code generation
│   ├── raya-stdlib/      # Standard library
│   ├── raya-cli/         # CLI tool (rayac)
│   └── raya-pm/             # Package manager
├── stdlib/                 # Raya standard library source
├── tests/                  # Integration tests
├── examples/               # Example programs
├── design/                 # Specification docs
└── plans/                  # Implementation plans
```

**Dependencies:**
- `clap` - CLI argument parsing
- `serde` / `serde_json` - Serialization
- `logos` - Lexer generation
- `lalrpop` - Parser generation (alternative)
- `crossbeam` - Work-stealing scheduler
- `parking_lot` - Efficient synchronization
- `rustc-hash` - Fast hashing
- `mimalloc` - Fast allocator

---

## Phase 1: VM Core

**Goal:** Build a functional bytecode interpreter with garbage collection, task scheduling, and VM controllability.

### 1.1 Project Setup ✅

**Status:** Complete

**Tasks:**
- [x] Initialize Rust workspace
- [x] Set up crate structure
- [x] Configure dependencies
- [x] Set up testing infrastructure

**Files:**
```
Cargo.toml (workspace)
crates/raya-bytecode/Cargo.toml
crates/raya-core/Cargo.toml
```

### 1.2 Bytecode Definitions ✅

**Crate:** `raya-bytecode`

**Status:** Complete

**Tasks:**
- [x] Define `Opcode` enum (all opcodes from OPCODE.md)
- [x] Implement bytecode encoding/decoding
- [x] Create bytecode module format
- [x] Add constant pool structure
- [x] Implement bytecode verification

**Files:**
```rust
// crates/raya-bytecode/src/lib.rs
pub mod opcode;
pub mod module;
pub mod constants;
pub mod verify;

// crates/raya-bytecode/src/opcode.rs
#[repr(u8)]
pub enum Opcode {
    Nop = 0x00,
    ConstI32 = 0x01,
    ConstF64 = 0x02,
    // ... all opcodes
}

// crates/raya-bytecode/src/module.rs
pub struct Module {
    pub magic: [u8; 4],      // "RAYA"
    pub version: u32,
    pub constants: ConstantPool,
    pub functions: Vec<Function>,
    pub classes: Vec<ClassDef>,
    pub metadata: Metadata,
}
```

**Reference:** `design/OPCODE.md`

### 1.3 Value Representation & Type Metadata

**Crate:** `raya-core`

**Status:** ✅ Complete

**Goal:** Foundation for precise GC - efficient value encoding and type metadata for pointer scanning.

**Tasks:**
- [x] Implement tagged pointer value representation
- [x] Build PointerMap system for precise GC
- [x] Create TypeRegistry for runtime type information
- [x] Register standard built-in types
- [x] Implement GcHeader with type metadata
- [x] Create GcPtr smart pointer
- [x] Add VmContext structure with resource limits
- [x] Implement per-context heap allocator

**Files Implemented:**
```rust
// crates/raya-core/src/value.rs - ✅ COMPLETE
#[repr(transparent)]
pub struct Value(u64);  // Tagged pointer: i32, bool, null inline, heap pointers

impl Value {
    pub const fn null() -> Self;
    pub const fn bool(b: bool) -> Self;
    pub const fn i32(i: i32) -> Self;
    pub unsafe fn from_ptr<T>(ptr: NonNull<T>) -> Self;

    pub const fn is_null(&self) -> bool;
    pub const fn is_bool(&self) -> bool;
    pub const fn is_i32(&self) -> bool;
    pub const fn is_ptr(&self) -> bool;
    pub const fn is_heap_allocated(&self) -> bool;

    pub unsafe fn as_ptr<T>(&self) -> Option<NonNull<T>>;
}

// crates/raya-core/src/types/pointer_map.rs - ✅ COMPLETE
pub enum PointerMap {
    None,                         // No pointers (primitives)
    All(usize),                   // All fields are pointers
    Offsets(Vec<usize>),          // Specific field offsets
    Array { length, element_map } // Array elements
}

impl PointerMap {
    pub fn none() -> Self;
    pub fn offsets(offsets: Vec<usize>) -> Self;
    pub fn has_pointers(&self) -> bool;
    pub fn pointer_count(&self) -> usize;
    pub fn for_each_pointer_offset<F>(&self, base_offset: usize, f: F);
}

// crates/raya-core/src/types/registry.rs - ✅ COMPLETE
pub struct TypeInfo {
    pub type_id: TypeId,
    pub name: &'static str,
    pub size: usize,
    pub align: usize,
    pub pointer_map: PointerMap,
    pub drop_fn: Option<DropFn>,
}

pub struct TypeRegistry {
    types: Arc<HashMap<TypeId, TypeInfo>>,
}

impl TypeRegistry {
    pub fn builder() -> TypeRegistryBuilder;
    pub fn get(&self, type_id: TypeId) -> Option<&TypeInfo>;
    pub fn for_each_pointer<F>(&self, base_ptr: *mut u8, type_id: TypeId, f: F);
}

pub fn create_standard_registry() -> TypeRegistry; // i32, i64, f32, f64, bool, String, etc.

// crates/raya-core/src/gc/header.rs - ✅ COMPLETE
#[repr(C, align(8))]
pub struct GcHeader {
    marked: bool,
    context_id: VmContextId,
    type_id: TypeId,
    size: usize,
}

// crates/raya-core/src/gc/ptr.rs - ✅ COMPLETE
pub struct GcPtr<T: ?Sized> {
    ptr: NonNull<T>,
}

impl<T: ?Sized> GcPtr<T> {
    pub unsafe fn new(ptr: NonNull<T>) -> Self;
    pub fn as_ptr(&self) -> *mut T;
    pub unsafe fn header(&self) -> &GcHeader;
    pub fn is_marked(&self) -> bool;
    pub fn mark(&self);
    pub fn unmark(&self);
}

// crates/raya-core/src/vm/context.rs - ✅ COMPLETE
pub struct VmContext {
    id: VmContextId,
    gc: GarbageCollector,
    globals: HashMap<String, Value>,
    limits: ResourceLimits,
    counters: ResourceCounters,
    type_registry: Arc<TypeRegistry>,
}

pub struct ResourceLimits {
    pub max_heap_bytes: Option<usize>,
    pub max_tasks: Option<usize>,
    pub max_step_budget: Option<u64>,
}

// crates/raya-core/src/gc/heap.rs - ✅ COMPLETE
pub struct Heap {
    context_id: VmContextId,
    type_registry: Arc<TypeRegistry>,
    allocations: Vec<*mut GcHeader>,
    allocated_bytes: usize,
    max_heap_bytes: usize,
}

impl Heap {
    pub fn allocate<T: 'static>(&mut self, value: T) -> GcPtr<T>;
    pub fn allocate_array<T: 'static>(&mut self, len: usize) -> GcPtr<[T]>;
    pub unsafe fn free(&mut self, header_ptr: *mut GcHeader);
}
```

**Reference:** `design/ARCHITECTURE.md` Section 5.2, 5.3

**What's Complete:**
- Tagged pointer value system with 64-bit encoding
- Complete type metadata infrastructure (PointerMap + TypeRegistry)
- GC-managed heap allocator with per-context isolation
- Resource limits and accounting (heap size, task count, CPU budget)
- GC header with mark bit, context ID, type ID, and allocation size
- Smart pointer type (GcPtr) with automatic header access

**Next Steps:**
- Section 1.4: Stack & Frame Management
- Section 1.5: Basic Bytecode Interpreter (opcodes for constants, arithmetic, control flow)

### 1.4 Stack & Frame Management

**Tasks:**
- [ ] Implement operand stack
- [ ] Create call frame structure
- [ ] Add stack overflow protection
- [ ] Implement function call mechanism

**Files:**
```rust
// crates/raya-core/src/stack.rs
pub struct Stack {
    slots: Vec<Value>,
    frames: Vec<CallFrame>,
    sp: usize,  // Stack pointer
    fp: usize,  // Frame pointer
}

pub struct CallFrame {
    function: FunctionRef,
    ip: usize,          // Instruction pointer
    base_pointer: usize,
    local_count: usize,
}
```

**Reference:** `design/ARCHITECTURE.md` Section 3

### 1.5 Bytecode Interpreter (Basic)

**Status:** ✅ Complete

**Goal:** Execute simple bytecode programs without GC or concurrency.

**Tasks:**
- [x] Build instruction dispatch loop
- [x] Implement arithmetic opcodes (IADD, ISUB, IMUL, IDIV, IMOD, INEG)
- [x] Implement comparison opcodes (IEQ, INE, ILT, ILE, IGT, IGE)
- [x] Implement control flow (JMP, JMP_IF_TRUE, JMP_IF_FALSE)
- [x] Implement function calls (CALL, RETURN)
- [x] Add local variable access (LOAD_LOCAL, STORE_LOCAL)
- [x] Add stack manipulation (POP, DUP, SWAP)
- [x] Add constant operations (CONST_NULL, CONST_TRUE, CONST_FALSE, CONST_I32)
- [x] Basic error handling (division by zero, type errors, bounds checking)
- [x] Comprehensive test coverage (17 tests, all passing)

**Files:**
```rust
// crates/raya-core/src/vm/interpreter.rs
pub struct Vm {
    gc: GarbageCollector,
    stack: Stack,
    globals: HashMap<String, Value>,
}

impl Vm {
    pub fn execute(&mut self, module: &Module) -> Result<Value, VmError>;

    fn dispatch(&mut self, opcode: Opcode) -> Result<(), VmError> {
        match opcode {
            Opcode::ConstI32 => self.op_const_i32(),
            Opcode::Iadd => self.op_iadd(),
            Opcode::Call => self.op_call(),
            // ... all opcodes
        }
    }
}
```

**Reference:** `design/OPCODE.md` Sections 3, 7

### 1.6 Object Model

**Goal:** Heap-allocated objects with class-based structure.

**Tasks:**
- [ ] Implement Object and Class structures
- [ ] Add field access (LOAD_FIELD, STORE_FIELD)
- [ ] Build vtable system for method dispatch
- [ ] Add array operations (NEW_ARRAY, ARRAY_LOAD, ARRAY_STORE)
- [ ] Implement string operations

**Files:**
```rust
// crates/raya-core/src/object.rs
pub struct Object {
    class_id: usize,
    fields: Vec<Value>,
}

pub struct Class {
    name: String,
    field_count: usize,
    methods: Vec<Method>,
    vtable: VTable,
}

pub struct VTable {
    entries: Vec<FunctionRef>,
}
```

**Reference:** `design/LANG.md` Section 8, `design/ARCHITECTURE.md` Section 2

### 1.7 Memory Management & Garbage Collection

**Goal:** Per-context precise mark-sweep GC with type metadata.

**Status:** ✅ Complete

**Tasks:**
- [x] Create VmContext structure (heap, resources, limits)
- [x] Implement per-context heap allocator
- [x] Create GcHeader with type metadata
- [x] Build basic GarbageCollector structure
- [x] Add allocation threshold checking
- [x] Build precise mark-sweep GC with type-metadata-guided pointer traversal
- [x] Root set management (stack, globals) integration
- [x] GC statistics and tuning

**Files Implemented:**
```rust
// crates/raya-core/src/vm/context.rs - ✅ COMPLETE
pub struct VmContext {
    id: VmContextId,
    gc: GarbageCollector,
    globals: HashMap<String, Value>,
    limits: ResourceLimits,
    counters: ResourceCounters,
    type_registry: Arc<TypeRegistry>,
}

impl VmContext {
    pub fn new() -> Self;
    pub fn with_options(options: VmOptions) -> Self;
    pub fn gc(&self) -> &GarbageCollector;
    pub fn gc_mut(&mut self) -> &mut GarbageCollector;
    pub fn collect_garbage(&mut self);
}

// crates/raya-core/src/gc/collector.rs - 🔄 PARTIAL (structure done, mark phase needs completion)
pub struct GarbageCollector {
    heap: Heap,
    roots: RootSet,
    threshold: usize,
    stats: GcStats,
}

impl GarbageCollector {
    pub fn new(context_id: VmContextId, type_registry: Arc<TypeRegistry>) -> Self;
    pub fn allocate<T: 'static>(&mut self, value: T) -> GcPtr<T>;
    pub fn collect(&mut self);  // Per-context collection
    pub fn add_root(&mut self, value: Value);

    // TODO: Implement precise marking with type metadata
    fn mark_value(&mut self, value: Value);  // Currently placeholder
}

// crates/raya-core/src/gc/roots.rs - ✅ COMPLETE
pub struct RootSet {
    stack_roots: Vec<Value>,
    global_roots: Vec<Value>,
}
```

**Reference:** `design/ARCHITECTURE.md` Section 5, `plans/milestone-1.3.md`, `plans/milestone-1.7.md`

**Complete Implementation:**
- VmContext with isolated per-context heaps
- Resource limits (max heap size, max tasks, CPU budget)
- Heap allocator with type-aware allocation
- Precise mark-sweep GC with type-metadata-guided pointer traversal
- Automatic root collection from stack (operands + locals) and globals
- Comprehensive GC statistics (pause time, survival rate, live objects/bytes)
- Automatic threshold adjustment (2x live size, min 1MB)
- Special handling for Object, Array, RayaString with dynamic fields

**Future Enhancements:**
- Phase 2: Generational GC (young-gen copying collector)
- Phase 3: Incremental/Concurrent GC (if needed)

### 1.8 Native JSON Type

**Goal:** Dynamic JSON values with runtime type casting and validation.

**Status:** 📋 Planned

**Tasks:**
- [ ] Implement JsonValue runtime type (enum with Null/Bool/Number/String/Array/Object/Undefined)
- [ ] Add JSON_GET, JSON_INDEX, JSON_CAST opcodes
- [ ] Implement dynamic property access (returns json)
- [ ] Implement dynamic array indexing (returns json)
- [ ] Build runtime validation algorithm for type casting
- [ ] Add JSON.parse() and JSON.stringify() to stdlib
- [ ] GC integration for JsonValue marking
- [ ] Type schema storage in TypeRegistry

**Files:**
```rust
// crates/raya-core/src/json/mod.rs
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(GcPtr<RayaString>),
    Array(GcPtr<Vec<JsonValue>>),
    Object(GcPtr<HashMap<String, JsonValue>>),
    Undefined,
}

impl JsonValue {
    pub fn get_property(&self, key: &str) -> JsonValue;
    pub fn get_index(&self, index: usize) -> JsonValue;
}

// crates/raya-core/src/json/cast.rs
pub struct TypeSchema {
    pub type_id: usize,
    pub kind: TypeKind,
}

pub fn validate_cast(
    json: JsonValue,
    schema: &TypeSchema,
    gc: &mut GarbageCollector,
) -> VmResult<Value>;

// crates/raya-stdlib/src/json.rs
pub fn parse(json_text: String, gc: &mut GarbageCollector) -> VmResult<Value>;
pub fn stringify(json_value: JsonValue) -> VmResult<String>;
```

**Reference:** `design/JSON-TYPE.md`, `plans/milestone-1.8.md`

**Dependencies:**
- `serde_json` crate for parsing/serialization

### 1.9 Safepoint Infrastructure

**Goal:** Coordinated stop-the-world pauses for GC and snapshotting.

**Tasks:**
- [ ] Implement SafepointCoordinator
- [ ] Add safepoint poll mechanism
- [ ] STW pause protocol (request, wait, resume)
- [ ] Insert safepoints at: function calls, loop back-edges, allocations, await points
- [ ] Integration with interpreter loop

**Files:**
```rust
// crates/raya-core/src/vm/safepoint.rs
pub struct SafepointCoordinator {
    gc_pending: AtomicBool,
    snapshot_pending: AtomicBool,
    workers_at_safepoint: AtomicUsize,
    barrier: Barrier,
}

impl SafepointCoordinator {
    #[inline(always)]
    pub fn poll(&self) {
        if self.gc_pending.load(Ordering::Acquire) ||
           self.snapshot_pending.load(Ordering::Acquire) {
            self.enter_safepoint();
        }
    }

    pub fn request_stw_pause(&self, reason: StopReason);
    pub fn resume_from_pause(&self);
}
```

**Reference:** `design/ARCHITECTURE.md` Section 5.6, `design/SNAPSHOTTING.md` Section 2

### 1.10 Task Scheduler (Goroutine-Style) ✅

**Status:** Complete

**Goal:** Work-stealing multi-threaded task execution with Go-style asynchronous preemption.

**Tasks:**
- [x] Implement Task structure with state machine (Created, Running, Suspended, Resumed, Completed, Failed)
- [x] Build work-stealing deques (crossbeam-deque with LIFO/FIFO strategy)
- [x] Create worker thread pool with dynamic worker count
- [x] Add task spawning (SPAWN opcode in both VM and worker executor)
- [x] Implement await mechanism (AWAIT opcode with polling loop)
- [x] Task completion tracking with waiter lists
- [x] Go-style asynchronous preemption (PreemptMonitor thread, 10ms threshold)
- [x] SchedulerLimits for inner VMs (max_workers, max_concurrent_tasks, max_stack_size, max_heap_size)
- [x] Nested task spawning support (tasks can spawn tasks)
- [x] Safepoint integration with preemption checks at loop headers
- [x] Fair scheduling across workers with random victim selection
- [x] Comprehensive integration testing (13 scheduler tests + 9 concurrency tests)

**Files:**
```rust
// crates/raya-core/src/scheduler/mod.rs
pub use scheduler::{Scheduler, SchedulerLimits};
pub use task::{Task, TaskHandle, TaskId, TaskState};
pub use worker::Worker;
pub use preempt::{PreemptMonitor, DEFAULT_PREEMPT_THRESHOLD};

// crates/raya-core/src/scheduler/scheduler.rs
pub struct Scheduler {
    workers: Vec<Worker>,
    tasks: Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,
    injector: Arc<Injector<Arc<Task>>>,
    safepoint: Arc<SafepointCoordinator>,
    preempt_monitor: PreemptMonitor,
    worker_count: usize,
    started: bool,
    limits: SchedulerLimits,
}

// crates/raya-core/src/scheduler/task.rs
pub struct Task {
    id: TaskId,
    state: Mutex<TaskState>,
    function_id: usize,
    module: Arc<Module>,
    stack: Mutex<Stack>,
    ip: AtomicUsize,
    result: Mutex<Option<Value>>,
    waiters: Mutex<Vec<TaskId>>,
    parent: Option<TaskId>,
    preempt_requested: AtomicBool,  // Async preemption
    start_time: Mutex<Option<Instant>>,
}

pub enum TaskState {
    Created, Running, Suspended, Resumed, Completed, Failed
}

// crates/raya-core/src/scheduler/worker.rs
// Worker threads execute tasks with SPAWN/AWAIT support
pub struct Worker {
    id: usize,
    stealers: Vec<Stealer<Arc<Task>>>,
    injector: Arc<Injector<Arc<Task>>>,
    tasks: Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,
    safepoint: Arc<SafepointCoordinator>,
    handle: Option<thread::JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
}

// crates/raya-core/src/scheduler/preempt.rs
// Go-style asynchronous preemption monitor (like sysmon)
pub struct PreemptMonitor {
    tasks: Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,
    threshold: Duration,  // Default: 10ms
    handle: Option<thread::JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
}

// crates/raya-core/src/vm/interpreter.rs
fn op_spawn(&mut self, func_index: usize, module: &Module) -> VmResult<()> {
    let task = Arc::new(Task::new(func_index, Arc::new(module.clone()), None));
    let task_id = self.scheduler.spawn(task)?;
    self.stack.push(Value::u64(task_id.as_u64()))?;
    Ok(())
}

fn op_await(&mut self) -> VmResult<()> {
    let task_id = TaskId::from_u64(self.stack.pop()?.as_u64()?);
    loop {
        let task = self.scheduler.get_task(task_id)?;
        match task.state() {
            TaskState::Completed => {
                self.stack.push(task.result().unwrap_or(Value::null()))?;
                return Ok(());
            }
            TaskState::Failed => return Err(...),
            _ => { self.safepoint().poll(); thread::sleep(...); }
        }
    }
}
```

**Tests:**
- `crates/raya-core/tests/scheduler_integration.rs` - 13 comprehensive scheduler tests
- `crates/raya-core/tests/concurrency_integration.rs` - 9 SPAWN/AWAIT integration tests (including nested task spawning)

**Reference:** `design/ARCHITECTURE.md` Section 4, `design/LANG.md` Section 14

### 1.12 Synchronization Primitives (Mutex) ✅

**Status:** Complete

**Goal:** Task-aware mutual exclusion with goroutine-style semantics.

**Tasks:**
- [x] Implement enhanced Mutex type with FIFO wait queue
- [x] Implement MutexId and MutexRegistry for global management
- [x] Add scheduler integration (block_on_mutex, resume_from_mutex)
- [x] MUTEX_LOCK / MUTEX_UNLOCK opcodes (NewMutex 0xE0, MutexLock 0xE1, MutexUnlock 0xE2)
- [x] Mutex serialization for VM snapshots
- [x] MutexGuard with RAII pattern for panic safety
- [x] Comprehensive testing (26 unit tests, all passing)

**Files:**
```rust
// crates/raya-core/src/sync/mutex.rs
pub struct Mutex {
    id: MutexId,
    owner: AtomicCell<Option<TaskId>>,
    wait_queue: Mutex<VecDeque<TaskId>>,
    lock_count: AtomicUsize,
}

impl Mutex {
    pub fn try_lock(&self, task_id: TaskId) -> Result<(), BlockReason>;
    pub fn unlock(&self, task_id: TaskId) -> Result<Option<TaskId>, MutexError>;
    pub fn serialize(&self) -> SerializedMutex;
    pub fn deserialize(data: SerializedMutex) -> Self;
}

// crates/raya-core/src/sync/guard.rs
pub struct MutexGuard<'a> { /* RAII auto-unlock */ }
```

**Reference:** `design/LANG.md` Section 15, `plans/milestone-1.12.md`

### 1.11 VM Snapshotting

**Status:** ✅ Complete

**Goal:** Pause, serialize, and resume entire VM state.

**Tasks:**
- [x] Define snapshot binary format (header, segments, checksums)
- [x] Implement snapshot writer with segment serialization
- [x] Implement snapshot reader with validation
- [x] Serialize task state (IP, stack, frames, blocked state)
- [x] Serialize heap snapshot (simplified for now)
- [x] Implement SHA-256 checksum validation
- [x] Test snapshot round-trip (14 integration tests, all passing)
- [x] Test Value serialization/deserialization

**Files:**
```rust
// crates/raya-core/src/vm/snapshot.rs
pub struct Snapshot {
    magic: [u8; 4],        // "SNAP"
    version: u32,
    contexts: Vec<ContextSnapshot>,
    checksum: u32,
}

pub fn snapshot_context(context: &VmContext) -> Result<Snapshot, SnapError> {
    // Ensure no GC in progress
    // Request STW pause
    // Serialize heap and metadata
    // Resume
}

pub fn restore_context(snapshot: Snapshot) -> Result<VmContext, RestoreError> {
    // Recreate heap
    // Restore pointer graph
    // Assign new context ID
}
```

**Reference:** `design/SNAPSHOTTING.md` (Full specification)

### 1.13 Inner VMs & Controllability

**Goal:** Nested VMs with resource limits and capability-based security.

**Tasks:**
- [ ] Implement Vm creation with VmOptions
- [ ] Resource accounting and enforcement
- [ ] Capability injection system
- [ ] Data marshalling across context boundaries
- [ ] Foreign handle system for cross-context references
- [ ] Context termination and cleanup

**Files:**
```rust
// crates/raya-core/src/vm/inner.rs
pub struct VmOptions {
    pub max_heap_bytes: Option<usize>,
    pub max_tasks: Option<usize>,
    pub max_step_budget: Option<usize>,
}

pub trait Capability {
    fn name(&self) -> &str;
    fn invoke(&self, args: &[Value]) -> Result<Value, VmError>;
}

// crates/raya-core/src/vm/marshal.rs
pub enum MarshalledValue {
    Null,
    Bool(bool),
    I32(i32),
    String(String),           // Deep copy
    Array(Vec<MarshalledValue>), // Deep copy
    Foreign(ForeignHandle),    // Opaque handle
}

pub fn marshal(value: Value, from_ctx: &VmContext) -> Result<MarshalledValue, MarshallError>;
pub fn unmarshal(marshalled: MarshalledValue, to_ctx: &mut VmContext) -> Result<Value, MarshallError>;
```

**Reference:** `design/INNER_VM.md` (Full specification), `plans/milestone-1.13.md` (Implementation plan)

### 1.14 Module System & Package Management

**Goal:** Efficient module system with global cache and bytecode-first storage (inspired by Bun and Go).

**Status:** 📋 Planned

**Architecture:**
```
~/.raya/cache/          # Global package cache
    ├── <hash>/         # Content-addressable storage
    │   └── module.rbin # Compiled bytecode only

my-project/
    ├── raya.toml       # Package descriptor
    ├── raya.lock       # Lockfile (exact versions)
    └── src/
```

**Key Features:**
- **Global cache:** Single `~/.raya/cache/` for all projects (no node_modules!)
- **Bytecode-first:** Store compiled `.rbin` files, not source
- **Content-addressable:** Packages identified by SHA-256 hash
- **Lockfile-based:** Reproducible builds with `raya.lock`
- **Zero duplication:** Same package version shared across projects
- **Fast:** Parallel downloads, incremental compilation
- **Offline-first:** Work without network once cached

**Import Syntax:**
```typescript
// Named package (from registry)
import { Logger } from "logging@1.2.3";

// URL import (decentralized)
import { utils } from "https://github.com/user/repo/v1.0.0";

// Local import
import { helper } from "./utils.raya";
```

**Crates:** `raya-core`, `raya-bytecode`, `raya-pm`

**Scope:** VM-side module system only (loading, linking, caching). Excludes compilation, CLI tools, and network operations.

**Phases:**
1. Module Loading & Bytecode Format (2 weeks)
2. Import Resolution (1 week)
3. Global Cache Management (1 week)
4. Module Linking (1 week)
5. Package Metadata & Lockfile (1 week)
6. Semver & Dependency Resolution (1 week)
7. Local Path Dependencies (3 days)
8. Testing & Error Handling (1 week)

**Detailed Plan:** See [plans/milestone-1.14.md](milestone-1.14.md) for complete task breakdown.

---

### 1.15 Native Module System

**Goal:** Enable Raya programs to call native functions written in C, C++, or Rust (similar to Node.js N-API).

**Status:** 🔄 In Progress

**Architecture:**
```
Raya Program (.raya)
    ↓ imports native:moduleName
Native Module (.so/.dylib/.dll)
    ↓ written in
C / C++ / Rust
```

**Tasks:**
- [x] Design native module system (NATIVE_BINDINGS.md)
- [x] Add comprehensive C++ examples
- [ ] Implement C API for native modules (raya-ffi crate)
  - [x] Value conversion functions (to/from native types)
  - [ ] Module registration API
  - [ ] Module builder functions
  - [ ] Array/Object accessor functions
  - [ ] Error creation functions
  - [ ] Context API
- [ ] Implement module loader in VM
  - [ ] Dynamic library loading (dlopen/LoadLibrary)
  - [ ] Symbol resolution (raya_module_init_NAME)
  - [ ] Version checking
  - [ ] Function registration in VM
  - [ ] Module path resolution ($RAYA_MODULE_PATH, ~/.raya/modules)
- [ ] Implement native function invocation
  - [ ] Call native functions from bytecode
  - [ ] Value marshalling (Raya <-> Native)
  - [ ] Error propagation
  - [ ] GC safety during native calls
- [ ] Implement Rust ergonomic API (raya-native crate)
  - [ ] #[function] proc-macro
  - [ ] #[module] proc-macro
  - [ ] Automatic type conversion
  - [ ] Result-based error handling
- [ ] Implement standard native modules
  - [ ] native:fs (file system operations)
  - [ ] native:crypto (hash, random)
- [ ] Documentation and examples
  - [ ] C API header (raya/module.h)
  - [ ] Example native module in C
  - [ ] Example native module in Rust
  - [ ] User guide for native module authors
- [ ] Testing
  - [ ] Module loading tests
  - [ ] Value marshalling tests
  - [ ] Error handling tests
  - [ ] Cross-language integration tests

**Example Usage:**

```typescript
// Raya program
import { hash } from "native:crypto";
const digest = hash("sha256", "hello world");
```

```rust
// Native module (Rust)
use raya_native::{module, function, Context, Error};

#[function]
fn hash(ctx: &Context, algorithm: String, data: String) -> Result<String, Error> {
    // Native implementation
}

#[module(name = "crypto", version = "1.0.0")]
mod crypto_module {
    exports! { hash }
}
```

**Files:**
```c
// raya-ffi/include/raya/module.h - C API for native modules
typedef struct RayaContext RayaContext;
typedef struct RayaValue RayaValue;
typedef struct RayaModule RayaModule;

typedef RayaValue* (*RayaNativeFunction)(
    RayaContext* ctx,
    RayaValue** args,
    size_t argc
);

#define RAYA_MODULE_INIT(name) \
    __attribute__((visibility("default"))) \
    RayaModule* raya_module_init_##name(void)

// Value conversion
const char* raya_value_to_string(RayaValue* value);
RayaValue* raya_value_from_string(RayaContext* ctx, const char* str);
// ... more conversion functions

// Module registration
RayaModuleBuilder* raya_module_builder_new(const char* name, const char* version);
void raya_module_add_function(RayaModuleBuilder* builder, const char* name,
                               RayaNativeFunction func, size_t arity);
RayaModule* raya_module_builder_finish(RayaModuleBuilder* builder);
```

```rust
// raya-core/src/native/loader.rs - Dynamic library loader
pub struct NativeModuleLoader {
    search_paths: Vec<PathBuf>,
    loaded_modules: HashMap<String, LoadedModule>,
}

impl NativeModuleLoader {
    pub fn load(&mut self, name: &str) -> Result<&LoadedModule, LoadError>;
    fn find_library(&self, name: &str) -> Option<PathBuf>;
    fn load_symbols(&self, lib: &Library) -> Result<ModuleDescriptor, LoadError>;
}
```

**ABI Stability:**
- Native module ABI follows semantic versioning (MAJOR.MINOR.PATCH)
- MAJOR version change = breaking ABI changes
- MINOR version change = new functions (backward compatible)
- PATCH version change = bug fixes (no API/ABI changes)
- Current ABI version: 1.0.0

**Reference:** `design/NATIVE_BINDINGS.md` (Complete specification)

### 1.16 Integration Testing & Validation

**Goal:** Comprehensive test coverage for all VM systems.

**Status:** ✅ Complete

**Tasks:**
- [x] Unit tests for each opcode (66 tests)
- [x] Integration tests for bytecode execution
- [x] GC stress tests (allocation patterns, memory pressure) (8 tests + 1 ignored)
- [x] Multi-context isolation tests (13 tests)
- [x] Concurrent task execution tests (16 tests)
- [x] Snapshot/restore validation (23 tests)
- [x] Endianness-aware snapshot system with byte-swapping
- [ ] Inner VM security boundary tests
- [ ] Resource limit enforcement tests
- [ ] Performance benchmarks
- [ ] End-to-end integration scenarios

**Files:**
```
crates/raya-core/tests/
├── opcodes.rs            # Individual opcode tests (66 tests) ✅
├── gc_stress.rs          # GC correctness and stress tests (8 tests + 1 ignored) ✅
├── multi_context_isolation.rs  # Multi-context isolation (13 tests) ✅
├── concurrency_integration.rs  # Concurrency tests (16 tests) ✅
├── snapshot_restore_validation.rs  # Snapshot/restore validation (23 tests) ✅
├── inner_vm.rs           # Inner VM isolation tests
├── integration.rs        # End-to-end scenarios
└── benchmarks.rs         # Performance measurements
```

**Test Coverage Goals:**
- >90% code coverage for core VM
- >85% for GC and memory management
- Stress tests running for hours without crashes
- All design examples from specification working

**Completed Features:**
- 524+ workspace tests passing
- Endianness-aware snapshot system with cross-platform support
- Comprehensive opcode test coverage
- GC stress testing with allocation patterns
- Multi-context isolation validation
- Concurrent task execution testing
- Snapshot/restore round-trip validation with checksums

---

## Phase 2: Parser & Type Checker

**Goal:** Parse Raya source code and perform sound type checking.

### 2.1 Lexer

**Crate:** `raya-parser`

**Tasks:**
- [ ] Define token types
- [ ] Implement lexer using `logos` or hand-written
- [ ] Handle keywords, identifiers, literals
- [ ] Track source locations for error reporting
- [ ] Support string templates

**Files:**
```rust
// crates/raya-parser/src/lexer.rs
use logos::Logos;

#[derive(Logos, Debug, PartialEq)]
pub enum Token {
    #[token("function")]
    Function,

    #[token("let")]
    Let,

    #[token("const")]
    Const,

    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
    Identifier,

    #[regex(r"\d+")]
    IntLiteral,

    // ... all tokens
}

pub struct Lexer<'a> {
    source: &'a str,
    tokens: Vec<(Token, Span)>,
}
```

**Reference:** `design/LANG.md` Section 2

### 2.2 AST Definition

**Tasks:**
- [ ] Define AST node types
- [ ] Implement visitor pattern
- [ ] Add source span tracking
- [ ] Create pretty-printer for debugging

**Files:**
```rust
// crates/raya-parser/src/ast.rs
pub struct Module {
    pub statements: Vec<Statement>,
    pub span: Span,
}

pub enum Statement {
    FunctionDecl(FunctionDecl),
    ClassDecl(ClassDecl),
    LetDecl(LetDecl),
    Expression(Expression),
}

pub enum Expression {
    Literal(Literal),
    Identifier(String),
    BinaryOp { op: BinOp, left: Box<Expr>, right: Box<Expr> },
    Call { callee: Box<Expr>, args: Vec<Expr> },
    // ... all expression types
}

pub enum Type {
    Number,
    String,
    Boolean,
    Null,
    Union(Vec<Type>),
    Function(FunctionType),
    Class(ClassType),
    Interface(InterfaceType),
    // ...
}
```

**Reference:** `design/LANG.md` All sections

### 2.3 Parser

**Tasks:**
- [ ] Implement recursive descent parser
- [ ] Handle operator precedence
- [ ] Parse function declarations
- [ ] Parse class declarations
- [ ] Parse type annotations
- [ ] Provide helpful error messages

**Files:**
```rust
// crates/raya-parser/src/parser.rs
pub struct Parser<'a> {
    lexer: Lexer<'a>,
    current: usize,
    errors: Vec<ParseError>,
}

impl<'a> Parser<'a> {
    pub fn parse_module(&mut self) -> Result<Module, Vec<ParseError>>;

    fn parse_statement(&mut self) -> Result<Statement, ParseError>;
    fn parse_expression(&mut self) -> Result<Expression, ParseError>;
    fn parse_type(&mut self) -> Result<Type, ParseError>;

    // Precedence climbing for binary operators
    fn parse_binary_expr(&mut self, min_prec: u8) -> Result<Expression, ParseError>;
}
```

**Reference:** `design/LANG.md` Sections 6, 7, 8

### 2.4 Type System

**Crate:** `raya-types`

**Tasks:**
- [ ] Implement type representation
- [ ] Build type inference engine
- [ ] Add subtyping rules
- [ ] Implement discriminated union checking
- [ ] Track type parameters for generics

**Files:**
```rust
// crates/raya-types/src/lib.rs
pub mod types;
pub mod inference;
pub mod unify;
pub mod subtyping;

// crates/raya-types/src/types.rs
pub enum Type {
    Primitive(PrimitiveType),
    Union(UnionType),
    Function(FunctionType),
    Class(ClassType),
    Interface(InterfaceType),
    TypeVar(TypeVar),  // For inference
    Generic(GenericType),
}

pub struct UnionType {
    pub variants: Vec<Type>,
    pub discriminant: Option<DiscriminantInfo>,
}

pub struct DiscriminantInfo {
    pub field: String,
    pub values: HashMap<String, Type>,
}
```

**Reference:** `design/LANG.md` Section 4

### 2.5 Type Checker

**Tasks:**
- [ ] Build symbol table
- [ ] Implement type checking for expressions
- [ ] Check function signatures
- [ ] Validate class definitions
- [ ] Enforce discriminated unions
- [ ] Check exhaustiveness
- [ ] Ban `typeof`, `instanceof`, `any`

**Files:**
```rust
// crates/raya-types/src/checker.rs
pub struct TypeChecker {
    symbols: SymbolTable,
    errors: Vec<TypeError>,
    current_scope: ScopeId,
}

impl TypeChecker {
    pub fn check_module(&mut self, module: &Module) -> Result<TypedModule, Vec<TypeError>>;

    fn check_statement(&mut self, stmt: &Statement) -> Result<TypedStatement, TypeError>;
    fn check_expression(&mut self, expr: &Expression) -> Result<(TypedExpression, Type), TypeError>;

    fn check_discriminated_union(&self, union: &UnionType) -> Result<(), TypeError>;
    fn check_exhaustiveness(&self, union: &UnionType, cases: &[String]) -> Result<(), TypeError>;
}

pub struct SymbolTable {
    scopes: Vec<Scope>,
    symbols: HashMap<String, Symbol>,
}

pub struct Symbol {
    name: String,
    ty: Type,
    kind: SymbolKind,
    span: Span,
}

pub enum SymbolKind {
    Variable,
    Function,
    Class,
    Interface,
    TypeAlias,
}
```

**Reference:** `design/LANG.md` Sections 4.7, 13A

### 2.6 Discriminant Inference

**Tasks:**
- [ ] Implement discriminant field detection
- [ ] Use priority order (kind > type > tag > variant > alphabetical)
- [ ] Validate all variants have common discriminant
- [ ] Generate compile errors for ambiguous unions

**Files:**
```rust
// crates/raya-types/src/discriminant.rs
pub struct DiscriminantInference;

impl DiscriminantInference {
    pub fn infer(union: &UnionType) -> Result<String, TypeError> {
        // Algorithm from LANG.md Section 17.6
        let common_fields = self.find_common_literal_fields(union);

        if common_fields.is_empty() {
            return Err(TypeError::NoDiscriminant);
        }

        // Priority: kind > type > tag > variant > alphabetical
        if common_fields.contains("kind") {
            return Ok("kind".to_string());
        }
        // ... etc
    }
}
```

**Reference:** `design/LANG.md` Section 17.6

### 2.7 Bare Union Transformation

**Tasks:**
- [ ] Detect bare primitive unions (`string | number`)
- [ ] Transform to `{ $type, $value }` representation
- [ ] Insert boxing/unboxing code automatically
- [ ] Prevent user access to `$type` and `$value`

**Files:**
```rust
// crates/raya-types/src/bare_union.rs
pub struct BareUnionTransform;

impl BareUnionTransform {
    pub fn transform(ty: &Type) -> Option<Type> {
        if let Type::Union(union) = ty {
            if self.is_bare_primitive_union(union) {
                return Some(self.create_boxed_union(union));
            }
        }
        None
    }

    fn is_bare_primitive_union(&self, union: &UnionType) -> bool {
        union.variants.iter().all(|v| matches!(v,
            Type::Primitive(PrimitiveType::String |
                           PrimitiveType::Number |
                           PrimitiveType::Boolean |
                           PrimitiveType::Null)
        ))
    }
}
```

**Reference:** `design/LANG.md` Section 4.3

### 2.8 Error Reporting

**Tasks:**
- [ ] Create helpful error messages
- [ ] Show source code context
- [ ] Suggest fixes (e.g., "use discriminated union instead of typeof")
- [ ] Support multiple error formats (human, JSON)

**Files:**
```rust
// crates/raya-parser/src/error.rs
pub struct ParseError {
    pub kind: ErrorKind,
    pub span: Span,
    pub message: String,
    pub suggestion: Option<String>,
}

impl ParseError {
    pub fn format(&self, source: &str) -> String {
        // Pretty-print with source context
    }
}
```

---

## Phase 3: Compiler & Code Generation

**Goal:** Translate typed AST to bytecode.

### 3.1 IR (Intermediate Representation)

**Crate:** `raya-compiler`

**Tasks:**
- [ ] Design IR structure (SSA form or three-address code)
- [ ] Lower AST to IR
- [ ] Implement basic optimizations (constant folding, DCE)
- [ ] Add type information to IR

**Files:**
```rust
// crates/raya-compiler/src/ir.rs
pub enum IrInstr {
    Assign { dest: Register, value: IrValue },
    BinaryOp { dest: Register, op: BinOp, left: Register, right: Register },
    Call { dest: Option<Register>, func: FunctionId, args: Vec<Register> },
    Jump { target: BasicBlockId },
    Branch { cond: Register, then_block: BasicBlockId, else_block: BasicBlockId },
    Return { value: Option<Register> },
}

pub struct BasicBlock {
    id: BasicBlockId,
    instructions: Vec<IrInstr>,
    terminator: Terminator,
}
```

### 3.2 Monomorphization

**Tasks:**
- [ ] Collect all generic instantiations
- [ ] Generate specialized versions of generic functions
- [ ] Generate specialized versions of generic classes
- [ ] Track monomorphized types

**Files:**
```rust
// crates/raya-compiler/src/monomorphize.rs
pub struct Monomorphizer {
    instantiations: HashMap<(FunctionId, Vec<Type>), FunctionId>,
}

impl Monomorphizer {
    pub fn monomorphize(&mut self, module: &TypedModule) -> MonomorphizedModule {
        // Replace all generic types with concrete types
        // Generate specialized functions/classes
    }
}
```

**Reference:** `design/LANG.md` Section 13.7

### 3.3 Code Generation

**Tasks:**
- [ ] Implement bytecode emitter
- [ ] Generate code for all expression types
- [ ] Handle control flow (if, while, switch)
- [ ] Emit function prologues/epilogues
- [ ] Generate vtables for classes
- [ ] Emit closures with captured variables

**Files:**
```rust
// crates/raya-compiler/src/codegen.rs
pub struct CodeGenerator {
    module: Module,
    current_function: Option<FunctionId>,
    bytecode: Vec<u8>,
    constant_pool: ConstantPool,
}

impl CodeGenerator {
    pub fn generate(&mut self, ir_module: &IrModule) -> Module;

    fn emit_opcode(&mut self, opcode: Opcode);
    fn emit_u32(&mut self, value: u32);
    fn add_constant(&mut self, constant: Constant) -> u32;

    fn generate_function(&mut self, func: &IrFunction);
    fn generate_expression(&mut self, expr: &IrExpr);
}
```

**Reference:** `design/MAPPING.md` All sections

### 3.4 Match Inlining

**Tasks:**
- [ ] Detect `match()` calls
- [ ] Inline match logic directly
- [ ] Generate switch-based bytecode for discriminants
- [ ] Optimize for exhaustiveness (no unreachable trap)

**Files:**
```rust
// crates/raya-compiler/src/match_inline.rs
pub struct MatchInliner;

impl MatchInliner {
    pub fn inline_match(&self, call: &CallExpr) -> Option<InlinedMatch> {
        // Check if this is a match() call
        // Generate inline bytecode for switch on discriminant
        // See MAPPING.md Section 15.5, 15.6
    }
}
```

**Reference:** `design/MAPPING.md` Sections 15.5, 15.6

### 3.5 JSON Codegen

**Tasks:**
- [ ] Detect `JSON.encode()` and `JSON.decode<T>()` calls
- [ ] Generate specialized encoder/decoder functions
- [ ] Handle bare unions in JSON
- [ ] Emit validation code for decoders

**Files:**
```rust
// crates/raya-compiler/src/json_codegen.rs
pub struct JsonCodegen;

impl JsonCodegen {
    pub fn generate_encoder(&self, ty: &Type) -> FunctionId;
    pub fn generate_decoder(&self, ty: &Type) -> FunctionId;
}
```

**Reference:** `design/LANG.md` Section 17.7

### 3.6 Module Compilation

**Tasks:**
- [ ] Resolve module dependencies
- [ ] Handle standard library modules (`raya:std`, `raya:json`)
- [ ] Support relative and absolute imports
- [ ] Detect circular dependencies (error)

**Files:**
```rust
// crates/raya-compiler/src/module_resolver.rs
pub struct ModuleResolver {
    resolved: HashMap<PathBuf, ModuleId>,
    stdlib: StdlibModules,
}

impl ModuleResolver {
    pub fn resolve(&mut self, import: &str, from: &Path) -> Result<ModuleId, ResolveError>;
}
```

**Reference:** `design/LANG.md` Section 16.8

### 3.7 Optimization

**Tasks:**
- [ ] Constant folding
- [ ] Dead code elimination
- [ ] Inline small functions
- [ ] Optimize typed arithmetic (IADD vs FADD vs NADD)
- [ ] Remove redundant type checks

**Files:**
```rust
// crates/raya-compiler/src/optimize.rs
pub struct Optimizer;

impl Optimizer {
    pub fn optimize(&self, ir: &mut IrModule) {
        self.constant_folding(ir);
        self.dead_code_elimination(ir);
        self.inline_functions(ir);
    }
}
```

### 3.8 Testing

**Tasks:**
- [ ] Write tests for each language construct
- [ ] Test monomorphization
- [ ] Test match inlining
- [ ] Test JSON codegen
- [ ] Compare output with expected bytecode

**Files:**
```
crates/raya-compiler/tests/
├── functions.rs
├── classes.rs
├── generics.rs
├── unions.rs
└── modules.rs
```

---

## Phase 4: Standard Library

**Goal:** Implement core runtime functionality.

### 4.1 Core Types

**Location:** `stdlib/core.raya`

**Tasks:**
- [ ] Implement `Error` class
- [ ] Define `Result<T, E>` type
- [ ] Define `Task<T>` interface
- [ ] Add `PromiseLike<T>` compatibility

**Files:**
```typescript
// stdlib/core.raya
export class Error {
  constructor(public message: string) {}
  stack?: string;
}

export type Result<T, E> =
  | { status: "ok"; value: T }
  | { status: "error"; error: E };

export interface Task<T> extends PromiseLike<T> {
  // No additional methods
}
```

**Reference:** `design/STDLIB.md` Section 1

### 4.2 raya:std Module

**Location:** `stdlib/std.raya`

**Tasks:**
- [ ] Implement `match()` function (compile-time magic)
- [ ] Implement `sleep()` (native)
- [ ] Implement `all()` for task aggregation
- [ ] Implement `race()` for task racing

**Files:**
```typescript
// stdlib/std.raya
export function match<T, R>(
  value: T,
  handlers: MatchHandlers<T, R>
): R {
  // Compiler intrinsic - replaced during compilation
  throw new Error("match() should be inlined by compiler");
}

// Native implementations
declare function sleep(ms: number): Task<void>;
declare function all<T>(tasks: Task<T>[]): Task<T[]>;
declare function race<T>(tasks: Task<T>[]): Task<T>;
```

**Native Implementation:**
```rust
// crates/raya-stdlib/src/std.rs
pub fn sleep(vm: &mut Vm, ms: f64) -> Result<TaskId, VmError> {
    let task = vm.scheduler.spawn_delayed(Duration::from_millis(ms as u64));
    Ok(task)
}

pub fn all(vm: &mut Vm, tasks: Vec<TaskId>) -> Result<TaskId, VmError> {
    let task = vm.scheduler.all(tasks);
    Ok(task)
}
```

**Reference:** `design/STDLIB.md` Section 2

### 4.3 raya:json Module

**Location:** `stdlib/json.raya`

**Tasks:**
- [ ] Define `JSON` class with `encode()` and `decode()`
- [ ] Both are compiler intrinsics
- [ ] Actual implementation generated per-type

**Files:**
```typescript
// stdlib/json.raya
export class JSON {
  static encode<T>(value: T): Result<string, Error> {
    // Compiler generates specialized encoder
    throw new Error("JSON.encode() should be replaced by compiler");
  }

  static decode<T>(input: string): Result<T, Error> {
    // Compiler generates specialized decoder
    throw new Error("JSON.decode() should be replaced by compiler");
  }
}
```

**Reference:** `design/STDLIB.md` Section 3

### 4.4 raya:json/internal Module

**Tasks:**
- [ ] Implement `JsonValue` type
- [ ] Implement `parseJson()` native function
- [ ] Build JSON parser in Rust

**Files:**
```typescript
// stdlib/json_internal.raya
export type JsonValue =
  | { kind: "null" }
  | { kind: "boolean"; value: boolean }
  | { kind: "number"; value: number }
  | { kind: "string"; value: string }
  | { kind: "array"; value: JsonValue[] }
  | { kind: "object"; value: Map<string, JsonValue> };

declare function parseJson(input: string): Result<JsonValue, Error>;
```

```rust
// crates/raya-stdlib/src/json.rs
pub fn parse_json(input: &str) -> Result<Value, VmError> {
    // Use serde_json or custom parser
    // Convert to Raya JsonValue representation
}
```

**Reference:** `design/STDLIB.md` Section 4

### 4.5 raya:reflect Module (Optional)

**Tasks:**
- [ ] Implement reflection API when `--emit-reflection` flag is set
- [ ] Add `REFLECT_*` opcodes
- [ ] Embed type metadata in bytecode
- [ ] Implement all reflection functions

**Files:**
```rust
// crates/raya-core/src/reflect.rs
#[cfg(feature = "reflection")]
pub mod reflect {
    pub fn type_of(vm: &Vm, value: Value) -> TypeInfo { /* ... */ }
    pub fn type_info<T>() -> TypeInfo { /* ... */ }
    pub fn get_property(obj: GcPtr<Object>, name: &str) -> Option<Value> { /* ... */ }
    // ... all reflection functions
}
```

**Reference:** `design/STDLIB.md` Section 5, `design/LANG.md` Section 18

### 4.6 Built-in Types

**Tasks:**
- [ ] Implement String methods (native)
- [ ] Implement Number methods (native)
- [ ] Implement Array methods (native)
- [ ] Implement Map class (native)
- [ ] Implement Set class (native)
- [ ] Implement Mutex class (native)

**Files:**
```rust
// crates/raya-stdlib/src/string.rs
pub fn string_to_upper_case(s: &str) -> String {
    s.to_uppercase()
}

pub fn string_substring(s: &str, start: usize, end: Option<usize>) -> String {
    // ...
}

// crates/raya-stdlib/src/array.rs
pub fn array_push(arr: &mut Vec<Value>, item: Value) {
    arr.push(item);
}

pub fn array_map(vm: &mut Vm, arr: &[Value], f: FunctionRef) -> Result<Vec<Value>, VmError> {
    // ...
}
```

**Reference:** `design/STDLIB.md` Section 7

### 4.7 Console API

**Tasks:**
- [ ] Implement `console.log()` (native)
- [ ] Implement `console.error()` (native)
- [ ] Implement `console.warn()` and `console.info()` (aliases)

**Files:**
```rust
// crates/raya-stdlib/src/console.rs
pub fn console_log(args: &[Value]) {
    for arg in args {
        print!("{} ", arg.to_string());
    }
    println!();
}

pub fn console_error(args: &[Value]) {
    for arg in args {
        eprint!("{} ", arg.to_string());
    }
    eprintln!();
}
```

**Reference:** `design/STDLIB.md` Section 6

### 4.8 Testing

**Tasks:**
- [ ] Test each stdlib function
- [ ] Test task utilities (sleep, all, race)
- [ ] Test JSON parsing and encoding
- [ ] Benchmark stdlib performance

---

## Phase 5: Package Manager

**Goal:** Create `raya-pm` for managing Raya packages.

### 5.1 Package Format

**Tasks:**
- [ ] Define `package.json` format (or `raya.toml`)
- [ ] Support semantic versioning
- [ ] Define dependency specification
- [ ] Add metadata (author, license, etc.)

**Files:**
```toml
# raya.toml
[package]
name = "my-package"
version = "1.0.0"
authors = ["Your Name <you@example.com>"]
license = "MIT"
description = "A sample Raya package"

[dependencies]
http = "2.1.0"
json = "1.0.0"

[dev-dependencies]
test-framework = "0.5.0"
```

### 5.2 Package Registry

**Tasks:**
- [ ] Design registry API
- [ ] Implement local package cache
- [ ] Support git dependencies
- [ ] Add lock file (`raya.lock`)

**Files:**
```rust
// crates/raya-pm/src/registry.rs
pub struct Registry {
    url: String,
    cache: PathBuf,
}

impl Registry {
    pub fn fetch(&self, package: &str, version: &str) -> Result<Package, RegistryError>;
    pub fn search(&self, query: &str) -> Result<Vec<PackageInfo>, RegistryError>;
}
```

### 5.3 CLI Commands

**Crate:** `raya-pm`

**Tasks:**
- [ ] `raya-pm init` - Initialize new project
- [ ] `raya-pm install` - Install dependencies
- [ ] `raya-pm add <package>` - Add dependency
- [ ] `raya-pm remove <package>` - Remove dependency
- [ ] `raya-pm publish` - Publish to registry
- [ ] `raya-pm search <query>` - Search packages

**Files:**
```rust
// crates/raya-pm/src/main.rs
use clap::{Parser, Subcommand};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Init,
    Install,
    Add { package: String },
    Remove { package: String },
    Publish,
    Search { query: String },
}
```

### 5.4 Dependency Resolution

**Tasks:**
- [ ] Implement SAT-based dependency resolver
- [ ] Handle version conflicts
- [ ] Generate lock file
- [ ] Support workspace projects

**Files:**
```rust
// crates/raya-pm/src/resolver.rs
pub struct DependencyResolver {
    packages: HashMap<String, Vec<Version>>,
}

impl DependencyResolver {
    pub fn resolve(&self, deps: &[Dependency]) -> Result<ResolvedDeps, ResolveError>;
}
```

### 5.5 Testing

**Tasks:**
- [ ] Test dependency resolution
- [ ] Test package installation
- [ ] Test lock file generation
- [ ] Integration tests with real packages

---

## Phase 6: Testing System

**Goal:** Build a test framework for Raya programs.

### 6.1 Test Framework Design

**Tasks:**
- [ ] Define test function syntax
- [ ] Support `describe` and `it` blocks
- [ ] Add assertions (`assert`, `assertEqual`, etc.)
- [ ] Support async tests

**Example:**
```typescript
// example.test.raya
import { describe, it, assert } from "raya:test";

describe("Math operations", () => {
  it("should add numbers correctly", () => {
    assert(1 + 1 === 2);
  });

  it("should handle async operations", async () => {
    const result = await fetchData();
    assert(result !== null);
  });
});
```

### 6.2 Test Runner

**Crate:** `raya-test`

**Tasks:**
- [ ] Discover test files
- [ ] Execute tests in parallel
- [ ] Report results (pass/fail/skip)
- [ ] Generate coverage reports
- [ ] Support watch mode

**Files:**
```rust
// crates/raya-test/src/runner.rs
pub struct TestRunner {
    tests: Vec<Test>,
    reporter: Box<dyn Reporter>,
}

impl TestRunner {
    pub fn run(&mut self) -> TestResults {
        // Execute all tests
        // Collect results
    }
}

pub struct TestResults {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub duration: Duration,
}
```

### 6.3 Assertions

**Location:** `stdlib/test.raya`

**Tasks:**
- [ ] Implement `assert()`
- [ ] Implement `assertEqual()`
- [ ] Implement `assertThrows()`
- [ ] Implement `assertAsync()`

**Files:**
```typescript
// stdlib/test.raya
export function assert(condition: boolean, message?: string): void {
  if (!condition) {
    throw new Error(message || "Assertion failed");
  }
}

export function assertEqual<T>(actual: T, expected: T, message?: string): void {
  if (actual !== expected) {
    throw new Error(message || `Expected ${expected}, got ${actual}`);
  }
}
```

### 6.4 Mocking & Stubbing

**Tasks:**
- [ ] Add basic mocking capabilities
- [ ] Support function spies
- [ ] Track function calls

**Files:**
```typescript
// stdlib/test.raya
export class Mock<T> {
  calls: any[][] = [];

  create(fn: T): T {
    // Return wrapped function that tracks calls
  }
}
```

### 6.5 Coverage

**Tasks:**
- [ ] Instrument bytecode for coverage
- [ ] Track line execution
- [ ] Generate coverage reports (HTML, JSON)

**Files:**
```rust
// crates/raya-test/src/coverage.rs
pub struct CoverageTracker {
    lines: HashMap<FileId, HashSet<usize>>,
}

impl CoverageTracker {
    pub fn record_line(&mut self, file: FileId, line: usize);
    pub fn generate_report(&self) -> CoverageReport;
}
```

### 6.6 Testing

**Tasks:**
- [ ] Test the test framework itself
- [ ] Write example tests
- [ ] Benchmark test execution performance

---

## Phase 7: Tooling & Developer Experience

**Goal:** Build developer tools for productivity.

### 7.1 CLI Tool (rayac)

**Crate:** `raya-cli`

**Tasks:**
- [ ] `rayac compile <file>` - Compile to bytecode
- [ ] `rayac run <file>` - Compile and execute
- [ ] `rayac check <file>` - Type check only
- [ ] `rayac build` - Build project
- [ ] `rayac test` - Run tests
- [ ] `rayac fmt` - Format code
- [ ] `rayac --version` - Show version

**Files:**
```rust
// crates/raya-cli/src/main.rs
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rayac")]
#[command(about = "Raya compiler and toolchain")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Compile { file: PathBuf },
    Run { file: PathBuf, args: Vec<String> },
    Check { file: PathBuf },
    Build,
    Test,
    Fmt { files: Vec<PathBuf> },
}
```

### 7.2 REPL

**Tasks:**
- [ ] Build interactive REPL
- [ ] Support multi-line input
- [ ] Add tab completion
- [ ] Show type information
- [ ] History and editing support

**Files:**
```rust
// crates/raya-cli/src/repl.rs
use rustyline::Editor;

pub struct Repl {
    vm: Vm,
    editor: Editor<()>,
}

impl Repl {
    pub fn run(&mut self) {
        loop {
            let line = self.editor.readline("raya> ");
            // Parse, type check, compile, execute
        }
    }
}
```

### 7.3 Code Formatter

**Crate:** `raya-fmt`

**Tasks:**
- [ ] Implement AST-based formatter
- [ ] Support configuration file
- [ ] Match common style guides (Prettier-like)

**Files:**
```rust
// crates/raya-fmt/src/lib.rs
pub struct Formatter {
    config: FormatConfig,
}

impl Formatter {
    pub fn format(&self, ast: &Module) -> String {
        // Pretty-print AST
    }
}
```

### 7.4 Language Server (LSP)

**Crate:** `raya-lsp`

**Tasks:**
- [ ] Implement LSP protocol
- [ ] Add diagnostics (errors, warnings)
- [ ] Add auto-completion
- [ ] Add go-to-definition
- [ ] Add hover information
- [ ] Add rename refactoring

**Files:**
```rust
// crates/raya-lsp/src/main.rs
use tower_lsp::{Server, LspService};

struct RayaLanguageServer {
    // ...
}

#[tower_lsp::async_trait]
impl LanguageServer for RayaLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult>;
    async fn did_open(&self, params: DidOpenTextDocumentParams);
    async fn completion(&self, params: CompletionParams) -> Result<CompletionResponse>;
    // ... all LSP methods
}
```

### 7.5 Debugger

**Tasks:**
- [ ] Add bytecode debugging support
- [ ] Support breakpoints
- [ ] Add step-through execution
- [ ] Inspect variables and stack
- [ ] Integrate with DAP (Debug Adapter Protocol)

**Files:**
```rust
// crates/raya-debugger/src/lib.rs
pub struct Debugger {
    vm: Vm,
    breakpoints: HashSet<(FunctionId, usize)>,
}

impl Debugger {
    pub fn set_breakpoint(&mut self, location: Location);
    pub fn step(&mut self);
    pub fn continue_execution(&mut self);
    pub fn inspect_variable(&self, name: &str) -> Option<Value>;
}
```

### 7.6 Documentation Generator

**Tasks:**
- [ ] Parse doc comments
- [ ] Generate HTML documentation
- [ ] Support markdown in comments
- [ ] Create API reference

**Files:**
```rust
// crates/raya-doc/src/lib.rs
pub struct DocGenerator;

impl DocGenerator {
    pub fn generate(&self, module: &TypedModule) -> Documentation {
        // Extract doc comments
        // Generate HTML
    }
}
```

---

## Milestones

### Milestone 1: Core VM with Integrated Memory System (Weeks 1-6)
- [x] Project setup
- [x] Bytecode definitions
- [ ] **Memory system (Phase 1):**
  - [ ] Per-context heaps with precise GC
  - [ ] VM snapshotting infrastructure
  - [ ] Inner VM support
  - [ ] Safepoint coordination
- [ ] Basic bytecode interpreter
- [ ] Stack and frame management
- [ ] Simple object model

**Goal:** Execute programs with full memory management, GC, and VM control.

**Key Achievement:** Integrated foundation for all advanced features.

```typescript
function main(): void {
  console.log("Hello, World!");
}
```

### Milestone 2: Core Features (Weeks 5-12)
- [ ] Full expression support
- [ ] Functions and closures
- [ ] Classes and objects
- [ ] Basic type checking
- [ ] Garbage collection

**Goal:** Run non-trivial programs with functions and objects.

### Milestone 3: Type System (Weeks 13-20)
- [ ] Complete type inference
- [ ] Discriminated unions
- [ ] Exhaustiveness checking
- [ ] Bare union transformation
- [ ] Generics and monomorphization

**Goal:** Enforce sound type safety.

### Milestone 4: Concurrency (Weeks 21-28)
- [x] Task scheduler (✅ Milestone 1.10 complete)
- [x] Work-stealing (✅ Milestone 1.10 complete)
- [x] Async/await (✅ SPAWN/AWAIT opcodes implemented)
- [x] Mutex support (✅ Milestone 1.12 complete)
- [ ] Task utilities (sleep, all, race)

**Goal:** Run concurrent programs efficiently.

**Progress:** Core scheduler, async/await, and synchronization primitives complete with Go-style preemption, nested task spawning, Task-aware Mutex with FIFO fairness, and comprehensive testing.

### Milestone 5: Standard Library (Weeks 29-32)
- [ ] Core types
- [ ] raya:std module
- [ ] raya:json module
- [ ] Built-in type methods
- [ ] Console API

**Goal:** Provide essential runtime functionality.

### Milestone 6: Tooling (Weeks 33-40)
- [ ] CLI tool (rayac)
- [ ] Package manager (raya-pm)
- [ ] Test framework
- [ ] REPL
- [ ] Code formatter

**Goal:** Productive developer experience.

### Milestone 7: Advanced Features & Optimization (Weeks 41-48)
- [ ] LSP server
- [ ] Debugger
- [ ] Documentation generator
- [ ] JIT compilation for hot code
- [ ] Reflection system (optional)
- [ ] **Phase 2 GC:** Generational young-gen copying collector
- [ ] **Phase 3 GC:** Incremental/concurrent marking (if needed)

**Goal:** Production-ready performance and tooling.

**GC Evolution:**
- Phase 2 adds young generation with write barriers
- Significantly improves throughput for object-heavy code
- Phase 3 only if pause times become bottleneck

### Milestone 8: Production Ready (Weeks 49-52)
- [ ] Performance optimization
- [ ] Security audit
- [ ] Documentation
- [ ] Example projects
- [ ] Public release

**Goal:** Stable 1.0 release.

---

## Dependencies Graph

```
raya-bytecode
    ↓
raya-core → raya-stdlib
    ↓              ↓
raya-types   raya-test
    ↓              ↓
raya-parser      ↓
    ↓              ↓
raya-compiler    ↓
    ↓              ↓
raya-cli ←-------┘
    ↓
raya-lsp
```

---

## Next Steps

1. **Set up project structure** - Create all crates
2. **Implement bytecode definitions** - Complete `raya-bytecode`
3. **Build interpreter core** - Start with `raya-core`
4. **Test with hand-written bytecode** - Validate VM works
5. **Build lexer and parser** - Start `raya-parser`
6. **Implement type checker** - Complete `raya-types`
7. **Continue with compilation pipeline** - Work on `raya-compiler`

---

**Status:** Planning Complete
**Version:** v0.1 (Implementation Plan)
**Last Updated:** 2026-01-04
