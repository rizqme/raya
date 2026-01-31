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
pub struct SnapshotWriter {
    buffer: Vec<u8>,
}

writer.write_header(&vm)
writer.write_tasks(&vm)
writer.write_heap(&vm)
writer.write_globals(&vm)
writer.finish() -> Vec<u8>
```

### SnapshotReader
```rust
pub struct SnapshotReader<'a> {
    data: &'a [u8],
    offset: usize,
}

reader.read_header() -> SnapshotHeader
reader.read_tasks() -> Vec<TaskSnapshot>
reader.read_heap() -> HeapSnapshot
reader.read_globals() -> GlobalsSnapshot
```

## Snapshot Process

### Creating a Snapshot
```rust
// 1. Pause all tasks (stop-the-world)
vm.pause_all_tasks();

// 2. Ensure all tasks are at safepoints
vm.wait_for_safepoints();

// 3. Write snapshot
let mut writer = SnapshotWriter::new();
writer.write(&vm);
let bytes = writer.finish();

// 4. Resume execution
vm.resume_all_tasks();
```

### Restoring a Snapshot
```rust
// 1. Load modules referenced by snapshot
let modules = load_modules(&snapshot_header.modules);

// 2. Create VM with snapshot
let mut vm = Vm::new();
let reader = SnapshotReader::new(&bytes);

// 3. Restore state
vm.restore_tasks(reader.read_tasks());
vm.restore_heap(reader.read_heap());
vm.restore_globals(reader.read_globals());

// 4. Resume execution
vm.run();
```

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
