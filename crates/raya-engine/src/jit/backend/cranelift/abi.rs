//! NaN-boxing ABI helpers for Cranelift IR generation
//!
//! Provides inline NaN-boxing/unboxing operations as Cranelift IR instructions.
//! These mirror the encoding in `crate::vm::value::Value`.

use cranelift_codegen::ir::{self, InstBuilder};
use cranelift_frontend::FunctionBuilder;

// NaN-boxing constants (from raya-engine value.rs)
// Layout: 0xFFF8_0000_0000_0000 | (tag << 48) | payload
pub const NAN_BOX_BASE: u64 = 0xFFF8_0000_0000_0000;
pub const TAG_SHIFT: u64 = 48;
pub const TAG_PTR: u64 = 0x0 << TAG_SHIFT;
pub const TAG_I32: u64 = 0x1 << TAG_SHIFT;
pub const TAG_BOOL: u64 = 0x2 << TAG_SHIFT;
pub const TAG_NULL: u64 = 0x6 << TAG_SHIFT;
pub const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
pub const PAYLOAD_MASK_32: u64 = 0x0000_0000_FFFF_FFFF;

// Pre-computed tagged bases
pub const I32_TAG_BASE: u64 = NAN_BOX_BASE | TAG_I32;
pub const BOOL_TAG_BASE: u64 = NAN_BOX_BASE | TAG_BOOL;
pub const NULL_VALUE: u64 = NAN_BOX_BASE | TAG_NULL;
pub const TRUE_VALUE: u64 = NAN_BOX_BASE | TAG_BOOL | 1;
pub const FALSE_VALUE: u64 = NAN_BOX_BASE | TAG_BOOL;

/// Box an i32 value into a NaN-boxed u64.
///
/// Cranelift IR equivalent of: `NAN_BOX_BASE | TAG_I32 | ((i32 as i64) as u64 & PAYLOAD_MASK)`
pub fn emit_box_i32(builder: &mut FunctionBuilder<'_>, val: ir::Value) -> ir::Value {
    let i64_type = ir::types::I64;

    // Sign-extend i32 to i64
    let extended = builder.ins().sextend(i64_type, val);
    // Mask to payload (48 bits)
    let mask = builder.ins().iconst(i64_type, PAYLOAD_MASK as i64);
    let payload = builder.ins().band(extended, mask);
    // OR with tag base
    let tag_base = builder.ins().iconst(i64_type, I32_TAG_BASE as i64);
    builder.ins().bor(tag_base, payload)
}

/// Unbox an i32 from a NaN-boxed u64.
///
/// Cranelift IR equivalent of: `(val & PAYLOAD_MASK_32) as i32`
pub fn emit_unbox_i32(builder: &mut FunctionBuilder<'_>, val: ir::Value) -> ir::Value {
    let i64_type = ir::types::I64;
    let i32_type = ir::types::I32;

    // Mask to 32-bit payload
    let mask = builder.ins().iconst(i64_type, PAYLOAD_MASK_32 as i64);
    let payload = builder.ins().band(val, mask);
    // Truncate to i32
    builder.ins().ireduce(i32_type, payload)
}

/// Box an f64 value into a NaN-boxed u64.
///
/// f64 values are stored as raw bits. If the bits collide with our NaN-box base
/// (negative quiet NaN), we canonicalize to positive quiet NaN.
pub fn emit_box_f64(builder: &mut FunctionBuilder<'_>, val: ir::Value) -> ir::Value {
    let i64_type = ir::types::I64;

    // Bitcast f64 to i64
    let bits = builder.ins().bitcast(i64_type, ir::MemFlags::new(), val);

    // Check for NaN-box collision: (bits & 0xFFF8...) == NAN_BOX_BASE
    let nan_base = builder.ins().iconst(i64_type, NAN_BOX_BASE as i64);
    let masked = builder.ins().band(bits, nan_base);
    let is_collision = builder.ins().icmp(ir::condcodes::IntCC::Equal, masked, nan_base);

    // Canonical positive QNaN
    let canonical_nan = builder.ins().iconst(i64_type, 0x7FF8_0000_0000_0000u64 as i64);

    // Select: if collision, use canonical NaN; otherwise, use raw bits
    builder.ins().select(is_collision, canonical_nan, bits)
}

