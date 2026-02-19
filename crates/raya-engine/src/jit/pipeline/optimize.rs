//! Backend-agnostic optimization passes on JIT IR
//!
//! Each pass implements the `OptPass` trait and transforms a `JitFunction` in place.

use rustc_hash::FxHashSet;
use crate::jit::ir::instr::{JitFunction, JitInstr, Reg};

/// An optimization pass on JIT IR
pub trait OptPass: Send + Sync {
    /// Name of this pass (for diagnostics)
    fn name(&self) -> &str;
    /// Run the pass, mutating the function in place
    fn run(&self, func: &mut JitFunction);
}

/// Optimizer that runs a sequence of passes
pub struct JitOptimizer {
    passes: Vec<Box<dyn OptPass>>,
}

impl JitOptimizer {
    /// Create an optimizer with the default pass pipeline
    pub fn new() -> Self {
        JitOptimizer {
            passes: vec![
                Box::new(BoxElimination),
                Box::new(CopyPropagation),
                Box::new(ConstantFolding),
                Box::new(DeadCodeElimination),
            ],
        }
    }

    /// Create an empty optimizer (no passes)
    pub fn empty() -> Self {
        JitOptimizer { passes: vec![] }
    }

    /// Add a pass to the pipeline
    pub fn add_pass(&mut self, pass: Box<dyn OptPass>) {
        self.passes.push(pass);
    }

    /// Run all passes in order
    pub fn optimize(&self, func: &mut JitFunction) {
        for pass in &self.passes {
            pass.run(func);
        }
    }
}

impl Default for JitOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

// ===== Pass 1: Box/Unbox Elimination =====

/// Eliminates redundant box/unbox pairs.
///
/// When a value is boxed then immediately unboxed (or vice versa),
/// both operations can be removed, leaving the original register.
/// Pattern: `BoxI32(r1) -> r2`, then `UnboxI32(r2) -> r3` becomes `Move(r3, r1)`.
pub struct BoxElimination;

impl OptPass for BoxElimination {
    fn name(&self) -> &str { "box-elimination" }

