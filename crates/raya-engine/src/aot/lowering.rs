#![allow(missing_docs)]
//! State machine → Cranelift IR lowering
//!
//! Takes a `StateMachineFunction` (from the state machine transform) and
//! produces Cranelift IR that implements the resumable state machine.
//!
//! All runtime calls go through `AotHelperTable` (indirect calls through
//! the context pointer), so the generated code has zero relocations.

use std::collections::HashMap;

use cranelift_codegen::ir::{self, types, condcodes, AbiParam, InstBuilder, MemFlags, Value};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};
#[cfg(test)]
use cranelift_frontend::FunctionBuilderContext;

use super::abi;
use super::statemachine::{
    SmBlock, SmBlockId, SmCmpOp, SmF64BinOp, SmI32BinOp,
    SmInstr, SmTerminator, StateMachineFunction, HelperCall,
};

// =============================================================================
// Public API
// =============================================================================

/// The AOT entry point signature for all compiled functions.
///
/// ```text
/// fn(frame: *mut AotFrame, ctx: *mut AotTaskContext) -> u64
/// ```
pub fn aot_entry_signature(call_conv: CallConv) -> ir::Signature {
    let mut sig = ir::Signature::new(call_conv);
    sig.params.push(AbiParam::new(types::I64)); // frame: *mut AotFrame
    sig.params.push(AbiParam::new(types::I64)); // ctx: *mut AotTaskContext
    sig.returns.push(AbiParam::new(types::I64)); // return value (NaN-boxed or AOT_SUSPEND)
    sig
}

/// Errors that can occur during Cranelift lowering.
#[derive(Debug)]
pub enum LoweringError {
    /// Cranelift codegen error
    Codegen(String),

    /// Unsupported instruction
    Unsupported(String),

    /// Missing block reference
    MissingBlock(SmBlockId),
}

impl std::fmt::Display for LoweringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoweringError::Codegen(s) => write!(f, "Cranelift codegen error: {}", s),
            LoweringError::Unsupported(s) => write!(f, "Unsupported instruction: {}", s),
            LoweringError::MissingBlock(id) => write!(f, "Missing block: {:?}", id),
        }
    }
}

impl std::error::Error for LoweringError {}

/// Lower a state machine function to Cranelift IR.
///
/// The generated function:
/// 1. Reads `frame.resume_point` to dispatch to the right state
/// 2. Executes the function body (typed operations, helper calls)
/// 3. At suspension points: saves locals to frame, sets resume_point, returns AOT_SUSPEND
/// 4. On return: returns the NaN-boxed result value
pub fn lower_function(
    sm_func: &StateMachineFunction,
    mut builder: FunctionBuilder<'_>,
) -> Result<(), LoweringError> {
    let call_conv = builder.func.signature.call_conv;

    // 1. Create Cranelift blocks for each SmBlock
    let mut block_map = HashMap::new();
    for block in &sm_func.blocks {
        let cl_block = builder.create_block();
        block_map.insert(block.id, cl_block);
    }

    // Also create a trap block for unreachable/invalid dispatch
    let trap_block = builder.create_block();

    // 2. Determine register types from all blocks
    let reg_types = determine_reg_types(&sm_func.blocks);

    // 3. Declare variables for all registers
    let mut reg_vars = HashMap::new();
    for (&reg_id, &ty) in &reg_types {
        let var = builder.declare_var(ty);
        reg_vars.insert(reg_id, var);
    }

    // Declare special variables for frame_ptr and ctx_ptr
    let frame_var = builder.declare_var(types::I64);
    let ctx_var = builder.declare_var(types::I64);

    // 4. Set up entry block (first SmBlock gets function parameters)
    if sm_func.blocks.is_empty() {
        builder.seal_all_blocks();
        builder.finalize();
        return Ok(());
    }

    let entry_cl_block = block_map[&sm_func.blocks[0].id];
    builder.append_block_params_for_function_params(entry_cl_block);
    builder.switch_to_block(entry_cl_block);

    let frame_ptr = builder.block_params(entry_cl_block)[0];
    let ctx_ptr = builder.block_params(entry_cl_block)[1];
    builder.def_var(frame_var, frame_ptr);
    builder.def_var(ctx_var, ctx_ptr);

    // 5. Build phi resolution map
    let phi_copies = build_phi_copies(&sm_func.blocks);

    // 6. Create lowering context
    let entry_sm_block_id = sm_func.blocks[0].id;
    let mut ctx = LoweringCtx {
        block_map,
        trap_block,
        entry_sm_block_id,
        reg_vars,
        frame_var,
        ctx_var,
        call_conv,
        phi_copies,
    };

    // 7. Lower each block
    for (i, block) in sm_func.blocks.iter().enumerate() {
        if i > 0 {
            let cl_block = ctx.block_map[&block.id];
            builder.switch_to_block(cl_block);
        }
        ctx.lower_block(block, &mut builder)?;
    }

    // 8. Fill trap block
    builder.switch_to_block(trap_block);
    builder.ins().trap(ir::TrapCode::user(1).unwrap());

    // 9. Seal all blocks (deferred sealing for simplicity —
    //    state machine blocks can contain back-edges from original loops)
    builder.seal_all_blocks();

    // 10. Finalize (consumes the builder's mutable borrow)
    builder.finalize();
    Ok(())
}

// =============================================================================
// Register type determination
// =============================================================================

