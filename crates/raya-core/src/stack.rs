//! Stack and call frame management
//!
//! This module provides the operand stack and call frame infrastructure for
//! function execution in the Raya VM.
//!
//! # Architecture
//!
//! The stack is a unified structure that holds both:
//! - Operand values (temporary computation results)
//! - Call frames (function activation records with locals)
//!
//! # Memory Layout
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │ Operand Stack (top)                 │  ← sp (stack pointer)
//! │   value₁                            │
//! │   value₀                            │
//! ├─────────────────────────────────────┤
//! │ Call Frame N (current)              │  ← fp (frame pointer)
//! │   local₂                            │
//! │   local₁                            │
//! │   local₀                            │
//! ├─────────────────────────────────────┤
//! │ Call Frame N-1                      │
//! │   ...                               │
//! └─────────────────────────────────────┘
//! ```

use crate::{value::Value, VmError, VmResult};

/// Default maximum stack size (in slots)
const DEFAULT_MAX_STACK_SIZE: usize = 1024 * 64;

/// Call frame for function invocation
///
/// Each call frame represents one function activation with its own
/// local variables, arguments, and return address.
#[derive(Debug, Clone)]
pub struct CallFrame {
    /// Function ID being executed
    pub function_id: usize,

    /// Return instruction pointer
    pub return_ip: usize,

    /// Base pointer (start of locals in stack)
    pub base_pointer: usize,

    /// Number of local variables
    pub local_count: usize,

    /// Number of arguments passed to this function
    pub arg_count: usize,
}

impl CallFrame {
    /// Create a new call frame
    pub fn new(
        function_id: usize,
        return_ip: usize,
        base_pointer: usize,
        local_count: usize,
        arg_count: usize,
    ) -> Self {
        Self {
            function_id,
            return_ip,
            base_pointer,
            local_count,
            arg_count,
        }
    }

    /// Get the total frame size (locals + args)
    #[inline]
    pub fn frame_size(&self) -> usize {
        self.local_count
    }

    /// Get the starting index of locals in the stack
    #[inline]
    pub fn locals_start(&self) -> usize {
        self.base_pointer
    }

    /// Get the number of local variables
    #[inline]
    pub fn locals_count(&self) -> usize {
        self.local_count
    }
}

/// Operand and call frame stack for the VM
///
/// This structure manages both operand values and function call frames.
/// It provides overflow protection, local variable access, and GC root integration.
pub struct Stack {
    /// Stack slots (operands + locals)
    slots: Vec<Value>,

    /// Call frames
    frames: Vec<CallFrame>,

    /// Stack pointer (points to next free slot)
    sp: usize,

    /// Frame pointer (points to current frame base)
    fp: usize,

    /// Maximum stack size (in slots)
    max_size: usize,
}

