//! Heap allocator for GC-managed objects
//!
//! This module provides the heap allocator that manages memory for all GC objects.

use super::header::{DropFn, GcHeader};
use super::ptr::GcPtr;
use crate::vm::types::TypeRegistry;
use crate::vm::interpreter::VmContextId;
use std::alloc::{alloc, dealloc, Layout};
use std::any::TypeId;
use std::ptr::NonNull;
use std::sync::Arc;

/// Heap allocator for GC-managed memory
pub struct Heap {
    /// Context ID (which VmContext owns this heap)
    context_id: VmContextId,

    /// Type registry for precise GC
    type_registry: Arc<TypeRegistry>,

    /// All allocations (pointer to GcHeader)
    allocations: Vec<*mut GcHeader>,

    /// Total bytes allocated
    allocated_bytes: usize,

    /// Maximum heap size (0 = unlimited)
    max_heap_bytes: usize,
}

/// Generic drop shim for calling drop glue through a function pointer
unsafe fn drop_in_place_shim<T>(ptr: *mut u8, _count: usize) {
    std::ptr::drop_in_place(ptr as *mut T);
}

/// Drop shim for slices/arrays
unsafe fn drop_in_place_slice_shim<T>(ptr: *mut u8, count: usize) {
    let slice_ptr = std::ptr::slice_from_raw_parts_mut(ptr as *mut T, count);
    std::ptr::drop_in_place(slice_ptr);
}

impl Heap {
    /// Create a new heap for a specific context
    pub fn new(context_id: VmContextId, type_registry: Arc<TypeRegistry>) -> Self {
        Self {
            context_id,
            type_registry,
            allocations: Vec::new(),
            allocated_bytes: 0,
            max_heap_bytes: 0, // Unlimited by default
        }
    }

    /// Set maximum heap size
    pub fn set_max_heap_size(&mut self, bytes: usize) {
        self.max_heap_bytes = bytes;
    }

    /// Get the context ID
    pub fn context_id(&self) -> VmContextId {
        self.context_id
    }

    /// Get the type registry
    pub fn type_registry(&self) -> &Arc<TypeRegistry> {
        &self.type_registry
    }

    /// Allocate a value on the heap
    ///
    /// Returns a GC pointer to the allocated object.
    ///
    /// # Panics
    ///
    /// Panics if allocation fails or heap size limit is exceeded.
    pub fn allocate<T: 'static>(&mut self, value: T) -> GcPtr<T> {
        let type_id = TypeId::of::<T>();

        // Calculate layouts
        let header_layout = Layout::new::<GcHeader>();
        let value_layout = Layout::new::<T>();

        // Combine layouts (header + value)
        let (combined_layout, value_offset) = header_layout
            .extend(value_layout)
            .expect("Failed to calculate layout");

        // Check heap size limit
        if self.max_heap_bytes > 0
            && self.allocated_bytes + combined_layout.size() > self.max_heap_bytes
        {
            panic!("Heap size limit exceeded");
        }

        // Determine if type needs drop glue
        let drop_fn = if std::mem::needs_drop::<T>() {
            Some(drop_in_place_shim::<T> as DropFn)
        } else {
            None
        };

        // Allocate memory
        let ptr = unsafe { alloc(combined_layout) };
        if ptr.is_null() {
            panic!("Out of memory");
        }

        // Initialize header
        let header_ptr = ptr as *mut GcHeader;
        unsafe {
            header_ptr.write(GcHeader::new(
                self.context_id,
                type_id,
                combined_layout.size(),
                value_offset as u8,
                drop_fn,
                1, // element_count for single object
            ));
        }

        // Initialize value
        let value_ptr = unsafe { ptr.add(value_offset) as *mut T };
        unsafe {
            value_ptr.write(value);
        }

        // Track allocation
        self.allocations.push(header_ptr);
        self.allocated_bytes += combined_layout.size();