/// Pre-scan all blocks to determine the Cranelift type of each virtual register.
fn determine_reg_types(blocks: &[SmBlock]) -> HashMap<u32, ir::types::Type> {
    let mut types_map: HashMap<u32, ir::types::Type> = HashMap::new();

    for block in blocks {
        for instr in &block.instructions {
            match instr {
                // Constants
                SmInstr::ConstI32 { dest, .. } => { types_map.insert(*dest, types::I32); }
                SmInstr::ConstF64 { dest, .. } => { types_map.insert(*dest, types::F64); }
                SmInstr::ConstBool { dest, .. } => { types_map.insert(*dest, types::I8); }
                SmInstr::ConstNull { dest } => { types_map.insert(*dest, types::I64); }

                // Typed arithmetic
                SmInstr::I32BinOp { dest, .. } => { types_map.insert(*dest, types::I32); }
                SmInstr::I32Neg { dest, .. } => { types_map.insert(*dest, types::I32); }
                SmInstr::I32BitNot { dest, .. } => { types_map.insert(*dest, types::I32); }
                SmInstr::F64BinOp { dest, .. } => { types_map.insert(*dest, types::F64); }
                SmInstr::F64Neg { dest, .. } => { types_map.insert(*dest, types::F64); }

                // Comparisons → bool (I8)
                SmInstr::I32Cmp { dest, .. } => { types_map.insert(*dest, types::I8); }
                SmInstr::F64Cmp { dest, .. } => { types_map.insert(*dest, types::I8); }

                // Boolean logic
                SmInstr::BoolNot { dest, .. } => { types_map.insert(*dest, types::I8); }
                SmInstr::BoolAnd { dest, .. } => { types_map.insert(*dest, types::I8); }
                SmInstr::BoolOr { dest, .. } => { types_map.insert(*dest, types::I8); }

                // NaN-boxing conversions
                SmInstr::BoxI32 { dest, .. } => { types_map.insert(*dest, types::I64); }
                SmInstr::UnboxI32 { dest, .. } => { types_map.insert(*dest, types::I32); }
                SmInstr::BoxF64 { dest, .. } => { types_map.insert(*dest, types::I64); }
                SmInstr::UnboxF64 { dest, .. } => { types_map.insert(*dest, types::F64); }
                SmInstr::BoxBool { dest, .. } => { types_map.insert(*dest, types::I64); }
                SmInstr::UnboxBool { dest, .. } => { types_map.insert(*dest, types::I8); }

                // Frame / state access
                SmInstr::LoadLocal { dest, .. } => { types_map.insert(*dest, types::I64); }
                SmInstr::LoadResumePoint { dest } => { types_map.insert(*dest, types::I32); }
                SmInstr::LoadChildFrame { dest } => { types_map.insert(*dest, types::I64); }
                SmInstr::LoadResumeValue { dest } => { types_map.insert(*dest, types::I64); }

                // Memory access
                SmInstr::LoadGlobal { dest, .. } => { types_map.insert(*dest, types::I64); }

                // Helper calls (return NaN-boxed I64)
                SmInstr::CallHelper { dest: Some(d), .. } => { types_map.insert(*d, types::I64); }

                // AOT function calls (return NaN-boxed I64 or AOT_SUSPEND)
                SmInstr::CallAot { dest, .. } => { types_map.insert(*dest, types::I64); }

                // Suspend check → bool
                SmInstr::IsSuspend { dest, .. } => { types_map.insert(*dest, types::I8); }

                // Phi: type from first known source
                SmInstr::Phi { dest, sources } => {
                    if !types_map.contains_key(dest) {
                        for (_, src_reg) in sources {
                            if let Some(&ty) = types_map.get(src_reg) {
                                types_map.insert(*dest, ty);
                                break;
                            }
                        }
                        // Default to I64 if no source type known yet
                        types_map.entry(*dest).or_insert(types::I64);
                    }
                }

                // Move: same type as source
                SmInstr::Move { dest, src } => {
                    let ty = types_map.get(src).copied().unwrap_or(types::I64);
                    types_map.insert(*dest, ty);
                }

                // Instructions with no dest register
                SmInstr::StoreLocal { .. }
                | SmInstr::StoreResumePoint { .. }
                | SmInstr::StoreChildFrame { .. }
                | SmInstr::StoreSuspendReason { .. }
                | SmInstr::StoreSuspendPayload { .. }
                | SmInstr::StoreGlobal { .. }
                | SmInstr::CallHelper { dest: None, .. }
                | SmInstr::ReturnValue { .. } => {}
            }
        }
    }

    types_map
}

/// Build a Phi resolution map: for each predecessor block ID, collect
/// (phi_dest_reg, source_reg) pairs that need def_var before the terminator.
fn build_phi_copies(blocks: &[SmBlock]) -> HashMap<SmBlockId, Vec<(u32, u32)>> {
    let mut copies: HashMap<SmBlockId, Vec<(u32, u32)>> = HashMap::new();
    for block in blocks {
        for instr in &block.instructions {
            if let SmInstr::Phi { dest, sources } = instr {
                for (src_block, src_reg) in sources {
                    copies.entry(*src_block)
                        .or_default()
                        .push((*dest, *src_reg));
                }
            }
        }
    }
    copies
}

// =============================================================================
// Lowering context
// =============================================================================

struct LoweringCtx {
    block_map: HashMap<SmBlockId, ir::Block>,
    trap_block: ir::Block,
    entry_sm_block_id: SmBlockId,
    reg_vars: HashMap<u32, Variable>,
    frame_var: Variable,
    ctx_var: Variable,
    call_conv: CallConv,
    phi_copies: HashMap<SmBlockId, Vec<(u32, u32)>>,
}

impl LoweringCtx {
    // ---- Register access ----

    fn use_reg(&self, builder: &mut FunctionBuilder<'_>, reg: u32) -> Value {
        let var = self.reg_vars[&reg];
        builder.use_var(var)
    }

    fn def_reg(&self, builder: &mut FunctionBuilder<'_>, reg: u32, val: Value) {
        let var = self.reg_vars[&reg];
        builder.def_var(var, val);
    }

    /// Ensure a register variable exists, declaring it with a default type if needed.
    fn ensure_reg(&mut self, builder: &mut FunctionBuilder<'_>, reg: u32, ty: ir::types::Type) {
        self.reg_vars.entry(reg).or_insert_with(|| {
            builder.declare_var(ty)
        });
    }

    // ---- Frame / context access ----

    fn frame_ptr(&self, builder: &mut FunctionBuilder<'_>) -> Value {
        builder.use_var(self.frame_var)
    }

    fn ctx_ptr(&self, builder: &mut FunctionBuilder<'_>) -> Value {
        builder.use_var(self.ctx_var)
    }

    fn load_frame_field(&self, builder: &mut FunctionBuilder<'_>, offset: i32, ty: ir::types::Type) -> Value {
        let fp = self.frame_ptr(builder);
        builder.ins().load(ty, MemFlags::trusted(), fp, offset)
    }

    fn store_frame_field(&self, builder: &mut FunctionBuilder<'_>, offset: i32, val: Value) {
        let fp = self.frame_ptr(builder);
        builder.ins().store(MemFlags::trusted(), val, fp, offset);
    }

    fn load_ctx_field(&self, builder: &mut FunctionBuilder<'_>, offset: i32, ty: ir::types::Type) -> Value {
        let cp = self.ctx_ptr(builder);
        builder.ins().load(ty, MemFlags::trusted(), cp, offset)
    }

    fn store_ctx_field(&self, builder: &mut FunctionBuilder<'_>, offset: i32, val: Value) {
        let cp = self.ctx_ptr(builder);
        builder.ins().store(MemFlags::trusted(), val, cp, offset);
    }

    /// Load the locals pointer from the frame, then load a local at the given index.
    fn load_local(&self, builder: &mut FunctionBuilder<'_>, index: u32) -> Value {
        let locals_ptr = self.load_frame_field(builder, frame_offsets::LOCALS_PTR, types::I64);
        let offset = (index as i32) * 8;
        builder.ins().load(types::I64, MemFlags::trusted(), locals_ptr, offset)
    }

    /// Load the locals pointer from the frame, then store a value at the given index.
    fn store_local(&self, builder: &mut FunctionBuilder<'_>, index: u32, val: Value) {
        let locals_ptr = self.load_frame_field(builder, frame_offsets::LOCALS_PTR, types::I64);
        let offset = (index as i32) * 8;
        builder.ins().store(MemFlags::trusted(), val, locals_ptr, offset);
    }

    // ---- Helper function pointer loading ----

    /// Load a helper function pointer from the AotHelperTable.
    fn load_helper_fn_ptr(&self, builder: &mut FunctionBuilder<'_>, helper: &HelperCall) -> Option<Value> {
        let offset = helper_table_field_offset(helper)?;
        let combined_offset = ctx_offsets::HELPERS + offset;
        let cp = self.ctx_ptr(builder);
        let fn_ptr = builder.ins().load(types::I64, MemFlags::trusted(), cp, combined_offset);
        Some(fn_ptr)
    }

    /// Import a helper call signature and emit call_indirect.
    fn call_helper_indirect(
        &self,
        builder: &mut FunctionBuilder<'_>,
        fn_ptr: Value,
        sig: ir::Signature,
        args: &[Value],
    ) -> ir::Inst {
        let sig_ref = builder.import_signature(sig);
        builder.ins().call_indirect(sig_ref, fn_ptr, args)
    }

    // ---- Block lowering ----