impl Stack {
    /// Create a new stack with default size
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_MAX_STACK_SIZE)
    }

    /// Create a stack with specific capacity
    pub fn with_capacity(max_size: usize) -> Self {
        Self {
            slots: Vec::with_capacity(1024),
            frames: Vec::with_capacity(256),
            sp: 0,
            fp: 0,
            max_size,
        }
    }

    // ========================================================================
    // Operand Stack Operations
    // ========================================================================

    /// Push a value onto the stack
    ///
    /// # Errors
    ///
    /// Returns `VmError::StackOverflow` if the stack is full.
    #[inline]
    pub fn push(&mut self, value: Value) -> VmResult<()> {
        if self.sp >= self.max_size {
            return Err(VmError::StackOverflow);
        }

        if self.sp >= self.slots.len() {
            self.slots.push(value);
        } else {
            self.slots[self.sp] = value;
        }

        self.sp += 1;
        Ok(())
    }

    /// Pop a value from the stack
    ///
    /// # Errors
    ///
    /// Returns `VmError::StackUnderflow` if the stack is empty.
    #[inline]
    pub fn pop(&mut self) -> VmResult<Value> {
        if self.sp == 0 {
            return Err(VmError::StackUnderflow);
        }

        self.sp -= 1;
        Ok(self.slots[self.sp])
    }

    /// Peek at the top value without popping
    ///
    /// # Errors
    ///
    /// Returns `VmError::StackUnderflow` if the stack is empty.
    #[inline]
    pub fn peek(&self) -> VmResult<Value> {
        if self.sp == 0 {
            return Err(VmError::StackUnderflow);
        }

        Ok(self.slots[self.sp - 1])
    }

    /// Peek at value N slots from top (0 = top)
    ///
    /// # Errors
    ///
    /// Returns `VmError::StackUnderflow` if not enough values on stack.
    #[inline]
    pub fn peek_n(&self, n: usize) -> VmResult<Value> {
        if self.sp <= n {
            return Err(VmError::StackUnderflow);
        }

        Ok(self.slots[self.sp - 1 - n])
    }

    /// Peek at value at absolute stack position
    ///
    /// # Errors
    ///
    /// Returns `VmError::StackUnderflow` if position is out of bounds.
    #[inline]
    pub fn peek_at(&self, pos: usize) -> VmResult<Value> {
        if pos >= self.sp {
            return Err(VmError::StackUnderflow);
        }

        Ok(self.slots[pos])
    }

    /// Set value at absolute stack position
    ///
    /// # Errors
    ///
    /// Returns `VmError::StackUnderflow` if position is out of bounds.
    #[inline]
    pub fn set_at(&mut self, pos: usize, value: Value) -> VmResult<()> {
        if pos >= self.sp {
            return Err(VmError::StackUnderflow);
        }

        self.slots[pos] = value;
        Ok(())
    }

    /// Get current stack depth
    #[inline]
    pub fn depth(&self) -> usize {
        self.sp
    }

    /// Check if stack is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.sp == 0
    }

    /// Get maximum stack size
    #[inline]
    pub fn max_size(&self) -> usize {
        self.max_size
    }

    // ========================================================================
    // Call Frame Management
    // ========================================================================

    /// Push a new call frame
    ///
    /// This allocates space for local variables on the stack and creates
    /// a new activation record.
    ///
    /// # Errors
    ///
    /// Returns `VmError::StackOverflow` if not enough stack space for locals.
    pub fn push_frame(
        &mut self,
        function_id: usize,
        return_ip: usize,
        local_count: usize,
        arg_count: usize,
    ) -> VmResult<()> {
        // Check if we have enough stack space for locals
        if self.sp + local_count > self.max_size {
            return Err(VmError::StackOverflow);
        }

        // Create frame
        let frame = CallFrame::new(function_id, return_ip, self.sp, local_count, arg_count);

        // Push frame
        self.frames.push(frame);

        // Update frame pointer
        self.fp = self.sp;

        // Allocate space for locals (initialize to null)
        for _ in 0..local_count {
            self.push(Value::null())?;
        }

        Ok(())
    }

    /// Pop the current call frame
    ///
    /// This deallocates the frame's local variables and restores the
    /// previous frame pointer.
    ///
    /// # Errors
    ///
    /// Returns `VmError::RuntimeError` if no frames to pop.
    pub fn pop_frame(&mut self) -> VmResult<CallFrame> {
        // Pop frame
        let frame = self
            .frames
            .pop()
            .ok_or_else(|| VmError::RuntimeError("No call frame to pop".to_string()))?;

        // Reset stack pointer to frame base
        self.sp = frame.base_pointer;

        // Update frame pointer to previous frame (if any)
        if let Some(prev_frame) = self.frames.last() {
            self.fp = prev_frame.base_pointer;
        } else {
            self.fp = 0;
        }

        Ok(frame)
    }

    /// Get the current call frame
    #[inline]
    pub fn current_frame(&self) -> Option<&CallFrame> {
        self.frames.last()
    }

    /// Get mutable reference to current call frame
    #[inline]
    pub fn current_frame_mut(&mut self) -> Option<&mut CallFrame> {
        self.frames.last_mut()
    }

    /// Get the number of active frames
    #[inline]
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    // ========================================================================
    // Local Variable Access
    // ========================================================================

    /// Load a local variable by index
    ///
    /// # Errors
    ///
    /// Returns error if no active frame or index out of bounds.
    #[inline]
    pub fn load_local(&self, index: usize) -> VmResult<Value> {
        let frame = self
            .current_frame()
            .ok_or_else(|| VmError::RuntimeError("No active call frame".to_string()))?;

        if index >= frame.local_count {
            return Err(VmError::RuntimeError(format!(
                "Local index {} out of bounds (max {})",
                index, frame.local_count
            )));
        }

        let slot_index = frame.base_pointer + index;
        Ok(self.slots[slot_index])
    }

    /// Store a value to a local variable
    ///
    /// # Errors
    ///
    /// Returns error if no active frame or index out of bounds.
    #[inline]
    pub fn store_local(&mut self, index: usize, value: Value) -> VmResult<()> {
        let frame = self
            .current_frame()
            .ok_or_else(|| VmError::RuntimeError("No active call frame".to_string()))?;

        if index >= frame.local_count {
            return Err(VmError::RuntimeError(format!(
                "Local index {} out of bounds (max {})",
                index, frame.local_count
            )));
        }

        let slot_index = frame.base_pointer + index;
        self.slots[slot_index] = value;
        Ok(())
    }

    /// Get a mutable reference to a local variable
    ///
    /// # Errors
    ///
    /// Returns error if no active frame or index out of bounds.
    #[inline]
    pub fn local_mut(&mut self, index: usize) -> VmResult<&mut Value> {
        let frame = self
            .current_frame()
            .ok_or_else(|| VmError::RuntimeError("No active call frame".to_string()))?;

        if index >= frame.local_count {
            return Err(VmError::RuntimeError(format!(
                "Local index {} out of bounds (max {})",
                index, frame.local_count
            )));
        }

        let slot_index = frame.base_pointer + index;
        Ok(&mut self.slots[slot_index])
    }

    // ========================================================================
    // GC Root Integration
    // ========================================================================

    /// Iterate over all values on the stack (for GC)
    ///
    /// This returns all live stack values that should be treated as GC roots.
    pub fn iter_values(&self) -> impl Iterator<Item = Value> + '_ {
        self.slots[0..self.sp].iter().copied()
    }

    /// Visit all stack values (for precise GC)
    ///
    /// The visitor function is called for each value on the stack.
    pub fn visit_roots<F>(&self, mut visitor: F)
    where
        F: FnMut(Value),
    {
        for i in 0..self.sp {
            visitor(self.slots[i]);
        }
    }

    /// Get all stack values as a slice
    pub fn as_slice(&self) -> &[Value] {
        &self.slots[0..self.sp]
    }

    /// Iterate over all call frames
    pub fn frames(&self) -> FrameIterator<'_> {
        FrameIterator {
            stack: self,
            frame_idx: 0,
        }
    }

    // ========================================================================
    // Debugging & Inspection
    // ========================================================================

    /// Print stack trace (for debugging)
    pub fn print_trace(&self) {
        println!("=== Stack Trace ===");
        println!("Stack depth: {}", self.sp);
        println!("Frame count: {}", self.frames.len());
        println!("Max size: {}", self.max_size);

        for (i, frame) in self.frames.iter().enumerate() {
            println!(
                "Frame {}: function={}, return_ip={}, bp={}, locals={}, args={}",
                i,
                frame.function_id,
                frame.return_ip,
                frame.base_pointer,
                frame.local_count,
                frame.arg_count
            );
        }

        let display_count = std::cmp::min(10, self.sp);
        if display_count > 0 {
            println!("\nTop {} values:", display_count);
            for i in 0..display_count {
                let index = self.sp - 1 - i;
                println!("  [{}] {:?}", index, self.slots[index]);
            }
        }

        println!("==================");
    }

    /// Get stack statistics
    pub fn stats(&self) -> StackStats {
        StackStats {
            depth: self.sp,
            capacity: self.slots.capacity(),
            max_size: self.max_size,
            frame_count: self.frames.len(),
            utilization: if self.max_size > 0 {
                (self.sp as f64 / self.max_size as f64) * 100.0
            } else {
                0.0
            },
        }
    }
}

