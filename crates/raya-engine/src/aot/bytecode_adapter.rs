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

use super::analysis::SuspensionAnalysis;
use super::statemachine::{SmBlock, SmBlockKind, SmInstr, SmBlockId, SmTerminator};
use super::traits::AotCompilable;

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
pub fn lift_bytecode_module(
    _module: &Module,
) -> Result<Vec<LiftedFunction>, BytecodeAdapterError> {
    // TODO: Wire up the JIT pipeline lifter
    //
    // let pipeline = JitPipeline::new();
    // let mut lifted = Vec::new();
    // for (idx, func) in module.functions.iter().enumerate() {
    //     let jit_func = pipeline.lift_and_optimize(func, module, idx as u32)
    //         .map_err(|e| BytecodeAdapterError::LiftFailed {
    //             func_index: idx,
    //             message: e.to_string(),
    //         })?;
    //     lifted.push(LiftedFunction::from_jit(jit_func, idx as u32));
    // }
    // Ok(lifted)

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

    // TODO: Add JitFunction when wiring up the JIT lifter
    // pub jit_func: JitFunction,
}

impl AotCompilable for LiftedFunction {
    fn analyze(&self) -> SuspensionAnalysis {
        // TODO: Analyze the JitFunction for suspension points.
        // Walk JitInstr variants and classify:
        // - JitInstr::Await → SuspensionKind::Await
        // - JitInstr::Yield → SuspensionKind::Yield
        // - JitInstr::Sleep → SuspensionKind::Sleep
        // - JitInstr::CallNative → SuspensionKind::NativeCall
        // - JitInstr::Call → SuspensionKind::AotCall
        // - JitInstr::MutexLock → SuspensionKind::MutexLock
        // - JitInstr::CheckPreemption → SuspensionKind::PreemptionCheck
        SuspensionAnalysis::none()
    }

    fn emit_blocks(&self) -> Vec<SmBlock> {
        // TODO: Translate JitFunction blocks to SmBlocks.
        // For each JitBlock:
        //   1. Map JitInstr variants to SmInstr variants
        //      (typed arithmetic for unboxed i32/f64 ops, helpers for rest)
        //   2. Map JitTerminator to SmTerminator
        //   3. Map Reg(u32) → u32 register IDs
        //   4. Map JitBlockId → SmBlockId
        //
        // The JIT IR already has typed operations (IAdd, FAdd, etc.)
        // and NaN-boxing conversions (BoxI32, UnboxF64, etc.), so the
        // mapping to SmInstr is mostly 1:1.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lifted_function_compilable() {
        let func = LiftedFunction {
            func_index: 0,
            param_count: 2,
            local_count: 4,
            name: Some("add".to_string()),
        };

        assert_eq!(func.param_count(), 2);
        assert_eq!(func.local_count(), 4);
        assert_eq!(func.name(), Some("add"));

        let analysis = func.analyze();
        assert!(!analysis.has_suspensions);

        let blocks = func.emit_blocks();
        assert_eq!(blocks.len(), 1);
    }
}