    fn lower_block(&mut self, block: &SmBlock, builder: &mut FunctionBuilder<'_>) -> Result<(), LoweringError> {
        // Lower instructions (skip Phi nodes — handled via resolution map)
        for instr in &block.instructions {
            if matches!(instr, SmInstr::Phi { .. }) {
                continue;
            }
            self.lower_instr(instr, builder)?;
        }

        // Emit phi resolution copies before the terminator
        if let Some(copies) = self.phi_copies.get(&block.id).cloned() {
            for (phi_dest, src_reg) in &copies {
                let val = self.use_reg(builder, *src_reg);
                self.def_reg(builder, *phi_dest, val);
            }
        }

        // Lower terminator
        self.lower_terminator(&block.terminator, builder)
    }

    // ---- Instruction lowering ----

    fn lower_instr(&mut self, instr: &SmInstr, builder: &mut FunctionBuilder<'_>) -> Result<(), LoweringError> {
        match instr {
            // ===== Frame / State access =====
            SmInstr::LoadLocal { dest, index } => {
                self.ensure_reg(builder, *dest, types::I64);
                let val = self.load_local(builder, *index);
                self.def_reg(builder, *dest, val);
            }
            SmInstr::StoreLocal { index, src } => {
                let val = self.use_reg(builder, *src);
                self.store_local(builder, *index, val);
            }
            SmInstr::LoadResumePoint { dest } => {
                self.ensure_reg(builder, *dest, types::I32);
                let val = self.load_frame_field(builder, frame_offsets::RESUME_POINT, types::I32);
                self.def_reg(builder, *dest, val);
            }
            SmInstr::StoreResumePoint { value } => {
                let val = self.use_reg(builder, *value);
                self.store_frame_field(builder, frame_offsets::RESUME_POINT, val);
            }
            SmInstr::LoadChildFrame { dest } => {
                self.ensure_reg(builder, *dest, types::I64);
                let val = self.load_frame_field(builder, frame_offsets::CHILD_FRAME, types::I64);
                self.def_reg(builder, *dest, val);
            }
            SmInstr::StoreChildFrame { src } => {
                let val = self.use_reg(builder, *src);
                self.store_frame_field(builder, frame_offsets::CHILD_FRAME, val);
            }
            SmInstr::StoreSuspendReason { reason } => {
                let val = self.use_reg(builder, *reason);
                self.store_ctx_field(builder, ctx_offsets::SUSPEND_REASON, val);
            }
            SmInstr::StoreSuspendPayload { src } => {
                let val = self.use_reg(builder, *src);
                self.store_ctx_field(builder, ctx_offsets::SUSPEND_PAYLOAD, val);
            }
            SmInstr::LoadResumeValue { dest } => {
                self.ensure_reg(builder, *dest, types::I64);
                let val = self.load_ctx_field(builder, ctx_offsets::RESUME_VALUE, types::I64);
                self.def_reg(builder, *dest, val);
            }

            // ===== Constants =====
            SmInstr::ConstI32 { dest, value } => {
                self.ensure_reg(builder, *dest, types::I32);
                let val = builder.ins().iconst(types::I32, *value as i64);
                self.def_reg(builder, *dest, val);
            }
            SmInstr::ConstF64 { dest, bits } => {
                self.ensure_reg(builder, *dest, types::F64);
                let val = builder.ins().f64const(f64::from_bits(*bits));
                self.def_reg(builder, *dest, val);
            }
            SmInstr::ConstBool { dest, value } => {
                self.ensure_reg(builder, *dest, types::I8);
                let val = builder.ins().iconst(types::I8, if *value { 1 } else { 0 });
                self.def_reg(builder, *dest, val);
            }
            SmInstr::ConstNull { dest } => {
                self.ensure_reg(builder, *dest, types::I64);
                let val = abi::emit_null(builder);
                self.def_reg(builder, *dest, val);
            }

            // ===== Typed Integer Arithmetic =====
            SmInstr::I32BinOp { dest, op, left, right } => {
                self.ensure_reg(builder, *dest, types::I32);
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = self.lower_i32_binop(builder, *op, l, r);
                self.def_reg(builder, *dest, result);
            }
            SmInstr::I32Neg { dest, src } => {
                self.ensure_reg(builder, *dest, types::I32);
                let v = self.use_reg(builder, *src);
                let result = builder.ins().ineg(v);
                self.def_reg(builder, *dest, result);
            }
            SmInstr::I32BitNot { dest, src } => {
                self.ensure_reg(builder, *dest, types::I32);
                let v = self.use_reg(builder, *src);
                let result = builder.ins().bnot(v);
                self.def_reg(builder, *dest, result);
            }

            // ===== Typed Float Arithmetic =====
            SmInstr::F64BinOp { dest, op, left, right } => {
                self.ensure_reg(builder, *dest, types::F64);
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = self.lower_f64_binop(builder, *op, l, r);
                self.def_reg(builder, *dest, result);
            }
            SmInstr::F64Neg { dest, src } => {
                self.ensure_reg(builder, *dest, types::F64);
                let v = self.use_reg(builder, *src);
                let result = builder.ins().fneg(v);
                self.def_reg(builder, *dest, result);
            }

            // ===== Typed Comparisons =====
            SmInstr::I32Cmp { dest, op, left, right } => {
                self.ensure_reg(builder, *dest, types::I8);
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let cc = cmp_op_to_intcc(*op);
                let result = builder.ins().icmp(cc, l, r);
                self.def_reg(builder, *dest, result);
            }
            SmInstr::F64Cmp { dest, op, left, right } => {
                self.ensure_reg(builder, *dest, types::I8);
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let cc = cmp_op_to_floatcc(*op);
                let result = builder.ins().fcmp(cc, l, r);
                self.def_reg(builder, *dest, result);
            }

            // ===== Boolean Logic =====
            SmInstr::BoolNot { dest, src } => {
                self.ensure_reg(builder, *dest, types::I8);
                let v = self.use_reg(builder, *src);
                let one = builder.ins().iconst(types::I8, 1);
                let result = builder.ins().bxor(v, one);
                self.def_reg(builder, *dest, result);
            }
            SmInstr::BoolAnd { dest, left, right } => {
                self.ensure_reg(builder, *dest, types::I8);
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().band(l, r);
                self.def_reg(builder, *dest, result);
            }
            SmInstr::BoolOr { dest, left, right } => {
                self.ensure_reg(builder, *dest, types::I8);
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().bor(l, r);
                self.def_reg(builder, *dest, result);
            }

            // ===== NaN-boxing Conversions =====
            SmInstr::BoxI32 { dest, src } => {
                self.ensure_reg(builder, *dest, types::I64);
                let v = self.use_reg(builder, *src);
                let result = abi::emit_box_i32(builder, v);
                self.def_reg(builder, *dest, result);
            }
            SmInstr::UnboxI32 { dest, src } => {
                self.ensure_reg(builder, *dest, types::I32);
                let v = self.use_reg(builder, *src);
                let result = abi::emit_unbox_i32(builder, v);
                self.def_reg(builder, *dest, result);
            }
            SmInstr::BoxF64 { dest, src } => {
                self.ensure_reg(builder, *dest, types::I64);
                let v = self.use_reg(builder, *src);
                let result = abi::emit_box_f64(builder, v);
                self.def_reg(builder, *dest, result);
            }
            SmInstr::UnboxF64 { dest, src } => {
                self.ensure_reg(builder, *dest, types::F64);
                let v = self.use_reg(builder, *src);
                let result = abi::emit_unbox_f64(builder, v);
                self.def_reg(builder, *dest, result);
            }
            SmInstr::BoxBool { dest, src } => {
                self.ensure_reg(builder, *dest, types::I64);
                let v = self.use_reg(builder, *src);
                let result = abi::emit_box_bool(builder, v);
                self.def_reg(builder, *dest, result);
            }
            SmInstr::UnboxBool { dest, src } => {
                self.ensure_reg(builder, *dest, types::I8);
                let v = self.use_reg(builder, *src);
                let result = abi::emit_unbox_bool(builder, v);
                self.def_reg(builder, *dest, result);
            }

            // ===== Global access =====
            SmInstr::LoadGlobal { dest, index } => {
                // TODO: Globals are stored in SharedVmState; load via ctx.shared_state
                // For now, emit a null placeholder
                self.ensure_reg(builder, *dest, types::I64);
                let _ = index;
                let val = abi::emit_null(builder);
                self.def_reg(builder, *dest, val);
            }
            SmInstr::StoreGlobal { index, src } => {
                // TODO: Store to SharedVmState globals array
                let _ = (index, src);
            }

            // ===== Helper Calls =====
            SmInstr::CallHelper { dest, helper, args } => {
                self.lower_helper_call(builder, *dest, helper, args)?;
            }

            // ===== AOT Function Calls =====
            SmInstr::CallAot { dest, func_id, callee_frame } => {
                self.ensure_reg(builder, *dest, types::I64);
                self.lower_aot_call(builder, *dest, *func_id, *callee_frame)?;
            }

            // ===== Suspend Check =====
            SmInstr::IsSuspend { dest, value } => {
                self.ensure_reg(builder, *dest, types::I8);
                let val = self.use_reg(builder, *value);
                let result = abi::emit_is_suspend(builder, val);
                self.def_reg(builder, *dest, result);
            }

            // ===== ReturnValue (used in state machine save blocks) =====
            SmInstr::ReturnValue { value } => {
                // This is handled as part of the terminator, but if it appears
                // as an instruction, it's a no-op (the terminator does the return)
                let _ = value;
            }

            // ===== SSA =====
            SmInstr::Phi { .. } => {
                // Handled via resolution map — skip during instruction lowering
            }
            SmInstr::Move { dest, src } => {
                let ty = self.reg_vars.get(src)
                    .map(|_| types::I64)
                    .unwrap_or(types::I64);
                self.ensure_reg(builder, *dest, ty);
                let val = self.use_reg(builder, *src);
                self.def_reg(builder, *dest, val);
            }
        }
        Ok(())
    }

