//! Register-based code generator: IR → reg_code
//!
//! Translates IrModule functions into register bytecode (Vec<u32>) using
//! fixed 32-bit RegOpcode instructions. Runs as a post-pass after the
//! stack codegen has built the full Module (classes, reflection, constants).

use crate::compiler::bytecode::reg_opcode::{RegBytecodeWriter, RegOpcode};
use crate::compiler::bytecode::{Function, Module};
use crate::compiler::error::{CompileError, CompileResult};
use crate::compiler::ir::{
    BasicBlock, BasicBlockId, BinaryOp, IrConstant, IrFunction, IrInstr, IrModule, IrValue,
    Register, StringCompareMode, Terminator, UnaryOp,
};
use rustc_hash::FxHashMap;

/// Constant pool Bx tag bits (upper 2 bits of 16-bit Bx field)
const CONST_TAG_INTEGER: u16 = 0b00 << 14;
const CONST_TAG_FLOAT: u16 = 0b01 << 14;
const CONST_TAG_STRING: u16 = 0b10 << 14;

/// Type IDs used for typed opcode selection
const INT_TYPE_ID: u32 = 16;

/// Register code generator that transforms IR functions into register bytecode.
///
/// Operates as a post-pass: takes an already-built Module (with stack bytecode)
/// and the original IrModule, populates `reg_code` + `register_count` on each Function.
pub struct RegCodeGenerator<'a> {
    ir_module: &'a IrModule,
}

/// Per-function context for register code generation
struct RegFuncCtx {
    writer: RegBytecodeWriter,
    /// IR RegisterId(u32) → physical u8 register
    reg_map: FxHashMap<u32, u8>,
    /// Next available physical register
    next_reg: u8,
    /// Maximum register used (for register_count)
    max_reg: u8,
    /// Block start positions (instruction index)
    block_positions: FxHashMap<BasicBlockId, usize>,
    /// Pending jump patches: (instruction position, target block)
    pending_jumps: Vec<(usize, BasicBlockId)>,
    /// Pending Try patches: (extra word position, catch_block, finally_block)
    pending_try: Vec<(usize, BasicBlockId, Option<BasicBlockId>)>,
    /// Base register for temporary argument packing
    temp_base: u8,
    /// Catch register for the next SetupTry (from scanning the catch block's PopToLocal)
    catch_reg_hint: Option<u8>,
}

impl RegFuncCtx {
    fn new(param_count: u8) -> Self {
        Self {
            writer: RegBytecodeWriter::new(),
            reg_map: FxHashMap::default(),
            next_reg: param_count,
            max_reg: param_count.saturating_sub(1),
            block_positions: FxHashMap::default(),
            pending_jumps: Vec::new(),
            pending_try: Vec::new(),
            temp_base: 0,
            catch_reg_hint: None,
        }
    }

    /// Map an IR RegisterId to a physical u8 register, allocating on first use.
    fn map_reg(&mut self, reg: &Register) -> CompileResult<u8> {
        let id = reg.id.as_u32();
        if let Some(&phys) = self.reg_map.get(&id) {
            return Ok(phys);
        }
        let phys = self.next_reg;
        if phys == 255 {
            return Err(CompileError::UnsupportedFeature {
                feature: "Function requires more than 255 registers".to_string(),
            });
        }
        self.next_reg = phys + 1;
        if phys > self.max_reg {
            self.max_reg = phys;
        }
        self.reg_map.insert(id, phys);
        Ok(phys)
    }

    /// Get physical register for an IR register that should already be mapped.
    fn get_reg(&self, reg: &Register) -> CompileResult<u8> {
        self.reg_map.get(&reg.id.as_u32()).copied().ok_or_else(|| {
            CompileError::UnsupportedFeature {
                feature: format!("Unmapped register r{}", reg.id.as_u32()),
            }
        })
    }

    /// Get or allocate a physical register (convenience combining map_reg logic).
    fn reg(&mut self, reg: &Register) -> CompileResult<u8> {
        let id = reg.id.as_u32();
        if let Some(&phys) = self.reg_map.get(&id) {
            Ok(phys)
        } else {
            self.map_reg(reg)
        }
    }

    /// Pack a list of IR registers into contiguous physical registers for a call.
    /// Returns (start_reg, count). Emits Move instructions if args are not contiguous.
    fn pack_args(&mut self, args: &[Register]) -> CompileResult<(u8, u8)> {
        let count = args.len();
        if count == 0 {
            return Ok((0, 0));
        }

        // Get all physical registers
        let phys: Vec<u8> = args
            .iter()
            .map(|a| self.get_reg(a))
            .collect::<CompileResult<Vec<_>>>()?;

        // Check if already contiguous
        let first = phys[0];
        let contiguous = phys
            .iter()
            .enumerate()
            .all(|(i, &r)| r == first.wrapping_add(i as u8));

        if contiguous {
            return Ok((first, count as u8));
        }

        // Emit Moves into temp range
        let base = self.temp_base;
        for (i, &src) in phys.iter().enumerate() {
            let dst = base.wrapping_add(i as u8);
            if dst > self.max_reg {
                self.max_reg = dst;
            }
            if src != dst {
                self.writer.emit_abc(RegOpcode::Move, dst, src, 0);
            }
        }
        Ok((base, count as u8))
    }

    /// Pack object + args into contiguous registers for CallMethod/SpawnClosure.
    fn pack_obj_args(&mut self, obj: &Register, args: &[Register]) -> CompileResult<(u8, u8)> {
        let mut combined = Vec::with_capacity(1 + args.len());
        combined.push(obj.clone());
        combined.extend_from_slice(args);
        self.pack_args(&combined)
    }

    fn record_block(&mut self, block_id: BasicBlockId) {
        self.block_positions.insert(block_id, self.writer.position());
    }

    fn record_jump(&mut self, pos: usize, target: BasicBlockId) {
        self.pending_jumps.push((pos, target));
    }

