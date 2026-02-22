//! Stack-to-SSA lifter
//!
//! Converts stack-based bytecode + CFG into JIT IR by abstractly
//! simulating the operand stack. Each stack slot gets assigned a
//! virtual register, and at merge points (multiple predecessors)
//! Phi nodes are inserted when registers differ.

use rustc_hash::{FxHashMap, FxHashSet};
use crate::compiler::bytecode::{Module, Function, Opcode};
use crate::jit::analysis::decoder::{decode_function, DecodedInstr, Operands};
use crate::jit::analysis::cfg::{build_cfg, BlockId, ControlFlowGraph, CfgTerminator, BranchKind};
use crate::jit::ir::types::JitType;
use crate::jit::ir::instr::*;

/// Error during lifting
#[derive(Debug, thiserror::Error)]
pub enum LiftError {
    #[error("Decode error: {0}")]
    Decode(#[from] crate::jit::analysis::decoder::DecodeError),
    #[error("Stack underflow at offset {offset}")]
    StackUnderflow { offset: usize },
    #[error("Unsupported opcode {opcode:?} at offset {offset}")]
    UnsupportedOpcode { opcode: Opcode, offset: usize },
}

/// Abstract stack state during lifting
struct StackState {
    /// Virtual register occupying each stack slot
    stack: Vec<Reg>,
}

impl StackState {
    fn new() -> Self {
        StackState { stack: Vec::new() }
    }

    fn push(&mut self, reg: Reg) {
        self.stack.push(reg);
    }

    fn pop(&mut self, offset: usize) -> Result<Reg, LiftError> {
        self.stack.pop().ok_or(LiftError::StackUnderflow { offset })
    }

    fn peek(&self) -> Option<Reg> {
        self.stack.last().copied()
    }

    fn _depth(&self) -> usize {
        self.stack.len()
    }

    fn clone_state(&self) -> Vec<Reg> {
        self.stack.clone()
    }

