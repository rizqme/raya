//! Value representation using NaN-boxing (64-bit)
//!
//! This module implements an efficient value representation using NaN-boxing.
//! Values are stored in 64 bits using tagged NaN patterns or direct encoding.
//!
//! # Encoding Strategy
//!
//! Uses NaN-boxing for efficient value representation:
//!
//! ```text
//! f64 (float): Any value where upper 13 bits != 0x1FFF (regular IEEE 754 double)
//! Tagged:      0x1FFF + 3-bit tag + 48-bit payload (NaN-boxed)
//!   - Pointer:   0xFFF8000000000000 | (ptr & 0xFFFFFFFFFFFF)    [tag=000]
//!   - i32 (int): 0xFFF8001000000000 | (i32 as u64)              [tag=001]
//!   - bool:      0xFFF8002000000000 | (b as u64)                [tag=010]
//!   - u32:       0xFFF8003000000000 | (u32 as u64)              [tag=011]
//!   - f32:       0xFFF8004000000000 | (f32.to_bits() as u64)    [tag=100]
//!   - i64:       0xFFF8005000000000 | (i64 as u64 & 0xFFFFFFFF) [tag=101]
//!   - null:      0xFFF8006000000000                             [tag=110]
//!   - u64:       0xFFF8007000000000 | (u64 & 0xFFFFFFFFFFFF)    [tag=111]
//! ```
//!
//! The key insight: IEEE 754 quiet NaN has exponent=0x7FF and mantissa bit 51=1,
//! giving us 0x7FF8... in the upper 16 bits. By using the sign bit and making it
//! 0xFFF8..., we create tagged values that are quiet NaNs but distinguishable.
//!
//! # Type Mapping
//!
//! Raya types map to internal representations:
//! - `number` (integer) -> i32 (main integer type, 32-bit signed)
//! - `number` (float) -> f64 (main float type, 64-bit IEEE 754)
//! - Additional types for FFI and special cases: u32, f32, i64, u64

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
    // NaN-boxing constants
    // Quiet NaN base: sign=1, exp=0x7FF, quiet=1 => 0xFFF8 in upper 16 bits
    const NAN_BOX_BASE: u64 = 0xFFF8_0000_0000_0000;

    // Tag is in bits 48-50 (3 bits, shifted by 48)
    const TAG_SHIFT: u64 = 48;
    const TAG_MASK: u64 = 0x7 << Self::TAG_SHIFT; // 3 bits at position 48

    const TAG_PTR: u64 = 0x0 << Self::TAG_SHIFT;
    const TAG_I32: u64 = 0x1 << Self::TAG_SHIFT;
    const TAG_BOOL: u64 = 0x2 << Self::TAG_SHIFT;
    const TAG_U32: u64 = 0x3 << Self::TAG_SHIFT;
    const TAG_F32: u64 = 0x4 << Self::TAG_SHIFT;
    const TAG_I64: u64 = 0x5 << Self::TAG_SHIFT;
    const TAG_NULL: u64 = 0x6 << Self::TAG_SHIFT;
    const TAG_U64: u64 = 0x7 << Self::TAG_SHIFT;

    // Payload mask (lower 48 bits)
    const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    // 32-bit payload mask
    const PAYLOAD_MASK_32: u64 = 0x0000_0000_FFFF_FFFF;

    // Special values (NaN-boxed)
    const NULL: u64 = Self::NAN_BOX_BASE | Self::TAG_NULL;
    const TRUE: u64 = Self::NAN_BOX_BASE | Self::TAG_BOOL | 1;
    const FALSE: u64 = Self::NAN_BOX_BASE | Self::TAG_BOOL | 0;

    /// Check if this value is NaN-boxed (tagged)
    #[inline]
    const fn is_nan_boxed(&self) -> bool {
        // Check if upper 13 bits are 0x1FFF (sign=1, exp=0x7FF, quiet=1)
        (self.0 & 0xFFF8_0000_0000_0000) == Self::NAN_BOX_BASE
    }

    /// Get the tag from a NaN-boxed value
    #[inline]
    const fn get_tag(&self) -> u64 {
        (self.0 & Self::TAG_MASK) >> Self::TAG_SHIFT
    }

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
        // NaN-box: base | tag | i32 payload (sign-extended to 48 bits)
        Value(Self::NAN_BOX_BASE | Self::TAG_I32 | ((i as i64) as u64 & Self::PAYLOAD_MASK))
    }

    /// Create an f64 value
    #[inline]
    pub fn f64(f: f64) -> Self {
        // Store f64 as IEEE 754 double directly (not NaN-boxed)
        // Valid f64 values won't have 0xFFF8 in upper 16 bits
        Value(f.to_bits())
    }

    /// Create a u32 value
    #[inline]
    pub const fn u32(u: u32) -> Self {
        // NaN-box: base | tag | u32 payload
        Value(Self::NAN_BOX_BASE | Self::TAG_U32 | (u as u64))
    }

    /// Create an f32 value
    #[inline]
    pub const fn f32(f: f32) -> Self {
        // NaN-box: base | tag | f32 bits as payload
        Value(Self::NAN_BOX_BASE | Self::TAG_F32 | (f.to_bits() as u64))
    }

    /// Create an i64 value (limited to 32-bit storage in NaN-box)
    #[inline]
    pub const fn i64(i: i64) -> Self {
        // Store only lower 32 bits - i64 values outside i32 range cannot be represented
        // This is a limitation of NaN-boxing with 48-bit payload
        Value(Self::NAN_BOX_BASE | Self::TAG_I64 | ((i as i32) as u32 as u64))
    }

    /// Create a u64 value (limited to 48-bit storage in NaN-box)
    #[inline]
    pub const fn u64(u: u64) -> Self {
        // Store only lower 48 bits
        Value(Self::NAN_BOX_BASE | Self::TAG_U64 | (u & Self::PAYLOAD_MASK))
    }

    // Raya language type aliases

    /// Create an integer value (alias for i32)
    #[inline]
    pub const fn integer(i: i32) -> Self {
        Self::i32(i)
    }

    /// Create a float value (alias for f64)
    #[inline]
    pub fn float(f: f64) -> Self {
        Self::f64(f)
    }

    /// Create a pointer value (for heap-allocated objects)
    ///
    /// # Safety
    ///
    /// The pointer must be:
    /// - Valid for the lifetime of this Value
    /// - Managed by the GC
    /// - Fits in 48 bits (must be < 2^48)
    #[inline]
    pub unsafe fn from_ptr<T>(ptr: NonNull<T>) -> Self {
        let addr = ptr.as_ptr() as usize as u64;
        debug_assert_eq!(addr & !Self::PAYLOAD_MASK, 0, "Pointer must fit in 48 bits");
        Value(Self::NAN_BOX_BASE | Self::TAG_PTR | (addr & Self::PAYLOAD_MASK))
    }

    /// Check if this value is null
    #[inline]
    pub const fn is_null(&self) -> bool {
        self.0 == Self::NULL
    }

    /// Check if this value is a boolean
    #[inline]
    pub const fn is_bool(&self) -> bool {
        self.is_nan_boxed() && self.get_tag() == (Self::TAG_BOOL >> Self::TAG_SHIFT)
    }

    /// Check if this value is an i32
    #[inline]
    pub const fn is_i32(&self) -> bool {
        self.is_nan_boxed() && self.get_tag() == (Self::TAG_I32 >> Self::TAG_SHIFT)
    }

    /// Check if this value is an f64
    #[inline]
    pub const fn is_f64(&self) -> bool {
        // f64 values are NOT NaN-boxed (they're direct IEEE 754)
        !self.is_nan_boxed()
    }

    /// Check if this value is a u32
    #[inline]
    pub const fn is_u32(&self) -> bool {
        self.is_nan_boxed() && self.get_tag() == (Self::TAG_U32 >> Self::TAG_SHIFT)
    }

    /// Check if this value is an f32
    #[inline]
    pub const fn is_f32(&self) -> bool {
        self.is_nan_boxed() && self.get_tag() == (Self::TAG_F32 >> Self::TAG_SHIFT)
    }

    /// Check if this value is an i64
    #[inline]
    pub const fn is_i64(&self) -> bool {
        self.is_nan_boxed() && self.get_tag() == (Self::TAG_I64 >> Self::TAG_SHIFT)
    }

    /// Check if this value is a u64
    #[inline]
    pub const fn is_u64(&self) -> bool {
        self.is_nan_boxed() && self.get_tag() == (Self::TAG_U64 >> Self::TAG_SHIFT)
    }

    /// Check if this value is a heap pointer
    #[inline]
    pub const fn is_ptr(&self) -> bool {
        self.is_nan_boxed() && self.get_tag() == (Self::TAG_PTR >> Self::TAG_SHIFT)
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
            Some((self.0 & Self::PAYLOAD_MASK) != 0)
        } else {
            None
        }
    }

    /// Extract i32 value
    #[inline]
    pub const fn as_i32(&self) -> Option<i32> {
        if self.is_i32() {
            // Extract payload and sign-extend from 48 bits to 32 bits
            let payload = (self.0 & Self::PAYLOAD_MASK) as i64;
            // Check if sign bit (bit 47) is set
            let sign_extended = if payload & 0x8000_0000_0000 != 0 {
                // Negative: sign-extend from 48 bits
                payload as i32
            } else {
                // Positive
                payload as i32
            };
            Some(sign_extended)
        } else {
            None
        }
    }

    /// Extract f64 value
    #[inline]
    pub fn as_f64(&self) -> Option<f64> {
        if self.is_f64() {
            Some(f64::from_bits(self.0))
        } else {
            None
        }
    }

    /// Extract u32 value
    #[inline]
    pub const fn as_u32(&self) -> Option<u32> {
        if self.is_u32() {
            Some((self.0 & Self::PAYLOAD_MASK_32) as u32)
        } else {
            None
        }
    }

    /// Extract f32 value
    #[inline]
    pub fn as_f32(&self) -> Option<f32> {
        if self.is_f32() {
            let bits = (self.0 & Self::PAYLOAD_MASK_32) as u32;
            Some(f32::from_bits(bits))
        } else {
            None
        }
    }

    /// Extract i64 value (limited to i32 range due to NaN-boxing)
    #[inline]
    pub const fn as_i64(&self) -> Option<i64> {
        if self.is_i64() {
            // Extract as i32 and convert to i64
            let value = (self.0 & Self::PAYLOAD_MASK_32) as i32;
            Some(value as i64)
        } else {
            None
        }
    }

    /// Extract u64 value (limited to 48-bit range)
    #[inline]
    pub const fn as_u64(&self) -> Option<u64> {
        if self.is_u64() {
            Some(self.0 & Self::PAYLOAD_MASK)
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
            let addr = (self.0 & Self::PAYLOAD_MASK) as usize;
            Some(NonNull::new_unchecked(addr as *mut T))
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
        if self.is_nan_boxed() {
            self.get_tag()
        } else {
            // Not NaN-boxed, it's an f64
            0xFF // Special marker for f64
        }
    }

    /// Check if value is truthy (for conditionals)
    pub fn is_truthy(&self) -> bool {
        if let Some(b) = self.as_bool() {
            b
        } else if self.is_null() {
            false
        } else if let Some(i) = self.as_i32() {
            i != 0
        } else if let Some(f) = self.as_f64() {
            f != 0.0 && !f.is_nan()
        } else if let Some(u) = self.as_u32() {
            u != 0
        } else if let Some(f) = self.as_f32() {
            f != 0.0 && !f.is_nan()
        } else if let Some(i) = self.as_i64() {
            i != 0
        } else if let Some(u) = self.as_u64() {
            u != 0
        } else {
            // Pointers are always truthy
            true
        }
    }

    /// Get type name for debugging
    pub const fn type_name(&self) -> &'static str {
        if !self.is_nan_boxed() {
            "float" // f64
        } else {
            match self.get_tag() {
                0 => "pointer",
                1 => "int", // i32
                2 => "bool",
                3 => "u32",
                4 => "f32",
                5 => "i64",
                6 => "null",
                7 => "u64",
                _ => "unknown",
            }
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_null() {
            write!(f, "null")
        } else if self.is_bool() {
            write!(f, "bool({})", self.as_bool().unwrap())
        } else if self.is_i32() {
            write!(f, "int({})", self.as_i32().unwrap())
        } else if self.is_f64() {
            write!(f, "float({})", self.as_f64().unwrap())
        } else if self.is_u32() {
            write!(f, "u32({})", self.as_u32().unwrap())
        } else if self.is_f32() {
            write!(f, "f32({})", self.as_f32().unwrap())
        } else if self.is_i64() {
            write!(f, "i64({})", self.as_i64().unwrap())
        } else if self.is_u64() {
            write!(f, "u64({})", self.as_u64().unwrap())
        } else if self.is_ptr() {
            write!(f, "ptr({:#x})", self.0 & Self::PAYLOAD_MASK)
        } else {
            write!(f, "Value({:#x})", self.0)
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_null() {
            write!(f, "null")
        } else if self.is_bool() {
            write!(f, "{}", self.as_bool().unwrap())
        } else if self.is_i32() {
            write!(f, "{}", self.as_i32().unwrap())
        } else if self.is_f64() {
            write!(f, "{}", self.as_f64().unwrap())
        } else if self.is_u32() {
            write!(f, "{}", self.as_u32().unwrap())
        } else if self.is_f32() {
            write!(f, "{}", self.as_f32().unwrap())
        } else if self.is_i64() {
            write!(f, "{}", self.as_i64().unwrap())
        } else if self.is_u64() {
            write!(f, "{}", self.as_u64().unwrap())
        } else if self.is_ptr() {
            write!(f, "[object@{:#x}]", self.0 & Self::PAYLOAD_MASK)
        } else {
            write!(f, "<??>")
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
        assert_eq!(v.type_name(), "int");

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
        assert_eq!(Value::null().tag(), (Value::TAG_NULL >> Value::TAG_SHIFT));
        assert_eq!(
            Value::bool(true).tag(),
            (Value::TAG_BOOL >> Value::TAG_SHIFT)
        );
        assert_eq!(Value::i32(42).tag(), (Value::TAG_I32 >> Value::TAG_SHIFT));
        assert_eq!(Value::f64(3.14).tag(), 0xFF); // Special marker for f64
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
        assert_eq!(format!("{:?}", Value::i32(42)), "int(42)");
        assert_eq!(format!("{:?}", Value::f64(3.14)), "float(3.14)");
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
        unsafe {
            drop(Box::from_raw(extracted.as_ptr()));
        }
    }
}
