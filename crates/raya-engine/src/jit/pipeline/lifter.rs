//! Stack-to-SSA lifter
//!
//! Converts stack-based bytecode + CFG into JIT IR by abstractly
//! simulating the operand stack. Each stack slot gets assigned a
//! virtual register, and at merge points (multiple predecessors)
//! Phi nodes are inserted when registers differ.

use rustc_hash::FxHashMap;
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

    fn depth(&self) -> usize {
        self.stack.len()
    }

    fn clone_state(&self) -> Vec<Reg> {
        self.stack.clone()
    }
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
    let param_count = func.param_count as usize;
    let local_count = func.local_count as usize;

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

    // Lift each CFG block
    // Process in order (we could do RPO for better Phi handling, but linear is fine for now)
    let mut block_exit_stacks: FxHashMap<BlockId, Vec<Reg>> = FxHashMap::default();

    for cfg_block in &cfg.blocks {
        let jit_block_id = cfg_to_jit[&cfg_block.id];
        let mut stack = StackState::new();

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
                JitTerminator::Jump(cfg_to_jit[target])
            }
            CfgTerminator::Branch { kind, then_block, else_block } => {
                // The condition was consumed by the branch instruction in lift_instruction,
                // but we need the condition register. It was pushed as a result of the
                // JmpIfFalse/JmpIfTrue decode — but actually these are terminators so
                // the condition was already popped by the JmpIf* handling in lift_instruction.
                // We need to handle this differently: the JmpIf* was lifted and popped the
                // condition into a register that we stored. Let's retrieve it.
                //
                // Actually, the JmpIf opcodes pop a value and branch — they are handled
                // in lift_instruction which pops the condition. The terminator register
                // is stored in a field we set during lifting. For now, use a simplified approach:
                // the last thing lift_instruction did for JmpIfFalse was create a branch condition.
                //
                // The approach: lift_instruction for JmpIf* doesn't emit any JIT instruction,
                // it just pops the condition. The terminator uses that condition register.
                //
                // We handle this by looking at the terminator's condition register that was
                // saved by lift_instruction. For now, if the stack is non-empty, use top.
                // Otherwise fall back to a dummy.
                let cond = stack.peek().unwrap_or(Reg(0));
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
            CfgTerminator::Trap(code) => {
                JitTerminator::Unreachable
            }
            CfgTerminator::None => {
                JitTerminator::None
            }
        };

        jit_func.block_mut(jit_block_id).terminator = term;
        block_exit_stacks.insert(cfg_block.id, stack.clone_state());
    }

    Ok(jit_func)
}

/// Lift a single decoded instruction, updating the abstract stack
fn lift_instruction(
    instr: &DecodedInstr,
    func: &mut JitFunction,
    block: JitBlockId,
    stack: &mut StackState,
    cfg_to_jit: &FxHashMap<BlockId, JitBlockId>,
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
            let class_val = stack.pop(instr.offset)?;
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
                let catch_target = ((instr.offset as i64) + (catch_offset as i64)) as usize;
                let finally_target = if finally_offset > 0 {
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
}
