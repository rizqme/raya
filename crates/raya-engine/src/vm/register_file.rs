//! Register file for register-based interpreter
//!
//! Replaces the operand stack for value storage. Each function frame
//! occupies a window of registers in a contiguous array.
//!
//! # Memory Layout
//!
//! ```text
//! ┌────────────────────────────────────────┐
//! │ Frame 2 registers (current)            │  ← top
//! │   r0 (param 0)                         │
//! │   r1 (param 1)                         │
//! │   r2 (local)                           │
//! │   r3 (temp)                            │
//! ├────────────────────────────────────────┤
//! │ Frame 1 registers                      │  ← reg_base for frame 2
//! │   r0..rN                               │
//! ├────────────────────────────────────────┤
//! │ Frame 0 registers (entry function)     │  ← reg_base for frame 1
//! │   r0..rM                               │
//! └────────────────────────────────────────┘  ← reg_base=0 for frame 0
//! ```

use crate::vm::{value::Value, VmError, VmResult};

/// Default maximum register file size (in slots)
const DEFAULT_MAX_SIZE: usize = 1024 * 64; // 65536 registers

/// Register file for a task — contiguous array of Values
///
/// Frames are windows into this array. Each frame has a `reg_base`
/// and `reg_count` defining its register window.
#[derive(Debug)]
pub struct RegisterFile {
    /// Contiguous register storage
    registers: Vec<Value>,
    /// Next free register slot (top of allocated space)
    top: usize,
    /// Maximum number of registers
    max_size: usize,
}

impl RegisterFile {
    /// Create a new register file with default max size
    pub fn new() -> Self {
        Self {
            registers: Vec::with_capacity(256),
            top: 0,
            max_size: DEFAULT_MAX_SIZE,
        }
    }

    /// Create a new register file with specified max size
    pub fn with_max_size(max_size: usize) -> Self {
        Self {
            registers: Vec::with_capacity(256),
            top: 0,
            max_size,
        }
    }

    /// Allocate a new frame of `count` registers, returning the base index.
    ///
    /// All registers are initialized to null.
    pub fn alloc_frame(&mut self, count: usize) -> VmResult<usize> {
        let base = self.top;
        let new_top = base + count;
        if new_top > self.max_size {
            return Err(VmError::StackOverflow);
        }
        // Grow backing storage if needed
        if new_top > self.registers.len() {
            self.registers.resize(new_top, Value::null());
        } else {
            // Zero out reused slots
            for slot in &mut self.registers[base..new_top] {
                *slot = Value::null();
            }
        }
        self.top = new_top;
        Ok(base)
    }

    /// Free the topmost frame, shrinking back to `base`.
    ///
    /// `base` should be the value returned by the corresponding `alloc_frame`.
    #[inline]
    pub fn free_frame(&mut self, base: usize) {
        debug_assert!(base <= self.top);
        self.top = base;
    }

    /// Get register value at absolute index
    #[inline]
    pub fn get(&self, index: usize) -> VmResult<Value> {
        if index < self.top {
            Ok(self.registers[index])
        } else {
            Err(VmError::RuntimeError(format!(
                "Register index {} out of bounds (top={})",
                index, self.top
            )))
        }
    }

    /// Get register value at `reg_base + offset`
    #[inline]
    pub fn get_reg(&self, reg_base: usize, offset: u8) -> VmResult<Value> {
        let index = reg_base + offset as usize;
        if index < self.top {
            Ok(self.registers[index])
        } else {
            Err(VmError::RuntimeError(format!(
                "Register r{} out of bounds (base={}, top={})",
                offset, reg_base, self.top
            )))
        }
    }

    /// Set register value at absolute index
    #[inline]
    pub fn set(&mut self, index: usize, value: Value) -> VmResult<()> {
        if index < self.top {
            self.registers[index] = value;
            Ok(())
        } else {
            Err(VmError::RuntimeError(format!(
                "Register index {} out of bounds (top={})",
                index, self.top
            )))
        }
    }

    /// Set register value at `reg_base + offset`
    #[inline]
    pub fn set_reg(&mut self, reg_base: usize, offset: u8, value: Value) -> VmResult<()> {
        let index = reg_base + offset as usize;
        if index < self.top {
            self.registers[index] = value;
            Ok(())
        } else {
            Err(VmError::RuntimeError(format!(
                "Register r{} out of bounds (base={}, top={})",
                offset, reg_base, self.top
            )))
        }
    }

