# Milestone 7.1: VM Snapshotting

**Goal:** Implement safe pause, snapshot, transfer, and resume semantics for the Raya VM

**Depends on:**
- Milestone 2: Core VM features (heap, GC)
- Milestone 4: Task scheduler and concurrency primitives
- Milestone 1.2: Bytecode encoding/decoding

**Reference:** [design/SNAPSHOTTING.md](../design/SNAPSHOTTING.md)

---

## Overview

This milestone implements VM snapshotting as a first-class runtime capability, allowing the entire execution state to be serialized to a portable snapshot and later resumed.

### Key Features

- Stop-the-world snapshotting protocol
- Safepoint-based task suspension
- Complete state serialization (heap, tasks, scheduler, mutexes)
- Portable snapshot format with versioning
- Deterministic resume semantics
- Multi-context snapshotting support

---

## Task Breakdown

### Task 1: Safepoint Infrastructure

**Goal:** Implement safepoint mechanism for safe task suspension

**Subtasks:**

1. **Add Safepoint Enum**
   ```rust
   // crates/raya-core/src/safepoint.rs
   pub enum SafepointReason {
       FunctionCall,
       FunctionReturn,
       BackwardJump,
       Await,
       Yield,
       Periodic(usize), // Every N instructions
   }
   ```

2. **Extend Interpreter Loop**
   - Add safepoint checks in interpreter main loop
   - Check for pause request at each safepoint
   - Implement cooperative suspension

3. **Worker Thread Coordination**
   - Add pause request flag (atomic bool)
   - Implement worker parking mechanism
   - Add quiescence detection (all workers parked)

**Files:**
- `crates/raya-core/src/safepoint.rs` (new)
- `crates/raya-core/src/vm/interpreter.rs` (modify)
- `crates/raya-core/src/scheduler/worker.rs` (modify)

**Tests:**
- [ ] Single task reaches safepoint
- [ ] Multiple tasks reach safepoints
- [ ] All workers park correctly
- [ ] Resume after pause works

**Estimated Time:** 3-4 days

---

### Task 2: Snapshot Format Definition

**Goal:** Define serialization format for VM snapshots

**Subtasks:**

1. **Define Snapshot Header**
   ```rust
   // crates/raya-snapshot/src/format.rs
   pub struct SnapshotHeader {
       pub magic: [u8; 4],           // "SNAP"
       pub version: u32,              // Snapshot format version
       pub vm_version: u32,           // Compatible VM version
       pub endianness: u8,            // 0 = little, 1 = big
       pub word_size: u8,             // 4 or 8 bytes
       pub created_at: u64,           // Unix timestamp
       pub checksum: u32,             // CRC32 of all segments
   }
   ```

2. **Define Segment Types**
   ```rust
   pub enum SegmentType {
       Metadata,       // Module/function/type tables
       Heap,          // Object graph
       Tasks,         // Task execution state
       Scheduler,     // Queue state
       Synchronization, // Mutexes, channels, etc.
   }

   pub struct Segment {
       pub segment_type: SegmentType,
       pub size: u32,
       pub data: Vec<u8>,
   }
   ```

3. **Create Snapshot Container**
   ```rust
   pub struct Snapshot {
       pub header: SnapshotHeader,
       pub metadata: MetadataSegment,
       pub heap: HeapSegment,
       pub tasks: TaskSegment,
       pub scheduler: SchedulerSegment,
       pub sync: SyncSegment,
   }
   ```

**Files:**
- `crates/raya-snapshot/Cargo.toml` (new crate)
- `crates/raya-snapshot/src/lib.rs` (new)
- `crates/raya-snapshot/src/format.rs` (new)

**Tests:**
- [ ] Header serialization roundtrip
- [ ] Segment encoding/decoding
- [ ] Version compatibility checking
- [ ] Checksum validation

**Estimated Time:** 2-3 days

---

### Task 3: Metadata Snapshot

**Goal:** Serialize VM metadata tables

**Subtasks:**

1. **Serialize Module Table**
   - Capture all loaded modules
   - Include module metadata
   - Preserve module IDs

2. **Serialize Function Table**
   - Capture function definitions
   - Include bytecode
   - Preserve function IDs

3. **Serialize Type Table**
   - Capture type definitions
   - Include class layouts
   - Preserve type IDs

4. **Serialize Global State**
   - Capture global variables
   - Include interned strings
   - Preserve constant pool

**Files:**
- `crates/raya-snapshot/src/metadata.rs` (new)
- `crates/raya-core/src/vm/context.rs` (modify - add serialization methods)

