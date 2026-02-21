//! Stack pool for reusing Stack allocations across task lifetimes.
//!
//! When a task completes, its Stack (which may have grown to hold hundreds of
//! values) is returned to the pool. The next spawned task acquires a recycled
//! Stack, reusing the existing Vec capacity and avoiding re-allocation.

use crate::vm::stack::Stack;
use parking_lot::Mutex;

/// Pool of reusable Stack objects.
///
/// Stacks retain their allocated Vec capacity when returned to the pool,
/// so subsequent tasks reuse the memory without re-allocating.
pub struct StackPool {
    stacks: Mutex<Vec<Stack>>,
    max_size: usize,
}

impl StackPool {
    /// Create a new pool that holds up to `max_size` stacks.
    pub fn new(max_size: usize) -> Self {
        Self {
            stacks: Mutex::new(Vec::with_capacity(max_size)),
            max_size,
        }
    }

    /// Get a stack from the pool, or create a new one.
    pub fn acquire(&self) -> Stack {
        self.stacks.lock().pop().unwrap_or_else(Stack::new)
    }

    /// Return a stack to the pool for reuse.
    pub fn release(&self, mut stack: Stack) {
        stack.reset();
        let mut pool = self.stacks.lock();
        if pool.len() < self.max_size {
            pool.push(stack);
        }
        // else: drop — pool is full
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::value::Value;

    #[test]
    fn test_pool_acquire_release() {
        let pool = StackPool::new(4);

        // Acquire returns fresh stack
        let mut stack = pool.acquire();
        assert_eq!(stack.depth(), 0);

        // Use the stack
        stack.push(Value::i32(42)).unwrap();
        stack.push(Value::i32(100)).unwrap();
        assert_eq!(stack.depth(), 2);

        // Return it
        pool.release(stack);

        // Acquire again — should get the recycled stack (reset to empty)
        let recycled = pool.acquire();
        assert_eq!(recycled.depth(), 0);
        // The recycled stack has retained its Vec capacity (not observable via public API,
        // but avoids re-allocation on next use).
    }

    #[test]
    fn test_pool_max_size() {
        let pool = StackPool::new(2);

        let s1 = pool.acquire();
        let s2 = pool.acquire();
        let s3 = pool.acquire();

        pool.release(s1);
        pool.release(s2);
        pool.release(s3); // exceeds max_size — should be dropped

        // Pool should have exactly 2 stacks
        let _a = pool.acquire();
        let _b = pool.acquire();
        let c = pool.acquire(); // pool empty — creates fresh
        assert_eq!(c.depth(), 0);
    }
}