/// Iterator over call frames
pub struct FrameIterator<'a> {
    stack: &'a Stack,
    frame_idx: usize,
}

impl<'a> Iterator for FrameIterator<'a> {
    type Item = &'a CallFrame;

    fn next(&mut self) -> Option<Self::Item> {
        if self.frame_idx < self.stack.frames.len() {
            let frame = &self.stack.frames[self.frame_idx];
            self.frame_idx += 1;
            Some(frame)
        } else {
            None
        }
    }
}

impl Default for Stack {
    fn default() -> Self {
        Self::new()
    }
}

/// Stack statistics
#[derive(Debug, Clone)]
pub struct StackStats {
    /// Current stack depth
    pub depth: usize,

    /// Allocated capacity
    pub capacity: usize,

    /// Maximum allowed size
    pub max_size: usize,

    /// Number of active call frames
    pub frame_count: usize,

    /// Stack utilization percentage (0-100)
    pub utilization: f64,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_creation() {
        let stack = Stack::new();
        assert_eq!(stack.depth(), 0);
        assert!(stack.is_empty());
        assert_eq!(stack.frame_count(), 0);
    }

    #[test]
    fn test_push_pop() {
        let mut stack = Stack::new();

        stack.push(Value::i32(42)).unwrap();
        stack.push(Value::i32(100)).unwrap();

        assert_eq!(stack.depth(), 2);
        assert!(!stack.is_empty());

        assert_eq!(stack.pop().unwrap(), Value::i32(100));
        assert_eq!(stack.pop().unwrap(), Value::i32(42));
        assert!(stack.is_empty());
    }