    fn from_regs(regs: Vec<Reg>) -> Self {
        StackState { stack: regs }
    }
}

/// Compute reverse post-order traversal of CFG blocks.
/// Ensures predecessors (excluding back-edges) are processed before successors.
fn compute_rpo(cfg: &ControlFlowGraph) -> Vec<BlockId> {
    let mut visited = FxHashSet::default();
    let mut post_order = Vec::new();

    fn dfs(
        block_id: BlockId,
        cfg: &ControlFlowGraph,
        visited: &mut FxHashSet<BlockId>,
        post_order: &mut Vec<BlockId>,
    ) {
        if !visited.insert(block_id) {
            return;
        }
        for succ in cfg.successors(block_id) {
            dfs(succ, cfg, visited, post_order);
        }
        post_order.push(block_id);
    }

    dfs(cfg.entry, cfg, &mut visited, &mut post_order);
    post_order.reverse();

    // Include any unreachable blocks not visited by DFS (e.g. dead code after return)
    for block in &cfg.blocks {
        if !visited.contains(&block.id) {
            post_order.push(block.id);
        }
    }

    post_order
}

/// Identify loop headers: blocks that have at least one predecessor
/// with a higher RPO index (i.e. a back-edge).
fn identify_loop_headers(
    cfg: &ControlFlowGraph,
    rpo: &[BlockId],
    cfg_to_jit: &FxHashMap<BlockId, JitBlockId>,
) -> FxHashSet<JitBlockId> {
    let rpo_index: FxHashMap<BlockId, usize> = rpo.iter().enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let mut headers = FxHashSet::default();
    for block in &cfg.blocks {
        for pred in &block.predecessors {
            // Back-edge: predecessor has a higher RPO index than this block
            if let (Some(&pred_idx), Some(&block_idx)) =
                (rpo_index.get(pred), rpo_index.get(&block.id))
            {
                if pred_idx >= block_idx {
                    headers.insert(cfg_to_jit[&block.id]);
                }
            }
        }
    }
    headers
}

/// Merge stack states from multiple predecessors. If all predecessors agree
/// on the same register at a slot, use it directly. If they differ, insert
/// a Phi node. Returns the merged stack state.
fn merge_stacks(
    predecessors: &[BlockId],
    exit_stacks: &FxHashMap<BlockId, Vec<Reg>>,
    func: &mut JitFunction,
    jit_block: JitBlockId,
    cfg_to_jit: &FxHashMap<BlockId, JitBlockId>,
) -> StackState {
    // Gather only predecessors that have been processed (have exit stacks)
    let available: Vec<_> = predecessors.iter()
        .filter(|p| exit_stacks.contains_key(p))
        .copied()
        .collect();

    if available.is_empty() {
        return StackState::new();
    }

    // Use the minimum depth (safe: structured control flow should agree)
    let depth = available.iter()
        .map(|p| exit_stacks[p].len())
        .min()
        .unwrap_or(0);

    if depth == 0 {
        return StackState::new();
    }

    let mut merged = Vec::with_capacity(depth);
    for (slot, &first_reg) in exit_stacks[&available[0]].iter().enumerate().take(depth) {
        let all_same = available.iter().all(|p| exit_stacks[p][slot] == first_reg);

        if all_same {
            merged.push(first_reg);
        } else {
            // Different registers — insert Phi
            let ty = func.reg_type(first_reg);
            let dest = func.alloc_reg(ty);
            let sources: Vec<(JitBlockId, Reg)> = available.iter()
                .map(|p| (cfg_to_jit[p], exit_stacks[p][slot]))
                .collect();
            func.block_mut(jit_block).instrs.push(JitInstr::Phi { dest, sources });
            merged.push(dest);
        }
    }

    StackState::from_regs(merged)
}

/// Lift a bytecode function into JIT IR
pub fn lift_function(
    func: &Function,
    _module: &Module,
    func_index: u32,
) -> Result<JitFunction, LiftError> {
    let instrs = decode_function(&func.code)?;
    let cfg = build_cfg(&instrs);

    let name = if func.name.is_empty() {
        format!("func_{}", func_index)
    } else {
        func.name.clone()
    };
    let param_count = func.param_count;
    let local_count = func.local_count;

    let mut jit_func = JitFunction::new(func_index, name, param_count, local_count);

    // Create JIT blocks corresponding to CFG blocks
    let mut cfg_to_jit: FxHashMap<BlockId, JitBlockId> = FxHashMap::default();
    for cfg_block in &cfg.blocks {
        let jit_block = jit_func.add_block();
        cfg_to_jit.insert(cfg_block.id, jit_block);
    }

    if cfg.blocks.is_empty() {
        return Ok(jit_func);
    }

    jit_func.entry = cfg_to_jit[&cfg.entry];

    // Wire up JIT block predecessors from CFG predecessor data
    for cfg_block in &cfg.blocks {
        let jit_id = cfg_to_jit[&cfg_block.id];
        let jit_preds: Vec<JitBlockId> = cfg_block.predecessors.iter()
            .filter_map(|p| cfg_to_jit.get(p).copied())
            .collect();
        jit_func.block_mut(jit_id).predecessors = jit_preds;
    }

    // Build CFG block lookup by ID
    let cfg_block_map: FxHashMap<BlockId, usize> = cfg.blocks.iter()
        .enumerate()
        .map(|(i, b)| (b.id, i))
        .collect();

    // Compute reverse post-order for proper loop handling
    let rpo = compute_rpo(&cfg);
    let loop_headers = identify_loop_headers(&cfg, &rpo, &cfg_to_jit);

    // Lift each CFG block in RPO
    let mut block_exit_stacks: FxHashMap<BlockId, Vec<Reg>> = FxHashMap::default();

    for &cfg_block_id in &rpo {
        let block_idx = cfg_block_map[&cfg_block_id];
        let cfg_block = &cfg.blocks[block_idx];
        let jit_block_id = cfg_to_jit[&cfg_block_id];

        // Determine entry stack state from predecessors
        let mut stack = if cfg_block.predecessors.is_empty() {
            // Entry block or unreachable — start with empty stack
            StackState::new()
        } else if cfg_block.predecessors.len() == 1 {
            // Single predecessor — clone its exit stack if available
            let pred = cfg_block.predecessors[0];
            match block_exit_stacks.get(&pred) {
                Some(exit) => StackState::from_regs(exit.clone()),
                None => StackState::new(), // Back-edge not yet processed
            }
        } else {
            // Multiple predecessors — merge stacks, inserting Phi where needed
            merge_stacks(
                &cfg_block.predecessors,
                &block_exit_stacks,
                &mut jit_func,
                jit_block_id,
                &cfg_to_jit,
            )
        };

        // Lift each instruction in this block
        for &instr_idx in &cfg_block.instrs {
            let instr = &instrs[instr_idx];
            lift_instruction(instr, &mut jit_func, jit_block_id, &mut stack, &cfg_to_jit)?;
        }

        // Set terminator based on CFG terminator
        let term = match &cfg_block.terminator {
            CfgTerminator::Fallthrough(target) => {
                JitTerminator::Jump(cfg_to_jit[target])
            }
            CfgTerminator::Jump(target) => {
                // Detect back-edge: target has lower or equal block ID (loop back-jump)
                if target.0 <= cfg_block_id.0 {
                    // Insert preemption check before backward jump
                    jit_func.block_mut(jit_block_id).instrs.push(JitInstr::CheckPreemption);
                }
                JitTerminator::Jump(cfg_to_jit[target])
            }
            CfgTerminator::Branch { kind, then_block, else_block } => {
                let cond = stack.peek().unwrap_or(Reg(0));
                // Check for back-edge branches too (e.g. JmpIfTrue looping back)
                if then_block.0 <= cfg_block_id.0 || else_block.0 <= cfg_block_id.0 {
                    jit_func.block_mut(jit_block_id).instrs.push(JitInstr::CheckPreemption);
                }
                match kind {
                    BranchKind::IfFalse | BranchKind::IfTrue => {
                        JitTerminator::Branch {
                            cond,
                            then_block: cfg_to_jit[then_block],
                            else_block: cfg_to_jit[else_block],
                        }
                    }
                    BranchKind::IfNull | BranchKind::IfNotNull => {
                        JitTerminator::BranchNull {
                            value: cond,
                            null_block: cfg_to_jit[then_block],
                            not_null_block: cfg_to_jit[else_block],
                        }
                    }
                }
            }
            CfgTerminator::Return => {
                let val = stack.peek();
                JitTerminator::Return(val)
            }
            CfgTerminator::ReturnVoid => {
                JitTerminator::Return(None)
            }
            CfgTerminator::Throw => {
                let val = stack.peek().unwrap_or(Reg(0));
                JitTerminator::Throw(val)
            }
            CfgTerminator::Trap(_code) => {
                JitTerminator::Unreachable
            }
            CfgTerminator::None => {
                JitTerminator::None
            }
        };

        jit_func.block_mut(jit_block_id).terminator = term;
        block_exit_stacks.insert(cfg_block_id, stack.clone_state());
    }

    // Phase 2: Fix up Phi nodes at loop headers.
    // Back-edge predecessors weren't processed when the header was lifted,
    // so their stack entries are missing from Phi sources.
    let rpo_index: FxHashMap<BlockId, usize> = rpo.iter().enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    for &header_jit_id in &loop_headers {
        // Find the CFG block for this header
        let header_cfg_id = cfg_to_jit.iter()
            .find(|(_, &jit)| jit == header_jit_id)
            .map(|(&cfg_id, _)| cfg_id)
            .unwrap();
        let block_idx = cfg_block_map[&header_cfg_id];
        let cfg_block = &cfg.blocks[block_idx];
        let header_rpo = rpo_index[&header_cfg_id];

        // Collect back-edge predecessors and their exit stacks
        let back_edge_preds: Vec<(JitBlockId, Vec<Reg>)> = cfg_block.predecessors.iter()
            .filter_map(|pred| {
                let pred_rpo = rpo_index.get(pred)?;
                if *pred_rpo >= header_rpo {
                    let exit = block_exit_stacks.get(pred)?;
                    Some((cfg_to_jit[pred], exit.clone()))
                } else {
                    None
                }
            })
            .collect();

        if back_edge_preds.is_empty() {
            continue;
        }

        // Find Phi instruction indices and update their sources
        let block = jit_func.block_mut(header_jit_id);
        let mut phi_slot = 0usize;
        for i in 0..block.instrs.len() {
            if let JitInstr::Phi { ref mut sources, .. } = block.instrs[i] {
                for (pred_jit, exit_stack) in &back_edge_preds {
                    let already_has = sources.iter().any(|(b, _)| b == pred_jit);
                    if !already_has && phi_slot < exit_stack.len() {
                        sources.push((*pred_jit, exit_stack[phi_slot]));
                    }
                }
                phi_slot += 1;
            }
        }
    }

    Ok(jit_func)
}

/// Lift a single decoded instruction, updating the abstract stack
fn lift_instruction(
    instr: &DecodedInstr,
    func: &mut JitFunction,
    block: JitBlockId,
    stack: &mut StackState,
    _cfg_to_jit: &FxHashMap<BlockId, JitBlockId>,
) -> Result<(), LiftError> {
    match instr.opcode {
        // ===== Stack Manipulation =====
        Opcode::Nop => {}
        Opcode::Pop => {
            let _ = stack.pop(instr.offset)?;
        }
        Opcode::Dup => {
            let top = stack.pop(instr.offset)?;
            let dup = func.alloc_reg(func.reg_type(top));
            func.block_mut(block).instrs.push(JitInstr::Move { dest: dup, src: top });
            stack.push(top);
            stack.push(dup);
        }
        Opcode::Swap => {
            let a = stack.pop(instr.offset)?;
            let b = stack.pop(instr.offset)?;
            stack.push(a);
            stack.push(b);
        }

        // ===== Constants =====
        Opcode::ConstNull => {
            let dest = func.alloc_reg(JitType::Value);
            func.block_mut(block).instrs.push(JitInstr::ConstNull { dest });
            stack.push(dest);
        }
        Opcode::ConstTrue => {
            let dest = func.alloc_reg(JitType::Bool);
            func.block_mut(block).instrs.push(JitInstr::ConstBool { dest, value: true });
            stack.push(dest);
        }
        Opcode::ConstFalse => {
            let dest = func.alloc_reg(JitType::Bool);
            func.block_mut(block).instrs.push(JitInstr::ConstBool { dest, value: false });
            stack.push(dest);
        }
        Opcode::ConstI32 => {
            if let Operands::I32(value) = instr.operands {
                let dest = func.alloc_reg(JitType::I32);
                func.block_mut(block).instrs.push(JitInstr::ConstI32 { dest, value });
                stack.push(dest);
            }
        }
        Opcode::ConstF64 => {
            if let Operands::F64(value) = instr.operands {
                let dest = func.alloc_reg(JitType::F64);
                func.block_mut(block).instrs.push(JitInstr::ConstF64 { dest, value });
                stack.push(dest);
            }
        }
        Opcode::ConstStr => {
            if let Operands::U16(index) = instr.operands {
                let dest = func.alloc_reg(JitType::Ptr);
                func.block_mut(block).instrs.push(JitInstr::ConstStr { dest, str_index: index });
                stack.push(dest);
            }
        }
        Opcode::LoadConst => {
            if let Operands::U32(index) = instr.operands {
                let dest = func.alloc_reg(JitType::Value);
                func.block_mut(block).instrs.push(JitInstr::LoadConst { dest, const_index: index });
                stack.push(dest);
            }
        }

        // ===== Local Variables =====
        Opcode::LoadLocal => {
            if let Operands::U16(index) = instr.operands {
                let dest = func.alloc_reg(JitType::Value);
                func.block_mut(block).instrs.push(JitInstr::LoadLocal { dest, index });
                stack.push(dest);
            }
        }
        Opcode::LoadLocal0 => {
            let dest = func.alloc_reg(JitType::Value);
            func.block_mut(block).instrs.push(JitInstr::LoadLocal { dest, index: 0 });
            stack.push(dest);
        }
        Opcode::LoadLocal1 => {
            let dest = func.alloc_reg(JitType::Value);
            func.block_mut(block).instrs.push(JitInstr::LoadLocal { dest, index: 1 });
            stack.push(dest);
        }
        Opcode::StoreLocal => {
            if let Operands::U16(index) = instr.operands {
                let value = stack.pop(instr.offset)?;
                func.block_mut(block).instrs.push(JitInstr::StoreLocal { index, value });
            }
        }
        Opcode::StoreLocal0 => {
            let value = stack.pop(instr.offset)?;
            func.block_mut(block).instrs.push(JitInstr::StoreLocal { index: 0, value });
        }
        Opcode::StoreLocal1 => {
            let value = stack.pop(instr.offset)?;
            func.block_mut(block).instrs.push(JitInstr::StoreLocal { index: 1, value });
        }
        Opcode::GetArgCount => {
            // GetArgCount doesn't have any runtime effect in JIT (it reads from call frame)
            // For now, we'll treat it as a no-op since JIT has different semantics
            // TODO: Implement proper GetArgCount support in JIT
        }
        Opcode::LoadArgLocal => {
            // LoadArgLocal loads from a dynamic local index
            // For now, just push 0 as a placeholder
            let _index = stack.pop(instr.offset)?;
            let dest = func.alloc_reg(JitType::Value);
            func.block_mut(block).instrs.push(JitInstr::ConstI32 { dest, value: 0 });
            stack.push(dest);
        }

        // ===== Integer Arithmetic =====
        Opcode::Iadd => lift_binary_i32(func, block, stack, instr.offset, |d, l, r| JitInstr::IAdd { dest: d, left: l, right: r })?,
        Opcode::Isub => lift_binary_i32(func, block, stack, instr.offset, |d, l, r| JitInstr::ISub { dest: d, left: l, right: r })?,
        Opcode::Imul => lift_binary_i32(func, block, stack, instr.offset, |d, l, r| JitInstr::IMul { dest: d, left: l, right: r })?,
        Opcode::Idiv => lift_binary_i32(func, block, stack, instr.offset, |d, l, r| JitInstr::IDiv { dest: d, left: l, right: r })?,
        Opcode::Imod => lift_binary_i32(func, block, stack, instr.offset, |d, l, r| JitInstr::IMod { dest: d, left: l, right: r })?,
        Opcode::Ipow => lift_binary_i32(func, block, stack, instr.offset, |d, l, r| JitInstr::IPow { dest: d, left: l, right: r })?,
        Opcode::Ineg => lift_unary_i32(func, block, stack, instr.offset, |d, o| JitInstr::INeg { dest: d, operand: o })?,

        // ===== Integer Bitwise =====
        Opcode::Ishl => lift_binary_i32(func, block, stack, instr.offset, |d, l, r| JitInstr::IShl { dest: d, left: l, right: r })?,
        Opcode::Ishr => lift_binary_i32(func, block, stack, instr.offset, |d, l, r| JitInstr::IShr { dest: d, left: l, right: r })?,
        Opcode::Iushr => lift_binary_i32(func, block, stack, instr.offset, |d, l, r| JitInstr::IUshr { dest: d, left: l, right: r })?,
        Opcode::Iand => lift_binary_i32(func, block, stack, instr.offset, |d, l, r| JitInstr::IAnd { dest: d, left: l, right: r })?,
        Opcode::Ior => lift_binary_i32(func, block, stack, instr.offset, |d, l, r| JitInstr::IOr { dest: d, left: l, right: r })?,
        Opcode::Ixor => lift_binary_i32(func, block, stack, instr.offset, |d, l, r| JitInstr::IXor { dest: d, left: l, right: r })?,
        Opcode::Inot => lift_unary_i32(func, block, stack, instr.offset, |d, o| JitInstr::INot { dest: d, operand: o })?,

        // ===== Float Arithmetic =====
        Opcode::Fadd => lift_binary_f64(func, block, stack, instr.offset, |d, l, r| JitInstr::FAdd { dest: d, left: l, right: r })?,
        Opcode::Fsub => lift_binary_f64(func, block, stack, instr.offset, |d, l, r| JitInstr::FSub { dest: d, left: l, right: r })?,
        Opcode::Fmul => lift_binary_f64(func, block, stack, instr.offset, |d, l, r| JitInstr::FMul { dest: d, left: l, right: r })?,
        Opcode::Fdiv => lift_binary_f64(func, block, stack, instr.offset, |d, l, r| JitInstr::FDiv { dest: d, left: l, right: r })?,
        Opcode::Fneg => lift_unary_f64(func, block, stack, instr.offset, |d, o| JitInstr::FNeg { dest: d, operand: o })?,
        Opcode::Fpow => lift_binary_f64(func, block, stack, instr.offset, |d, l, r| JitInstr::FPow { dest: d, left: l, right: r })?,
        Opcode::Fmod => lift_binary_f64(func, block, stack, instr.offset, |d, l, r| JitInstr::FMod { dest: d, left: l, right: r })?,

        // ===== Integer Comparison =====
        Opcode::Ieq => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::ICmpEq { dest: d, left: l, right: r })?,
        Opcode::Ine => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::ICmpNe { dest: d, left: l, right: r })?,
        Opcode::Ilt => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::ICmpLt { dest: d, left: l, right: r })?,
        Opcode::Ile => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::ICmpLe { dest: d, left: l, right: r })?,
        Opcode::Igt => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::ICmpGt { dest: d, left: l, right: r })?,
        Opcode::Ige => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::ICmpGe { dest: d, left: l, right: r })?,

        // ===== Float Comparison =====
        Opcode::Feq => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::FCmpEq { dest: d, left: l, right: r })?,
        Opcode::Fne => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::FCmpNe { dest: d, left: l, right: r })?,
        Opcode::Flt => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::FCmpLt { dest: d, left: l, right: r })?,
        Opcode::Fle => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::FCmpLe { dest: d, left: l, right: r })?,
        Opcode::Fgt => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::FCmpGt { dest: d, left: l, right: r })?,
        Opcode::Fge => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::FCmpGe { dest: d, left: l, right: r })?,

        // ===== String Comparison =====
        Opcode::Seq => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::SCmpEq { dest: d, left: l, right: r })?,
        Opcode::Sne => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::SCmpNe { dest: d, left: l, right: r })?,
        Opcode::Slt => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::SCmpLt { dest: d, left: l, right: r })?,
        Opcode::Sle => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::SCmpLe { dest: d, left: l, right: r })?,
        Opcode::Sgt => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::SCmpGt { dest: d, left: l, right: r })?,
        Opcode::Sge => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::SCmpGe { dest: d, left: l, right: r })?,

        // ===== Generic Comparison =====
        Opcode::Eq => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::Eq { dest: d, left: l, right: r })?,
        Opcode::Ne => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::Ne { dest: d, left: l, right: r })?,
        Opcode::StrictEq => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::StrictEq { dest: d, left: l, right: r })?,
        Opcode::StrictNe => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::StrictNe { dest: d, left: l, right: r })?,

        // ===== Logical =====
        Opcode::Not => {
            let operand = stack.pop(instr.offset)?;
            let dest = func.alloc_reg(JitType::Bool);
            func.block_mut(block).instrs.push(JitInstr::Not { dest, operand });
            stack.push(dest);
        }
        Opcode::And => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::And { dest: d, left: l, right: r })?,
        Opcode::Or => lift_binary_bool(func, block, stack, instr.offset, |d, l, r| JitInstr::Or { dest: d, left: l, right: r })?,
        Opcode::Typeof => {
            let operand = stack.pop(instr.offset)?;
            let dest = func.alloc_reg(JitType::Ptr);
            func.block_mut(block).instrs.push(JitInstr::Typeof { dest, operand });
            stack.push(dest);
        }

        // ===== String Operations =====
        Opcode::Sconcat => {
            let right = stack.pop(instr.offset)?;
            let left = stack.pop(instr.offset)?;
            let dest = func.alloc_reg(JitType::Ptr);
            func.block_mut(block).instrs.push(JitInstr::SConcat { dest, left, right });
            stack.push(dest);
        }
        Opcode::Slen => {
            let string = stack.pop(instr.offset)?;
            let dest = func.alloc_reg(JitType::I32);
            func.block_mut(block).instrs.push(JitInstr::SLen { dest, string });
            stack.push(dest);
        }
        Opcode::ToString => {
            let value = stack.pop(instr.offset)?;
            let dest = func.alloc_reg(JitType::Ptr);
            func.block_mut(block).instrs.push(JitInstr::ToString { dest, value });
            stack.push(dest);
        }

        // ===== Global/Static Variables =====
        Opcode::LoadGlobal => {
            if let Operands::U32(index) = instr.operands {
                let dest = func.alloc_reg(JitType::Value);
                func.block_mut(block).instrs.push(JitInstr::LoadGlobal { dest, index });
                stack.push(dest);
            }
        }
        Opcode::StoreGlobal => {
            if let Operands::U32(index) = instr.operands {
                let value = stack.pop(instr.offset)?;
                func.block_mut(block).instrs.push(JitInstr::StoreGlobal { index, value });
            }
        }
        Opcode::LoadStatic => {
            if let Operands::U32(index) = instr.operands {
                let dest = func.alloc_reg(JitType::Value);
                func.block_mut(block).instrs.push(JitInstr::LoadStatic { dest, index });
                stack.push(dest);
            }
        }
        Opcode::StoreStatic => {
            if let Operands::U32(index) = instr.operands {
                let value = stack.pop(instr.offset)?;
                func.block_mut(block).instrs.push(JitInstr::StoreStatic { index, value });
            }
        }

        // ===== Object Operations =====
        Opcode::New => {
            if let Operands::U32(class_id) = instr.operands {
                let dest = func.alloc_reg(JitType::Ptr);
                func.block_mut(block).instrs.push(JitInstr::NewObject { dest, class_id });
                stack.push(dest);
            }
        }
        Opcode::LoadField => {
            if let Operands::U16(offset) = instr.operands {
                let object = stack.pop(instr.offset)?;
                let dest = func.alloc_reg(JitType::Value);
                func.block_mut(block).instrs.push(JitInstr::LoadField { dest, object, offset });
                stack.push(dest);
            }
        }
        Opcode::StoreField => {
            if let Operands::U16(offset) = instr.operands {
                let value = stack.pop(instr.offset)?;
                let object = stack.pop(instr.offset)?;
                func.block_mut(block).instrs.push(JitInstr::StoreField { object, offset, value });
            }
        }
        Opcode::LoadFieldFast => {
            if let Operands::U16(offset) = instr.operands {
                let object = stack.pop(instr.offset)?;
                let dest = func.alloc_reg(JitType::Value);
                func.block_mut(block).instrs.push(JitInstr::LoadFieldFast { dest, object, offset });
                stack.push(dest);
            }
        }
        Opcode::StoreFieldFast => {
            if let Operands::U16(offset) = instr.operands {
                let value = stack.pop(instr.offset)?;
                let object = stack.pop(instr.offset)?;
                func.block_mut(block).instrs.push(JitInstr::StoreFieldFast { object, offset, value });
            }
        }
        Opcode::OptionalField => {
            if let Operands::U16(offset) = instr.operands {
                let object = stack.pop(instr.offset)?;
                let dest = func.alloc_reg(JitType::Value);
                func.block_mut(block).instrs.push(JitInstr::OptionalField { dest, object, offset });
                stack.push(dest);
            }
        }
        Opcode::InstanceOf => {
            // InstanceOf pops object, pushes bool; class_id on stack below
            let object = stack.pop(instr.offset)?;
            let _class_val = stack.pop(instr.offset)?;
            // The class ID is typically encoded differently, but for now treat as value
            let dest = func.alloc_reg(JitType::Bool);
            func.block_mut(block).instrs.push(JitInstr::InstanceOf { dest, object, class_id: 0 });
            stack.push(dest);
        }
        Opcode::Cast => {
            let object = stack.pop(instr.offset)?;
            let dest = func.alloc_reg(JitType::Ptr);
            func.block_mut(block).instrs.push(JitInstr::Cast { dest, object, class_id: 0 });
            stack.push(dest);
        }

        // ===== Array Operations =====
        Opcode::NewArray => {
            if let Operands::U32(type_index) = instr.operands {
                let dest = func.alloc_reg(JitType::Ptr);
                func.block_mut(block).instrs.push(JitInstr::NewArray { dest, type_index });
                stack.push(dest);
            }
        }
        Opcode::LoadElem => {
            let index = stack.pop(instr.offset)?;
            let array = stack.pop(instr.offset)?;
            let dest = func.alloc_reg(JitType::Value);
            func.block_mut(block).instrs.push(JitInstr::LoadElem { dest, array, index });
            stack.push(dest);
        }
        Opcode::StoreElem => {
            let value = stack.pop(instr.offset)?;
            let index = stack.pop(instr.offset)?;
            let array = stack.pop(instr.offset)?;
            func.block_mut(block).instrs.push(JitInstr::StoreElem { array, index, value });
        }
        Opcode::ArrayLen => {
            let array = stack.pop(instr.offset)?;
            let dest = func.alloc_reg(JitType::I32);
            func.block_mut(block).instrs.push(JitInstr::ArrayLen { dest, array });
            stack.push(dest);
        }
        Opcode::ArrayPush => {
            let value = stack.pop(instr.offset)?;
            let array = stack.pop(instr.offset)?;
            func.block_mut(block).instrs.push(JitInstr::ArrayPush { array, value });
        }
        Opcode::ArrayPop => {
            let array = stack.pop(instr.offset)?;
            let dest = func.alloc_reg(JitType::Value);
            func.block_mut(block).instrs.push(JitInstr::ArrayPop { dest, array });
            stack.push(dest);
        }
        Opcode::ArrayLiteral => {
            if let Operands::ArrayLiteral { type_index, length } = instr.operands {
                let mut elements = Vec::new();
                for _ in 0..length {
                    elements.push(stack.pop(instr.offset)?);
                }
                elements.reverse();
                let dest = func.alloc_reg(JitType::Ptr);
                func.block_mut(block).instrs.push(JitInstr::ArrayLiteral { dest, type_index, elements });
                stack.push(dest);
            }
        }
        Opcode::InitArray => {
            if let Operands::U16(count) = instr.operands {
                let mut elements = Vec::new();
                for _ in 0..count {
                    elements.push(stack.pop(instr.offset)?);
                }
                elements.reverse();
                let dest = func.alloc_reg(JitType::Ptr);
                func.block_mut(block).instrs.push(JitInstr::InitArray { dest, count, elements });
                stack.push(dest);
            }
        }

        // ===== Function Calls =====
        Opcode::Call => {
            if let Operands::Call { func_index: target, arg_count } = instr.operands {
                let mut args = Vec::new();
                for _ in 0..arg_count {
                    args.push(stack.pop(instr.offset)?);
                }
                args.reverse();
                let dest = func.alloc_reg(JitType::Value);
                func.block_mut(block).instrs.push(JitInstr::Call { dest: Some(dest), func_index: target, args });
                stack.push(dest);
            }
        }
        Opcode::CallMethod => {
            if let Operands::Call { func_index: method_index, arg_count } = instr.operands {
                let mut args = Vec::new();
                for _ in 0..arg_count {
                    args.push(stack.pop(instr.offset)?);
                }
                args.reverse();
                let receiver = stack.pop(instr.offset)?;
                let dest = func.alloc_reg(JitType::Value);
                func.block_mut(block).instrs.push(JitInstr::CallMethod { dest: Some(dest), method_index, receiver, args });
                stack.push(dest);
            }
        }
        Opcode::CallConstructor => {
            if let Operands::Call { func_index: class_id, arg_count } = instr.operands {
                let mut args = Vec::new();
                for _ in 0..arg_count {
                    args.push(stack.pop(instr.offset)?);
                }
                args.reverse();
                let dest = func.alloc_reg(JitType::Ptr);
                func.block_mut(block).instrs.push(JitInstr::CallConstructor { dest, class_id, args });
                stack.push(dest);
            }
        }
        Opcode::CallSuper => {
            if let Operands::Call { func_index: method_index, arg_count } = instr.operands {
                let mut args = Vec::new();
                for _ in 0..arg_count {
                    args.push(stack.pop(instr.offset)?);
                }
                args.reverse();
                let dest = func.alloc_reg(JitType::Value);
                func.block_mut(block).instrs.push(JitInstr::CallSuper { dest: Some(dest), method_index, args });
                stack.push(dest);
            }
        }
        Opcode::CallStatic => {
            if let Operands::Call { func_index: target, arg_count } = instr.operands {
                let mut args = Vec::new();
                for _ in 0..arg_count {
                    args.push(stack.pop(instr.offset)?);
                }
                args.reverse();
                let dest = func.alloc_reg(JitType::Value);
                func.block_mut(block).instrs.push(JitInstr::CallStatic { dest: Some(dest), func_index: target, args });
                stack.push(dest);
            }
        }
        Opcode::NativeCall | Opcode::ModuleNativeCall => {
            if let Operands::NativeCall { native_id, arg_count } = instr.operands {
                let mut args = Vec::new();
                for _ in 0..arg_count {
                    args.push(stack.pop(instr.offset)?);
                }
                args.reverse();
                let dest = func.alloc_reg(JitType::Value);
                func.block_mut(block).instrs.push(JitInstr::CallNative { dest: Some(dest), native_id, args });
                stack.push(dest);
            }
        }

        // ===== Closures =====
        Opcode::MakeClosure => {
            if let Operands::MakeClosure { func_index: target, capture_count } = instr.operands {
                let mut captures = Vec::new();
                for _ in 0..capture_count {
                    captures.push(stack.pop(instr.offset)?);
                }
                captures.reverse();
                let dest = func.alloc_reg(JitType::Ptr);
                func.block_mut(block).instrs.push(JitInstr::MakeClosure { dest, func_index: target, captures });
                stack.push(dest);
            }
        }
        Opcode::LoadCaptured => {
            if let Operands::U16(index) = instr.operands {
                let dest = func.alloc_reg(JitType::Value);
                func.block_mut(block).instrs.push(JitInstr::LoadCaptured { dest, index });
                stack.push(dest);
            }
        }
        Opcode::StoreCaptured => {
            if let Operands::U16(index) = instr.operands {
                let value = stack.pop(instr.offset)?;
                func.block_mut(block).instrs.push(JitInstr::StoreCaptured { index, value });
            }
        }
        Opcode::SetClosureCapture => {
            if let Operands::U16(index) = instr.operands {
                let value = stack.pop(instr.offset)?;
                let closure = stack.pop(instr.offset)?;
                func.block_mut(block).instrs.push(JitInstr::SetClosureCapture { closure, index, value });
            }
        }
        Opcode::CloseVar => {
            if let Operands::U16(index) = instr.operands {
                func.block_mut(block).instrs.push(JitInstr::CloseVar { index });
            }
        }

        // ===== RefCell =====
        Opcode::NewRefCell => {
            let value = stack.pop(instr.offset)?;
            let dest = func.alloc_reg(JitType::Ptr);
            func.block_mut(block).instrs.push(JitInstr::NewRefCell { dest, value });
            stack.push(dest);
        }
        Opcode::LoadRefCell => {
            let cell = stack.pop(instr.offset)?;
            let dest = func.alloc_reg(JitType::Value);
            func.block_mut(block).instrs.push(JitInstr::LoadRefCell { dest, cell });
            stack.push(dest);
        }
        Opcode::StoreRefCell => {
            let value = stack.pop(instr.offset)?;
            let cell = stack.pop(instr.offset)?;
            func.block_mut(block).instrs.push(JitInstr::StoreRefCell { cell, value });
        }

        // ===== Concurrency =====
        Opcode::Spawn => {
            if let Operands::Spawn { func_index: target, arg_count } = instr.operands {
                let mut args = Vec::new();
                for _ in 0..arg_count {
                    args.push(stack.pop(instr.offset)?);
                }
                args.reverse();
                let dest = func.alloc_reg(JitType::Ptr);
                func.block_mut(block).instrs.push(JitInstr::Spawn { dest, func_index: target, args });
                stack.push(dest);
            }
        }
        Opcode::SpawnClosure => {
            if let Operands::U16(arg_count) = instr.operands {
                let mut args = Vec::new();
                for _ in 0..arg_count {
                    args.push(stack.pop(instr.offset)?);
                }
                args.reverse();
                let closure = stack.pop(instr.offset)?;
                let dest = func.alloc_reg(JitType::Ptr);
                func.block_mut(block).instrs.push(JitInstr::SpawnClosure { dest, closure, args });
                stack.push(dest);
            }
        }
        Opcode::Await => {
            let task = stack.pop(instr.offset)?;
            let dest = func.alloc_reg(JitType::Value);
            func.block_mut(block).instrs.push(JitInstr::Await { dest, task });
            stack.push(dest);
        }
        Opcode::Yield => {
            func.block_mut(block).instrs.push(JitInstr::Yield);
        }
        Opcode::Sleep => {
            let duration = stack.pop(instr.offset)?;
            func.block_mut(block).instrs.push(JitInstr::Sleep { duration });
        }
        Opcode::NewMutex => {
            let dest = func.alloc_reg(JitType::Ptr);
            func.block_mut(block).instrs.push(JitInstr::NewMutex { dest });
            stack.push(dest);
        }
        Opcode::MutexLock => {
            let mutex = stack.pop(instr.offset)?;
            func.block_mut(block).instrs.push(JitInstr::MutexLock { mutex });
        }
        Opcode::MutexUnlock => {
            let mutex = stack.pop(instr.offset)?;
            func.block_mut(block).instrs.push(JitInstr::MutexUnlock { mutex });
        }
        Opcode::NewChannel => {
            let dest = func.alloc_reg(JitType::Ptr);
            func.block_mut(block).instrs.push(JitInstr::NewChannel { dest });
            stack.push(dest);
        }
        Opcode::NewSemaphore => {
            let dest = func.alloc_reg(JitType::Ptr);
            func.block_mut(block).instrs.push(JitInstr::NewSemaphore { dest });
            stack.push(dest);
        }
        Opcode::SemAcquire => {
            let sem = stack.pop(instr.offset)?;
            func.block_mut(block).instrs.push(JitInstr::SemAcquire { sem });
        }
        Opcode::SemRelease => {
            let sem = stack.pop(instr.offset)?;
            func.block_mut(block).instrs.push(JitInstr::SemRelease { sem });
        }
        Opcode::WaitAll => {
            let tasks = stack.pop(instr.offset)?;
            let dest = func.alloc_reg(JitType::Value);
            func.block_mut(block).instrs.push(JitInstr::WaitAll { dest, tasks });
            stack.push(dest);
        }
        Opcode::TaskCancel => {
            let task = stack.pop(instr.offset)?;
            func.block_mut(block).instrs.push(JitInstr::TaskCancel { task });
        }
        Opcode::TaskThen => {
            if let Operands::U32(callback_index) = instr.operands {
                let task = stack.pop(instr.offset)?;
                func.block_mut(block).instrs.push(JitInstr::TaskThen { task, callback_index });
            }
        }

        // ===== Object/Tuple Literals =====
        Opcode::ObjectLiteral => {
            if let Operands::Call { func_index: type_index, arg_count } = instr.operands {
                let mut fields = Vec::new();
                for _ in 0..arg_count {
                    fields.push(stack.pop(instr.offset)?);
                }
                fields.reverse();
                let dest = func.alloc_reg(JitType::Ptr);
                func.block_mut(block).instrs.push(JitInstr::ObjectLiteral { dest, type_index, fields });
                stack.push(dest);
            }
        }
        Opcode::TupleLiteral => {
            if let Operands::Call { func_index: type_index, arg_count } = instr.operands {
                let mut elements = Vec::new();
                for _ in 0..arg_count {
                    elements.push(stack.pop(instr.offset)?);
                }
                elements.reverse();
                let dest = func.alloc_reg(JitType::Ptr);
                func.block_mut(block).instrs.push(JitInstr::TupleLiteral { dest, type_index, elements });
                stack.push(dest);
            }
        }
        Opcode::TupleGet => {
            let tuple = stack.pop(instr.offset)?;
            let dest = func.alloc_reg(JitType::Value);
            func.block_mut(block).instrs.push(JitInstr::TupleGet { dest, tuple });
            stack.push(dest);
        }
        Opcode::InitObject => {
            if let Operands::U16(count) = instr.operands {
                let mut fields = Vec::new();
                for _ in 0..count {
                    fields.push(stack.pop(instr.offset)?);
                }
                fields.reverse();
                let dest = func.alloc_reg(JitType::Ptr);
                func.block_mut(block).instrs.push(JitInstr::InitObject { dest, count, fields });
                stack.push(dest);
            }
        }
        Opcode::InitTuple => {
            if let Operands::U16(count) = instr.operands {
                let mut elements = Vec::new();
                for _ in 0..count {
                    elements.push(stack.pop(instr.offset)?);
                }
                elements.reverse();
                let dest = func.alloc_reg(JitType::Ptr);
                func.block_mut(block).instrs.push(JitInstr::InitTuple { dest, count, elements });
                stack.push(dest);
            }
        }

        // ===== Module =====
        Opcode::LoadModule => {
            if let Operands::U32(module_index) = instr.operands {
                let dest = func.alloc_reg(JitType::Ptr);
                func.block_mut(block).instrs.push(JitInstr::LoadModule { dest, module_index });
                stack.push(dest);
            }
        }

        // ===== Exception Handling =====
        Opcode::Try => {
            if let Operands::Try { catch_offset, finally_offset } = instr.operands {
                let _catch_target = ((instr.offset as i64) + (catch_offset as i64)) as usize;
                let _finally_target = if finally_offset > 0 {
                    Some(((instr.offset as i64) + (finally_offset as i64)) as usize)
                } else {
                    None
                };
                // We'd need offset_to_block mapping here, but for now emit a simplified version
                // The catch/finally blocks will be resolved later
                func.block_mut(block).instrs.push(JitInstr::SetupTry {
                    catch_block: JitBlockId(0), // placeholder
                    finally_block: None,
                });
            }
        }
        Opcode::EndTry => {
            func.block_mut(block).instrs.push(JitInstr::EndTry);
        }
        Opcode::Throw => {
            let value = stack.pop(instr.offset)?;
            func.block_mut(block).instrs.push(JitInstr::Throw { value });
        }
        Opcode::Rethrow => {
            func.block_mut(block).instrs.push(JitInstr::Rethrow);
        }

        // ===== JSON Operations =====
        Opcode::JsonGet => {
            if let Operands::U32(key_index) = instr.operands {
                let object = stack.pop(instr.offset)?;
                let dest = func.alloc_reg(JitType::Value);
                func.block_mut(block).instrs.push(JitInstr::JsonGet { dest, object, key_index });
                stack.push(dest);
            }
        }
        Opcode::JsonSet => {
            if let Operands::U32(key_index) = instr.operands {
                let value = stack.pop(instr.offset)?;
                let object = stack.pop(instr.offset)?;
                func.block_mut(block).instrs.push(JitInstr::JsonSet { object, key_index, value });
            }
        }
        Opcode::JsonDelete => {
            if let Operands::U32(key_index) = instr.operands {
                let object = stack.pop(instr.offset)?;
                func.block_mut(block).instrs.push(JitInstr::JsonDelete { object, key_index });
            }
        }
        Opcode::JsonIndex => {
            let index = stack.pop(instr.offset)?;
            let object = stack.pop(instr.offset)?;
            let dest = func.alloc_reg(JitType::Value);
            func.block_mut(block).instrs.push(JitInstr::JsonIndex { dest, object, index });
            stack.push(dest);
        }
        Opcode::JsonIndexSet => {
            let value = stack.pop(instr.offset)?;
            let index = stack.pop(instr.offset)?;
            let object = stack.pop(instr.offset)?;
            func.block_mut(block).instrs.push(JitInstr::JsonIndexSet { object, index, value });
        }
        Opcode::JsonPush => {
            let value = stack.pop(instr.offset)?;
            let array = stack.pop(instr.offset)?;
            func.block_mut(block).instrs.push(JitInstr::JsonPush { array, value });
        }
        Opcode::JsonPop => {
            let array = stack.pop(instr.offset)?;
            let dest = func.alloc_reg(JitType::Value);
            func.block_mut(block).instrs.push(JitInstr::JsonPop { dest, array });
            stack.push(dest);
        }
        Opcode::JsonNewObject => {
            let dest = func.alloc_reg(JitType::Ptr);
            func.block_mut(block).instrs.push(JitInstr::JsonNewObject { dest });
            stack.push(dest);
        }
        Opcode::JsonNewArray => {
            let dest = func.alloc_reg(JitType::Ptr);
            func.block_mut(block).instrs.push(JitInstr::JsonNewArray { dest });
            stack.push(dest);
        }
        Opcode::JsonKeys => {
            let object = stack.pop(instr.offset)?;
            let dest = func.alloc_reg(JitType::Ptr);
            func.block_mut(block).instrs.push(JitInstr::JsonKeys { dest, object });
            stack.push(dest);
        }
        Opcode::JsonLength => {
            let object = stack.pop(instr.offset)?;
            let dest = func.alloc_reg(JitType::I32);
            func.block_mut(block).instrs.push(JitInstr::JsonLength { dest, object });
            stack.push(dest);
        }

        // ===== Control Flow (jumps handled as terminators, not instructions) =====
        Opcode::Jmp => {
            // Jump terminator — handled at block level, not instruction level
        }
        Opcode::JmpIfFalse | Opcode::JmpIfTrue => {
            // Pop condition and leave it accessible for the block terminator
            let cond = stack.pop(instr.offset)?;
            stack.push(cond); // Push back so terminator can find it
        }
        Opcode::JmpIfNull | Opcode::JmpIfNotNull => {
            let val = stack.pop(instr.offset)?;
            stack.push(val);
        }

        // ===== Return (handled as terminators) =====
        Opcode::Return => {}
        Opcode::ReturnVoid => {}

        // ===== Trap =====
        Opcode::Trap => {}

        // ===== Debug =====
        Opcode::Debugger => {
            // No-op in JIT — debugger breakpoints are not supported in compiled code
        }

        // ===== Bound Methods =====
        Opcode::BindMethod => {
            // Falls back to interpreter — bound method creation requires GC allocation
        }
    }

    Ok(())
}