    // ---- Terminator lowering ----

    fn lower_terminator(&self, term: &SmTerminator, builder: &mut FunctionBuilder<'_>) -> Result<(), LoweringError> {
        match term {
            SmTerminator::Jump(target) => {
                let cl_block = self.resolve_block(*target)?;
                builder.ins().jump(cl_block, &[]);
            }

            SmTerminator::Branch { cond, then_block, else_block } => {
                let cond_val = self.use_reg(builder, *cond);
                let then_cl = self.resolve_block(*then_block)?;
                let else_cl = self.resolve_block(*else_block)?;
                builder.ins().brif(cond_val, then_cl, &[], else_cl, &[]);
            }

            SmTerminator::BranchNull { value, null_block, not_null_block } => {
                let val = self.use_reg(builder, *value);
                let null_const = abi::emit_null(builder);
                let is_null = builder.ins().icmp(condcodes::IntCC::Equal, val, null_const);
                let null_cl = self.resolve_block(*null_block)?;
                let not_null_cl = self.resolve_block(*not_null_block)?;
                builder.ins().brif(is_null, null_cl, &[], not_null_cl, &[]);
            }

            SmTerminator::BrTable { index, default, targets } => {
                // Emit as if-else chain for simplicity and correctness
                let index_val = self.use_reg(builder, *index);
                self.emit_dispatch_chain(builder, index_val, targets, *default)?;
            }

            SmTerminator::Return { value } => {
                let val = self.use_reg(builder, *value);
                builder.ins().return_(&[val]);
            }

            SmTerminator::Unreachable => {
                builder.ins().trap(ir::TrapCode::user(1).unwrap());
            }
        }
        Ok(())
    }

    fn resolve_block(&self, id: SmBlockId) -> Result<ir::Block, LoweringError> {
        self.block_map.get(&id).copied().ok_or(LoweringError::MissingBlock(id))
    }

    /// Emit a dispatch chain (if-else) for BrTable.
    ///
    /// For small state counts, this is efficient and avoids jump table complexity.
    fn emit_dispatch_chain(
        &self,
        builder: &mut FunctionBuilder<'_>,
        index_val: Value,
        targets: &[SmBlockId],
        default: SmBlockId,
    ) -> Result<(), LoweringError> {
        // Resolve the default block. If it's the entry block (which can't be
        // branched to in Cranelift), use the trap block instead.
        let default_cl = self.resolve_block(default)
            .map(|b| {
                // Check if this maps to the entry block (first block created).
                // In our layout, the entry block gets function params and can't
                // be a branch target. Use the trap block as fallback.
                if self.is_entry_block(default) { self.trap_block } else { b }
            })
            .unwrap_or(self.trap_block);

        if targets.is_empty() {
            builder.ins().jump(default_cl, &[]);
            return Ok(());
        }

        // For each target, check if index == i and branch
        for (i, target) in targets.iter().enumerate() {
            let is_last = i == targets.len() - 1;
            let target_cl = self.resolve_block(*target)?;

            let i_val = builder.ins().iconst(types::I32, i as i64);
            let cmp = builder.ins().icmp(condcodes::IntCC::Equal, index_val, i_val);

            if is_last {
                // Last entry: branch to target or default
                builder.ins().brif(cmp, target_cl, &[], default_cl, &[]);
            } else {
                // More entries follow: branch to target or continue checking
                let next_check = builder.create_block();
                builder.ins().brif(cmp, target_cl, &[], next_check, &[]);
                builder.switch_to_block(next_check);
            }
        }
        Ok(())
    }

    /// Check if a SmBlockId corresponds to the entry block (first block in the function).
    fn is_entry_block(&self, id: SmBlockId) -> bool {
        id == self.entry_sm_block_id
    }

    // ---- i32 binary operations ----

    fn lower_i32_binop(&self, builder: &mut FunctionBuilder<'_>, op: SmI32BinOp, l: Value, r: Value) -> Value {
        match op {
            SmI32BinOp::Add => builder.ins().iadd(l, r),
            SmI32BinOp::Sub => builder.ins().isub(l, r),
            SmI32BinOp::Mul => builder.ins().imul(l, r),
            SmI32BinOp::Div => builder.ins().sdiv(l, r),
            SmI32BinOp::Mod => builder.ins().srem(l, r),
            SmI32BinOp::Pow => {
                // Integer pow: fall back to multiplication loop
                // TODO: Call runtime helper for i32 pow
                // For now, return left (placeholder)
                l
            }
            SmI32BinOp::Shl => builder.ins().ishl(l, r),
            SmI32BinOp::Shr => builder.ins().sshr(l, r),
            SmI32BinOp::Ushr => builder.ins().ushr(l, r),
            SmI32BinOp::And => builder.ins().band(l, r),
            SmI32BinOp::Or => builder.ins().bor(l, r),
            SmI32BinOp::Xor => builder.ins().bxor(l, r),
        }
    }

    // ---- f64 binary operations ----

