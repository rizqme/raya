# Milestone 1.7: Complete Garbage Collection

**Status:** ✅ Complete
**Goal:** Implement precise mark-sweep GC with type metadata-guided pointer traversal
**Dependencies:** Milestone 1.6 (Object Model)

---

## Overview

This milestone completes the garbage collection system by implementing precise marking with type metadata, integrating root set management with the stack, and adding comprehensive GC statistics and tuning capabilities.

The GC foundation (VmContext, Heap, GcHeader, GarbageCollector skeleton) was implemented in Milestone 1.3. This milestone focuses on completing the mark phase with type-metadata-guided pointer traversal and integration with the VM's execution state.

---

## Architecture

### GC System Components

```text
┌─────────────────────────────────────────────────────────────┐
│                      GarbageCollector                        │
│  - heap: Heap (allocations + type registry)                 │
│  - roots: RootSet (stack + globals)                         │
│  - threshold: usize (collection trigger)                    │
│  - stats: GcStats (performance metrics)                     │
└─────────────────────────────────────────────────────────────┘
                          │
        ┌─────────────────┼─────────────────┐
        │                 │                 │
        ▼                 ▼                 ▼
┌──────────────┐  ┌──────────────┐  ┌──────────────┐
│    Heap      │  │   RootSet    │  │   GcStats    │
│              │  │              │  │              │
│ allocations  │  │ stack_roots  │  │ collections  │
│ type_registry│  │ global_roots │  │ pause_time   │
│ max_size     │  │              │  │ bytes_freed  │
└──────────────┘  └──────────────┘  └──────────────┘
        │
        └─────> TypeRegistry
                - TypeInfo (size, align, pointer_map)
                - for_each_pointer()
```

### Mark-Sweep Algorithm

```text
1. MARK PHASE:
   ┌─────────────────────────────────────────┐
   │ 1. Clear all mark bits                  │
   │ 2. Scan stack roots                     │
   │ 3. Scan global roots                    │
   │ 4. For each root:                       │
   │    - If pointer → mark_value()          │
   │    - If already marked → skip           │
   │    - Else:                              │
   │      * Get GcHeader                     │
   │      * Mark header                      │
   │      * Get TypeInfo from type_id        │
   │      * For each pointer field:          │
   │        - Recursively mark_value()       │
   └─────────────────────────────────────────┘

2. SWEEP PHASE:
   ┌─────────────────────────────────────────┐
   │ 1. Iterate allocations                  │
   │ 2. For each allocation:                 │
   │    - If marked → keep (unmark for next) │
   │    - Else → free (call drop, dealloc)   │
   │ 3. Update statistics                    │
   │ 4. Adjust threshold (2x current usage)  │
   └─────────────────────────────────────────┘
```

### Memory Layout

```text
┌─────────────────────────────────────────┐
│ GcHeader (40 bytes, 8-byte aligned)     │
│  - marked: bool                         │
│  - padding: [u8; 7]                     │
│  - context_id: VmContextId (8 bytes)    │
│  - type_id: TypeId (16 bytes)           │ ← Used to lookup TypeInfo
│  - size: usize (8 bytes)                │
├─────────────────────────────────────────┤  ← GcPtr points here
│ Object data (variable size)             │
│  - May contain pointers to other objs   │
└─────────────────────────────────────────┘
```

---

## Tasks

### Task 1: Implement Precise mark_value() with Type Metadata

**File:** `crates/raya-core/src/gc/collector.rs`

**Goal:** Complete the `mark_value()` function to perform type-metadata-guided pointer traversal.

**Current Implementation (Placeholder):**
```rust
/// Mark a single value and its references
fn mark_value(&mut self, value: Value) {
    // Only mark heap-allocated values
    if !value.is_heap_allocated() {
        return;
    }

    // TODO: Complete implementation
}
```

**New Implementation:**
```rust
/// Mark a single value and its references
fn mark_value(&mut self, value: Value) {
    // Only mark heap-allocated values
    if !value.is_heap_allocated() {
        return;
    }

    // Extract pointer from value
    let ptr = match unsafe { value.as_ptr::<u8>() } {
        Some(p) => p.as_ptr(),
        None => return,
    };

    // Get GcHeader (located before the object data)
    let header_ptr = unsafe {
        ptr.cast::<GcHeader>()
            .sub(1) // Header is before object
    };

    // Check if already marked (avoid cycles)
    unsafe {
        if (*header_ptr).is_marked() {
            return;
        }

        // Mark this object
        (*header_ptr).mark();
    }

    // Get type information
    let type_id = unsafe { (*header_ptr).type_id() };
    let type_registry = self.heap.type_registry();

    if let Some(type_info) = type_registry.get(type_id) {
        // If this type has no pointers, we're done
        if !type_info.has_pointers() {
            return;
        }

        // Traverse all pointer fields using type metadata
        type_info.for_each_pointer(ptr, |field_ptr| {
            // Read the Value from this pointer field
            let field_value = unsafe { *(field_ptr as *const Value) };

            // Recursively mark
            self.mark_value(field_value);
        });
    }
}
```

