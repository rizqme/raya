# Raya Inner VM Design & Controllability

A specification for safely instantiating, controlling, and executing nested Raya virtual machines (VmContexts) on a shared scheduler

---

## 1. Concept & Motivation

Raya supports **instantiable virtual machines** from within Raya code itself. A program may:

- Create a new isolated `VmContext` (Vm)
- Load bytecode or compiled modules into that context
- Spawn Tasks within it
- Limit and monitor its resources
- Snapshot & resume its entire execution state

All `VmContext`s run on the **same global scheduler** and OS worker threads, providing efficient resource usage while maintaining strict heap, type, and capability isolation between VMs.

---

## 2. Architecture Model

### 2.1 Global Runtime

The process runtime contains:

- A global task scheduler
- A shared OS worker pool
- A registry of VmContexts

Each Task is tagged with its owning VmContext:

```rust
struct Task {
    id: TaskId,
    vm: VmId,
    stack: Vec<CallFrame>,
    ip: InstructionPointer,
    status: TaskStatus, // Ready | Running | Blocked | Completed | Failed
}
```

The worker loop switches the current context when selecting a Task to run.

---

### 2.2 VmContext

A `VmContext` encapsulates:

- A private heap
- Type & function tables
- Global variables
- Task registry belonging to the context
- Resource counters & limits

**No heap object in one VmContext may directly reference objects in another.**

---

## 3. Isolation Guarantees

Raya enforces:

- **Heap isolation** — No cross-context pointers
- **Type isolation** — Metadata tables are context-scoped
- **Task isolation** — Tasks belong to one and only one VmContext
- **Capability isolation** — Host APIs are granted explicitly per-context

Values passed between VmContexts are **marshalled** (deep-copied or converted to value types). There is no shared mutable object state by default.

---

## 4. Resource Accounting & Limits

Each VmContext tracks resource usage:

- `heapBytesUsed`
- `taskCount`
- (optional) `stepsExecuted` as a CPU/fuel counter

Limits may be configured at creation time:

```typescript
interface VmOptions {
  maxHeapBytes?: number;
  maxTasks?: number;
  maxStepBudget?: number;   // per time slice or cumulative
}
```

**Exceeding a limit** results in controlled termination of the offending Task or the entire VmContext, with an error propagated to the caller.

This prevents untrusted or buggy code from exhausting global resources.

---

## 5. Controllability Surface

The host (outer Raya code) controls each inner VM via a well-defined API.

### 5.1 Lifecycle

```typescript
class Vm {
  constructor(options?: VmOptions);

  loadBytecode(bytes: ArrayBuffer): void;
  loadModule(name: string, sourceOrBytes: string | ArrayBuffer): void;

  runEntry(name: string, args?: unknown[]): Task<unknown>;
  spawn(funcName: string, args?: unknown[]): Task<unknown>;

  terminate(): void;               // kills all Tasks in this VmContext
}
```

### 5.2 Execution Control

- Tasks created inside a VmContext are returned as `Task` handles in the outer VM
- The outer code may await, cancel, or observe them
- An inner VM may host multiple Tasks; they fairly share the global scheduler

### 5.3 Observation & Instrumentation

```typescript
interface VmStats {
  heapBytesUsed: number;
  maxHeapBytes: number;
  tasks: number;
  maxTasks: number;
  stepsExecuted: number;
}

vm.getStats(): VmStats;
```

Instrumentation enables monitoring, throttling, and policy enforcement.

---

## 6. Capability Model

An inner VM does **not** automatically inherit host capabilities. It runs in a **sandbox** unless the outer code explicitly injects APIs.

**Example capability injection:**

```typescript
const vm = new Vm({
  capabilities: {
    log: (msg: string) => logger.info("[inner]", msg),
    query: (sql: string) => db.safeQuery(sql),
  }
});
```

These are exposed via a dedicated import namespace such as `host:`.

Capabilities are **purely opt-in** and allow fine-grained trust control.

---

## 7. Data Marshalling Boundary

Values crossing a context boundary are **marshalled**:

**Supported by default:**
- Primitives (number, string, boolean, null)
- Arrays of supported values
- Plain object maps `{ [key: string]: Value }`

**Optionally supported via handles:**
- Opaque `Foreign` references registered by the outer VM

This prevents pointer aliasing across heaps.

---

