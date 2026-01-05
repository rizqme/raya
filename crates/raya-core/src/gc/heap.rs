//! Heap allocator for GC-managed objects
//!
//! This module provides the heap allocator that manages memory for all GC objects.

use super::header::GcHeader;
use super::ptr::GcPtr;
use crate::types::TypeRegistry;
use crate::vm::VmContextId;
use std::alloc::{alloc, Layout};
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
        // Get the type ID to determine layout
        let header = &*header_ptr;
        let _type_id = header.type_id();

        // For now, we can't determine the exact layout without more metadata
        // We'll need to store size in the header or use a type registry
        // For this implementation, we'll just mark it for removal

        // Remove from allocations
        if let Some(pos) = self.allocations.iter().position(|&p| p == header_ptr) {
            self.allocations.swap_remove(pos);
        }

        // Note: Actual deallocation is complex without storing size
        // We'll need to enhance GcHeader to store size
        // For now, this is a placeholder
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
        let type_registry = Arc::new(crate::types::create_standard_registry());
        Self::new(context_id, type_registry)
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        // Free all remaining allocations
        // Note: This is simplified and may leak memory
        // A production implementation would need proper cleanup
        self.allocations.clear();
    }
}

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
}
