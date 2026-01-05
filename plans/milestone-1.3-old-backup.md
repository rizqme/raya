# Milestone 1.3: Memory Management & Garbage Collection

**Phase:** 1 - VM Core
**Crate:** `raya-core`
**Status:** Not Started
**Prerequisites:** Milestone 1.2 (Bytecode Definitions) ✅

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

Implement the memory management system and garbage collector for the Raya VM. This milestone provides the foundation for dynamic memory allocation and automatic memory reclamation, enabling the VM to safely execute programs with complex object graphs and concurrent tasks.

**Key Deliverable:** A fully functional memory management system with automatic garbage collection that supports:

- Efficient value representation (tagged pointers or NaN boxing)
- Fast heap allocation with minimal fragmentation
- Precise mark-sweep garbage collection
- GC root tracking across stacks, globals, and tasks
- Thread-safe GC pauses in multi-threaded scheduler
- Pointer maps for precise collection

---

## Goals

### Primary Goals

- ✅ Define efficient value representation strategy
- ✅ Implement heap allocator with memory pools
- ✅ Build precise mark-sweep garbage collector
- ✅ Track GC roots (stack, globals, tasks)
- ✅ Ensure GC safety in concurrent environment
- ✅ Achieve >90% test coverage

### Secondary Goals

- Optimize allocation performance (arena allocators)
- Add GC statistics and profiling
- Implement write barriers for future generational GC
- Support for weak references
- GC tuning parameters (threshold, heap size limits)

---

## Tasks

### Task 1: Value Representation

**File:** `crates/raya-core/src/value.rs`

**Checklist:**

- [ ] Choose value representation strategy
  - [ ] Option A: Tagged pointers (preferred for 64-bit)
  - [ ] Option B: NaN boxing (compact representation)
  - [ ] Option C: Enum-based (simpler, larger)
- [ ] Define `Value` type
  - [ ] Immediate values (i32, bool, null)
  - [ ] Heap-allocated values (String, Object, Array, Closure)
  - [ ] Tagged pointer encoding/decoding
- [ ] Implement `Value` constructors
  - [ ] `Value::null()`
  - [ ] `Value::bool(b: bool)`
  - [ ] `Value::i32(i: i32)`
  - [ ] `Value::f64(f: f64)`
  - [ ] `Value::string(s: GcPtr<String>)`
  - [ ] `Value::object(o: GcPtr<Object>)`
  - [ ] `Value::array(a: GcPtr<Array>)`
  - [ ] `Value::closure(c: GcPtr<Closure>)`
- [ ] Implement type checking methods
  - [ ] `is_null()`, `is_bool()`, `is_i32()`, `is_f64()`
  - [ ] `is_string()`, `is_object()`, `is_array()`, `is_closure()`
  - [ ] `is_number()` (i32 or f64)
  - [ ] `is_heap_allocated()`
- [ ] Implement value extraction methods
  - [ ] `as_bool()`, `as_i32()`, `as_f64()`
  - [ ] `as_string()`, `as_object()`, `as_array()`, `as_closure()`
  - [ ] Return `Option<T>` or panic variants
- [ ] Add equality and comparison
  - [ ] `eq()` for value equality
  - [ ] `identical()` for reference equality
- [ ] Implement `Display` and `Debug` traits
- [ ] Add conversion utilities
  - [ ] `to_bool()` for truthiness
  - [ ] `to_string()` for string representation
  - [ ] `to_number()` for numeric coercion

**Implementation Choice:**

**Tagged Pointer Strategy (64-bit):**
```rust
// Use lowest 3 bits as tag
// Pointer values: xxx...xxx000 (aligned, tag = 0)
// i32:           xxx...xxx001
// bool:          0000...0b010 (b = 0 or 1)
// null:          0000...00110
// Special:       xxx...xxNNN (NNN != 000)

pub struct Value(u64);

impl Value {
    const TAG_MASK: u64 = 0b111;
    const TAG_PTR: u64 = 0b000;
    const TAG_I32: u64 = 0b001;
    const TAG_BOOL: u64 = 0b010;
    const TAG_NULL: u64 = 0b110;

    pub fn i32(i: i32) -> Self {
        Value(((i as i64 as u64) << 32) | Self::TAG_I32)
    }

    pub fn is_i32(&self) -> bool {
        (self.0 & Self::TAG_MASK) == Self::TAG_I32
    }

    pub fn as_i32(&self) -> Option<i32> {
        if self.is_i32() {
            Some((self.0 >> 32) as i32)
        } else {
            None
        }
    }
}
```

