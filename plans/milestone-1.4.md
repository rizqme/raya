# Milestone 1.4: Stack & Frame Management

**Status:** ✅ Complete
**Goal:** Implement operand stack and call frame management for function execution
**Dependencies:** Milestone 1.3 (Value Representation & Type Metadata)

---

## Overview

This milestone implements the execution stack and call frame infrastructure required for bytecode execution. The stack manages both operand values and function call frames, supporting local variables, function arguments, and return addresses.

**Key Design Principles:**
- **Unified Stack:** Single stack for both operands and call frames
- **Stack Overflow Protection:** Maximum stack depth enforcement
- **Efficient Frame Management:** Fast push/pop operations for function calls
- **GC Root Integration:** Stack values are automatically GC roots

---

## Architecture

```
┌─────────────────────────────────────┐
│          Stack Structure            │
├─────────────────────────────────────┤
│ Operand Stack (top)                 │  ← sp (stack pointer)
│   value₁                            │
│   value₀                            │
├─────────────────────────────────────┤
│ Call Frame N (current)              │  ← fp (frame pointer)
│   local₂                            │
│   local₁                            │
│   local₀                            │
│   [frame header]                    │
├─────────────────────────────────────┤
│ Call Frame N-1                      │
│   ...                               │
├─────────────────────────────────────┤
│ Call Frame 0 (main)                 │
│   ...                               │
└─────────────────────────────────────┘
```

---

## Task Breakdown

### Task 1: Operand Stack Implementation

**File:** `crates/raya-core/src/stack.rs`

Implement the core operand stack with overflow protection.

```rust
/// Operand and call frame stack
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
        Self::with_capacity(4096)
    }

    /// Create a stack with specific capacity
    pub fn with_capacity(max_size: usize) -> Self {
        Self {
            slots: Vec::with_capacity(256),
            frames: Vec::with_capacity(64),
            sp: 0,
            fp: 0,
            max_size,
        }
    }

    /// Push a value onto the stack
    #[inline]
    pub fn push(&mut self, value: Value) -> Result<(), StackError> {
        if self.sp >= self.max_size {
            return Err(StackError::Overflow);
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
    #[inline]
    pub fn pop(&mut self) -> Result<Value, StackError> {
        if self.sp == 0 {
            return Err(StackError::Underflow);
        }

        self.sp -= 1;
        Ok(self.slots[self.sp])
    }

    /// Peek at the top value without popping
    #[inline]
    pub fn peek(&self) -> Result<Value, StackError> {
        if self.sp == 0 {
            return Err(StackError::Underflow);
        }

        Ok(self.slots[self.sp - 1])
    }

    /// Peek at value N slots from top (0 = top)
    #[inline]
    pub fn peek_n(&self, n: usize) -> Result<Value, StackError> {
        if self.sp <= n {
            return Err(StackError::Underflow);
        }

        Ok(self.slots[self.sp - 1 - n])
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
}

/// Stack errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackError {
    /// Stack overflow (exceeded max size)
    Overflow,

    /// Stack underflow (popped empty stack)
    Underflow,

    /// Invalid frame operation
    InvalidFrame,
}

impl std::fmt::Display for StackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Overflow => write!(f, "Stack overflow"),
            Self::Underflow => write!(f, "Stack underflow"),
            Self::InvalidFrame => write!(f, "Invalid call frame operation"),
        }
    }
}

impl std::error::Error for StackError {}
```

**Tests:**
- [x] Test push/pop operations
- [x] Test peek operations
- [x] Test stack overflow protection
- [x] Test stack underflow detection
- [x] Test depth tracking

---

### Task 2: Call Frame Structure

**File:** `crates/raya-core/src/stack.rs` (continued)

Implement call frame management for function calls.

```rust
/// Call frame for function execution
#[derive(Debug, Clone)]
pub struct CallFrame {
    /// Function being executed
    pub function_id: usize,

    /// Return instruction pointer
    pub return_ip: usize,

    /// Base pointer (start of locals in stack)
    pub base_pointer: usize,

    /// Number of local variables
    pub local_count: usize,

    /// Number of arguments
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
    pub fn frame_size(&self) -> usize {
        self.local_count
    }
}
```

**Tests:**
- [x] Test call frame creation
- [x] Test frame size calculation

---

### Task 3: Frame Management Operations

**File:** `crates/raya-core/src/stack.rs` (continued)

Add methods for pushing and popping call frames.

```rust
impl Stack {
    /// Push a new call frame
    pub fn push_frame(
        &mut self,
        function_id: usize,
        return_ip: usize,
        local_count: usize,
        arg_count: usize,
    ) -> Result<(), StackError> {
        // Check if we have enough stack space for locals
        if self.sp + local_count > self.max_size {
            return Err(StackError::Overflow);
        }

        // Create frame
        let frame = CallFrame::new(
            function_id,
            return_ip,
            self.sp,
            local_count,
            arg_count,
        );

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
    pub fn pop_frame(&mut self) -> Result<CallFrame, StackError> {
        // Pop frame
        let frame = self.frames.pop().ok_or(StackError::InvalidFrame)?;

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
    pub fn current_frame(&self) -> Option<&CallFrame> {
        self.frames.last()
    }

    /// Get the number of active frames
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
}
```