// ===== Helper functions for common lifting patterns =====

fn lift_binary_i32(
    func: &mut JitFunction,
    block: JitBlockId,
    stack: &mut StackState,
    offset: usize,
    make_instr: impl FnOnce(Reg, Reg, Reg) -> JitInstr,
) -> Result<(), LiftError> {
    let right = stack.pop(offset)?;
    let left = stack.pop(offset)?;
    let dest = func.alloc_reg(JitType::I32);
    func.block_mut(block).instrs.push(make_instr(dest, left, right));
    stack.push(dest);
    Ok(())
}

fn lift_unary_i32(
    func: &mut JitFunction,
    block: JitBlockId,
    stack: &mut StackState,
    offset: usize,
    make_instr: impl FnOnce(Reg, Reg) -> JitInstr,
) -> Result<(), LiftError> {
    let operand = stack.pop(offset)?;
    let dest = func.alloc_reg(JitType::I32);
    func.block_mut(block).instrs.push(make_instr(dest, operand));
    stack.push(dest);
    Ok(())
}

fn lift_binary_f64(
    func: &mut JitFunction,
    block: JitBlockId,
    stack: &mut StackState,
    offset: usize,
    make_instr: impl FnOnce(Reg, Reg, Reg) -> JitInstr,
) -> Result<(), LiftError> {
    let right = stack.pop(offset)?;
    let left = stack.pop(offset)?;
    let dest = func.alloc_reg(JitType::F64);
    func.block_mut(block).instrs.push(make_instr(dest, left, right));
    stack.push(dest);
    Ok(())
}

