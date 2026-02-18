//! NativeValue — NaN-boxed u64 value representation
//!
//! Uses the **same encoding** as the engine's internal `Value(u64)` for
//! zero-cost conversion at the ABI boundary. No tag decoding, no branching,
//! no data copying — just reinterpret the u64 bits.
//!
//! # Encoding
//!
//! ```text
//! f64 (float): Any value where upper 13 bits != 0x1FFF (raw IEEE 754)
//! Tagged:      0xFFF8 + 3-bit tag + 48-bit payload (NaN-boxed)
//!   - Pointer:   0xFFF8000000000000 | (ptr & 0xFFFFFFFFFFFF)    [tag=000]
//!   - i32 (int): 0xFFF8001000000000 | (i32 as u64)              [tag=001]
//!   - bool:      0xFFF8002000000000 | (b as u64)                [tag=010]
//!   - u32:       0xFFF8003000000000 | (u32 as u64)              [tag=011]
//!   - f32:       0xFFF8004000000000 | (f32.to_bits() as u64)    [tag=100]
//!   - i64:       0xFFF8005000000000 | (i64 as u64 & 0xFFFFFFFF) [tag=101]
//!   - null:      0xFFF8006000000000                             [tag=110]
//!   - u64:       0xFFF8007000000000 | (u64 & 0xFFFFFFFFFFFF)    [tag=111]
//! ```

use std::ptr::NonNull;

/// NaN-boxed 64-bit value — identical bit layout to the engine's `Value`.
///
/// Conversion between `NativeValue` and the engine `Value` is zero-cost
/// (just reinterpret the u64 bits via `from_bits`/`to_bits`).
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct NativeValue(u64);

// NaN-boxing constants (same as engine Value)
const NAN_BOX_BASE: u64 = 0xFFF8_0000_0000_0000;
const TAG_SHIFT: u64 = 48;
const TAG_MASK: u64 = 0x7 << TAG_SHIFT;
const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const PAYLOAD_MASK_32: u64 = 0x0000_0000_FFFF_FFFF;

const TAG_PTR: u64 = 0x0 << TAG_SHIFT;
const TAG_I32: u64 = 0x1 << TAG_SHIFT;
const TAG_BOOL: u64 = 0x2 << TAG_SHIFT;
const TAG_U32: u64 = 0x3 << TAG_SHIFT;
const TAG_F32: u64 = 0x4 << TAG_SHIFT;
const TAG_I64: u64 = 0x5 << TAG_SHIFT;
const TAG_NULL: u64 = 0x6 << TAG_SHIFT;
const TAG_U64: u64 = 0x7 << TAG_SHIFT;

const NULL_BITS: u64 = NAN_BOX_BASE | TAG_NULL;
const TRUE_BITS: u64 = NAN_BOX_BASE | TAG_BOOL | 1;
const FALSE_BITS: u64 = NAN_BOX_BASE | TAG_BOOL;

unsafe impl Send for NativeValue {}
unsafe impl Sync for NativeValue {}

impl NativeValue {
    // ========================================================================
    // Zero-cost conversion with engine Value
    // ========================================================================

    /// Create from raw u64 bits (same encoding as engine Value)
    #[inline(always)]
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// Get raw u64 bits (same encoding as engine Value)
    #[inline(always)]
    pub const fn to_bits(self) -> u64 {
        self.0
    }

    // ========================================================================
    // Constructors
    // ========================================================================

    /// Create a null value
    #[inline]
    pub const fn null() -> Self {
        Self(NULL_BITS)
    }

    /// Create a boolean value
    #[inline]
    pub const fn bool(b: bool) -> Self {
        Self(if b { TRUE_BITS } else { FALSE_BITS })
    }

    /// Create an i32 value
    #[inline]
    pub const fn i32(i: i32) -> Self {
        Self(NAN_BOX_BASE | TAG_I32 | ((i as i64) as u64 & PAYLOAD_MASK))
    }

    /// Create an f64 value (stored as raw IEEE 754 double — NOT NaN-boxed)
    #[inline]
    pub fn f64(f: f64) -> Self {
        Self(f.to_bits())
    }

    /// Create a u32 value
    #[inline]
    pub const fn u32(u: u32) -> Self {
        Self(NAN_BOX_BASE | TAG_U32 | (u as u64))
    }

    /// Create an f32 value
    #[inline]
    pub fn f32(f: f32) -> Self {
        Self(NAN_BOX_BASE | TAG_F32 | (f.to_bits() as u64))
    }

