#![allow(missing_docs)]
//! Bytecode adapter (Path B)
//!
//! Lifts `.ryb` bytecode modules through the JIT pipeline to produce
//! functions that can be fed into the AOT state machine transform.
//!
//! This reuses the existing JIT lifter (stack→SSA) at build time rather
//! than runtime. The lifted functions implement the same `AotCompilable`
//! trait as Path A (source IR) functions.

use crate::compiler::bytecode::module::Module;

use super::analysis::{SuspensionAnalysis, SuspensionKind, SuspensionPoint};
use super::statemachine::{SmBlock, SmBlockKind, SmInstr, SmBlockId, SmTerminator, SmI32BinOp, SmF64BinOp, SmCmpOp};
use super::traits::AotCompilable;

#[cfg(all(feature = "aot", feature = "jit"))]
use crate::jit::ir::instr::{JitFunction, JitInstr, JitTerminator, Reg};
#[cfg(all(feature = "aot", feature = "jit"))]
use crate::jit::pipeline::{lifter, optimize::JitOptimizer};

/// Errors that can occur during bytecode lifting.
#[derive(Debug)]
pub enum BytecodeAdapterError {
    /// Failed to decode a function's bytecode.
    DecodeFailed {
        func_index: usize,
        message: String,
    },

    /// Failed to lift bytecode to SSA form.
    LiftFailed {
        func_index: usize,
        message: String,
    },
}

impl std::fmt::Display for BytecodeAdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BytecodeAdapterError::DecodeFailed { func_index, message } => {
                write!(f, "Failed to decode function {}: {}", func_index, message)
            }
            BytecodeAdapterError::LiftFailed { func_index, message } => {
                write!(f, "Failed to lift function {}: {}", func_index, message)
            }
        }
    }
}

impl std::error::Error for BytecodeAdapterError {}

/// Lift all functions in a bytecode module through the JIT pipeline.
///
/// For each function in the module:
/// 1. Decode bytecode to instruction stream
/// 2. Build CFG from jump targets
/// 3. RPO traversal + loop header detection
/// 4. Stack simulation → register assignment
/// 5. Phi node insertion at merge points
///
/// Returns the lifted functions ready for the AOT state machine transform.
///
/// This is the exact same lifting pipeline the JIT uses at runtime,
/// but run at build time.
#[cfg(all(feature = "aot", feature = "jit"))]
pub fn lift_bytecode_module(
    module: &Module,
) -> Result<Vec<LiftedFunction>, BytecodeAdapterError> {
    let optimizer = JitOptimizer::new();
    let mut lifted = Vec::new();

    for (idx, func) in module.functions.iter().enumerate() {
        let mut jit_func = lifter::lift_function(func, module, idx as u32)
            .map_err(|e| BytecodeAdapterError::LiftFailed {
                func_index: idx,
                message: e.to_string(),
            })?;

        optimizer.optimize(&mut jit_func);

        let name = Some(func.name.clone());

        lifted.push(LiftedFunction {
            func_index: idx as u32,
            param_count: func.param_count as u32,
            local_count: func.local_count as u32,
            name,
            jit_func,
        });
    }

    Ok(lifted)
}

/// Stub version when JIT feature is not enabled.
#[cfg(not(all(feature = "aot", feature = "jit")))]
pub fn lift_bytecode_module(
    _module: &Module,
) -> Result<Vec<LiftedFunction>, BytecodeAdapterError> {
    Ok(Vec::new())
}

/// A function lifted from bytecode, ready for AOT compilation.
#[derive(Debug)]
pub struct LiftedFunction {
    /// Index within the source module.
    pub func_index: u32,

    /// Number of parameters.
    pub param_count: u32,

    /// Number of locals.
    pub local_count: u32,

    /// Function name (from module metadata, if available).
    pub name: Option<String>,

    /// The lifted JIT IR (only available when both aot and jit features are enabled).
    #[cfg(all(feature = "aot", feature = "jit"))]
    pub jit_func: JitFunction,
}

/// Helper function to map JitInstr to SuspensionKind (when JIT feature is enabled)
#[cfg(all(feature = "aot", feature = "jit"))]
fn classify_suspension(instr: &JitInstr) -> Option<SuspensionKind> {
    match instr {
        // Always suspends
        JitInstr::Await { .. } => Some(SuspensionKind::Await),
        JitInstr::Yield => Some(SuspensionKind::Yield),
        JitInstr::Sleep { .. } => Some(SuspensionKind::Sleep),

        // May suspend - native call
        JitInstr::CallNative { .. } => Some(SuspensionKind::NativeCall),

        // May suspend - AOT function call
        JitInstr::Call { .. } => Some(SuspensionKind::AotCall),

        // May suspend - mutex lock
        JitInstr::MutexLock { .. } => Some(SuspensionKind::MutexLock),

        // Preemption check
        JitInstr::CheckPreemption => Some(SuspensionKind::PreemptionCheck),

        // Channel operations (if implemented)
        // JitInstr::ChannelRecv { .. } => Some(SuspensionKind::ChannelRecv),
        // JitInstr::ChannelSend { .. } => Some(SuspensionKind::ChannelSend),

        _ => None,
    }
}