    fn lower_f64_binop(&self, builder: &mut FunctionBuilder<'_>, op: SmF64BinOp, l: Value, r: Value) -> Value {
        match op {
            SmF64BinOp::Add => builder.ins().fadd(l, r),
            SmF64BinOp::Sub => builder.ins().fsub(l, r),
            SmF64BinOp::Mul => builder.ins().fmul(l, r),
            SmF64BinOp::Div => builder.ins().fdiv(l, r),
            SmF64BinOp::Mod => {
                // f64 fmod: a - floor(a / b) * b
                let div = builder.ins().fdiv(l, r);
                let floored = builder.ins().floor(div);
                let product = builder.ins().fmul(floored, r);
                builder.ins().fsub(l, product)
            }
            SmF64BinOp::Pow => {
                // TODO: Call runtime libm pow helper
                // For now, return left (placeholder)
                l
            }
        }
    }

    // ---- Helper call lowering ----

    fn lower_helper_call(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        dest: Option<u32>,
        helper: &HelperCall,
        args: &[u32],
    ) -> Result<(), LoweringError> {
        // Check if this helper maps to a direct table entry
        if let Some(fn_ptr) = self.load_helper_fn_ptr(builder, helper) {
            let sig = helper_call_signature(helper, self.call_conv);
            let call_args = self.build_helper_args(builder, helper, args);
            let inst = self.call_helper_indirect(builder, fn_ptr, sig, &call_args);

            if let Some(d) = dest {
                self.ensure_reg(builder, d, types::I64);
                let results = builder.inst_results(inst);
                if !results.is_empty() {
                    self.def_reg(builder, d, results[0]);
                }
            }
            return Ok(());
        }

        // Compound operations: emit specialized code or placeholder
        match helper {
            HelperCall::GenericAdd | HelperCall::GenericSub | HelperCall::GenericMul
            | HelperCall::GenericDiv | HelperCall::GenericMod | HelperCall::GenericNeg
            | HelperCall::GenericNot | HelperCall::GenericNotEqual | HelperCall::GenericLessEqual
            | HelperCall::GenericGreater | HelperCall::GenericGreaterEqual
            | HelperCall::GenericConcat | HelperCall::ToString
            | HelperCall::NewObject | HelperCall::ObjectLiteral | HelperCall::ArrayLiteral
            | HelperCall::ArrayPop | HelperCall::LoadElement | HelperCall::StoreElement
            | HelperCall::LoadField | HelperCall::StoreField
            | HelperCall::ModuleNativeCall | HelperCall::MakeClosure
            | HelperCall::LoadCaptured | HelperCall::StoreCaptured | HelperCall::CallClosure
            | HelperCall::NewRefCell | HelperCall::LoadRefCell | HelperCall::StoreRefCell
            | HelperCall::InstanceOf | HelperCall::Cast | HelperCall::Typeof
            | HelperCall::JsonLoadProperty | HelperCall::JsonStoreProperty
            | HelperCall::StringCompare
            | HelperCall::AwaitTask | HelperCall::AwaitAll | HelperCall::YieldTask
            | HelperCall::SleepTask | HelperCall::SpawnClosure
            | HelperCall::NewMutex | HelperCall::MutexLock | HelperCall::MutexUnlock
            | HelperCall::NewChannel | HelperCall::TaskCancel
            | HelperCall::SetupTry | HelperCall::EndTry => {
                // TODO: Implement compound operations via extended helper table
                // For now, produce null for dest
                if let Some(d) = dest {
                    self.ensure_reg(builder, d, types::I64);
                    let null = abi::emit_null(builder);
                    self.def_reg(builder, d, null);
                }
            }

            // Direct table entries are handled above
            _ => {}
        }

        Ok(())
    }

    /// Build the full argument list for a helper call (including implicit ctx/frame).
    fn build_helper_args(&self, builder: &mut FunctionBuilder<'_>, helper: &HelperCall, args: &[u32]) -> Vec<Value> {
        let ctx = self.ctx_ptr(builder);

        match helper {
            // (func_id: u32, local_count: u32, func_ptr: i64) -> *mut AotFrame
            HelperCall::AllocFrame => {
                args.iter().map(|a| self.use_reg(builder, *a)).collect()
            }

            // (frame_ptr: i64)
            HelperCall::FreeFrame => {
                args.iter().map(|a| self.use_reg(builder, *a)).collect()
            }

            // (ctx: i64)
            HelperCall::SafepointPoll => vec![ctx],

            // (ctx, class_id: u32) -> u64
            HelperCall::AllocObject => {
                let mut v = vec![ctx];
                for a in args { v.push(self.use_reg(builder, *a)); }
                v
            }

            // (ctx, type_id: u32, capacity: u32) -> u64
            HelperCall::AllocArray => {
                let mut v = vec![ctx];
                for a in args { v.push(self.use_reg(builder, *a)); }
                v
            }

            // (ctx, data_ptr: i64, len: u32) -> u64
            HelperCall::AllocString => {
                let mut v = vec![ctx];
                for a in args { v.push(self.use_reg(builder, *a)); }
                v
            }

            // (ctx, a: u64, b: u64) -> u64
            HelperCall::StringConcat => {
                let mut v = vec![ctx];
                for a in args { v.push(self.use_reg(builder, *a)); }
                v
            }

            // (val: u64) -> u64
            HelperCall::StringLen | HelperCall::ArrayLen => {
                args.iter().map(|a| self.use_reg(builder, *a)).collect()
            }

            // (array: u64, index: u64) -> u64
            HelperCall::ArrayGet => {
                args.iter().map(|a| self.use_reg(builder, *a)).collect()
            }

            // (array: u64, index: u64, value: u64) -> void
            HelperCall::ArraySet => {
                args.iter().map(|a| self.use_reg(builder, *a)).collect()
            }

            // (ctx, array: u64, value: u64)
            HelperCall::ArrayPush => {
                let mut v = vec![ctx];
                for a in args { v.push(self.use_reg(builder, *a)); }
                v
            }

            // (a: u64, b: u64) -> u8
            HelperCall::GenericEquals | HelperCall::GenericLessThan => {
                args.iter().map(|a| self.use_reg(builder, *a)).collect()
            }

            // (obj: u64, field_index: u32) -> u64
            HelperCall::ObjectGetField => {
                args.iter().map(|a| self.use_reg(builder, *a)).collect()
            }

            // (obj: u64, field_index: u32, value: u64)
            HelperCall::ObjectSetField => {
                args.iter().map(|a| self.use_reg(builder, *a)).collect()
            }

            // (ctx, native_id: u16, args_ptr: i64, argc: u8) -> u64
            HelperCall::NativeCall => {
                let mut v = vec![ctx];
                for a in args { v.push(self.use_reg(builder, *a)); }
                v
            }

            // (result: u64) -> u8
            HelperCall::IsNativeSuspend => {
                args.iter().map(|a| self.use_reg(builder, *a)).collect()
            }

            // (ctx, func_id: u32, args_ptr: i64, argc: u32) -> u64
            HelperCall::Spawn => {
                let mut v = vec![ctx];
                for a in args { v.push(self.use_reg(builder, *a)); }
                v
            }

            // (ctx) -> u8
            HelperCall::CheckPreemption => vec![ctx],

            // (ctx, exception_val: u64)
            HelperCall::ThrowException => {
                let mut v = vec![ctx];
                for a in args { v.push(self.use_reg(builder, *a)); }
                v
            }

            // (func_id: u32) -> i64
            HelperCall::GetAotFuncPtr => {
                args.iter().map(|a| self.use_reg(builder, *a)).collect()
            }

            // (ctx, const_index: u32) -> u64
            HelperCall::LoadStringConstant => {
                let mut v = vec![ctx];
                for a in args { v.push(self.use_reg(builder, *a)); }
                v
            }

            // (value: i32) -> u64
            HelperCall::LoadI32Constant => {
                args.iter().map(|a| self.use_reg(builder, *a)).collect()
            }

            // (value: f64) -> u64
            HelperCall::LoadF64Constant => {
                args.iter().map(|a| self.use_reg(builder, *a)).collect()
            }

            // Compound operations (not direct table entries) — handled elsewhere
            _ => args.iter().map(|a| self.use_reg(builder, *a)).collect(),
        }
    }

