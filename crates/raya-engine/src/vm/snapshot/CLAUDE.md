# snapshot module

VM state serialization for pause/resume functionality.

## Overview

Enables capturing the complete VM state to disk and resuming execution later. Useful for:
- Long-running computations
- Migration between machines
- Debugging and time-travel

## Module Structure

```
snapshot/
├── mod.rs      # Re-exports, SnapshotReader, SnapshotWriter
├── task.rs     # SerializedTask, SerializedFrame, BlockedReason
├── format.rs   # Binary format definitions
├── encode.rs   # State serialization
└── decode.rs   # State deserialization
```

## Snapshot Format

```
┌─────────────────────────────────────────┐
│ Header                                  │
│   magic: [u8; 4]     "RSNA"            │
│   version: u32                          │
│   flags: u32                            │
│   timestamp: u64                        │
│   checksum: [u8; 32] (SHA-256)         │
├─────────────────────────────────────────┤
│ Module References                       │
│   List of (module_name, checksum)       │
├─────────────────────────────────────────┤
│ Task States                             │
│   For each task:                        │
│     - TaskId, state                     │
│     - IP, function index                │
│     - Call stack frames                 │
│     - Operand stack                     │
│     - Blocked reason (if any)          │
├─────────────────────────────────────────┤
│ Heap Snapshot                           │
│   All live objects with references      │
├─────────────────────────────────────────┤
│ Global State                            │
│   Global variables                      │
│   Mutex states                          │
│   Channel states (if any)              │
└─────────────────────────────────────────┘
```

## Key Types

### SnapshotWriter
```rust
pub struct SnapshotWriter { /* internal buffer */ }
writer.add_task(SerializedTask)     // add a task to the snapshot
writer.write_to_file(&Path)         // write binary snapshot to file
writer.write_snapshot(&mut impl Write)  // write to any writer
```

### SnapshotReader
```rust
pub struct SnapshotReader { /* parsed snapshot data */ }
SnapshotReader::from_file(&Path)    // read from file
SnapshotReader::from_reader(&mut impl Read)  // read from any reader
reader.tasks() -> &[SerializedTask] // get deserialized tasks
```

### Task ↔ SerializedTask Bridge
```rust
// scheduler::Task has snapshot bridge methods:
task.to_serialized() -> SerializedTask    // live task → snapshot format
Task::from_serialized(SerializedTask, Arc<Module>) -> Task  // restore from snapshot
```

Mappings: `ExecutionFrame` ↔ `SerializedFrame`, `SuspendReason` ↔ `BlockedReason`

## Snapshot Process

### Creating a Snapshot
```rust
// Via Vm facade (call when no tasks are actively executing):
let bytes = vm.snapshot_to_bytes()?;
// or
vm.snapshot_to_file(&path)?;
```

### Restoring a Snapshot
```rust
// 1. Load modules first (snapshot references them by heuristic)
vm.load_rbin_bytes(&module_bytes)?;

// 2. Restore snapshot
vm.restore_from_bytes(&snap_bytes)?;
// or
vm.restore_from_file(&path)?;
```

**Note:** Heap serialization is future work — only task state is currently persisted.

## Value Encoding

Values are encoded with type tags:
```
TAG_NULL    = 0x00
TAG_BOOL    = 0x01  + u8 (0/1)
TAG_I32     = 0x02  + i32
TAG_I64     = 0x03  + i64
TAG_F64     = 0x04  + f64
TAG_STRING  = 0x05  + u32 (heap ref)
TAG_OBJECT  = 0x06  + u32 (heap ref)
TAG_ARRAY   = 0x07  + u32 (heap ref)
TAG_CLOSURE = 0x08  + u32 (heap ref)
TAG_TASK    = 0x09  + TaskId
```

## Safepoints

Tasks must be at safepoints for consistent snapshots:
- Between bytecode instructions
- Not in the middle of native calls
- Not holding internal VM locks

```rust
// In interpreter loop
if self.safepoint_requested() {
    self.enter_safepoint();
    // Wait for snapshot to complete
    self.exit_safepoint();
}
```

## For AI Assistants

- Snapshots capture COMPLETE VM state
- All tasks paused during snapshot (stop-the-world)
- Heap objects serialized with reference graph
- Module bytecode NOT included (referenced by checksum)
- Safepoints ensure consistent state
- Checksums verify snapshot integrity
- Cross-machine restore requires same module versions