    /// Create an i64 value (limited to 32-bit range in NaN-box)
    #[inline]
    pub const fn i64(i: i64) -> Self {
        Self(NAN_BOX_BASE | TAG_I64 | ((i as i32) as u32 as u64))
    }

    /// Create a u64 value (limited to 48-bit range)
    #[inline]
    pub const fn u64(u: u64) -> Self {
        Self(NAN_BOX_BASE | TAG_U64 | (u & PAYLOAD_MASK))
    }

    /// Create from an opaque pointer
    ///
    /// # Safety
    /// The pointer must be valid and managed by the VM's GC.
    #[inline]
    pub unsafe fn from_ptr<T>(ptr: NonNull<T>) -> Self {
        let addr = ptr.as_ptr() as usize as u64;
        Self(NAN_BOX_BASE | TAG_PTR | (addr & PAYLOAD_MASK))
    }

    // ========================================================================
    // Type checks
    // ========================================================================

    #[inline]
    const fn is_nan_boxed(&self) -> bool {
        (self.0 & NAN_BOX_BASE) == NAN_BOX_BASE
    }

    #[inline]
    const fn get_tag(&self) -> u64 {
        (self.0 & TAG_MASK) >> TAG_SHIFT
    }

    /// Check if value is null
    #[inline]
    pub const fn is_null(&self) -> bool {
        self.0 == NULL_BITS
    }

    /// Check if value is a boolean
    #[inline]
    pub const fn is_bool(&self) -> bool {
        self.is_nan_boxed() && self.get_tag() == 2
    }

    /// Check if value is an i32
    #[inline]
    pub const fn is_i32(&self) -> bool {
        self.is_nan_boxed() && self.get_tag() == 1
    }

    /// Check if value is an f64 (raw IEEE 754 — not NaN-boxed)
    #[inline]
    pub const fn is_f64(&self) -> bool {
        !self.is_nan_boxed()
    }

    /// Check if value is a u32
    #[inline]
    pub const fn is_u32(&self) -> bool {
        self.is_nan_boxed() && self.get_tag() == 3
    }

    /// Check if value is an f32
    #[inline]
    pub const fn is_f32(&self) -> bool {
        self.is_nan_boxed() && self.get_tag() == 4
    }

    /// Check if value is an i64
    #[inline]
    pub const fn is_i64(&self) -> bool {
        self.is_nan_boxed() && self.get_tag() == 5
    }

    /// Check if value is a u64
    #[inline]
    pub const fn is_u64(&self) -> bool {
        self.is_nan_boxed() && self.get_tag() == 7
    }

    /// Check if value is a heap pointer (string, buffer, array, object, etc.)
    #[inline]
    pub const fn is_ptr(&self) -> bool {
        self.is_nan_boxed() && self.get_tag() == 0
    }

    // ========================================================================
    // Extractors
    // ========================================================================

    /// Extract boolean value
    #[inline]
    pub const fn as_bool(&self) -> Option<bool> {
        if self.is_bool() {
            Some((self.0 & PAYLOAD_MASK) != 0)
        } else {
            None
        }
    }

    /// Extract i32 value
    #[inline]
    pub const fn as_i32(&self) -> Option<i32> {
        if self.is_i32() {
            let payload = (self.0 & PAYLOAD_MASK) as i64;
            Some(payload as i32)
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
            Some((self.0 & PAYLOAD_MASK_32) as u32)
        } else {
            None
        }
    }

    /// Extract f32 value
    #[inline]
    pub fn as_f32(&self) -> Option<f32> {
        if self.is_f32() {
            let bits = (self.0 & PAYLOAD_MASK_32) as u32;
            Some(f32::from_bits(bits))
        } else {
            None
        }
    }

    /// Extract i64 value
    #[inline]
    pub const fn as_i64(&self) -> Option<i64> {
        if self.is_i64() {
            let value = (self.0 & PAYLOAD_MASK_32) as i32;
            Some(value as i64)
        } else {
            None
        }
    }

    /// Extract u64 value
    #[inline]
    pub const fn as_u64(&self) -> Option<u64> {
        if self.is_u64() {
            Some(self.0 & PAYLOAD_MASK)
        } else {
            None
        }
    }

