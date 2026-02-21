//! Mark-sweep garbage collector
//!
//! This module implements a simple mark-sweep garbage collector.

use super::header::GcHeader;
use super::heap::Heap;
use super::ptr::GcPtr;
use super::roots::RootSet;
use crate::vm::types::TypeRegistry;
use crate::vm::value::Value;
use crate::vm::interpreter::VmContextId;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Garbage collector statistics
#[derive(Debug, Clone)]
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

impl Default for GcStats {
    fn default() -> Self {
        Self {
            collections: 0,
            objects_freed: 0,
            bytes_freed: 0,
            total_pause_time: Duration::ZERO,
            last_pause_time: Duration::ZERO,
            avg_pause_time: Duration::ZERO,
            max_pause_time: Duration::ZERO,
            min_pause_time: Duration::ZERO,
            last_marked_count: 0,
            last_freed_count: 0,
            last_freed_bytes: 0,
            live_objects: 0,
            live_bytes: 0,
        }
    }
}

impl GcStats {
    /// Update statistics after a collection
    fn update(
        &mut self,
        pause_time: Duration,
        marked: usize,
        freed: usize,
        freed_bytes: usize,
        live_objects: usize,
        live_bytes: usize,
    ) {
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

/// Mark-sweep garbage collector
pub struct GarbageCollector {
    /// Heap allocator
    heap: Heap,

    /// Root set
    roots: RootSet,

    /// GC threshold (bytes)
    threshold: usize,

    /// Statistics
    stats: GcStats,
}

impl GarbageCollector {
    /// Create a new garbage collector for a specific context
    pub fn new(context_id: VmContextId, type_registry: Arc<TypeRegistry>) -> Self {
        Self {
            heap: Heap::new(context_id, type_registry),
            roots: RootSet::new(),
            threshold: crate::vm::defaults::DEFAULT_GC_THRESHOLD,
            stats: GcStats::default(),
        }
    }

    /// Set GC threshold
    pub fn set_threshold(&mut self, bytes: usize) {
        self.threshold = bytes;
    }

    /// Set maximum heap size
    pub fn set_max_heap_size(&mut self, bytes: usize) {
        self.heap.set_max_heap_size(bytes);
    }

    /// Allocate a value
    pub fn allocate<T: 'static>(&mut self, value: T) -> GcPtr<T> {
        // Check if we should collect
        if self.should_collect() {
            self.collect();
        }

        self.heap.allocate(value)
    }

    /// Allocate an array
    pub fn allocate_array<T>(&mut self, len: usize) -> GcPtr<[T]>
    where
        T: 'static + Default + Clone,
    {
        // Check if we should collect
        if self.should_collect() {
            self.collect();
        }

        self.heap.allocate_array(len)
    }

    /// Add a root
    pub fn add_root(&mut self, value: Value) {
        self.roots.add_stack_root(value);
    }

    /// Clear stack roots (called between VM instructions)
    pub fn clear_stack_roots(&mut self) {
        self.roots.clear_stack_roots();
    }

    /// Check if we should collect
    fn should_collect(&self) -> bool {
        self.heap.allocated_bytes() > self.threshold
    }

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
        self.threshold = (current_usage * 2).max(crate::vm::defaults::DEFAULT_GC_THRESHOLD);
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

        // Mark from roots (collect first to avoid borrow checker issues)
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

    /// Mark a single value and its references
    fn mark_value(&mut self, value: Value) {
        use crate::vm::object::{Array, Object};

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
        // The GcHeader is stored immediately before the object in memory
        let header_ptr = unsafe { ptr.cast::<GcHeader>().sub(1) };

        // Check if already marked (avoid cycles and redundant work)
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
            // Special handling for Object and Array types (dynamic field counts)
            let type_name = type_info.name;
            match type_name {
                "Object" => {
                    // Cast to Object and mark each field
                    let obj = unsafe { &*(ptr as *const Object) };
                    for &field_value in &obj.fields {
                        self.mark_value(field_value);
                    }
                    return;
                }
                "Array" => {
                    // Cast to Array and mark each element
                    let arr = unsafe { &*(ptr as *const Array) };
                    for &elem_value in &arr.elements {
                        self.mark_value(elem_value);
                    }
                    return;
                }
                "RayaString" => {
                    // Strings have no GC pointers
                    return;
                }
                "BoundMethod" => {
                    // Trace the receiver (it's a GC-allocated object)
                    let bm = unsafe { &*(ptr as *const crate::vm::object::BoundMethod) };
                    self.mark_value(bm.receiver);
                    return;
                }
                "JsonValue" => {
                    // Cast to JsonValue and mark recursively
                    let json = unsafe { &*(ptr as *const crate::vm::json::JsonValue) };
                    self.mark_json_value(json);
                    return;
                }
                _ => {
                    // Use normal pointer map traversal for other types
                }
            }

            // If this type has no pointers, we're done
            if !type_info.has_pointers() {
                return;
            }

            // Traverse all pointer fields using type metadata
            // Collect field values first to avoid borrow checker issues
            let mut field_values = Vec::new();
            type_info.for_each_pointer(ptr, |field_ptr| {
                // Read the Value from this pointer field
                let field_value = unsafe { *(field_ptr as *const Value) };
                field_values.push(field_value);
            });

            // Now recursively mark each field
            for field_value in field_values {
                self.mark_value(field_value);
            }
        }
    }