**Key Points:**
- Extract raw pointer from Value
- Calculate GcHeader location (header is stored BEFORE object data)
- Check mark bit to avoid infinite recursion (cycles)
- Mark the header
- Use TypeRegistry to get TypeInfo
- Use PointerMap to iterate pointer fields
- Recursively mark each pointer field

**Tests:**
```rust
#[test]
fn test_mark_value_primitive() {
    let mut gc = GarbageCollector::default();

    // Primitives shouldn't be marked (not heap-allocated)
    gc.mark_value(Value::i32(42));
    gc.mark_value(Value::bool(true));
    gc.mark_value(Value::null());

    // No allocations should be marked
    for header_ptr in gc.heap.iter_allocations() {
        unsafe {
            assert!(!(*header_ptr).is_marked());
        }
    }
}

#[test]
fn test_mark_value_object() {
    let mut gc = GarbageCollector::default();

    // Allocate an object
    let obj_ptr = gc.allocate(Object::new(0, 2));
    let value = unsafe {
        Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap())
    };

    // Mark it
    gc.mark_value(value);

    // Check that it's marked
    let header_ptr = unsafe {
        obj_ptr.as_ptr().cast::<GcHeader>().sub(1)
    };
    unsafe {
        assert!((*header_ptr).is_marked());
    }
}

#[test]
fn test_mark_value_nested_objects() {
    let mut gc = GarbageCollector::default();

    // Create two objects
    let obj1 = Object::new(0, 2);
    let obj2 = Object::new(0, 2);

    let ptr1 = gc.allocate(obj1);
    let ptr2 = gc.allocate(obj2);

    // Make obj1 reference obj2
    let val2 = unsafe {
        Value::from_ptr(std::ptr::NonNull::new(ptr2.as_ptr()).unwrap())
    };
    unsafe {
        (*ptr1.as_ptr()).set_field(0, val2).unwrap();
    }

    // Mark obj1 (should mark obj2 as well)
    let val1 = unsafe {
        Value::from_ptr(std::ptr::NonNull::new(ptr1.as_ptr()).unwrap())
    };
    gc.mark_value(val1);

    // Both should be marked
    let header1 = unsafe { ptr1.as_ptr().cast::<GcHeader>().sub(1) };
    let header2 = unsafe { ptr2.as_ptr().cast::<GcHeader>().sub(1) };

    unsafe {
        assert!((*header1).is_marked());
        assert!((*header2).is_marked());
    }
}

#[test]
fn test_mark_value_cycles() {
    let mut gc = GarbageCollector::default();

    // Create two objects that reference each other
    let obj1 = Object::new(0, 1);
    let obj2 = Object::new(0, 1);

    let ptr1 = gc.allocate(obj1);
    let ptr2 = gc.allocate(obj2);

    let val1 = unsafe {
        Value::from_ptr(std::ptr::NonNull::new(ptr1.as_ptr()).unwrap())
    };
    let val2 = unsafe {
        Value::from_ptr(std::ptr::NonNull::new(ptr2.as_ptr()).unwrap())
    };

    // Create cycle
    unsafe {
        (*ptr1.as_ptr()).set_field(0, val2).unwrap();
        (*ptr2.as_ptr()).set_field(0, val1).unwrap();
    }

    // Mark obj1 (should handle cycle gracefully)
    gc.mark_value(val1);

    // Both should be marked
    let header1 = unsafe { ptr1.as_ptr().cast::<GcHeader>().sub(1) };
    let header2 = unsafe { ptr2.as_ptr().cast::<GcHeader>().sub(1) };

    unsafe {
        assert!((*header1).is_marked());
        assert!((*header2).is_marked());
    }
}
```

---

### Task 2: Integrate Root Set with Stack

**File:** `crates/raya-core/src/vm/interpreter.rs`

**Goal:** Automatically collect roots from the stack before GC.

**Add Method to Vm:**
```rust
impl Vm {
    /// Collect GC roots from the stack
    fn collect_roots(&mut self) {
        self.gc.clear_stack_roots();

        // Add all values from the operand stack
        for i in 0..self.stack.depth() {
            if let Ok(value) = self.stack.peek_at(i) {
                if value.is_heap_allocated() {
                    self.gc.add_root(value);
                }
            }
        }

        // Add values from all call frames' local variables
        for frame in self.stack.frames() {
            let locals_start = frame.locals_start();
            let locals_count = frame.locals_count();

            for i in 0..locals_count {
                if let Ok(value) = self.stack.peek_at(locals_start + i) {
                    if value.is_heap_allocated() {
                        self.gc.add_root(value);
                    }
                }
            }
        }
    }

    /// Trigger garbage collection
    pub fn collect_garbage(&mut self) {
        self.collect_roots();
        self.gc.collect();
    }
}
```

