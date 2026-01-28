//! Control Flow Lowering Utilities
//!
//! Helper structures and methods for managing control flow during lowering.

use crate::compiler::ir::BasicBlockId;

/// Context for managing loop control flow
#[derive(Debug, Clone)]
pub struct LoopContext {
    /// Block to jump to for 'break'
    pub break_block: BasicBlockId,
    /// Block to jump to for 'continue'
    pub continue_block: BasicBlockId,
    /// Optional loop label
    pub label: Option<String>,
}

impl LoopContext {
    /// Create a new loop context
    pub fn new(break_block: BasicBlockId, continue_block: BasicBlockId) -> Self {
        Self {
            break_block,
            continue_block,
            label: None,
        }
    }

    /// Create a new labeled loop context
    pub fn labeled(
        break_block: BasicBlockId,
        continue_block: BasicBlockId,
        label: impl Into<String>,
    ) -> Self {
        Self {
            break_block,
            continue_block,
            label: Some(label.into()),
        }
    }
}

/// Stack of active loop contexts for nested loops
#[derive(Debug, Default)]
pub struct LoopStack {
    stack: Vec<LoopContext>,
}

impl LoopStack {
    /// Create a new empty loop stack
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a new loop context
    pub fn push(&mut self, ctx: LoopContext) {
        self.stack.push(ctx);
    }

    /// Pop the current loop context
    pub fn pop(&mut self) -> Option<LoopContext> {
        self.stack.pop()
    }

    /// Get the current (innermost) loop context
    pub fn current(&self) -> Option<&LoopContext> {
        self.stack.last()
    }

    /// Find a loop by label
    pub fn find_by_label(&self, label: &str) -> Option<&LoopContext> {
        self.stack
            .iter()
            .rev()
            .find(|ctx| ctx.label.as_deref() == Some(label))
    }

    /// Check if we're inside any loop
    pub fn is_in_loop(&self) -> bool {
        !self.stack.is_empty()
    }

    /// Get the break target for the current or labeled loop
    pub fn break_target(&self, label: Option<&str>) -> Option<BasicBlockId> {
        match label {
            Some(l) => self.find_by_label(l).map(|ctx| ctx.break_block),
            None => self.current().map(|ctx| ctx.break_block),
        }
    }

    /// Get the continue target for the current or labeled loop
    pub fn continue_target(&self, label: Option<&str>) -> Option<BasicBlockId> {
        match label {
            Some(l) => self.find_by_label(l).map(|ctx| ctx.continue_block),
            None => self.current().map(|ctx| ctx.continue_block),
        }
    }
}

/// Context for managing try/catch control flow
#[derive(Debug, Clone)]
pub struct TryContext {
    /// Block to jump to for exception handling
    pub catch_block: Option<BasicBlockId>,
    /// Block for finally (always executed)
    pub finally_block: Option<BasicBlockId>,
    /// Block to continue after try/catch/finally
    pub exit_block: BasicBlockId,
}

/// Context for managing switch/match control flow
#[derive(Debug, Clone)]
pub struct SwitchContext {
    /// Block to jump to for 'break' in switch
    pub exit_block: BasicBlockId,
    /// Current case block for fall-through
    pub current_case: Option<BasicBlockId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loop_stack() {
        let mut stack = LoopStack::new();
        assert!(!stack.is_in_loop());

        stack.push(LoopContext::new(BasicBlockId(1), BasicBlockId(2)));
        assert!(stack.is_in_loop());
        assert_eq!(stack.break_target(None), Some(BasicBlockId(1)));
        assert_eq!(stack.continue_target(None), Some(BasicBlockId(2)));

        stack.push(LoopContext::labeled(BasicBlockId(3), BasicBlockId(4), "outer"));
        assert_eq!(stack.break_target(Some("outer")), Some(BasicBlockId(3)));

        stack.pop();
        stack.pop();
        assert!(!stack.is_in_loop());
    }
}