**Tests:**
- [ ] Empty VM snapshot
- [ ] Single module snapshot
- [ ] Multiple modules snapshot
- [ ] Globals preservation

**Estimated Time:** 3-4 days

---

### Task 4: Heap Snapshot

**Goal:** Serialize the GC heap with object graph integrity

**Subtasks:**

1. **Implement Object ID Assignment**
   ```rust
   pub struct ObjectId(u64);

   impl Gc {
       pub fn assign_object_ids(&mut self) -> HashMap<GcPtr, ObjectId> {
           // Assign stable IDs to all heap objects
       }
   }
   ```

2. **Serialize Object Graph**
   - Walk heap starting from roots
   - Serialize each object with its ID
   - Preserve reference relationships
   - Handle cycles correctly

3. **Serialize Different Object Types**
   - Plain objects (`{ x: 1, y: 2 }`)
   - Arrays
   - Strings
   - Class instances
   - Closures

4. **Implement Heap Reconstruction**
   - Allocate objects in correct order
   - Patch references using ID map
   - Restore object layouts

**Files:**
- `crates/raya-snapshot/src/heap.rs` (new)
- `crates/raya-core/src/gc.rs` (modify - add snapshot support)

**Tests:**
- [ ] Simple object snapshot
- [ ] Object graph with references
- [ ] Cyclic references
- [ ] Array snapshot
- [ ] String snapshot
- [ ] Mixed object types

**Estimated Time:** 5-6 days

---

### Task 5: Task State Snapshot

**Goal:** Capture execution state of all tasks

**Subtasks:**

1. **Define Task State Structure**
   ```rust
   pub struct TaskSnapshot {
       pub task_id: TaskId,
       pub status: TaskStatus,
       pub context_id: VmContextId,
       pub call_stack: Vec<Frame>,
       pub blocked_on: Option<BlockedReason>,
   }

   pub struct Frame {
       pub function_id: FunctionId,
       pub instruction_pointer: usize,
       pub locals: Vec<Value>,
       pub operand_stack: Vec<Value>,
   }

   pub enum BlockedReason {
       Await(TaskId),
       Mutex(MutexId),
       IoToken(u64),
   }
   ```

2. **Serialize Task State**
   - Capture all task metadata
   - Serialize call stacks
   - Preserve instruction pointers
   - Save blocked reasons

3. **Serialize Value Stack**
   - Convert stack values to portable format
   - Handle object references (use ObjectIds)

4. **Implement Task Reconstruction**
   - Recreate task structures
   - Restore call stacks
   - Re-establish blocking relationships

**Files:**
- `crates/raya-snapshot/src/task.rs` (new)
- `crates/raya-core/src/task.rs` (modify - add snapshot support)

**Tests:**
- [ ] Single task snapshot (running)
- [ ] Task with call stack
- [ ] Blocked task (await)
- [ ] Multiple tasks
- [ ] Task with locals and stack

**Estimated Time:** 4-5 days

---

### Task 6: Scheduler State Snapshot

**Goal:** Capture scheduler queues and state

**Subtasks:**

1. **Define Scheduler State**
   ```rust
   pub struct SchedulerSnapshot {
       pub ready_tasks: Vec<TaskId>,
       pub blocked_tasks: HashMap<TaskId, BlockedReason>,
       pub per_context_counters: HashMap<VmContextId, Counters>,
   }
   ```

2. **Serialize Queue State**
   - Capture ready queue contents
   - Capture blocked task list
   - Preserve task ordering (if needed)

3. **Implement Scheduler Reconstruction**
   - Rebuild ready queues
   - Rebuild blocked lists
   - Restore task priorities

**Files:**
- `crates/raya-snapshot/src/scheduler.rs` (new)
- `crates/raya-core/src/scheduler/mod.rs` (modify)

**Tests:**
- [ ] Empty scheduler
- [ ] Scheduler with ready tasks
- [ ] Scheduler with blocked tasks
- [ ] Multiple contexts

**Estimated Time:** 2-3 days

---

### Task 7: Synchronization Primitives Snapshot

**Goal:** Capture state of mutexes and other sync primitives

**Subtasks:**

1. **Define Mutex State**
   ```rust
   pub struct MutexSnapshot {
       pub mutex_id: MutexId,
       pub locked: bool,
       pub owner: Option<TaskId>,
       pub wait_queue: Vec<TaskId>,
   }
   ```

2. **Serialize Mutex State**
   - Capture lock status
   - Preserve owner information
   - Save wait queue

3. **Implement Mutex Reconstruction**
   - Recreate mutex objects
   - Restore lock state
   - Rebuild wait queues

