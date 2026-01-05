//! Value representation using tagged pointers (64-bit)
//!
//! This module implements an efficient value representation using tagged pointers.
//! Values are stored in 64 bits with the lowest 3 bits used as a type tag.
//!
//! # Encoding Strategy
//!
//! ```text
//! Pointer:  pppppppppppppppppppppppppppppppppppppppppppppppppppppppppp000
//! i32:      000000000000000000000000000000iiiiiiiiiiiiiiiiiiiiiiiiiiii001
//! bool:     000000000000000000000000000000000000000000000000000000000b010
//! null:     0000000000000000000000000000000000000000000000000000000000110
//! f64:      Special NaN-boxed encoding (future optimization)
//! ```
//!
//! Pointers must be 8-byte aligned (guaranteed by allocator).

use std::fmt;
use std::ptr::NonNull;

/// Tagged pointer value representation
///
/// Values are encoded in 64 bits with tag bits in the lowest 3 bits.
/// This allows for efficient type checking and inline storage of small values.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Value(u64);

impl Value {
    // Tag constants (lowest 3 bits)
    const TAG_MASK: u64 = 0b111;
    const TAG_PTR: u64 = 0b000;
    const TAG_I32: u64 = 0b001;
    const TAG_BOOL: u64 = 0b010;
    const TAG_NULL: u64 = 0b110;

    // Special values
    const NULL: u64 = Self::TAG_NULL;
    const TRUE: u64 = (1 << 3) | Self::TAG_BOOL;
    const FALSE: u64 = (0 << 3) | Self::TAG_BOOL;

    /// Create a null value
    #[inline]
    pub const fn null() -> Self {
        Value(Self::NULL)
    }

    /// Create a boolean value
    #[inline]
    pub const fn bool(b: bool) -> Self {
        Value(if b { Self::TRUE } else { Self::FALSE })
    }

    /// Create an i32 value
    #[inline]
    pub const fn i32(i: i32) -> Self {
        // Store i32 in upper 32 bits, tag in lower bits
        Value((((i as i64) as u64) << 32) | Self::TAG_I32)
    }

    /// Create a pointer value (for heap-allocated objects)
    ///
    /// # Safety
    ///
    /// The pointer must be:
    /// - 8-byte aligned (lowest 3 bits must be 0)
    /// - Valid for the lifetime of this Value
    /// - Managed by the GC
    #[inline]
    pub unsafe fn from_ptr<T>(ptr: NonNull<T>) -> Self {
        let addr = ptr.as_ptr() as usize as u64;
        debug_assert_eq!(addr & Self::TAG_MASK, 0, "Pointer must be 8-byte aligned");
        Value(addr)
    }

    /// Check if this value is null
    #[inline]
    pub const fn is_null(&self) -> bool {
        self.0 == Self::NULL
    }

    /// Check if this value is a boolean
    #[inline]
    pub const fn is_bool(&self) -> bool {
        (self.0 & Self::TAG_MASK) == Self::TAG_BOOL
    }

    /// Check if this value is an i32
    #[inline]
    pub const fn is_i32(&self) -> bool {
        (self.0 & Self::TAG_MASK) == Self::TAG_I32
    }

    /// Check if this value is a heap pointer
    #[inline]
    pub const fn is_ptr(&self) -> bool {
        (self.0 & Self::TAG_MASK) == Self::TAG_PTR
    }

    /// Check if this value is heap-allocated
    #[inline]
    pub const fn is_heap_allocated(&self) -> bool {
        self.is_ptr()
    }

    /// Extract boolean value
    #[inline]
    pub const fn as_bool(&self) -> Option<bool> {
        if self.is_bool() {
            Some((self.0 >> 3) != 0)
        } else {
            None
        }
    }

    /// Extract i32 value
    #[inline]
    pub const fn as_i32(&self) -> Option<i32> {
        if self.is_i32() {
            Some((self.0 >> 32) as i32)
        } else {
            None
        }
    }

    /// Extract pointer value
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - This value is actually a pointer
    /// - The pointer is still valid (not freed by GC)
    /// - The pointer type T matches the actual object type
    #[inline]
    pub unsafe fn as_ptr<T>(&self) -> Option<NonNull<T>> {
        if self.is_ptr() {
            Some(NonNull::new_unchecked(self.0 as usize as *mut T))
        } else {
            None
        }
    }

    /// Get raw bits (for debugging)
    #[inline]
    pub const fn raw(&self) -> u64 {
        self.0
    }

    /// Get tag bits
    #[inline]
    pub const fn tag(&self) -> u64 {
        self.0 & Self::TAG_MASK
    }

    /// Check if value is truthy (for conditionals)
    pub fn is_truthy(&self) -> bool {
        if let Some(b) = self.as_bool() {
            b
        } else if self.is_null() {
            false
        } else if let Some(i) = self.as_i32() {
            i != 0
        } else {
            // Pointers are always truthy
            true
        }
    }