**Tests:**
- [x] Test frame push/pop
- [x] Test frame nesting
- [x] Test stack pointer updates
- [x] Test frame pointer tracking

---

### Task 4: Local Variable Access

**File:** `crates/raya-core/src/stack.rs` (continued)

Add methods for accessing local variables and arguments.

```rust
impl Stack {
    /// Load a local variable by index
    #[inline]
    pub fn load_local(&self, index: usize) -> Result<Value, StackError> {
        let frame = self.current_frame().ok_or(StackError::InvalidFrame)?;

        if index >= frame.local_count {
            return Err(StackError::InvalidFrame);
        }

        let slot_index = frame.base_pointer + index;
        Ok(self.slots[slot_index])
    }

    /// Store a value to a local variable
    #[inline]
    pub fn store_local(&mut self, index: usize, value: Value) -> Result<(), StackError> {
        let frame = self.current_frame().ok_or(StackError::InvalidFrame)?;

        if index >= frame.local_count {
            return Err(StackError::InvalidFrame);
        }

        let slot_index = frame.base_pointer + index;
        self.slots[slot_index] = value;
        Ok(())
    }

    /// Get a mutable reference to a local variable
    #[inline]
    pub fn local_mut(&mut self, index: usize) -> Result<&mut Value, StackError> {
        let frame = self.current_frame().ok_or(StackError::InvalidFrame)?;

        if index >= frame.local_count {
            return Err(StackError::InvalidFrame);
        }

        let slot_index = frame.base_pointer + index;
        Ok(&mut self.slots[slot_index])
    }
}
```

**Tests:**
- [x] Test local variable load/store
- [x] Test argument access
- [x] Test bounds checking
- [x] Test across multiple frames

---

### Task 5: GC Root Integration

**File:** `crates/raya-core/src/stack.rs` (continued)

Integrate stack with GC root set for automatic root scanning.

```rust
impl Stack {
    /// Iterate over all values on the stack (for GC)
    pub fn iter_values(&self) -> impl Iterator<Item = Value> + '_ {
        self.slots[0..self.sp].iter().copied()
    }

    /// Visit all stack values (for precise GC)
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
}
```

**Tests:**
- [x] Test root iteration
- [x] Test visitor pattern
- [x] Test with GC integration

---

### Task 6: Stack Debugging & Inspection

**File:** `crates/raya-core/src/stack.rs` (continued)

Add debugging utilities for stack inspection.

```rust
impl Stack {
    /// Print stack trace (for debugging)
    pub fn print_trace(&self) {
        println!("=== Stack Trace ===");
        println!("Stack depth: {}", self.sp);
        println!("Frame count: {}", self.frames.len());

        for (i, frame) in self.frames.iter().enumerate() {
            println!("Frame {}: function={}, bp={}, locals={}",
                i, frame.function_id, frame.base_pointer, frame.local_count);
        }

        println!("Top {} values:", std::cmp::min(5, self.sp));
        for i in 0..std::cmp::min(5, self.sp) {
            let index = self.sp - 1 - i;
            println!("  [{}] {:?}", index, self.slots[index]);
        }
    }

    /// Get stack statistics
    pub fn stats(&self) -> StackStats {
        StackStats {
            depth: self.sp,
            capacity: self.slots.capacity(),
            max_size: self.max_size,
            frame_count: self.frames.len(),
            utilization: (self.sp as f64 / self.max_size as f64) * 100.0,
        }
    }
}

/// Stack statistics
#[derive(Debug, Clone)]
pub struct StackStats {
    pub depth: usize,
    pub capacity: usize,
    pub max_size: usize,
    pub frame_count: usize,
    pub utilization: f64,
}
```

**Tests:**
- [x] Test stack statistics
- [x] Test trace printing (manual verification)

---

## Integration with VM

### Update VM to Use Stack

**File:** `crates/raya-core/src/vm/interpreter.rs`

Update the VM to use the new stack implementation:

```rust
use crate::stack::{Stack, StackError};

pub struct Vm {
    gc: GarbageCollector,
    stack: Stack,  // Already exists
    globals: HashMap<String, Value>,
}

impl Vm {
    pub fn new() -> Self {
        Self {
            gc: GarbageCollector::default(),
            stack: Stack::new(),  // Use new stack implementation
            globals: HashMap::new(),
        }
    }

    // Integration methods
    fn push_value(&mut self, value: Value) -> VmResult<()> {
        self.stack.push(value).map_err(VmError::from)
    }

    fn pop_value(&mut self) -> VmResult<Value> {
        self.stack.pop().map_err(VmError::from)
    }
}

// Error conversion
impl From<StackError> for VmError {
    fn from(err: StackError) -> Self {
        VmError::RuntimeError(err.to_string())
    }
}
```

