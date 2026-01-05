# Milestone 7.2: Inner VM & Controllability

**Goal:** Implement nested VmContexts with resource isolation, capability control, and shared scheduler execution

**Depends on:**
- Milestone 2: Core VM features (heap, GC)
- Milestone 4: Task scheduler and concurrency primitives
- Milestone 7.1: Snapshotting (optional but recommended)

**Reference:** [design/INNER_VM.md](../design/INNER_VM.md)

---

## Overview

This milestone implements **instantiable virtual machines** that can be created, controlled, and resource-limited from within Raya code. Each `VmContext` provides strong isolation guarantees while sharing the global scheduler and worker threads.

### Key Features

- Multiple isolated VmContexts on shared scheduler
- Heap, type, and capability isolation per context
- Resource limits (memory, CPU fuel, max tasks)
- Capability-based security model
- Data marshalling across context boundaries
- Per-context snapshotting
- Fair scheduling with anti-starvation guarantees

---

## Task Breakdown

### Task 1: VmContext Infrastructure

**Goal:** Implement core VmContext abstraction and registry

**Subtasks:**

1. **Define VmContext ID System**
   ```rust
   // crates/raya-core/src/vm/context.rs
   #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
   pub struct VmContextId(pub u64);

   impl VmContextId {
       pub fn new() -> Self {
           static NEXT_ID: AtomicU64 = AtomicU64::new(1);
           Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
       }
   }
   ```

2. **Implement VmContext Structure**
   ```rust
   pub struct VmContext {
       pub id: VmContextId,
       pub heap: Heap,
       pub globals: HashMap<String, Value>,
       pub module_table: Vec<Module>,
       pub function_table: Vec<Function>,
       pub type_table: Vec<TypeInfo>,
       pub task_registry: HashMap<TaskId, Task>,
       pub resource_limits: ResourceLimits,
       pub resource_counters: ResourceCounters,
       pub capabilities: CapabilitySet,
   }
   ```

3. **Resource Limits and Counters**
   ```rust
   pub struct ResourceLimits {
       pub max_heap_bytes: Option<usize>,
       pub max_tasks: Option<usize>,
       pub max_step_budget: Option<u64>,
   }

   pub struct ResourceCounters {
       pub heap_bytes_used: AtomicUsize,
       pub tasks_active: AtomicUsize,
       pub steps_executed: AtomicU64,
   }
   ```

4. **Global VmContext Registry**
   ```rust
   pub struct VmRegistry {
       contexts: RwLock<HashMap<VmContextId, Arc<VmContext>>>,
   }

   impl VmRegistry {
       pub fn register(&self, context: VmContext) -> VmContextId;
       pub fn get(&self, id: VmContextId) -> Option<Arc<VmContext>>;
       pub fn remove(&self, id: VmContextId);
   }
   ```

**Files:**
- `crates/raya-core/src/vm/context.rs` (new)
- `crates/raya-core/src/vm/registry.rs` (new)
- `crates/raya-core/src/vm/resources.rs` (new)

**Tests:**
- [ ] Create VmContext with unique IDs
- [ ] Register and retrieve contexts
- [ ] Resource limit initialization
- [ ] Resource counter tracking

**Estimated Time:** 3-4 days

---

### Task 2: Task Ownership and Context Tagging

**Goal:** Tag tasks with owning VmContext and enforce isolation

**Subtasks:**

1. **Add VmContextId to Task**
   ```rust
   pub struct Task {
       pub id: TaskId,
       pub context_id: VmContextId,  // NEW
       pub stack: Vec<CallFrame>,
       pub ip: InstructionPointer,
       pub status: TaskStatus,
       // ... other fields
   }
   ```

2. **Update Task Creation**
   - All task creation must specify owning context
   - Task spawning inherits parent's context
   - Cross-context spawning requires explicit API

3. **Context Switching in Worker Loop**
   ```rust
   impl Worker {
       fn run_task(&mut self, task: Task) {
           // Get the task's owning context
           let context = self.registry.get(task.context_id).unwrap();

           // Switch to that context
           self.current_context = Some(context);

           // Execute with context-specific state
           self.execute_with_context(&task);

           // Clear current context
           self.current_context = None;
       }
   }
   ```

**Files:**
- `crates/raya-core/src/task.rs` (modify)
- `crates/raya-core/src/scheduler/worker.rs` (modify)

**Tests:**
- [ ] Tasks tagged with correct context
- [ ] Worker switches context correctly
- [ ] Multiple contexts execute correctly