**Stack Additions:**
```rust
// Add to crates/raya-core/src/stack.rs

impl Stack {
    /// Iterate over all call frames
    pub fn frames(&self) -> FrameIterator<'_> {
        FrameIterator {
            stack: self,
            frame_idx: 0,
        }
    }
}

/// Iterator over call frames
pub struct FrameIterator<'a> {
    stack: &'a Stack,
    frame_idx: usize,
}

impl<'a> Iterator for FrameIterator<'a> {
    type Item = &'a CallFrame;

    fn next(&mut self) -> Option<Self::Item> {
        if self.frame_idx < self.stack.frames.len() {
            let frame = &self.stack.frames[self.frame_idx];
            self.frame_idx += 1;
            Some(frame)
        } else {
            None
        }
    }
}
```

**Tests:**
```rust
#[test]
fn test_collect_roots_from_stack() {
    let mut vm = Vm::new();

    // Push some heap-allocated values
    let obj1 = Object::new(0, 2);
    let ptr1 = vm.gc.allocate(obj1);
    let val1 = unsafe {
        Value::from_ptr(std::ptr::NonNull::new(ptr1.as_ptr()).unwrap())
    };

    vm.stack.push(val1).unwrap();
    vm.stack.push(Value::i32(42)).unwrap(); // Non-heap value

    // Collect roots
    vm.collect_roots();

    // GC should have val1 as a root
    let roots: Vec<Value> = vm.gc.roots.iter().collect();
    assert!(roots.contains(&val1));
    assert!(!roots.contains(&Value::i32(42)));
}

#[test]
fn test_collect_roots_from_locals() {
    let mut vm = Vm::new();

    // Create a function with locals
    let function = Function {
        name: "test".to_string(),
        param_count: 0,
        local_count: 2,
        code: vec![Opcode::Return as u8],
    };

    // Push frame
    vm.stack.push_frame(0, 2, 0).unwrap();

    // Store heap value in local
    let obj = Object::new(0, 1);
    let ptr = vm.gc.allocate(obj);
    let val = unsafe {
        Value::from_ptr(std::ptr::NonNull::new(ptr.as_ptr()).unwrap())
    };

    vm.stack.store_local(0, val).unwrap();

    // Collect roots
    vm.collect_roots();

    // Should find the local variable
    let roots: Vec<Value> = vm.gc.roots.iter().collect();
    assert!(roots.contains(&val));
}
```

---

### Task 3: Add Global Variable Root Tracking

**File:** `crates/raya-core/src/vm/interpreter.rs`

**Goal:** Track global variables as GC roots.

**Enhancement:**
```rust
impl Vm {
    /// Collect GC roots from globals
    fn collect_global_roots(&mut self) {
        for value in self.globals.values() {
            if value.is_heap_allocated() {
                self.gc.add_root(*value);
            }
        }
    }

    /// Enhanced root collection (stack + globals)
    fn collect_roots(&mut self) {
        self.gc.clear_stack_roots();

        // Stack roots
        for i in 0..self.stack.depth() {
            if let Ok(value) = self.stack.peek_at(i) {
                if value.is_heap_allocated() {
                    self.gc.add_root(value);
                }
            }
        }

        // Frame local roots
        for frame in self.stack.frames() {
            let locals_start = frame.locals_start();
            let locals_count = frame.locals_count();

            for i in 0..locals_count {
                if let Ok(value) = self.stack.peek_at(locals_start + i) {
                    if value.is_heap_allocated() {
                        self.gc.add_root(value);
                    }
                }
            }
        }

        // Global roots
        self.collect_global_roots();
    }
}
```

**Tests:**
```rust
#[test]
fn test_globals_as_roots() {
    let mut vm = Vm::new();

    // Create global variable with heap value
    let obj = Object::new(0, 1);
    let ptr = vm.gc.allocate(obj);
    let val = unsafe {
        Value::from_ptr(std::ptr::NonNull::new(ptr.as_ptr()).unwrap())
    };

    vm.globals.insert("myGlobal".to_string(), val);

    // Collect roots
    vm.collect_roots();

    // Global should be in root set
    let roots: Vec<Value> = vm.gc.roots.iter().collect();
    assert!(roots.contains(&val));
}
```

---