impl AotCompilable for LiftedFunction {
    #[cfg(all(feature = "aot", feature = "jit"))]
    fn analyze(&self) -> SuspensionAnalysis {
        let mut points = Vec::new();
        let mut index = 0u32;
        let mut loop_headers = std::collections::HashSet::new();

        // Identify loop headers (blocks with back-edges from later blocks)
        for (block_idx, block) in self.jit_func.blocks.iter().enumerate() {
            for pred in &block.predecessors {
                if pred.0 > block_idx as u32 {
                    loop_headers.insert(block_idx as u32);
                }
            }
        }

        // Walk all blocks and instructions to find suspension points
        for (block_idx, block) in self.jit_func.blocks.iter().enumerate() {
            for (instr_idx, instr) in block.instrs.iter().enumerate() {
                if let Some(kind) = classify_suspension(instr) {
                    points.push(SuspensionPoint {
                        index,
                        block_id: block_idx as u32,
                        instr_index: instr_idx as u32,
                        kind,
                        live_locals: std::collections::HashSet::new(), // TODO: liveness analysis
                    });
                    index += 1;
                }
            }
        }

        let has_suspensions = !points.is_empty();

        SuspensionAnalysis {
            points,
            has_suspensions,
            loop_headers,
        }
    }

    #[cfg(not(all(feature = "aot", feature = "jit")))]
    fn analyze(&self) -> SuspensionAnalysis {
        SuspensionAnalysis::none()
    }

    #[cfg(all(feature = "aot", feature = "jit"))]
    fn emit_blocks(&self) -> Vec<SmBlock> {
        self.jit_func.blocks.iter().enumerate().map(|(idx, jit_block)| {
            let mut instructions = Vec::new();

            // Map each JitInstr to SmInstr
            for instr in &jit_block.instrs {
                if let Some(sm_instr) = map_jit_instr_to_sm(instr) {
                    instructions.push(sm_instr);
                }
            }

            // Map the terminator
            let terminator = map_jit_terminator(&jit_block.terminator);

            SmBlock {
                id: SmBlockId(idx as u32),
                kind: SmBlockKind::Body,
                instructions,
                terminator,
            }
        }).collect()
    }

    #[cfg(not(all(feature = "aot", feature = "jit")))]
    fn emit_blocks(&self) -> Vec<SmBlock> {
        vec![SmBlock {
            id: SmBlockId(0),
            kind: SmBlockKind::Body,
            instructions: vec![SmInstr::ConstNull { dest: 0 }],
            terminator: SmTerminator::Return { value: 0 },
        }]
    }

    fn param_count(&self) -> u32 {
        self.param_count
    }