**Tests:**
- [ ] Test value creation for all types
- [ ] Test type checking methods
- [ ] Test value extraction
- [ ] Test null representation
- [ ] Test bool representation (true/false)
- [ ] Test integer roundtrip (positive, negative, zero)
- [ ] Test float representation (normal, NaN, infinity)
- [ ] Test pointer alignment requirements
- [ ] Test equality comparison
- [ ] Test string representation

---

### Task 2: GC Pointer Type

**File:** `crates/raya-core/src/gc/ptr.rs`

**Checklist:**

- [ ] Define `GcPtr<T>` smart pointer type
  - [ ] Wrap raw pointer with type information
  - [ ] Add header with metadata before allocation
  - [ ] Store mark bit in header
  - [ ] Store type ID in header
- [ ] Implement `GcHeader` structure
  ```rust
  struct GcHeader {
      marked: bool,
      type_id: TypeId,
      size: usize,
  }
  ```
- [ ] Implement `GcPtr<T>` methods
  - [ ] `new(ptr: *mut T) -> GcPtr<T>`
  - [ ] `as_ptr() -> *mut T`
  - [ ] `header() -> &GcHeader`
  - [ ] `header_mut() -> &mut GcHeader`
  - [ ] `is_marked() -> bool`
  - [ ] `mark()`
  - [ ] `unmark()`
- [ ] Implement `Deref` and `DerefMut` traits
- [ ] Add safety documentation
  - [ ] Document lifetime guarantees
  - [ ] Document GC safety requirements
  - [ ] Note: pointers invalidated after collection
- [ ] Add `Clone` (shallow copy of pointer)
- [ ] Add `Debug` trait

**Tests:**
- [ ] Test GcPtr creation
- [ ] Test header access
- [ ] Test mark/unmark operations
- [ ] Test dereferencing
- [ ] Test pointer equality

---

### Task 3: Heap Allocator

**File:** `crates/raya-core/src/gc/heap.rs`

**Checklist:**

- [ ] Define `Heap` structure
  ```rust
  pub struct Heap {
      allocations: Vec<*mut u8>,
      allocated_bytes: usize,
      gc_threshold: usize,
      type_registry: TypeRegistry,
  }
  ```
- [ ] Implement allocation methods
  - [ ] `allocate<T>(&mut self, value: T) -> GcPtr<T>`
  - [ ] `allocate_array<T>(&mut self, len: usize) -> GcPtr<[T]>`
  - [ ] `allocate_string(&mut self, s: &str) -> GcPtr<String>`
- [ ] Add memory layout calculation
  - [ ] Align to 8 bytes for GC header
  - [ ] Calculate total size with header
  - [ ] Initialize header with metadata
- [ ] Track allocations
  - [ ] Store pointer to all allocations
  - [ ] Update `allocated_bytes` counter
  - [ ] Trigger GC when threshold exceeded
- [ ] Implement `free()` for deallocating
  - [ ] Called by GC sweep phase
  - [ ] Update counters
  - [ ] Actually free memory
- [ ] Add heap statistics
  - [ ] `total_allocated() -> usize`
  - [ ] `allocation_count() -> usize`
  - [ ] `fragmentation_ratio() -> f64`
- [ ] Implement heap limits
  - [ ] `set_max_heap_size(bytes: usize)`
  - [ ] Check limit before allocation
  - [ ] Throw OOM error if exceeded

**Allocation Strategy:**

```rust
impl Heap {
    pub fn allocate<T>(&mut self, value: T) -> GcPtr<T> {
        let layout = Layout::new::<T>();
        let header_layout = Layout::new::<GcHeader>();

        // Calculate total size with alignment
        let (combined_layout, offset) = header_layout
            .extend(layout)
            .expect("Layout calculation failed");

        // Allocate memory
        let ptr = unsafe { alloc(combined_layout) };
        if ptr.is_null() {
            panic!("Out of memory");
        }

        // Initialize header
        let header_ptr = ptr as *mut GcHeader;
        unsafe {
            header_ptr.write(GcHeader {
                marked: false,
                type_id: TypeId::of::<T>(),
                size: combined_layout.size(),
            });
        }

        // Initialize value
        let value_ptr = unsafe { ptr.add(offset) as *mut T };
        unsafe {
            value_ptr.write(value);
        }

        // Track allocation
        self.allocations.push(ptr);
        self.allocated_bytes += combined_layout.size();

        GcPtr::new(value_ptr)
    }
}
```