fn lift_unary_f64(
    func: &mut JitFunction,
    block: JitBlockId,
    stack: &mut StackState,
    offset: usize,
    make_instr: impl FnOnce(Reg, Reg) -> JitInstr,
) -> Result<(), LiftError> {
    let operand = stack.pop(offset)?;
    let dest = func.alloc_reg(JitType::F64);
    func.block_mut(block).instrs.push(make_instr(dest, operand));
    stack.push(dest);
    Ok(())
}

fn lift_binary_bool(
    func: &mut JitFunction,
    block: JitBlockId,
    stack: &mut StackState,
    offset: usize,
    make_instr: impl FnOnce(Reg, Reg, Reg) -> JitInstr,
) -> Result<(), LiftError> {
    let right = stack.pop(offset)?;
    let left = stack.pop(offset)?;
    let dest = func.alloc_reg(JitType::Bool);
    func.block_mut(block).instrs.push(make_instr(dest, left, right));
    stack.push(dest);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::bytecode::Module;

    fn make_function(code: Vec<u8>, param_count: u16, local_count: u16) -> Function {
        Function {
            name: "test".to_string(),
            param_count: param_count as usize,
            local_count: local_count as usize,
            code,
        }
    }

    fn make_module() -> Module {
        Module {
            magic: *b"RAYA",
            version: 1,
            flags: 0,
            constants: crate::compiler::bytecode::ConstantPool::new(),
            functions: vec![],
            classes: vec![],
            metadata: crate::compiler::bytecode::Metadata {
                name: "test".to_string(),
                source_file: None,
            },
            exports: vec![],
            imports: vec![],
            checksum: [0; 32],
            reflection: None,
            debug_info: None,
            native_functions: vec![],
            jit_hints: vec![],
        }
    }

    fn emit(code: &mut Vec<u8>, op: Opcode) { code.push(op as u8); }
    fn emit_i32(code: &mut Vec<u8>, val: i32) {
        code.push(Opcode::ConstI32 as u8);
        code.extend_from_slice(&val.to_le_bytes());
    }
    fn emit_f64(code: &mut Vec<u8>, val: f64) {
        code.push(Opcode::ConstF64 as u8);
        code.extend_from_slice(&val.to_le_bytes());
    }
    fn emit_load_local(code: &mut Vec<u8>, idx: u16) {
        code.push(Opcode::LoadLocal as u8);
        code.extend_from_slice(&idx.to_le_bytes());
    }
    fn emit_store_local(code: &mut Vec<u8>, idx: u16) {
        code.push(Opcode::StoreLocal as u8);
        code.extend_from_slice(&idx.to_le_bytes());
    }

    #[test]
    fn test_lift_const_return() {
        let mut code = Vec::new();
        emit_i32(&mut code, 42);
        emit(&mut code, Opcode::Return);

        let func = make_function(code, 0, 0);
        let module = make_module();
        let jit_func = lift_function(&func, &module, 0).unwrap();

        assert_eq!(jit_func.name, "test");
        assert!(!jit_func.blocks.is_empty());
        // Should have a ConstI32 instruction
        let entry = jit_func.block(jit_func.entry);
        assert!(entry.instrs.iter().any(|i| matches!(i, JitInstr::ConstI32 { value: 42, .. })));
    }

    #[test]
    fn test_lift_iadd() {
        let mut code = Vec::new();
        emit_i32(&mut code, 3);
        emit_i32(&mut code, 5);
        emit(&mut code, Opcode::Iadd);
        emit(&mut code, Opcode::Return);

        let func = make_function(code, 0, 0);
        let module = make_module();
        let jit_func = lift_function(&func, &module, 0).unwrap();

        let entry = jit_func.block(jit_func.entry);
        assert!(entry.instrs.iter().any(|i| matches!(i, JitInstr::IAdd { .. })));
    }

    #[test]
    fn test_lift_locals() {
        // store local 0, load local 0, return
        let mut code = Vec::new();
        emit_i32(&mut code, 10);
        emit_store_local(&mut code, 0);
        emit_load_local(&mut code, 0);
        emit(&mut code, Opcode::Return);

        let func = make_function(code, 0, 1);
        let module = make_module();
        let jit_func = lift_function(&func, &module, 0).unwrap();

        let entry = jit_func.block(jit_func.entry);
        assert!(entry.instrs.iter().any(|i| matches!(i, JitInstr::StoreLocal { index: 0, .. })));
        assert!(entry.instrs.iter().any(|i| matches!(i, JitInstr::LoadLocal { index: 0, .. })));
    }

    #[test]
    fn test_lift_float_ops() {
        let mut code = Vec::new();
        emit_f64(&mut code, 1.5);
        emit_f64(&mut code, 2.5);
        emit(&mut code, Opcode::Fadd);
        emit(&mut code, Opcode::Return);

        let func = make_function(code, 0, 0);
        let module = make_module();
        let jit_func = lift_function(&func, &module, 0).unwrap();

        let entry = jit_func.block(jit_func.entry);
        assert!(entry.instrs.iter().any(|i| matches!(i, JitInstr::FAdd { .. })));
    }

    #[test]
    fn test_lift_display() {
        let mut code = Vec::new();
        emit_i32(&mut code, 3);
        emit_i32(&mut code, 5);
        emit(&mut code, Opcode::Iadd);
        emit(&mut code, Opcode::Return);

        let func = make_function(code, 0, 0);
        let module = make_module();
        let jit_func = lift_function(&func, &module, 0).unwrap();

        // Verify it can be displayed without panicking
        let display = format!("{}", jit_func);
        assert!(display.contains("function @test"));
        assert!(display.contains("iadd"));
    }

    #[test]
    fn test_lift_empty_function() {
        let func = make_function(vec![], 0, 0);
        let module = make_module();
        let jit_func = lift_function(&func, &module, 0).unwrap();
        assert_eq!(jit_func.blocks.len(), 1); // One empty block from CFG
    }

    fn emit_jmp(code: &mut Vec<u8>, op: Opcode, offset: i32) {
        code.push(op as u8);
        code.extend_from_slice(&offset.to_le_bytes());
    }

    #[test]
    fn test_lift_simple_loop() {
        // while (true) { }
        // offset 0: ConstTrue           (1 byte)
        // offset 1: JmpIfFalse +10      (5 bytes, target = 1+10 = 11)
        // offset 6: Jmp -6              (5 bytes, target = 6+(-6) = 0)
        // offset 11: ReturnVoid         (1 byte)
        let mut code = Vec::new();
        emit(&mut code, Opcode::ConstTrue);
        emit_jmp(&mut code, Opcode::JmpIfFalse, 10);
        emit_jmp(&mut code, Opcode::Jmp, -6);
        emit(&mut code, Opcode::ReturnVoid);

        let func = make_function(code, 0, 0);
        let module = make_module();
        let jit_func = lift_function(&func, &module, 0).unwrap();

        // Should have multiple blocks (header, body, exit)
        assert!(jit_func.blocks.len() >= 3, "expected >= 3 blocks, got {}", jit_func.blocks.len());

        // Should have at least one CheckPreemption (at the back-edge)
        let has_preemption = jit_func.blocks.iter().any(|b| {
            b.instrs.iter().any(|i| matches!(i, JitInstr::CheckPreemption))
        });
        assert!(has_preemption, "expected CheckPreemption at back-edge");

        // Should have a backward Jump terminator (back-edge to header)
        let has_back_edge = jit_func.blocks.iter().any(|b| {
            matches!(b.terminator, JitTerminator::Jump(target) if target.0 < b.id.0)
        });
        assert!(has_back_edge, "expected backward Jump (back-edge)");
    }

    #[test]
    fn test_lift_loop_with_accumulator() {
        // i = 0; while (i < 10) { i = i + 1; } return i;
        // local[0] = i
        //
        // offset 0:  ConstI32 0         (5 bytes)
        // offset 5:  StoreLocal 0       (3 bytes) → i = 0
        // offset 8:  LoadLocal 0        (3 bytes) → push i  [loop header]
        // offset 11: ConstI32 10        (5 bytes) → push 10
        // offset 16: Ilt               (1 byte)  → push i < 10
        // offset 17: JmpIfFalse 22     (5 bytes) → target = 17+22 = 39 (exit)
        // offset 22: LoadLocal 0        (3 bytes) → push i  [loop body]
        // offset 25: ConstI32 1         (5 bytes) → push 1
        // offset 30: Iadd              (1 byte)  → push i + 1
        // offset 31: StoreLocal 0       (3 bytes) → i = i + 1
        // offset 34: Jmp -26           (5 bytes) → target = 34+(-26) = 8 (back-edge)
        // offset 39: LoadLocal 0        (3 bytes) → push i  [exit]
        // offset 42: Return            (1 byte)
        let mut code = Vec::new();
        emit_i32(&mut code, 0);
        emit_store_local(&mut code, 0);
        emit_load_local(&mut code, 0);
        emit_i32(&mut code, 10);
        emit(&mut code, Opcode::Ilt);
        emit_jmp(&mut code, Opcode::JmpIfFalse, 22);
        emit_load_local(&mut code, 0);
        emit_i32(&mut code, 1);
        emit(&mut code, Opcode::Iadd);
        emit_store_local(&mut code, 0);
        emit_jmp(&mut code, Opcode::Jmp, -26);
        emit_load_local(&mut code, 0);
        emit(&mut code, Opcode::Return);

        let func = make_function(code, 0, 1);
        let module = make_module();
        let jit_func = lift_function(&func, &module, 0).unwrap();

        // Should have 4 blocks: init, header, body, exit
        assert!(jit_func.blocks.len() >= 4, "expected >= 4 blocks, got {}", jit_func.blocks.len());

        // Verify LoadLocal/StoreLocal in loop body
        let has_store_local = jit_func.blocks.iter().any(|b| {
            b.instrs.iter().any(|i| matches!(i, JitInstr::StoreLocal { index: 0, .. }))
        });
        assert!(has_store_local, "expected StoreLocal 0 in loop body");

        // Verify IAdd is present
        let has_iadd = jit_func.blocks.iter().any(|b| {
            b.instrs.iter().any(|i| matches!(i, JitInstr::IAdd { .. }))
        });
        assert!(has_iadd, "expected IAdd in loop body");

        // Verify CheckPreemption before back-edge
        let has_preemption = jit_func.blocks.iter().any(|b| {
            b.instrs.iter().any(|i| matches!(i, JitInstr::CheckPreemption))
        });
        assert!(has_preemption, "expected CheckPreemption at back-edge");

        // Verify predecessors are wired — loop header should have 2 predecessors
        let header_block = jit_func.blocks.iter().find(|b| {
            b.predecessors.len() >= 2
        });
        assert!(header_block.is_some(), "expected loop header with >= 2 predecessors");
    }

    #[test]
    fn test_lift_preemption_at_back_edges() {
        // Two back-edges: outer loop with inner loop
        // Simplified: two back-to-back loops
        //
        // Loop 1: offset 0..11
        // offset 0: ConstTrue           (1 byte)
        // offset 1: JmpIfFalse +10      (5 bytes, target = 11)
        // offset 6: Jmp -6              (5 bytes, target = 0)
        //
        // Loop 2: offset 11..22
        // offset 11: ConstTrue          (1 byte)
        // offset 12: JmpIfFalse +10     (5 bytes, target = 22)
        // offset 17: Jmp -6             (5 bytes, target = 11)
        //
        // offset 22: ReturnVoid         (1 byte)
        let mut code = Vec::new();
        emit(&mut code, Opcode::ConstTrue);
        emit_jmp(&mut code, Opcode::JmpIfFalse, 10);
        emit_jmp(&mut code, Opcode::Jmp, -6);
        emit(&mut code, Opcode::ConstTrue);
        emit_jmp(&mut code, Opcode::JmpIfFalse, 10);
        emit_jmp(&mut code, Opcode::Jmp, -6);
        emit(&mut code, Opcode::ReturnVoid);

        let func = make_function(code, 0, 0);
        let module = make_module();
        let jit_func = lift_function(&func, &module, 0).unwrap();

        // Should have at least 2 CheckPreemption instructions (one per back-edge)
        let preemption_count: usize = jit_func.blocks.iter()
            .flat_map(|b| &b.instrs)
            .filter(|i| matches!(i, JitInstr::CheckPreemption))
            .count();
        assert!(preemption_count >= 2, "expected >= 2 CheckPreemption, got {}", preemption_count);
    }
}