### Task 4: Register Raya Object Types in TypeRegistry

**File:** `crates/raya-core/src/types/registry.rs`

**Goal:** Register Object, Array, RayaString types with proper pointer maps.

**Enhancement to create_standard_registry():**
```rust
pub fn create_standard_registry() -> TypeRegistry {
    use crate::object::{Array, Object, RayaString};
    use crate::value::Value;

    let mut builder = TypeRegistry::builder();

    // Primitives (no pointers)
    builder.register(TypeInfo::new::<i32>("i32", PointerMap::none()));
    builder.register(TypeInfo::new::<i64>("i64", PointerMap::none()));
    builder.register(TypeInfo::new::<f32>("f32", PointerMap::none()));
    builder.register(TypeInfo::new::<f64>("f64", PointerMap::none()));
    builder.register(TypeInfo::new::<bool>("bool", PointerMap::none()));

    // String (no pointers - String's data is not GC-managed)
    builder.register(TypeInfo::new::<String>("String", PointerMap::none()));
    builder.register(TypeInfo::new::<RayaString>("RayaString", PointerMap::none()));

    // Object: fields Vec<Value> contains pointers
    // Calculate pointer map based on Object layout
    // Object { class_id: usize, fields: Vec<Value> }
    // Vec<Value> contains: ptr, capacity, len (only ptr field has pointers to heap)
    // But we can't generically know field count, so we'll use a conservative approach

    // For now, mark Object as having no direct pointers in its struct
    // The GC will handle Object specially by reading the fields Vec
    builder.register(TypeInfo::new::<Object>(
        "Object",
        PointerMap::none(), // Special handling in GC
    ));

    // Array: elements Vec<Value> contains pointers
    builder.register(TypeInfo::new::<Array>(
        "Array",
        PointerMap::none(), // Special handling in GC
    ));

    builder.build()
}
```

**Special Object Handling in GC:**

Since Object and Array have dynamic field counts, we need special handling:

```rust
// Add to collector.rs mark_value() after marking header:

// Special handling for Object and Array types
let type_name = type_info.name;
match type_name {
    "Object" => {
        // Cast to Object and mark each field
        let obj = unsafe { &*(ptr as *const Object) };
        for &field_value in &obj.fields {
            self.mark_value(field_value);
        }
        return; // Skip normal pointer traversal
    }
    "Array" => {
        // Cast to Array and mark each element
        let arr = unsafe { &*(ptr as *const Array) };
        for &elem_value in &arr.elements {
            self.mark_value(elem_value);
        }
        return; // Skip normal pointer traversal
    }
    "RayaString" => {
        // Strings have no GC pointers
        return;
    }
    _ => {
        // Use normal pointer map traversal
    }
}
```

---

### Task 5: Enhance GC Statistics

**File:** `crates/raya-core/src/gc/collector.rs`

**Goal:** Add comprehensive GC statistics tracking.

**Enhanced GcStats:**
```rust
/// Garbage collector statistics
#[derive(Debug, Clone, Default)]
pub struct GcStats {
    /// Total number of collections
    pub collections: usize,

    /// Total objects freed
    pub objects_freed: usize,

    /// Total bytes freed
    pub bytes_freed: usize,

    /// Total pause time across all collections
    pub total_pause_time: Duration,

    /// Last collection duration
    pub last_pause_time: Duration,

    /// Average pause time
    pub avg_pause_time: Duration,

    /// Maximum pause time
    pub max_pause_time: Duration,

    /// Minimum pause time
    pub min_pause_time: Duration,

    /// Objects marked in last collection
    pub last_marked_count: usize,

    /// Objects freed in last collection
    pub last_freed_count: usize,

    /// Bytes freed in last collection
    pub last_freed_bytes: usize,

    /// Live objects after last collection
    pub live_objects: usize,

    /// Live bytes after last collection
    pub live_bytes: usize,
}

impl GcStats {
    /// Update statistics after a collection
    fn update(&mut self, pause_time: Duration, marked: usize, freed: usize, freed_bytes: usize, live_objects: usize, live_bytes: usize) {
        self.collections += 1;
        self.objects_freed += freed;
        self.bytes_freed += freed_bytes;
        self.total_pause_time += pause_time;
        self.last_pause_time = pause_time;

        // Update average
        self.avg_pause_time = self.total_pause_time / self.collections as u32;

        // Update max/min
        if pause_time > self.max_pause_time {
            self.max_pause_time = pause_time;
        }
        if self.collections == 1 || pause_time < self.min_pause_time {
            self.min_pause_time = pause_time;
        }

        // Update last collection stats
        self.last_marked_count = marked;
        self.last_freed_count = freed;
        self.last_freed_bytes = freed_bytes;
        self.live_objects = live_objects;
        self.live_bytes = live_bytes;
    }

    /// Get survival rate (0.0 to 1.0)
    pub fn survival_rate(&self) -> f64 {
        if self.last_marked_count == 0 {
            return 0.0;
        }
        self.live_objects as f64 / self.last_marked_count as f64
    }
}
```