/// Unbox an f64 from a NaN-boxed u64.
///
/// f64 values are stored directly as bits (not tagged), so just bitcast.
pub fn emit_unbox_f64(builder: &mut FunctionBuilder<'_>, val: ir::Value) -> ir::Value {
    let f64_type = ir::types::F64;
    builder.ins().bitcast(f64_type, ir::MemFlags::new(), val)
}

/// Box a boolean into a NaN-boxed u64.
///
/// Cranelift IR equivalent of: `NAN_BOX_BASE | TAG_BOOL | (b as u64)`
pub fn emit_box_bool(builder: &mut FunctionBuilder<'_>, val: ir::Value) -> ir::Value {
    let i64_type = ir::types::I64;

    // Zero-extend i8 bool to i64
    let extended = builder.ins().uextend(i64_type, val);
    // OR with bool tag base
    let tag_base = builder.ins().iconst(i64_type, BOOL_TAG_BASE as i64);
    builder.ins().bor(tag_base, extended)
}

/// Unbox a boolean from a NaN-boxed u64.
///
/// Cranelift IR equivalent of: `(val & 1) != 0`
pub fn emit_unbox_bool(builder: &mut FunctionBuilder<'_>, val: ir::Value) -> ir::Value {
    let i64_type = ir::types::I64;
    let i8_type = ir::types::I8;

    let one = builder.ins().iconst(i64_type, 1);
    let bit = builder.ins().band(val, one);
    builder.ins().ireduce(i8_type, bit)
}

/// Emit a null constant (NaN-boxed).
pub fn emit_null(builder: &mut FunctionBuilder<'_>) -> ir::Value {
    builder.ins().iconst(ir::types::I64, NULL_VALUE as i64)
}

/// Emit a true constant (NaN-boxed).
pub fn emit_true(builder: &mut FunctionBuilder<'_>) -> ir::Value {
    builder.ins().iconst(ir::types::I64, TRUE_VALUE as i64)
}

/// Emit a false constant (NaN-boxed).
pub fn emit_false(builder: &mut FunctionBuilder<'_>) -> ir::Value {
    builder.ins().iconst(ir::types::I64, FALSE_VALUE as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nan_boxing_constants_match_engine() {
        // Verify our constants match the engine's Value encoding
        // NAN_BOX_BASE = 0xFFF8..., tags are at bits 48-50
        assert_eq!(NAN_BOX_BASE, 0xFFF8_0000_0000_0000);
        assert_eq!(I32_TAG_BASE, 0xFFF9_0000_0000_0000);  // base | (1 << 48)
        assert_eq!(BOOL_TAG_BASE, 0xFFFA_0000_0000_0000);  // base | (2 << 48)
        assert_eq!(NULL_VALUE, 0xFFFE_0000_0000_0000);     // base | (6 << 48)
        assert_eq!(TRUE_VALUE, 0xFFFA_0000_0000_0001);     // bool_base | 1
        assert_eq!(FALSE_VALUE, 0xFFFA_0000_0000_0000);    // bool_base | 0
    }

    #[test]
    fn test_i32_encoding_matches() {
        // Value::i32(42) = NAN_BOX_BASE | TAG_I32 | (42 as u64 & PAYLOAD_MASK)
        let expected = NAN_BOX_BASE | TAG_I32 | (42u64 & PAYLOAD_MASK);
        assert_eq!(expected, I32_TAG_BASE | 42);
    }

    #[test]
    fn test_negative_i32_encoding() {
        // Value::i32(-1) = NAN_BOX_BASE | TAG_I32 | ((-1i64 as u64) & PAYLOAD_MASK)
        let expected = NAN_BOX_BASE | TAG_I32 | ((-1i64 as u64) & PAYLOAD_MASK);
        assert_eq!(expected & PAYLOAD_MASK, 0x0000_FFFF_FFFF_FFFF);
    }
}