    #[test]
    fn test_stack_overflow() {
        let mut stack = Stack::with_capacity(2);

        stack.push(Value::i32(1)).unwrap();
        stack.push(Value::i32(2)).unwrap();

        // This should fail
        let result = stack.push(Value::i32(3));
        assert!(result.is_err());
        assert!(matches!(result, Err(VmError::StackOverflow)));
    }

    #[test]
    fn test_stack_underflow() {
        let mut stack = Stack::new();

        let result = stack.pop();
        assert!(result.is_err());
        assert!(matches!(result, Err(VmError::StackUnderflow)));
    }

    #[test]
    fn test_peek() {
        let mut stack = Stack::new();

        stack.push(Value::i32(42)).unwrap();
        stack.push(Value::bool(true)).unwrap();

        // Peek doesn't remove
        assert_eq!(stack.peek().unwrap(), Value::bool(true));
        assert_eq!(stack.depth(), 2);

        // Pop does remove
        assert_eq!(stack.pop().unwrap(), Value::bool(true));
        assert_eq!(stack.depth(), 1);
        assert_eq!(stack.peek().unwrap(), Value::i32(42));
    }

    #[test]
    fn test_peek_n() {
        let mut stack = Stack::new();

        stack.push(Value::i32(10)).unwrap();
        stack.push(Value::i32(20)).unwrap();
        stack.push(Value::i32(30)).unwrap();

        assert_eq!(stack.peek_n(0).unwrap(), Value::i32(30)); // top
        assert_eq!(stack.peek_n(1).unwrap(), Value::i32(20));
        assert_eq!(stack.peek_n(2).unwrap(), Value::i32(10));

        // Out of bounds
        assert!(stack.peek_n(3).is_err());
    }

    #[test]
    fn test_call_frame() {
        let mut stack = Stack::new();

        // Push initial frame (function 0, 3 locals, 0 args)
        stack.push_frame(0, 0, 3, 0).unwrap();
        assert_eq!(stack.frame_count(), 1);
        assert_eq!(stack.depth(), 3); // 3 locals allocated

        // Set local variables
        stack.store_local(0, Value::i32(10)).unwrap();
        stack.store_local(1, Value::i32(20)).unwrap();
        stack.store_local(2, Value::i32(30)).unwrap();

        // Read them back
        assert_eq!(stack.load_local(0).unwrap(), Value::i32(10));
        assert_eq!(stack.load_local(1).unwrap(), Value::i32(20));
        assert_eq!(stack.load_local(2).unwrap(), Value::i32(30));

        // Pop frame
        let frame = stack.pop_frame().unwrap();
        assert_eq!(frame.local_count, 3);
        assert_eq!(frame.function_id, 0);
        assert_eq!(stack.frame_count(), 0);
        assert_eq!(stack.depth(), 0);
    }

    #[test]
    fn test_nested_frames() {
        let mut stack = Stack::new();

        // Frame 0: main with 2 locals
        stack.push_frame(0, 0, 2, 0).unwrap();
        stack.store_local(0, Value::i32(1)).unwrap();
        stack.store_local(1, Value::i32(2)).unwrap();

        // Push some operands
        stack.push(Value::i32(10)).unwrap();
        stack.push(Value::i32(20)).unwrap();

        assert_eq!(stack.depth(), 4); // 2 locals + 2 operands

        // Frame 1: nested function with 1 local, 2 args
        stack.push_frame(1, 100, 1, 2).unwrap();
        stack.store_local(0, Value::i32(99)).unwrap();

        assert_eq!(stack.frame_count(), 2);
        assert_eq!(stack.depth(), 5); // prev depth + 1 local

        // Check nested frame local
        assert_eq!(stack.load_local(0).unwrap(), Value::i32(99));

        // Pop nested frame
        let frame1 = stack.pop_frame().unwrap();
        assert_eq!(frame1.function_id, 1);
        assert_eq!(frame1.return_ip, 100);

        // Should be back to depth 4
        assert_eq!(stack.depth(), 4);
        assert_eq!(stack.frame_count(), 1);

        // Check we're back to frame 0 locals
        assert_eq!(stack.load_local(0).unwrap(), Value::i32(1));
        assert_eq!(stack.load_local(1).unwrap(), Value::i32(2));

        // Operands should still be there
        assert_eq!(stack.pop().unwrap(), Value::i32(20));
        assert_eq!(stack.pop().unwrap(), Value::i32(10));

        // Pop main frame
        let frame0 = stack.pop_frame().unwrap();
        assert_eq!(frame0.function_id, 0);
        assert_eq!(stack.frame_count(), 0);
        assert_eq!(stack.depth(), 0);
    }