    /// Get a slice of registers starting at `base` with `count` elements.
    ///
    /// Useful for collecting function call arguments from consecutive registers.
    #[inline]
    pub fn get_slice(&self, base: usize, count: usize) -> VmResult<&[Value]> {
        let end = base + count;
        if end <= self.top {
            Ok(&self.registers[base..end])
        } else {
            Err(VmError::RuntimeError(format!(
                "Register slice [{}..{}] out of bounds (top={})",
                base, end, self.top
            )))
        }
    }

    /// Copy `count` registers from `src_base` to `dst_base`.
    ///
    /// Used for copying arguments during function calls.
    pub fn copy_regs(&mut self, src_base: usize, dst_base: usize, count: usize) -> VmResult<()> {
        let src_end = src_base + count;
        let dst_end = dst_base + count;
        if src_end > self.top || dst_end > self.top {
            return Err(VmError::RuntimeError(format!(
                "Register copy out of bounds: src=[{}..{}], dst=[{}..{}], top={}",
                src_base, src_end, dst_base, dst_end, self.top
            )));
        }
        // Handle overlapping regions safely
        self.registers.copy_within(src_base..src_end, dst_base);
        Ok(())
    }

    /// Current top (next free slot)
    #[inline]
    pub fn top(&self) -> usize {
        self.top
    }

    /// Total number of registers currently in use
    #[inline]
    pub fn depth(&self) -> usize {
        self.top
    }

    /// Maximum register file size
    #[inline]
    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// Check if the register file is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.top == 0
    }

    /// Get statistics about register file usage
    pub fn stats(&self) -> RegisterFileStats {
        RegisterFileStats {
            top: self.top,
            capacity: self.registers.capacity(),
            max_size: self.max_size,
        }
    }
}

impl Default for RegisterFile {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about register file usage
#[derive(Debug, Clone, Copy)]
pub struct RegisterFileStats {
    /// Current top position (registers in use)
    pub top: usize,
    /// Allocated capacity
    pub capacity: usize,
    /// Maximum allowed size
    pub max_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_register_file() {
        let rf = RegisterFile::new();
        assert_eq!(rf.top(), 0);
        assert!(rf.is_empty());
        assert_eq!(rf.max_size(), DEFAULT_MAX_SIZE);
    }

    #[test]
    fn test_alloc_and_free_frame() {
        let mut rf = RegisterFile::new();

        // Allocate frame with 4 registers
        let base = rf.alloc_frame(4).unwrap();
        assert_eq!(base, 0);
        assert_eq!(rf.top(), 4);

        // All registers initialized to null
        for i in 0..4 {
            assert!(rf.get(i).unwrap().is_null());
        }

        // Allocate second frame
        let base2 = rf.alloc_frame(3).unwrap();
        assert_eq!(base2, 4);
        assert_eq!(rf.top(), 7);

        // Free second frame
        rf.free_frame(base2);
        assert_eq!(rf.top(), 4);

        // Free first frame
        rf.free_frame(base);
        assert_eq!(rf.top(), 0);
        assert!(rf.is_empty());
    }

    #[test]
    fn test_get_set_absolute() {
        let mut rf = RegisterFile::new();
        rf.alloc_frame(4).unwrap();

        rf.set(0, Value::i32(42)).unwrap();
        rf.set(1, Value::i32(10)).unwrap();
        rf.set(2, Value::bool(true)).unwrap();
        rf.set(3, Value::null()).unwrap();

        assert_eq!(rf.get(0).unwrap().as_i32(), Some(42));
        assert_eq!(rf.get(1).unwrap().as_i32(), Some(10));
        assert_eq!(rf.get(2).unwrap().as_bool(), Some(true));
        assert!(rf.get(3).unwrap().is_null());
    }

    #[test]
    fn test_get_set_reg() {
        let mut rf = RegisterFile::new();

        // Frame 0: 3 registers at base 0
        let base0 = rf.alloc_frame(3).unwrap();
        rf.set_reg(base0, 0, Value::i32(100)).unwrap();
        rf.set_reg(base0, 1, Value::i32(200)).unwrap();
        rf.set_reg(base0, 2, Value::i32(300)).unwrap();

        // Frame 1: 2 registers at base 3
        let base1 = rf.alloc_frame(2).unwrap();
        rf.set_reg(base1, 0, Value::i32(999)).unwrap();
        rf.set_reg(base1, 1, Value::i32(888)).unwrap();

        // Frame 0 values still intact
        assert_eq!(rf.get_reg(base0, 0).unwrap().as_i32(), Some(100));
        assert_eq!(rf.get_reg(base0, 1).unwrap().as_i32(), Some(200));
        assert_eq!(rf.get_reg(base0, 2).unwrap().as_i32(), Some(300));

        // Frame 1 values
        assert_eq!(rf.get_reg(base1, 0).unwrap().as_i32(), Some(999));
        assert_eq!(rf.get_reg(base1, 1).unwrap().as_i32(), Some(888));
    }

