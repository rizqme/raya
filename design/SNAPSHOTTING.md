# Raya VM Snapshotting Design

A specification for safe pause, snapshot, transfer, and resume semantics in the Raya Virtual Machine

---

## 1. Overview & Goals

**Snapshotting** allows a running Raya VM to be paused at a safe point, have its entire execution state serialized into a portable snapshot, and later be resumed from that snapshot — either in the same process or a compatible Raya VM instance elsewhere.

Snapshotting is a first-class runtime capability and is intentionally supported **without a JIT**, meaning the full execution state lives in bytecode, metadata, and heap structures that are **deterministic and serializable**.

### Goals

- Support stop-the-world snapshotting of all Raya execution state
- Support resume from that snapshot with identical semantics
- Make snapshot blobs portable across compatible VM versions / systems
- Ensure snapshots are consistent, safe, and deterministic
- Allow multiple VmContexts snapshotting independently or together
- Integrate cleanly with Task-based concurrency & await semantics

### Non-goals (v1)

- Snapshotting OS-level resources (sockets, files, devices)
- Cross-version migration between incompatible bytecode/runtime formats
- Incremental snapshotting (eventual direction but not required initially)

---

## 2. What State is Captured

A snapshot captures **pure Raya VM state only**.

### 2.1 VM Graph

For one or more VmContexts:

- Function & type tables
- Module metadata
- Global variables
- Interned constants / strings

### 2.2 Heap

All allocated objects:

- Objects & structs
- Arrays & maps
- Strings & buffers
- Class instances

The heap is serialized using **stable object IDs** to preserve reference identity.

### 2.3 Tasks (Green Threads)

For every Task:

- Task ID & status
- Owning VmContext
- Current function reference
- Instruction pointer (bytecode offset)
- Call stack frames
  - locals
  - arguments
  - return IPs
- Stored result/error where applicable
- Blocked reason (await, mutex, I/O token, etc.)

### 2.4 Scheduler State

- Ready queues
- Blocked lists
- Per-context resource counters

### 2.5 Synchronization Primitives

For each Mutex:

- State (locked/unlocked)
- Owning Task ID
- Wait queue

---

## 3. Snapshotting Model

Snapshotting uses a **stop-the-world protocol** with **safepoints**.

### 3.1 Safepoints

A Task may be paused only at defined safepoints:

- At function calls / returns
- At backward jumps (loop heads)
- Around `await`
- Around `yield`
- Periodically (N instructions)

Safepoints guarantee stack & frame integrity.

### 3.2 Stop-The-World Protocol

1. Host requests pause
2. Runtime sets `pauseRequested = true`
3. Workers run until each Task reaches a safepoint
4. Workers park; no Task is mid-instruction
5. Runtime marks system quiescent
6. Snapshot serialization begins

GC cooperation enables consistent heap traversal during this pause.

---

## 4. Snapshot Format

Snapshots are logically composed of segments:

### 1. Header

- snapshot version
- Raya VM version compatibility
- endianness, word size

### 2. Metadata

- module table
- function table
- type table

### 3. Heap Segment

- object graph with IDs
- primitive inline data

### 4. Task Segment

- per-task execution state

### 5. Scheduler Segment

- queue state
- runnable vs blocked

### 6. Sync Segment

- all Mutex + synchronization objects

### 7. Integrity checksum

Snapshots must be **validatable before resume**.

---

## 5. Resume Semantics

Resuming from snapshot:

1. Create a new empty runtime
2. Validate header & compatibility
3. Reconstruct metadata tables
4. Allocate heap objects using stored IDs
5. Patch references
6. Rebuild Tasks & call stacks
7. Re-establish scheduler queues
8. Start Workers

Execution resumes **as if no time passed**.

---

## 6. Multi-Context Snapshotting

Raya supports multiple `VmContext`s on a shared worker scheduler.

Snapshots may:

- Serialize individual VmContexts
- Serialize the entire runtime state

Each Task is tagged with its owning context.

Heaps remain isolated; cross-context marshaling results in deep copies or handles.

---

## 7. Interaction with Async / Await

Tasks blocked on `await` are captured as:

```
status = BLOCKED
blockedOnTaskId = X
```

When resumed:

- Completion of X reactivates waiters normally

`await` therefore remains **deterministic** across snapshot boundaries.

---

## 8. External Resource Policy

Raya does **not** guarantee snapshot safety for OS resources such as:

- file descriptors
- network sockets
- device handles

These must be wrapped by **logical handles** or **application-level recovery layers**.

VM-level guarantee applies to **pure Raya state only**.

---

## 9. Safety & Determinism

Snapshotting guarantees:

- No torn frames or partial writes
- Atomic Task & heap visibility
- Deterministic resume semantics

Errors during resume must:

- be surfaced to the host
- never corrupt runtime state

---

## 10. Performance Considerations

- Snapshotting is stop-the-world, similar to GC pause
- Optimizations may include:
  - incremental heap diffing (future)
  - parallel serialization
  - compression

---

## 11. API Surface (Draft)

```typescript
class Vm {
  pause(): void;                // reaches safepoint globally
  snapshot(): Snapshot;         // returns blob
  resume(snapshot: Snapshot): void;
}

class Snapshot {
  toBytes(): ArrayBuffer;
  static fromBytes(buf: ArrayBuffer): Snapshot;
}
```

---

## 12. Failure Handling

Errors during pause, snapshot, or resume must:

- fail predictably
- NOT leave runtime partially paused

Examples include:

- incompatible runtime
- corrupted snapshot
- resource limits exceeded during reconstruction

---

## 13. Future Extensions

- Incremental snapshotting
- Differential replication
- Deterministic replay & time control
- Distributed cross-node resume

---

## 14. Summary

Snapshotting turns the Raya VM into a **portable, stoppable, resumable execution substrate**. By leaning on its typed bytecode model, Task-based concurrency, and shared-thread scheduling, Raya provides a clean and deterministic way to capture program execution state without JIT complexity — an ideal foundation for sandboxing, migration, debugging, and stateful compute orchestration.