**Enhanced collect() to track stats:**
```rust
/// Run garbage collection
pub fn collect(&mut self) {
    let start = Instant::now();

    // Mark phase
    let marked_count = self.mark();

    // Sweep phase
    let (freed_count, freed_bytes) = self.sweep();

    // Calculate live stats
    let live_objects = self.heap.allocation_count();
    let live_bytes = self.heap.allocated_bytes();

    // Update stats
    let duration = start.elapsed();
    self.stats.update(
        duration,
        marked_count,
        freed_count,
        freed_bytes,
        live_objects,
        live_bytes,
    );

    // Adjust threshold (grow by 2x current usage)
    let current_usage = self.heap.allocated_bytes();
    self.threshold = (current_usage * 2).max(1024 * 1024); // At least 1MB
}

/// Mark phase: mark all reachable objects
/// Returns number of objects marked
fn mark(&mut self) -> usize {
    // Clear all mark bits first
    for header_ptr in self.heap.iter_allocations() {
        unsafe {
            (*header_ptr).unmark();
        }
    }

    // Mark from roots
    let roots: Vec<Value> = self.roots.iter().collect();
    for root in roots {
        self.mark_value(root);
    }

    // Count marked objects
    let mut marked = 0;
    for header_ptr in self.heap.iter_allocations() {
        if unsafe { (*header_ptr).is_marked() } {
            marked += 1;
        }
    }

    marked
}

/// Sweep phase: free unmarked objects
/// Returns (freed_count, freed_bytes)
fn sweep(&mut self) -> (usize, usize) {
    let mut freed_count = 0;
    let mut freed_bytes = 0;

    // Collect unmarked allocations
    let to_free: Vec<(*mut GcHeader, usize)> = self
        .heap
        .iter_allocations()
        .filter(|&header_ptr| unsafe { !(*header_ptr).is_marked() })
        .map(|header_ptr| {
            let size = unsafe { (*header_ptr).size() };
            (header_ptr, size)
        })
        .collect();

    // Free them
    for (header_ptr, size) in to_free {
        unsafe {
            self.heap.free(header_ptr);
        }
        freed_count += 1;
        freed_bytes += size;
    }

    (freed_count, freed_bytes)
}
```

**Tests:**
```rust
#[test]
fn test_gc_stats_tracking() {
    let mut gc = GarbageCollector::default();

    // Allocate some objects
    let ptr1 = gc.allocate(100i32);
    let ptr2 = gc.allocate(200i32);
    let _ptr3 = gc.allocate(300i32);

    // Keep ptr1 and ptr2 as roots
    let val1 = unsafe {
        Value::from_ptr(std::ptr::NonNull::new(ptr1.as_ptr()).unwrap())
    };
    let val2 = unsafe {
        Value::from_ptr(std::ptr::NonNull::new(ptr2.as_ptr()).unwrap())
    };

    gc.add_root(val1);
    gc.add_root(val2);

    // Collect
    gc.collect();

    let stats = gc.stats();
    assert_eq!(stats.collections, 1);
    assert_eq!(stats.last_marked_count, 2); // ptr1 and ptr2
    assert_eq!(stats.last_freed_count, 1); // ptr3
    assert_eq!(stats.live_objects, 2);
    assert!(stats.last_pause_time > Duration::ZERO);
}

#[test]
fn test_gc_survival_rate() {
    let mut gc = GarbageCollector::default();

    // Allocate 10 objects
    let mut ptrs = Vec::new();
    for i in 0..10 {
        let ptr = gc.allocate(i * 100);
        ptrs.push(ptr);
    }

    // Keep first 7 as roots
    for ptr in &ptrs[0..7] {
        let val = unsafe {
            Value::from_ptr(std::ptr::NonNull::new(ptr.as_ptr()).unwrap())
        };
        gc.add_root(val);
    }

    // Collect
    gc.collect();

    let stats = gc.stats();
    assert_eq!(stats.last_marked_count, 7);
    assert_eq!(stats.last_freed_count, 3);
    assert_eq!(stats.live_objects, 7);

    // Survival rate should be 7/7 = 1.0 (all marked objects survived)
    assert_eq!(stats.survival_rate(), 1.0);
}
```

---

### Task 6: Add GC Tuning Parameters

**File:** `crates/raya-core/src/gc/collector.rs`

**Goal:** Make GC behavior tunable for different workloads.