**Tests:**
- [ ] Test single allocation
- [ ] Test multiple allocations
- [ ] Test allocation tracking
- [ ] Test heap size limits
- [ ] Test OOM handling
- [ ] Test allocation alignment
- [ ] Test different value types
- [ ] Test large allocations (>1MB)

---

### Task 4: Type Registry & Pointer Maps

**File:** `crates/raya-core/src/gc/type_registry.rs`

**Checklist:**

- [ ] Define `TypeInfo` structure
  ```rust
  pub struct TypeInfo {
      type_id: TypeId,
      name: &'static str,
      size: usize,
      pointer_map: PointerMap,
      drop_fn: Option<DropFn>,
  }
  ```
- [ ] Define `PointerMap` for GC traversal
  ```rust
  pub enum PointerMap {
      None,                    // No pointers (primitives)
      All,                     // All pointers (arrays of objects)
      Offsets(Vec<usize>),     // Specific field offsets
  }
  ```
- [ ] Implement `TypeRegistry`
  - [ ] `register_type<T>(&mut self, pointer_map: PointerMap)`
  - [ ] `get_type_info(&self, type_id: TypeId) -> &TypeInfo`
  - [ ] `has_pointers(&self, type_id: TypeId) -> bool`
- [ ] Add pointer enumeration
  - [ ] `for_each_pointer<F>(&self, ptr: *mut u8, type_id: TypeId, f: F)`
  - [ ] Iterate over pointer fields based on map
  - [ ] Used by GC mark phase
- [ ] Register built-in types
  - [ ] String (no pointers in data)
  - [ ] Array (all elements may be pointers)
  - [ ] Object (pointer map from class definition)
  - [ ] Closure (captured values are pointers)
- [ ] Add drop function support
  - [ ] Call destructors if needed
  - [ ] Used during sweep phase

**Tests:**
- [ ] Test type registration
- [ ] Test pointer map creation
- [ ] Test pointer enumeration
- [ ] Test built-in type registration
- [ ] Test for_each_pointer callback

---

### Task 5: GC Root Tracking

**File:** `crates/raya-core/src/gc/roots.rs`

**Checklist:**

- [ ] Define `RootSet` structure
  ```rust
  pub struct RootSet {
      stack_roots: Vec<GcPtr<dyn Any>>,
      global_roots: Vec<GcPtr<dyn Any>>,
      task_roots: HashMap<TaskId, Vec<GcPtr<dyn Any>>>,
  }
  ```
- [ ] Implement root registration
  - [ ] `add_stack_root(&mut self, ptr: GcPtr<T>)`
  - [ ] `remove_stack_root(&mut self, ptr: GcPtr<T>)`
  - [ ] `add_global_root(&mut self, ptr: GcPtr<T>)`
  - [ ] `add_task_root(&mut self, task_id: TaskId, ptr: GcPtr<T>)`
- [ ] Implement root iteration
  - [ ] `for_each_root<F>(&self, f: F)` where F: FnMut(GcPtr<dyn Any>)
  - [ ] Iterate over all root sets
- [ ] Add automatic stack scanning
  - [ ] Scan stack frames for GC pointers
  - [ ] Extract pointers from `CallFrame` locals
  - [ ] Extract pointers from operand stack
- [ ] Implement task root collection
  - [ ] Scan all task stacks
  - [ ] Scan blocked tasks
  - [ ] Scan task result storage
- [ ] Add temporary root protection
  - [ ] `RootGuard<T>` RAII type
  - [ ] Automatically register/unregister on scope

**Stack Scanning Example:**