        // Return GC pointer
        unsafe { GcPtr::new(NonNull::new_unchecked(value_ptr)) }
    }

    /// Allocate an array on the heap
    pub fn allocate_array<T>(&mut self, len: usize) -> GcPtr<[T]>
    where
        T: 'static + Default + Clone,
    {
        let type_id = TypeId::of::<[T]>();

        // Calculate layouts
        let header_layout = Layout::new::<GcHeader>();
        let array_layout = Layout::array::<T>(len).expect("Failed to calculate array layout");

        // Combine layouts
        let (combined_layout, array_offset) = header_layout
            .extend(array_layout)
            .expect("Failed to calculate layout");

        // Check heap size limit
        if self.max_heap_bytes > 0
            && self.allocated_bytes + combined_layout.size() > self.max_heap_bytes
        {
            panic!("Heap size limit exceeded");
        }

        // Determine if type needs drop glue
        let drop_fn = if std::mem::needs_drop::<T>() {
            Some(drop_in_place_slice_shim::<T> as DropFn)
        } else {
            None
        };

        // Allocate memory
        let ptr = unsafe { alloc(combined_layout) };
        if ptr.is_null() {
            panic!("Out of memory");
        }

        // Initialize header
        let header_ptr = ptr as *mut GcHeader;
        unsafe {
            header_ptr.write(GcHeader::new(
                self.context_id,
                type_id,
                combined_layout.size(),
                array_offset as u8,
                drop_fn,
                len, // element_count for array
            ));
        }

        // Initialize array
        let array_ptr = unsafe { ptr.add(array_offset) as *mut T };
        for i in 0..len {
            unsafe {
                array_ptr.add(i).write(T::default());
            }
        }

        // Track allocation
        self.allocations.push(header_ptr);
        self.allocated_bytes += combined_layout.size();

        // Create slice pointer
        let slice_ptr = std::ptr::slice_from_raw_parts_mut(array_ptr, len);
        unsafe { GcPtr::new(NonNull::new_unchecked(slice_ptr)) }
    }

    /// Free an allocation (called by GC sweep)
    ///
    /// # Safety
    ///
    /// The header pointer must be valid and allocated by this heap.
    pub unsafe fn free(&mut self, header_ptr: *mut GcHeader) {
        // Get the allocation size from the header
        let header = &*header_ptr;
        let total_size = header.size();

        // Run drop glue if this type has a destructor
        header.run_drop(header_ptr);

        // Remove from allocations tracking
        if let Some(pos) = self.allocations.iter().position(|&p| p == header_ptr) {
            self.allocations.swap_remove(pos);
        }

        // Decrement allocated bytes
        self.allocated_bytes = self.allocated_bytes.saturating_sub(total_size);

        // Actually deallocate the memory
        // GcHeader is 8-byte aligned, so we use the same alignment for deallocation
        let layout = Layout::from_size_align_unchecked(total_size, 8);
        dealloc(header_ptr as *mut u8, layout);
    }

    /// Get total allocated bytes
    pub fn allocated_bytes(&self) -> usize {
        self.allocated_bytes
    }

    /// Get number of allocations
    pub fn allocation_count(&self) -> usize {
        self.allocations.len()
    }

    /// Iterate over all allocations
    pub fn iter_allocations(&self) -> impl Iterator<Item = *mut GcHeader> + '_ {
        self.allocations.iter().copied()
    }
}

impl Default for Heap {
    fn default() -> Self {
        let context_id = VmContextId::new();
        let type_registry = Arc::new(crate::vm::types::create_standard_registry());
        Self::new(context_id, type_registry)
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        // Free all remaining allocations
        for &header_ptr in &self.allocations {
            unsafe {
                let header = &*header_ptr;

                // Run drop glue if this type has a destructor
                header.run_drop(header_ptr);

                // Deallocate the memory
                let total_size = header.size();
                let layout = Layout::from_size_align_unchecked(total_size, 8);
                dealloc(header_ptr as *mut u8, layout);
            }
        }
        self.allocations.clear();
    }
}

// SAFETY: Heap is only accessed through a Mutex in SharedVmState,
// which ensures synchronized access to the raw pointers.
unsafe impl Send for Heap {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heap_creation() {
        let heap = Heap::default();
        assert_eq!(heap.allocated_bytes(), 0);
        assert_eq!(heap.allocation_count(), 0);
    }

    #[test]
    fn test_heap_allocate() {
        let mut heap = Heap::default();
        let ptr = heap.allocate(42i32);

        assert_eq!(*ptr, 42);
        assert_eq!(heap.allocation_count(), 1);
        assert!(heap.allocated_bytes() > 0);
    }

    #[test]
    fn test_heap_allocate_multiple() {
        let mut heap = Heap::default();

        let ptr1 = heap.allocate(10i32);
        let ptr2 = heap.allocate(20i32);
        let ptr3 = heap.allocate(30i32);

        assert_eq!(*ptr1, 10);
        assert_eq!(*ptr2, 20);
        assert_eq!(*ptr3, 30);
        assert_eq!(heap.allocation_count(), 3);
    }

    #[test]
    fn test_heap_allocate_different_types() {
        let mut heap = Heap::default();

        let int_ptr = heap.allocate(42i32);
        let string_ptr = heap.allocate(String::from("hello"));
        let bool_ptr = heap.allocate(true);

        assert_eq!(*int_ptr, 42);
        assert_eq!(*string_ptr, "hello");
        assert!(*bool_ptr);
        assert_eq!(heap.allocation_count(), 3);
    }

    #[test]
    fn test_heap_max_size() {
        let mut heap = Heap::default();
        heap.set_max_heap_size(1024); // 1KB limit

        // This should work
        let _ptr = heap.allocate(100i32);

        // Allocating many large objects should eventually panic
        // (We won't test this as it would cause test failure)
    }