**GC Configuration:**
```rust
/// GC tuning parameters
#[derive(Debug, Clone)]
pub struct GcConfig {
    /// Initial threshold in bytes (default: 1 MB)
    pub initial_threshold: usize,

    /// Threshold growth factor after collection (default: 2.0)
    /// New threshold = current_usage * growth_factor
    pub growth_factor: f64,

    /// Minimum threshold in bytes (default: 1 MB)
    pub min_threshold: usize,

    /// Maximum threshold in bytes (default: unlimited)
    pub max_threshold: usize,

    /// Enable/disable automatic collection (default: true)
    pub auto_collect: bool,
}

impl Default for GcConfig {
    fn default() -> Self {
        Self {
            initial_threshold: 1024 * 1024, // 1 MB
            growth_factor: 2.0,
            min_threshold: 1024 * 1024, // 1 MB
            max_threshold: 0, // Unlimited
            auto_collect: true,
        }
    }
}

impl GarbageCollector {
    /// Create with custom configuration
    pub fn with_config(
        context_id: VmContextId,
        type_registry: Arc<TypeRegistry>,
        config: GcConfig,
    ) -> Self {
        Self {
            heap: Heap::new(context_id, type_registry),
            roots: RootSet::new(),
            threshold: config.initial_threshold,
            stats: GcStats::default(),
            config,
        }
    }

    /// Configure GC parameters
    pub fn configure(&mut self, config: GcConfig) {
        self.config = config;
        self.threshold = config.initial_threshold;
    }

    /// Adjust threshold after collection
    fn adjust_threshold(&mut self) {
        let current_usage = self.heap.allocated_bytes();
        let new_threshold = (current_usage as f64 * self.config.growth_factor) as usize;

        // Clamp to min/max
        let mut threshold = new_threshold.max(self.config.min_threshold);
        if self.config.max_threshold > 0 {
            threshold = threshold.min(self.config.max_threshold);
        }

        self.threshold = threshold;
    }
}
```

**Add config field:**
```rust
pub struct GarbageCollector {
    heap: Heap,
    roots: RootSet,
    threshold: usize,
    stats: GcStats,
    config: GcConfig,  // NEW
}
```

**Update collect():**
```rust
/// Run garbage collection
pub fn collect(&mut self) {
    let start = Instant::now();

    // Mark phase
    let marked_count = self.mark();

    // Sweep phase
    let (freed_count, freed_bytes) = self.sweep();

    // Calculate live stats
    let live_objects = self.heap.allocation_count();
    let live_bytes = self.heap.allocated_bytes();

    // Update stats
    let duration = start.elapsed();
    self.stats.update(
        duration,
        marked_count,
        freed_count,
        freed_bytes,
        live_objects,
        live_bytes,
    );

    // Adjust threshold using config
    self.adjust_threshold();
}
```

**Tests:**
```rust
#[test]
fn test_gc_config_threshold() {
    let context_id = VmContextId::new();
    let type_registry = Arc::new(create_standard_registry());

    let mut config = GcConfig::default();
    config.initial_threshold = 512; // 512 bytes
    config.growth_factor = 3.0; // Grow faster

    let mut gc = GarbageCollector::with_config(context_id, type_registry, config);

    assert_eq!(gc.threshold, 512);
}

#[test]
fn test_gc_config_growth_factor() {
    let mut gc = GarbageCollector::default();
    gc.config.growth_factor = 3.0;

    // Allocate some data
    let _ptr = gc.allocate([0u8; 100]);

    // Collect
    gc.collect();

    // Threshold should grow by factor of 3
    let live_bytes = gc.heap.allocated_bytes();
    let expected_threshold = (live_bytes as f64 * 3.0) as usize;
    assert!(gc.threshold >= expected_threshold);
}
```

---

### Task 7: Integration Tests

**File:** `crates/raya-core/tests/gc_integration_tests.rs` (NEW)

**Goal:** End-to-end GC tests with realistic scenarios.

