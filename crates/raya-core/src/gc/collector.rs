//! Mark-sweep garbage collector
//!
//! This module implements a simple mark-sweep garbage collector.

use super::header::GcHeader;
use super::heap::Heap;
use super::ptr::GcPtr;
use super::roots::RootSet;
use crate::types::TypeRegistry;
use crate::value::Value;
use crate::vm::VmContextId;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Garbage collector statistics
#[derive(Debug, Clone, Default)]
pub struct GcStats {
    /// Total number of collections
    pub collections: usize,

    /// Total objects freed
    pub objects_freed: usize,

    /// Total bytes freed
    pub bytes_freed: usize,

    /// Total pause time
    pub total_pause_time: Duration,

    /// Last collection duration
    pub last_pause_time: Duration,
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
            threshold: 1024 * 1024, // 1 MB initial threshold
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
    pub fn allocate_array<T: 'static>(&mut self, len: usize) -> GcPtr<[T]>
    where
        T: Default + Clone,
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
        self.mark();

        // Sweep phase
        let freed = self.sweep();

        // Update stats
        let duration = start.elapsed();
        self.stats.collections += 1;
        self.stats.objects_freed += freed;
        self.stats.last_pause_time = duration;
        self.stats.total_pause_time += duration;

        // Adjust threshold (grow by 2x current usage)
        let current_usage = self.heap.allocated_bytes();
        self.threshold = (current_usage * 2).max(1024 * 1024); // At least 1MB
    }

    /// Mark phase: mark all reachable objects
    fn mark(&mut self) {
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
    }

    /// Mark a single value and its references
    fn mark_value(&mut self, value: Value) {
        // Only mark heap-allocated values
        if !value.is_heap_allocated() {
            return;
        }

        // Get the pointer (this is unsafe because we don't have proper object types yet)
        // In a complete implementation, we would:
        // 1. Extract the pointer from the value
        // 2. Check if already marked
        // 3. Mark it
        // 4. Recursively mark its children based on type info
        //
        // For now, this is a placeholder
    }

    /// Sweep phase: free unmarked objects
    fn sweep(&mut self) -> usize {
        let mut freed_count = 0;

        // Collect unmarked allocations
        let to_free: Vec<*mut GcHeader> = self
            .heap
            .iter_allocations()
            .filter(|&header_ptr| unsafe { !(*header_ptr).is_marked() })
            .collect();

        // Free them
        for header_ptr in to_free {
            unsafe {
                self.heap.free(header_ptr);
            }
            freed_count += 1;
        }

        freed_count
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
        let type_registry = Arc::new(crate::types::create_standard_registry());
        Self::new(context_id, type_registry)
    }
}

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