**Files:**
- `crates/raya-snapshot/src/sync.rs` (new)
- `crates/raya-core/src/sync/mutex.rs` (modify)

**Tests:**
- [ ] Unlocked mutex
- [ ] Locked mutex
- [ ] Mutex with waiters
- [ ] Multiple mutexes

**Estimated Time:** 2-3 days

---

### Task 8: Stop-The-World Protocol

**Goal:** Implement safe global pause mechanism

**Subtasks:**

1. **Implement Pause Request**
   ```rust
   impl Vm {
       pub fn request_pause(&self) {
           self.pause_requested.store(true, Ordering::SeqCst);
       }

       pub fn wait_for_quiescence(&self) {
           // Wait until all workers are parked
       }
   }
   ```

2. **Implement Worker Parking**
   - Each worker checks pause flag at safepoints
   - Workers park when pause requested
   - Signal when parked

3. **Implement Quiescence Detection**
   - Track parked worker count
   - Signal when all workers parked
   - Ensure no task is mid-instruction

**Files:**
- `crates/raya-core/src/vm/mod.rs` (modify)
- `crates/raya-core/src/scheduler/worker.rs` (modify)

**Tests:**
- [ ] Single worker pause
- [ ] Multiple workers pause
- [ ] Pause during computation
- [ ] Pause with blocked tasks

**Estimated Time:** 3-4 days

---

### Task 9: Snapshot API Implementation

**Goal:** Implement high-level snapshot/resume API

**Subtasks:**

1. **Implement Snapshot Creation**
   ```rust
   impl Vm {
       pub fn pause(&mut self) -> Result<(), SnapshotError> {
           self.request_pause();
           self.wait_for_quiescence()?;
           Ok(())
       }

       pub fn snapshot(&self) -> Result<Snapshot, SnapshotError> {
           // Serialize all state
           let mut snapshot = Snapshot::new();
           snapshot.metadata = self.snapshot_metadata()?;
           snapshot.heap = self.snapshot_heap()?;
           snapshot.tasks = self.snapshot_tasks()?;
           snapshot.scheduler = self.snapshot_scheduler()?;
           snapshot.sync = self.snapshot_sync()?;
           snapshot.finalize(); // Calculate checksum
           Ok(snapshot)
       }
   }
   ```

2. **Implement Resume**
   ```rust
   impl Vm {
       pub fn resume(snapshot: Snapshot) -> Result<Self, SnapshotError> {
           snapshot.validate()?;

           let mut vm = Vm::new_empty();
           vm.restore_metadata(&snapshot.metadata)?;
           vm.restore_heap(&snapshot.heap)?;
           vm.restore_tasks(&snapshot.tasks)?;
           vm.restore_scheduler(&snapshot.scheduler)?;
           vm.restore_sync(&snapshot.sync)?;

           Ok(vm)
       }
   }
   ```

3. **Implement Snapshot Persistence**
   ```rust
   impl Snapshot {
       pub fn to_bytes(&self) -> Vec<u8> {
           // Serialize to binary format
       }

       pub fn from_bytes(data: &[u8]) -> Result<Self, SnapshotError> {
           // Deserialize from binary
       }

       pub fn save_to_file(&self, path: &Path) -> io::Result<()> {
           // Write to file
       }

       pub fn load_from_file(path: &Path) -> Result<Self, SnapshotError> {
           // Read from file
       }
   }
   ```

**Files:**
- `crates/raya-core/src/vm/snapshot.rs` (new)
- `crates/raya-snapshot/src/lib.rs` (modify)

**Tests:**
- [ ] Snapshot empty VM
- [ ] Snapshot running program
- [ ] Resume and continue execution
- [ ] Snapshot to file and reload
- [ ] Multiple snapshot/resume cycles

**Estimated Time:** 4-5 days

---

### Task 10: Multi-Context Snapshotting

**Goal:** Support snapshotting individual or all VmContexts

**Subtasks:**

1. **Implement Context-Specific Snapshots**
   ```rust
   impl Vm {
       pub fn snapshot_context(&self, context_id: VmContextId) -> Result<ContextSnapshot, SnapshotError> {
           // Snapshot only one context
       }
   }
   ```

2. **Implement Full VM Snapshots**
   ```rust
   impl Vm {
       pub fn snapshot_all(&self) -> Result<Snapshot, SnapshotError> {
           // Snapshot all contexts
       }
   }
   ```

3. **Handle Cross-Context References**
   - Tag objects with owning context
   - Handle shared heap state
   - Maintain context isolation

**Files:**
- `crates/raya-core/src/vm/snapshot.rs` (modify)