```rust
impl RootSet {
    pub fn scan_stack(&mut self, stack: &Stack) {
        // Scan operand stack
        for value in stack.operands() {
            if value.is_heap_allocated() {
                self.add_stack_root(value.as_gc_ptr());
            }
        }

        // Scan call frames
        for frame in stack.frames() {
            for local in frame.locals() {
                if local.is_heap_allocated() {
                    self.add_stack_root(local.as_gc_ptr());
                }
            }
        }
    }
}
```

**Tests:**
- [ ] Test root registration
- [ ] Test root removal
- [ ] Test root iteration
- [ ] Test stack scanning
- [ ] Test global roots
- [ ] Test task roots
- [ ] Test RootGuard RAII

---

### Task 6: Mark-Sweep Garbage Collector

**File:** `crates/raya-core/src/gc/collector.rs`

**Checklist:**

- [ ] Define `GarbageCollector` structure
  ```rust
  pub struct GarbageCollector {
      heap: Heap,
      roots: RootSet,
      type_registry: TypeRegistry,
      stats: GcStats,
  }
  ```
- [ ] Implement mark phase
  - [ ] `mark(&mut self)`
  - [ ] Clear all mark bits
  - [ ] Mark from roots
  - [ ] Recursively mark reachable objects
- [ ] Implement `mark_value()` recursive traversal
  ```rust
  fn mark_value(&mut self, ptr: GcPtr<dyn Any>) {
      if ptr.is_marked() {
          return; // Already marked
      }

      ptr.mark();

      // Mark referenced objects
      let type_info = self.type_registry.get(ptr.type_id());
      type_info.for_each_pointer(ptr.as_ptr(), |child_ptr| {
          self.mark_value(child_ptr);
      });
  }
  ```
- [ ] Implement sweep phase
  - [ ] `sweep(&mut self)`
  - [ ] Iterate through all allocations
  - [ ] Free unmarked objects
  - [ ] Keep marked objects, unmark them
- [ ] Add collection entry point
  - [ ] `collect(&mut self)`
  - [ ] Stop-the-world pause
  - [ ] Run mark phase
  - [ ] Run sweep phase
  - [ ] Update statistics
- [ ] Implement automatic collection triggers
  - [ ] Check threshold after allocation
  - [ ] `should_collect() -> bool`
  - [ ] Configurable threshold multiplier
- [ ] Add GC statistics
  ```rust
  pub struct GcStats {
      collections: usize,
      objects_freed: usize,
      bytes_freed: usize,
      total_pause_time: Duration,
  }
  ```
- [ ] Implement write barriers (placeholder for future)
  - [ ] Needed for generational GC
  - [ ] Track object mutations

**Tests:**
- [ ] Test simple mark-sweep cycle
- [ ] Test unreachable objects are collected
- [ ] Test reachable objects are preserved
- [ ] Test circular references
- [ ] Test deep object graphs
- [ ] Test root preservation
- [ ] Test statistics collection
- [ ] Test threshold triggering

---

### Task 7: Concurrent GC Safety

**File:** `crates/raya-core/src/gc/concurrent.rs`

**Checklist:**

- [ ] Implement stop-the-world mechanism
  - [ ] Pause all worker threads
  - [ ] Wait for all tasks to reach safe points
  - [ ] Safepoints at: function calls, back edges, allocations
- [ ] Add safepoint infrastructure
  - [ ] `enter_safepoint()` - check if GC pending
  - [ ] `GC_PENDING` atomic flag
  - [ ] Tasks poll flag at safepoints
- [ ] Implement thread synchronization
  - [ ] Use barrier to synchronize workers
  - [ ] `std::sync::Barrier` or `crossbeam::sync::WaitGroup`
  - [ ] Ensure all threads stopped before GC
- [ ] Add GC coordination
  ```rust
  pub struct GcCoordinator {
      gc_pending: AtomicBool,
      workers_stopped: AtomicUsize,
      barrier: Barrier,
  }
  ```
- [ ] Implement collection protocol
  - [ ] Signal GC needed
  - [ ] Wait for all workers to stop
  - [ ] Perform collection
  - [ ] Resume workers
- [ ] Add per-task GC state
  - [ ] Track if task is at safepoint
  - [ ] Track if task is blocked
  - [ ] Blocked tasks are automatically at safepoint
- [ ] Handle allocations during GC
  - [ ] Block allocations during GC
  - [ ] Queue allocation requests
  - [ ] Or fail-fast and retry