    /// Lower an AOT function call: look up the function pointer, create a child
    /// frame, call through it, and store the result.
    fn lower_aot_call(
        &self,
        builder: &mut FunctionBuilder<'_>,
        dest: u32,
        func_id: u32,
        callee_frame: u32,
    ) -> Result<(), LoweringError> {
        // Load the callee's entry function pointer via GetAotFuncPtr helper
        let get_func_ptr = self.load_helper_fn_ptr(builder, &HelperCall::GetAotFuncPtr)
            .ok_or_else(|| LoweringError::Unsupported("GetAotFuncPtr helper not found".into()))?;

        let mut sig = ir::Signature::new(self.call_conv);
        sig.params.push(AbiParam::new(types::I32)); // func_id
        sig.returns.push(AbiParam::new(types::I64)); // fn_ptr

        let func_id_val = builder.ins().iconst(types::I32, func_id as i64);
        let sig_ref = builder.import_signature(sig);
        let inst = builder.ins().call_indirect(sig_ref, get_func_ptr, &[func_id_val]);
        let callee_fn_ptr = builder.inst_results(inst)[0];

        // Call the callee: callee_fn(callee_frame, ctx) -> u64
        let callee_frame_val = self.use_reg(builder, callee_frame);
        let ctx = self.ctx_ptr(builder);

        let call_sig = aot_entry_signature(self.call_conv);
        let call_sig_ref = builder.import_signature(call_sig);
        let call_inst = builder.ins().call_indirect(
            call_sig_ref, callee_fn_ptr, &[callee_frame_val, ctx],
        );
        let result = builder.inst_results(call_inst)[0];
        self.def_reg(builder, dest, result);

        Ok(())
    }
}

// =============================================================================
// Helper function signatures
// =============================================================================

/// Get the Cranelift function signature for a direct helper table entry.
fn helper_call_signature(helper: &HelperCall, call_conv: CallConv) -> ir::Signature {
    let mut sig = ir::Signature::new(call_conv);

    match helper {
        // (func_id: u32, local_count: u32, func_ptr: i64) -> *mut AotFrame (i64)
        HelperCall::AllocFrame => {
            sig.params.push(AbiParam::new(types::I32));
            sig.params.push(AbiParam::new(types::I32));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
        }
        // (frame_ptr: i64)
        HelperCall::FreeFrame => {
            sig.params.push(AbiParam::new(types::I64));
        }
        // (ctx: i64)
        HelperCall::SafepointPoll => {
            sig.params.push(AbiParam::new(types::I64));
        }
        // (ctx: i64, class_id: u32) -> u64
        HelperCall::AllocObject => {
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I32));
            sig.returns.push(AbiParam::new(types::I64));
        }
        // (ctx: i64, type_id: u32, capacity: u32) -> u64
        HelperCall::AllocArray => {
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I32));
            sig.params.push(AbiParam::new(types::I32));
            sig.returns.push(AbiParam::new(types::I64));
        }
        // (ctx: i64, data_ptr: i64, len: u32) -> u64
        HelperCall::AllocString => {
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I32));
            sig.returns.push(AbiParam::new(types::I64));
        }
        // (ctx: i64, a: u64, b: u64) -> u64
        HelperCall::StringConcat => {
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
        }
        // (val: u64) -> u64
        HelperCall::StringLen | HelperCall::ArrayLen => {
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
        }
        // (array: u64, index: u64) -> u64
        HelperCall::ArrayGet => {
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
        }
        // (array: u64, index: u64, value: u64)
        HelperCall::ArraySet => {
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
        }
        // (ctx: i64, array: u64, value: u64)
        HelperCall::ArrayPush => {
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
        }
        // (a: u64, b: u64) -> u8
        HelperCall::GenericEquals | HelperCall::GenericLessThan => {
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I8));
        }
        // (obj: u64, field_index: u32) -> u64
        HelperCall::ObjectGetField => {
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I32));
            sig.returns.push(AbiParam::new(types::I64));
        }
        // (obj: u64, field_index: u32, value: u64)
        HelperCall::ObjectSetField => {
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I32));
            sig.params.push(AbiParam::new(types::I64));
        }
        // (ctx: i64, native_id: u16, args_ptr: i64, argc: u8) -> u64
        HelperCall::NativeCall => {
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I16));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I8));
            sig.returns.push(AbiParam::new(types::I64));
        }
        // (result: u64) -> u8
        HelperCall::IsNativeSuspend => {
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I8));
        }
        // (ctx: i64, func_id: u32, args_ptr: i64, argc: u32) -> u64
        HelperCall::Spawn => {
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I32));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I32));
            sig.returns.push(AbiParam::new(types::I64));
        }
        // (ctx: i64) -> u8
        HelperCall::CheckPreemption => {
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I8));
        }
        // (ctx: i64, exception_val: u64)
        HelperCall::ThrowException => {
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
        }
        // (func_id: u32) -> i64 (fn ptr)
        HelperCall::GetAotFuncPtr => {
            sig.params.push(AbiParam::new(types::I32));
            sig.returns.push(AbiParam::new(types::I64));
        }
        // (ctx: i64, const_index: u32) -> u64
        HelperCall::LoadStringConstant => {
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I32));
            sig.returns.push(AbiParam::new(types::I64));
        }
        // (value: i32) -> u64
        HelperCall::LoadI32Constant => {
            sig.params.push(AbiParam::new(types::I32));
            sig.returns.push(AbiParam::new(types::I64));
        }
        // (value: f64) -> u64
        HelperCall::LoadF64Constant => {
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::I64));
        }

        // Compound operations don't have direct table entries — shouldn't reach here
        _ => {}
    }

    sig
}

/// Get the byte offset of a helper within AotHelperTable.
/// Returns None for compound operations that don't map to a table entry.
fn helper_table_field_offset(helper: &HelperCall) -> Option<i32> {
    // Each field is a function pointer: 8 bytes on 64-bit
    let index = match helper {
        HelperCall::AllocFrame => 0,
        HelperCall::FreeFrame => 1,
        HelperCall::SafepointPoll => 2,
        HelperCall::AllocObject => 3,
        HelperCall::AllocArray => 4,
        HelperCall::AllocString => 5,
        HelperCall::StringConcat => 6,
        HelperCall::StringLen => 7,
        HelperCall::ArrayLen => 8,
        HelperCall::ArrayGet => 9,
        HelperCall::ArraySet => 10,
        HelperCall::ArrayPush => 11,
        HelperCall::GenericEquals => 12,
        HelperCall::GenericLessThan => 13,
        HelperCall::ObjectGetField => 14,
        HelperCall::ObjectSetField => 15,
        HelperCall::NativeCall => 16,
        HelperCall::IsNativeSuspend => 17,
        HelperCall::Spawn => 18,
        HelperCall::CheckPreemption => 19,
        HelperCall::ThrowException => 20,
        HelperCall::GetAotFuncPtr => 21,
        HelperCall::LoadStringConstant => 22,
        HelperCall::LoadI32Constant => 23,
        HelperCall::LoadF64Constant => 24,
        _ => return None, // Compound operation
    };
    Some(index * 8)
}