**Tests:**
- [ ] Single context snapshot
- [ ] Multi-context full snapshot
- [ ] Context isolation preserved

**Estimated Time:** 2-3 days

---

### Task 11: Error Handling and Validation

**Goal:** Ensure robust error handling and snapshot validation

**Subtasks:**

1. **Define Error Types**
   ```rust
   #[derive(Debug, Error)]
   pub enum SnapshotError {
       #[error("Incompatible VM version: snapshot {snapshot}, current {current}")]
       IncompatibleVersion { snapshot: u32, current: u32 },

       #[error("Corrupted snapshot: checksum mismatch")]
       CorruptedSnapshot,

       #[error("Failed to reach quiescence: timeout")]
       QuiescenceTimeout,

       #[error("Invalid object reference: {0}")]
       InvalidObjectRef(ObjectId),
   }
   ```

2. **Implement Snapshot Validation**
   - Check header magic number
   - Verify version compatibility
   - Validate checksum
   - Verify object graph integrity

3. **Implement Graceful Failure**
   - Never leave VM in partially paused state
   - Clean up on snapshot failure
   - Clean up on resume failure

**Files:**
- `crates/raya-snapshot/src/error.rs` (new)
- `crates/raya-snapshot/src/validate.rs` (new)

**Tests:**
- [ ] Incompatible version detection
- [ ] Corrupted snapshot detection
- [ ] Invalid references detection
- [ ] Recovery from failed snapshot
- [ ] Recovery from failed resume

**Estimated Time:** 2-3 days

---

### Task 12: Testing and Documentation

**Goal:** Comprehensive testing and documentation

**Subtasks:**

1. **Integration Tests**
   - Snapshot during computation
   - Snapshot with async tasks
   - Snapshot with mutexes
   - Cross-platform compatibility
   - Large snapshot stress test

2. **Benchmark Performance**
   - Time to pause
   - Time to snapshot
   - Time to resume
   - Snapshot size vs heap size

3. **Write Documentation**
   - API documentation
   - Usage examples
   - Performance characteristics
   - Limitations and caveats

4. **Create Examples**
   ```typescript
   // examples/snapshotting/checkpoint.raya
   async function longComputation(): Task<number> {
       let sum = 0;
       for (let i = 0; i < 1000000; i++) {
           sum += i;
           if (i % 100000 === 0) {
               // Checkpoint opportunity
           }
       }
       return sum;
   }
   ```

**Files:**
- `tests/snapshot_integration.rs` (new)
- `benches/snapshot_bench.rs` (new)
- `examples/snapshotting/` (new directory)
- `crates/raya-snapshot/README.md` (new)

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
- Task 4: 5-6 days
- Task 5: 4-5 days
- Task 6: 2-3 days
- Task 7: 2-3 days
- Task 8: 3-4 days
- Task 9: 4-5 days
- Task 10: 2-3 days
- Task 11: 2-3 days
- Task 12: 5-6 days

**Total:** 38-49 days (approximately 8-10 weeks)

---

## Success Criteria

- [ ] VM can be paused safely at safepoints
- [ ] Complete VM state can be serialized to snapshot
- [ ] Snapshot can be saved to file and loaded
- [ ] VM can resume from snapshot with identical behavior
- [ ] Multiple snapshot/resume cycles work correctly
- [ ] Snapshot format is portable across compatible VMs
- [ ] All tests pass
- [ ] Documentation is complete
- [ ] Performance is acceptable (pause < 100ms for typical programs)

---

## Future Enhancements

These are not part of this milestone but could be added later:

1. **Incremental Snapshotting**
   - Only serialize changed objects
   - Differential snapshots
   - Faster checkpoint times

2. **Compressed Snapshots**
   - Apply compression to reduce size
   - Trade CPU time for storage

3. **Distributed Resume**
   - Resume snapshot on different machine
   - Handle network transfer

4. **Deterministic Replay**
   - Record non-deterministic inputs
   - Replay execution from snapshot

5. **Time-Travel Debugging**
   - Multiple snapshots during execution
   - Step backwards in debugger

---

## Dependencies

**Required Rust Crates:**
- `serde` / `serde_json` - Serialization
- `crc32fast` - Checksums
- `parking_lot` - Synchronization

**Internal Dependencies:**
- `raya-bytecode` - For metadata serialization
- `raya-core` - VM runtime (all subsystems)

---

## Notes

- Snapshotting is designed for **pure Raya state only**
- OS resources (files, sockets) are **not** included
- Snapshot format must remain **stable** across versions
- Performance impact should be **minimal** when not snapshotting
- Resume must be **deterministic** - same snapshot always produces same behavior
