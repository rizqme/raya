//! Suspension point analysis
//!
//! Walks IR or JIT IR and classifies each instruction by its suspension behavior:
//! - **Always suspends**: Await, Yield, Sleep
//! - **May suspend**: Call (to another AOT function), NativeCall (fs/net/etc.)
//! - **Never suspends**: Arithmetic, field access, local load/store, control flow
//! - **Preemption point**: Loop back-edges (synthetic suspension point)
//!
//! The output is used by the state machine transform to determine where to
//! insert save/restore points and resume dispatch.

use std::collections::HashSet;

pub use crate::vm::suspend::ExecutionSuspendKind;

/// A suspension point within a function.
#[derive(Debug, Clone)]
pub struct SuspensionPoint {
    /// Unique index within this function (0 = entry, 1..N = continuations).
    pub index: u32,

    /// The basic block containing this suspension point.
    pub block_id: u32,

    /// Instruction index within the basic block.
    pub instr_index: u32,

    /// What kind of suspension this is.
    pub kind: ExecutionSuspendKind,

    /// Set of local variable indices that are live across this suspension point.
    /// These must be saved to the frame before suspending and restored on resume.
    pub live_locals: HashSet<u32>,
}

/// Result of analyzing a function for suspension points.
#[derive(Debug, Clone)]
pub struct SuspensionAnalysis {
    /// All suspension points, ordered by their index.
    pub points: Vec<SuspensionPoint>,

    /// Whether this function has any suspension points at all.
    /// If false, the function can be called directly without state machine overhead.
    pub has_suspensions: bool,

    /// Set of basic block IDs that are loop headers (have back-edges).
    pub loop_headers: HashSet<u32>,
}

impl SuspensionAnalysis {
    /// Create an analysis result with no suspension points.
    pub fn none() -> Self {
        Self {
            points: Vec::new(),
            has_suspensions: false,
            loop_headers: HashSet::new(),
        }
    }

    /// Get the total number of resume states (entry + suspension points).
    pub fn state_count(&self) -> u32 {
        self.points.len() as u32 + 1 // +1 for the entry state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_suspension_analysis_none() {
        let analysis = SuspensionAnalysis::none();
        assert!(!analysis.has_suspensions);
        assert_eq!(analysis.state_count(), 1); // just entry
    }

    #[test]
    fn test_suspension_kind_properties() {
        assert!(ExecutionSuspendKind::AwaitTask.always_suspends());
        assert!(ExecutionSuspendKind::YieldNow.always_suspends());
        assert!(!ExecutionSuspendKind::AotCall.always_suspends());
        assert!(!ExecutionSuspendKind::KernelBoundary.always_suspends());
        assert!(!ExecutionSuspendKind::Preemption.always_suspends());

        assert!(ExecutionSuspendKind::AotCall.has_child_frame());
        assert!(!ExecutionSuspendKind::AwaitTask.has_child_frame());
    }
}