**Estimated Time:** 2-3 days

---

### Task 3: Heap Isolation

**Goal:** Ensure no cross-context heap pointers

**Subtasks:**

1. **Add Context ID to Heap Objects**
   ```rust
   pub struct GcHeader {
       pub marked: bool,
       pub context_id: VmContextId,  // NEW
       pub object_type: ObjectType,
       // ... other fields
   }
   ```

2. **Validate References**
   - Add debug assertions checking context IDs match
   - Prevent accidental cross-context pointers
   - GC only traces objects within same context

3. **Context-Scoped GC**
   ```rust
   impl Gc {
       pub fn collect(&mut self, context_id: VmContextId) {
           // Only collect objects belonging to this context
       }
   }
   ```

**Files:**
- `crates/raya-core/src/gc.rs` (modify)
- `crates/raya-core/src/value.rs` (modify)

**Tests:**
- [ ] Objects tagged with context ID
- [ ] GC only collects context-specific objects
- [ ] Cross-context reference detection (debug mode)

**Estimated Time:** 3-4 days

---

### Task 4: Data Marshalling

**Goal:** Implement safe value transfer between contexts

**Subtasks:**

1. **Define Marshallable Types**
   ```rust
   pub enum MarshalledValue {
       Null,
       Boolean(bool),
       Number(f64),
       String(String),
       Array(Vec<MarshalledValue>),
       Object(HashMap<String, MarshalledValue>),
       Foreign(ForeignHandle),
   }
   ```

2. **Implement Marshal/Unmarshal**
   ```rust
   impl VmContext {
       pub fn marshal(&self, value: Value) -> Result<MarshalledValue, MarshalError>;
       pub fn unmarshal(&self, marshalled: MarshalledValue) -> Result<Value, MarshalError>;
   }
   ```

3. **Deep Copy Semantics**
   - Recursively copy objects and arrays
   - Preserve structure but create new heap objects
   - Handle cycles (error or use handle system)

4. **Foreign Handle System** (optional)
   ```rust
   pub struct ForeignHandle(u64);

   impl VmContext {
       pub fn register_foreign(&mut self, value: Value) -> ForeignHandle;
       pub fn resolve_foreign(&self, handle: ForeignHandle) -> Option<Value>;
   }
   ```

**Files:**
- `crates/raya-core/src/marshal.rs` (new)
- `crates/raya-core/src/foreign.rs` (new)

**Tests:**
- [ ] Marshal primitives
- [ ] Marshal arrays and objects
- [ ] Marshal nested structures
- [ ] Unmarshal creates new heap objects
- [ ] Foreign handles (if implemented)

**Estimated Time:** 4-5 days

---

### Task 5: Capability System

**Goal:** Implement capability-based API injection

**Subtasks:**

1. **Define Capability Structure**
   ```rust
   pub struct Capability {
       pub name: String,
       pub handler: Box<dyn Fn(Vec<Value>) -> Result<Value, String>>,
   }

   pub struct CapabilitySet {
       capabilities: HashMap<String, Capability>,
   }
   ```

2. **Capability Registration**
   ```rust
   impl VmContext {
       pub fn register_capability(
           &mut self,
           name: String,
           handler: impl Fn(Vec<Value>) -> Result<Value, String> + 'static
       ) {
           self.capabilities.add(name, handler);
       }
   }
   ```

3. **Capability Invocation**
   - Add `CALL_CAPABILITY` opcode or use special import namespace
   - Resolve capability by name
   - Marshal arguments and return value

4. **Host Import Namespace**
   ```typescript
   // In inner VM code
   import { log } from "host:";
   log("Hello from inner VM!");
   ```

**Files:**
- `crates/raya-core/src/capability.rs` (new)
- `crates/raya-compiler/src/import_resolver.rs` (modify)

**Tests:**
- [ ] Register capability
- [ ] Invoke capability from inner VM
- [ ] Marshalling across capability boundary
- [ ] Missing capability error

**Estimated Time:** 3-4 days

---

### Task 6: Resource Enforcement

**Goal:** Enforce memory, task, and CPU limits

**Subtasks:**

