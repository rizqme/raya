//! JIT IR → Cranelift IR lowering
//!
//! Translates the backend-agnostic JIT IR (SSA form) into Cranelift IR that can
//! be compiled to native code. Handles typed arithmetic, NaN-boxing conversions,
//! local variable access, and control flow.

use cranelift_codegen::ir::AbiParam;
use cranelift_codegen::ir::{
    self, condcodes, types, InstBuilder, MemFlags, StackSlotData, StackSlotKind,
};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};
use rustc_hash::{FxHashMap, FxHashSet};

use super::abi;
use crate::compiler::Opcode;
use crate::jit::ir::instr::{JitBlockId, JitFunction, JitInstr, JitTerminator, Reg};
use crate::jit::runtime::helpers::{
    JIT_INTERPRETER_EXCEPTION_SENTINEL, JIT_INTERPRETER_FALLBACK_SENTINEL,
    JIT_NATIVE_SUSPEND_SENTINEL, JIT_SHAPE_FIELD_FALLBACK_SENTINEL,
    JIT_STRING_LEN_FALLBACK_SENTINEL,
};
use crate::jit::runtime::trampoline::{JitExitKind, JitSuspendReason, JIT_EXIT_MAX_NATIVE_ARGS};

/// State maintained during lowering of a single function
pub struct LoweringContext<'a> {
    /// Map from JIT IR Reg → Cranelift Variable
    reg_vars: FxHashMap<Reg, Variable>,
    /// Map from JIT BlockId → Cranelift Block
    block_map: FxHashMap<JitBlockId, ir::Block>,
    /// The JIT function being lowered
    func: &'a JitFunction,
    /// The bytecode module providing constant-pool data.
    module: &'a crate::compiler::bytecode::Module,
    /// Cranelift function parameters (args_ptr, arg_count, locals_ptr, local_count, ctx_ptr)
    params: FunctionParams,
    /// Phi resolution: for each block, a list of (phi_dest_reg, source_reg) to def_var before terminator
    phi_copies: FxHashMap<JitBlockId, Vec<(Reg, Reg)>>,
    /// Imported signature for RuntimeHelperTable.safepoint_poll
    sig_safepoint_poll: Option<ir::SigRef>,
    /// Imported signature for RuntimeHelperTable.check_preemption
    sig_check_preemption: Option<ir::SigRef>,
    /// Imported signature for RuntimeHelperTable.native_call_dispatch
    sig_native_call_dispatch: Option<ir::SigRef>,
    /// Imported signature for RuntimeHelperTable.alloc_object
    sig_alloc_object: Option<ir::SigRef>,
    /// Imported signature for RuntimeHelperTable.alloc_string
    sig_alloc_string: Option<ir::SigRef>,
    /// Imported signature for RuntimeHelperTable.object_get_field
    sig_object_get_field: Option<ir::SigRef>,
    /// Imported signature for RuntimeHelperTable.object_get_shape_field
    sig_object_get_shape_field: Option<ir::SigRef>,
    /// Imported signature for RuntimeHelperTable.object_set_shape_field
    sig_object_set_shape_field: Option<ir::SigRef>,
    /// Imported signature for RuntimeHelperTable.object_implements_shape
    sig_object_implements_shape: Option<ir::SigRef>,
    /// Imported signature for RuntimeHelperTable.object_is_nominal
    sig_object_is_nominal: Option<ir::SigRef>,
    /// Imported signature for RuntimeHelperTable.interpreter_call
    sig_interpreter_call: Option<ir::SigRef>,
    /// Imported signature for RuntimeHelperTable.string_len
    sig_string_len: Option<ir::SigRef>,
}

/// The five parameters of the JIT entry function ABI
struct FunctionParams {
    _args_ptr: ir::Value,
    _arg_count: ir::Value,
    locals_ptr: ir::Value,
    _local_count: ir::Value,
    ctx_ptr: ir::Value,
    exit_info_ptr: ir::Value,
}

/// Identify loop headers: blocks where at least one predecessor has a higher
/// block index (indicating a back-edge).
fn identify_loop_headers(func: &JitFunction) -> FxHashSet<JitBlockId> {
    let mut headers = FxHashSet::default();
    for block in &func.blocks {
        for pred in &block.predecessors {
            // Back-edge: predecessor ID >= this block's ID
            if pred.0 >= block.id.0 {
                headers.insert(block.id);
            }
        }
    }
    headers
}

/// Build the Phi resolution map: for each predecessor block, collect
/// (phi_dest_reg, source_reg) pairs that need def_var before the terminator.
fn build_phi_copies(func: &JitFunction) -> FxHashMap<JitBlockId, Vec<(Reg, Reg)>> {
    let mut copies: FxHashMap<JitBlockId, Vec<(Reg, Reg)>> = FxHashMap::default();
    for block in &func.blocks {
        for instr in &block.instrs {
            if let JitInstr::Phi { dest, sources } = instr {
                for (src_block, src_reg) in sources {
                    copies
                        .entry(*src_block)
                        .or_default()
                        .push((*dest, *src_reg));
                }
            }
        }
    }
    copies
}

impl<'a> LoweringContext<'a> {
    fn clif_type_for_jit_type(ty: crate::jit::ir::types::JitType) -> ir::Type {
        match ty {
            crate::jit::ir::types::JitType::F64 => types::F64,
            crate::jit::ir::types::JitType::Bool => types::I8,
            crate::jit::ir::types::JitType::I32 => types::I32,
            _ => types::I64,
        }
    }

    /// Lower an entire JIT function into Cranelift IR.
    /// Takes ownership of the FunctionBuilder since finalize() consumes it.
    pub fn lower(
        func: &'a JitFunction,
        module: &'a crate::compiler::bytecode::Module,
        mut builder: FunctionBuilder<'_>,
    ) -> Result<(), LowerError> {
        // Create Cranelift blocks for each JIT block
        let mut block_map = FxHashMap::default();
        for jit_block in &func.blocks {
            let cl_block = builder.create_block();
            block_map.insert(jit_block.id, cl_block);
        }

        // Identify loop headers (blocks with back-edge predecessors)
        let loop_headers = identify_loop_headers(func);

        // Build Phi resolution copies
        let phi_copies = build_phi_copies(func);

        // Entry block gets the function parameters
        let entry_block = block_map[&func.entry];
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);

        // Only seal the entry block if it's not a loop header
        if !loop_headers.contains(&func.entry) {
            builder.seal_block(entry_block);
        }

        // Extract parameters
        let params = FunctionParams {
            _args_ptr: builder.block_params(entry_block)[0],
            _arg_count: builder.block_params(entry_block)[1],
            locals_ptr: builder.block_params(entry_block)[2],
            _local_count: builder.block_params(entry_block)[3],
            ctx_ptr: builder.block_params(entry_block)[4],
            exit_info_ptr: builder.block_params(entry_block)[5],
        };

        let mut ctx = LoweringContext {
            reg_vars: FxHashMap::default(),
            block_map,
            func,
            module,
            params,
            phi_copies,
            sig_safepoint_poll: None,
            sig_check_preemption: None,
            sig_native_call_dispatch: None,
            sig_alloc_object: None,
            sig_alloc_string: None,
            sig_object_get_field: None,
            sig_object_get_shape_field: None,
            sig_object_set_shape_field: None,
            sig_object_implements_shape: None,
            sig_object_is_nominal: None,
            sig_interpreter_call: None,
            sig_string_len: None,
        };

        // Declare all registers as Cranelift variables
        ctx.declare_all_regs(&mut builder);

        // Lower each block
        let block_ids: Vec<_> = func.blocks.iter().map(|b| b.id).collect();
        for (idx, block_id) in block_ids.iter().enumerate() {
            let cl_block = ctx.block_map[block_id];

            // Switch to block (entry already active for first block)
            if idx > 0 {
                builder.switch_to_block(cl_block);

                // Seal immediately unless it's a loop header (defer those)
                if !loop_headers.contains(block_id) {
                    builder.seal_block(cl_block);
                }
            }

            ctx.lower_block(*block_id, &mut builder)?;
        }

        // Seal all deferred loop headers now that all predecessors are known
        for header_id in &loop_headers {
            let cl_block = ctx.block_map[header_id];
            builder.seal_block(cl_block);
        }

