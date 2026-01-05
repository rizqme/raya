//! Pointer maps for precise garbage collection
//!
//! Pointer maps describe the layout of pointers within objects,
//! enabling precise GC that knows exactly where pointers are located.

use std::fmt;

/// Pointer map describing pointer locations in an object
///
/// This allows the GC to precisely identify and traverse all pointers
/// without conservative scanning.
#[derive(Debug, Clone)]
pub enum PointerMap {
    /// No pointers in this type (e.g., primitives, strings)
    None,

    /// All fields are pointers, with the given count
    /// Used for arrays of objects: `Object[]`
    All(usize),

    /// Specific field offsets that contain pointers
    /// Used for structs/objects with mixed pointer/non-pointer fields
    Offsets(Vec<usize>),

    /// Array of values, each element has the child pointer map
    /// Used for nested structures: `Array<Array<T>>`
    Array {
        /// Number of elements
        length: usize,
        /// Pointer map for each element
        element_map: Box<PointerMap>,
    },
}

impl PointerMap {
    /// Create a pointer map with no pointers
    pub fn none() -> Self {
        PointerMap::None
    }

    /// Create a pointer map for all-pointer layout
    pub fn all(count: usize) -> Self {
        PointerMap::All(count)
    }

    /// Create a pointer map with specific offsets
    pub fn offsets(offsets: Vec<usize>) -> Self {
        PointerMap::Offsets(offsets)
    }

    /// Create a pointer map for an array
    pub fn array(length: usize, element_map: PointerMap) -> Self {
        PointerMap::Array {
            length,
            element_map: Box::new(element_map),
        }
    }

    /// Check if this map contains any pointers
    pub fn has_pointers(&self) -> bool {
        match self {
            PointerMap::None => false,
            PointerMap::All(count) => *count > 0,
            PointerMap::Offsets(offsets) => !offsets.is_empty(),
            PointerMap::Array { length, element_map } => {
                *length > 0 && element_map.has_pointers()
            }
        }
    }

    /// Iterate over all pointer offsets in this object
    ///
    /// Calls `f` for each byte offset that contains a pointer.
    pub fn for_each_pointer_offset<F>(&self, base_offset: usize, mut f: F)
    where
        F: FnMut(usize),
    {
        self.for_each_pointer_offset_impl(base_offset, &mut f);
    }

    fn for_each_pointer_offset_impl(&self, base_offset: usize, f: &mut dyn FnMut(usize)) {
        match self {
            PointerMap::None => {}
            PointerMap::All(count) => {
                // All fields are pointers (8 bytes each on 64-bit)
                for i in 0..*count {
                    f(base_offset + i * 8);
                }
            }
            PointerMap::Offsets(offsets) => {
                for &offset in offsets {
                    f(base_offset + offset);
                }
            }
            PointerMap::Array { length, element_map } => {
                // Assume 8-byte elements for now (Value size)
                for i in 0..*length {
                    element_map.for_each_pointer_offset_impl(base_offset + i * 8, f);
                }
            }
        }
    }

    /// Get the total number of pointers
    pub fn pointer_count(&self) -> usize {
        match self {
            PointerMap::None => 0,
            PointerMap::All(count) => *count,
            PointerMap::Offsets(offsets) => offsets.len(),
            PointerMap::Array { length, element_map } => {
                length * element_map.pointer_count()
            }
        }
    }
}

impl fmt::Display for PointerMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PointerMap::None => write!(f, "None"),
            PointerMap::All(count) => write!(f, "All({})", count),
            PointerMap::Offsets(offsets) => {
                write!(f, "Offsets({:?})", offsets)
            }
            PointerMap::Array { length, element_map } => {
                write!(f, "Array[{}]({})", length, element_map)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pointer_map_none() {
        let map = PointerMap::none();
        assert!(!map.has_pointers());
        assert_eq!(map.pointer_count(), 0);
    }

    #[test]
    fn test_pointer_map_all() {
        let map = PointerMap::all(3);
        assert!(map.has_pointers());
        assert_eq!(map.pointer_count(), 3);

        let mut offsets = Vec::new();
        map.for_each_pointer_offset(0, |offset| offsets.push(offset));
        assert_eq!(offsets, vec![0, 8, 16]);
    }

    #[test]
    fn test_pointer_map_offsets() {
        let map = PointerMap::offsets(vec![0, 16, 32]);
        assert!(map.has_pointers());
        assert_eq!(map.pointer_count(), 3);

        let mut offsets = Vec::new();
        map.for_each_pointer_offset(0, |offset| offsets.push(offset));
        assert_eq!(offsets, vec![0, 16, 32]);
    }

    #[test]
    fn test_pointer_map_array() {
        let element_map = PointerMap::offsets(vec![0]);
        let map = PointerMap::array(3, element_map);
        assert!(map.has_pointers());
        assert_eq!(map.pointer_count(), 3);

        let mut offsets = Vec::new();
        map.for_each_pointer_offset(0, |offset| offsets.push(offset));
        assert_eq!(offsets, vec![0, 8, 16]);
    }

    #[test]
    fn test_pointer_map_with_base_offset() {
        let map = PointerMap::all(2);
        let mut offsets = Vec::new();
        map.for_each_pointer_offset(100, |offset| offsets.push(offset));
        assert_eq!(offsets, vec![100, 108]);
    }
}