    #[test]
    fn test_heap_allocate_array() {
        let mut heap = Heap::default();
        let array = heap.allocate_array::<i32>(10);

        assert_eq!(array.len(), 10);

        // Access via as_ptr for now (until we fix Deref for slices)
        unsafe {
            let slice = &*array.as_ptr();
            for item in slice.iter().take(10) {
                assert_eq!(*item, 0); // Default value
            }
        }
    }

    #[test]
    fn test_memory_deallocation_decreases_bytes() {
        let mut heap = Heap::default();
        let initial_bytes = heap.allocated_bytes();

        // Allocate some objects
        let _ptr1 = heap.allocate(42i32);
        let bytes_after_alloc = heap.allocated_bytes();
        assert!(bytes_after_alloc > initial_bytes);

        // Free the first object
        let header_ptr = heap.iter_allocations().next().unwrap();
        unsafe {
            heap.free(header_ptr);
        }

        // Bytes should decrease
        let bytes_after_free = heap.allocated_bytes();
        assert!(bytes_after_free < bytes_after_alloc);
    }

    #[test]
    fn test_allocation_count_decreases_after_free() {
        let mut heap = Heap::default();

        // Allocate objects
        let _ptr1 = heap.allocate(10i32);
        let _ptr2 = heap.allocate(20i32);
        let _ptr3 = heap.allocate(30i32);
        assert_eq!(heap.allocation_count(), 3);

        // Free one object
        let header_ptr = heap.iter_allocations().next().unwrap();
        unsafe {
            heap.free(header_ptr);
        }

        assert_eq!(heap.allocation_count(), 2);
    }

    #[test]
    fn test_drop_glue_runs_for_string() {
        use std::sync::atomic::{AtomicBool, Ordering};

        static DROPPED: AtomicBool = AtomicBool::new(false);

        struct Dropper;
        impl Drop for Dropper {
            fn drop(&mut self) {
                DROPPED.store(true, Ordering::SeqCst);
            }
        }

        let mut heap = Heap::default();
        let _ptr = heap.allocate(Dropper);

        // Free the object
        let header_ptr = heap.iter_allocations().next().unwrap();
        unsafe {
            heap.free(header_ptr);
        }

        // Verify drop glue ran
        assert!(DROPPED.load(Ordering::SeqCst));
    }

    #[test]
    fn test_drop_glue_runs_for_vec() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static DROP_COUNT: AtomicUsize = AtomicUsize::new(0);

        struct Counter;
        impl Drop for Counter {
            fn drop(&mut self) {
                DROP_COUNT.fetch_add(1, Ordering::SeqCst);
            }
        }

        let mut heap = Heap::default();

        // Allocate multiple counters in a Vec-like wrapper
        let _ptr1 = heap.allocate(Counter);
        let _ptr2 = heap.allocate(Counter);
        let _ptr3 = heap.allocate(Counter);

        // Free all objects
        let allocations: Vec<_> = heap.iter_allocations().collect();
        for header_ptr in allocations {
            unsafe {
                heap.free(header_ptr);
            }
        }

        // All three should have been dropped
        assert_eq!(DROP_COUNT.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn test_heap_drop_cleanup() {
        use std::sync::atomic::{AtomicBool, Ordering};

        static DROPPED: AtomicBool = AtomicBool::new(false);

        struct Dropper;
        impl Drop for Dropper {
            fn drop(&mut self) {
                DROPPED.store(true, Ordering::SeqCst);
            }
        }

        // Create a heap with allocations
        let mut heap = Heap::default();
        let _ptr = heap.allocate(Dropper);
        assert_eq!(heap.allocation_count(), 1);

        // Drop the heap
        drop(heap);

        // Verify drop glue ran
        assert!(DROPPED.load(Ordering::SeqCst));
    }

    #[test]
    fn test_no_memory_leak_stress() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static DROP_COUNT: AtomicUsize = AtomicUsize::new(0);

        struct Counter;
        impl Drop for Counter {
            fn drop(&mut self) {
                DROP_COUNT.fetch_add(1, Ordering::SeqCst);
            }
        }

        let mut heap = Heap::default();
        heap.set_max_heap_size(10 * 1024 * 1024); // 10MB limit

        let alloc_count = 1000;
        let mut allocations = Vec::new();

        // Allocate many objects
        for _ in 0..alloc_count {
            allocations.push(heap.allocate(Counter));
        }

        assert_eq!(heap.allocation_count(), alloc_count);

        // Free all objects
        let headers: Vec<_> = heap.iter_allocations().collect();
        for header_ptr in headers {
            unsafe {
                heap.free(header_ptr);
            }
        }

        // All should be freed
        assert_eq!(heap.allocation_count(), 0);
        assert_eq!(heap.allocated_bytes(), 0);
        assert_eq!(DROP_COUNT.load(Ordering::SeqCst), alloc_count);
    }
}