**Stop-the-World Protocol:**

```rust
impl GcCoordinator {
    pub fn request_gc(&self) {
        // 1. Set GC pending flag
        self.gc_pending.store(true, Ordering::SeqCst);

        // 2. Wait for all workers to reach barrier
        self.barrier.wait();

        // 3. All workers stopped, perform GC
        self.gc.collect();

        // 4. Clear flag and resume
        self.gc_pending.store(false, Ordering::SeqCst);
        self.barrier.wait();
    }

    pub fn safepoint_poll(&self) {
        if self.gc_pending.load(Ordering::Acquire) {
            self.barrier.wait(); // Stop at safepoint
            self.barrier.wait(); // Wait for GC completion
        }
    }
}
```

**Tests:**
- [ ] Test stop-the-world with single thread
- [ ] Test stop-the-world with multiple threads
- [ ] Test safepoint polling
- [ ] Test GC during concurrent task execution
- [ ] Test allocation blocking during GC
- [ ] Test no data races during GC

---

### Task 8: Integration with Value System

**File:** `crates/raya-core/src/value.rs`, `crates/raya-core/src/gc/mod.rs`

**Checklist:**

- [ ] Add GC integration to Value
  - [ ] `Value::allocate_string(gc: &mut Gc, s: &str) -> Value`
  - [ ] `Value::allocate_object(gc: &mut Gc, class: ClassId) -> Value`
  - [ ] `Value::allocate_array(gc: &mut Gc, len: usize) -> Value`
- [ ] Update heap allocation to use GC
  - [ ] Replace direct allocation with GC-tracked allocation
  - [ ] Trigger GC if threshold exceeded
- [ ] Add GC-safe value operations
  - [ ] Ensure all heap access goes through GC
  - [ ] Add root guards for temporary values
- [ ] Implement object types
  ```rust
  pub struct RayaString {
      len: usize,
      data: [u8], // Flexible array member
  }

  pub struct RayaArray {
      len: usize,
      capacity: usize,
      elements: [Value], // Flexible array
  }

  pub struct RayaObject {
      class_id: ClassId,
      fields: [Value], // Number of fields from class
  }
  ```
- [ ] Add string interning (optional optimization)
  - [ ] Cache immutable strings
  - [ ] Deduplicate string allocations

**Tests:**
- [ ] Test Value allocation through GC
- [ ] Test GC collection with Values
- [ ] Test string allocation
- [ ] Test array allocation
- [ ] Test object allocation
- [ ] Test value preservation across GC

---

### Task 9: Memory Utilities & Debugging

**File:** `crates/raya-core/src/gc/debug.rs`

**Checklist:**

- [ ] Implement heap dumper
  - [ ] `dump_heap(&self) -> String`
  - [ ] Show all allocations
  - [ ] Show marked/unmarked status
  - [ ] Show object types and sizes
- [ ] Add GC visualization
  - [ ] `print_object_graph(&self)`
  - [ ] Show root set
  - [ ] Show reachable objects
  - [ ] Show unreachable objects
- [ ] Implement leak detection
  - [ ] Track allocation sites
  - [ ] Report objects that should have been freed
  - [ ] Integration with tests
- [ ] Add GC profiling
  - [ ] Measure mark phase time
  - [ ] Measure sweep phase time
  - [ ] Track pause times
  - [ ] Generate reports
- [ ] Create GC stress testing utilities
  - [ ] Force GC after every allocation
  - [ ] Randomize GC timing
  - [ ] Validate heap after each GC
- [ ] Add heap validation
  - [ ] `validate_heap(&self) -> Result<(), String>`
  - [ ] Check all pointers are valid
  - [ ] Check all marked objects are reachable
  - [ ] Check no unmarked objects in use

**Tests:**
- [ ] Test heap dumper
- [ ] Test object graph visualization
- [ ] Test leak detection
- [ ] Test GC profiling
- [ ] Test stress testing utilities

---

### Task 10: Performance Optimization

**File:** `crates/raya-core/benches/gc_bench.rs`

**Checklist:**

- [ ] Create GC benchmarks
  - [ ] Allocation throughput
  - [ ] Collection pause time
  - [ ] Mark phase performance
  - [ ] Sweep phase performance
