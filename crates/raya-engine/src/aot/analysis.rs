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
    pub kind: SuspensionKind,

    /// Set of local variable indices that are live across this suspension point.
    /// These must be saved to the frame before suspending and restored on resume.
    pub live_locals: HashSet<u32>,
}

/// Classification of suspension points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuspensionKind {
    /// `await expr` — suspends until a spawned task completes
    Await,

    /// `yield` — voluntarily yields to the scheduler
    Yield,

    /// `sleep(ms)` — suspends for a duration
    Sleep,

    /// Call to another AOT function that may itself suspend.
    /// The callee's frame is linked as a child.
    AotCall,

    /// Native function call that may return Suspend (blocking I/O).
    NativeCall,

    /// Preemption check at a loop back-edge.
    PreemptionCheck,

    /// Channel receive that may block.
    ChannelRecv,

    /// Channel send that may block (backpressure).
    ChannelSend,

    /// Mutex lock that may block.
    MutexLock,
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

impl SuspensionKind {
    /// Whether this kind of suspension always suspends (vs. may suspend).
    pub fn always_suspends(&self) -> bool {
        matches!(self, SuspensionKind::Await | SuspensionKind::Yield | SuspensionKind::Sleep)
    }

    /// Whether this suspension involves a child frame (callee that suspended).
    pub fn has_child_frame(&self) -> bool {
        matches!(self, SuspensionKind::AotCall)
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
        assert!(SuspensionKind::Await.always_suspends());
        assert!(SuspensionKind::Yield.always_suspends());
        assert!(!SuspensionKind::AotCall.always_suspends());
        assert!(!SuspensionKind::NativeCall.always_suspends());
        assert!(!SuspensionKind::PreemptionCheck.always_suspends());

        assert!(SuspensionKind::AotCall.has_child_frame());
        assert!(!SuspensionKind::Await.has_child_frame());
    }
}