// =============================================================================
// Comparison operator mapping
// =============================================================================

fn cmp_op_to_intcc(op: SmCmpOp) -> condcodes::IntCC {
    match op {
        SmCmpOp::Eq => condcodes::IntCC::Equal,
        SmCmpOp::Ne => condcodes::IntCC::NotEqual,
        SmCmpOp::Lt => condcodes::IntCC::SignedLessThan,
        SmCmpOp::Le => condcodes::IntCC::SignedLessThanOrEqual,
        SmCmpOp::Gt => condcodes::IntCC::SignedGreaterThan,
        SmCmpOp::Ge => condcodes::IntCC::SignedGreaterThanOrEqual,
    }
}

fn cmp_op_to_floatcc(op: SmCmpOp) -> condcodes::FloatCC {
    match op {
        SmCmpOp::Eq => condcodes::FloatCC::Equal,
        SmCmpOp::Ne => condcodes::FloatCC::NotEqual,
        SmCmpOp::Lt => condcodes::FloatCC::LessThan,
        SmCmpOp::Le => condcodes::FloatCC::LessThanOrEqual,
        SmCmpOp::Gt => condcodes::FloatCC::GreaterThan,
        SmCmpOp::Ge => condcodes::FloatCC::GreaterThanOrEqual,
    }
}

// =============================================================================
// Frame and context field offsets
// =============================================================================

/// Field offsets within AotFrame for Cranelift code generation.
///
/// These must match the `#[repr(C)]` layout of `AotFrame`.
pub mod frame_offsets {
    /// Offset of `function_id` field (u32)
    pub const FUNCTION_ID: i32 = 0;
    /// Offset of `resume_point` field (u32)
    pub const RESUME_POINT: i32 = 4;
    /// Offset of `locals` pointer field (*mut u64)
    pub const LOCALS_PTR: i32 = 8;
    /// Offset of `local_count` field (u32)
    pub const LOCAL_COUNT: i32 = 16;
    /// Offset of `param_count` field (u32)
    pub const PARAM_COUNT: i32 = 20;
    /// Offset of `child_frame` pointer field (*mut AotFrame)
    pub const CHILD_FRAME: i32 = 24;
    /// Offset of `function_ptr` field (AotEntryFn)
    pub const FUNCTION_PTR: i32 = 32;
    /// Offset of `suspend_payload` field (u64)
    pub const SUSPEND_PAYLOAD: i32 = 40;
}

/// Field offsets within AotTaskContext for Cranelift code generation.
///
/// These must match the `#[repr(C)]` layout of `AotTaskContext`.
pub mod ctx_offsets {
    /// Offset of `preempt_requested` pointer field
    pub const PREEMPT_REQUESTED: i32 = 0;
    /// Offset of `resume_value` field
    pub const RESUME_VALUE: i32 = 8;
    /// Offset of `suspend_reason` field (SuspendReason is 4 bytes, #[repr(C)] enum)
    pub const SUSPEND_REASON: i32 = 16;
    /// Offset of `suspend_payload` field (u64, after 4 bytes padding)
    pub const SUSPEND_PAYLOAD: i32 = 24;
    /// Offset of `helpers` field (start of AotHelperTable)
    pub const HELPERS: i32 = 32;
    /// Offset of `shared_state` pointer field
    pub const SHARED_STATE: i32 = HELPERS + super::helper_table_size() as i32;
}

/// Number of entries in the AotHelperTable.
pub const HELPER_TABLE_ENTRY_COUNT: usize = 25;