- [ ] Benchmark different heap sizes
  - [ ] Small heap (<1MB)
  - [ ] Medium heap (1-100MB)
  - [ ] Large heap (>100MB)
- [ ] Benchmark object graph complexity
  - [ ] Shallow graphs (few references)
  - [ ] Deep graphs (long chains)
  - [ ] Wide graphs (many references)
- [ ] Optimize hot paths
  - [ ] Fast allocation path
  - [ ] Inline small allocations
  - [ ] Cache type info lookups
- [ ] Add arena allocator for short-lived objects
  - [ ] Allocate in bump-pointer arena
  - [ ] Free entire arena at once
  - [ ] Reduce GC pressure
- [ ] Optimize mark phase
  - [ ] Use iterative marking (avoid recursion)
  - [ ] Use work queue for marking
  - [ ] Batch mark operations
- [ ] Profile and identify bottlenecks
  - [ ] Use `perf` or `cargo-flamegraph`
  - [ ] Identify allocation hotspots
  - [ ] Optimize based on data

**Tests:**
- [ ] Benchmark allocation performance
- [ ] Benchmark GC pause times
- [ ] Benchmark throughput
- [ ] Regression tests for performance

---

### Task 11: Documentation

**Files:** Various

**Checklist:**

- [ ] Add module-level documentation
  - [ ] `src/gc/mod.rs` - GC overview
  - [ ] `src/value.rs` - Value representation
- [ ] Document all public APIs
  - [ ] GarbageCollector
  - [ ] Heap
  - [ ] Value
  - [ ] GcPtr
- [ ] Add usage examples
  ```rust
  // Example: Allocating and collecting
  let mut gc = GarbageCollector::new();
  let s = Value::allocate_string(&mut gc, "hello");
  let arr = Value::allocate_array(&mut gc, 10);
  gc.collect(); // Run collection
  ```
- [ ] Document GC algorithm
  - [ ] Mark-sweep explanation
  - [ ] Root set definition
  - [ ] Pointer map usage
- [ ] Add safety guidelines
  - [ ] GcPtr lifetime rules
  - [ ] Root registration requirements
  - [ ] Concurrent GC considerations
- [ ] Create architecture diagrams
  - [ ] Value representation
  - [ ] Heap layout
  - [ ] GC phases
- [ ] Add performance tuning guide
  - [ ] Threshold configuration
  - [ ] Heap size limits
  - [ ] Arena allocator usage

---

### Task 12: Testing & Validation

**File:** `crates/raya-core/tests/gc_tests.rs`

**Checklist:**

- [ ] Write comprehensive test suite
  - [ ] Basic allocation and deallocation
  - [ ] Simple GC cycles
  - [ ] Circular references
  - [ ] Deep object graphs
  - [ ] Concurrent allocations
  - [ ] Stress tests
- [ ] Add property-based tests
  - [ ] Use `proptest` or `quickcheck`
  - [ ] Generate random object graphs
  - [ ] Verify GC correctness
- [ ] Create memory leak tests
  - [ ] Allocate objects
  - [ ] Drop references
  - [ ] Force GC
  - [ ] Verify all freed
- [ ] Add fuzzing tests
  - [ ] Fuzz allocation patterns
  - [ ] Fuzz object graph shapes
  - [ ] Use `cargo-fuzz`
- [ ] Test edge cases
  - [ ] Zero-sized allocations
  - [ ] Maximum heap size
  - [ ] Out of memory
  - [ ] Empty collections
- [ ] Validate GC correctness
  - [ ] No use-after-free
  - [ ] No double-free
  - [ ] No memory leaks
  - [ ] All roots preserved

---

## Implementation Details

### Value Representation Strategy

**Chosen: Tagged Pointers (64-bit)**

Rationale:
- Efficient on 64-bit systems
- Inline small integers without allocation
- Fast type checking (bit masking)
- Allows for future NaN-boxing optimization

**Layout:**
```
Value (64 bits):
- Pointer:  pppppppppppppppppppppppppppppppppppppppppppppppppppppppppp000
- i32:      000000000000000000000000000000iiiiiiiiiiiiiiiiiiiiiiiiiiii001
- bool:     000000000000000000000000000000000000000000000000000000000b010
- null:     0000000000000000000000000000000000000000000000000000000000110
```

