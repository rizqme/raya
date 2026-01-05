//! GC-managed smart pointers
//!
//! This module provides `GcPtr<T>`, a smart pointer type for GC-managed objects.

use super::header::GcHeader;
use std::fmt;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

/// A GC-managed pointer to a heap-allocated object
///
/// # Memory Layout
///
/// ```text
/// ┌─────────────────────────────────────────┐
/// │ GcHeader (16 bytes)                     │
/// ├─────────────────────────────────────────┤  ← ptr points here
/// │ T (object data)                         │
/// └─────────────────────────────────────────┘
/// ```
///
/// # Safety
///
/// - The pointer must always point to valid memory allocated by the GC
/// - The object may be freed during GC collection
/// - Users must ensure pointers are registered as GC roots if they need to survive collection
#[derive(Debug)]
pub struct GcPtr<T: ?Sized> {
    ptr: NonNull<T>,
    _phantom: PhantomData<T>,
}

impl<T: ?Sized> GcPtr<T> {
    /// Create a new GC pointer (used by heap allocator)
    ///
    /// # Safety
    ///
    /// The pointer must point to a valid object allocated by the GC,
    /// with a GcHeader immediately preceding it.
    #[inline]
    pub unsafe fn new(ptr: NonNull<T>) -> Self {
        Self {
            ptr,
            _phantom: PhantomData,
        }
    }

    /// Get the raw pointer
    #[inline]
    pub fn as_ptr(&self) -> *mut T {
        self.ptr.as_ptr()
    }

    /// Get the GC header for this object
    ///
    /// # Safety
    ///
    /// The caller must ensure the pointer is still valid (not freed).
    #[inline]
    pub unsafe fn header(&self) -> &GcHeader {
        let header_ptr = (self.ptr.as_ptr() as *const u8).sub(std::mem::size_of::<GcHeader>());
        &*(header_ptr as *const GcHeader)
    }

    /// Get a mutable reference to the GC header
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - The pointer is still valid (not freed)
    /// - No other code is accessing the header
    #[inline]
    pub unsafe fn header_mut(&self) -> &mut GcHeader {
        let header_ptr = (self.ptr.as_ptr() as *mut u8).sub(std::mem::size_of::<GcHeader>());
        &mut *(header_ptr as *mut GcHeader)
    }

    /// Check if this object is marked
    #[inline]
    pub fn is_marked(&self) -> bool {
        unsafe { self.header().is_marked() }
    }

    /// Mark this object as reachable
    #[inline]
    pub fn mark(&self) {
        unsafe { self.header_mut().mark() }
    }

    /// Unmark this object
    #[inline]
    pub fn unmark(&self) {
        unsafe { self.header_mut().unmark() }
    }

    /// Get the address as usize (for hashing/comparison)
    #[inline]
    pub fn addr(&self) -> usize {
        // Cast to thin pointer first to handle unsized types
        self.ptr.as_ptr() as *const () as usize
    }
}

// Clone creates a new pointer to the same object (shallow copy)
impl<T> Clone for GcPtr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

// Copy allows bitwise copying
impl<T> Copy for GcPtr<T> {}

// Equality based on pointer address
impl<T: ?Sized> PartialEq for GcPtr<T> {
    fn eq(&self, other: &Self) -> bool {
        self.ptr == other.ptr
    }
}

impl<T: ?Sized> Eq for GcPtr<T> {}

// Hash based on pointer address
impl<T: ?Sized> std::hash::Hash for GcPtr<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.addr().hash(state);
    }
}

// Deref to access the object
impl<T: ?Sized> Deref for GcPtr<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        unsafe { self.ptr.as_ref() }
    }
}

// DerefMut for mutable access
impl<T: ?Sized> DerefMut for GcPtr<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        unsafe { self.ptr.as_mut() }
    }
}

// Display shows the pointer address
impl<T: ?Sized> fmt::Display for GcPtr<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GcPtr({:#x})", self.addr())
    }
}

// Special impl for slices
impl<T> GcPtr<[T]> {
    /// Get the length of the slice
    #[inline]
    pub fn len(&self) -> usize {
        unsafe { (*self.ptr.as_ptr()).len() }
    }

    /// Check if the slice is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::any::TypeId;

    #[test]
    fn test_gcptr_creation() {
        let data = Box::new(42i32);
        let ptr = NonNull::from(Box::leak(data));
        let gc_ptr = unsafe { GcPtr::new(ptr) };

        assert_eq!(*gc_ptr, 42);

        // Cleanup
        unsafe { drop(Box::from_raw(gc_ptr.as_ptr())); }
    }

    #[test]
    fn test_gcptr_deref() {
        let data = Box::new(100i32);
        let ptr = NonNull::from(Box::leak(data));
        let gc_ptr = unsafe { GcPtr::new(ptr) };

        // Test deref
        assert_eq!(*gc_ptr, 100);

        // Cleanup
        unsafe { drop(Box::from_raw(gc_ptr.as_ptr())); }
    }

    #[test]
    fn test_gcptr_deref_mut() {
        let data = Box::new(50i32);
        let ptr = NonNull::from(Box::leak(data));
        let mut gc_ptr = unsafe { GcPtr::new(ptr) };

        // Test deref_mut
        *gc_ptr = 75;
        assert_eq!(*gc_ptr, 75);

        // Cleanup
        unsafe { drop(Box::from_raw(gc_ptr.as_ptr())); }
    }

    #[test]
    fn test_gcptr_clone() {
        let data = Box::new(200i32);
        let ptr = NonNull::from(Box::leak(data));
        let gc_ptr1 = unsafe { GcPtr::new(ptr) };
        let gc_ptr2 = gc_ptr1.clone();

        assert_eq!(gc_ptr1, gc_ptr2);
        assert_eq!(*gc_ptr1, *gc_ptr2);

        // Cleanup
        unsafe { drop(Box::from_raw(gc_ptr1.as_ptr())); }
    }

    #[test]
    fn test_gcptr_equality() {
        let data1 = Box::new(1i32);
        let data2 = Box::new(1i32);

        let ptr1 = NonNull::from(Box::leak(data1));
        let ptr2 = NonNull::from(Box::leak(data2));

        let gc_ptr1 = unsafe { GcPtr::new(ptr1) };
        let gc_ptr2 = unsafe { GcPtr::new(ptr2) };

        // Different pointers, even if same value
        assert_ne!(gc_ptr1, gc_ptr2);

        // Same pointer
        let gc_ptr3 = gc_ptr1;
        assert_eq!(gc_ptr1, gc_ptr3);

        // Cleanup
        unsafe {
            drop(Box::from_raw(gc_ptr1.as_ptr()));
            drop(Box::from_raw(gc_ptr2.as_ptr()));
        }
    }

    #[test]
    fn test_gcptr_size() {
        // GcPtr should be the size of a pointer
        assert_eq!(
            std::mem::size_of::<GcPtr<i32>>(),
            std::mem::size_of::<*mut i32>()
        );
    }
}