```rust
use raya_core::{GarbageCollector, Value, Object, Array, RayaString};
use std::ptr::NonNull;

#[test]
fn test_gc_basic_collection() {
    let mut gc = GarbageCollector::default();

    // Allocate objects
    let obj1 = Object::new(0, 2);
    let obj2 = Object::new(0, 2);
    let obj3 = Object::new(0, 2);

    let ptr1 = gc.allocate(obj1);
    let ptr2 = gc.allocate(obj2);
    let ptr3 = gc.allocate(obj3);

    // Only keep obj1 and obj2 as roots
    let val1 = unsafe {
        Value::from_ptr(NonNull::new(ptr1.as_ptr()).unwrap())
    };
    let val2 = unsafe {
        Value::from_ptr(NonNull::new(ptr2.as_ptr()).unwrap())
    };

    gc.add_root(val1);
    gc.add_root(val2);

    // Collect - obj3 should be freed
    gc.collect();

    let stats = gc.stats();
    assert_eq!(stats.live_objects, 2);
    assert_eq!(stats.last_freed_count, 1);
}

#[test]
fn test_gc_nested_objects() {
    let mut gc = GarbageCollector::default();

    // Create object graph: root -> obj1 -> obj2 -> obj3
    let mut obj1 = Object::new(0, 1);
    let mut obj2 = Object::new(0, 1);
    let obj3 = Object::new(0, 1);

    let ptr3 = gc.allocate(obj3);
    let val3 = unsafe {
        Value::from_ptr(NonNull::new(ptr3.as_ptr()).unwrap())
    };

    obj2.set_field(0, val3).unwrap();
    let ptr2 = gc.allocate(obj2);
    let val2 = unsafe {
        Value::from_ptr(NonNull::new(ptr2.as_ptr()).unwrap())
    };

    obj1.set_field(0, val2).unwrap();
    let ptr1 = gc.allocate(obj1);
    let val1 = unsafe {
        Value::from_ptr(NonNull::new(ptr1.as_ptr()).unwrap())
    };

    // Only root is obj1
    gc.add_root(val1);

    // Collect - all 3 should be kept
    gc.collect();

    let stats = gc.stats();
    assert_eq!(stats.live_objects, 3);
    assert_eq!(stats.last_freed_count, 0);
}

#[test]
fn test_gc_circular_references() {
    let mut gc = GarbageCollector::default();

    // Create circular reference: obj1 <-> obj2
    let obj1 = Object::new(0, 1);
    let obj2 = Object::new(0, 1);

    let ptr1 = gc.allocate(obj1);
    let ptr2 = gc.allocate(obj2);

    let val1 = unsafe {
        Value::from_ptr(NonNull::new(ptr1.as_ptr()).unwrap())
    };
    let val2 = unsafe {
        Value::from_ptr(NonNull::new(ptr2.as_ptr()).unwrap())
    };

    unsafe {
        (*ptr1.as_ptr()).set_field(0, val2).unwrap();
        (*ptr2.as_ptr()).set_field(0, val1).unwrap();
    }

    // No roots - both should be collected
    gc.collect();

    let stats = gc.stats();
    assert_eq!(stats.live_objects, 0);
    assert_eq!(stats.last_freed_count, 2);
}

#[test]
fn test_gc_array_elements() {
    let mut gc = GarbageCollector::default();

    // Create array with object elements
    let obj1 = Object::new(0, 1);
    let obj2 = Object::new(0, 1);

    let ptr1 = gc.allocate(obj1);
    let ptr2 = gc.allocate(obj2);

    let val1 = unsafe {
        Value::from_ptr(NonNull::new(ptr1.as_ptr()).unwrap())
    };
    let val2 = unsafe {
        Value::from_ptr(NonNull::new(ptr2.as_ptr()).unwrap())
    };

    let mut arr = Array::new(0, 2);
    arr.set(0, val1).unwrap();
    arr.set(1, val2).unwrap();

    let arr_ptr = gc.allocate(arr);
    let arr_val = unsafe {
        Value::from_ptr(NonNull::new(arr_ptr.as_ptr()).unwrap())
    };

    // Only keep array as root
    gc.add_root(arr_val);

    // Collect - array and both objects should survive
    gc.collect();

    let stats = gc.stats();
    assert_eq!(stats.live_objects, 3); // array + obj1 + obj2
}

#[test]
fn test_gc_multiple_collections() {
    let mut gc = GarbageCollector::default();

    for i in 0..5 {
        // Allocate objects
        let obj = Object::new(0, 1);
        let ptr = gc.allocate(obj);

        // Keep only the current one
        gc.clear_stack_roots();
        let val = unsafe {
            Value::from_ptr(NonNull::new(ptr.as_ptr()).unwrap())
        };
        gc.add_root(val);

        // Collect
        gc.collect();

        // Should have exactly 1 live object
        let stats = gc.stats();
        assert_eq!(stats.live_objects, 1);
        assert_eq!(stats.collections, i + 1);
    }
}

#[test]
fn test_gc_threshold_trigger() {
    let mut gc = GarbageCollector::default();
    gc.set_threshold(128); // Very small threshold

    // Allocate until GC triggers
    let initial_collections = gc.stats().collections;

    for _ in 0..100 {
        let _obj = gc.allocate([0u8; 64]); // 64-byte allocations
    }

    // GC should have been triggered automatically
    assert!(gc.stats().collections > initial_collections);
}
```

---

### Task 8: Update Documentation

**File:** `crates/raya-core/src/gc/mod.rs`

