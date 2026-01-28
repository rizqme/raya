//! GC root tracking
//!
//! This module manages the root set for garbage collection.
//! Roots are starting points for GC traversal and include:
//! - Stack values
//! - Global variables
//! - Task-local storage

use crate::vm::value::Value;

/// Root set for garbage collection
///
/// The root set contains all values that are directly accessible
/// and should not be collected, even if no other objects reference them.
pub struct RootSet {
    /// Stack roots (values on VM stacks)
    stack_roots: Vec<Value>,

    /// Global roots (global variables)
    global_roots: Vec<Value>,
}

impl RootSet {
    /// Create a new root set
    pub fn new() -> Self {
        Self {
            stack_roots: Vec::new(),
            global_roots: Vec::new(),
        }
    }

    /// Add a stack root
    pub fn add_stack_root(&mut self, value: Value) {
        if value.is_heap_allocated() {
            self.stack_roots.push(value);
        }
    }

    /// Add a global root
    pub fn add_global_root(&mut self, value: Value) {
        if value.is_heap_allocated() {
            self.global_roots.push(value);
        }
    }

    /// Clear all stack roots
    pub fn clear_stack_roots(&mut self) {
        self.stack_roots.clear();
    }

    /// Iterate over all roots
    pub fn iter(&self) -> impl Iterator<Item = Value> + '_ {
        self.stack_roots
            .iter()
            .chain(self.global_roots.iter())
            .copied()
    }

    /// Get total number of roots
    pub fn len(&self) -> usize {
        self.stack_roots.len() + self.global_roots.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for RootSet {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_root_set_creation() {
        let roots = RootSet::new();
        assert_eq!(roots.len(), 0);
        assert!(roots.is_empty());
    }

    #[test]
    fn test_root_set_add() {
        let mut roots = RootSet::new();

        // Non-heap values should be ignored
        roots.add_stack_root(Value::i32(42));
        roots.add_stack_root(Value::bool(true));
        roots.add_stack_root(Value::null());

        assert_eq!(roots.len(), 0);
    }

    #[test]
    fn test_root_set_clear() {
        let mut roots = RootSet::new();

        // Add some values (they won't actually be added since they're not heap-allocated)
        roots.add_stack_root(Value::i32(1));
        roots.add_stack_root(Value::i32(2));

        roots.clear_stack_roots();
        assert_eq!(roots.len(), 0);
    }
}