    /// Extract pointer value
    ///
    /// # Safety
    /// The pointer must still be valid (not freed by GC) and T must match.
    #[inline]
    pub unsafe fn as_ptr<T>(&self) -> Option<NonNull<T>> {
        if self.is_ptr() {
            let addr = (self.0 & PAYLOAD_MASK) as usize;
            Some(NonNull::new_unchecked(addr as *mut T))
        } else {
            None
        }
    }

    /// Get type name for debugging
    pub const fn type_name(&self) -> &'static str {
        if !self.is_nan_boxed() {
            "float"
        } else {
            match self.get_tag() {
                0 => "pointer",
                1 => "int",
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

impl Default for NativeValue {
    fn default() -> Self {
        Self::null()
    }
}

impl std::fmt::Debug for NativeValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.is_nan_boxed() {
            write!(f, "NativeValue::F64({})", f64::from_bits(self.0))
        } else {
            match self.get_tag() {
                0 => write!(f, "NativeValue::Ptr({:#x})", self.0 & PAYLOAD_MASK),
                1 => write!(f, "NativeValue::I32({})", (self.0 & PAYLOAD_MASK) as i64 as i32),
                2 => write!(f, "NativeValue::Bool({})", (self.0 & PAYLOAD_MASK) != 0),
                3 => write!(f, "NativeValue::U32({})", (self.0 & PAYLOAD_MASK_32) as u32),
                4 => write!(f, "NativeValue::F32({})", f32::from_bits((self.0 & PAYLOAD_MASK_32) as u32)),
                5 => write!(f, "NativeValue::I64({})", (self.0 & PAYLOAD_MASK_32) as i32 as i64),
                6 => write!(f, "NativeValue::Null"),
                7 => write!(f, "NativeValue::U64({})", self.0 & PAYLOAD_MASK),
                _ => write!(f, "NativeValue::Unknown({:#x})", self.0),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_null() {
        let v = NativeValue::null();
        assert!(v.is_null());
        assert!(!v.is_i32());
        assert!(!v.is_f64());
        assert!(!v.is_ptr());
    }

    #[test]
    fn test_bool() {
        let t = NativeValue::bool(true);
        let f = NativeValue::bool(false);
        assert_eq!(t.as_bool(), Some(true));
        assert_eq!(f.as_bool(), Some(false));
        assert!(t.is_bool());
        assert!(f.is_bool());
        assert!(!t.is_null());
    }

    #[test]
    fn test_i32() {
        let v = NativeValue::i32(42);
        assert_eq!(v.as_i32(), Some(42));
        assert!(v.is_i32());

        let neg = NativeValue::i32(-100);
        assert_eq!(neg.as_i32(), Some(-100));
    }

    #[test]
    fn test_f64() {
        let v = NativeValue::f64(3.14159);
        assert!((v.as_f64().unwrap() - 3.14159).abs() < 1e-10);
        assert!(v.is_f64());
        assert!(!v.is_nan_boxed());
    }

    #[test]
    fn test_i64() {
        let v = NativeValue::i64(999);
        assert_eq!(v.as_i64(), Some(999));
        assert!(v.is_i64());
    }

    #[test]
    fn test_u32() {
        let v = NativeValue::u32(12345);
        assert_eq!(v.as_u32(), Some(12345));
        assert!(v.is_u32());
    }

    #[test]
    fn test_from_bits_roundtrip() {
        let original = NativeValue::i32(42);
        let bits = original.to_bits();
        let restored = NativeValue::from_bits(bits);
        assert_eq!(original, restored);
        assert_eq!(restored.as_i32(), Some(42));
    }

    #[test]
    fn test_f64_from_bits_roundtrip() {
        let original = NativeValue::f64(2.71828);
        let bits = original.to_bits();
        let restored = NativeValue::from_bits(bits);
        assert_eq!(original, restored);
        assert!((restored.as_f64().unwrap() - 2.71828).abs() < 1e-10);
    }

    #[test]
    fn test_type_discrimination() {
        let null = NativeValue::null();
        let b = NativeValue::bool(true);
        let i = NativeValue::i32(1);
        let f = NativeValue::f64(1.0);

        assert!(null.is_null());
        assert!(!null.is_bool());

        assert!(b.is_bool());
        assert!(!b.is_i32());

        assert!(i.is_i32());
        assert!(!i.is_f64());

        assert!(f.is_f64());
        assert!(!f.is_i32());
    }

    #[test]
    fn test_debug_format() {
        let v = NativeValue::i32(42);
        let s = format!("{:?}", v);
        assert!(s.contains("42"));
    }
}