1. **Heap Limit Enforcement**
   ```rust
   impl Heap {
       pub fn allocate(&mut self, size: usize, context: &VmContext) -> Result<GcPtr, AllocationError> {
           let current = context.counters.heap_bytes_used.load(Ordering::Relaxed);
           let new_total = current + size;

           if let Some(max) = context.limits.max_heap_bytes {
               if new_total > max {
                   return Err(AllocationError::HeapLimitExceeded);
               }
           }

           context.counters.heap_bytes_used.store(new_total, Ordering::Relaxed);
           // ... perform allocation
       }
   }
   ```

2. **Task Limit Enforcement**
   ```rust
   impl VmContext {
       pub fn spawn_task(&mut self, func: FunctionId) -> Result<TaskId, VmError> {
           let current = self.counters.tasks_active.load(Ordering::Relaxed);

           if let Some(max) = self.limits.max_tasks {
               if current >= max {
                   return Err(VmError::TaskLimitExceeded);
               }
           }

           self.counters.tasks_active.fetch_add(1, Ordering::Relaxed);
           // ... spawn task
       }
   }
   ```

3. **CPU Fuel System**
   ```rust
   impl Interpreter {
       pub fn execute_instruction(&mut self, context: &VmContext) -> Result<(), VmError> {
           // Decrement fuel counter
           let fuel = context.counters.steps_executed.fetch_add(1, Ordering::Relaxed);

           if let Some(max) = context.limits.max_step_budget {
               if fuel >= max {
                   return Err(VmError::StepBudgetExceeded);
               }
           }

           // Execute instruction
           // ...
       }
   }
   ```

4. **Graceful Limit Violations**
   - Terminate task on limit violation
   - Propagate error to caller
   - Clean up resources

**Files:**
- `crates/raya-core/src/heap.rs` (modify)
- `crates/raya-core/src/vm/interpreter.rs` (modify)
- `crates/raya-core/src/error.rs` (modify)

**Tests:**
- [ ] Heap limit enforcement
- [ ] Task limit enforcement
- [ ] CPU fuel enforcement
- [ ] Error propagation on violation

**Estimated Time:** 4-5 days

---

### Task 7: Fair Scheduling Across Contexts

**Goal:** Prevent context starvation with fairness policies

**Subtasks:**

1. **Per-Context Ready Queues** (optional weighted scheduling)
   ```rust
   pub struct Scheduler {
       global_queue: Arc<Injector<Task>>,
       context_queues: HashMap<VmContextId, Worker<Task>>,
       // ... other fields
   }
   ```

2. **Round-Robin Context Selection**
   - Track last-scheduled context
   - Cycle through contexts fairly
   - Give each context time slice

3. **Fuel-Based Yielding**
   - When fuel runs out, task yields
   - Scheduler picks from different context
   - Ensures no single context hogs CPU

4. **QoS Tiers** (future enhancement)
   - Priority levels per context
   - Weighted scheduling
   - Quota-based fairness

**Files:**
- `crates/raya-core/src/scheduler/mod.rs` (modify)
- `crates/raya-core/src/scheduler/fairness.rs` (new)

**Tests:**
- [ ] Multiple contexts execute fairly
- [ ] No single context starves others
- [ ] Fuel-based yielding works
- [ ] Task throughput is balanced

**Estimated Time:** 4-5 days

---

### Task 8: Vm API Implementation

**Goal:** Implement high-level Vm class for inner VM control

**Subtasks:**

1. **VmOptions Structure**
   ```rust
   pub struct VmOptions {
       pub max_heap_bytes: Option<usize>,
       pub max_tasks: Option<usize>,
       pub max_step_budget: Option<u64>,
   }
   ```

2. **Vm Class**
   ```rust
   pub struct Vm {
       context: Arc<VmContext>,
   }

   impl Vm {
       pub fn new(options: Option<VmOptions>) -> Self {
           let context = VmContext::new(options.unwrap_or_default());
           let id = REGISTRY.register(context.clone());
           Self { context }
       }

       pub fn load_bytecode(&mut self, bytes: &[u8]) -> Result<(), VmError> {
           let module = Module::decode(bytes)?;
           self.context.load_module(module)
       }

       pub fn run_entry(&self, name: &str, args: Vec<Value>) -> Result<TaskId, VmError> {
           let func_id = self.context.find_function(name)?;
           self.context.spawn_task(func_id, args)
       }

       pub fn terminate(&mut self) {
           // Kill all tasks belonging to this context
           // Clean up resources
       }

       pub fn get_stats(&self) -> VmStats {
           VmStats {
               heap_bytes_used: self.context.counters.heap_bytes_used.load(Ordering::Relaxed),
               max_heap_bytes: self.context.limits.max_heap_bytes,
               tasks: self.context.counters.tasks_active.load(Ordering::Relaxed),
               max_tasks: self.context.limits.max_tasks,
               steps_executed: self.context.counters.steps_executed.load(Ordering::Relaxed),
           }
       }
   }
   ```

