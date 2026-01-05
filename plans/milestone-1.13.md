# Milestone 1.13: Inner VMs & Controllability

**Phase:** 1 - VM Core
**Crate:** `raya-core`
**Status:** 🔄 In Progress
**Prerequisites:** Milestones 1.1-1.12 ✅

---

## Table of Contents

1. [Overview](#overview)
2. [Goals](#goals)
3. [Tasks](#tasks)
4. [Implementation Details](#implementation-details)
5. [Testing Requirements](#testing-requirements)
6. [Success Criteria](#success-criteria)
7. [Dependencies](#dependencies)
8. [References](#references)

---

## Overview

Implement nested, isolated virtual machine contexts (Inner VMs) that run on the shared global scheduler while maintaining strict heap, type, and capability isolation. This enables sandboxing, plugin systems, multi-tenant compute, and metaprogramming with strong guarantees around safety, resource control, and determinism.

**Key Deliverable:** A fully functional Inner VM system that allows Raya code to instantiate, control, and monitor isolated VmContexts with resource limits and capability-based security.

---

## Goals

### Primary Goals

- ✅ Define VmContext structure with isolated heaps and metadata
- ⬜ Implement VmOptions for resource limits and configuration
- ⬜ Build resource accounting and enforcement mechanisms
- ⬜ Create capability injection system for host API access
- ⬜ Implement data marshalling across context boundaries
- ⬜ Add foreign handle system for cross-context references
- ⬜ Integrate with existing Task scheduler
- ⬜ Support context termination and cleanup
- ⬜ Enable independent snapshotting per VmContext
- ⬜ Achieve >90% test coverage

### Secondary Goals

- Fair scheduling across multiple contexts
- CPU/instruction budgeting (fuel-based execution)
- Memory pressure and back-pressure handling
- Observability and stats APIs
- QoS/priority tiers for contexts

---

## Tasks

### Task 1: VmContext Structure Enhancement

**Files:**
- `crates/raya-core/src/vm/context.rs`
- `crates/raya-core/src/vm/mod.rs`

**Objectives:**
- [ ] Enhance existing VmContext with isolation features
- [ ] Add VmContextId for unique identification
- [ ] Add resource counters (heap bytes, task count, steps executed)
- [ ] Add resource limits configuration
- [ ] Add capability registry per context
- [ ] Ensure heap objects cannot cross context boundaries

**Implementation:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VmContextId(u64);

pub struct VmContext {
    pub id: VmContextId,
    pub heap: Heap,
    pub globals: Vec<Value>,
    pub type_registry: TypeRegistry,
    pub function_table: FunctionTable,
    pub class_registry: ClassRegistry,
    pub task_registry: Vec<TaskId>,  // Tasks owned by this context
    pub resource_counters: ResourceCounters,
    pub resource_limits: ResourceLimits,
    pub capabilities: CapabilityRegistry,
    pub parent: Option<VmContextId>,  // For nested contexts
}

pub struct ResourceCounters {
    pub heap_bytes_used: AtomicUsize,
    pub task_count: AtomicUsize,
    pub steps_executed: AtomicU64,
}

pub struct ResourceLimits {
    pub max_heap_bytes: Option<usize>,
    pub max_tasks: Option<usize>,
    pub max_step_budget: Option<u64>,
}
```

**Tests:**
- [ ] Create VmContext with unique ID
- [ ] Verify heap isolation between contexts
- [ ] Test resource counter updates
- [ ] Test resource limit enforcement

---

### Task 2: VmOptions Configuration

**File:** `crates/raya-core/src/vm/options.rs`

**Objectives:**
- [ ] Define VmOptions struct for context creation
- [ ] Support optional resource limits
- [ ] Support capability injection
- [ ] Provide sensible defaults

**Implementation:**
```rust
#[derive(Debug, Clone)]
pub struct VmOptions {
    /// Maximum heap memory in bytes
    pub max_heap_bytes: Option<usize>,

    /// Maximum number of concurrent Tasks
    pub max_tasks: Option<usize>,

    /// Maximum execution steps (fuel-based control)
    pub max_step_budget: Option<u64>,

    /// Capabilities to inject
    pub capabilities: Vec<Box<dyn Capability>>,

    /// Parent context (for nested VMs)
    pub parent: Option<VmContextId>,
}

impl Default for VmOptions {
    fn default() -> Self {
        Self {
            max_heap_bytes: Some(64 * 1024 * 1024), // 64 MB default
            max_tasks: Some(1000),
            max_step_budget: None, // Unlimited by default
            capabilities: Vec::new(),
            parent: None,
        }
    }
}
```

**Tests:**
- [ ] Test default options
- [ ] Test custom options
- [ ] Test option validation

---

### Task 3: Context Registry

**File:** `crates/raya-core/src/vm/context_registry.rs`

**Objectives:**
- [ ] Implement global VmContext registry
- [ ] Support context creation and lookup
- [ ] Support context removal and cleanup
- [ ] Thread-safe concurrent access

**Implementation:**
```rust
use dashmap::DashMap;
use std::sync::Arc;

pub struct ContextRegistry {
    contexts: DashMap<VmContextId, Arc<RwLock<VmContext>>>,
    next_id: AtomicU64,
}

impl ContextRegistry {
    pub fn new() -> Self {
        Self {
            contexts: DashMap::new(),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn create_context(&self, options: VmOptions) -> Result<VmContextId, VmError> {
        let id = VmContextId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let context = VmContext::new(id, options)?;
        self.contexts.insert(id, Arc::new(RwLock::new(context)));
        Ok(id)
    }

    pub fn get(&self, id: VmContextId) -> Option<Arc<RwLock<VmContext>>> {
        self.contexts.get(&id).map(|entry| entry.clone())
    }

    pub fn remove(&self, id: VmContextId) -> Option<Arc<RwLock<VmContext>>> {
        self.contexts.remove(&id).map(|(_, context)| context)
    }

    pub fn terminate(&self, id: VmContextId) -> Result<(), VmError> {
        if let Some(context) = self.get(id) {
            let mut ctx = context.write();
            // Terminate all tasks belonging to this context
            for task_id in ctx.task_registry.drain(..) {
                // Signal task termination
            }
            ctx.heap.clear();
            drop(ctx);
            self.remove(id);
            Ok(())
        } else {
            Err(VmError::ContextNotFound(id))
        }
    }
}
```

**Tests:**
- [ ] Create multiple contexts
- [ ] Lookup existing contexts
- [ ] Remove contexts
- [ ] Terminate contexts and verify cleanup
- [ ] Concurrent context operations

---

### Task 4: Resource Accounting & Enforcement

**File:** `crates/raya-core/src/vm/resources.rs`

**Objectives:**
- [ ] Implement resource tracking hooks
- [ ] Add enforcement checks during execution
- [ ] Support graceful limit violations
- [ ] Integrate with heap allocator

**Implementation:**
```rust
impl VmContext {
    /// Check if resource limits are exceeded
    pub fn check_limits(&self) -> Result<(), VmError> {
        let counters = &self.resource_counters;
        let limits = &self.resource_limits;

        // Check heap limit
        if let Some(max) = limits.max_heap_bytes {
            let used = counters.heap_bytes_used.load(Ordering::Relaxed);
            if used > max {
                return Err(VmError::HeapLimitExceeded { used, max });
            }
        }

        // Check task limit
        if let Some(max) = limits.max_tasks {
            let count = counters.task_count.load(Ordering::Relaxed);
            if count > max {
                return Err(VmError::TaskLimitExceeded { count, max });
            }
        }

        // Check step budget
        if let Some(max) = limits.max_step_budget {
            let steps = counters.steps_executed.load(Ordering::Relaxed);
            if steps > max {
                return Err(VmError::StepBudgetExceeded { steps, max });
            }
        }

        Ok(())
    }

    /// Allocate heap memory with limit checking
    pub fn allocate(&mut self, size: usize) -> Result<*mut u8, VmError> {
        // Check if allocation would exceed limit
        if let Some(max) = self.resource_limits.max_heap_bytes {
            let current = self.resource_counters.heap_bytes_used.load(Ordering::Relaxed);
            if current + size > max {
                return Err(VmError::HeapLimitExceeded {
                    used: current + size,
                    max
                });
            }
        }

        // Perform allocation
        let ptr = self.heap.allocate(size)?;

        // Update counter
        self.resource_counters.heap_bytes_used.fetch_add(size, Ordering::Relaxed);

        Ok(ptr)
    }

    /// Track step execution (fuel-based control)
    pub fn consume_steps(&self, steps: u64) -> Result<(), VmError> {
        let total = self.resource_counters.steps_executed.fetch_add(steps, Ordering::Relaxed) + steps;

        if let Some(max) = self.resource_limits.max_step_budget {
            if total > max {
                return Err(VmError::StepBudgetExceeded { steps: total, max });
            }
        }

        Ok(())
    }
}
```

**Tests:**
- [ ] Test heap limit enforcement
- [ ] Test task limit enforcement
- [ ] Test step budget enforcement
- [ ] Test graceful error handling on limit violations
- [ ] Test resource counter accuracy

---

### Task 5: Capability System

**File:** `crates/raya-core/src/vm/capability.rs`

**Objectives:**
- [ ] Define Capability trait
- [ ] Implement CapabilityRegistry
- [ ] Support capability injection at context creation
- [ ] Enable capability invocation from inner VM

**Implementation:**
```rust
pub trait Capability: Send + Sync {
    /// Capability name (e.g., "log", "query")
    fn name(&self) -> &str;

    /// Invoke capability with arguments
    fn invoke(&self, args: &[Value]) -> Result<Value, VmError>;
}

pub struct CapabilityRegistry {
    capabilities: HashMap<String, Box<dyn Capability>>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        Self {
            capabilities: HashMap::new(),
        }
    }

    pub fn register(&mut self, capability: Box<dyn Capability>) {
        self.capabilities.insert(capability.name().to_string(), capability);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Capability> {
        self.capabilities.get(name).map(|c| c.as_ref())
    }

    pub fn invoke(&self, name: &str, args: &[Value]) -> Result<Value, VmError> {
        match self.get(name) {
            Some(cap) => cap.invoke(args),
            None => Err(VmError::CapabilityNotFound(name.to_string())),
        }
    }
}

// Example capability implementation
pub struct LogCapability;

impl Capability for LogCapability {
    fn name(&self) -> &str {
        "log"
    }

    fn invoke(&self, args: &[Value]) -> Result<Value, VmError> {
        for arg in args {
            println!("[inner VM] {:?}", arg);
        }
        Ok(Value::Null)
    }
}
```

**Tests:**
- [ ] Register capabilities
- [ ] Invoke capabilities successfully
- [ ] Handle missing capabilities
- [ ] Test multiple capabilities in one context
- [ ] Test capability isolation between contexts

---

### Task 6: Data Marshalling

**File:** `crates/raya-core/src/vm/marshal.rs`

**Objectives:**
- [ ] Implement value marshalling across context boundaries
- [ ] Support primitives, arrays, and plain objects
- [ ] Implement foreign handle system
- [ ] Prevent pointer aliasing across heaps

**Implementation:**
```rust
#[derive(Debug, Clone)]
pub enum MarshalledValue {
    Null,
    Bool(bool),
    I32(i32),
    F64(f64),
    String(String),                    // Deep copy
    Array(Vec<MarshalledValue>),       // Deep copy
    Object(HashMap<String, MarshalledValue>), // Deep copy
    Foreign(ForeignHandle),             // Opaque handle
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ForeignHandle {
    context_id: VmContextId,
    object_id: u64,
}

pub struct ForeignRegistry {
    handles: DashMap<ForeignHandle, Arc<RwLock<Value>>>,
    next_id: AtomicU64,
}

impl ForeignRegistry {
    pub fn register(&self, context_id: VmContextId, value: Value) -> ForeignHandle {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let handle = ForeignHandle { context_id, object_id: id };
        self.handles.insert(handle, Arc::new(RwLock::new(value)));
        handle
    }

    pub fn resolve(&self, handle: ForeignHandle) -> Option<Arc<RwLock<Value>>> {
        self.handles.get(&handle).map(|entry| entry.clone())
    }
}

/// Marshal a value from one context for use in another
pub fn marshal(value: &Value, from_ctx: &VmContext, to_ctx: &VmContext) -> Result<MarshalledValue, VmError> {
    match value {
        Value::Null => Ok(MarshalledValue::Null),
        Value::Bool(b) => Ok(MarshalledValue::Bool(*b)),
        Value::I32(i) => Ok(MarshalledValue::I32(*i)),
        Value::F64(f) => Ok(MarshalledValue::F64(*f)),
        Value::String(ptr) => {
            // Deep copy string
            let s = from_ctx.heap.read_string(*ptr)?;
            Ok(MarshalledValue::String(s.to_string()))
        }
        Value::Array(ptr) => {
            // Deep copy array
            let arr = from_ctx.heap.read_array(*ptr)?;
            let marshalled: Result<Vec<_>, _> = arr.iter()
                .map(|v| marshal(v, from_ctx, to_ctx))
                .collect();
            Ok(MarshalledValue::Array(marshalled?))
        }
        Value::Object(ptr) => {
            // Deep copy plain object
            let obj = from_ctx.heap.read_object(*ptr)?;
            let mut marshalled = HashMap::new();
            for (key, value) in obj.fields.iter() {
                marshalled.insert(key.clone(), marshal(value, from_ctx, to_ctx)?);
            }
            Ok(MarshalledValue::Object(marshalled))
        }
        _ => {
            // Complex objects become foreign handles
            // TODO: Register in foreign registry
            Err(VmError::MarshalError("Complex objects not yet supported".to_string()))
        }
    }
}

/// Unmarshal a marshalled value into a context
pub fn unmarshal(marshalled: MarshalledValue, ctx: &mut VmContext) -> Result<Value, VmError> {
    match marshalled {
        MarshalledValue::Null => Ok(Value::Null),
        MarshalledValue::Bool(b) => Ok(Value::Bool(b)),
        MarshalledValue::I32(i) => Ok(Value::I32(i)),
        MarshalledValue::F64(f) => Ok(Value::F64(f)),
        MarshalledValue::String(s) => {
            let ptr = ctx.heap.allocate_string(&s)?;
            Ok(Value::String(ptr))
        }
        MarshalledValue::Array(arr) => {
            let values: Result<Vec<_>, _> = arr.into_iter()
                .map(|v| unmarshal(v, ctx))
                .collect();
            let ptr = ctx.heap.allocate_array(&values?)?;
            Ok(Value::Array(ptr))
        }
        MarshalledValue::Object(map) => {
            let mut fields = HashMap::new();
            for (key, value) in map {
                fields.insert(key, unmarshal(value, ctx)?);
            }
            let ptr = ctx.heap.allocate_object(fields)?;
            Ok(Value::Object(ptr))
        }
        MarshalledValue::Foreign(handle) => {
            // TODO: Create proxy object
            Err(VmError::MarshalError("Foreign handles not yet supported".to_string()))
        }
    }
}
```

**Tests:**
- [ ] Marshal primitives
- [ ] Marshal strings (deep copy)
- [ ] Marshal arrays (deep copy)
- [ ] Marshal plain objects (deep copy)
- [ ] Test marshalling prevents pointer sharing
- [ ] Unmarshal values into different context
- [ ] Round-trip marshal/unmarshal

---

### Task 7: Scheduler Integration

**File:** `crates/raya-core/src/scheduler/context_aware.rs`

**Objectives:**
- [ ] Tag Tasks with their owning VmContextId
- [ ] Switch context when selecting Tasks to run
- [ ] Implement fair scheduling across contexts
- [ ] Support context-specific task limits

**Implementation:**
```rust
// Enhance Task struct
pub struct Task {
    pub id: TaskId,
    pub context: VmContextId,  // NEW: Context ownership
    pub stack: Stack,
    pub ip: usize,
    pub state: TaskState,
}

impl Scheduler {
    /// Spawn a task in a specific context
    pub fn spawn_in_context(
        &self,
        context_id: VmContextId,
        function_id: usize,
        args: Vec<Value>
    ) -> Result<TaskId, VmError> {
        // Check context task limit
        let context = self.contexts.get(context_id)?;
        let task_count = context.resource_counters.task_count.load(Ordering::Relaxed);

        if let Some(max) = context.resource_limits.max_tasks {
            if task_count >= max {
                return Err(VmError::TaskLimitExceeded {
                    count: task_count,
                    max
                });
            }
        }

        // Create task
        let task_id = self.next_task_id();
        let task = Task::new(task_id, context_id, function_id, args);

        // Update counter
        context.resource_counters.task_count.fetch_add(1, Ordering::Relaxed);

        // Add to scheduler
        self.tasks.insert(task_id, task);
        self.injector.push(task_id);

        Ok(task_id)
    }

    /// Execute task with context switching
    fn execute_task(&self, task_id: TaskId) -> Result<TaskStatus, VmError> {
        let task = self.tasks.get(task_id)?;
        let context_id = task.context;

        // Switch to task's context
        let context = self.contexts.get(context_id)?;
        let mut ctx = context.write();

        // Execute with step counting
        let steps_before = ctx.resource_counters.steps_executed.load(Ordering::Relaxed);
        let result = self.execute_with_context(&mut ctx, task_id);
        let steps_after = ctx.resource_counters.steps_executed.load(Ordering::Relaxed);

        // Check step budget
        if let Some(max) = ctx.resource_limits.max_step_budget {
            if steps_after > max {
                return Err(VmError::StepBudgetExceeded {
                    steps: steps_after,
                    max
                });
            }
        }

        result
    }
}
```

**Tests:**
- [ ] Spawn tasks in different contexts
- [ ] Verify context switching during execution
- [ ] Test task limit enforcement per context
- [ ] Test fair scheduling across contexts
- [ ] Test step counting per context

---

### Task 8: VM Lifecycle & Control

**File:** `crates/raya-core/src/vm/lifecycle.rs`

**Objectives:**
- [ ] Implement VM creation API
- [ ] Support .rbin file loading (compiled binaries only)
- [ ] Support creation from snapshot
- [ ] Enable entry point execution
- [ ] Support VM termination
- [ ] Implement stats observation
- [ ] Support VM snapshot/restore

**Note:** Inner VMs do NOT support loading .raya source files. Only pre-compiled .rbin binaries can be loaded. Compilation is a separate build-time phase.

**Implementation:**
```rust
impl Vm {
    /// Create a new isolated VmContext
    pub fn new(options: VmOptions) -> Result<Self, VmError> {
        let context_id = CONTEXT_REGISTRY.create_context(options)?;
        Ok(Self { context_id })
    }

    /// Create a Vm from a snapshot
    pub fn from_snapshot(snapshot: VmSnapshot, options: Option<VmOptions>) -> Result<Self, VmError> {
        let context = VmContext::restore(snapshot.context)?;

        // Apply new options if provided (can update limits)
        if let Some(opts) = options {
            context.resource_limits = opts.into();
        }

        let context_id = context.id;
        CONTEXT_REGISTRY.register(context)?;

        Ok(Self { context_id })
    }

    /// Load .rbin file into context
    pub fn load_rbin(&self, path: &Path) -> Result<(), VmError> {
        let bytes = std::fs::read(path)
            .map_err(|e| VmError::IoError(e))?;
        self.load_rbin_bytes(&bytes)
    }

    /// Load .rbin from bytes
    pub fn load_rbin_bytes(&self, bytes: &[u8]) -> Result<(), VmError> {
        let context = CONTEXT_REGISTRY.get(self.context_id)?;
        let mut ctx = context.write();

        // Parse .rbin format (includes header, constant pool, functions, etc.)
        let module = Module::decode(bytes)?;

        // Verify it's a valid .rbin file
        if !module.has_rbin_magic() {
            return Err(VmError::InvalidBinaryFormat("Not a valid .rbin file".to_string()));
        }

        ctx.load_module(module)?;

        Ok(())
    }

    /// Load raw bytecode module into context (legacy support)
    pub fn load_bytecode(&self, bytecode: &[u8]) -> Result<(), VmError> {
        let context = CONTEXT_REGISTRY.get(self.context_id)?;
        let mut ctx = context.write();

        // Parse and load module
        let module = Module::decode(bytecode)?;
        ctx.load_module(module)?;

        Ok(())
    }

    /// Run entry point function
    pub fn run_entry(&self, name: &str, args: Vec<Value>) -> Result<TaskId, VmError> {
        let context = CONTEXT_REGISTRY.get(self.context_id)?;
        let ctx = context.read();

        // Find entry function
        let function_id = ctx.function_table.find(name)
            .ok_or_else(|| VmError::FunctionNotFound(name.to_string()))?;

        // Marshal arguments
        let marshalled_args: Result<Vec<_>, _> = args.iter()
            .map(|v| marshal(v, &ctx, &ctx))
            .collect();

        // Spawn task in context
        SCHEDULER.spawn_in_context(self.context_id, function_id, marshalled_args?)
    }

    /// Terminate all tasks and clean up context
    pub fn terminate(&self) -> Result<(), VmError> {
        CONTEXT_REGISTRY.terminate(self.context_id)
    }

    /// Get resource usage statistics
    pub fn get_stats(&self) -> Result<VmStats, VmError> {
        let context = CONTEXT_REGISTRY.get(self.context_id)?;
        let ctx = context.read();

        Ok(VmStats {
            heap_bytes_used: ctx.resource_counters.heap_bytes_used.load(Ordering::Relaxed),
            max_heap_bytes: ctx.resource_limits.max_heap_bytes.unwrap_or(0),
            tasks: ctx.resource_counters.task_count.load(Ordering::Relaxed),
            max_tasks: ctx.resource_limits.max_tasks.unwrap_or(0),
            steps_executed: ctx.resource_counters.steps_executed.load(Ordering::Relaxed),
        })
    }

    /// Snapshot this VM's complete state
    pub fn snapshot(&self) -> Result<VmSnapshot, VmError> {
        let context = CONTEXT_REGISTRY.get(self.context_id)?;
        let ctx = context.read();

        Ok(VmSnapshot {
            context: ctx.snapshot()?,
        })
    }

    /// Restore VM state from snapshot (replaces current state)
    pub fn restore(&mut self, snapshot: VmSnapshot) -> Result<(), VmError> {
        let context = CONTEXT_REGISTRY.get(self.context_id)?;
        let mut ctx = context.write();

        // Restore context state
        let restored = VmContext::restore(snapshot.context)?;

        // Replace current context
        *ctx = restored;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct VmStats {
    pub heap_bytes_used: usize,
    pub max_heap_bytes: usize,
    pub tasks: usize,
    pub max_tasks: usize,
    pub steps_executed: u64,
}

#[derive(Debug, Clone)]
pub struct VmSnapshot {
    pub context: ContextSnapshot,
}
```

**Tests:**
- [ ] Create VM with options
- [ ] Create VM from snapshot
- [ ] Load .rbin file
- [ ] Load .rbin from bytes
- [ ] Load raw bytecode module
- [ ] Run entry point function
- [ ] Terminate VM and verify cleanup
- [ ] Get stats and verify accuracy
- [ ] Snapshot VM state
- [ ] Restore VM from snapshot
- [ ] Verify snapshot/restore roundtrip

---

### Task 9: Snapshot Integration

**File:** `crates/raya-core/src/vm/snapshot_context.rs`

**Objectives:**
- [ ] Enable independent snapshots per VmContext
- [ ] Support multi-context snapshots
- [ ] Ensure snapshot/restore consistency

**Implementation:**
```rust
impl VmContext {
    /// Snapshot this context's state
    pub fn snapshot(&self) -> Result<ContextSnapshot, VmError> {
        Ok(ContextSnapshot {
            id: self.id,
            heap_snapshot: self.heap.snapshot()?,
            globals: self.globals.clone(),
            task_ids: self.task_registry.clone(),
            resource_counters: ResourceCountersSnapshot {
                heap_bytes_used: self.resource_counters.heap_bytes_used.load(Ordering::Relaxed),
                task_count: self.resource_counters.task_count.load(Ordering::Relaxed),
                steps_executed: self.resource_counters.steps_executed.load(Ordering::Relaxed),
            },
            resource_limits: self.resource_limits.clone(),
        })
    }

    /// Restore context from snapshot
    pub fn restore(snapshot: ContextSnapshot) -> Result<Self, VmError> {
        Ok(Self {
            id: snapshot.id,
            heap: Heap::restore(snapshot.heap_snapshot)?,
            globals: snapshot.globals,
            task_registry: snapshot.task_ids,
            resource_counters: ResourceCounters {
                heap_bytes_used: AtomicUsize::new(snapshot.resource_counters.heap_bytes_used),
                task_count: AtomicUsize::new(snapshot.resource_counters.task_count),
                steps_executed: AtomicU64::new(snapshot.resource_counters.steps_executed),
            },
            resource_limits: snapshot.resource_limits,
            // ... other fields
        })
    }
}
```

**Tests:**
- [ ] Snapshot single context
- [ ] Restore from snapshot
- [ ] Snapshot multiple contexts
- [ ] Verify resource counters in snapshot
- [ ] Test snapshot/restore with running tasks

---

## Testing Requirements

### Unit Tests

**File:** `crates/raya-core/src/vm/tests/inner_vm_tests.rs`

- [ ] Test VmContext creation with options
- [ ] Test resource limit enforcement
- [ ] Test capability injection and invocation
- [ ] Test data marshalling
- [ ] Test foreign handle system
- [ ] Test context termination
- [ ] Test stats observation

### Integration Tests

**File:** `crates/raya-core/tests/inner_vm_integration.rs`

- [ ] Create nested VMs (3 levels deep)
- [ ] Load .rbin files into inner VMs
- [ ] Load .rbin with main() and execute
- [ ] Load .rbin with exports and import functions
- [ ] Create VM from snapshot
- [ ] Snapshot VM and restore in new VM
- [ ] Run tasks in multiple contexts concurrently
- [ ] Test heap isolation between contexts
- [ ] Test resource limits (heap, tasks, steps)
- [ ] Test capability-based access control
- [ ] Test marshalling across context boundaries
- [ ] Test fair scheduling across contexts
- [ ] Test context termination and cleanup
- [ ] Test snapshot/restore of multiple contexts
- [ ] Test error containment (errors in inner VM don't affect outer)
- [ ] Test .rbin reflection metadata access
- [ ] Test .rbin export table parsing

### Performance Tests

**File:** `benches/inner_vm_bench.rs`

- [ ] Context creation overhead
- [ ] Marshalling performance
- [ ] Cross-context call latency
- [ ] Scheduler overhead with multiple contexts

---

## Success Criteria

### Functional Requirements

- ✅ VmContext can be created with resource limits
- ✅ VmContext can be created from snapshot
- ✅ .rbin files can be loaded into contexts
- ✅ .rbin reflection metadata is accessible
- ✅ .rbin export tables are parsed correctly
- ✅ Heaps are fully isolated between contexts
- ✅ Resource limits are enforced correctly
- ✅ Capabilities can be injected and invoked
- ✅ Values can be marshalled across contexts
- ✅ Tasks are scheduled fairly across contexts
- ✅ Contexts can be terminated cleanly
- ✅ Stats can be observed for each context
- ✅ Contexts can be snapshotted independently
- ✅ VM state can be restored from snapshot

### Non-Functional Requirements

- Context creation < 100 microseconds
- Marshalling overhead < 10% for typical values
- Fair scheduling with < 5% variance across contexts
- Memory overhead per context < 1 MB baseline
- All tests passing with >90% coverage

### Safety Requirements

- No heap pointer aliasing across contexts
- Resource limits prevent exhaustion
- Error containment prevents cascading failures
- Snapshot/restore maintains consistency

---

## Dependencies

### Crates

```toml
[dependencies]
dashmap = "5.5"              # Context registry
parking_lot = "0.12"         # RwLock for contexts
```

### Internal Dependencies

- Milestone 1.2: Bytecode Definitions (for .rbin format parsing)
- Milestone 1.10: Task Scheduler (for context-aware scheduling)
- Milestone 1.11: VM Snapshotting (for context snapshots)
- Milestone 1.12: Synchronization Primitives (for thread-safe operations)

### Design Documents

- [design/INNER_VM.md](../design/INNER_VM.md) - Complete Inner VM specification
- [design/ARCHITECTURE.md](../design/ARCHITECTURE.md) - VM architecture
- [design/SNAPSHOTTING.md](../design/SNAPSHOTTING.md) - Snapshot integration

---

## References

### Example Usage

```typescript
// Example 1: Load .rbin file
import { Vm } from "raya:vm";

const vm = new Vm({
  maxHeapBytes: 16 * 1024 * 1024,
  maxTasks: 10,
});

// Load compiled .rbin file
vm.loadRbin("./mymodule.rbin");

const task = vm.runEntry("main");
const result = await task;  // → 42

vm.terminate();
```

```typescript
// Example 2: Snapshot and restore
const vm = new Vm({ maxHeapBytes: 32 * 1024 * 1024 });
vm.loadRbin("./app.rbin");

const task = vm.runEntry("compute");
await task;

// Snapshot VM state
const snapshot = vm.snapshot();

// Later... create new VM from snapshot
const vm2 = Vm.fromSnapshot(snapshot, {
  maxHeapBytes: 64 * 1024 * 1024,  // Can update limits
});

// Continue execution
const task2 = vm2.runEntry("continue");
await task2;
```

```typescript
// Example 3: Load .rbin with exports (library usage)
const vm = new Vm({ maxHeapBytes: 16 * 1024 * 1024 });
vm.loadRbin("./math.rbin");  // Contains export function add(a, b)

// Access exported function
const task = vm.runEntry("add", [2, 3]);
const result = await task;  // → 5
```

```typescript
// Example 4: Nested VMs with isolation
const outerVm = new Vm({ maxHeapBytes: 128 * 1024 * 1024 });
outerVm.loadRbin("./host.rbin");

// Host code can create inner VMs
const innerVm = new Vm({
  maxHeapBytes: 16 * 1024 * 1024,  // Limited memory
  maxTasks: 5,                      // Limited tasks
  capabilities: [
    new LogCapability(),             // Controlled host access
  ],
});

innerVm.loadRbin("./plugin.rbin");
const pluginTask = innerVm.runEntry("main");
const result = await pluginTask;
```

### Key APIs

```rust
// Core types
pub struct Vm { context_id: VmContextId }
pub struct VmOptions { ... }
pub struct VmStats { ... }
pub struct VmSnapshot { context: ContextSnapshot }

// Vm lifecycle
impl Vm {
    pub fn new(options: VmOptions) -> Result<Self, VmError>
    pub fn from_snapshot(snapshot: VmSnapshot, options: Option<VmOptions>) -> Result<Self, VmError>

    // .rbin loading
    pub fn load_rbin(&self, path: &Path) -> Result<(), VmError>
    pub fn load_rbin_bytes(&self, bytes: &[u8]) -> Result<(), VmError>
    pub fn load_bytecode(&self, bytecode: &[u8]) -> Result<(), VmError>

    // Execution
    pub fn run_entry(&self, name: &str, args: Vec<Value>) -> Result<TaskId, VmError>

    // Control
    pub fn terminate(&self) -> Result<(), VmError>
    pub fn get_stats(&self) -> Result<VmStats, VmError>

    // Snapshot/restore
    pub fn snapshot(&self) -> Result<VmSnapshot, VmError>
    pub fn restore(&mut self, snapshot: VmSnapshot) -> Result<(), VmError>
}

// Capability system
pub trait Capability { ... }
pub struct CapabilityRegistry { ... }

// Marshalling
pub enum MarshalledValue { ... }
pub fn marshal(value: &Value, from: &VmContext, to: &VmContext) -> Result<MarshalledValue>
pub fn unmarshal(value: MarshalledValue, to: &mut VmContext) -> Result<Value>

// Context management
pub struct ContextRegistry { ... }
impl ContextRegistry {
    pub fn create_context(options: VmOptions) -> Result<VmContextId>
    pub fn register(context: VmContext) -> Result<(), VmError>
    pub fn terminate(id: VmContextId) -> Result<()>
}
```

---

**Status:** Ready for Implementation
**Estimated Effort:** 2-3 weeks
**Next Milestone:** 1.14 Integration Testing & Validation
