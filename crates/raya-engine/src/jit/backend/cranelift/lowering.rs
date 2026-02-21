//! JIT IR → Cranelift IR lowering
//!
//! Translates the backend-agnostic JIT IR (SSA form) into Cranelift IR that can
//! be compiled to native code. Handles typed arithmetic, NaN-boxing conversions,
//! local variable access, and control flow.

use cranelift_codegen::ir::{self, condcodes, types, InstBuilder, MemFlags};
use cranelift_codegen::ir::AbiParam;
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::jit::ir::instr::{JitBlockId, JitFunction, JitInstr, JitTerminator, Reg};
use super::abi;

/// State maintained during lowering of a single function
pub struct LoweringContext<'a> {
    /// Map from JIT IR Reg → Cranelift Variable
    reg_vars: FxHashMap<Reg, Variable>,
    /// Map from JIT BlockId → Cranelift Block
    block_map: FxHashMap<JitBlockId, ir::Block>,
    /// The JIT function being lowered
    func: &'a JitFunction,
    /// Cranelift function parameters (args_ptr, arg_count, locals_ptr, local_count, ctx_ptr)
    params: FunctionParams,
    /// Phi resolution: for each block, a list of (phi_dest_reg, source_reg) to def_var before terminator
    phi_copies: FxHashMap<JitBlockId, Vec<(Reg, Reg)>>,
}

/// The five parameters of the JIT entry function ABI
struct FunctionParams {
    _args_ptr: ir::Value,
    _arg_count: ir::Value,
    locals_ptr: ir::Value,
    _local_count: ir::Value,
    _ctx_ptr: ir::Value,
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
                    copies.entry(*src_block)
                        .or_default()
                        .push((*dest, *src_reg));
                }
            }
        }
    }
    copies
}

impl<'a> LoweringContext<'a> {
    /// Lower an entire JIT function into Cranelift IR.
    /// Takes ownership of the FunctionBuilder since finalize() consumes it.
    pub fn lower(
        func: &'a JitFunction,
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
            _ctx_ptr: builder.block_params(entry_block)[4],
        };