3. **Integrate with Standard Library**
   - Expose `Vm` class in `raya:vm` module
   - Add `Compiler` class for runtime compilation
   - Export types (`VmOptions`, `VmStats`)

**Files:**
- `crates/raya-stdlib/src/vm.rs` (new)
- `crates/raya-core/src/vm/api.rs` (new)

**Tests:**
- [ ] Create Vm with options
- [ ] Load bytecode into Vm
- [ ] Run entry function
- [ ] Get stats
- [ ] Terminate Vm

**Estimated Time:** 3-4 days

---

### Task 9: Cross-Context Task Management

**Goal:** Allow outer VM to control inner VM tasks

**Subtasks:**

1. **Task Handles Across Contexts**
   ```rust
   pub struct TaskHandle {
       task_id: TaskId,
       context_id: VmContextId,
   }

   impl TaskHandle {
       pub async fn await_result(&self) -> Result<Value, VmError> {
           // Wait for task completion
           // Marshal result value
       }

       pub fn cancel(&self) {
           // Cancel the task
       }
   }
   ```

2. **Result Marshalling**
   - Task completes in inner context
   - Result is marshalled automatically
   - Returned to outer context

3. **Error Propagation**
   - Inner VM errors wrapped in VmError
   - Preserve stack trace information
   - Distinguish error sources

**Files:**
- `crates/raya-core/src/task_handle.rs` (new)
- `crates/raya-core/src/error.rs` (modify)

**Tests:**
- [ ] Await task from outer VM
- [ ] Result marshalled correctly
- [ ] Error propagation works
- [ ] Task cancellation

**Estimated Time:** 3-4 days

---

### Task 10: Per-Context Snapshotting

**Goal:** Enable snapshotting individual VmContexts

**Subtasks:**

1. **Context-Scoped Snapshot**
   ```rust
   impl VmContext {
       pub fn snapshot(&self) -> Result<ContextSnapshot, SnapshotError> {
           // Snapshot only this context's state
           ContextSnapshot {
               id: self.id,
               heap: self.heap.snapshot()?,
               globals: self.globals.clone(),
               tasks: self.snapshot_tasks()?,
               // ... other state
           }
       }

       pub fn restore(snapshot: ContextSnapshot) -> Result<Self, SnapshotError> {
           // Restore context from snapshot
       }
   }
   ```

2. **Multi-Context Full Snapshot**
   - Snapshot all registered contexts
   - Include context registry state
   - Preserve cross-context relationships (if any)

3. **Integration with Existing Snapshotting**
   - Reuse snapshot infrastructure from Milestone 7.1
   - Extend to handle multiple contexts
   - Maintain compatibility

**Files:**
- `crates/raya-core/src/vm/snapshot.rs` (modify from 7.1)
- `crates/raya-snapshot/src/context.rs` (new)

**Tests:**
- [ ] Snapshot single context
- [ ] Restore context from snapshot
- [ ] Snapshot multiple contexts
- [ ] Full VM snapshot with contexts

**Estimated Time:** 3-4 days (if 7.1 is complete)

---

### Task 11: Error Containment and Debugging

**Goal:** Ensure robust error handling and debugging support

**Subtasks:**

1. **VmError Types**
   ```rust
   #[derive(Debug, Error)]
   pub enum VmError {
       #[error("Heap limit exceeded: {current} > {max}")]
       HeapLimitExceeded { current: usize, max: usize },

       #[error("Task limit exceeded: {current} >= {max}")]
       TaskLimitExceeded { current: usize, max: usize },

       #[error("Step budget exceeded: {steps}")]
       StepBudgetExceeded { steps: u64 },

       #[error("Runtime error in context {context}: {error}")]
       RuntimeError { context: VmContextId, error: String },

       #[error("Capability not found: {name}")]
       CapabilityNotFound { name: String },
   }
   ```

2. **Error Recovery**
   - Clean up context resources on error
   - Never corrupt outer VM state
   - Allow retry or termination

3. **Debugging Support**
   - Track execution in each context
   - Separate stack traces per context
   - Context identification in errors

