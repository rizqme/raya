//! Per-task nursery allocator for short-lived objects
//!
//! The nursery is a fast bump-allocated region that avoids GC lock contention
//! for temporary allocations. Objects allocated in the nursery are either:
//! - Promoted to the shared heap when they escape (stored in globals, channels, etc.)
//! - Discarded en masse when the nursery is reset (at task completion or when full)

use std::cell::UnsafeCell;

/// Default capacity for the nursery (64 KB)
const NURSERY_CAPACITY: usize = 64 * 1024;

/// Per-task bump allocator for short-lived objects
///
/// The nursery provides fast allocation without acquiring the GC lock.
/// It uses a simple bump pointer strategy: allocations just advance a cursor.
///
/// # Safety
///
/// The nursery is `!Sync` and must be owned by a single task. All pointers
/// allocated from a nursery become invalid after `reset()` is called.
#[derive(Debug)]
pub struct Nursery {
    /// Backing buffer for allocations
    buffer: UnsafeCell<Vec<u8>>,

    /// Current allocation cursor (offset into buffer)
    cursor: UnsafeCell<usize>,

    /// Total capacity of the nursery
    capacity: usize,

    /// Number of allocations made (for statistics)
    allocation_count: UnsafeCell<usize>,
}

impl Nursery {
    /// Create a new nursery with default capacity
    pub fn new() -> Self {
        Self {
            buffer: UnsafeCell::new(vec![0; NURSERY_CAPACITY]),
            cursor: UnsafeCell::new(0),
            capacity: NURSERY_CAPACITY,
            allocation_count: UnsafeCell::new(0),
        }
    }

    /// Create a new nursery with custom capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: UnsafeCell::new(vec![0; capacity]),
            cursor: UnsafeCell::new(0),
            capacity,
            allocation_count: UnsafeCell::new(0),
        }
    }

    /// Bump-allocate within the nursery. Returns `None` if full.
    ///
    /// # Safety
    ///
    /// The returned pointer is valid only until `reset()` is called.
    /// The caller must ensure the value doesn't outlive the nursery.
    pub unsafe fn allocate<T>(&mut self, value: T) -> Option<*mut T> {
        let size = std::mem::size_of::<T>();
        let align = std::mem::align_of::<T>();

        let cursor = *self.cursor.get();

        // Align the cursor
        let aligned_offset = (cursor + align - 1) & !(align - 1);

        // Check if we have enough space
        if aligned_offset + size > self.capacity {
            return None; // Nursery full, fall back to shared GC
        }

        let ptr = (*self.buffer.get()).as_mut_ptr().add(aligned_offset) as *mut T;

        // Write the value
        ptr.write(value);

        // Update cursor
        *self.cursor.get() = aligned_offset + size;
        *self.allocation_count.get() += 1;

        Some(ptr)
    }

    /// Reset the nursery (reuse buffer). All pointers become invalid.
    ///
    /// # Safety
    ///
    /// All pointers previously returned by `allocate()` become invalid.
    /// The caller must ensure no live references exist to nursery objects.
    pub unsafe fn reset(&mut self) {
        *self.cursor.get() = 0;
        *self.allocation_count.get() = 0;
    }

    /// Get the current cursor position (bytes used)
    pub fn used_bytes(&self) -> usize {
        unsafe { *self.cursor.get() }
    }

    /// Get the total capacity
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Get the number of allocations
    pub fn allocation_count(&self) -> usize {
        unsafe { *self.allocation_count.get() }
    }

    /// Check if the nursery is empty
    pub fn is_empty(&self) -> bool {
        self.used_bytes() == 0
    }

    /// Get remaining space
    pub fn remaining_bytes(&self) -> usize {
        self.capacity - self.used_bytes()
    }
}

impl Default for Nursery {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nursery_allocate() {
        let mut nursery = Nursery::new();

        let ptr = nursery.allocate(42i32);
        assert!(ptr.is_some());
        assert_eq!(unsafe { *ptr.unwrap() }, 42);
    }

    #[test]
    fn test_nursery_multiple_allocations() {
        let mut nursery = Nursery::new();

        let p1 = nursery.allocate(1i32);
        let p2 = nursery.allocate(2i32);
        let p3 = nursery.allocate(3i32);

        assert!(p1.is_some());
        assert!(p2.is_some());
        assert!(p3.is_some());

        assert_eq!(unsafe { *p1.unwrap() }, 1);
        assert_eq!(unsafe { *p2.unwrap() }, 2);
        assert_eq!(unsafe { *p3.unwrap() }, 3);

        assert_eq!(nursery.allocation_count(), 3);
    }

    #[test]
    fn test_nursery_reset() {
        let mut nursery = Nursery::new();

        nursery.allocate(42i32);
        assert_eq!(nursery.used_bytes(), 4);
        assert_eq!(nursery.allocation_count(), 1);

        unsafe {
            nursery.reset();
        }

        assert_eq!(nursery.used_bytes(), 0);
        assert_eq!(nursery.allocation_count(), 0);
    }

    #[test]
    fn test_nursery_overflow() {
        let mut nursery = Nursery::with_capacity(16);

        // Small allocations should work
        assert!(nursery.allocate(1i32).is_some());
        assert!(nursery.allocate(2i32).is_some());
        assert!(nursery.allocate(3i32).is_some());

        // Large allocation should fail
        let large: [u8; 32] = [0; 32];
        assert!(nursery.allocate(large).is_none());
    }

    #[test]
    fn test_nursery_remaining_bytes() {
        let mut nursery = Nursery::with_capacity(64);

        assert_eq!(nursery.remaining_bytes(), 64);

        nursery.allocate(1i32);
        assert_eq!(nursery.remaining_bytes(), 60);

        nursery.allocate(1i32);
        assert_eq!(nursery.remaining_bytes(), 56);
    }

    #[test]
    fn test_nursery_alignment() {
        let mut nursery = Nursery::new();

        // Allocate a bool (1 byte) then a u64 (8 byte aligned)
        nursery.allocate(true);
        let ptr = nursery.allocate(42u64);

        assert!(ptr.is_some());
        // The u64 should be properly aligned
        assert_eq!(unsafe { ptr.unwrap() as usize } % 8, 0);
    }

    #[test]
    fn test_nursery_drop() {
        use std::sync::atomic::{AtomicBool, Ordering};

        static DROPPED: AtomicBool = AtomicBool::new(false);

        struct Dropper;
        impl Drop for Dropper {
            fn drop(&mut self) {
                DROPPED.store(true, Ordering::SeqCst);
            }
        }

        let mut nursery = Nursery::new();
        nursery.allocate(Dropper);

        // Reset doesn't drop - we need to drop manually before reset
        unsafe {
            nursery.reset();
        }

        // Note: In the actual implementation, we'll need to call drop glue
        // before resetting. For now, this test documents the behavior.
    }
}