        let mut ctx = LoweringContext {
            reg_vars: FxHashMap::default(),
            block_map,
            func,
            params,
            phi_copies,
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

            // All JIT registers are i64 (NaN-boxed Value or unboxed depending on context)
            // We use i64 uniformly and bitcast when needed for f64
            let ty = match self.func.reg_type(reg) {
                crate::jit::ir::types::JitType::F64 => types::F64,
                crate::jit::ir::types::JitType::Bool => types::I8,
                crate::jit::ir::types::JitType::I32 => types::I32,
                _ => types::I64,
            };
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

    /// Lower all instructions and terminator for a single block
    fn lower_block(
        &mut self,
        block_id: JitBlockId,
        builder: &mut FunctionBuilder<'_>,
    ) -> Result<(), LowerError> {
        let block = self.func.block(block_id);
        let instrs = block.instrs.clone();
        let terminator = block.terminator.clone();

        for instr in &instrs {
            self.lower_instr(instr, builder)?;
        }

        // Emit Phi resolution copies before the terminator.
        // For each Phi in a successor block that sources from this block,
        // def_var the Phi's dest register with the source value from this block.
        // Cranelift's SSA construction will merge these into block params when sealed.
        if let Some(copies) = self.phi_copies.get(&block_id) {
            for &(phi_dest, src_reg) in copies {
                let val = self.use_reg(builder, src_reg);
                self.def_reg(builder, phi_dest, val);
            }
        }

        self.lower_terminator(&terminator, builder)?;
        Ok(())
    }

    /// Lower a single JIT IR instruction to Cranelift IR
    fn lower_instr(
        &mut self,
        instr: &JitInstr,
        builder: &mut FunctionBuilder<'_>,
    ) -> Result<(), LowerError> {
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
                // String constants remain as NaN-boxed values; load from constant pool
                // For now, emit as a tagged constant with the pool index
                // Real implementation would look up the string in the constant pool
                let val = builder.ins().iconst(types::I64, *pool_index as i64);
                self.def_reg(builder, *dest, val);
            }
            JitInstr::ConstStr { dest, str_index } => {
                let val = builder.ins().iconst(types::I64, *str_index as i64);
                self.def_reg(builder, *dest, val);
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
                self.lower_icmp(builder, condcodes::IntCC::SignedLessThan, *dest, *left, *right);
            }
            JitInstr::ICmpLe { dest, left, right } => {
                self.lower_icmp(builder, condcodes::IntCC::SignedLessThanOrEqual, *dest, *left, *right);
            }
            JitInstr::ICmpGt { dest, left, right } => {
                self.lower_icmp(builder, condcodes::IntCC::SignedGreaterThan, *dest, *left, *right);
            }
            JitInstr::ICmpGe { dest, left, right } => {
                self.lower_icmp(builder, condcodes::IntCC::SignedGreaterThanOrEqual, *dest, *left, *right);
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
                self.lower_fcmp(builder, condcodes::FloatCC::LessThanOrEqual, *dest, *left, *right);
            }
            JitInstr::FCmpGt { dest, left, right } => {
                self.lower_fcmp(builder, condcodes::FloatCC::GreaterThan, *dest, *left, *right);
            }
            JitInstr::FCmpGe { dest, left, right } => {
                self.lower_fcmp(builder, condcodes::FloatCC::GreaterThanOrEqual, *dest, *left, *right);
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
                let v = self.use_reg(builder, *value);
                let offset = (*index as i32) * 8;
                builder.ins().store(
                    MemFlags::trusted(),
                    v,
                    self.params.locals_ptr,
                    offset,
                );
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
            JitInstr::GcSafepoint => {
                // Would call safepoint_poll via RuntimeHelperTable
                // For now, emit a nop
                builder.ins().nop();
            }
            JitInstr::CheckPreemption => {
                // Would call check_preemption via RuntimeHelperTable
                builder.ins().nop();
            }

            // ===== Everything else: unsupported for now =====
            _ => {
                // Unsupported instructions get a placeholder
                // In a real implementation, these would deoptimize to the interpreter
                return Err(LowerError::UnsupportedInstruction(format!("{:?}", instr)));
            }
        }
        Ok(())
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
        match term {
            JitTerminator::Return(Some(reg)) => {
                let val = self.use_reg(builder, *reg);
                // Ensure return value is i64 (NaN-boxed)
                let ret_val = match self.func.reg_type(*reg) {
                    crate::jit::ir::types::JitType::I32 => abi::emit_box_i32(builder, val),
                    crate::jit::ir::types::JitType::F64 => abi::emit_box_f64(builder, val),
                    crate::jit::ir::types::JitType::Bool => abi::emit_box_bool(builder, val),
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
            JitTerminator::Branch { cond, then_block, else_block } => {
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
            JitTerminator::Throw(_) | JitTerminator::Deoptimize { .. } | JitTerminator::BranchNull { .. } => {
                // For now, trap on unsupported terminators
                builder.ins().trap(ir::TrapCode::user(2).unwrap());
            }
        }
        Ok(())
    }
}

/// Build the Cranelift function signature for JIT entry functions.
///
/// ABI: `extern "C" fn(args: *const u64, arg_count: u32, locals: *mut u64, local_count: u32, ctx: *mut RuntimeContext) -> u64`
pub fn jit_entry_signature(call_conv: CallConv) -> ir::Signature {
    let mut sig = ir::Signature::new(call_conv);
    sig.params.push(AbiParam::new(types::I64));  // args_ptr
    sig.params.push(AbiParam::new(types::I32));  // arg_count
    sig.params.push(AbiParam::new(types::I64));  // locals_ptr
    sig.params.push(AbiParam::new(types::I32));  // local_count
    sig.params.push(AbiParam::new(types::I64));  // ctx_ptr
    sig.returns.push(AbiParam::new(types::I64));  // return value (NaN-boxed)
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