**Files:**
- `crates/raya-core/src/error.rs` (modify)
- `crates/raya-core/src/debug.rs` (modify)

**Tests:**
- [ ] Limit violations handled correctly
- [ ] Runtime errors contained
- [ ] Error recovery works
- [ ] Stack traces show context

**Estimated Time:** 2-3 days

---

### Task 12: Testing and Documentation

**Goal:** Comprehensive testing and documentation

**Subtasks:**

1. **Integration Tests**
   - Single inner VM execution
   - Multiple concurrent inner VMs
   - Resource limit enforcement
   - Capability system usage
   - Cross-context communication
   - Snapshotting inner VMs
   - Error containment

2. **Performance Benchmarks**
   - Context creation overhead
   - Task switching overhead
   - Marshalling performance
   - Fair scheduling effectiveness

3. **Write Documentation**
   - API documentation
   - Usage examples
   - Security model explanation
   - Best practices

4. **Create Examples**
   ```typescript
   // examples/inner-vm/plugin-system.raya
   import { Vm } from "raya:vm";

   // Load untrusted plugin code
   const plugin = new Vm({
       maxHeapBytes: 1024 * 1024,  // 1MB limit
       maxTasks: 10,
       maxStepBudget: 1_000_000,   // 1M instructions
   });

   // Grant limited capabilities
   plugin.registerCapability("log", (msg: string) => {
       console.log("[plugin]", msg);
   });

   // Load and run plugin
   plugin.loadBytecode(pluginBytecode);
   const result = await plugin.runEntry("main");
   console.log("Plugin returned:", result);
   ```

**Files:**
- `tests/inner_vm_integration.rs` (new)
- `benches/inner_vm_bench.rs` (new)
- `examples/inner-vm/` (new directory)
- `crates/raya-stdlib/README_VM.md` (new)

**Tests:**
- [ ] All integration tests pass
- [ ] Benchmarks run successfully
- [ ] Examples compile and run
- [ ] Documentation is complete

**Estimated Time:** 5-6 days

---

## Total Estimated Time

- Task 1: 3-4 days
- Task 2: 2-3 days
- Task 3: 3-4 days
- Task 4: 4-5 days
- Task 5: 3-4 days
- Task 6: 4-5 days
- Task 7: 4-5 days
- Task 8: 3-4 days
- Task 9: 3-4 days
- Task 10: 3-4 days
- Task 11: 2-3 days
- Task 12: 5-6 days

**Total:** 40-52 days (approximately 8-11 weeks)

---

## Success Criteria

- [ ] Multiple VmContexts can be created and managed
- [ ] Heaps are completely isolated between contexts
- [ ] Resource limits are enforced correctly
- [ ] Capability system works for API injection
- [ ] Data marshalling prevents cross-context references
- [ ] Fair scheduling prevents starvation
- [ ] Snapshotting works per-context and globally
- [ ] Error containment prevents corruption
- [ ] All tests pass
- [ ] Documentation is complete
- [ ] Performance is acceptable (context switch < 1μs)

---

## Future Enhancements

These are not part of this milestone but could be added later:

1. **Advanced Scheduling**
   - QoS tiers with priorities
   - Weighted fair scheduling
   - CPU quotas per context

2. **Enhanced Capabilities**
   - Capability descriptors with permissions
   - Revocable capabilities
   - Capability delegation

3. **Distributed Inner VMs**
   - Cross-machine inner VM communication
   - Remote capability invocation
   - Distributed snapshotting

4. **JIT Compilation**
   - Per-context JIT compilation
   - Code cache isolation
   - Profile-guided optimization per context

5. **Advanced Marshalling**
   - Shared memory regions (with locks)
   - Zero-copy for large buffers
   - Streaming data transfer

---

## Dependencies

**Required Rust Crates:**
- `parking_lot` - Synchronization primitives
- `crossbeam` - Work-stealing scheduler
- `rustc-hash` - Fast hashing for registries

**Internal Dependencies:**
- `raya-bytecode` - Module loading
- `raya-core` - VM runtime (all subsystems)
- `raya-snapshot` - Snapshotting support (Milestone 7.1)

---

## Notes

- Inner VMs provide **strong isolation** at the heap and capability level
- All contexts share the **same scheduler** for efficiency
- Resource limits are **mandatory** for untrusted code
- Snapshotting enables **checkpointing and migration** of sandboxed code
- The capability model enables **fine-grained security control**
- This design is ideal for **plugin systems, multi-tenancy, and sandboxing**