    #[test]
    fn test_get_slice() {
        let mut rf = RegisterFile::new();
        let base = rf.alloc_frame(5).unwrap();

        for i in 0..5u8 {
            rf.set_reg(base, i, Value::i32(i as i32 * 10)).unwrap();
        }

        let slice = rf.get_slice(base + 1, 3).unwrap();
        assert_eq!(slice.len(), 3);
        assert_eq!(slice[0].as_i32(), Some(10));
        assert_eq!(slice[1].as_i32(), Some(20));
        assert_eq!(slice[2].as_i32(), Some(30));
    }

    #[test]
    fn test_copy_regs() {
        let mut rf = RegisterFile::new();
        let base0 = rf.alloc_frame(4).unwrap();

        // Set up source values
        rf.set_reg(base0, 0, Value::i32(1)).unwrap();
        rf.set_reg(base0, 1, Value::i32(2)).unwrap();

        // Allocate callee frame
        let base1 = rf.alloc_frame(3).unwrap();

        // Copy 2 args from caller to callee
        rf.copy_regs(base0, base1, 2).unwrap();

        assert_eq!(rf.get_reg(base1, 0).unwrap().as_i32(), Some(1));
        assert_eq!(rf.get_reg(base1, 1).unwrap().as_i32(), Some(2));
        assert!(rf.get_reg(base1, 2).unwrap().is_null()); // not copied
    }

    #[test]
    fn test_out_of_bounds() {
        let mut rf = RegisterFile::new();
        rf.alloc_frame(2).unwrap();

        // Out of bounds get
        assert!(rf.get(2).is_err());
        assert!(rf.get(100).is_err());

        // Out of bounds set
        assert!(rf.set(2, Value::null()).is_err());

        // Out of bounds get_reg
        assert!(rf.get_reg(0, 2).is_err());

        // Out of bounds set_reg
        assert!(rf.set_reg(0, 2, Value::null()).is_err());
    }

    #[test]
    fn test_overflow() {
        let mut rf = RegisterFile::with_max_size(10);

        rf.alloc_frame(8).unwrap();
        // Can allocate 2 more
        rf.alloc_frame(2).unwrap();
        // Can't allocate any more
        assert!(rf.alloc_frame(1).is_err());
    }

    #[test]
    fn test_frame_reuse() {
        let mut rf = RegisterFile::new();

        // Allocate and populate
        let base = rf.alloc_frame(3).unwrap();
        rf.set(0, Value::i32(42)).unwrap();
        rf.set(1, Value::i32(43)).unwrap();
        rf.set(2, Value::i32(44)).unwrap();

        // Free and reallocate
        rf.free_frame(base);
        let base2 = rf.alloc_frame(3).unwrap();
        assert_eq!(base2, 0);

        // Registers should be zeroed (null)
        assert!(rf.get(0).unwrap().is_null());
        assert!(rf.get(1).unwrap().is_null());
        assert!(rf.get(2).unwrap().is_null());
    }

    #[test]
    fn test_stats() {
        let mut rf = RegisterFile::new();
        rf.alloc_frame(10).unwrap();

        let stats = rf.stats();
        assert_eq!(stats.top, 10);
        assert!(stats.capacity >= 10);
        assert_eq!(stats.max_size, DEFAULT_MAX_SIZE);
    }

    #[test]
    fn test_get_slice_out_of_bounds() {
        let mut rf = RegisterFile::new();
        rf.alloc_frame(3).unwrap();

        // Valid slice
        assert!(rf.get_slice(0, 3).is_ok());

        // Partially out of bounds
        assert!(rf.get_slice(1, 3).is_err());

        // Completely out of bounds
        assert!(rf.get_slice(5, 2).is_err());
    }

    #[test]
    fn test_copy_regs_out_of_bounds() {
        let mut rf = RegisterFile::new();
        rf.alloc_frame(4).unwrap();

        // Source out of bounds
        assert!(rf.copy_regs(3, 0, 2).is_err());

        // Dest out of bounds
        assert!(rf.copy_regs(0, 3, 2).is_err());

        // Valid copy
        assert!(rf.copy_regs(0, 2, 2).is_ok());
    }
}