/// Size of the AotHelperTable in bytes (for computing offsets after it).
pub const fn helper_table_size() -> usize {
    // 25 function pointers × 8 bytes each
    HELPER_TABLE_ENTRY_COUNT * 8
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift_codegen::settings;

    #[test]
    fn test_aot_entry_signature() {
        let sig = aot_entry_signature(CallConv::SystemV);
        assert_eq!(sig.params.len(), 2);
        assert_eq!(sig.returns.len(), 1);
        assert_eq!(sig.params[0].value_type, types::I64);
        assert_eq!(sig.params[1].value_type, types::I64);
        assert_eq!(sig.returns[0].value_type, types::I64);
    }

    #[test]
    fn test_helper_table_size() {
        // 25 function pointers × 8 bytes = 200 bytes
        assert_eq!(helper_table_size(), 200);
    }

    #[test]
    fn test_helper_offsets() {
        assert_eq!(helper_table_field_offset(&HelperCall::AllocFrame), Some(0));
        assert_eq!(helper_table_field_offset(&HelperCall::FreeFrame), Some(8));
        assert_eq!(helper_table_field_offset(&HelperCall::SafepointPoll), Some(16));
        assert_eq!(helper_table_field_offset(&HelperCall::LoadF64Constant), Some(24 * 8));
        // Compound operations return None
        assert_eq!(helper_table_field_offset(&HelperCall::GenericAdd), None);
    }

    #[test]
    fn test_cmp_op_mapping() {
        assert_eq!(cmp_op_to_intcc(SmCmpOp::Eq), condcodes::IntCC::Equal);
        assert_eq!(cmp_op_to_intcc(SmCmpOp::Lt), condcodes::IntCC::SignedLessThan);
        assert_eq!(cmp_op_to_floatcc(SmCmpOp::Gt), condcodes::FloatCC::GreaterThan);
    }

    #[test]
    fn test_determine_reg_types() {
        use super::super::statemachine::{SmBlock, SmBlockId, SmBlockKind, SmTerminator};

        let blocks = vec![SmBlock {
            id: SmBlockId(0),
            kind: SmBlockKind::Body,
            instructions: vec![
                SmInstr::ConstI32 { dest: 0, value: 42 },
                SmInstr::ConstF64 { dest: 1, bits: f64::to_bits(3.14) },
                SmInstr::ConstBool { dest: 2, value: true },
                SmInstr::BoxI32 { dest: 3, src: 0 },
                SmInstr::UnboxF64 { dest: 4, src: 3 },
            ],
            terminator: SmTerminator::Return { value: 3 },
        }];

        let types = determine_reg_types(&blocks);
        assert_eq!(types[&0], types::I32);
        assert_eq!(types[&1], types::F64);
        assert_eq!(types[&2], types::I8);
        assert_eq!(types[&3], types::I64); // boxed
        assert_eq!(types[&4], types::F64); // unboxed
    }

    #[test]
    fn test_lower_simple_function() {
        use super::super::statemachine::{SmBlock, SmBlockId, SmBlockKind, SmTerminator};
        use super::super::analysis::SuspensionAnalysis;

        let sm_func = StateMachineFunction {
            function_id: 0,
            local_count: 1,
            param_count: 0,
            name: Some("test".to_string()),
            analysis: SuspensionAnalysis::none(),
            blocks: vec![SmBlock {
                id: SmBlockId(0),
                kind: SmBlockKind::Body,
                instructions: vec![
                    SmInstr::ConstI32 { dest: 0, value: 42 },
                    SmInstr::BoxI32 { dest: 1, src: 0 },
                ],
                terminator: SmTerminator::Return { value: 1 },
            }],
        };

        // Build Cranelift IR
        let flags = settings::Flags::new(settings::builder());
        let mut codegen_ctx = cranelift_codegen::Context::new();
        codegen_ctx.func.signature = aot_entry_signature(CallConv::SystemV);

        let mut func_builder_ctx = FunctionBuilderContext::new();
        let builder = FunctionBuilder::new(
            &mut codegen_ctx.func,
            &mut func_builder_ctx,
        );

        // Lower should succeed
        let result = lower_function(&sm_func, builder);
        assert!(result.is_ok(), "lower_function failed: {:?}", result.err());

        // Verify the function can be verified by Cranelift
        let verify_result = cranelift_codegen::verify_function(&codegen_ctx.func, &flags);
        assert!(verify_result.is_ok(), "Cranelift verify failed: {:?}", verify_result.err());
    }

    #[test]
    fn test_lower_arithmetic_function() {
        use super::super::statemachine::{SmBlock, SmBlockId, SmBlockKind, SmTerminator};
        use super::super::analysis::SuspensionAnalysis;

        // Function: unbox two i32 params, add them, box the result
        let sm_func = StateMachineFunction {
            function_id: 1,
            local_count: 2,
            param_count: 2,
            name: Some("add".to_string()),
            analysis: SuspensionAnalysis::none(),
            blocks: vec![SmBlock {
                id: SmBlockId(0),
                kind: SmBlockKind::Body,
                instructions: vec![
                    // Load params from frame
                    SmInstr::LoadLocal { dest: 0, index: 0 },
                    SmInstr::LoadLocal { dest: 1, index: 1 },
                    // Unbox to i32
                    SmInstr::UnboxI32 { dest: 2, src: 0 },
                    SmInstr::UnboxI32 { dest: 3, src: 1 },
                    // Add
                    SmInstr::I32BinOp { dest: 4, op: SmI32BinOp::Add, left: 2, right: 3 },
                    // Box result
                    SmInstr::BoxI32 { dest: 5, src: 4 },
                ],
                terminator: SmTerminator::Return { value: 5 },
            }],
        };

        let flags = settings::Flags::new(settings::builder());
        let mut codegen_ctx = cranelift_codegen::Context::new();
        codegen_ctx.func.signature = aot_entry_signature(CallConv::SystemV);

        let mut func_builder_ctx = FunctionBuilderContext::new();
        let builder = FunctionBuilder::new(
            &mut codegen_ctx.func,
            &mut func_builder_ctx,
        );

        let result = lower_function(&sm_func, builder);
        assert!(result.is_ok(), "lower_function failed: {:?}", result.err());

        let verify_result = cranelift_codegen::verify_function(&codegen_ctx.func, &flags);
        assert!(verify_result.is_ok(), "Cranelift verify failed: {:?}", verify_result.err());
    }

    #[test]
    fn test_lower_with_branch() {
        use super::super::statemachine::{SmBlock, SmBlockId, SmBlockKind, SmTerminator};
        use super::super::analysis::SuspensionAnalysis;

        // Function: if (param0 < 10) return 1 else return 0
        let sm_func = StateMachineFunction {
            function_id: 2,
            local_count: 1,
            param_count: 1,
            name: Some("test_branch".to_string()),
            analysis: SuspensionAnalysis::none(),
            blocks: vec![
                SmBlock {
                    id: SmBlockId(0),
                    kind: SmBlockKind::Body,
                    instructions: vec![
                        SmInstr::LoadLocal { dest: 0, index: 0 },
                        SmInstr::UnboxI32 { dest: 1, src: 0 },
                        SmInstr::ConstI32 { dest: 2, value: 10 },
                        SmInstr::I32Cmp { dest: 3, op: SmCmpOp::Lt, left: 1, right: 2 },
                    ],
                    terminator: SmTerminator::Branch {
                        cond: 3,
                        then_block: SmBlockId(1),
                        else_block: SmBlockId(2),
                    },
                },
                SmBlock {
                    id: SmBlockId(1),
                    kind: SmBlockKind::Body,
                    instructions: vec![
                        SmInstr::ConstI32 { dest: 10, value: 1 },
                        SmInstr::BoxI32 { dest: 11, src: 10 },
                    ],
                    terminator: SmTerminator::Return { value: 11 },
                },
                SmBlock {
                    id: SmBlockId(2),
                    kind: SmBlockKind::Body,
                    instructions: vec![
                        SmInstr::ConstI32 { dest: 20, value: 0 },
                        SmInstr::BoxI32 { dest: 21, src: 20 },
                    ],
                    terminator: SmTerminator::Return { value: 21 },
                },
            ],
        };

        let flags = settings::Flags::new(settings::builder());
        let mut codegen_ctx = cranelift_codegen::Context::new();
        codegen_ctx.func.signature = aot_entry_signature(CallConv::SystemV);

        let mut func_builder_ctx = FunctionBuilderContext::new();
        let builder = FunctionBuilder::new(
            &mut codegen_ctx.func,
            &mut func_builder_ctx,
        );

        let result = lower_function(&sm_func, builder);
        assert!(result.is_ok(), "lower_function failed: {:?}", result.err());

        let verify_result = cranelift_codegen::verify_function(&codegen_ctx.func, &flags);
        assert!(verify_result.is_ok(), "Cranelift verify failed: {:?}", verify_result.err());
    }

    #[test]
    fn test_lower_state_machine_dispatch() {
        use super::super::statemachine::{SmBlock, SmBlockId, SmBlockKind, SmTerminator};
        use super::super::analysis::{SuspensionAnalysis, SuspensionPoint, SuspensionKind};
        use std::collections::HashSet;

        // Simulate a function with a dispatch block and body
        let sm_func = StateMachineFunction {
            function_id: 3,
            local_count: 2,
            param_count: 0,
            name: Some("test_dispatch".to_string()),
            analysis: SuspensionAnalysis {
                points: vec![SuspensionPoint {
                    index: 0,
                    block_id: 0,
                    instr_index: 0,
                    kind: SuspensionKind::Await,
                    live_locals: HashSet::new(),
                }],
                has_suspensions: true,
                loop_headers: HashSet::new(),
            },
            blocks: vec![
                // Dispatch block
                SmBlock {
                    id: SmBlockId(100),
                    kind: SmBlockKind::Dispatch,
                    instructions: vec![
                        SmInstr::LoadResumePoint { dest: 50 },
                    ],
                    terminator: SmTerminator::BrTable {
                        index: 50,
                        default: SmBlockId(100), // self (unreachable in practice)
                        targets: vec![SmBlockId(0), SmBlockId(200)],
                    },
                },
                // Entry body
                SmBlock {
                    id: SmBlockId(0),
                    kind: SmBlockKind::Body,
                    instructions: vec![
                        SmInstr::ConstI32 { dest: 0, value: 42 },
                        SmInstr::BoxI32 { dest: 1, src: 0 },
                    ],
                    terminator: SmTerminator::Return { value: 1 },
                },
                // Restore block
                SmBlock {
                    id: SmBlockId(200),
                    kind: SmBlockKind::RestoreState { suspension_index: 0 },
                    instructions: vec![
                        SmInstr::LoadResumeValue { dest: 60 },
                    ],
                    terminator: SmTerminator::Jump(SmBlockId(0)),
                },
            ],
        };

        let flags = settings::Flags::new(settings::builder());
        let mut codegen_ctx = cranelift_codegen::Context::new();
        codegen_ctx.func.signature = aot_entry_signature(CallConv::SystemV);

        let mut func_builder_ctx = FunctionBuilderContext::new();
        let builder = FunctionBuilder::new(
            &mut codegen_ctx.func,
            &mut func_builder_ctx,
        );

        let result = lower_function(&sm_func, builder);
        assert!(result.is_ok(), "lower_function failed: {:?}", result.err());

        let verify_result = cranelift_codegen::verify_function(&codegen_ctx.func, &flags);
        assert!(verify_result.is_ok(), "Cranelift verify failed: {:?}", verify_result.err());
    }
}