### Heap Allocation Layout

```
┌─────────────────────────────────────────┐
│ GcHeader (16 bytes, aligned)            │
│  - marked: bool (1 byte)                │
│  - padding: [u8; 3]                     │
│  - type_id: TypeId (4 bytes)            │
│  - size: usize (8 bytes)                │
├─────────────────────────────────────────┤
│ Value Data (variable size)              │
│  - Actual object/array/string data      │
└─────────────────────────────────────────┘
```

### GC Algorithm: Mark-Sweep

**Mark Phase:**
1. Clear all mark bits
2. Start from root set (stacks, globals, tasks)
3. Recursively mark all reachable objects
4. Use pointer maps to find references

**Sweep Phase:**
1. Iterate through all allocations
2. Free unmarked objects (unreachable)
3. Unmark marked objects (prepare for next cycle)
4. Update heap statistics

**Complexity:**
- Mark: O(live objects)
- Sweep: O(total allocations)
- Total: O(heap size)

### Concurrent GC Strategy

**Stop-the-World Collection:**
1. Set GC_PENDING flag
2. All workers poll at safepoints
3. Workers reach barrier and stop
4. Single thread performs GC
5. Clear flag, workers resume

**Safepoints:**
- Function calls
- Loop back-edges
- Allocations
- Explicit polls

---

## Testing Requirements

### Unit Tests

**Minimum Coverage:** 90%

**Required Test Categories:**

1. **Value tests** (15+ tests)
   - Creation and type checking
   - Extraction and conversion
   - Equality comparison
   - Edge cases (null, bool, limits)

2. **Heap tests** (20+ tests)
   - Allocation and deallocation
   - Alignment verification
   - Size tracking
   - Limit enforcement

3. **GC tests** (25+ tests)
   - Mark phase correctness
   - Sweep phase correctness
   - Root preservation
   - Unreachable collection
   - Circular references
   - Concurrent safety

4. **Integration tests** (15+ tests)
   - End-to-end allocation and collection
   - Multi-threaded scenarios
   - Stress tests
   - Memory leak detection

### Performance Tests

**File:** `crates/raya-core/benches/gc_bench.rs`

- [ ] Allocation throughput benchmark
- [ ] GC pause time benchmark
- [ ] Memory overhead measurement
- [ ] Scalability tests (varying heap sizes)

### Stress Tests

- [ ] Continuous allocation and collection
- [ ] Random object graph generation
- [ ] Concurrent multi-threaded stress
- [ ] Memory pressure scenarios

---

## Success Criteria

### Must Have

- ✅ Value representation works for all types
- ✅ Heap allocation and tracking functional
- ✅ Mark-sweep GC correctly reclaims memory
- ✅ No memory leaks in tests
- ✅ No use-after-free errors
- ✅ Thread-safe GC in multi-threaded environment
- ✅ All tests pass
- ✅ Test coverage >90%
- ✅ Documentation complete
- ✅ No clippy warnings
- ✅ Valgrind clean (no memory errors)

### Nice to Have

- Arena allocator for temp objects
- GC profiling and statistics
- Heap visualization tools
- Write barriers for future generational GC
- Weak reference support
- Performance within 20% of reference implementation

### Exit Criteria

✅ **Ready to proceed to Milestone 1.4 when:**

1. All tasks marked as complete
2. `cargo test --package raya-core` passes
3. `cargo clippy --package raya-core` has no warnings
4. Memory tests pass under Valgrind/MIRI
5. Can allocate objects and collect garbage correctly
6. Concurrent GC works with simulated multi-threaded workload
7. Code reviewed and approved

---

## Dependencies

### Internal Dependencies

- ✅ Milestone 1.1 (Project Setup) - Complete
- ✅ Milestone 1.2 (Bytecode Definitions) - Complete

### External Dependencies

```toml
[dependencies]
parking_lot = { workspace = true }
crossbeam = { workspace = true }
rustc-hash = { workspace = true }

[dev-dependencies]
criterion = { workspace = true }
proptest = "1.0"
```

### Design Documents

- [design/ARCHITECTURE.md](../design/ARCHITECTURE.md) - VM architecture, Section 5 (Heap & Object Model)
- [design/LANG.md](../design/LANG.md) - Type system
- [design/OPCODE.md](../design/OPCODE.md) - Bytecode for allocation instructions