## 8. Scheduling on Shared Threads

Although VMs are isolated, they **share worker threads**. The scheduler:

- Maintains a global READY queue (or per-worker deques)
- Pulls Tasks regardless of context
- Switches current VmContext when running each Task

This enables **efficient parallelism** while retaining isolation.

---

## 9. Interaction with Snapshotting

Each VmContext is **independently snapshot-capable**:

```typescript
const snap = vm.snapshot();
vm.restore(snap);
```

When the outer runtime snapshots the entire process, each VmContext's state is embedded as an opaque blob. Resume reconstructs all contexts, heaps, Tasks, and scheduler state consistently.

Only **pure Raya state** is guaranteed snapshot-safe; external resources must be abstracted.

---

## 10. Error Containment

Errors inside an inner VM are **contained**:

- Runtime errors terminate the Task and propagate via its Task handle
- Limit violations (OOM, step budget) propagate as `VmError`
- Crashes or panics inside an inner VM do not corrupt the outer runtime

The outer VM may:
- Retry
- Terminate the VmContext
- Recreate a new one

---

## 11. Security & Safety Principles

1. **Least privilege** — Inner VMs have no host access unless granted
2. **Strong isolation** — Heaps & metadata per-context
3. **Deterministic control** — Tasks are controllable & observable
4. **Bounded execution** — Memory & CPU budgets
5. **Portable state** — Snapshot-safe pure Raya state

---

## 12. Example Usage

```typescript
import { Vm, Compiler } from "raya:vm";

const compiler = new Compiler();
const program = compiler.compileString(`
  export async function main() {
    return 42;
  }
`);

const vm = new Vm({ maxHeapBytes: 16 * 1024 * 1024 });
vm.loadBytecode(program.bytecode);

const task = vm.runEntry("main");
const result = await task;  // → 42
```

The inner VM runs on the same scheduler threads as the outer code but remains fully isolated and controllable.

---

## 13. Future Enhancements

- Namespaces for multi-tenancy & QoS scheduling
- Deterministic CPU quotas per context
- Explicit cancellation & pre-emption APIs
- Fine-grained capability descriptors

---

## 14. Resource Control & Anti-Starvation Guarantees

To ensure that an inner VmContext cannot starve the host runtime or other VmContexts, Raya enforces resource control at both accounting and scheduling layers.

### 14.1 CPU / Instruction Budgeting

Each VmContext may be configured with **fuel-based execution control**:

- Every executed instruction decrements a per-context step counter
- When the step counter falls to zero, the Task must yield back to the global scheduler
- The budget is then replenished based on policy (time slice / quota)

This ensures:
- Cooperative fairness across all contexts
- Prevention of runaway tight loops
- Predictable latency for host Tasks

If a hard limit is exceeded (e.g., cumulative step budget), the Task or VmContext may be terminated with a controlled error.

---

### 14.2 Fair Scheduling Across Contexts

Even though all Tasks share the same worker threads, the scheduler applies **fairness policies** across VmContexts, such as:

- Round-robin / weighted queues per VmContext
- Maximum runnable Tasks per context per slice
- Optional QoS / priority tiers

This prevents a single VmContext from flooding the READY queue.

---

### 14.3 Memory Pressure & Back-Pressure

Resource controls include:

- `maxHeapBytes` hard cap per VmContext
- Metrics-driven soft limits for proactive throttling

When approaching limits, a VmContext may:
- Block new allocations
- Reject new Task creation
- Signal pressure to the outer VM

---

### 14.4 I/O & Capability Throttling

Host-provided capabilities may themselves enforce quotas, e.g.:

- Max concurrent database calls
- Rate-limited logging
- Bounded mailbox size

This prevents indirect starvation vectors.

---

### 14.5 Observability for Governance

Outer code can monitor VmContext usage via stats APIs and react accordingly, including:

- Pausing
- Snapshotting
- Terminating
- Migrating via snapshot + restore

These controls make multi-tenant & sandboxed execution **safe by default**.

---

## 15. Summary

Inner VMs in Raya provide a **safe, composable execution substrate** that can be created, controlled, resource-limited, monitored, and snapshotted — all from within Raya code — while sharing the same global scheduler and OS threads. This design enables sandboxing, plugin systems, multi-tenant compute, and metaprogramming with strong guarantees around safety and determinism.