    #[test]
    fn test_local_bounds_checking() {
        let mut stack = Stack::new();

        stack.push_frame(0, 0, 3, 0).unwrap();

        // Valid indices
        assert!(stack.store_local(0, Value::i32(1)).is_ok());
        assert!(stack.store_local(1, Value::i32(2)).is_ok());
        assert!(stack.store_local(2, Value::i32(3)).is_ok());

        // Invalid index
        assert!(stack.store_local(3, Value::i32(4)).is_err());
        assert!(stack.load_local(3).is_err());
    }

    #[test]
    fn test_gc_root_iteration() {
        let mut stack = Stack::new();

        stack.push(Value::i32(1)).unwrap();
        stack.push(Value::i32(2)).unwrap();
        stack.push(Value::i32(3)).unwrap();

        let values: Vec<Value> = stack.iter_values().collect();
        assert_eq!(values.len(), 3);
        assert_eq!(values[0], Value::i32(1));
        assert_eq!(values[1], Value::i32(2));
        assert_eq!(values[2], Value::i32(3));
    }

    #[test]
    fn test_visit_roots() {
        let mut stack = Stack::new();

        stack.push(Value::i32(10)).unwrap();
        stack.push(Value::i32(20)).unwrap();
        stack.push(Value::i32(30)).unwrap();

        let mut visited = Vec::new();
        stack.visit_roots(|value| {
            visited.push(value);
        });

        assert_eq!(visited.len(), 3);
        assert_eq!(visited[0], Value::i32(10));
        assert_eq!(visited[1], Value::i32(20));
        assert_eq!(visited[2], Value::i32(30));
    }

    #[test]
    fn test_stack_stats() {
        let mut stack = Stack::with_capacity(100);

        stack.push(Value::i32(1)).unwrap();
        stack.push(Value::i32(2)).unwrap();

        let stats = stack.stats();
        assert_eq!(stats.depth, 2);
        assert_eq!(stats.max_size, 100);
        assert_eq!(stats.frame_count, 0);
        assert_eq!(stats.utilization, 2.0);
    }

    #[test]
    fn test_function_call_simulation() {
        let mut stack = Stack::new();

        // Simulate: main() calls foo(42, 100)

        // Main frame with 1 local
        stack.push_frame(0, 0, 1, 0).unwrap();
        stack.store_local(0, Value::i32(999)).unwrap();

        // Push arguments for foo
        stack.push(Value::i32(42)).unwrap();
        stack.push(Value::i32(100)).unwrap();

        // Call foo (2 locals for args)
        stack.push_frame(1, 5, 2, 2).unwrap();

        // Arguments should be accessible as locals
        // (in real implementation, we'd copy from stack to locals)
        // For now, just set them manually
        stack.store_local(0, Value::i32(42)).unwrap();
        stack.store_local(1, Value::i32(100)).unwrap();

        // Compute result in foo
        let a = stack.load_local(0).unwrap().as_i32().unwrap();
        let b = stack.load_local(1).unwrap().as_i32().unwrap();
        let result = Value::i32(a + b);

        // Return from foo
        let frame = stack.pop_frame().unwrap();
        assert_eq!(frame.return_ip, 5);

        // Pop the argument operands that were left on stack
        stack.pop().unwrap(); // 100
        stack.pop().unwrap(); // 42

        // Push result on caller's stack
        stack.push(result).unwrap();

        // Verify we're back in main
        assert_eq!(stack.frame_count(), 1);
        assert_eq!(stack.load_local(0).unwrap(), Value::i32(999));
        assert_eq!(stack.pop().unwrap(), Value::i32(142));
    }

    #[test]
    fn test_frame_with_args() {
        let frame = CallFrame::new(5, 100, 10, 3, 2);

        assert_eq!(frame.function_id, 5);
        assert_eq!(frame.return_ip, 100);
        assert_eq!(frame.base_pointer, 10);
        assert_eq!(frame.local_count, 3);
        assert_eq!(frame.arg_count, 2);
        assert_eq!(frame.frame_size(), 3);
    }
}
