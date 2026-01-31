# gc module

Garbage collector for Raya VM heap management.

## Overview

Manages memory allocation and reclamation for heap-allocated values (objects, arrays, strings, closures). Uses a mark-sweep algorithm with stop-the-world pauses.

## Module Structure

```
gc/
├── mod.rs       # Entry point, GC struct
├── heap.rs      # Heap allocation
├── mark.rs      # Mark phase
├── sweep.rs     # Sweep phase
└── roots.rs     # Root set management
```

## Current Status

**Status: Placeholder/Minimal**

The GC is currently minimal - basic allocation without collection. Full implementation is planned.

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
│              Heap                        │
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
| Mark phase | Partial |
| Sweep phase | TODO |
| Stop-the-world | TODO |
| Root tracking | Partial |
| Native pinning | Complete |
| Generational GC | Future |
| Concurrent marking | Future |

## For AI Assistants

- GC is currently minimal (no collection)
- Mark-sweep is the planned algorithm
- Stop-the-world required for consistency
- Native code must pin values during calls
- Safepoints enable safe collection points
- Future: generational for better performance
- Future: concurrent marking for lower latency