    fn patch_all(&mut self) {
        // Patch jumps: sBx = target - (source + 1) [relative to next instruction]
        for &(src_pos, target_block) in &self.pending_jumps {
            if let Some(&target_pos) = self.block_positions.get(&target_block) {
                let offset = target_pos as i32 - (src_pos as i32 + 1);
                self.writer.patch_sbx(src_pos, offset as i16);
            }
        }
        // Patch Try: extra = (catch_ip << 16) | finally_ip, using absolute positions
        for &(extra_pos, catch_block, finally_block) in &self.pending_try {
            let catch_ip = self
                .block_positions
                .get(&catch_block)
                .copied()
                .unwrap_or(0xFFFF);
            let finally_ip = finally_block
                .and_then(|fb| self.block_positions.get(&fb).copied())
                .unwrap_or(0xFFFF);
            let extra = ((catch_ip as u32) << 16) | (finally_ip as u32 & 0xFFFF);
            self.writer.patch(extra_pos, extra);
        }
    }

    fn finish(mut self) -> (Vec<u32>, u16) {
        self.patch_all();
        let reg_count = if self.max_reg == 0 && self.reg_map.is_empty() {
            0
        } else {
            (self.max_reg as u16) + 1
        };
        (self.writer.finish(), reg_count)
    }
}

impl<'a> RegCodeGenerator<'a> {
    pub fn new(ir_module: &'a IrModule) -> Self {
        Self { ir_module }
    }

    /// Generate register bytecode for all functions and patch the module.
    pub fn generate(&self, module: &mut Module) -> CompileResult<()> {
        for (func_idx, ir_func) in self.ir_module.functions().enumerate() {
            let (reg_code, register_count) = self.generate_function(ir_func, module)?;
            if let Some(func) = module.functions.get_mut(func_idx) {
                func.reg_code = reg_code;
                func.register_count = register_count;
            }
        }
        Ok(())
    }

