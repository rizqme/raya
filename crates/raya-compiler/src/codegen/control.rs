//! Control Flow Helpers
//!
//! Utilities for generating control flow bytecode patterns.

use crate::ir::BasicBlockId;
use rustc_hash::FxHashMap;

/// Manages loop context for break/continue statements
pub struct LoopStack {
    /// Stack of loop contexts (break target, continue target)
    loops: Vec<LoopContext>,
}

/// Context for a single loop
struct LoopContext {
    /// Block to jump to for break
    break_target: BasicBlockId,
    /// Block to jump to for continue
    continue_target: BasicBlockId,
    /// Label for labeled breaks (optional)
    label: Option<String>,
}

impl LoopStack {
    /// Create a new empty loop stack
    pub fn new() -> Self {
        Self { loops: Vec::new() }
    }

    /// Push a new loop context
    pub fn push(&mut self, break_target: BasicBlockId, continue_target: BasicBlockId, label: Option<String>) {
        self.loops.push(LoopContext {
            break_target,
            continue_target,
            label,
        });
    }

    /// Pop the current loop context
    pub fn pop(&mut self) {
        self.loops.pop();
    }

    /// Get the break target for the current loop
    pub fn break_target(&self) -> Option<BasicBlockId> {
        self.loops.last().map(|ctx| ctx.break_target)
    }

    /// Get the continue target for the current loop
    pub fn continue_target(&self) -> Option<BasicBlockId> {
        self.loops.last().map(|ctx| ctx.continue_target)
    }

    /// Get the break target for a labeled loop
    pub fn labeled_break_target(&self, label: &str) -> Option<BasicBlockId> {
        self.loops
            .iter()
            .rev()
            .find(|ctx| ctx.label.as_deref() == Some(label))
            .map(|ctx| ctx.break_target)
    }

    /// Get the continue target for a labeled loop
    pub fn labeled_continue_target(&self, label: &str) -> Option<BasicBlockId> {
        self.loops
            .iter()
            .rev()
            .find(|ctx| ctx.label.as_deref() == Some(label))
            .map(|ctx| ctx.continue_target)
    }

    /// Check if we're inside a loop
    pub fn is_in_loop(&self) -> bool {
        !self.loops.is_empty()
    }
}

impl Default for LoopStack {
    fn default() -> Self {
        Self::new()
    }
}

/// Block ordering for code generation
///
/// Determines the order in which basic blocks should be emitted to minimize
/// the number of jumps needed.
pub struct BlockOrdering {
    /// Ordered list of block IDs
    order: Vec<BasicBlockId>,
    /// Map from block ID to position in order
    positions: FxHashMap<BasicBlockId, usize>,
}

impl BlockOrdering {
    /// Create a new block ordering
    pub fn new(blocks: impl IntoIterator<Item = BasicBlockId>) -> Self {
        let order: Vec<_> = blocks.into_iter().collect();
        let positions: FxHashMap<_, _> = order
            .iter()
            .enumerate()
            .map(|(i, &id)| (id, i))
            .collect();
        Self { order, positions }
    }

    /// Check if a block comes immediately after another
    pub fn is_fallthrough(&self, from: BasicBlockId, to: BasicBlockId) -> bool {
        if let (Some(&from_pos), Some(&to_pos)) = (self.positions.get(&from), self.positions.get(&to)) {
            to_pos == from_pos + 1
        } else {
            false
        }
    }

    /// Get the position of a block
    pub fn position(&self, block: BasicBlockId) -> Option<usize> {
        self.positions.get(&block).copied()
    }

    /// Iterate over blocks in order
    pub fn iter(&self) -> impl Iterator<Item = BasicBlockId> + '_ {
        self.order.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loop_stack() {
        let mut stack = LoopStack::new();
        assert!(!stack.is_in_loop());

        stack.push(BasicBlockId(10), BasicBlockId(5), None);
        assert!(stack.is_in_loop());
        assert_eq!(stack.break_target(), Some(BasicBlockId(10)));
        assert_eq!(stack.continue_target(), Some(BasicBlockId(5)));

        stack.push(BasicBlockId(20), BasicBlockId(15), Some("inner".to_string()));
        assert_eq!(stack.break_target(), Some(BasicBlockId(20)));
        assert_eq!(stack.labeled_break_target("inner"), Some(BasicBlockId(20)));

        stack.pop();
        assert_eq!(stack.break_target(), Some(BasicBlockId(10)));

        stack.pop();
        assert!(!stack.is_in_loop());
    }

    #[test]
    fn test_block_ordering() {
        let blocks = vec![BasicBlockId(0), BasicBlockId(1), BasicBlockId(2)];
        let ordering = BlockOrdering::new(blocks);

        assert!(ordering.is_fallthrough(BasicBlockId(0), BasicBlockId(1)));
        assert!(ordering.is_fallthrough(BasicBlockId(1), BasicBlockId(2)));
        assert!(!ordering.is_fallthrough(BasicBlockId(0), BasicBlockId(2)));
        assert!(!ordering.is_fallthrough(BasicBlockId(2), BasicBlockId(0)));
    }
}
