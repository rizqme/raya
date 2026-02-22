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
/// │ GcHeader (64 bytes, 8-byte aligned)     │
/// │  - marked: bool (1 byte)                │
/// │  - padding: [u8; 7]                     │
/// │  - context_id: VmContextId (8 bytes)    │
/// │  - type_id: TypeId (16 bytes)           │
/// │  - size: usize (8 bytes)                │
/// │  - value_offset: u8 (1 byte)            │
/// │  - drop_fn: Option<DropFn> (8 bytes)    │
/// │  - element_count: usize (8 bytes)       │
/// │  - _padding2: [u8; 7]                   │
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

    /// Offset from header start to value data
    value_offset: u8,

    /// Drop glue function (if type has destructor)
    drop_fn: Option<DropFn>,

    /// Element count (for arrays/slices)
    element_count: usize,

    /// Additional padding to maintain alignment
    _padding2: [u8; 7],
}

impl GcHeader {
    /// Create a new GC header
    pub fn new(context_id: VmContextId, type_id: TypeId, size: usize, value_offset: u8, drop_fn: Option<DropFn>, element_count: usize) -> Self {
        Self {
            marked: false,
            _padding: [0; 7],
            context_id,
            type_id,
            size,
            value_offset,
            drop_fn,
            element_count,
            _padding2: [0; 7],
        }
    }

    /// Get the value offset
    #[inline]
    pub fn value_offset(&self) -> u8 {
        self.value_offset
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
            let value_ptr = (header_ptr as *mut u8).add(self.value_offset as usize);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_size() {
        // Header size: 1 byte (marked) + 7 bytes (padding) + 8 bytes (context_id) +
        //              16 bytes (TypeId) + 8 bytes (size) + 1 byte (value_offset) +
        //              7 bytes (padding) + 8 bytes (drop_fn) + 8 bytes (element_count) +
        //              7 bytes (padding2) + 1 byte (alignment padding) = 72 bytes
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
        let mut header = GcHeader::new(context_id, TypeId::of::<i32>(), 64, 0, None, 1);
        assert!(!header.is_marked());

        header.mark();
        assert!(header.is_marked());

        header.unmark();
        assert!(!header.is_marked());
    }

    #[test]
    fn test_header_type_id() {
        let context_id = VmContextId::new();
        let header = GcHeader::new(context_id, TypeId::of::<String>(), 128, 0, None, 1);
        assert_eq!(header.type_id(), TypeId::of::<String>());
    }
}
