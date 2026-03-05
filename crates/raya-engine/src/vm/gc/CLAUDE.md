# gc module

_Verified against source on 2026-03-06._

Garbage collector for Raya VM heap management.

## Overview

Manages memory allocation and reclamation for heap-allocated values (objects, arrays, strings, closures). Uses a mark-sweep algorithm with stop-the-world pauses.

## Module Structure

```
gc/
├── mod.rs       # Entry point, GC struct
├── heap.rs      # Shared heap allocation
├── nursery.rs   # Per-task bump allocator (NEW)
├── mark.rs      # Mark phase
├── sweep.rs     # Sweep phase
├── ptr.rs       # Smart pointers (ObjectRef, ArrayRef, etc.)
└── header.rs    # Object headers
```

## Current Status

**Status: Active Development**

The GC uses mark-sweep collection with a **per-task nursery allocator** to reduce lock contention.

### Nursery Allocator (NEW)

Each task has a 64KB bump allocator for short-lived objects:

```rust
pub struct Nursery {
    buffer: UnsafeCell<Vec<u8>>,    // 64KB backing buffer
    cursor: UnsafeCell<usize>,       // Bump pointer
    capacity: usize,                 // 64 * 1024
    allocation_count: UnsafeCell<usize>,
}
```

**Allocation Strategy:**
- Fast bump allocation (no GC lock acquired)
- Objects either:
  - Promoted to shared heap when they escape (stored in globals, channels, etc.)
  - Discarded en masse when nursery resets (at task completion or when full)
- Fallback to shared GC when nursery is full

**Benefits:**
- Zero GC lock contention for temporary allocations
- Fast allocation (pointer bump only)
- Reduced pressure on shared GC

```rust
// Usage in task
let nursery = Nursery::new();
let ptr = unsafe { nursery.allocate(value) };  // Fast, no lock

// When full or task completes
unsafe { nursery.reset(); }  // All pointers invalidated
```

## Planned Design

### Mark-Sweep Algorithm

```
1. Stop the World
   - Pause all tasks at safepoints
   - Ensure consistent heap view

2. Mark Phase
   - Start from roots (stack, globals, registers)
   - Traverse object graph
   - Mark all reachable objects

3. Sweep Phase
   - Scan entire heap
   - Free unmarked objects
   - Coalesce free blocks

4. Resume
   - Resume all tasks
```

### Root Set

Roots are values that keep objects alive:

```rust
pub struct RootSet {
    stack_roots: Vec<*const Value>,    // Task stacks
    global_roots: Vec<*const Value>,   // Global variables
    native_pins: HashSet<*const ()>,   // Pinned by native calls
}
```

### Heap Layout

```
┌─────────────────────────────────────────┐
│         Per-Task Nursery (64KB)         │
├─────────────────────────────────────────┤
│ Bump allocator for short-lived objects │
│ Reset on task completion or when full   │
└─────────────────────────────────────────┘
                  ↓ (on escape)
┌─────────────────────────────────────────┐
│           Shared GC Heap                │
├─────────────────────────────────────────┤
│ Object | Array | Free | String | ...    │
├─────────────────────────────────────────┤
│ Block Header:                           │
│   - size: u32                           │
│   - marked: bool                        │
│   - type_tag: u8                        │
└─────────────────────────────────────────┘
```

### GC Interface

```rust
pub struct GC {
    heap: Heap,
    roots: RootSet,
    bytes_allocated: usize,
    threshold: usize,
}

// Allocation
gc.allocate<T>(size) -> *mut T
gc.allocate_string(data) -> StringRef
gc.allocate_array(len) -> ArrayRef
gc.allocate_object(class_id) -> ObjectRef

// Collection
gc.collect()
gc.should_collect() -> bool

// Root management
gc.add_root(ptr)
gc.remove_root(ptr)

// Native call pinning
gc.pin(ptr) -> PinGuard
gc.unpin(guard)
```

### Write Barriers

For generational GC (future):

```rust
// Called when writing a reference
fn write_barrier(object: &Object, field: &mut Value, new_value: Value) {
    if gc.is_old(object) && gc.is_young(new_value) {
        gc.remember_set.add(object);
    }
    *field = new_value;
}
```

## Safepoints

GC can only run when all tasks are at safepoints:

```rust
// In interpreter loop
if gc.should_collect() {
    self.enter_safepoint();
    gc.collect();
    self.exit_safepoint();
}
```

## Planned Features

| Feature | Status |
|---------|--------|
| Basic allocation | Complete |
| Nursery allocator | **Complete** ✅ |
| Mark phase | Partial |
| Sweep phase | Partial |
| Stop-the-world | TODO |
| Root tracking | Partial |
| Native pinning | Complete |
| JSON GC safety | **Complete** ✅ |
| Generational GC | Future |
| Concurrent marking | Future |

## For AI Assistants

- **Nursery allocator** (64KB per-task bump allocator) reduces GC lock contention for temporary objects
- Objects escape to shared heap when stored in globals/channels
- Mark-sweep is the collection algorithm (partial implementation)
- Stop-the-world required for consistency
- Native code must pin values during calls
- Safepoints enable safe collection points
- JSON parsing now pins objects to prevent premature GC
- Future: generational for better performance
- Future: concurrent marking for lower latency


<!-- AUTO-FOLDER-SNAPSHOT:START -->
## Auto Folder Snapshot

- Updated: 2026-03-06
- Directory: `crates/raya-engine/src/vm/gc`
- Direct subdirectories: (none)
- Direct files (excluding `CLAUDE.md`): collector.rs, header.rs, heap.rs, mod.rs, nursery.rs, ptr.rs, roots.rs
- Rust files in this directory: collector.rs, header.rs, heap.rs, mod.rs, nursery.rs, ptr.rs, roots.rs

<!-- AUTO-FOLDER-SNAPSHOT:END -->
