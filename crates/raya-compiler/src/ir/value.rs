//! IR Values and Registers
//!
//! Defines the value types used in IR instructions.

use raya_parser::TypeId;

/// Virtual register identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegisterId(pub u32);

impl RegisterId {
    /// Create a new register ID
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the raw ID value
    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for RegisterId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "r{}", self.0)
    }
}

/// Register with type information
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Register {
    pub id: RegisterId,
    pub ty: TypeId,
}

impl Register {
    /// Create a new register
    pub fn new(id: RegisterId, ty: TypeId) -> Self {
        Self { id, ty }
    }
}

impl std::fmt::Display for Register {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.id, self.ty.as_u32())
    }
}

/// IR values (right-hand side of assignments)
#[derive(Debug, Clone)]
pub enum IrValue {
    /// A register reference
    Register(Register),
    /// A constant value
    Constant(IrConstant),
}

impl std::fmt::Display for IrValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IrValue::Register(reg) => write!(f, "{}", reg),
            IrValue::Constant(c) => write!(f, "{}", c),
        }
    }
}

/// Constant values in IR
#[derive(Debug, Clone, PartialEq)]
pub enum IrConstant {
    /// 32-bit integer
    I32(i32),
    /// 64-bit float
    F64(f64),
    /// String literal
    String(String),
    /// Boolean value
    Boolean(bool),
    /// Null value
    Null,
}

impl IrConstant {
    /// Check if this is a numeric constant
    pub fn is_numeric(&self) -> bool {
        matches!(self, IrConstant::I32(_) | IrConstant::F64(_))
    }

    /// Try to get as i32
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            IrConstant::I32(v) => Some(*v),
            _ => None,
        }
    }

    /// Try to get as f64
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            IrConstant::F64(v) => Some(*v),
            IrConstant::I32(v) => Some(*v as f64),
            _ => None,
        }
    }

    /// Try to get as bool
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            IrConstant::Boolean(v) => Some(*v),
            _ => None,
        }
    }
}

impl std::fmt::Display for IrConstant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IrConstant::I32(v) => write!(f, "{}", v),
            IrConstant::F64(v) => write!(f, "{:.6}", v),
            IrConstant::String(s) => write!(f, "\"{}\"", s.escape_default()),
            IrConstant::Boolean(b) => write!(f, "{}", b),
            IrConstant::Null => write!(f, "null"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_id_display() {
        let id = RegisterId(42);
        assert_eq!(format!("{}", id), "r42");
    }

    #[test]
    fn test_constant_display() {
        assert_eq!(format!("{}", IrConstant::I32(42)), "42");
        assert_eq!(format!("{}", IrConstant::Boolean(true)), "true");
        assert_eq!(format!("{}", IrConstant::Null), "null");
        assert_eq!(format!("{}", IrConstant::String("hello".to_string())), "\"hello\"");
    }

    #[test]
    fn test_constant_as_i32() {
        assert_eq!(IrConstant::I32(42).as_i32(), Some(42));
        assert_eq!(IrConstant::F64(3.14).as_i32(), None);
    }

    #[test]
    fn test_constant_as_f64() {
        assert_eq!(IrConstant::F64(3.14).as_f64(), Some(3.14));
        assert_eq!(IrConstant::I32(42).as_f64(), Some(42.0));
    }
}