    /// Mark a JsonValue and all its nested values
    #[allow(clippy::only_used_in_recursion)]
    fn mark_json_value(&mut self, json: &crate::vm::json::JsonValue) {
        use crate::vm::json::JsonValue;

        match json {
            JsonValue::String(s_ptr) => {
                // Mark the string GC object
                let str_ptr = s_ptr.as_ptr();
                let header_ptr = unsafe { (str_ptr as *mut crate::gc::header::GcHeader).sub(1) };

                // Mark if not already marked
                unsafe {
                    if !(*header_ptr).is_marked() {
                        (*header_ptr).mark();
                    }
                }
            }
            JsonValue::Array(arr_ptr) => {
                // Mark the array Vec GC object
                let vec_ptr = arr_ptr.as_ptr();
                let header_ptr = unsafe { (vec_ptr as *mut crate::gc::header::GcHeader).sub(1) };

                // Mark the Vec itself
                unsafe {
                    if (*header_ptr).is_marked() {
                        return; // Already marked, avoid infinite recursion
                    }
                    (*header_ptr).mark();
                }

                // Mark all elements
                let arr = unsafe { &**vec_ptr };
                for elem in arr {
                    self.mark_json_value(elem);
                }
            }
            JsonValue::Object(obj_ptr) => {
                // Mark the object HashMap GC object
                let map_ptr = obj_ptr.as_ptr();
                let header_ptr = unsafe { (map_ptr as *mut crate::gc::header::GcHeader).sub(1) };

                // Mark the HashMap itself
                unsafe {
                    if (*header_ptr).is_marked() {
                        return; // Already marked, avoid infinite recursion
                    }
                    (*header_ptr).mark();
                }

                // Mark all values in the map
                let obj = unsafe { &*map_ptr };
                for value in obj.values() {
                    self.mark_json_value(value);
                }
            }
            // Primitives don't need marking
            JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Undefined => {}
        }
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

    /// Get GC statistics
    pub fn stats(&self) -> &GcStats {
        &self.stats
    }

    /// Get heap statistics
    pub fn heap_stats(&self) -> HeapStats {
        HeapStats {
            allocated_bytes: self.heap.allocated_bytes(),
            allocation_count: self.heap.allocation_count(),
            threshold: self.threshold,
        }
    }

    /// Get read-only access to the heap for reflection/debugging
    pub fn heap(&self) -> &Heap {
        &self.heap
    }
}

/// Heap statistics
#[derive(Debug, Clone)]
pub struct HeapStats {
    /// Total allocated bytes
    pub allocated_bytes: usize,

    /// Number of allocations
    pub allocation_count: usize,

    /// GC threshold
    pub threshold: usize,
}

impl Default for GarbageCollector {
    fn default() -> Self {
        let context_id = VmContextId::new();
        let type_registry = Arc::new(crate::vm::types::create_standard_registry());
        Self::new(context_id, type_registry)
    }
}

// SAFETY: GarbageCollector is only accessed through a Mutex in SharedVmState,
// which ensures synchronized access to the internal heap and raw pointers.
unsafe impl Send for GarbageCollector {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gc_creation() {
        let gc = GarbageCollector::default();
        let stats = gc.heap_stats();

        assert_eq!(stats.allocated_bytes, 0);
        assert_eq!(stats.allocation_count, 0);
    }

    #[test]
    fn test_gc_allocate() {
        let mut gc = GarbageCollector::default();
        let ptr = gc.allocate(42i32);

        assert_eq!(*ptr, 42);

        let stats = gc.heap_stats();
        assert_eq!(stats.allocation_count, 1);
        assert!(stats.allocated_bytes > 0);
    }

    #[test]
    fn test_gc_allocate_multiple() {
        let mut gc = GarbageCollector::default();

        let ptr1 = gc.allocate(10i32);
        let ptr2 = gc.allocate(20i32);
        let ptr3 = gc.allocate(30i32);

        assert_eq!(*ptr1, 10);
        assert_eq!(*ptr2, 20);
        assert_eq!(*ptr3, 30);

        let stats = gc.heap_stats();
        assert_eq!(stats.allocation_count, 3);
    }

    #[test]
    fn test_gc_threshold() {
        let mut gc = GarbageCollector::default();
        gc.set_threshold(1024); // 1KB threshold

        // Allocate below threshold
        let _ptr = gc.allocate(100i32);

        let stats = gc.stats();
        assert_eq!(stats.collections, 0); // No collection yet
    }

    #[test]
    fn test_gc_collect() {
        let mut gc = GarbageCollector::default();

        // Allocate some objects
        let _ptr1 = gc.allocate(10i32);
        let _ptr2 = gc.allocate(20i32);

        // Run collection
        gc.collect();

        let stats = gc.stats();
        assert_eq!(stats.collections, 1);
    }

    #[test]
    fn test_gc_array() {
        let mut gc = GarbageCollector::default();
        let array = gc.allocate_array::<i32>(10);

        assert_eq!(array.len(), 10);

        let stats = gc.heap_stats();
        assert_eq!(stats.allocation_count, 1);
    }
}
