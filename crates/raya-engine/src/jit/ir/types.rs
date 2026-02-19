//! JIT type system
//!
//! Types for the JIT IR. Since Raya uses typed opcodes (Iadd vs Fadd),
//! the JIT can infer concrete types from bytecode — enabling unboxed
//! operations in native code with box/unbox only at boundaries.

/// JIT IR type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JitType {
    /// NaN-boxed u64 (unknown/polymorphic)
    Value,
    /// Known i32 (from Iadd, ConstI32, etc.)
    I32,
    /// Known f64 (from Fadd, ConstF64, etc.)
    F64,
    /// Known boolean
    Bool,
    /// Known heap pointer (object, array, string, closure)
    Ptr,
    /// No value (void return)
    Void,
}

impl JitType {
    /// Whether this type is a known primitive (not Value)
    pub fn is_concrete(&self) -> bool {
        !matches!(self, JitType::Value)
    }

    /// Whether this type needs NaN-boxing to be stored in a Value slot
    pub fn needs_boxing(&self) -> bool {
        matches!(self, JitType::I32 | JitType::F64 | JitType::Bool | JitType::Ptr)
    }

    /// Whether this type is already a NaN-boxed value
    pub fn is_boxed(&self) -> bool {
        matches!(self, JitType::Value)
    }
}

impl std::fmt::Display for JitType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JitType::Value => write!(f, "val"),
            JitType::I32 => write!(f, "i32"),
            JitType::F64 => write!(f, "f64"),
            JitType::Bool => write!(f, "bool"),
            JitType::Ptr => write!(f, "ptr"),
            JitType::Void => write!(f, "void"),
        }
    }
}