    fn local_count(&self) -> u32 {
        self.local_count
    }

    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

/// Map a JitInstr to SmInstr (when JIT feature is enabled)
#[cfg(all(feature = "aot", feature = "jit"))]
fn map_jit_instr_to_sm(instr: &JitInstr) -> Option<SmInstr> {
    Some(match instr {
        // Constants
        JitInstr::ConstI32 { dest, value } => SmInstr::ConstI32 {
            dest: dest.0,
            value: *value,
        },
        JitInstr::ConstF64 { dest, value } => SmInstr::ConstF64 {
            dest: dest.0,
            bits: value.to_bits(),
        },
        JitInstr::ConstBool { dest, value } => SmInstr::ConstBool {
            dest: dest.0,
            value: *value,
        },
        JitInstr::ConstNull { dest } => SmInstr::ConstNull {
            dest: dest.0,
        },

        // Integer arithmetic
        JitInstr::IAdd { dest, left, right } => SmInstr::I32BinOp {
            dest: dest.0,
            op: SmI32BinOp::Add,
            left: left.0,
            right: right.0,
        },
        JitInstr::ISub { dest, left, right } => SmInstr::I32BinOp {
            dest: dest.0,
            op: SmI32BinOp::Sub,
            left: left.0,
            right: right.0,
        },
        JitInstr::IMul { dest, left, right } => SmInstr::I32BinOp {
            dest: dest.0,
            op: SmI32BinOp::Mul,
            left: left.0,
            right: right.0,
        },
        JitInstr::IDiv { dest, left, right } => SmInstr::I32BinOp {
            dest: dest.0,
            op: SmI32BinOp::Div,
            left: left.0,
            right: right.0,
        },
        JitInstr::IMod { dest, left, right } => SmInstr::I32BinOp {
            dest: dest.0,
            op: SmI32BinOp::Mod,
            left: left.0,
            right: right.0,
        },

        // Float arithmetic
        JitInstr::FAdd { dest, left, right } => SmInstr::F64BinOp {
            dest: dest.0,
            op: SmF64BinOp::Add,
            left: left.0,
            right: right.0,
        },
        JitInstr::FSub { dest, left, right } => SmInstr::F64BinOp {
            dest: dest.0,
            op: SmF64BinOp::Sub,
            left: left.0,
            right: right.0,
        },
        JitInstr::FMul { dest, left, right } => SmInstr::F64BinOp {
            dest: dest.0,
            op: SmF64BinOp::Mul,
            left: left.0,
            right: right.0,
        },
        JitInstr::FDiv { dest, left, right } => SmInstr::F64BinOp {
            dest: dest.0,
            op: SmF64BinOp::Div,
            left: left.0,
            right: right.0,
        },

        // Comparisons
        JitInstr::ICmpEq { dest, left, right } => SmInstr::I32Cmp {
            dest: dest.0,
            op: SmCmpOp::Eq,
            left: left.0,
            right: right.0,
        },
        JitInstr::ICmpLt { dest, left, right } => SmInstr::I32Cmp {
            dest: dest.0,
            op: SmCmpOp::Lt,
            left: left.0,
            right: right.0,
        },
        JitInstr::FCmpEq { dest, left, right } => SmInstr::F64Cmp {
            dest: dest.0,
            op: SmCmpOp::Eq,
            left: left.0,
            right: right.0,
        },
        JitInstr::FCmpLt { dest, left, right } => SmInstr::F64Cmp {
            dest: dest.0,
            op: SmCmpOp::Lt,
            left: left.0,
            right: right.0,
        },

        // NaN-boxing
        JitInstr::BoxI32 { dest, src } => SmInstr::BoxI32 {
            dest: dest.0,
            src: src.0,
        },
        JitInstr::UnboxI32 { dest, src } => SmInstr::UnboxI32 {
            dest: dest.0,
            src: src.0,
        },
        JitInstr::BoxF64 { dest, src } => SmInstr::BoxF64 {
            dest: dest.0,
            src: src.0,
        },
        JitInstr::UnboxF64 { dest, src } => SmInstr::UnboxF64 {
            dest: dest.0,
            src: src.0,
        },
        JitInstr::BoxBool { dest, src } => SmInstr::BoxBool {
            dest: dest.0,
            src: src.0,
        },
        JitInstr::UnboxBool { dest, src } => SmInstr::UnboxBool {
            dest: dest.0,
            src: src.0,
        },

        // Local variables
        JitInstr::LoadLocal { dest, index } => SmInstr::LoadLocal {
            dest: dest.0,
            index: *index as u32,
        },
        JitInstr::StoreLocal { index, value } => SmInstr::StoreLocal {
            index: *index as u32,
            src: value.0,
        },

        // Phi and Move
        JitInstr::Phi { dest, .. } => SmInstr::Phi {
            dest: dest.0,
            sources: Vec::new(), // TODO: map sources
        },
        JitInstr::Move { dest, src } => SmInstr::Move {
            dest: dest.0,
            src: src.0,
        },

        // For other instructions, use helper calls or stub with Unreachable
        _ => return None, // Skip unsupported instructions for now
    })
}

/// Map a JitTerminator to SmTerminator (when JIT feature is enabled)
#[cfg(all(feature = "aot", feature = "jit"))]
fn map_jit_terminator(terminator: &JitTerminator) -> SmTerminator {
    match terminator {
        JitTerminator::Jump(target) => SmTerminator::Jump(SmBlockId(target.0)),
        JitTerminator::Branch { cond, then_block, else_block } => SmTerminator::Branch {
            cond: cond.0,
            then_block: SmBlockId(then_block.0),
            else_block: SmBlockId(else_block.0),
        },
        JitTerminator::Return(Some(value)) => SmTerminator::Return { value: value.0 },
        JitTerminator::Return(None) => SmTerminator::Return { value: 0 }, // Return null
        JitTerminator::Unreachable => SmTerminator::Return { value: 0 }, // Fallback
        _ => SmTerminator::Return { value: 0 }, // Fallback for other terminators
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lifted_function_compilable() {
        // Note: When JIT feature is not enabled, we can't create a full LiftedFunction
        // This test just verifies the AotCompilable trait is implemented
        #[cfg(all(feature = "aot", feature = "jit"))]
        {
            use crate::jit::ir::instr::JitFunction;
            use crate::jit::ir::types::JitType;

            let jit_func = JitFunction::new(0, "test".to_string(), 2, 4);

            let func = LiftedFunction {
                func_index: 0,
                param_count: 2,
                local_count: 4,
                name: Some("add".to_string()),
                jit_func,
            };

            assert_eq!(func.param_count(), 2);
            assert_eq!(func.local_count(), 4);
            assert_eq!(func.name(), Some("add"));

            let analysis = func.analyze();
            assert!(!analysis.has_suspensions);

            let blocks = func.emit_blocks();
            // Empty JIT function has no blocks
            assert_eq!(blocks.len(), 0);
        }

        #[cfg(not(all(feature = "aot", feature = "jit")))]
        {
            // When JIT is not enabled, LiftedFunction can't be created from bytecode
            // This test just verifies the module compiles
        }
    }
}