    /// Get type name for debugging
    pub const fn type_name(&self) -> &'static str {
        match self.tag() {
            Self::TAG_NULL => "null",
            Self::TAG_BOOL => "bool",
            Self::TAG_I32 => "i32",
            Self::TAG_PTR => "pointer",
            _ => "unknown",
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.tag() {
            Self::TAG_NULL => write!(f, "null"),
            Self::TAG_BOOL => write!(f, "bool({})", self.as_bool().unwrap()),
            Self::TAG_I32 => write!(f, "i32({})", self.as_i32().unwrap()),
            Self::TAG_PTR => write!(f, "ptr({:#x})", self.0),
            _ => write!(f, "Value({:#x})", self.0),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.tag() {
            Self::TAG_NULL => write!(f, "null"),
            Self::TAG_BOOL => write!(f, "{}", self.as_bool().unwrap()),
            Self::TAG_I32 => write!(f, "{}", self.as_i32().unwrap()),
            Self::TAG_PTR => write!(f, "[object@{:#x}]", self.0),
            _ => write!(f, "<??>"),
        }
    }
}

// Implement Default to return null
impl Default for Value {
    fn default() -> Self {
        Value::null()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_null() {
        let v = Value::null();
        assert!(v.is_null());
        assert!(!v.is_bool());
        assert!(!v.is_i32());
        assert!(!v.is_ptr());
        assert_eq!(v.type_name(), "null");
    }

    #[test]
    fn test_value_bool() {
        let t = Value::bool(true);
        assert!(!t.is_null());
        assert!(t.is_bool());
        assert_eq!(t.as_bool(), Some(true));
        assert_eq!(t.type_name(), "bool");

        let f = Value::bool(false);
        assert!(f.is_bool());
        assert_eq!(f.as_bool(), Some(false));
    }

    #[test]
    fn test_value_i32() {
        let v = Value::i32(42);
        assert!(v.is_i32());
        assert_eq!(v.as_i32(), Some(42));
        assert_eq!(v.type_name(), "i32");

        // Test negative numbers
        let neg = Value::i32(-100);
        assert_eq!(neg.as_i32(), Some(-100));

        // Test zero
        let zero = Value::i32(0);
        assert_eq!(zero.as_i32(), Some(0));

        // Test min/max
        let min = Value::i32(i32::MIN);
        assert_eq!(min.as_i32(), Some(i32::MIN));

        let max = Value::i32(i32::MAX);
        assert_eq!(max.as_i32(), Some(i32::MAX));
    }

    #[test]
    fn test_value_truthiness() {
        assert!(!Value::null().is_truthy());
        assert!(!Value::bool(false).is_truthy());
        assert!(Value::bool(true).is_truthy());
        assert!(!Value::i32(0).is_truthy());
        assert!(Value::i32(1).is_truthy());
        assert!(Value::i32(-1).is_truthy());
    }

    #[test]
    fn test_value_tag() {
        assert_eq!(Value::null().tag(), Value::TAG_NULL);
        assert_eq!(Value::bool(true).tag(), Value::TAG_BOOL);
        assert_eq!(Value::i32(42).tag(), Value::TAG_I32);
    }

    #[test]
    fn test_value_display() {
        assert_eq!(format!("{}", Value::null()), "null");
        assert_eq!(format!("{}", Value::bool(true)), "true");
        assert_eq!(format!("{}", Value::bool(false)), "false");
        assert_eq!(format!("{}", Value::i32(42)), "42");
        assert_eq!(format!("{}", Value::i32(-10)), "-10");
    }

    #[test]
    fn test_value_debug() {
        assert_eq!(format!("{:?}", Value::null()), "null");
        assert_eq!(format!("{:?}", Value::bool(true)), "bool(true)");
        assert_eq!(format!("{:?}", Value::i32(42)), "i32(42)");
    }

    #[test]
    fn test_value_size() {
        // Value should be exactly 8 bytes
        assert_eq!(std::mem::size_of::<Value>(), 8);
    }

    #[test]
    fn test_value_copy() {
        let v1 = Value::i32(42);
        let v2 = v1; // Should copy, not move
        assert_eq!(v1.as_i32(), v2.as_i32());
    }

    #[test]
    fn test_value_equality() {
        assert_eq!(Value::null(), Value::null());
        assert_eq!(Value::bool(true), Value::bool(true));
        assert_eq!(Value::bool(false), Value::bool(false));
        assert_eq!(Value::i32(42), Value::i32(42));

        assert_ne!(Value::bool(true), Value::bool(false));
        assert_ne!(Value::i32(1), Value::i32(2));
        assert_ne!(Value::null(), Value::bool(false));
    }

    #[test]
    fn test_value_pointer_aligned() {
        // Test that pointer encoding preserves alignment
        let data = Box::new(42u64);
        let ptr = NonNull::from(Box::leak(data));

        let v = unsafe { Value::from_ptr(ptr) };
        assert!(v.is_ptr());
        assert_eq!(v.tag(), Value::TAG_PTR);

        // Extract and verify
        let extracted = unsafe { v.as_ptr::<u64>().unwrap() };
        assert_eq!(ptr, extracted);

        // Cleanup
        unsafe { drop(Box::from_raw(extracted.as_ptr())); }
    }
}
