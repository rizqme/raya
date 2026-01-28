//! Parser guards to prevent infinite loops and stack overflow

use super::ParseError;
use crate::parser::token::Span;

/// Maximum iterations for any parser loop before panic
const MAX_LOOP_ITERATIONS: usize = 10_000;

/// Maximum nesting depth before rejecting parse
///
/// NOTE: This is set conservatively to 30 because:
/// - In debug builds, Rust's call stack can overflow before higher limits
/// - Object expressions have deeper call stacks (overflow at ~39 levels in debug)
/// - Arrays/expressions can go higher (~50 levels) but we use one limit for all
/// - 30 levels is still far more than any realistic code needs
/// - In release builds with optimization, this could be higher, but we keep it
///   low for safety and consistency across build modes
/// - This gives a safety margin before stack overflow in test threads
pub const MAX_PARSE_DEPTH: usize = 30;

/// Default span for errors without location
#[inline]
fn default_span() -> Span {
    Span::new(0, 0, 0, 0)
}

/// Guard against infinite loops in parser
///
/// Tracks iteration count and returns error if exceeded.
///
/// # Example
///
/// ```ignore
/// let mut guard = LoopGuard::new("parse_attributes");
/// while !done {
///     guard.check()?;
///     // ... parse something ...
/// }
/// ```
pub struct LoopGuard {
    name: &'static str,
    count: usize,
    max: usize,
}

impl LoopGuard {
    /// Create a new loop guard with default limit
    #[inline]
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            count: 0,
            max: MAX_LOOP_ITERATIONS,
        }
    }

    /// Create a loop guard with custom limit
    #[inline]
    pub fn with_limit(name: &'static str, max: usize) -> Self {
        Self { name, count: 0, max }
    }

    /// Check iteration count, return error if exceeded
    #[inline]
    pub fn check(&mut self) -> Result<(), ParseError> {
        self.count += 1;
        if self.count > self.max {
            return Err(ParseError::parser_limit_exceeded(
                format!("Loop '{}' exceeded {} iterations", self.name, self.max),
                default_span(),
            ));
        }
        Ok(())
    }

    /// Reset counter (for nested loops)
    #[inline]
    pub fn reset(&mut self) {
        self.count = 0;
    }
}

/// RAII guard that tracks recursion depth
///
/// Automatically decrements depth on drop.
///
/// # Example
///
/// ```ignore
/// fn parse_expression(parser: &mut Parser) -> Result<Expression, ParseError> {
///     let _guard = parser.enter_depth("expression")?;
///     // ... recursive parsing ...
/// }
/// ```
pub struct DepthGuard<'a> {
    depth: &'a mut usize,
}

impl<'a> DepthGuard<'a> {
    /// Create a new depth guard
    #[inline]
    pub fn new(depth: &'a mut usize, name: &'static str) -> Result<Self, ParseError> {
        *depth += 1;
        if *depth > MAX_PARSE_DEPTH {
            *depth -= 1; // Reset before returning error
            return Err(ParseError::parser_limit_exceeded(
                format!("Maximum nesting depth ({}) exceeded in {}", MAX_PARSE_DEPTH, name),
                default_span(),
            ));
        }
        Ok(Self { depth })
    }
}

impl Drop for DepthGuard<'_> {
    #[inline]
    fn drop(&mut self) {
        *self.depth -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loop_guard_under_limit() {
        let mut guard = LoopGuard::with_limit("test", 10);
        for _ in 0..10 {
            assert!(guard.check().is_ok());
        }
    }

    #[test]
    fn test_loop_guard_exceeds_limit() {
        let mut guard = LoopGuard::with_limit("test", 10);
        for _ in 0..10 {
            let _ = guard.check();
        }
        // 11th iteration should fail
        assert!(guard.check().is_err());
    }

    #[test]
    fn test_loop_guard_reset() {
        let mut guard = LoopGuard::with_limit("test", 5);
        for _ in 0..5 {
            let _ = guard.check();
        }
        guard.reset();
        // Should work again after reset
        assert!(guard.check().is_ok());
    }

    #[test]
    fn test_depth_guard_within_limit() {
        let mut depth = 0;

        // Test that guard increments and decrements properly
        {
            let _g1 = DepthGuard::new(&mut depth, "test").unwrap();
        }
        // After guard is dropped, depth should be back to 0
        assert_eq!(depth, 0);

        // Create and drop multiple guards
        for _ in 0..10 {
            let _g = DepthGuard::new(&mut depth, "test").unwrap();
            // Guard drops at end of loop iteration
        }
        // All guards dropped, depth should be 0
        assert_eq!(depth, 0);
    }

    #[test]
    fn test_depth_guard_exceeds_limit() {
        let mut depth = MAX_PARSE_DEPTH;
        {
            let result = DepthGuard::new(&mut depth, "test");
            assert!(result.is_err());
        }
        // Should not have incremented
        assert_eq!(depth, MAX_PARSE_DEPTH);
    }
}