        // Finalize (consumes the builder)
        builder.finalize();
        Ok(())
    }

    /// Declare all JIT registers as Cranelift variables
    fn declare_all_regs(&mut self, builder: &mut FunctionBuilder<'_>) {
        for reg_idx in 0..self.func.next_reg {
            let reg = Reg(reg_idx);
            let ty = Self::clif_type_for_jit_type(self.func.reg_type(reg));
            // In Cranelift 0.128, declare_var takes only a type and returns the Variable
            let var = builder.declare_var(ty);
            self.reg_vars.insert(reg, var);
        }
    }

    /// Get or create a Cranelift variable for a JIT register
    fn var_for(&self, reg: Reg) -> Variable {
        self.reg_vars[&reg]
    }

    /// Read a JIT register as a Cranelift value
    fn use_reg(&self, builder: &mut FunctionBuilder<'_>, reg: Reg) -> ir::Value {
        builder.use_var(self.var_for(reg))
    }

    /// Write a Cranelift value to a JIT register
    fn def_reg(&self, builder: &mut FunctionBuilder<'_>, reg: Reg, val: ir::Value) {
        builder.def_var(self.var_for(reg), val);
    }

    fn boxed_reg_value(&self, builder: &mut FunctionBuilder<'_>, reg: Reg) -> ir::Value {
        let raw = self.use_reg(builder, reg);
        match self.func.reg_type(reg) {
            crate::jit::ir::types::JitType::I32 => abi::emit_box_i32(builder, raw),
            crate::jit::ir::types::JitType::F64 => abi::emit_box_f64(builder, raw),
            crate::jit::ir::types::JitType::Bool => abi::emit_box_bool(builder, raw),
            crate::jit::ir::types::JitType::Ptr => abi::emit_box_ptr(builder, raw),
            _ => raw,
        }
    }

    fn coerce_boxed_value_for_reg(
        &self,
        builder: &mut FunctionBuilder<'_>,
        reg: Reg,
        boxed: ir::Value,
    ) -> ir::Value {
        match self.func.reg_type(reg) {
            crate::jit::ir::types::JitType::I32 => abi::emit_unbox_i32(builder, boxed),
            crate::jit::ir::types::JitType::F64 => abi::emit_unbox_f64(builder, boxed),
            crate::jit::ir::types::JitType::Bool => abi::emit_unbox_bool(builder, boxed),
            crate::jit::ir::types::JitType::Ptr => abi::emit_unbox_ptr(builder, boxed),
            _ => boxed,
        }
    }

    fn emit_interpreter_boundary_exit(
        &self,
        builder: &mut FunctionBuilder<'_>,
        regs: &[Reg],
        bytecode_offset: u32,
    ) {
        let count = regs.len().min(JIT_EXIT_MAX_NATIVE_ARGS) as i64;
        let count_val = builder.ins().iconst(types::I32, count);
        builder.ins().store(
            MemFlags::trusted(),
            count_val,
            self.params.exit_info_ptr,
            40,
        );
        for (i, reg) in regs.iter().take(JIT_EXIT_MAX_NATIVE_ARGS).enumerate() {
            let boxed = self.boxed_reg_value(builder, *reg);
            let off = 48 + (i as i32) * 8;
            builder
                .ins()
                .store(MemFlags::trusted(), boxed, self.params.exit_info_ptr, off);
        }
        self.emit_exit_return(
            builder,
            JitExitKind::Suspended as i64,
            JitSuspendReason::InterpreterBoundary as i64,
            bytecode_offset as i64,
        );
    }

    fn emit_failed_exit(
        &self,
        builder: &mut FunctionBuilder<'_>,
        regs: &[Reg],
        bytecode_offset: u32,
    ) {
        let count = regs.len().min(JIT_EXIT_MAX_NATIVE_ARGS) as i64;
        let count_val = builder.ins().iconst(types::I32, count);
        builder.ins().store(
            MemFlags::trusted(),
            count_val,
            self.params.exit_info_ptr,
            40,
        );
        for (i, reg) in regs.iter().take(JIT_EXIT_MAX_NATIVE_ARGS).enumerate() {
            let boxed = self.boxed_reg_value(builder, *reg);
            let off = 48 + (i as i32) * 8;
            builder
                .ins()
                .store(MemFlags::trusted(), boxed, self.params.exit_info_ptr, off);
        }
        self.emit_exit_return(
            builder,
            JitExitKind::Failed as i64,
            0,
            bytecode_offset as i64,
        );
    }

    /// Lower all instructions and terminator for a single block
    fn lower_block(
        &mut self,
        block_id: JitBlockId,
        builder: &mut FunctionBuilder<'_>,
    ) -> Result<(), LowerError> {
        let block = self.func.block(block_id);
        let instrs = block.instrs.clone();
        let terminator = block.terminator.clone();

        let mut terminated_early = false;
        for instr in &instrs {
            if self.lower_instr(instr, builder)? {
                terminated_early = true;
                break;
            }
        }

        // Emit Phi resolution copies before the terminator.
        // For each Phi in a successor block that sources from this block,
        // def_var the Phi's dest register with the source value from this block.
        // Cranelift's SSA construction will merge these into block params when sealed.
        if !terminated_early {
            if let Some(copies) = self.phi_copies.get(&block_id) {
                for &(phi_dest, src_reg) in copies {
                    let val = self.use_reg(builder, src_reg);
                    self.def_reg(builder, phi_dest, val);
                }
            }

            self.lower_terminator(&terminator, builder)?;
        }
        Ok(())
    }

    /// Lower a single JIT IR instruction to Cranelift IR
    fn lower_instr(
        &mut self,
        instr: &JitInstr,
        builder: &mut FunctionBuilder<'_>,
    ) -> Result<bool, LowerError> {
        match instr {
            // ===== Constants =====
            JitInstr::ConstI32 { dest, value } => {
                let val = builder.ins().iconst(types::I32, *value as i64);
                self.def_reg(builder, *dest, val);
            }
            JitInstr::ConstF64 { dest, value } => {
                let val = builder.ins().f64const(*value);
                self.def_reg(builder, *dest, val);
            }
            JitInstr::ConstBool { dest, value } => {
                let val = builder.ins().iconst(types::I8, *value as i64);
                self.def_reg(builder, *dest, val);
            }
            JitInstr::ConstNull { dest } => {
                let val = abi::emit_null(builder);
                self.def_reg(builder, *dest, val);
            }
            JitInstr::ConstString { dest, pool_index } => {
                let text = self
                    .module
                    .constants
                    .get_string(*pool_index)
                    .unwrap_or_default();
                self.lower_const_string_ptr(builder, *dest, text.as_bytes());
            }
            JitInstr::ConstStr { dest, str_index } => {
                let text = self
                    .module
                    .constants
                    .get_string(*str_index as u32)
                    .unwrap_or_default();
                self.lower_const_string_ptr(builder, *dest, text.as_bytes());
            }

            // ===== Integer Arithmetic =====
            JitInstr::IAdd { dest, left, right } => {
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().iadd(l, r);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::ISub { dest, left, right } => {
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().isub(l, r);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::IMul { dest, left, right } => {
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().imul(l, r);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::IDiv { dest, left, right } => {
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().sdiv(l, r);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::IMod { dest, left, right } => {
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().srem(l, r);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::INeg { dest, operand } => {
                let v = self.use_reg(builder, *operand);
                let result = builder.ins().ineg(v);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::IPow { dest, left, right } => {
                // No native pow for integers in Cranelift; emit a loop or deopt
                // For now, just pass through as multiply (placeholder)
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().imul(l, r);
                self.def_reg(builder, *dest, result);
            }

            // ===== Integer Bitwise =====
            JitInstr::IShl { dest, left, right } => {
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().ishl(l, r);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::IShr { dest, left, right } => {
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().sshr(l, r);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::IUshr { dest, left, right } => {
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().ushr(l, r);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::IAnd { dest, left, right } => {
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().band(l, r);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::IOr { dest, left, right } => {
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().bor(l, r);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::IXor { dest, left, right } => {
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().bxor(l, r);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::INot { dest, operand } => {
                let v = self.use_reg(builder, *operand);
                let result = builder.ins().bnot(v);
                self.def_reg(builder, *dest, result);
            }

            // ===== Float Arithmetic =====
            JitInstr::FAdd { dest, left, right } => {
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().fadd(l, r);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::FSub { dest, left, right } => {
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().fsub(l, r);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::FMul { dest, left, right } => {
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().fmul(l, r);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::FDiv { dest, left, right } => {
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().fdiv(l, r);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::FNeg { dest, operand } => {
                let v = self.use_reg(builder, *operand);
                let result = builder.ins().fneg(v);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::FPow { dest, left, right } => {
                // No native fpow in Cranelift; placeholder — would call runtime
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().fmul(l, r);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::FMod { dest, left, right } => {
                // No native fmod in Cranelift; placeholder — would call runtime fmod
                let l = self.use_reg(builder, *left);
                let _r = self.use_reg(builder, *right);
                self.def_reg(builder, *dest, l);
            }

            // ===== Integer Comparison =====
            JitInstr::ICmpEq { dest, left, right } => {
                self.lower_icmp(builder, condcodes::IntCC::Equal, *dest, *left, *right);
            }
            JitInstr::ICmpNe { dest, left, right } => {
                self.lower_icmp(builder, condcodes::IntCC::NotEqual, *dest, *left, *right);
            }
            JitInstr::ICmpLt { dest, left, right } => {
                self.lower_icmp(
                    builder,
                    condcodes::IntCC::SignedLessThan,
                    *dest,
                    *left,
                    *right,
                );
            }
            JitInstr::ICmpLe { dest, left, right } => {
                self.lower_icmp(
                    builder,
                    condcodes::IntCC::SignedLessThanOrEqual,
                    *dest,
                    *left,
                    *right,
                );
            }
            JitInstr::ICmpGt { dest, left, right } => {
                self.lower_icmp(
                    builder,
                    condcodes::IntCC::SignedGreaterThan,
                    *dest,
                    *left,
                    *right,
                );
            }
            JitInstr::ICmpGe { dest, left, right } => {
                self.lower_icmp(
                    builder,
                    condcodes::IntCC::SignedGreaterThanOrEqual,
                    *dest,
                    *left,
                    *right,
                );
            }

            // ===== Float Comparison =====
            JitInstr::FCmpEq { dest, left, right } => {
                self.lower_fcmp(builder, condcodes::FloatCC::Equal, *dest, *left, *right);
            }
            JitInstr::FCmpNe { dest, left, right } => {
                self.lower_fcmp(builder, condcodes::FloatCC::NotEqual, *dest, *left, *right);
            }
            JitInstr::FCmpLt { dest, left, right } => {
                self.lower_fcmp(builder, condcodes::FloatCC::LessThan, *dest, *left, *right);
            }
            JitInstr::FCmpLe { dest, left, right } => {
                self.lower_fcmp(
                    builder,
                    condcodes::FloatCC::LessThanOrEqual,
                    *dest,
                    *left,
                    *right,
                );
            }
            JitInstr::FCmpGt { dest, left, right } => {
                self.lower_fcmp(
                    builder,
                    condcodes::FloatCC::GreaterThan,
                    *dest,
                    *left,
                    *right,
                );
            }
            JitInstr::FCmpGe { dest, left, right } => {
                self.lower_fcmp(
                    builder,
                    condcodes::FloatCC::GreaterThanOrEqual,
                    *dest,
                    *left,
                    *right,
                );
            }

            // ===== Logical =====
            JitInstr::Not { dest, operand } => {
                let v = self.use_reg(builder, *operand);
                // Boolean not: XOR with 1
                let one = builder.ins().iconst(types::I8, 1);
                let result = builder.ins().bxor(v, one);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::And { dest, left, right } => {
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().band(l, r);
                self.def_reg(builder, *dest, result);
            }
            JitInstr::Or { dest, left, right } => {
                let l = self.use_reg(builder, *left);
                let r = self.use_reg(builder, *right);
                let result = builder.ins().bor(l, r);
                self.def_reg(builder, *dest, result);
            }

            // ===== NaN-box Conversion =====
            JitInstr::BoxI32 { dest, src } => {
                let v = self.use_reg(builder, *src);
                let boxed = abi::emit_box_i32(builder, v);
                self.def_reg(builder, *dest, boxed);
            }
            JitInstr::UnboxI32 { dest, src } => {
                let v = self.use_reg(builder, *src);
                let unboxed = abi::emit_unbox_i32(builder, v);
                self.def_reg(builder, *dest, unboxed);
            }
            JitInstr::BoxF64 { dest, src } => {
                let v = self.use_reg(builder, *src);
                let boxed = abi::emit_box_f64(builder, v);
                self.def_reg(builder, *dest, boxed);
            }
            JitInstr::UnboxF64 { dest, src } => {
                let v = self.use_reg(builder, *src);
                let unboxed = abi::emit_unbox_f64(builder, v);
                self.def_reg(builder, *dest, unboxed);
            }
            JitInstr::BoxBool { dest, src } => {
                let v = self.use_reg(builder, *src);
                let boxed = abi::emit_box_bool(builder, v);
                self.def_reg(builder, *dest, boxed);
            }
            JitInstr::UnboxBool { dest, src } => {
                let v = self.use_reg(builder, *src);
                let unboxed = abi::emit_unbox_bool(builder, v);
                self.def_reg(builder, *dest, unboxed);
            }
            JitInstr::BoxPtr { dest, src } | JitInstr::UnboxPtr { dest, src } => {
                // Ptr boxing: NAN_BOX_BASE | TAG_PTR(0) | (ptr & PAYLOAD_MASK)
                // For now, just pass through (ptr is already i64)
                let v = self.use_reg(builder, *src);
                self.def_reg(builder, *dest, v);
            }

            // ===== Local Variable Access =====
            JitInstr::LoadLocal { dest, index } => {
                // Load from locals array: locals_ptr[index * 8]
                let offset = (*index as i32) * 8;
                let val = builder.ins().load(
                    types::I64,
                    MemFlags::trusted(),
                    self.params.locals_ptr,
                    offset,
                );
                self.def_reg(builder, *dest, val);
            }
            JitInstr::StoreLocal { index, value } => {
                let raw = self.use_reg(builder, *value);
                let v = match self.func.reg_type(*value) {
                    crate::jit::ir::types::JitType::I32 => abi::emit_box_i32(builder, raw),
                    crate::jit::ir::types::JitType::F64 => abi::emit_box_f64(builder, raw),
                    crate::jit::ir::types::JitType::Bool => abi::emit_box_bool(builder, raw),
                    crate::jit::ir::types::JitType::Ptr => abi::emit_box_ptr(builder, raw),
                    _ => raw,
                };
                let offset = (*index as i32) * 8;
                builder
                    .ins()
                    .store(MemFlags::trusted(), v, self.params.locals_ptr, offset);
            }

            // ===== String Operations =====
            JitInstr::SLen {
                dest,
                string,
                stack,
                bytecode_offset,
            } => {
                let ctx = self.params.ctx_ptr;
                let is_ctx_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, ctx, 0);
                let call_block = builder.create_block();
                let fallback_block = builder.create_block();
                let done = builder.create_block();
                builder.append_block_param(done, types::I32);
                builder
                    .ins()
                    .brif(is_ctx_null, fallback_block, &[], call_block, &[]);
                builder.seal_block(call_block);

                builder.switch_to_block(call_block);
                let shared_state = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 0);
                let fn_ptr = builder
                    .ins()
                    .load(types::I64, MemFlags::trusted(), ctx, 160); // 24 + 136
                let sig = self.string_len_sig(builder);
                let string_val = self.boxed_reg_value(builder, *string);
                let call = builder
                    .ins()
                    .call_indirect(sig, fn_ptr, &[string_val, shared_state]);
                let result = builder.inst_results(call)[0];
                let fallback = builder
                    .ins()
                    .iconst(types::I32, JIT_STRING_LEN_FALLBACK_SENTINEL as i64);
                let is_fallback =
                    builder
                        .ins()
                        .icmp(condcodes::IntCC::Equal, result, fallback);
                let fast_continue = builder.create_block();
                builder
                    .ins()
                    .brif(is_fallback, fallback_block, &[], fast_continue, &[]);
                builder.seal_block(fast_continue);
                builder.switch_to_block(fast_continue);
                builder.ins().jump(done, &[ir::BlockArg::Value(result)]);

                builder.seal_block(fallback_block);
                builder.switch_to_block(fallback_block);
                self.emit_interpreter_boundary_exit(builder, stack, *bytecode_offset);

                builder.seal_block(done);
                builder.switch_to_block(done);
                let merged = builder.block_params(done)[0];
                self.def_reg(builder, *dest, merged);
            }

            // ===== Object Field Access (shape-aware helper path) =====
            JitInstr::LoadFieldExact {
                dest,
                object,
                offset,
            }
            | JitInstr::OptionalFieldExact {
                dest,
                object,
                offset,
            } => {
                let ctx = self.params.ctx_ptr;
                let is_ctx_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, ctx, 0);
                let call_block = builder.create_block();
                let null_block = builder.create_block();
                let done = builder.create_block();
                builder.append_block_param(done, types::I64);
                builder
                    .ins()
                    .brif(is_ctx_null, null_block, &[], call_block, &[]);
                builder.seal_block(call_block);
                builder.seal_block(null_block);

                builder.switch_to_block(call_block);
                let shared_state = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 0);
                let module_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 16);
                let fn_ptr = builder
                    .ins()
                    .load(types::I64, MemFlags::trusted(), ctx, 112); // 24 + 88
                let sig = self.object_get_field_sig(builder);
                let object_val = self.use_reg(builder, *object);
                let slot = builder.ins().iconst(types::I32, *offset as i64);
                let func_id = builder
                    .ins()
                    .iconst(types::I32, self.func.func_index as i64);
                let call = builder.ins().call_indirect(
                    sig,
                    fn_ptr,
                    &[object_val, slot, func_id, module_ptr, shared_state],
                );
                let result = builder.inst_results(call)[0];
                let result_arg = [ir::BlockArg::Value(result)];
                builder.ins().jump(done, &result_arg);

                builder.switch_to_block(null_block);
                let null = abi::emit_null(builder);
                let null_arg = [ir::BlockArg::Value(null)];
                builder.ins().jump(done, &null_arg);

                builder.seal_block(done);
                builder.switch_to_block(done);
                let merged = builder.block_params(done)[0];
                self.def_reg(builder, *dest, merged);
            }
            JitInstr::LoadFieldShape {
                dest,
                object,
                shape_id,
                offset,
                optional,
                stack,
                bytecode_offset,
            } => {
                let ctx = self.params.ctx_ptr;
                let is_ctx_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, ctx, 0);
                let call_block = builder.create_block();
                let fallback_block = builder.create_block();
                let done = builder.create_block();
                builder.append_block_param(done, types::I64);
                builder
                    .ins()
                    .brif(is_ctx_null, fallback_block, &[], call_block, &[]);
                builder.seal_block(call_block);

                builder.switch_to_block(call_block);
                let shared_state = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 0);
                let module_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 16);
                let fn_ptr = builder
                    .ins()
                    .load(types::I64, MemFlags::trusted(), ctx, 144); // 24 + 120
                let sig = self.object_get_shape_field_sig(builder);
                let object_val = self.boxed_reg_value(builder, *object);
                let shape_val = builder.ins().iconst(types::I64, *shape_id as i64);
                let slot_val = builder.ins().iconst(types::I32, *offset as i64);
                let optional_val = builder.ins().iconst(types::I8, if *optional { 1 } else { 0 });
                let func_id = builder
                    .ins()
                    .iconst(types::I32, self.func.func_index as i64);
                let call = builder.ins().call_indirect(
                    sig,
                    fn_ptr,
                    &[
                        object_val,
                        shape_val,
                        slot_val,
                        optional_val,
                        func_id,
                        module_ptr,
                        shared_state,
                    ],
                );
                let result = builder.inst_results(call)[0];
                let fallback = builder
                    .ins()
                    .iconst(types::I64, JIT_SHAPE_FIELD_FALLBACK_SENTINEL as i64);
                let is_fallback =
                    builder
                        .ins()
                        .icmp(condcodes::IntCC::Equal, result, fallback);
                let fast_continue = builder.create_block();
                builder
                    .ins()
                    .brif(is_fallback, fallback_block, &[], fast_continue, &[]);
                builder.seal_block(fallback_block);
                builder.seal_block(fast_continue);

                builder.switch_to_block(fast_continue);
                builder.ins().jump(done, &[ir::BlockArg::Value(result)]);
                builder.seal_block(done);

                builder.switch_to_block(fallback_block);
                self.emit_interpreter_boundary_exit(builder, stack, *bytecode_offset);

                builder.switch_to_block(done);
                let merged = builder.block_params(done)[0];
                self.def_reg(builder, *dest, merged);
            }
            JitInstr::NewObject {
                dest,
                nominal_type_id,
                bytecode_offset,
            } => {
                let ctx = self.params.ctx_ptr;
                let is_ctx_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, ctx, 0);
                let call_block = builder.create_block();
                let fallback_block = builder.create_block();
                let success_block = builder.create_block();
                builder
                    .ins()
                    .brif(is_ctx_null, fallback_block, &[], call_block, &[]);
                builder.seal_block(call_block);

                builder.switch_to_block(call_block);
                let shared_state = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 0);
                let module_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 16);
                let fn_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 24);
                let sig = self.alloc_object_sig(builder);
                let nominal_type_id_val = builder.ins().iconst(types::I32, *nominal_type_id as i64);
                let call = builder.ins().call_indirect(
                    sig,
                    fn_ptr,
                    &[nominal_type_id_val, module_ptr, shared_state],
                );
                let object_ptr = builder.inst_results(call)[0];
                let is_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, object_ptr, 0);
                builder
                    .ins()
                    .brif(is_null, fallback_block, &[], success_block, &[]);
                builder.seal_block(fallback_block);
                builder.seal_block(success_block);

                builder.switch_to_block(fallback_block);
                self.emit_interpreter_boundary_exit(builder, &[], *bytecode_offset);

                builder.switch_to_block(success_block);
                self.def_reg(builder, *dest, object_ptr);
            }
            JitInstr::InstanceOf {
                dest,
                object,
                nominal_type_id,
            } => {
                let ctx = self.params.ctx_ptr;
                let is_ctx_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, ctx, 0);
                let call_block = builder.create_block();
                let null_block = builder.create_block();
                let done = builder.create_block();
                builder.append_block_param(done, types::I8);
                builder
                    .ins()
                    .brif(is_ctx_null, null_block, &[], call_block, &[]);
                builder.seal_block(call_block);
                builder.seal_block(null_block);

                builder.switch_to_block(call_block);
                let shared_state = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 0);
                let module_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 16);
                let fn_ptr = builder
                    .ins()
                    .load(types::I64, MemFlags::trusted(), ctx, 136); // 24 + 112
                let sig = self.object_is_nominal_sig(builder);
                let object_val = self.boxed_reg_value(builder, *object);
                let nominal_type_id_val = builder.ins().iconst(types::I32, *nominal_type_id as i64);
                let call = builder.ins().call_indirect(
                    sig,
                    fn_ptr,
                    &[object_val, nominal_type_id_val, module_ptr, shared_state],
                );
                let result = builder.inst_results(call)[0];
                let result_arg = [ir::BlockArg::Value(result)];
                builder.ins().jump(done, &result_arg);

                builder.switch_to_block(null_block);
                let false_val = builder.ins().iconst(types::I8, 0);
                let false_arg = [ir::BlockArg::Value(false_val)];
                builder.ins().jump(done, &false_arg);

                builder.seal_block(done);
                builder.switch_to_block(done);
                let merged = builder.block_params(done)[0];
                self.def_reg(builder, *dest, merged);
            }
            JitInstr::Cast {
                dest,
                object,
                nominal_type_id,
                bytecode_offset,
            } => {
                let ctx = self.params.ctx_ptr;
                let is_ctx_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, ctx, 0);
                let call_block = builder.create_block();
                let fallback_block = builder.create_block();
                let success_block = builder.create_block();
                builder
                    .ins()
                    .brif(is_ctx_null, fallback_block, &[], call_block, &[]);
                builder.seal_block(call_block);

                builder.switch_to_block(call_block);
                let shared_state = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 0);
                let module_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 16);
                let fn_ptr = builder
                    .ins()
                    .load(types::I64, MemFlags::trusted(), ctx, 136); // 24 + 112
                let sig = self.object_is_nominal_sig(builder);
                let object_val = self.boxed_reg_value(builder, *object);
                let nominal_type_id_val = builder.ins().iconst(types::I32, *nominal_type_id as i64);
                let call = builder.ins().call_indirect(
                    sig,
                    fn_ptr,
                    &[object_val, nominal_type_id_val, module_ptr, shared_state],
                );
                let result = builder.inst_results(call)[0];
                builder
                    .ins()
                    .brif(result, success_block, &[], fallback_block, &[]);
                builder.seal_block(fallback_block);
                builder.seal_block(success_block);

                builder.switch_to_block(fallback_block);
                self.emit_interpreter_boundary_exit(builder, &[*object], *bytecode_offset);

                builder.switch_to_block(success_block);
                let object_val = self.boxed_reg_value(builder, *object);
                self.def_reg(builder, *dest, object_val);
            }
            JitInstr::ImplementsShape {
                dest,
                object,
                shape_id,
                ..
            } => {
                let ctx = self.params.ctx_ptr;
                let is_ctx_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, ctx, 0);
                let call_block = builder.create_block();
                let null_block = builder.create_block();
                let done = builder.create_block();
                builder.append_block_param(done, types::I8);
                builder
                    .ins()
                    .brif(is_ctx_null, null_block, &[], call_block, &[]);
                builder.seal_block(call_block);
                builder.seal_block(null_block);

                builder.switch_to_block(call_block);
                let shared_state = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 0);
                let fn_ptr = builder
                    .ins()
                    .load(types::I64, MemFlags::trusted(), ctx, 128); // 24 + 104
                let sig = self.object_implements_shape_sig(builder);
                let object_val = self.boxed_reg_value(builder, *object);
                let shape_id_val = builder.ins().iconst(types::I64, *shape_id as i64);
                let call = builder.ins().call_indirect(
                    sig,
                    fn_ptr,
                    &[object_val, shape_id_val, shared_state],
                );
                let result = builder.inst_results(call)[0];
                let result_arg = [ir::BlockArg::Value(result)];
                builder.ins().jump(done, &result_arg);

                builder.switch_to_block(null_block);
                let false_val = builder.ins().iconst(types::I8, 0);
                let false_arg = [ir::BlockArg::Value(false_val)];
                builder.ins().jump(done, &false_arg);

                builder.seal_block(done);
                builder.switch_to_block(done);
                let merged = builder.block_params(done)[0];
                self.def_reg(builder, *dest, merged);
            }
            JitInstr::CastShape {
                dest,
                object,
                shape_id,
                bytecode_offset,
            } => {
                let ctx = self.params.ctx_ptr;
                let is_ctx_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, ctx, 0);
                let call_block = builder.create_block();
                let fallback_block = builder.create_block();
                let success_block = builder.create_block();
                builder
                    .ins()
                    .brif(is_ctx_null, fallback_block, &[], call_block, &[]);
                builder.seal_block(call_block);

                builder.switch_to_block(call_block);
                let shared_state = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 0);
                let fn_ptr = builder
                    .ins()
                    .load(types::I64, MemFlags::trusted(), ctx, 128); // 24 + 104
                let sig = self.object_implements_shape_sig(builder);
                let object_val = self.boxed_reg_value(builder, *object);
                let shape_id_val = builder.ins().iconst(types::I64, *shape_id as i64);
                let call = builder.ins().call_indirect(
                    sig,
                    fn_ptr,
                    &[object_val, shape_id_val, shared_state],
                );
                let result = builder.inst_results(call)[0];
                builder
                    .ins()
                    .brif(result, success_block, &[], fallback_block, &[]);
                builder.seal_block(fallback_block);
                builder.seal_block(success_block);

                builder.switch_to_block(fallback_block);
                self.emit_interpreter_boundary_exit(builder, &[*object], *bytecode_offset);

                builder.switch_to_block(success_block);
                let object_val = self.boxed_reg_value(builder, *object);
                self.def_reg(builder, *dest, object_val);
            }
            JitInstr::StoreFieldShape {
                object,
                shape_id,
                offset,
                value,
                stack,
                bytecode_offset,
            } => {
                let ctx = self.params.ctx_ptr;
                let is_ctx_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, ctx, 0);
                let call_block = builder.create_block();
                let fallback_block = builder.create_block();
                let success_block = builder.create_block();
                builder
                    .ins()
                    .brif(is_ctx_null, fallback_block, &[], call_block, &[]);
                builder.seal_block(call_block);

                builder.switch_to_block(call_block);
                let shared_state = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 0);
                let module_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 16);
                let fn_ptr = builder
                    .ins()
                    .load(types::I64, MemFlags::trusted(), ctx, 152); // 24 + 128
                let sig = self.object_set_shape_field_sig(builder);
                let object_val = self.boxed_reg_value(builder, *object);
                let shape_val = builder.ins().iconst(types::I64, *shape_id as i64);
                let slot_val = builder.ins().iconst(types::I32, *offset as i64);
                let value_val = self.boxed_reg_value(builder, *value);
                let func_id = builder
                    .ins()
                    .iconst(types::I32, self.func.func_index as i64);
                let call = builder.ins().call_indirect(
                    sig,
                    fn_ptr,
                    &[
                        object_val,
                        shape_val,
                        slot_val,
                        value_val,
                        func_id,
                        module_ptr,
                        shared_state,
                    ],
                );
                let result = builder.inst_results(call)[0];
                let is_success = builder.ins().icmp_imm(condcodes::IntCC::Equal, result, 1);
                builder
                    .ins()
                    .brif(is_success, success_block, &[], fallback_block, &[]);
                builder.seal_block(fallback_block);
                builder.seal_block(success_block);

                builder.switch_to_block(fallback_block);
                self.emit_interpreter_boundary_exit(builder, stack, *bytecode_offset);

                builder.switch_to_block(success_block);
            }
            JitInstr::Call {
                dest,
                func_index,
                closure,
                args,
                stack,
                bytecode_offset,
            } => {
                let ctx = self.params.ctx_ptr;
                let is_ctx_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, ctx, 0);
                let call_block = builder.create_block();
                let fallback_block = builder.create_block();
                let exception_block = builder.create_block();
                let success_block = builder.create_block();
                let done = builder.create_block();
                builder.append_block_param(done, types::I64);
                builder.ins().brif(is_ctx_null, fallback_block, &[], call_block, &[]);
                builder.seal_block(call_block);

                builder.switch_to_block(call_block);
                let shared_state = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 0);
                let module_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 16);
                let fn_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 72);
                let sig = self.interpreter_call_sig(builder);
                let arg_count = args.len().min(u16::MAX as usize);
                let args_slot = builder.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    (arg_count * 8) as u32,
                    3,
                ));
                let args_ptr = builder.ins().stack_addr(types::I64, args_slot, 0);
                for (i, reg) in args.iter().take(arg_count).enumerate() {
                    let boxed = self.boxed_reg_value(builder, *reg);
                    builder.ins().store(MemFlags::trusted(), boxed, args_ptr, (i as i32) * 8);
                }
                let opcode_val = builder.ins().iconst(types::I8, Opcode::Call as u8 as i64);
                let operand_u64 = builder.ins().iconst(types::I64, 0);
                let operand_u32 = builder.ins().iconst(types::I32, *func_index as i64);
                let receiver_val = closure
                    .map(|reg| self.boxed_reg_value(builder, reg))
                    .unwrap_or_else(|| abi::emit_null(builder));
                let arg_count_val = builder.ins().iconst(types::I16, arg_count as i64);
                let call = builder.ins().call_indirect(
                    sig,
                    fn_ptr,
                    &[
                        opcode_val,
                        operand_u64,
                        operand_u32,
                        receiver_val,
                        args_ptr,
                        arg_count_val,
                        module_ptr,
                        shared_state,
                    ],
                );
                let result = builder.inst_results(call)[0];
                let fallback = builder
                    .ins()
                    .iconst(types::I64, JIT_INTERPRETER_FALLBACK_SENTINEL as i64);
                let exception = builder
                    .ins()
                    .iconst(types::I64, JIT_INTERPRETER_EXCEPTION_SENTINEL as i64);
                let is_fallback = builder.ins().icmp(condcodes::IntCC::Equal, result, fallback);
                let is_exception = builder.ins().icmp(condcodes::IntCC::Equal, result, exception);
                let after_fallback_check = builder.create_block();
                builder
                    .ins()
                    .brif(is_fallback, fallback_block, &[], after_fallback_check, &[]);
                builder.seal_block(after_fallback_check);
                builder.switch_to_block(after_fallback_check);
                builder
                    .ins()
                    .brif(is_exception, exception_block, &[], success_block, &[]);
                builder.seal_block(fallback_block);
                builder.seal_block(exception_block);
                builder.seal_block(success_block);

                builder.switch_to_block(fallback_block);
                self.emit_interpreter_boundary_exit(builder, stack, *bytecode_offset);

                builder.switch_to_block(exception_block);
                self.emit_failed_exit(builder, stack, *bytecode_offset);

                builder.switch_to_block(success_block);
                builder.ins().jump(done, &[ir::BlockArg::Value(result)]);

                builder.seal_block(done);
                builder.switch_to_block(done);
                if let Some(dest) = dest {
                    let merged = builder.block_params(done)[0];
                    self.def_reg(builder, *dest, merged);
                }
            }
            JitInstr::CallMethodExact {
                dest,
                method_index,
                receiver,
                args,
                optional,
                stack,
                bytecode_offset,
            } => {
                let ctx = self.params.ctx_ptr;
                let is_ctx_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, ctx, 0);
                let call_block = builder.create_block();
                let fallback_block = builder.create_block();
                let exception_block = builder.create_block();
                let success_block = builder.create_block();
                let done = builder.create_block();
                builder.append_block_param(done, types::I64);
                builder.ins().brif(is_ctx_null, fallback_block, &[], call_block, &[]);
                builder.seal_block(call_block);

                builder.switch_to_block(call_block);
                let shared_state = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 0);
                let module_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 16);
                let fn_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 72);
                let sig = self.interpreter_call_sig(builder);
                let arg_count = args.len().min(u16::MAX as usize);
                let args_slot = builder.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    (arg_count * 8) as u32,
                    3,
                ));
                let args_ptr = builder.ins().stack_addr(types::I64, args_slot, 0);
                for (i, reg) in args.iter().take(arg_count).enumerate() {
                    let boxed = self.boxed_reg_value(builder, *reg);
                    builder.ins().store(MemFlags::trusted(), boxed, args_ptr, (i as i32) * 8);
                }
                let opcode_val = builder.ins().iconst(
                    types::I8,
                    if *optional {
                        Opcode::OptionalCallMethodExact as u8 as i64
                    } else {
                        Opcode::CallMethodExact as u8 as i64
                    },
                );
                let operand_u64 = builder.ins().iconst(types::I64, 0);
                let operand_u32 = builder.ins().iconst(types::I32, *method_index as i64);
                let receiver_val = self.boxed_reg_value(builder, *receiver);
                let arg_count_val = builder.ins().iconst(types::I16, arg_count as i64);
                let call = builder.ins().call_indirect(
                    sig,
                    fn_ptr,
                    &[
                        opcode_val,
                        operand_u64,
                        operand_u32,
                        receiver_val,
                        args_ptr,
                        arg_count_val,
                        module_ptr,
                        shared_state,
                    ],
                );
                let result = builder.inst_results(call)[0];
                let fallback = builder
                    .ins()
                    .iconst(types::I64, JIT_INTERPRETER_FALLBACK_SENTINEL as i64);
                let exception = builder
                    .ins()
                    .iconst(types::I64, JIT_INTERPRETER_EXCEPTION_SENTINEL as i64);
                let is_fallback = builder.ins().icmp(condcodes::IntCC::Equal, result, fallback);
                let is_exception = builder.ins().icmp(condcodes::IntCC::Equal, result, exception);
                let after_fallback_check = builder.create_block();
                builder
                    .ins()
                    .brif(is_fallback, fallback_block, &[], after_fallback_check, &[]);
                builder.seal_block(after_fallback_check);
                builder.switch_to_block(after_fallback_check);
                builder
                    .ins()
                    .brif(is_exception, exception_block, &[], success_block, &[]);
                builder.seal_block(fallback_block);
                builder.seal_block(exception_block);
                builder.seal_block(success_block);

                builder.switch_to_block(fallback_block);
                self.emit_interpreter_boundary_exit(builder, stack, *bytecode_offset);
                builder.switch_to_block(exception_block);
                self.emit_failed_exit(builder, stack, *bytecode_offset);
                builder.switch_to_block(success_block);
                builder.ins().jump(done, &[ir::BlockArg::Value(result)]);
                builder.seal_block(done);
                builder.switch_to_block(done);
                if let Some(dest) = dest {
                    let merged = builder.block_params(done)[0];
                    self.def_reg(builder, *dest, merged);
                }
            }
            JitInstr::CallMethodShape {
                dest,
                shape_id,
                method_index,
                receiver,
                args,
                optional,
                stack,
                bytecode_offset,
            } => {
                let ctx = self.params.ctx_ptr;
                let is_ctx_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, ctx, 0);
                let call_block = builder.create_block();
                let fallback_block = builder.create_block();
                let exception_block = builder.create_block();
                let success_block = builder.create_block();
                let done = builder.create_block();
                builder.append_block_param(done, types::I64);
                builder.ins().brif(is_ctx_null, fallback_block, &[], call_block, &[]);
                builder.seal_block(call_block);

                builder.switch_to_block(call_block);
                let shared_state = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 0);
                let module_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 16);
                let fn_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 72);
                let sig = self.interpreter_call_sig(builder);
                let arg_count = args.len().min(u16::MAX as usize);
                let args_slot = builder.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    (arg_count * 8) as u32,
                    3,
                ));
                let args_ptr = builder.ins().stack_addr(types::I64, args_slot, 0);
                for (i, reg) in args.iter().take(arg_count).enumerate() {
                    let boxed = self.boxed_reg_value(builder, *reg);
                    builder.ins().store(MemFlags::trusted(), boxed, args_ptr, (i as i32) * 8);
                }
                let opcode_val = builder.ins().iconst(
                    types::I8,
                    if *optional {
                        Opcode::OptionalCallMethodShape as u8 as i64
                    } else {
                        Opcode::CallMethodShape as u8 as i64
                    },
                );
                let operand_u64 = builder.ins().iconst(types::I64, *shape_id as i64);
                let operand_u32 = builder.ins().iconst(types::I32, *method_index as i64);
                let receiver_val = self.boxed_reg_value(builder, *receiver);
                let arg_count_val = builder.ins().iconst(types::I16, arg_count as i64);
                let call = builder.ins().call_indirect(
                    sig,
                    fn_ptr,
                    &[
                        opcode_val,
                        operand_u64,
                        operand_u32,
                        receiver_val,
                        args_ptr,
                        arg_count_val,
                        module_ptr,
                        shared_state,
                    ],
                );
                let result = builder.inst_results(call)[0];
                let fallback = builder
                    .ins()
                    .iconst(types::I64, JIT_INTERPRETER_FALLBACK_SENTINEL as i64);
                let exception = builder
                    .ins()
                    .iconst(types::I64, JIT_INTERPRETER_EXCEPTION_SENTINEL as i64);
                let is_fallback = builder.ins().icmp(condcodes::IntCC::Equal, result, fallback);
                let is_exception = builder.ins().icmp(condcodes::IntCC::Equal, result, exception);
                let after_fallback_check = builder.create_block();
                builder
                    .ins()
                    .brif(is_fallback, fallback_block, &[], after_fallback_check, &[]);
                builder.seal_block(after_fallback_check);
                builder.switch_to_block(after_fallback_check);
                builder
                    .ins()
                    .brif(is_exception, exception_block, &[], success_block, &[]);
                builder.seal_block(fallback_block);
                builder.seal_block(exception_block);
                builder.seal_block(success_block);

                builder.switch_to_block(fallback_block);
                self.emit_interpreter_boundary_exit(builder, stack, *bytecode_offset);
                builder.switch_to_block(exception_block);
                self.emit_failed_exit(builder, stack, *bytecode_offset);
                builder.switch_to_block(success_block);
                builder.ins().jump(done, &[ir::BlockArg::Value(result)]);
                builder.seal_block(done);
                builder.switch_to_block(done);
                if let Some(dest) = dest {
                    let merged = builder.block_params(done)[0];
                    self.def_reg(builder, *dest, merged);
                }
            }
            JitInstr::ConstructType {
                dest,
                nominal_type_id,
                object,
                args,
                stack,
                bytecode_offset,
            } => {
                let ctx = self.params.ctx_ptr;
                let is_ctx_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, ctx, 0);
                let call_block = builder.create_block();
                let fallback_block = builder.create_block();
                let exception_block = builder.create_block();
                let success_block = builder.create_block();
                let done = builder.create_block();
                builder.append_block_param(done, types::I64);
                builder.ins().brif(is_ctx_null, fallback_block, &[], call_block, &[]);
                builder.seal_block(call_block);
                builder.switch_to_block(call_block);
                let shared_state = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 0);
                let module_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 16);
                let fn_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 72);
                let sig = self.interpreter_call_sig(builder);
                let arg_count = args.len().min(u16::MAX as usize);
                let args_slot = builder.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    (arg_count * 8) as u32,
                    3,
                ));
                let args_ptr = builder.ins().stack_addr(types::I64, args_slot, 0);
                for (i, reg) in args.iter().take(arg_count).enumerate() {
                    let boxed = self.boxed_reg_value(builder, *reg);
                    builder.ins().store(MemFlags::trusted(), boxed, args_ptr, (i as i32) * 8);
                }
                let opcode_val = builder.ins().iconst(types::I8, Opcode::ConstructType as u8 as i64);
                let operand_u64 = builder.ins().iconst(types::I64, 0);
                let operand_u32 = builder.ins().iconst(types::I32, *nominal_type_id as i64);
                let receiver_val = self.boxed_reg_value(builder, *object);
                let arg_count_val = builder.ins().iconst(types::I16, arg_count as i64);
                let call = builder.ins().call_indirect(
                    sig,
                    fn_ptr,
                    &[
                        opcode_val,
                        operand_u64,
                        operand_u32,
                        receiver_val,
                        args_ptr,
                        arg_count_val,
                        module_ptr,
                        shared_state,
                    ],
                );
                let result = builder.inst_results(call)[0];
                let fallback = builder
                    .ins()
                    .iconst(types::I64, JIT_INTERPRETER_FALLBACK_SENTINEL as i64);
                let exception = builder
                    .ins()
                    .iconst(types::I64, JIT_INTERPRETER_EXCEPTION_SENTINEL as i64);
                let is_fallback = builder.ins().icmp(condcodes::IntCC::Equal, result, fallback);
                let is_exception = builder.ins().icmp(condcodes::IntCC::Equal, result, exception);
                let after_fallback_check = builder.create_block();
                builder
                    .ins()
                    .brif(is_fallback, fallback_block, &[], after_fallback_check, &[]);
                builder.seal_block(after_fallback_check);
                builder.switch_to_block(after_fallback_check);
                builder
                    .ins()
                    .brif(is_exception, exception_block, &[], success_block, &[]);
                builder.seal_block(fallback_block);
                builder.seal_block(exception_block);
                builder.seal_block(success_block);

                builder.switch_to_block(fallback_block);
                self.emit_interpreter_boundary_exit(builder, stack, *bytecode_offset);
                builder.switch_to_block(exception_block);
                self.emit_failed_exit(builder, stack, *bytecode_offset);
                builder.switch_to_block(success_block);
                builder.ins().jump(done, &[ir::BlockArg::Value(result)]);
                builder.seal_block(done);
                builder.switch_to_block(done);
                let merged = builder.block_params(done)[0];
                let coerced = self.coerce_boxed_value_for_reg(builder, *dest, merged);
                self.def_reg(builder, *dest, coerced);
            }
            JitInstr::CallConstructor {
                dest,
                nominal_type_id,
                args,
                stack,
                bytecode_offset,
            }
            | JitInstr::CallStatic {
                dest: Some(dest),
                func_index: nominal_type_id,
                args,
                stack,
                bytecode_offset,
            } => {
                let call_opcode = if matches!(instr, JitInstr::CallConstructor { .. }) {
                    Opcode::CallConstructor
                } else {
                    Opcode::CallStatic
                };
                let ctx = self.params.ctx_ptr;
                let is_ctx_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, ctx, 0);
                let call_block = builder.create_block();
                let fallback_block = builder.create_block();
                let exception_block = builder.create_block();
                let success_block = builder.create_block();
                let done = builder.create_block();
                builder.append_block_param(done, types::I64);
                builder.ins().brif(is_ctx_null, fallback_block, &[], call_block, &[]);
                builder.seal_block(call_block);

                builder.switch_to_block(call_block);
                let shared_state = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 0);
                let module_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 16);
                let fn_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 72);
                let sig = self.interpreter_call_sig(builder);
                let arg_count = args.len().min(u16::MAX as usize);
                let args_slot = builder.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    (arg_count * 8) as u32,
                    3,
                ));
                let args_ptr = builder.ins().stack_addr(types::I64, args_slot, 0);
                for (i, reg) in args.iter().take(arg_count).enumerate() {
                    let boxed = self.boxed_reg_value(builder, *reg);
                    builder.ins().store(MemFlags::trusted(), boxed, args_ptr, (i as i32) * 8);
                }
                let opcode_val = builder.ins().iconst(types::I8, call_opcode as u8 as i64);
                let operand_u64 = builder.ins().iconst(types::I64, 0);
                let operand_u32 = builder.ins().iconst(types::I32, *nominal_type_id as i64);
                let receiver_val = abi::emit_null(builder);
                let arg_count_val = builder.ins().iconst(types::I16, arg_count as i64);
                let call = builder.ins().call_indirect(
                    sig,
                    fn_ptr,
                    &[
                        opcode_val,
                        operand_u64,
                        operand_u32,
                        receiver_val,
                        args_ptr,
                        arg_count_val,
                        module_ptr,
                        shared_state,
                    ],
                );
                let result = builder.inst_results(call)[0];
                let fallback = builder
                    .ins()
                    .iconst(types::I64, JIT_INTERPRETER_FALLBACK_SENTINEL as i64);
                let exception = builder
                    .ins()
                    .iconst(types::I64, JIT_INTERPRETER_EXCEPTION_SENTINEL as i64);
                let is_fallback = builder.ins().icmp(condcodes::IntCC::Equal, result, fallback);
                let is_exception = builder.ins().icmp(condcodes::IntCC::Equal, result, exception);
                let after_fallback_check = builder.create_block();
                builder
                    .ins()
                    .brif(is_fallback, fallback_block, &[], after_fallback_check, &[]);
                builder.seal_block(after_fallback_check);
                builder.switch_to_block(after_fallback_check);
                builder
                    .ins()
                    .brif(is_exception, exception_block, &[], success_block, &[]);
                builder.seal_block(fallback_block);
                builder.seal_block(exception_block);
                builder.seal_block(success_block);

                builder.switch_to_block(fallback_block);
                self.emit_interpreter_boundary_exit(builder, stack, *bytecode_offset);
                builder.switch_to_block(exception_block);
                self.emit_failed_exit(builder, stack, *bytecode_offset);
                builder.switch_to_block(success_block);
                builder.ins().jump(done, &[ir::BlockArg::Value(result)]);
                builder.seal_block(done);
                builder.switch_to_block(done);
                let merged = builder.block_params(done)[0];
                let coerced = self.coerce_boxed_value_for_reg(builder, *dest, merged);
                self.def_reg(builder, *dest, coerced);
            }
            JitInstr::CallSuper {
                dest,
                nominal_type_id,
                receiver,
                args,
                stack,
                bytecode_offset,
            } => {
                let ctx = self.params.ctx_ptr;
                let is_ctx_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, ctx, 0);
                let call_block = builder.create_block();
                let fallback_block = builder.create_block();
                let exception_block = builder.create_block();
                let success_block = builder.create_block();
                let done = builder.create_block();
                builder.append_block_param(done, types::I64);
                builder.ins().brif(is_ctx_null, fallback_block, &[], call_block, &[]);
                builder.seal_block(call_block);
                builder.switch_to_block(call_block);
                let shared_state = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 0);
                let module_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 16);
                let fn_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 72);
                let sig = self.interpreter_call_sig(builder);
                let arg_count = args.len().min(u16::MAX as usize);
                let args_slot = builder.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    (arg_count * 8) as u32,
                    3,
                ));
                let args_ptr = builder.ins().stack_addr(types::I64, args_slot, 0);
                for (i, reg) in args.iter().take(arg_count).enumerate() {
                    let boxed = self.boxed_reg_value(builder, *reg);
                    builder.ins().store(MemFlags::trusted(), boxed, args_ptr, (i as i32) * 8);
                }
                let opcode_val = builder.ins().iconst(types::I8, Opcode::CallSuper as u8 as i64);
                let operand_u64 = builder.ins().iconst(types::I64, 0);
                let operand_u32 = builder.ins().iconst(types::I32, *nominal_type_id as i64);
                let receiver_val = self.boxed_reg_value(builder, *receiver);
                let arg_count_val = builder.ins().iconst(types::I16, arg_count as i64);
                let call = builder.ins().call_indirect(
                    sig,
                    fn_ptr,
                    &[
                        opcode_val,
                        operand_u64,
                        operand_u32,
                        receiver_val,
                        args_ptr,
                        arg_count_val,
                        module_ptr,
                        shared_state,
                    ],
                );
                let result = builder.inst_results(call)[0];
                let fallback = builder
                    .ins()
                    .iconst(types::I64, JIT_INTERPRETER_FALLBACK_SENTINEL as i64);
                let exception = builder
                    .ins()
                    .iconst(types::I64, JIT_INTERPRETER_EXCEPTION_SENTINEL as i64);
                let is_fallback = builder.ins().icmp(condcodes::IntCC::Equal, result, fallback);
                let is_exception = builder.ins().icmp(condcodes::IntCC::Equal, result, exception);
                let after_fallback_check = builder.create_block();
                builder
                    .ins()
                    .brif(is_fallback, fallback_block, &[], after_fallback_check, &[]);
                builder.seal_block(after_fallback_check);
                builder.switch_to_block(after_fallback_check);
                builder
                    .ins()
                    .brif(is_exception, exception_block, &[], success_block, &[]);
                builder.seal_block(fallback_block);
                builder.seal_block(exception_block);
                builder.seal_block(success_block);

                builder.switch_to_block(fallback_block);
                self.emit_interpreter_boundary_exit(builder, stack, *bytecode_offset);
                builder.switch_to_block(exception_block);
                self.emit_failed_exit(builder, stack, *bytecode_offset);
                builder.switch_to_block(success_block);
                builder.ins().jump(done, &[ir::BlockArg::Value(result)]);
                builder.seal_block(done);
                builder.switch_to_block(done);
                if let Some(dest) = dest {
                    let merged = builder.block_params(done)[0];
                    self.def_reg(builder, *dest, merged);
                }
            }

            JitInstr::InterpreterBoundary {
                bytecode_offset,
                stack,
                ..
            } => {
                self.emit_interpreter_boundary_exit(builder, stack, *bytecode_offset);
                return Ok(true);
            }

            // ===== Move / Phi =====
            JitInstr::Move { dest, src } => {
                let v = self.use_reg(builder, *src);
                self.def_reg(builder, *dest, v);
            }
            JitInstr::Phi { .. } => {
                // Phi resolution is handled by def_var copies in predecessor blocks
                // (see phi_copies in lower_block). Cranelift's SSA construction
                // merges the values automatically when the block is sealed.
            }

            // ===== Runtime Integration =====
            JitInstr::GcSafepoint { .. } => {
                // if (ctx != null) helpers.safepoint_poll(shared_state)
                // RuntimeContext: shared_state@0, current_task@8, module@16, helpers@24
                // RuntimeHelperTable: safepoint_poll @ +24
                let ctx = self.params.ctx_ptr;
                let is_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, ctx, 0);
                let skip = builder.create_block();
                let do_poll = builder.create_block();
                builder.ins().brif(is_null, skip, &[], do_poll, &[]);
                builder.seal_block(do_poll);

                builder.switch_to_block(do_poll);
                let shared_state = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 0);
                let fn_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 48); // 24 + 24
                let sig = self.safepoint_sig(builder);
                builder.ins().call_indirect(sig, fn_ptr, &[shared_state]);
                builder.ins().jump(skip, &[]);
                builder.seal_block(skip);

                builder.switch_to_block(skip);
            }
            JitInstr::CheckPreemption { bytecode_offset } => {
                // if (ctx != null && helpers.check_preemption(current_task)) {
                //   exit.kind = Suspended
                //   exit.suspend_reason = 1 (preemption)
                //   return null
                // }
                let ctx = self.params.ctx_ptr;
                let is_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, ctx, 0);
                let cont = builder.create_block();
                let do_check = builder.create_block();
                builder.ins().brif(is_null, cont, &[], do_check, &[]);
                builder.seal_block(do_check);

                builder.switch_to_block(do_check);
                let current_task = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 8);
                let fn_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 56); // 24 + 32
                let sig = self.check_preemption_sig(builder);
                let call = builder.ins().call_indirect(sig, fn_ptr, &[current_task]);
                let should_preempt = builder.inst_results(call)[0];
                let exit = builder.create_block();
                builder.ins().brif(should_preempt, exit, &[], cont, &[]);
                builder.seal_block(exit);
                builder.seal_block(cont);

                builder.switch_to_block(exit);
                self.emit_exit_return(
                    builder,
                    JitExitKind::Suspended as i64,
                    JitSuspendReason::Preemption as i64,
                    *bytecode_offset as i64,
                );
                builder.switch_to_block(cont);
            }
            JitInstr::CallNative {
                native_id,
                bytecode_offset,
                args,
                dest,
            } => {
                if args.is_empty() {
                    // Zero-arg fast path:
                    // If runtime context is available, dispatch helper-native directly.
                    // If helper reports suspend sentinel, hand off to interpreter boundary.
                    // Otherwise continue with immediate native result.
                    let ctx = self.params.ctx_ptr;
                    let is_ctx_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, ctx, 0);
                    let fallback_suspend = builder.create_block();
                    let do_dispatch = builder.create_block();
                    builder
                        .ins()
                        .brif(is_ctx_null, fallback_suspend, &[], do_dispatch, &[]);
                    builder.seal_block(do_dispatch);
                    builder.switch_to_block(do_dispatch);

                    let shared_state = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 0);
                    let fn_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 64); // 24 + 40
                    let sig = self.native_call_dispatch_sig(builder);
                    let native_id_val = builder.ins().iconst(types::I16, *native_id as i64);
                    let null_args_ptr = builder.ins().iconst(types::I64, 0);
                    let zero_arg_count = builder.ins().iconst(types::I8, 0);
                    let call = builder.ins().call_indirect(
                        sig,
                        fn_ptr,
                        &[native_id_val, null_args_ptr, zero_arg_count, shared_state],
                    );
                    let result = builder.inst_results(call)[0];
                    let suspend_sentinel = builder
                        .ins()
                        .iconst(types::I64, JIT_NATIVE_SUSPEND_SENTINEL as i64);
                    let is_suspend =
                        builder
                            .ins()
                            .icmp(condcodes::IntCC::Equal, result, suspend_sentinel);
                    let suspend_exit = builder.create_block();
                    let fast_continue = builder.create_block();
                    builder
                        .ins()
                        .brif(is_suspend, suspend_exit, &[], fast_continue, &[]);
                    builder.seal_block(suspend_exit);
                    builder.seal_block(fast_continue);
                    builder.seal_block(fallback_suspend);

                    builder.switch_to_block(suspend_exit);
                    let zero_count = builder.ins().iconst(types::I32, 0);
                    builder.ins().store(
                        MemFlags::trusted(),
                        zero_count,
                        self.params.exit_info_ptr,
                        40,
                    );
                    self.emit_exit_return(
                        builder,
                        JitExitKind::Suspended as i64,
                        JitSuspendReason::NativeCallBoundary as i64,
                        *bytecode_offset as i64,
                    );

                    builder.switch_to_block(fallback_suspend);
                    let zero_count = builder.ins().iconst(types::I32, 0);
                    builder.ins().store(
                        MemFlags::trusted(),
                        zero_count,
                        self.params.exit_info_ptr,
                        40,
                    );
                    self.emit_exit_return(
                        builder,
                        JitExitKind::Suspended as i64,
                        JitSuspendReason::NativeCallBoundary as i64,
                        *bytecode_offset as i64,
                    );

                    builder.switch_to_block(fast_continue);
                    if let Some(d) = dest {
                        self.def_reg(builder, *d, result);
                    }
                    return Ok(false);
                } else {
                    // Arg-carrying fast path:
                    // If runtime context is available, marshal args and dispatch helper-native directly.
                    // On sentinel suspend token (or missing ctx), fall back to boundary suspend and
                    // materialize operands into exit_info for interpreter resume.
                    let ctx = self.params.ctx_ptr;
                    let is_ctx_null = builder.ins().icmp_imm(condcodes::IntCC::Equal, ctx, 0);
                    let fallback_suspend = builder.create_block();
                    let do_dispatch = builder.create_block();
                    builder
                        .ins()
                        .brif(is_ctx_null, fallback_suspend, &[], do_dispatch, &[]);
                    builder.seal_block(do_dispatch);
                    builder.switch_to_block(do_dispatch);

                    let arg_count = args.len().min(u8::MAX as usize);
                    let args_slot = builder.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        (arg_count * 8) as u32,
                        3,
                    ));
                    let args_ptr = builder.ins().stack_addr(types::I64, args_slot, 0);
                    for (i, reg) in args.iter().take(arg_count).enumerate() {
                        let raw = self.use_reg(builder, *reg);
                        let boxed = match self.func.reg_type(*reg) {
                            crate::jit::ir::types::JitType::I32 => abi::emit_box_i32(builder, raw),
                            crate::jit::ir::types::JitType::F64 => abi::emit_box_f64(builder, raw),
                            crate::jit::ir::types::JitType::Bool => {
                                abi::emit_box_bool(builder, raw)
                            }
                            crate::jit::ir::types::JitType::Ptr => abi::emit_box_ptr(builder, raw),
                            _ => raw,
                        };
                        builder
                            .ins()
                            .store(MemFlags::trusted(), boxed, args_ptr, (i as i32) * 8);
                    }

                    let shared_state = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 0);
                    let fn_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 64); // 24 + 40
                    let sig = self.native_call_dispatch_sig(builder);
                    let native_id_val = builder.ins().iconst(types::I16, *native_id as i64);
                    let arg_count_i8 = builder.ins().iconst(types::I8, arg_count as i64);
                    let call = builder.ins().call_indirect(
                        sig,
                        fn_ptr,
                        &[native_id_val, args_ptr, arg_count_i8, shared_state],
                    );
                    let result = builder.inst_results(call)[0];
                    let suspend_sentinel = builder
                        .ins()
                        .iconst(types::I64, JIT_NATIVE_SUSPEND_SENTINEL as i64);
                    let is_suspend =
                        builder
                            .ins()
                            .icmp(condcodes::IntCC::Equal, result, suspend_sentinel);
                    let suspend_exit = builder.create_block();
                    let fast_continue = builder.create_block();
                    builder
                        .ins()
                        .brif(is_suspend, suspend_exit, &[], fast_continue, &[]);
                    builder.seal_block(suspend_exit);
                    builder.seal_block(fast_continue);
                    builder.seal_block(fallback_suspend);

                    builder.switch_to_block(suspend_exit);
                    let count = args.len().min(JIT_EXIT_MAX_NATIVE_ARGS) as i64;
                    let count_val = builder.ins().iconst(types::I32, count);
                    builder.ins().store(
                        MemFlags::trusted(),
                        count_val,
                        self.params.exit_info_ptr,
                        40,
                    );
                    for (i, reg) in args.iter().take(JIT_EXIT_MAX_NATIVE_ARGS).enumerate() {
                        let raw = self.use_reg(builder, *reg);
                        let boxed = match self.func.reg_type(*reg) {
                            crate::jit::ir::types::JitType::I32 => abi::emit_box_i32(builder, raw),
                            crate::jit::ir::types::JitType::F64 => abi::emit_box_f64(builder, raw),
                            crate::jit::ir::types::JitType::Bool => {
                                abi::emit_box_bool(builder, raw)
                            }
                            crate::jit::ir::types::JitType::Ptr => abi::emit_box_ptr(builder, raw),
                            _ => raw,
                        };
                        let off = 48 + (i as i32) * 8;
                        builder.ins().store(
                            MemFlags::trusted(),
                            boxed,
                            self.params.exit_info_ptr,
                            off,
                        );
                    }
                    self.emit_exit_return(
                        builder,
                        JitExitKind::Suspended as i64,
                        JitSuspendReason::NativeCallBoundary as i64,
                        *bytecode_offset as i64,
                    );

                    builder.switch_to_block(fallback_suspend);
                    let count = args.len().min(JIT_EXIT_MAX_NATIVE_ARGS) as i64;
                    let count_val = builder.ins().iconst(types::I32, count);
                    builder.ins().store(
                        MemFlags::trusted(),
                        count_val,
                        self.params.exit_info_ptr,
                        40,
                    );
                    for (i, reg) in args.iter().take(JIT_EXIT_MAX_NATIVE_ARGS).enumerate() {
                        let raw = self.use_reg(builder, *reg);
                        let boxed = match self.func.reg_type(*reg) {
                            crate::jit::ir::types::JitType::I32 => abi::emit_box_i32(builder, raw),
                            crate::jit::ir::types::JitType::F64 => abi::emit_box_f64(builder, raw),
                            crate::jit::ir::types::JitType::Bool => {
                                abi::emit_box_bool(builder, raw)
                            }
                            crate::jit::ir::types::JitType::Ptr => abi::emit_box_ptr(builder, raw),
                            _ => raw,
                        };
                        let off = 48 + (i as i32) * 8;
                        builder.ins().store(
                            MemFlags::trusted(),
                            boxed,
                            self.params.exit_info_ptr,
                            off,
                        );
                    }
                    self.emit_exit_return(
                        builder,
                        JitExitKind::Suspended as i64,
                        JitSuspendReason::NativeCallBoundary as i64,
                        *bytecode_offset as i64,
                    );

                    builder.switch_to_block(fast_continue);
                    if let Some(d) = dest {
                        self.def_reg(builder, *d, result);
                    }
                    return Ok(false);
                }
            }

            // ===== Everything else: unsupported for now =====
            _ => {
                // Unsupported instructions get a placeholder
                // In a real implementation, these would deoptimize to the interpreter
                return Err(LowerError::UnsupportedInstruction(format!("{:?}", instr)));
            }
        }
        Ok(false)
    }

    #[inline]
    fn emit_exit_return(
        &self,
        builder: &mut FunctionBuilder<'_>,
        kind: i64,
        suspend_reason: i64,
        bytecode_offset: i64,
    ) {
        let kind_val = builder.ins().iconst(types::I32, kind);
        builder
            .ins()
            .store(MemFlags::trusted(), kind_val, self.params.exit_info_ptr, 0);
        let reason_val = builder.ins().iconst(types::I32, suspend_reason);
        builder.ins().store(
            MemFlags::trusted(),
            reason_val,
            self.params.exit_info_ptr,
            4,
        );
        let bc_val = builder.ins().iconst(types::I32, bytecode_offset);
        builder
            .ins()
            .store(MemFlags::trusted(), bc_val, self.params.exit_info_ptr, 8);
        let null = abi::emit_null(builder);
        builder.ins().return_(&[null]);
    }

    fn safepoint_sig(&mut self, builder: &mut FunctionBuilder<'_>) -> ir::SigRef {
        if let Some(sig) = self.sig_safepoint_poll {
            return sig;
        }
        let mut sig = ir::Signature::new(builder.func.signature.call_conv);
        sig.params.push(AbiParam::new(types::I64));
        let sig_ref = builder.func.import_signature(sig);
        self.sig_safepoint_poll = Some(sig_ref);
        sig_ref
    }

    fn check_preemption_sig(&mut self, builder: &mut FunctionBuilder<'_>) -> ir::SigRef {
        if let Some(sig) = self.sig_check_preemption {
            return sig;
        }
        let mut sig = ir::Signature::new(builder.func.signature.call_conv);
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I8));
        let sig_ref = builder.func.import_signature(sig);
        self.sig_check_preemption = Some(sig_ref);
        sig_ref
    }

    fn native_call_dispatch_sig(&mut self, builder: &mut FunctionBuilder<'_>) -> ir::SigRef {
        if let Some(sig) = self.sig_native_call_dispatch {
            return sig;
        }
        let mut sig = ir::Signature::new(builder.func.signature.call_conv);
        sig.params.push(AbiParam::new(types::I16)); // native_id
        sig.params.push(AbiParam::new(types::I64)); // args_ptr
        sig.params.push(AbiParam::new(types::I8)); // arg_count
        sig.params.push(AbiParam::new(types::I64)); // shared_state
        sig.returns.push(AbiParam::new(types::I64)); // NaN-boxed value or suspend sentinel
        let sig_ref = builder.func.import_signature(sig);
        self.sig_native_call_dispatch = Some(sig_ref);
        sig_ref
    }

    fn alloc_object_sig(&mut self, builder: &mut FunctionBuilder<'_>) -> ir::SigRef {
        if let Some(sig) = self.sig_alloc_object {
            return sig;
        }
        let mut sig = ir::Signature::new(builder.func.signature.call_conv);
        sig.params.push(AbiParam::new(types::I32)); // local nominal type index
        sig.params.push(AbiParam::new(types::I64)); // module ptr
        sig.params.push(AbiParam::new(types::I64)); // shared_state ptr
        sig.returns.push(AbiParam::new(types::I64)); // object ptr
        let sig_ref = builder.func.import_signature(sig);
        self.sig_alloc_object = Some(sig_ref);
        sig_ref
    }

    fn interpreter_call_sig(&mut self, builder: &mut FunctionBuilder<'_>) -> ir::SigRef {
        if let Some(sig) = self.sig_interpreter_call {
            return sig;
        }
        let mut sig = ir::Signature::new(builder.func.signature.call_conv);
        sig.params.push(AbiParam::new(types::I8)); // opcode
        sig.params.push(AbiParam::new(types::I64)); // operand_u64
        sig.params.push(AbiParam::new(types::I32)); // operand_u32
        sig.params.push(AbiParam::new(types::I64)); // receiver value
        sig.params.push(AbiParam::new(types::I64)); // args ptr
        sig.params.push(AbiParam::new(types::I16)); // arg_count
        sig.params.push(AbiParam::new(types::I64)); // module ptr
        sig.params.push(AbiParam::new(types::I64)); // shared_state ptr
        sig.returns.push(AbiParam::new(types::I64)); // value/sentinel
        let sig_ref = builder.func.import_signature(sig);
        self.sig_interpreter_call = Some(sig_ref);
        sig_ref
    }

    fn string_len_sig(&mut self, builder: &mut FunctionBuilder<'_>) -> ir::SigRef {
        if let Some(sig) = self.sig_string_len {
            return sig;
        }
        let mut sig = ir::Signature::new(builder.func.signature.call_conv);
        sig.params.push(AbiParam::new(types::I64)); // string value
        sig.params.push(AbiParam::new(types::I64)); // shared_state ptr
        sig.returns.push(AbiParam::new(types::I32)); // string len or fallback sentinel
        let sig_ref = builder.func.import_signature(sig);
        self.sig_string_len = Some(sig_ref);
        sig_ref
    }

    fn alloc_string_sig(&mut self, builder: &mut FunctionBuilder<'_>) -> ir::SigRef {
        if let Some(sig) = self.sig_alloc_string {
            return sig;
        }
        let mut sig = ir::Signature::new(builder.func.signature.call_conv);
        sig.params.push(AbiParam::new(types::I64)); // data_ptr
        sig.params.push(AbiParam::new(types::I64)); // len
        sig.params.push(AbiParam::new(types::I64)); // shared_state ptr
        sig.returns.push(AbiParam::new(types::I64)); // string ptr
        let sig_ref = builder.func.import_signature(sig);
        self.sig_alloc_string = Some(sig_ref);
        sig_ref
    }

    fn lower_const_string_ptr(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        dest: Reg,
        bytes: &[u8],
    ) {
        let ctx = self.params.ctx_ptr;
        let shared_state = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 0);
        let fn_ptr = builder.ins().load(types::I64, MemFlags::trusted(), ctx, 40);
        let slot_size = bytes.len().max(1) as u32;
        let data_slot = builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            slot_size,
            1,
        ));
        let data_ptr = builder.ins().stack_addr(types::I64, data_slot, 0);
        for (i, byte) in bytes.iter().copied().enumerate() {
            let value = builder.ins().iconst(types::I8, byte as i64);
            builder
                .ins()
                .store(MemFlags::trusted(), value, data_ptr, i as i32);
        }
        let len = builder.ins().iconst(types::I64, bytes.len() as i64);
        let sig = self.alloc_string_sig(builder);
        let call = builder
            .ins()
            .call_indirect(sig, fn_ptr, &[data_ptr, len, shared_state]);
        let string_ptr = builder.inst_results(call)[0];
        self.def_reg(builder, dest, string_ptr);
    }

    fn object_get_field_sig(&mut self, builder: &mut FunctionBuilder<'_>) -> ir::SigRef {
        if let Some(sig) = self.sig_object_get_field {
            return sig;
        }
        let mut sig = ir::Signature::new(builder.func.signature.call_conv);
        sig.params.push(AbiParam::new(types::I64)); // object value
        sig.params.push(AbiParam::new(types::I32)); // expected slot
        sig.params.push(AbiParam::new(types::I32)); // function id
        sig.params.push(AbiParam::new(types::I64)); // module ptr
        sig.params.push(AbiParam::new(types::I64)); // shared_state ptr
        sig.returns.push(AbiParam::new(types::I64)); // loaded value
        let sig_ref = builder.func.import_signature(sig);
        self.sig_object_get_field = Some(sig_ref);
        sig_ref
    }

    fn object_implements_shape_sig(&mut self, builder: &mut FunctionBuilder<'_>) -> ir::SigRef {
        if let Some(sig) = self.sig_object_implements_shape {
            return sig;
        }
        let mut sig = ir::Signature::new(builder.func.signature.call_conv);
        sig.params.push(AbiParam::new(types::I64)); // object value
        sig.params.push(AbiParam::new(types::I64)); // shape id
        sig.params.push(AbiParam::new(types::I64)); // shared_state ptr
        sig.returns.push(AbiParam::new(types::I8)); // bool result
        let sig_ref = builder.func.import_signature(sig);
        self.sig_object_implements_shape = Some(sig_ref);
        sig_ref
    }

    fn object_is_nominal_sig(&mut self, builder: &mut FunctionBuilder<'_>) -> ir::SigRef {
        if let Some(sig) = self.sig_object_is_nominal {
            return sig;
        }
        let mut sig = ir::Signature::new(builder.func.signature.call_conv);
        sig.params.push(AbiParam::new(types::I64)); // object value
        sig.params.push(AbiParam::new(types::I32)); // local nominal type index
        sig.params.push(AbiParam::new(types::I64)); // module ptr
        sig.params.push(AbiParam::new(types::I64)); // shared_state ptr
        sig.returns.push(AbiParam::new(types::I8)); // bool result
        let sig_ref = builder.func.import_signature(sig);
        self.sig_object_is_nominal = Some(sig_ref);
        sig_ref
    }

    fn object_get_shape_field_sig(&mut self, builder: &mut FunctionBuilder<'_>) -> ir::SigRef {
        if let Some(sig) = self.sig_object_get_shape_field {
            return sig;
        }
        let mut sig = ir::Signature::new(builder.func.signature.call_conv);
        sig.params.push(AbiParam::new(types::I64)); // object value
        sig.params.push(AbiParam::new(types::I64)); // shape id
        sig.params.push(AbiParam::new(types::I32)); // expected slot
        sig.params.push(AbiParam::new(types::I8)); // optional
        sig.params.push(AbiParam::new(types::I32)); // function id
        sig.params.push(AbiParam::new(types::I64)); // module ptr
        sig.params.push(AbiParam::new(types::I64)); // shared_state
        sig.returns.push(AbiParam::new(types::I64)); // value/sentinel
        let sig_ref = builder.func.import_signature(sig);
        self.sig_object_get_shape_field = Some(sig_ref);
        sig_ref
    }

    fn object_set_shape_field_sig(&mut self, builder: &mut FunctionBuilder<'_>) -> ir::SigRef {
        if let Some(sig) = self.sig_object_set_shape_field {
            return sig;
        }
        let mut sig = ir::Signature::new(builder.func.signature.call_conv);
        sig.params.push(AbiParam::new(types::I64)); // object value
        sig.params.push(AbiParam::new(types::I64)); // shape id
        sig.params.push(AbiParam::new(types::I32)); // expected slot
        sig.params.push(AbiParam::new(types::I64)); // value
        sig.params.push(AbiParam::new(types::I32)); // function id
        sig.params.push(AbiParam::new(types::I64)); // module ptr
        sig.params.push(AbiParam::new(types::I64)); // shared_state
        sig.returns.push(AbiParam::new(types::I8)); // status
        let sig_ref = builder.func.import_signature(sig);
        self.sig_object_set_shape_field = Some(sig_ref);
        sig_ref
    }

    /// Lower an integer comparison
    fn lower_icmp(
        &self,
        builder: &mut FunctionBuilder<'_>,
        cc: condcodes::IntCC,
        dest: Reg,
        left: Reg,
        right: Reg,
    ) {
        let l = self.use_reg(builder, left);
        let r = self.use_reg(builder, right);
        let result = builder.ins().icmp(cc, l, r);
        self.def_reg(builder, dest, result);
    }

    /// Lower a float comparison
    fn lower_fcmp(
        &self,
        builder: &mut FunctionBuilder<'_>,
        cc: condcodes::FloatCC,
        dest: Reg,
        left: Reg,
        right: Reg,
    ) {
        let l = self.use_reg(builder, left);
        let r = self.use_reg(builder, right);
        let result = builder.ins().fcmp(cc, l, r);
        self.def_reg(builder, dest, result);
    }

    /// Lower a block terminator
    fn lower_terminator(
        &self,
        term: &JitTerminator,
        builder: &mut FunctionBuilder<'_>,
    ) -> Result<(), LowerError> {
        // Write default exit status (Completed) when exit_info is provided.
        let completed = builder
            .ins()
            .iconst(types::I32, JitExitKind::Completed as i64);
        builder.ins().store(
            MemFlags::trusted(),
            completed,
            self.params.exit_info_ptr,
            0, // JitExitInfo.kind
        );

        match term {
            JitTerminator::Return(Some(reg)) => {
                let val = self.use_reg(builder, *reg);
                // Ensure return value is i64 (NaN-boxed)
                let ret_val = match self.func.reg_type(*reg) {
                    crate::jit::ir::types::JitType::I32 => abi::emit_box_i32(builder, val),
                    crate::jit::ir::types::JitType::F64 => abi::emit_box_f64(builder, val),
                    crate::jit::ir::types::JitType::Bool => abi::emit_box_bool(builder, val),
                    crate::jit::ir::types::JitType::Ptr => abi::emit_box_ptr(builder, val),
                    _ => val, // Already i64/Value
                };
                builder.ins().return_(&[ret_val]);
            }
            JitTerminator::Return(None) => {
                let null = abi::emit_null(builder);
                builder.ins().return_(&[null]);
            }
            JitTerminator::Jump(target) => {
                let cl_target = self.block_map[target];
                builder.ins().jump(cl_target, &[]);
            }
            JitTerminator::Branch {
                cond,
                then_block,
                else_block,
            } => {
                let cond_val = self.use_reg(builder, *cond);
                let then_cl = self.block_map[then_block];
                let else_cl = self.block_map[else_block];
                builder.ins().brif(cond_val, then_cl, &[], else_cl, &[]);
            }
            JitTerminator::Unreachable => {
                builder.ins().trap(ir::TrapCode::user(0).unwrap());
            }
            JitTerminator::None => {
                // Should not happen in a well-formed IR
                builder.ins().trap(ir::TrapCode::user(1).unwrap());
            }
            JitTerminator::Throw(_)
            | JitTerminator::Deoptimize { .. }
            | JitTerminator::BranchNull { .. } => {
                // These paths are not lowered yet; fail compilation so runtime
                // stays on interpreter for this function instead of emitting a
                // compiled trap that can SIGTRAP the process.
                return Err(LowerError::UnsupportedInstruction(format!(
                    "terminator {:?}",
                    term
                )));
            }
        }
        Ok(())
    }
}

/// Build the Cranelift function signature for JIT entry functions.
///
/// ABI: `extern "C" fn(args: *const u64, arg_count: u32, locals: *mut u64, local_count: u32, ctx: *mut RuntimeContext, exit_info: *mut JitExitInfo) -> u64`
pub fn jit_entry_signature(call_conv: CallConv) -> ir::Signature {
    let mut sig = ir::Signature::new(call_conv);
    sig.params.push(AbiParam::new(types::I64)); // args_ptr
    sig.params.push(AbiParam::new(types::I32)); // arg_count
    sig.params.push(AbiParam::new(types::I64)); // locals_ptr
    sig.params.push(AbiParam::new(types::I32)); // local_count
    sig.params.push(AbiParam::new(types::I64)); // ctx_ptr
    sig.params.push(AbiParam::new(types::I64)); // exit_info_ptr
    sig.returns.push(AbiParam::new(types::I64)); // return value (NaN-boxed)
    sig
}

/// Error during Cranelift lowering
#[derive(Debug, thiserror::Error)]
pub enum LowerError {
    #[error("Unsupported instruction: {0}")]
    UnsupportedInstruction(String),
    #[error("Cranelift error: {0}")]
    CraneliftError(String),
}
