//! GC object header
//!
//! Every heap-allocated object has a header that stores metadata for the GC.

use crate::vm::interpreter::VmContextId;
use std::any::TypeId;

/// Drop glue function pointer type
pub type DropFn = unsafe fn(*mut u8, usize);

/// GC header stored before each allocated object
///
/// Layout in memory:
/// ```text
/// ┌─────────────────────────────────────────┐
/// │ GcHeader (72 bytes, 8-byte aligned)     │
/// │  - marked: bool (1 byte)                │
/// │  - padding: [u8; 7]                     │
/// │  - context_id: VmContextId (8 bytes)    │
/// │  - type_id: TypeId (16 bytes)           │
/// │  - size: usize (8 bytes)                │
/// │  - align: usize (8 bytes)               │
/// │  - value_offset: usize (8 bytes)        │
/// │  - drop_fn: Option<DropFn> (8 bytes)    │
/// │  - element_count: usize (8 bytes)       │
/// ├─────────────────────────────────────────┤
/// │ Back-pointer to GcHeader (8 bytes)      │
/// ├─────────────────────────────────────────┤
/// │ Object data (variable size)             │
/// └─────────────────────────────────────────┘
/// ```
#[repr(C, align(8))]
#[derive(Debug, Clone, Copy)]
pub struct GcHeader {
    /// Mark bit for GC (true = reachable)
    marked: bool,

    /// Padding for alignment
    _padding: [u8; 7],

    /// Context ID (which VmContext owns this object)
    context_id: VmContextId,

    /// Type ID for runtime type information
    type_id: TypeId,

    /// Size of the allocation (including header)
    size: usize,

    /// Allocation alignment used for deallocation.
    align: usize,

    /// Offset from header start to value data
    value_offset: usize,

    /// Drop glue function (if type has destructor)
    drop_fn: Option<DropFn>,

    /// Element count (for arrays/slices)
    element_count: usize,
}

impl GcHeader {
    /// Create a new GC header
    pub fn new(
        context_id: VmContextId,
        type_id: TypeId,
        size: usize,
        align: usize,
        value_offset: usize,
        drop_fn: Option<DropFn>,
        element_count: usize,
    ) -> Self {
        Self {
            marked: false,
            _padding: [0; 7],
            context_id,
            type_id,
            size,
            align,
            value_offset,
            drop_fn,
            element_count,
        }
    }

    /// Get the value offset
    #[inline]
    pub fn value_offset(&self) -> usize {
        self.value_offset
    }

    /// Get the allocation alignment used for this object.
    #[inline]
    pub fn align(&self) -> usize {
        self.align
    }

    /// Get the drop function
    #[inline]
    pub fn drop_fn(&self) -> Option<DropFn> {
        self.drop_fn
    }

    /// Get the element count
    #[inline]
    pub fn element_count(&self) -> usize {
        self.element_count
    }

    /// Run the drop glue for this object (if it has one)
    pub unsafe fn run_drop(&self, header_ptr: *mut GcHeader) {
        if let Some(drop_fn) = self.drop_fn {
            let value_ptr = (header_ptr as *mut u8).add(self.value_offset);
            drop_fn(value_ptr, self.element_count);
        }
    }

    /// Check if this object is marked
    #[inline]
    pub fn is_marked(&self) -> bool {
        self.marked
    }

    /// Mark this object as reachable
    #[inline]
    pub fn mark(&mut self) {
        self.marked = true;
    }

    /// Unmark this object (for next GC cycle)
    #[inline]
    pub fn unmark(&mut self) {
        self.marked = false;
    }

    /// Get the context ID
    #[inline]
    pub fn context_id(&self) -> VmContextId {
        self.context_id
    }

    /// Get the type ID
    #[inline]
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    /// Get the allocation size
    #[inline]
    pub fn size(&self) -> usize {
        self.size
    }
}

/// Recover the GC header pointer from a value pointer using the stored back-pointer.
///
/// # Safety
///
/// `value_ptr` must point to an allocation produced by the GC heap allocator.
#[inline]
pub unsafe fn header_ptr_from_value_ptr(value_ptr: *const u8) -> *const GcHeader {
    let backlink_ptr =
        value_ptr.sub(std::mem::size_of::<*const GcHeader>()) as *const *const GcHeader;
    *backlink_ptr
}

/// Mutable version of [`header_ptr_from_value_ptr`].
///
/// # Safety
///
/// `value_ptr` must point to an allocation produced by the GC heap allocator.
#[inline]
pub unsafe fn header_mut_ptr_from_value_ptr(value_ptr: *mut u8) -> *mut GcHeader {
    let backlink_ptr = value_ptr.sub(std::mem::size_of::<*mut GcHeader>()) as *const *mut GcHeader;
    *backlink_ptr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_size() {
        assert_eq!(std::mem::size_of::<GcHeader>(), 72);
    }

    #[test]
    fn test_header_alignment() {
        // Header should be 8-byte aligned
        assert_eq!(std::mem::align_of::<GcHeader>(), 8);
    }

    #[test]
    fn test_header_mark_unmark() {
        let context_id = VmContextId::new();
        let mut header = GcHeader::new(context_id, TypeId::of::<i32>(), 64, 8, 0, None, 1);
        assert!(!header.is_marked());

        header.mark();
        assert!(header.is_marked());

        header.unmark();
        assert!(!header.is_marked());
    }

    #[test]
    fn test_header_type_id() {
        let context_id = VmContextId::new();
        let header = GcHeader::new(context_id, TypeId::of::<String>(), 128, 8, 0, None, 1);
        assert_eq!(header.type_id(), TypeId::of::<String>());
    }
}