    fn run(&self, func: &mut JitFunction) {
        use rustc_hash::FxHashMap;

        // Build a map: dest_reg -> (box_src, box_kind) for all Box instructions
        let mut box_sources: FxHashMap<Reg, (Reg, BoxKind)> = FxHashMap::default();

        for block in &func.blocks {
            for instr in &block.instrs {
                match instr {
                    JitInstr::BoxI32 { dest, src } => { box_sources.insert(*dest, (*src, BoxKind::I32)); }
                    JitInstr::BoxF64 { dest, src } => { box_sources.insert(*dest, (*src, BoxKind::F64)); }
                    JitInstr::BoxBool { dest, src } => { box_sources.insert(*dest, (*src, BoxKind::Bool)); }
                    JitInstr::BoxPtr { dest, src } => { box_sources.insert(*dest, (*src, BoxKind::Ptr)); }
                    _ => {}
                }
            }
        }

        // Replace Unbox(Box(x)) with Move(dest, x)
        for block in &mut func.blocks {
            for instr in &mut block.instrs {
                let replacement = match instr {
                    JitInstr::UnboxI32 { dest, src } => {
                        box_sources.get(src)
                            .filter(|(_, kind)| *kind == BoxKind::I32)
                            .map(|(orig, _)| JitInstr::Move { dest: *dest, src: *orig })
                    }
                    JitInstr::UnboxF64 { dest, src } => {
                        box_sources.get(src)
                            .filter(|(_, kind)| *kind == BoxKind::F64)
                            .map(|(orig, _)| JitInstr::Move { dest: *dest, src: *orig })
                    }
                    JitInstr::UnboxBool { dest, src } => {
                        box_sources.get(src)
                            .filter(|(_, kind)| *kind == BoxKind::Bool)
                            .map(|(orig, _)| JitInstr::Move { dest: *dest, src: *orig })
                    }
                    JitInstr::UnboxPtr { dest, src } => {
                        box_sources.get(src)
                            .filter(|(_, kind)| *kind == BoxKind::Ptr)
                            .map(|(orig, _)| JitInstr::Move { dest: *dest, src: *orig })
                    }
                    _ => None,
                };

                if let Some(new_instr) = replacement {
                    *instr = new_instr;
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoxKind { I32, F64, Bool, Ptr }

// ===== Pass 2: Copy Propagation =====

/// Replaces uses of `Move { dest, src }` with direct use of `src`.
pub struct CopyPropagation;

impl OptPass for CopyPropagation {
    fn name(&self) -> &str { "copy-propagation" }

    fn run(&self, func: &mut JitFunction) {
        use rustc_hash::FxHashMap;

        // Build copy chain: dest -> src for Move instructions
        let mut copies: FxHashMap<Reg, Reg> = FxHashMap::default();
        for block in &func.blocks {
            for instr in &block.instrs {
                if let JitInstr::Move { dest, src } = instr {
                    copies.insert(*dest, *src);
                }
            }
        }

        if copies.is_empty() {
            return;
        }

        // Resolve transitive copies: if r3 = Move(r2) and r2 = Move(r1), then r3 → r1
        let resolved: FxHashMap<Reg, Reg> = copies.keys().map(|&reg| {
            let mut current = reg;
            let mut depth = 0;
            while let Some(&src) = copies.get(&current) {
                current = src;
                depth += 1;
                if depth > 100 { break; } // cycle guard
            }
            (reg, current)
        }).collect();

        // Replace all uses of copied registers
        for block in &mut func.blocks {
            for instr in &mut block.instrs {
                replace_reg_uses(instr, &resolved);
            }
        }
    }
}

/// Replace register uses in an instruction according to a substitution map
fn replace_reg_uses(instr: &mut JitInstr, subs: &rustc_hash::FxHashMap<Reg, Reg>) {
    fn sub(reg: &mut Reg, subs: &rustc_hash::FxHashMap<Reg, Reg>) {
        if let Some(&new_reg) = subs.get(reg) {
            *reg = new_reg;
        }
    }

    match instr {
        // Binary ops
        JitInstr::IAdd { left, right, .. }
        | JitInstr::ISub { left, right, .. }
        | JitInstr::IMul { left, right, .. }
        | JitInstr::IDiv { left, right, .. }
        | JitInstr::IMod { left, right, .. }
        | JitInstr::IPow { left, right, .. }
        | JitInstr::IShl { left, right, .. }
        | JitInstr::IShr { left, right, .. }
        | JitInstr::IUshr { left, right, .. }
        | JitInstr::IAnd { left, right, .. }
        | JitInstr::IOr { left, right, .. }
        | JitInstr::IXor { left, right, .. }
        | JitInstr::FAdd { left, right, .. }
        | JitInstr::FSub { left, right, .. }
        | JitInstr::FMul { left, right, .. }
        | JitInstr::FDiv { left, right, .. }
        | JitInstr::FPow { left, right, .. }
        | JitInstr::FMod { left, right, .. }
        | JitInstr::ICmpEq { left, right, .. }
        | JitInstr::ICmpNe { left, right, .. }
        | JitInstr::ICmpLt { left, right, .. }
        | JitInstr::ICmpLe { left, right, .. }
        | JitInstr::ICmpGt { left, right, .. }
        | JitInstr::ICmpGe { left, right, .. }
        | JitInstr::FCmpEq { left, right, .. }
        | JitInstr::FCmpNe { left, right, .. }
        | JitInstr::FCmpLt { left, right, .. }
        | JitInstr::FCmpLe { left, right, .. }
        | JitInstr::FCmpGt { left, right, .. }
        | JitInstr::FCmpGe { left, right, .. }
        | JitInstr::SCmpEq { left, right, .. }
        | JitInstr::SCmpNe { left, right, .. }
        | JitInstr::SCmpLt { left, right, .. }
        | JitInstr::SCmpLe { left, right, .. }
        | JitInstr::SCmpGt { left, right, .. }
        | JitInstr::SCmpGe { left, right, .. }
        | JitInstr::Eq { left, right, .. }
        | JitInstr::Ne { left, right, .. }
        | JitInstr::StrictEq { left, right, .. }
        | JitInstr::StrictNe { left, right, .. }
        | JitInstr::And { left, right, .. }
        | JitInstr::Or { left, right, .. }
        | JitInstr::SConcat { left, right, .. } => {
            sub(left, subs);
            sub(right, subs);
        }

        // Unary ops
        JitInstr::INeg { operand, .. }
        | JitInstr::INot { operand, .. }
        | JitInstr::FNeg { operand, .. }
        | JitInstr::Not { operand, .. } => {
            sub(operand, subs);
        }

        // Boxing
        JitInstr::BoxI32 { src, .. }
        | JitInstr::BoxF64 { src, .. }
        | JitInstr::BoxBool { src, .. }
        | JitInstr::BoxPtr { src, .. }
        | JitInstr::UnboxI32 { src, .. }
        | JitInstr::UnboxF64 { src, .. }
        | JitInstr::UnboxBool { src, .. }
        | JitInstr::UnboxPtr { src, .. }
        | JitInstr::Move { src, .. } => {
            sub(src, subs);
        }

        // Store ops
        JitInstr::StoreLocal { value, .. }
        | JitInstr::StoreGlobal { value, .. }
        | JitInstr::StoreStatic { value, .. } => {
            sub(value, subs);
        }

        // Phi nodes: substitute sources
        JitInstr::Phi { sources, .. } => {
            for (_, reg) in sources.iter_mut() {
                sub(reg, subs);
            }
        }

        // For remaining complex instructions, we skip replacement for simplicity.
        // A production optimizer would handle all instruction variants.
        _ => {}
    }
}

// ===== Pass 3: Constant Folding =====

/// Folds arithmetic on constant operands.
///
/// `IAdd(ConstI32(3), ConstI32(5))` → `ConstI32(8)`
pub struct ConstantFolding;

impl OptPass for ConstantFolding {
    fn name(&self) -> &str { "constant-folding" }

    fn run(&self, func: &mut JitFunction) {
        use rustc_hash::FxHashMap;

        // Collect constant definitions: reg -> value
        let mut i32_consts: FxHashMap<Reg, i32> = FxHashMap::default();
        let mut f64_consts: FxHashMap<Reg, f64> = FxHashMap::default();

        for block in &func.blocks {
            for instr in &block.instrs {
                match instr {
                    JitInstr::ConstI32 { dest, value } => { i32_consts.insert(*dest, *value); }
                    JitInstr::ConstF64 { dest, value } => { f64_consts.insert(*dest, *value); }
                    _ => {}
                }
            }
        }

        // Fold binary i32 operations with constant inputs
        for block in &mut func.blocks {
            for instr in &mut block.instrs {
                let replacement = match instr {
                    JitInstr::IAdd { dest, left, right } => {
                        match (i32_consts.get(left), i32_consts.get(right)) {
                            (Some(&l), Some(&r)) => {
                                let result = l.wrapping_add(r);
                                i32_consts.insert(*dest, result);
                                Some(JitInstr::ConstI32 { dest: *dest, value: result })
                            }
                            _ => None,
                        }
                    }
                    JitInstr::ISub { dest, left, right } => {
                        match (i32_consts.get(left), i32_consts.get(right)) {
                            (Some(&l), Some(&r)) => {
                                let result = l.wrapping_sub(r);
                                i32_consts.insert(*dest, result);
                                Some(JitInstr::ConstI32 { dest: *dest, value: result })
                            }
                            _ => None,
                        }
                    }
                    JitInstr::IMul { dest, left, right } => {
                        match (i32_consts.get(left), i32_consts.get(right)) {
                            (Some(&l), Some(&r)) => {
                                let result = l.wrapping_mul(r);
                                i32_consts.insert(*dest, result);
                                Some(JitInstr::ConstI32 { dest: *dest, value: result })
                            }
                            _ => None,
                        }
                    }
                    JitInstr::FAdd { dest, left, right } => {
                        match (f64_consts.get(left), f64_consts.get(right)) {
                            (Some(&l), Some(&r)) => {
                                let result = l + r;
                                f64_consts.insert(*dest, result);
                                Some(JitInstr::ConstF64 { dest: *dest, value: result })
                            }
                            _ => None,
                        }
                    }
                    JitInstr::FSub { dest, left, right } => {
                        match (f64_consts.get(left), f64_consts.get(right)) {
                            (Some(&l), Some(&r)) => {
                                let result = l - r;
                                f64_consts.insert(*dest, result);
                                Some(JitInstr::ConstF64 { dest: *dest, value: result })
                            }
                            _ => None,
                        }
                    }
                    JitInstr::FMul { dest, left, right } => {
                        match (f64_consts.get(left), f64_consts.get(right)) {
                            (Some(&l), Some(&r)) => {
                                let result = l * r;
                                f64_consts.insert(*dest, result);
                                Some(JitInstr::ConstF64 { dest: *dest, value: result })
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };

                if let Some(new_instr) = replacement {
                    *instr = new_instr;
                }
            }
        }
    }
}

// ===== Pass 4: Dead Code Elimination =====

/// Removes instructions whose destination register is never used
/// (and which have no side effects).
pub struct DeadCodeElimination;

impl OptPass for DeadCodeElimination {
    fn name(&self) -> &str { "dead-code-elimination" }

    fn run(&self, func: &mut JitFunction) {
        // Collect all used registers (appeared as an operand)
        let mut used_regs = FxHashSet::default();

        for block in &func.blocks {
            for instr in &block.instrs {
                collect_used_regs(instr, &mut used_regs);
            }
            // Also collect registers used in terminators
            collect_terminator_regs(&block.terminator, &mut used_regs);
        }

        // Remove instructions that produce an unused dest and have no side effects
        for block in &mut func.blocks {
            block.instrs.retain(|instr| {
                if instr.has_side_effects() {
                    return true; // Keep side-effectful instructions
                }
                match instr.dest() {
                    Some(dest) => used_regs.contains(&dest),
                    None => true, // No dest = keep (shouldn't happen for pure instrs)
                }
            });
        }
    }
}

fn collect_used_regs(instr: &JitInstr, used: &mut FxHashSet<Reg>) {
    // Collect all register operands (not destinations)
    match instr {
        JitInstr::IAdd { left, right, .. }
        | JitInstr::ISub { left, right, .. }
        | JitInstr::IMul { left, right, .. }
        | JitInstr::IDiv { left, right, .. }
        | JitInstr::IMod { left, right, .. }
        | JitInstr::IPow { left, right, .. }
        | JitInstr::IShl { left, right, .. }
        | JitInstr::IShr { left, right, .. }
        | JitInstr::IUshr { left, right, .. }
        | JitInstr::IAnd { left, right, .. }
        | JitInstr::IOr { left, right, .. }
        | JitInstr::IXor { left, right, .. }
        | JitInstr::FAdd { left, right, .. }
        | JitInstr::FSub { left, right, .. }
        | JitInstr::FMul { left, right, .. }
        | JitInstr::FDiv { left, right, .. }
        | JitInstr::FPow { left, right, .. }
        | JitInstr::FMod { left, right, .. }
        | JitInstr::ICmpEq { left, right, .. }
        | JitInstr::ICmpNe { left, right, .. }
        | JitInstr::ICmpLt { left, right, .. }
        | JitInstr::ICmpLe { left, right, .. }
        | JitInstr::ICmpGt { left, right, .. }
        | JitInstr::ICmpGe { left, right, .. }
        | JitInstr::FCmpEq { left, right, .. }
        | JitInstr::FCmpNe { left, right, .. }
        | JitInstr::FCmpLt { left, right, .. }
        | JitInstr::FCmpLe { left, right, .. }
        | JitInstr::FCmpGt { left, right, .. }
        | JitInstr::FCmpGe { left, right, .. }
        | JitInstr::SCmpEq { left, right, .. }
        | JitInstr::SCmpNe { left, right, .. }
        | JitInstr::SCmpLt { left, right, .. }
        | JitInstr::SCmpLe { left, right, .. }
        | JitInstr::SCmpGt { left, right, .. }
        | JitInstr::SCmpGe { left, right, .. }
        | JitInstr::Eq { left, right, .. }
        | JitInstr::Ne { left, right, .. }
        | JitInstr::StrictEq { left, right, .. }
        | JitInstr::StrictNe { left, right, .. }
        | JitInstr::And { left, right, .. }
        | JitInstr::Or { left, right, .. }
        | JitInstr::SConcat { left, right, .. } => {
            used.insert(*left);
            used.insert(*right);
        }

        JitInstr::INeg { operand, .. }
        | JitInstr::INot { operand, .. }
        | JitInstr::FNeg { operand, .. }
        | JitInstr::Not { operand, .. } => { used.insert(*operand); }

        JitInstr::BoxI32 { src, .. }
        | JitInstr::BoxF64 { src, .. }
        | JitInstr::BoxBool { src, .. }
        | JitInstr::BoxPtr { src, .. }
        | JitInstr::UnboxI32 { src, .. }
        | JitInstr::UnboxF64 { src, .. }
        | JitInstr::UnboxBool { src, .. }
        | JitInstr::UnboxPtr { src, .. }
        | JitInstr::Move { src, .. } => { used.insert(*src); }

        JitInstr::StoreLocal { value, .. }
        | JitInstr::StoreGlobal { value, .. }
        | JitInstr::StoreStatic { value, .. } => { used.insert(*value); }

        JitInstr::StoreField { object, value, .. }
        | JitInstr::StoreFieldFast { object, value, .. } => {
            used.insert(*object);
            used.insert(*value);
        }
        JitInstr::StoreElem { array, index, value } => {
            used.insert(*array);
            used.insert(*index);
            used.insert(*value);
        }

        JitInstr::LoadField { object, .. }
        | JitInstr::LoadFieldFast { object, .. }
        | JitInstr::OptionalField { object, .. }
        | JitInstr::ArrayLen { array: object, .. }
        | JitInstr::ArrayPop { array: object, .. }
        | JitInstr::LoadRefCell { cell: object, .. }
        | JitInstr::Typeof { operand: object, .. }
        | JitInstr::ToString { value: object, .. }
        | JitInstr::SLen { string: object, .. } => { used.insert(*object); }

        JitInstr::LoadElem { array, index, .. } => {
            used.insert(*array);
            used.insert(*index);
        }

        JitInstr::Call { args, .. }
        | JitInstr::CallStatic { args, .. }
        | JitInstr::CallSuper { args, .. } => {
            for arg in args { used.insert(*arg); }
        }
        JitInstr::CallMethod { receiver, args, .. } => {
            used.insert(*receiver);
            for arg in args { used.insert(*arg); }
        }
        JitInstr::CallConstructor { args, .. } => {
            for arg in args { used.insert(*arg); }
        }
        JitInstr::CallNative { args, .. } => {
            for arg in args { used.insert(*arg); }
        }
        JitInstr::CallClosure { closure, args, .. } => {
            used.insert(*closure);
            for arg in args { used.insert(*arg); }
        }

        JitInstr::Throw { value } => { used.insert(*value); }
        JitInstr::Await { task, .. } => { used.insert(*task); }
        JitInstr::Sleep { duration } => { used.insert(*duration); }
        JitInstr::MutexLock { mutex } | JitInstr::MutexUnlock { mutex } => { used.insert(*mutex); }
        JitInstr::ArrayPush { array, value } | JitInstr::JsonPush { array, value } => {
            used.insert(*array);
            used.insert(*value);
        }
        JitInstr::StoreRefCell { cell, value } => {
            used.insert(*cell);
            used.insert(*value);
        }
        JitInstr::NewRefCell { value, .. } => { used.insert(*value); }
        JitInstr::StoreCaptured { value, .. } => { used.insert(*value); }

        JitInstr::Phi { sources, .. } => {
            for (_, reg) in sources { used.insert(*reg); }
        }

        // Instructions with no register operands — skip
        _ => {}
    }
}

fn collect_terminator_regs(term: &crate::jit::ir::instr::JitTerminator, used: &mut FxHashSet<Reg>) {
    match term {
        crate::jit::ir::instr::JitTerminator::Branch { cond, .. } => { used.insert(*cond); }
        crate::jit::ir::instr::JitTerminator::BranchNull { value, .. } => { used.insert(*value); }
        crate::jit::ir::instr::JitTerminator::Return(Some(reg)) => { used.insert(*reg); }
        crate::jit::ir::instr::JitTerminator::Throw(reg) => { used.insert(*reg); }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::ir::instr::*;
    use crate::jit::ir::types::JitType;

    fn make_func() -> JitFunction {
        let mut func = JitFunction::new(0, "test".to_string(), 0, 0);
        func.add_block();
        func
    }

    #[test]
    fn test_box_elimination() {
        let mut func = make_func();
        let r0 = func.alloc_reg(JitType::I32);
        let r1 = func.alloc_reg(JitType::Value);
        let r2 = func.alloc_reg(JitType::I32);

        func.block_mut(JitBlockId(0)).instrs = vec![
            JitInstr::ConstI32 { dest: r0, value: 42 },
            JitInstr::BoxI32 { dest: r1, src: r0 },
            JitInstr::UnboxI32 { dest: r2, src: r1 },
        ];
        func.block_mut(JitBlockId(0)).terminator = JitTerminator::Return(Some(r2));

        BoxElimination.run(&mut func);

        // UnboxI32 should be replaced with Move
        let instrs = &func.block(JitBlockId(0)).instrs;
        assert!(matches!(instrs[2], JitInstr::Move { dest, src } if dest == r2 && src == r0));
    }

    #[test]
    fn test_constant_folding() {
        let mut func = make_func();
        let r0 = func.alloc_reg(JitType::I32);
        let r1 = func.alloc_reg(JitType::I32);
        let r2 = func.alloc_reg(JitType::I32);

        func.block_mut(JitBlockId(0)).instrs = vec![
            JitInstr::ConstI32 { dest: r0, value: 3 },
            JitInstr::ConstI32 { dest: r1, value: 5 },
            JitInstr::IAdd { dest: r2, left: r0, right: r1 },
        ];
        func.block_mut(JitBlockId(0)).terminator = JitTerminator::Return(Some(r2));

        ConstantFolding.run(&mut func);

        let instrs = &func.block(JitBlockId(0)).instrs;
        assert!(matches!(instrs[2], JitInstr::ConstI32 { value: 8, .. }));
    }

    #[test]
    fn test_copy_propagation() {
        let mut func = make_func();
        let r0 = func.alloc_reg(JitType::I32);
        let r1 = func.alloc_reg(JitType::I32);
        let r2 = func.alloc_reg(JitType::I32);
        let r3 = func.alloc_reg(JitType::I32);

        func.block_mut(JitBlockId(0)).instrs = vec![
            JitInstr::ConstI32 { dest: r0, value: 42 },
            JitInstr::Move { dest: r1, src: r0 },
            JitInstr::IAdd { dest: r2, left: r1, right: r1 },
        ];
        func.block_mut(JitBlockId(0)).terminator = JitTerminator::Return(Some(r2));

        CopyPropagation.run(&mut func);

        // IAdd should now use r0 directly instead of r1
        let instrs = &func.block(JitBlockId(0)).instrs;
        if let JitInstr::IAdd { left, right, .. } = &instrs[2] {
            assert_eq!(*left, r0);
            assert_eq!(*right, r0);
        } else {
            panic!("expected IAdd");
        }
    }

    #[test]
    fn test_dead_code_elimination() {
        let mut func = make_func();
        let r0 = func.alloc_reg(JitType::I32);
        let r1 = func.alloc_reg(JitType::I32);
        let r2 = func.alloc_reg(JitType::I32); // unused

        func.block_mut(JitBlockId(0)).instrs = vec![
            JitInstr::ConstI32 { dest: r0, value: 42 },
            JitInstr::ConstI32 { dest: r1, value: 99 },  // dead
            JitInstr::ConstI32 { dest: r2, value: 100 }, // dead
        ];
        func.block_mut(JitBlockId(0)).terminator = JitTerminator::Return(Some(r0));

        DeadCodeElimination.run(&mut func);

        let instrs = &func.block(JitBlockId(0)).instrs;
        assert_eq!(instrs.len(), 1);
        assert!(matches!(instrs[0], JitInstr::ConstI32 { value: 42, .. }));
    }

    #[test]
    fn test_optimizer_pipeline() {
        let mut func = make_func();
        let r0 = func.alloc_reg(JitType::I32);
        let r1 = func.alloc_reg(JitType::I32);
        let r2 = func.alloc_reg(JitType::I32);

        func.block_mut(JitBlockId(0)).instrs = vec![
            JitInstr::ConstI32 { dest: r0, value: 3 },
            JitInstr::ConstI32 { dest: r1, value: 5 },
            JitInstr::IAdd { dest: r2, left: r0, right: r1 },
        ];
        func.block_mut(JitBlockId(0)).terminator = JitTerminator::Return(Some(r2));

        let optimizer = JitOptimizer::new();
        optimizer.optimize(&mut func);

        // After constant folding + DCE: should have just ConstI32(8) + possibly others
        let instrs = &func.block(JitBlockId(0)).instrs;
        // r0 and r1 are dead after folding, but r2 is used by Return
        assert!(instrs.iter().any(|i| matches!(i, JitInstr::ConstI32 { value: 8, .. })));
    }
}