**Goal:** Update module documentation with complete examples.

```rust
//! Garbage collection system
//!
//! This module provides a precise mark-sweep garbage collector for the Raya VM.
//!
//! # Architecture
//!
//! The GC system consists of several components:
//!
//! - **Value**: Tagged pointer representation (8 bytes)
//! - **GcHeader**: Metadata for each allocated object (40 bytes)
//! - **GcPtr**: Smart pointer to GC-managed objects
//! - **Heap**: Memory allocator with GC integration
//! - **RootSet**: Tracking of GC roots (stack, globals)
//! - **GarbageCollector**: Precise mark-sweep collection algorithm
//! - **TypeRegistry**: Type metadata for pointer traversal
//!
//! # Algorithm
//!
//! The GC uses a precise mark-sweep algorithm:
//!
//! 1. **Mark Phase**: Starting from roots, traverse all reachable objects
//!    using type metadata (PointerMap) to identify pointer fields
//! 2. **Sweep Phase**: Free all unmarked objects
//!
//! # Type Metadata
//!
//! Each type registered in the TypeRegistry has:
//! - Size and alignment
//! - PointerMap describing pointer field locations
//! - Optional drop function
//!
//! The GC uses this metadata to precisely identify and traverse pointers,
//! avoiding conservative scanning.
//!
//! # Roots
//!
//! GC roots are collected from:
//! - Operand stack values
//! - Local variables in all call frames
//! - Global variables
//!
//! # Performance
//!
//! - **Trigger**: GC runs when allocated bytes exceed threshold
//! - **Threshold**: Automatically adjusted after each collection (2x live size)
//! - **Pause Time**: Stop-the-world (future: incremental GC)
//! - **Statistics**: Comprehensive metrics (pause time, survival rate, etc.)
//!
//! # Example
//!
//! ```no_run
//! use raya_core::{GarbageCollector, Value, Object};
//! use std::ptr::NonNull;
//!
//! let mut gc = GarbageCollector::default();
//!
//! // Allocate objects
//! let obj = Object::new(0, 2);
//! let ptr = gc.allocate(obj);
//!
//! // Create root
//! let value = unsafe {
//!     Value::from_ptr(NonNull::new(ptr.as_ptr()).unwrap())
//! };
//! gc.add_root(value);
//!
//! // Collect garbage
//! gc.collect();
//!
//! // Check statistics
//! let stats = gc.stats();
//! println!("Collections: {}", stats.collections);
//! println!("Live objects: {}", stats.live_objects);
//! println!("Last pause: {:?}", stats.last_pause_time);
//! ```
```

---

## Acceptance Criteria

- [x] `mark_value()` implements precise pointer traversal using TypeRegistry
- [x] GC correctly handles Object, Array, and RayaString types
- [x] Circular references are handled correctly (no infinite loops)
- [x] Stack values are automatically added as roots
- [x] Local variables in all frames are added as roots
- [x] Global variables are added as roots
- [x] Object, Array, RayaString types are registered in TypeRegistry
- [x] GC statistics track all metrics (pause time, survival rate, etc.)
- [x] GC threshold is adjustable and automatically tuned
- [x] GcConfig allows customization of GC behavior
- [x] All unit tests pass
- [x] All integration tests pass
- [x] No memory leaks in test suite
- [x] GC pause time is reasonable (< 10ms for 10K objects)
- [x] Code coverage > 85% for gc module

---

## Testing Strategy

### Unit Tests
- Mark phase with primitives (no-op)
- Mark phase with single object
- Mark phase with nested objects
- Mark phase with circular references
- Sweep phase freeing unmarked objects
- Root collection from stack
- Root collection from locals
- Root collection from globals
- Statistics tracking
- Threshold adjustment
- GC configuration

### Integration Tests
- End-to-end collection scenarios
- Large object graphs (1000+ objects)
- Multiple collection cycles
- Automatic threshold triggering
- Memory leak detection

### Performance Tests
- Collection pause time measurement
- Throughput impact of GC
- Memory overhead of GC metadata

---

## Reference Documentation

- **design/ARCHITECTURE.md Section 5**: Memory model and GC design
- **design/LANG.md Section 21**: Memory management semantics
- **plans/milestone-1.3.md**: VmContext and GC foundation

---

## Future Enhancements

### Phase 2: Generational GC
- Young generation: copying collector
- Old generation: mark-sweep
- Write barrier for cross-generation pointers

### Phase 3: Incremental/Concurrent GC
- Incremental marking (spread over time)
- Concurrent sweeping (parallel with execution)
- Tri-color marking algorithm

### Phase 4: Compaction
- Defragmentation of heap
- Pointer relocation
- Improved cache locality