---

## References

### Related Files

- `crates/raya-core/src/value.rs`
- `crates/raya-core/src/gc/mod.rs`
- `crates/raya-core/src/gc/heap.rs`
- `crates/raya-core/src/gc/collector.rs`
- `crates/raya-core/src/gc/roots.rs`
- `crates/raya-core/src/gc/ptr.rs`
- `crates/raya-core/src/gc/type_registry.rs`

### External References

- [Crafting Interpreters - Garbage Collection](https://craftinginterpreters.com/garbage-collection.html)
- [The Garbage Collection Handbook](http://gchandbook.org/)
- [Rust's allocator API](https://doc.rust-lang.org/std/alloc/index.html)
- [V8's Mark-Sweep GC](https://v8.dev/blog/trash-talk)
- [Go's GC Design](https://go.dev/blog/ismmkeynote)

### Prior Art

- V8 JavaScript engine (generational GC)
- LuaJIT (simple mark-sweep)
- MicroPython (mark-sweep with GC pressure)
- Wren (mark-sweep with string interning)

---

## Progress Tracking

### Overall Progress: 0% Complete

- [ ] Task 1: Value Representation (0/30)
- [ ] Task 2: GC Pointer Type (0/15)
- [ ] Task 3: Heap Allocator (0/20)
- [ ] Task 4: Type Registry (0/15)
- [ ] Task 5: Root Tracking (0/18)
- [ ] Task 6: Mark-Sweep GC (0/20)
- [ ] Task 7: Concurrent Safety (0/15)
- [ ] Task 8: Value Integration (0/12)
- [ ] Task 9: Debug Utilities (0/12)
- [ ] Task 10: Optimization (0/15)
- [ ] Task 11: Documentation (0/10)
- [ ] Task 12: Testing (0/18)

**Total Checklist Items:** 200

**Estimated Time:** 3-4 weeks (120-160 hours)

---

## Notes

### Implementation Order

Recommended implementation order:

1. **Task 1** (Value Representation) - Foundation for all data
2. **Task 2** (GC Pointer) - Needed by heap allocator
3. **Task 3** (Heap Allocator) - Core allocation functionality
4. **Task 4** (Type Registry) - Needed by GC for traversal
5. **Task 5** (Root Tracking) - Needed by GC mark phase
6. **Task 6** (Mark-Sweep GC) - Core GC algorithm
7. **Task 8** (Value Integration) - Connect everything together
8. **Task 7** (Concurrent Safety) - Add thread safety
9. **Task 9** (Debug Utilities) - Helpful for debugging
10. **Task 10** (Optimization) - Performance tuning
11. **Task 11** (Documentation) - Throughout development
12. **Task 12** (Testing) - Continuous throughout

### Common Pitfalls

⚠️ **Watch out for:**

- **Pointer invalidation** - GC moves or frees memory
- **Missing roots** - Objects collected too early
- **Circular references** - Must handle correctly
- **Alignment issues** - GC headers must be properly aligned
- **Race conditions** - Concurrent allocation during GC
- **Memory leaks** - Objects never becoming unreachable
- **Use-after-free** - Accessing freed memory

### Tips

💡 **Pro tips:**

- Start with simple mark-sweep, optimize later
- Use Miri to detect undefined behavior
- Use Valgrind to detect memory leaks
- Write tests before implementation (TDD)
- Add extensive logging during development
- Use debug assertions liberally
- Test with GC stress mode (collect after every allocation)
- Verify heap invariants after each collection

### Design Decisions

**Why tagged pointers?**
- Fast type checking (single bit mask)
- No allocation for small integers
- Efficient on 64-bit systems
- Industry standard (V8, SpiderMonkey)

**Why mark-sweep over copying GC?**
- Simpler to implement initially
- No pointer fixup needed
- Good baseline for optimization
- Can add generational later

**Why stop-the-world?**
- Simpler concurrent correctness
- No read/write barriers
- Predictable pause times
- Good starting point

---

**Status:** Ready to Start
**Next Milestone:** 1.4 - Stack & Frame Management
**Version:** v1.0
**Last Updated:** 2026-01-05
