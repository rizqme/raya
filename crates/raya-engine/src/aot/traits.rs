//! AotCompilable trait
//!
//! Unified interface for both compilation paths:
//! - **Path A** (source IR): `IrFunctionAdapter` in `ir_adapter.rs`
//! - **Path B** (bytecode): `LiftedFunction` in `bytecode_adapter.rs`
//!
//! Both produce the same `SmBlock`/`SmInstr` representation that feeds into
//! the state machine transform and then Cranelift lowering.

use super::analysis::SuspensionAnalysis;
use super::statemachine::{SmBlock, StateMachineFunction, transform_to_state_machine};

/// Trait for types that can be compiled to AOT native code.
///
/// Implementors translate their IR into pre-transform state machine blocks.
/// The blocks contain typed operations (`SmInstr`) with suspension points
/// marked via `CallHelper` with suspension-causing helpers (e.g., `AwaitTask`).
///
/// The pipeline:
/// ```text
/// AotCompilable::analyze()     → SuspensionAnalysis
/// AotCompilable::emit_blocks() → Vec<SmBlock>
/// transform_to_state_machine() → StateMachineFunction
/// lower_function()             → Cranelift IR
/// ```
pub trait AotCompilable {
    /// Analyze this function for suspension points.
    ///
    /// Walks the function body and classifies each instruction:
    /// - Always suspends: Await, Yield, Sleep
    /// - May suspend: NativeCall, CallAot
    /// - Never suspends: arithmetic, field access, local load/store
    /// - Preemption point: loop back-edges
    fn analyze(&self) -> SuspensionAnalysis;

    /// Convert this function to pre-transform state machine blocks.
    ///
    /// Each block contains `SmInstr` operations that the Cranelift lowering
    /// understands. Suspension points are left as `CallHelper` instructions
    /// with suspension-causing helpers — the state machine transform will
    /// later wrap them with save/restore/dispatch machinery.
    fn emit_blocks(&self) -> Vec<SmBlock>;

    /// Number of parameters.
    fn param_count(&self) -> u32;

    /// Number of local variables.
    fn local_count(&self) -> u32;

    /// Function name (for debug info).
    fn name(&self) -> Option<&str>;
}

/// Errors that can occur during AOT compilation.
#[derive(Debug)]
pub enum AotError {
    /// An instruction couldn't be translated to the SM IR.
    UnsupportedInstruction(String),

    /// Suspension analysis failed.
    AnalysisFailed(String),

    /// State machine transform failed.
    TransformFailed(String),

    /// Cranelift lowering failed.
    LoweringFailed(String),

    /// Code generation (ObjectModule) failed.
    CodegenFailed(String),
}

impl std::fmt::Display for AotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AotError::UnsupportedInstruction(msg) => write!(f, "unsupported instruction: {msg}"),
            AotError::AnalysisFailed(msg) => write!(f, "analysis failed: {msg}"),
            AotError::TransformFailed(msg) => write!(f, "transform failed: {msg}"),
            AotError::LoweringFailed(msg) => write!(f, "lowering failed: {msg}"),
            AotError::CodegenFailed(msg) => write!(f, "codegen failed: {msg}"),
        }
    }
}

impl std::error::Error for AotError {}

/// Compile an `AotCompilable` function through the full pipeline up to
/// `StateMachineFunction` (ready for Cranelift lowering).
///
/// Steps:
/// 1. Analyze suspension points
/// 2. Emit pre-transform SM blocks
/// 3. Apply state machine transform (adds dispatch/save/restore)
pub fn compile_to_state_machine(
    func: &dyn AotCompilable,
    function_id: u32,
) -> StateMachineFunction {
    let analysis = func.analyze();
    let blocks = func.emit_blocks();
    transform_to_state_machine(
        function_id,
        blocks,
        analysis,
        func.param_count(),
        func.local_count(),
        func.name().map(|s| s.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aot::statemachine::*;

    /// A minimal test implementation of AotCompilable.
    struct TestFunc {
        blocks: Vec<SmBlock>,
    }

    impl AotCompilable for TestFunc {
        fn analyze(&self) -> SuspensionAnalysis {
            SuspensionAnalysis::none()
        }

        fn emit_blocks(&self) -> Vec<SmBlock> {
            self.blocks.clone()
        }

        fn param_count(&self) -> u32 { 0 }
        fn local_count(&self) -> u32 { 1 }
        fn name(&self) -> Option<&str> { Some("test") }
    }

    // SmBlock and SmInstr must impl Clone for the test above
    // (they do because SmInstr derives Clone)

    #[test]
    fn test_compile_simple_function() {
        let func = TestFunc {
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

        let sm = compile_to_state_machine(&func, 0);

        assert_eq!(sm.function_id, 0);
        assert_eq!(sm.param_count, 0);
        assert_eq!(sm.local_count, 1);
        assert_eq!(sm.name.as_deref(), Some("test"));
        assert_eq!(sm.blocks.len(), 1);
        assert!(!sm.analysis.has_suspensions);
    }
}
