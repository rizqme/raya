//! NaN-boxing ABI helpers for AOT Cranelift IR generation
//!
//! Provides inline NaN-boxing/unboxing operations as Cranelift IR instructions.
//! These mirror the encoding in `crate::vm::value::Value`.
//!
//! Self-contained — does not depend on the JIT feature flag.

use cranelift_codegen::ir::{self, InstBuilder};
use cranelift_frontend::FunctionBuilder;

// ============================================================================
// NaN-boxing constants (from raya-engine value.rs)
// Layout: 0xFFF8_0000_0000_0000 | (tag << 48) | payload
// ============================================================================

/// Base of the NaN-box encoding (negative quiet NaN)
pub const NAN_BOX_BASE: u64 = 0xFFF8_0000_0000_0000;
/// Bit shift for the 3-bit tag field
pub const TAG_SHIFT: u64 = 48;
/// Tag for heap pointers (objects, arrays, strings)
pub const TAG_PTR: u64 = 0x0 << TAG_SHIFT;
/// Tag for i32 values
pub const TAG_I32: u64 = 0x1 << TAG_SHIFT;
/// Tag for boolean values
pub const TAG_BOOL: u64 = 0x2 << TAG_SHIFT;
/// Tag for null
pub const TAG_NULL: u64 = 0x6 << TAG_SHIFT;
/// 48-bit payload mask
pub const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
/// 32-bit payload mask (for i32)
pub const PAYLOAD_MASK_32: u64 = 0x0000_0000_FFFF_FFFF;

// Pre-computed tagged bases
/// Base for i32 NaN-boxed values
pub const I32_TAG_BASE: u64 = NAN_BOX_BASE | TAG_I32;
/// Base for boolean NaN-boxed values
pub const BOOL_TAG_BASE: u64 = NAN_BOX_BASE | TAG_BOOL;
/// NaN-boxed null value
pub const NULL_VALUE: u64 = NAN_BOX_BASE | TAG_NULL;
/// NaN-boxed true value
pub const TRUE_VALUE: u64 = NAN_BOX_BASE | TAG_BOOL | 1;
/// NaN-boxed false value
pub const FALSE_VALUE: u64 = NAN_BOX_BASE | TAG_BOOL;

// ============================================================================
// Cranelift IR emit helpers
// ============================================================================

/// Box an i32 value into a NaN-boxed u64.
pub fn emit_box_i32(builder: &mut FunctionBuilder<'_>, val: ir::Value) -> ir::Value {
    let i64_type = ir::types::I64;
    let extended = builder.ins().sextend(i64_type, val);
    let mask = builder.ins().iconst(i64_type, PAYLOAD_MASK as i64);
    let payload = builder.ins().band(extended, mask);
    let tag_base = builder.ins().iconst(i64_type, I32_TAG_BASE as i64);
    builder.ins().bor(tag_base, payload)
}

/// Unbox an i32 from a NaN-boxed u64.
pub fn emit_unbox_i32(builder: &mut FunctionBuilder<'_>, val: ir::Value) -> ir::Value {
    let i64_type = ir::types::I64;
    let i32_type = ir::types::I32;
    let mask = builder.ins().iconst(i64_type, PAYLOAD_MASK_32 as i64);
    let payload = builder.ins().band(val, mask);
    builder.ins().ireduce(i32_type, payload)
}

/// Box an f64 value into a NaN-boxed u64.
pub fn emit_box_f64(builder: &mut FunctionBuilder<'_>, val: ir::Value) -> ir::Value {
    let i64_type = ir::types::I64;
    let bits = builder.ins().bitcast(i64_type, ir::MemFlags::new(), val);
    let nan_base = builder.ins().iconst(i64_type, NAN_BOX_BASE as i64);
    let masked = builder.ins().band(bits, nan_base);
    let is_collision = builder.ins().icmp(ir::condcodes::IntCC::Equal, masked, nan_base);
    let canonical_nan = builder.ins().iconst(i64_type, 0x7FF8_0000_0000_0000u64 as i64);
    builder.ins().select(is_collision, canonical_nan, bits)
}

/// Unbox an f64 from a NaN-boxed u64.
pub fn emit_unbox_f64(builder: &mut FunctionBuilder<'_>, val: ir::Value) -> ir::Value {
    let f64_type = ir::types::F64;
    builder.ins().bitcast(f64_type, ir::MemFlags::new(), val)
}

/// Box a boolean into a NaN-boxed u64.
pub fn emit_box_bool(builder: &mut FunctionBuilder<'_>, val: ir::Value) -> ir::Value {
    let i64_type = ir::types::I64;
    let extended = builder.ins().uextend(i64_type, val);
    let tag_base = builder.ins().iconst(i64_type, BOOL_TAG_BASE as i64);
    builder.ins().bor(tag_base, extended)
}

/// Unbox a boolean from a NaN-boxed u64.
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

/// Emit the AOT_SUSPEND sentinel constant.
pub fn emit_aot_suspend(builder: &mut FunctionBuilder<'_>) -> ir::Value {
    builder.ins().iconst(ir::types::I64, super::frame::AOT_SUSPEND as i64)
}

/// Check if a value equals AOT_SUSPEND: (val) -> i8 (0 or 1)
pub fn emit_is_suspend(builder: &mut FunctionBuilder<'_>, val: ir::Value) -> ir::Value {
    let suspend = emit_aot_suspend(builder);
    builder.ins().icmp(ir::condcodes::IntCC::Equal, val, suspend)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nan_boxing_constants() {
        assert_eq!(NAN_BOX_BASE, 0xFFF8_0000_0000_0000);
        assert_eq!(I32_TAG_BASE, 0xFFF9_0000_0000_0000);
        assert_eq!(BOOL_TAG_BASE, 0xFFFA_0000_0000_0000);
        assert_eq!(NULL_VALUE, 0xFFFE_0000_0000_0000);
        assert_eq!(TRUE_VALUE, 0xFFFA_0000_0000_0001);
        assert_eq!(FALSE_VALUE, 0xFFFA_0000_0000_0000);
    }
}