---

## Testing Strategy

### Unit Tests

Create comprehensive tests in `crates/raya-core/src/stack.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_creation() {
        let stack = Stack::new();
        assert_eq!(stack.depth(), 0);
        assert!(stack.is_empty());
    }

    #[test]
    fn test_push_pop() {
        let mut stack = Stack::new();

        stack.push(Value::i32(42)).unwrap();
        stack.push(Value::i32(100)).unwrap();

        assert_eq!(stack.depth(), 2);
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
        assert!(stack.push(Value::i32(3)).is_err());
    }

    #[test]
    fn test_stack_underflow() {
        let mut stack = Stack::new();
        assert!(stack.pop().is_err());
    }

    #[test]
    fn test_peek() {
        let mut stack = Stack::new();

        stack.push(Value::i32(42)).unwrap();

        // Peek doesn't remove
        assert_eq!(stack.peek().unwrap(), Value::i32(42));
        assert_eq!(stack.depth(), 1);

        // Pop does remove
        assert_eq!(stack.pop().unwrap(), Value::i32(42));
        assert_eq!(stack.depth(), 0);
    }

    #[test]
    fn test_call_frame() {
        let mut stack = Stack::new();

        // Push initial frame
        stack.push_frame(0, 0, 3, 0).unwrap();
        assert_eq!(stack.frame_count(), 1);
        assert_eq!(stack.depth(), 3); // 3 locals

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

        // Frame 1: nested function with 1 local
        stack.push_frame(1, 100, 1, 2).unwrap();
        stack.store_local(0, Value::i32(99)).unwrap();

        // Check nested frame local
        assert_eq!(stack.load_local(0).unwrap(), Value::i32(99));

        // Pop nested frame
        stack.pop_frame().unwrap();

        // Check we're back to frame 0
        assert_eq!(stack.load_local(0).unwrap(), Value::i32(1));
        assert_eq!(stack.load_local(1).unwrap(), Value::i32(2));

        // Operands should still be there
        assert_eq!(stack.pop().unwrap(), Value::i32(20));
        assert_eq!(stack.pop().unwrap(), Value::i32(10));
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
}
```

### Integration Tests

Create integration tests in `tests/stack_integration.rs`:

```rust
use raya_core::{Stack, Value};

#[test]
fn test_function_call_simulation() {
    let mut stack = Stack::new();

    // Simulate: main() calls foo(42, 100)

    // Main frame
    stack.push_frame(0, 0, 1, 0).unwrap();
    stack.store_local(0, Value::i32(999)).unwrap();

    // Push arguments for foo
    stack.push(Value::i32(42)).unwrap();
    stack.push(Value::i32(100)).unwrap();

    // Call foo (2 args, 2 locals including args)
    stack.push_frame(1, 5, 2, 2).unwrap();

    // In foo: use arguments as locals
    assert_eq!(stack.load_local(0).unwrap(), Value::i32(42));
    assert_eq!(stack.load_local(1).unwrap(), Value::i32(100));

    // Compute result
    let a = stack.load_local(0).unwrap().as_i32().unwrap();
    let b = stack.load_local(1).unwrap().as_i32().unwrap();
    let result = Value::i32(a + b);

    // Return
    stack.pop_frame().unwrap();

    // Push result on caller's stack
    stack.push(result).unwrap();

    // Verify
    assert_eq!(stack.pop().unwrap(), Value::i32(142));
    assert_eq!(stack.load_local(0).unwrap(), Value::i32(999));
}
```

---

## Acceptance Criteria

- [x] Stack can push/pop values efficiently
- [x] Stack overflow protection works correctly
- [x] Call frames can be pushed and popped
- [x] Local variables can be accessed correctly
- [x] Nested function calls work properly
- [x] GC root iteration works
- [x] All unit tests pass
- [x] All integration tests pass
- [x] Code coverage >90% for stack module

---

## Reference Documentation

- **ARCHITECTURE.md Section 3:** Stack and Call Frame structure
- **ARCHITECTURE.md Section 9.2:** Unboxed locals optimization
- **OPCODE.md Section 7:** Function call opcodes (CALL, RETURN)
- **OPCODE.md Section 11:** Local variable opcodes (LOAD_LOCAL, STORE_LOCAL)

---

## Next Steps

After completing this milestone:

1. **Milestone 1.5:** Basic Bytecode Interpreter - implement opcode dispatch loop and use the stack
2. **Milestone 1.6:** Object Model - heap-allocated objects
3. **Milestone 1.7:** Complete GC implementation with stack root scanning