    fn generate_function(
        &self,
        func: &IrFunction,
        module: &mut Module,
    ) -> CompileResult<(Vec<u32>, u16)> {
        let param_count = func.param_count() as u8;
        let mut ctx = RegFuncCtx::new(param_count);

        // Pre-map parameter registers
        for (i, param) in func.params.iter().enumerate() {
            ctx.reg_map.insert(param.id.as_u32(), i as u8);
        }

        // Pre-scan LoadLocal/StoreLocal to avoid register collisions
        let mut max_fixed: u8 = param_count;
        for block in func.blocks() {
            for instr in &block.instructions {
                match instr {
                    IrInstr::StoreLocal { index, .. } | IrInstr::LoadLocal { index, .. } => {
                        let idx = *index as u8;
                        if idx >= max_fixed {
                            max_fixed = idx + 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        if max_fixed > ctx.next_reg {
            ctx.next_reg = max_fixed;
        }

        // First pass: map all IR registers by scanning all instructions
        // This ensures we know the total register count before emitting code
        for block in func.blocks() {
            for instr in &block.instructions {
                self.prescan_instr(&mut ctx, instr)?;
            }
            self.prescan_terminator(&mut ctx, &block.terminator)?;
        }

        // Set temp_base above all IR registers (for argument packing)
        ctx.temp_base = ctx.next_reg;

        // Pre-scan catch blocks to find PopToLocal indices for Try catch_reg
        let mut catch_block_regs: FxHashMap<BasicBlockId, u8> = FxHashMap::default();
        for block in func.blocks() {
            if let Some(IrInstr::PopToLocal { index }) = block.instructions.first() {
                catch_block_regs.insert(block.id, *index as u8);
            }
        }

        // Second pass: emit register bytecode
        for block in func.blocks() {
            ctx.record_block(block.id);
            for instr in &block.instructions {
                self.emit_instr(&mut ctx, instr, module, &catch_block_regs)?;
            }
            self.emit_terminator(&mut ctx, &block.terminator, module)?;
        }

        Ok(ctx.finish())
    }

    /// Pre-scan to map all registers used in an instruction.
    fn prescan_instr(&self, ctx: &mut RegFuncCtx, instr: &IrInstr) -> CompileResult<()> {
        // Map destination and source registers
        match instr {
            IrInstr::Assign { dest, value } => {
                ctx.reg(dest)?;
                if let IrValue::Register(src) = value {
                    ctx.reg(src)?;
                }
            }
            IrInstr::BinaryOp {
                dest, left, right, ..
            } => {
                ctx.reg(dest)?;
                ctx.reg(left)?;
                ctx.reg(right)?;
            }
            IrInstr::UnaryOp { dest, operand, .. } => {
                ctx.reg(dest)?;
                ctx.reg(operand)?;
            }
            IrInstr::Call { dest, args, .. } => {
                if let Some(d) = dest {
                    ctx.reg(d)?;
                }
                for a in args {
                    ctx.reg(a)?;
                }
            }
            IrInstr::CallMethod {
                dest, object, args, ..
            } => {
                if let Some(d) = dest {
                    ctx.reg(d)?;
                }
                ctx.reg(object)?;
                for a in args {
                    ctx.reg(a)?;
                }
            }
            IrInstr::CallClosure {
                dest,
                closure,
                args,
                ..
            } => {
                if let Some(d) = dest {
                    ctx.reg(d)?;
                }
                ctx.reg(closure)?;
                for a in args {
                    ctx.reg(a)?;
                }
            }
            IrInstr::NativeCall { dest, args, .. } | IrInstr::ModuleNativeCall { dest, args, .. } => {
                if let Some(d) = dest {
                    ctx.reg(d)?;
                }
                for a in args {
                    ctx.reg(a)?;
                }
            }
            IrInstr::LoadLocal { dest, .. } => {
                ctx.reg(dest)?;
            }
            IrInstr::StoreLocal { value, .. } => {
                ctx.reg(value)?;
            }
            IrInstr::LoadGlobal { dest, .. } => {
                ctx.reg(dest)?;
            }
            IrInstr::StoreGlobal { value, .. } => {
                ctx.reg(value)?;
            }
            IrInstr::LoadField { dest, object, .. } => {
                ctx.reg(dest)?;
                ctx.reg(object)?;
            }
            IrInstr::StoreField { object, value, .. } => {
                ctx.reg(object)?;
                ctx.reg(value)?;
            }
            IrInstr::JsonLoadProperty { dest, object, .. } => {
                ctx.reg(dest)?;
                ctx.reg(object)?;
            }
            IrInstr::JsonStoreProperty { object, value, .. } => {
                ctx.reg(object)?;
                ctx.reg(value)?;
            }
            IrInstr::LoadElement { dest, array, index } => {
                ctx.reg(dest)?;
                ctx.reg(array)?;
                ctx.reg(index)?;
            }
            IrInstr::StoreElement { array, index, value } => {
                ctx.reg(array)?;
                ctx.reg(index)?;
                ctx.reg(value)?;
            }
            IrInstr::NewObject { dest, .. } => {
                ctx.reg(dest)?;
            }
            IrInstr::NewArray { dest, len, .. } => {
                ctx.reg(dest)?;
                ctx.reg(len)?;
            }
            IrInstr::ArrayLiteral { dest, elements, .. } => {
                ctx.reg(dest)?;
                for e in elements {
                    ctx.reg(e)?;
                }
            }
            IrInstr::ObjectLiteral { dest, fields, .. } => {
                ctx.reg(dest)?;
                for (_, v) in fields {
                    ctx.reg(v)?;
                }
            }
            IrInstr::ArrayLen { dest, array } => {
                ctx.reg(dest)?;
                ctx.reg(array)?;
            }
            IrInstr::ArrayPush { array, element } => {
                ctx.reg(array)?;
                ctx.reg(element)?;
            }
            IrInstr::ArrayPop { dest, array } => {
                ctx.reg(dest)?;
                ctx.reg(array)?;
            }
            IrInstr::InstanceOf { dest, object, .. } | IrInstr::Cast { dest, object, .. } => {
                ctx.reg(dest)?;
                ctx.reg(object)?;
            }
            IrInstr::Typeof { dest, operand }
            | IrInstr::ToString { dest, operand }
            | IrInstr::StringLen { dest, string: operand } => {
                ctx.reg(dest)?;
                ctx.reg(operand)?;
            }
            IrInstr::StringCompare {
                dest, left, right, ..
            } => {
                ctx.reg(dest)?;
                ctx.reg(left)?;
                ctx.reg(right)?;
            }
            IrInstr::MakeClosure {
                dest, captures, ..
            } => {
                ctx.reg(dest)?;
                for c in captures {
                    ctx.reg(c)?;
                }
            }
            IrInstr::LoadCaptured { dest, .. } => {
                ctx.reg(dest)?;
            }
            IrInstr::StoreCaptured { value, .. } => {
                ctx.reg(value)?;
            }
            IrInstr::SetClosureCapture { closure, value, .. } => {
                ctx.reg(closure)?;
                ctx.reg(value)?;
            }
            IrInstr::NewRefCell {
                dest,
                initial_value,
            } => {
                ctx.reg(dest)?;
                ctx.reg(initial_value)?;
            }
            IrInstr::LoadRefCell { dest, refcell } => {
                ctx.reg(dest)?;
                ctx.reg(refcell)?;
            }
            IrInstr::StoreRefCell { refcell, value } => {
                ctx.reg(refcell)?;
                ctx.reg(value)?;
            }
            IrInstr::Spawn { dest, args, .. } => {
                ctx.reg(dest)?;
                for a in args {
                    ctx.reg(a)?;
                }
            }
            IrInstr::SpawnClosure {
                dest,
                closure,
                args,
            } => {
                ctx.reg(dest)?;
                ctx.reg(closure)?;
                for a in args {
                    ctx.reg(a)?;
                }
            }
            IrInstr::Await { dest, task } => {
                ctx.reg(dest)?;
                ctx.reg(task)?;
            }
            IrInstr::AwaitAll { dest, tasks } => {
                ctx.reg(dest)?;
                ctx.reg(tasks)?;
            }
            IrInstr::Sleep { duration_ms } => {
                ctx.reg(duration_ms)?;
            }
            IrInstr::NewMutex { dest } => {
                ctx.reg(dest)?;
            }
            IrInstr::MutexLock { mutex } | IrInstr::MutexUnlock { mutex } => {
                ctx.reg(mutex)?;
            }
            IrInstr::NewChannel { dest, capacity } => {
                ctx.reg(dest)?;
                ctx.reg(capacity)?;
            }
            IrInstr::TaskCancel { task } => {
                ctx.reg(task)?;
            }
            IrInstr::Yield | IrInstr::SetupTry { .. } | IrInstr::EndTry | IrInstr::PopToLocal { .. } | IrInstr::Phi { .. } => {}
        }
        Ok(())
    }

    fn prescan_terminator(&self, ctx: &mut RegFuncCtx, term: &Terminator) -> CompileResult<()> {
        match term {
            Terminator::Return(Some(reg)) => {
                ctx.reg(reg)?;
            }
            Terminator::Branch { cond, .. } => {
                ctx.reg(cond)?;
            }
            Terminator::BranchIfNull { value, .. } => {
                ctx.reg(value)?;
            }
            Terminator::Switch { value, .. } => {
                ctx.reg(value)?;
            }
            Terminator::Throw(reg) => {
                ctx.reg(reg)?;
            }
            Terminator::Return(None) | Terminator::Jump(_) | Terminator::Unreachable => {}
        }
        Ok(())
    }

    /// Emit register bytecode for a single IR instruction.
    fn emit_instr(
        &self,
        ctx: &mut RegFuncCtx,
        instr: &IrInstr,
        module: &mut Module,
        catch_block_regs: &FxHashMap<BasicBlockId, u8>,
    ) -> CompileResult<()> {
        match instr {
            // ============ Assignment ============
            IrInstr::Assign { dest, value } => {
                let d = ctx.get_reg(dest)?;
                match value {
                    IrValue::Register(src) => {
                        let s = ctx.get_reg(src)?;
                        if d != s {
                            ctx.writer.emit_abc(RegOpcode::Move, d, s, 0);
                        }
                    }
                    IrValue::Constant(c) => {
                        self.emit_constant(ctx, d, c, module)?;
                    }
                }
            }

            // ============ Binary Operations ============
            IrInstr::BinaryOp {
                dest,
                op,
                left,
                right,
            } => {
                let d = ctx.get_reg(dest)?;
                let l = ctx.get_reg(left)?;
                let r = ctx.get_reg(right)?;
                let opcode = self.select_binary_op(*op, left.ty.as_u32(), right.ty.as_u32());
                ctx.writer.emit_abc(opcode, d, l, r);
            }

            // ============ Unary Operations ============
            IrInstr::UnaryOp { dest, op, operand } => {
                let d = ctx.get_reg(dest)?;
                let s = ctx.get_reg(operand)?;
                let opcode = match op {
                    UnaryOp::Neg => {
                        if operand.ty.as_u32() == INT_TYPE_ID {
                            RegOpcode::Ineg
                        } else {
                            RegOpcode::Fneg
                        }
                    }
                    UnaryOp::Not => RegOpcode::Not,
                    UnaryOp::BitNot => RegOpcode::Inot,
                };
                ctx.writer.emit_abc(opcode, d, s, 0);
            }

            // ============ Local Variables (= register moves) ============
            IrInstr::LoadLocal { dest, index } => {
                let d = ctx.get_reg(dest)?;
                let src = *index as u8;
                if d != src {
                    ctx.writer.emit_abc(RegOpcode::Move, d, src, 0);
                }
            }

            IrInstr::StoreLocal { index, value } => {
                let dst = *index as u8;
                let s = ctx.get_reg(value)?;
                if dst != s {
                    ctx.writer.emit_abc(RegOpcode::Move, dst, s, 0);
                }
            }

            IrInstr::PopToLocal { .. } => {
                // No-op in register mode: exception handler writes to catch_reg directly
            }

            // ============ Globals ============
            IrInstr::LoadGlobal { dest, index } => {
                let d = ctx.get_reg(dest)?;
                ctx.writer.emit_abx(RegOpcode::LoadGlobal, d, *index);
            }

            IrInstr::StoreGlobal { index, value } => {
                let s = ctx.get_reg(value)?;
                ctx.writer.emit_abx(RegOpcode::StoreGlobal, s, *index);
            }

            // ============ Function Calls ============
            IrInstr::Call { dest, func, args } => {
                let d = dest.as_ref().map(|r| ctx.get_reg(r)).transpose()?.unwrap_or(0);
                let (arg_start, arg_count) = ctx.pack_args(args)?;
                ctx.writer.emit_abcx(
                    RegOpcode::Call,
                    d,
                    arg_start,
                    arg_count,
                    func.as_u32(),
                );
            }

            IrInstr::CallMethod {
                dest,
                object,
                method,
                args,
            } => {
                let d = dest.as_ref().map(|r| ctx.get_reg(r)).transpose()?.unwrap_or(0);
                let (base, total) = ctx.pack_obj_args(object, args)?;
                ctx.writer.emit_abcx(
                    RegOpcode::CallMethod,
                    d,
                    base,
                    total,
                    *method as u32,
                );
            }

            IrInstr::CallClosure {
                dest,
                closure,
                args,
            } => {
                let d = dest.as_ref().map(|r| ctx.get_reg(r)).transpose()?.unwrap_or(0);
                let (base, total) = ctx.pack_obj_args(closure, args)?;
                ctx.writer.emit_abc(RegOpcode::CallClosure, d, base, total);
            }

            IrInstr::NativeCall {
                dest,
                native_id,
                args,
            } => {
                let d = dest.as_ref().map(|r| ctx.get_reg(r)).transpose()?.unwrap_or(0);
                let (arg_start, arg_count) = ctx.pack_args(args)?;
                ctx.writer.emit_abcx(
                    RegOpcode::NativeCall,
                    d,
                    arg_start,
                    arg_count,
                    *native_id as u32,
                );
            }

            IrInstr::ModuleNativeCall {
                dest,
                local_idx,
                args,
            } => {
                let d = dest.as_ref().map(|r| ctx.get_reg(r)).transpose()?.unwrap_or(0);
                let (arg_start, arg_count) = ctx.pack_args(args)?;
                ctx.writer.emit_abcx(
                    RegOpcode::ModuleNativeCall,
                    d,
                    arg_start,
                    arg_count,
                    *local_idx as u32,
                );
            }

            // ============ Object Operations ============
            IrInstr::NewObject { dest, class } => {
                let d = ctx.get_reg(dest)?;
                ctx.writer
                    .emit_abcx(RegOpcode::New, d, 0, 0, class.as_u32());
            }

            IrInstr::LoadField { dest, object, field } => {
                let d = ctx.get_reg(dest)?;
                let o = ctx.get_reg(object)?;
                ctx.writer.emit_abc(RegOpcode::LoadField, d, o, *field as u8);
            }

            IrInstr::StoreField {
                object,
                field,
                value,
            } => {
                let o = ctx.get_reg(object)?;
                let v = ctx.get_reg(value)?;
                ctx.writer.emit_abc(RegOpcode::StoreField, o, *field as u8, v);
            }

            IrInstr::ObjectLiteral {
                dest,
                class,
                fields,
            } => {
                let d = ctx.get_reg(dest)?;
                // Pack field values into contiguous registers
                let field_regs: Vec<Register> = fields.iter().map(|(_, v)| v.clone()).collect();
                let (base, count) = ctx.pack_args(&field_regs)?;
                ctx.writer
                    .emit_abcx(RegOpcode::ObjectLiteral, d, base, count, class.as_u32());
            }

            IrInstr::InstanceOf {
                dest,
                object,
                class_id,
            } => {
                let d = ctx.get_reg(dest)?;
                let o = ctx.get_reg(object)?;
                ctx.writer
                    .emit_abcx(RegOpcode::InstanceOf, d, o, 0, class_id.as_u32());
            }

            IrInstr::Cast {
                dest,
                object,
                class_id,
            } => {
                let d = ctx.get_reg(dest)?;
                let o = ctx.get_reg(object)?;
                ctx.writer
                    .emit_abcx(RegOpcode::Cast, d, o, 0, class_id.as_u32());
            }

            // ============ Array Operations ============
            IrInstr::NewArray { dest, len, .. } => {
                let d = ctx.get_reg(dest)?;
                let l = ctx.get_reg(len)?;
                ctx.writer.emit_abcx(RegOpcode::NewArray, d, l, 0, 0);
            }

            IrInstr::ArrayLiteral { dest, elements, .. } => {
                let d = ctx.get_reg(dest)?;
                let (base, count) = ctx.pack_args(elements)?;
                ctx.writer
                    .emit_abcx(RegOpcode::ArrayLiteral, d, base, count, 0);
            }

            IrInstr::LoadElement { dest, array, index } => {
                let d = ctx.get_reg(dest)?;
                let a = ctx.get_reg(array)?;
                let i = ctx.get_reg(index)?;
                ctx.writer.emit_abc(RegOpcode::LoadElem, d, a, i);
            }

            IrInstr::StoreElement {
                array,
                index,
                value,
            } => {
                let a = ctx.get_reg(array)?;
                let i = ctx.get_reg(index)?;
                let v = ctx.get_reg(value)?;
                ctx.writer.emit_abc(RegOpcode::StoreElem, a, i, v);
            }

            IrInstr::ArrayLen { dest, array } => {
                let d = ctx.get_reg(dest)?;
                let a = ctx.get_reg(array)?;
                ctx.writer.emit_abc(RegOpcode::ArrayLen, d, a, 0);
            }

            IrInstr::ArrayPush { array, element } => {
                let a = ctx.get_reg(array)?;
                let e = ctx.get_reg(element)?;
                ctx.writer.emit_abc(RegOpcode::ArrayPush, a, e, 0);
            }

            IrInstr::ArrayPop { dest, array } => {
                let d = ctx.get_reg(dest)?;
                let a = ctx.get_reg(array)?;
                ctx.writer.emit_abc(RegOpcode::ArrayPop, d, a, 0);
            }

            // ============ JSON (duck typing) ============
            IrInstr::JsonLoadProperty {
                dest,
                object,
                property,
            } => {
                let d = ctx.get_reg(dest)?;
                let o = ctx.get_reg(object)?;
                let str_idx = module.constants.add_string(property.clone());
                ctx.writer
                    .emit_abcx(RegOpcode::JsonGet, d, o, 0, str_idx);
            }

            IrInstr::JsonStoreProperty {
                object,
                property,
                value,
            } => {
                let o = ctx.get_reg(object)?;
                let v = ctx.get_reg(value)?;
                let str_idx = module.constants.add_string(property.clone());
                ctx.writer
                    .emit_abcx(RegOpcode::JsonSet, o, v, 0, str_idx);
            }

            // ============ Strings ============
            IrInstr::StringLen { dest, string } => {
                let d = ctx.get_reg(dest)?;
                let s = ctx.get_reg(string)?;
                ctx.writer.emit_abc(RegOpcode::Slen, d, s, 0);
            }

            IrInstr::StringCompare {
                dest,
                left,
                right,
                mode,
                negate,
            } => {
                let d = ctx.get_reg(dest)?;
                let l = ctx.get_reg(left)?;
                let r = ctx.get_reg(right)?;
                let opcode = match (mode, negate) {
                    (StringCompareMode::Index, false) => RegOpcode::Ieq,
                    (StringCompareMode::Index, true) => RegOpcode::Ine,
                    (StringCompareMode::Full, false) => RegOpcode::Seq,
                    (StringCompareMode::Full, true) => RegOpcode::Sne,
                };
                ctx.writer.emit_abc(opcode, d, l, r);
            }

            IrInstr::ToString { dest, operand } => {
                let d = ctx.get_reg(dest)?;
                let s = ctx.get_reg(operand)?;
                ctx.writer.emit_abc(RegOpcode::ToString, d, s, 0);
            }

            IrInstr::Typeof { dest, operand } => {
                let d = ctx.get_reg(dest)?;
                let s = ctx.get_reg(operand)?;
                ctx.writer.emit_abc(RegOpcode::Typeof, d, s, 0);
            }

            // ============ Closures ============
            IrInstr::MakeClosure {
                dest,
                func,
                captures,
            } => {
                let d = ctx.get_reg(dest)?;
                let (base, count) = ctx.pack_args(captures)?;
                ctx.writer
                    .emit_abcx(RegOpcode::MakeClosure, d, base, count, func.as_u32());
            }

            IrInstr::LoadCaptured { dest, index } => {
                let d = ctx.get_reg(dest)?;
                ctx.writer.emit_abx(RegOpcode::LoadCaptured, d, *index);
            }

            IrInstr::StoreCaptured { index, value } => {
                let v = ctx.get_reg(value)?;
                ctx.writer.emit_abx(RegOpcode::StoreCaptured, v, *index);
            }

            IrInstr::SetClosureCapture {
                closure,
                index,
                value,
            } => {
                let c = ctx.get_reg(closure)?;
                let v = ctx.get_reg(value)?;
                ctx.writer
                    .emit_abc(RegOpcode::SetClosureCapture, c, *index as u8, v);
            }

            IrInstr::NewRefCell {
                dest,
                initial_value,
            } => {
                let d = ctx.get_reg(dest)?;
                let v = ctx.get_reg(initial_value)?;
                ctx.writer.emit_abc(RegOpcode::NewRefCell, d, v, 0);
            }

            IrInstr::LoadRefCell { dest, refcell } => {
                let d = ctx.get_reg(dest)?;
                let r = ctx.get_reg(refcell)?;
                ctx.writer.emit_abc(RegOpcode::LoadRefCell, d, r, 0);
            }

            IrInstr::StoreRefCell { refcell, value } => {
                let r = ctx.get_reg(refcell)?;
                let v = ctx.get_reg(value)?;
                ctx.writer.emit_abc(RegOpcode::StoreRefCell, r, v, 0);
            }

            // ============ Concurrency ============
            IrInstr::Spawn { dest, func, args } => {
                let d = ctx.get_reg(dest)?;
                let (base, count) = ctx.pack_args(args)?;
                ctx.writer
                    .emit_abcx(RegOpcode::Spawn, d, base, count, func.as_u32() as u32);
            }

            IrInstr::SpawnClosure {
                dest,
                closure,
                args,
            } => {
                let d = ctx.get_reg(dest)?;
                let (base, total) = ctx.pack_obj_args(closure, args)?;
                ctx.writer
                    .emit_abc(RegOpcode::SpawnClosure, d, base, total);
            }

            IrInstr::Await { dest, task } => {
                let d = ctx.get_reg(dest)?;
                let t = ctx.get_reg(task)?;
                ctx.writer.emit_abc(RegOpcode::Await, d, t, 0);
            }

            IrInstr::AwaitAll { dest, tasks } => {
                let d = ctx.get_reg(dest)?;
                let t = ctx.get_reg(tasks)?;
                ctx.writer.emit_abc(RegOpcode::AwaitAll, d, t, 0);
            }

            IrInstr::Sleep { duration_ms } => {
                let r = ctx.get_reg(duration_ms)?;
                ctx.writer.emit_abc(RegOpcode::Sleep, r, 0, 0);
            }

            IrInstr::Yield => {
                ctx.writer.emit_abc(RegOpcode::Yield, 0, 0, 0);
            }

            IrInstr::NewMutex { dest } => {
                let d = ctx.get_reg(dest)?;
                ctx.writer.emit_abc(RegOpcode::NewMutex, d, 0, 0);
            }

            IrInstr::MutexLock { mutex } => {
                let m = ctx.get_reg(mutex)?;
                ctx.writer.emit_abc(RegOpcode::MutexLock, m, 0, 0);
            }

            IrInstr::MutexUnlock { mutex } => {
                let m = ctx.get_reg(mutex)?;
                ctx.writer.emit_abc(RegOpcode::MutexUnlock, m, 0, 0);
            }

            IrInstr::NewChannel { dest, capacity } => {
                let d = ctx.get_reg(dest)?;
                let c = ctx.get_reg(capacity)?;
                ctx.writer.emit_abc(RegOpcode::NewChannel, d, c, 0);
            }

            IrInstr::TaskCancel { task } => {
                let t = ctx.get_reg(task)?;
                ctx.writer.emit_abc(RegOpcode::TaskCancel, t, 0, 0);
            }

            // ============ Exception Handling ============
            IrInstr::SetupTry {
                catch_block,
                finally_block,
            } => {
                // Determine catch_reg from the catch block's PopToLocal index
                let catch_reg = catch_block_regs
                    .get(catch_block)
                    .copied()
                    .unwrap_or(0);

                // Emit Try (ABCx): A=catch_reg, extra=placeholder (patched later)
                let pos = ctx.writer.emit_abcx(RegOpcode::Try, catch_reg, 0, 0, 0);
                let extra_pos = pos + 1; // extra word is at pos+1
                ctx.pending_try.push((extra_pos, *catch_block, *finally_block));
            }

            IrInstr::EndTry => {
                ctx.writer.emit_abc(RegOpcode::EndTry, 0, 0, 0);
            }

            // ============ Phi (should not appear) ============
            IrInstr::Phi { .. } => {
                return Err(CompileError::UnsupportedFeature {
                    feature: "PHI nodes in register code generation".to_string(),
                });
            }
        }

        Ok(())
    }

    /// Emit register bytecode for a block terminator.
    fn emit_terminator(&self, ctx: &mut RegFuncCtx, term: &Terminator, module: &mut Module) -> CompileResult<()> {
        match term {
            Terminator::Return(None) => {
                ctx.writer.emit_abc(RegOpcode::ReturnVoid, 0, 0, 0);
            }

            Terminator::Return(Some(reg)) => {
                let r = ctx.get_reg(reg)?;
                ctx.writer.emit_abc(RegOpcode::Return, r, 0, 0);
            }

            Terminator::Jump(target) => {
                let pos = ctx.writer.emit_asbx(RegOpcode::Jmp, 0, 0);
                ctx.record_jump(pos, *target);
            }

            Terminator::Branch {
                cond,
                then_block,
                else_block,
            } => {
                let c = ctx.get_reg(cond)?;
                // JmpIfNot → else, then fallthrough/jump → then
                let else_pos = ctx.writer.emit_asbx(RegOpcode::JmpIfNot, c, 0);
                ctx.record_jump(else_pos, *else_block);
                let then_pos = ctx.writer.emit_asbx(RegOpcode::Jmp, 0, 0);
                ctx.record_jump(then_pos, *then_block);
            }

            Terminator::BranchIfNull {
                value,
                null_block,
                not_null_block,
            } => {
                let v = ctx.get_reg(value)?;
                let null_pos = ctx.writer.emit_asbx(RegOpcode::JmpIfNull, v, 0);
                ctx.record_jump(null_pos, *null_block);
                let not_null_pos = ctx.writer.emit_asbx(RegOpcode::Jmp, 0, 0);
                ctx.record_jump(not_null_pos, *not_null_block);
            }

            Terminator::Switch {
                value,
                cases,
                default,
            } => {
                let val_reg = ctx.get_reg(value)?;
                // Comparison chain: for each case, load constant, compare, jump if equal
                for (case_value, target) in cases {
                    // Use a temp register for the case constant
                    let temp = ctx.temp_base;
                    if temp > ctx.max_reg {
                        ctx.max_reg = temp;
                    }
                    let cmp = ctx.temp_base.wrapping_add(1);
                    if cmp > ctx.max_reg {
                        ctx.max_reg = cmp;
                    }

                    if *case_value >= i16::MIN as i32 && *case_value <= i16::MAX as i32 {
                        ctx.writer
                            .emit_asbx(RegOpcode::LoadInt, temp, *case_value as i16);
                    } else {
                        let idx = module.constants.add_integer(*case_value);
                        let tagged = CONST_TAG_INTEGER | ((idx as u16) & 0x3FFF);
                        ctx.writer.emit_abx(RegOpcode::LoadConst, temp, tagged);
                    }

                    ctx.writer.emit_abc(RegOpcode::Ieq, cmp, val_reg, temp);
                    let jmp_pos = ctx.writer.emit_asbx(RegOpcode::JmpIf, cmp, 0);
                    ctx.record_jump(jmp_pos, *target);
                }
                // Default: unconditional jump
                let def_pos = ctx.writer.emit_asbx(RegOpcode::Jmp, 0, 0);
                ctx.record_jump(def_pos, *default);
            }

            Terminator::Throw(reg) => {
                let r = ctx.get_reg(reg)?;
                ctx.writer.emit_abc(RegOpcode::Throw, r, 0, 0);
            }

            Terminator::Unreachable => {
                ctx.writer.emit_abx(RegOpcode::Trap, 0, 1);
            }
        }
        Ok(())
    }

    /// Emit a constant into a register.
    fn emit_constant(
        &self,
        ctx: &mut RegFuncCtx,
        dest: u8,
        constant: &IrConstant,
        module: &mut Module,
    ) -> CompileResult<()> {
        match constant {
            IrConstant::I32(v) => {
                if *v >= i16::MIN as i32 && *v <= i16::MAX as i32 {
                    ctx.writer.emit_asbx(RegOpcode::LoadInt, dest, *v as i16);
                } else {
                    let idx = module.constants.add_integer(*v);
                    let tagged = CONST_TAG_INTEGER | ((idx as u16) & 0x3FFF);
                    ctx.writer.emit_abx(RegOpcode::LoadConst, dest, tagged);
                }
            }
            IrConstant::F64(v) => {
                let idx = module.constants.add_float(*v);
                let tagged = CONST_TAG_FLOAT | ((idx as u16) & 0x3FFF);
                ctx.writer.emit_abx(RegOpcode::LoadConst, dest, tagged);
            }
            IrConstant::String(s) => {
                let idx = module.constants.add_string(s.clone());
                let tagged = CONST_TAG_STRING | ((idx as u16) & 0x3FFF);
                ctx.writer.emit_abx(RegOpcode::LoadConst, dest, tagged);
            }
            IrConstant::Boolean(true) => {
                ctx.writer.emit_abc(RegOpcode::LoadTrue, dest, 0, 0);
            }
            IrConstant::Boolean(false) => {
                ctx.writer.emit_abc(RegOpcode::LoadFalse, dest, 0, 0);
            }
            IrConstant::Null => {
                ctx.writer.emit_abc(RegOpcode::LoadNil, dest, 0, 0);
            }
        }
        Ok(())
    }

    /// Select the correct typed binary opcode based on operand types.
    fn select_binary_op(&self, op: BinaryOp, left_ty: u32, right_ty: u32) -> RegOpcode {
        let is_string = left_ty == 1 || right_ty == 1;
        let use_generic = left_ty == 3
            || right_ty == 3
            || left_ty == 6
            || right_ty == 6
            || (left_ty > 6 && left_ty != INT_TYPE_ID)
            || (right_ty > 6 && right_ty != INT_TYPE_ID);
        let is_float = left_ty == 0 || right_ty == 0;
        let is_int = left_ty == INT_TYPE_ID && right_ty == INT_TYPE_ID;

        if is_string {
            match op {
                BinaryOp::Add | BinaryOp::Concat => RegOpcode::Sconcat,
                BinaryOp::Equal => RegOpcode::Seq,
                BinaryOp::NotEqual => RegOpcode::Sne,
                BinaryOp::Less => RegOpcode::Slt,
                BinaryOp::LessEqual => RegOpcode::Sle,
                BinaryOp::Greater => RegOpcode::Sgt,
                BinaryOp::GreaterEqual => RegOpcode::Sge,
                _ => self.int_opcode(op),
            }
        } else if use_generic && matches!(op, BinaryOp::Equal | BinaryOp::NotEqual) {
            match op {
                BinaryOp::Equal => RegOpcode::Eq,
                BinaryOp::NotEqual => RegOpcode::Ne,
                _ => unreachable!(),
            }
        } else if use_generic || is_float {
            self.float_opcode(op)
        } else if is_int {
            self.int_opcode(op)
        } else {
            // Default to integer for boolean and unknown types
            self.int_opcode(op)
        }
    }

    fn int_opcode(&self, op: BinaryOp) -> RegOpcode {
        match op {
            BinaryOp::Add => RegOpcode::Iadd,
            BinaryOp::Sub => RegOpcode::Isub,
            BinaryOp::Mul => RegOpcode::Imul,
            BinaryOp::Div => RegOpcode::Idiv,
            BinaryOp::Mod => RegOpcode::Imod,
            BinaryOp::Pow => RegOpcode::Ipow,
            BinaryOp::Equal => RegOpcode::Ieq,
            BinaryOp::NotEqual => RegOpcode::Ine,
            BinaryOp::Less => RegOpcode::Ilt,
            BinaryOp::LessEqual => RegOpcode::Ile,
            BinaryOp::Greater => RegOpcode::Igt,
            BinaryOp::GreaterEqual => RegOpcode::Ige,
            BinaryOp::And => RegOpcode::And,
            BinaryOp::Or => RegOpcode::Or,
            BinaryOp::BitAnd => RegOpcode::Iand,
            BinaryOp::BitOr => RegOpcode::Ior,
            BinaryOp::BitXor => RegOpcode::Ixor,
            BinaryOp::ShiftLeft => RegOpcode::Ishl,
            BinaryOp::ShiftRight => RegOpcode::Ishr,
            BinaryOp::UnsignedShiftRight => RegOpcode::Iushr,
            BinaryOp::Concat => RegOpcode::Sconcat,
        }
    }

    fn float_opcode(&self, op: BinaryOp) -> RegOpcode {
        match op {
            BinaryOp::Add => RegOpcode::Fadd,
            BinaryOp::Sub => RegOpcode::Fsub,
            BinaryOp::Mul => RegOpcode::Fmul,
            BinaryOp::Div => RegOpcode::Fdiv,
            BinaryOp::Mod => RegOpcode::Fmod,
            BinaryOp::Pow => RegOpcode::Fpow,
            BinaryOp::Equal => RegOpcode::Feq,
            BinaryOp::NotEqual => RegOpcode::Fne,
            BinaryOp::Less => RegOpcode::Flt,
            BinaryOp::LessEqual => RegOpcode::Fle,
            BinaryOp::Greater => RegOpcode::Fgt,
            BinaryOp::GreaterEqual => RegOpcode::Fge,
            BinaryOp::And => RegOpcode::And,
            BinaryOp::Or => RegOpcode::Or,
            BinaryOp::BitAnd => RegOpcode::Iand,
            BinaryOp::BitOr => RegOpcode::Ior,
            BinaryOp::BitXor => RegOpcode::Ixor,
            BinaryOp::ShiftLeft => RegOpcode::Ishl,
            BinaryOp::ShiftRight => RegOpcode::Ishr,
            BinaryOp::UnsignedShiftRight => RegOpcode::Iushr,
            BinaryOp::Concat => RegOpcode::Sconcat,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::ir::{BasicBlock, IrFunction, IrModule};
    use crate::compiler::ir::block::Terminator;
    use crate::compiler::ir::value::{IrConstant, IrValue, Register, RegisterId};
    use crate::parser::TypeId;

    fn make_reg(id: u32, ty: u32) -> Register {
        Register::new(RegisterId::new(id), TypeId::new(ty))
    }

    #[test]
    fn test_reg_codegen_return_int() {
        let mut ir = IrModule::new("test");
        let mut func = IrFunction::new("main", vec![], TypeId::new(INT_TYPE_ID));
        let mut entry = BasicBlock::new(BasicBlockId(0));

        let r0 = make_reg(0, INT_TYPE_ID);
        entry.add_instr(IrInstr::Assign {
            dest: r0.clone(),
            value: IrValue::Constant(IrConstant::I32(42)),
        });
        entry.set_terminator(Terminator::Return(Some(r0)));
        func.add_block(entry);
        ir.add_function(func);

        // Build a stub Module
        let stack_module = crate::compiler::codegen::generate(&ir).unwrap();
        let mut module = stack_module;
        let gen = RegCodeGenerator::new(&ir);
        gen.generate(&mut module).unwrap();

        assert!(!module.functions[0].reg_code.is_empty());
        assert!(module.functions[0].register_count >= 1);
    }

    #[test]
    fn test_reg_codegen_binary_add() {
        let mut ir = IrModule::new("test");
        let mut func = IrFunction::new("add", vec![], TypeId::new(INT_TYPE_ID));
        let mut entry = BasicBlock::new(BasicBlockId(0));

        let r0 = make_reg(0, INT_TYPE_ID);
        let r1 = make_reg(1, INT_TYPE_ID);
        let r2 = make_reg(2, INT_TYPE_ID);

        entry.add_instr(IrInstr::Assign {
            dest: r0.clone(),
            value: IrValue::Constant(IrConstant::I32(10)),
        });
        entry.add_instr(IrInstr::Assign {
            dest: r1.clone(),
            value: IrValue::Constant(IrConstant::I32(20)),
        });
        entry.add_instr(IrInstr::BinaryOp {
            dest: r2.clone(),
            op: BinaryOp::Add,
            left: r0,
            right: r1,
        });
        entry.set_terminator(Terminator::Return(Some(r2)));
        func.add_block(entry);
        ir.add_function(func);

        let mut module = crate::compiler::codegen::generate(&ir).unwrap();
        RegCodeGenerator::new(&ir).generate(&mut module).unwrap();

        assert!(module.functions[0].register_count >= 3);
        assert!(!module.functions[0].reg_code.is_empty());
    }

    #[test]
    fn test_reg_codegen_jump() {
        let mut ir = IrModule::new("test");
        let mut func = IrFunction::new("main", vec![], TypeId::new(INT_TYPE_ID));

        // bb0: r0 = 1; jump bb1
        let r0 = make_reg(0, INT_TYPE_ID);
        let mut bb0 = BasicBlock::new(BasicBlockId(0));
        bb0.add_instr(IrInstr::Assign {
            dest: r0.clone(),
            value: IrValue::Constant(IrConstant::I32(1)),
        });
        bb0.set_terminator(Terminator::Jump(BasicBlockId(1)));

        // bb1: return r0
        let mut bb1 = BasicBlock::new(BasicBlockId(1));
        bb1.set_terminator(Terminator::Return(Some(r0)));

        func.add_block(bb0);
        func.add_block(bb1);
        ir.add_function(func);

        let mut module = crate::compiler::codegen::generate(&ir).unwrap();
        RegCodeGenerator::new(&ir).generate(&mut module).unwrap();

        assert!(!module.functions[0].reg_code.is_empty());
    }
}
