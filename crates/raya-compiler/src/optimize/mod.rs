//! IR Optimization Passes
//!
//! Provides basic optimizations on the IR before bytecode generation.

mod constant_fold;
mod dce;

pub use constant_fold::ConstantFolder;
pub use dce::DeadCodeEliminator;

use crate::ir::IrModule;

/// Optimization level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OptLevel {
    /// No optimizations
    None,
    /// Basic optimizations (constant folding, DCE)
    #[default]
    Basic,
    /// Full optimizations (includes inlining, etc.)
    Full,
}

/// Optimizer that runs multiple passes over the IR
pub struct Optimizer {
    level: OptLevel,
}

impl Optimizer {
    /// Create a new optimizer with the given level
    pub fn new(level: OptLevel) -> Self {
        Self { level }
    }

    /// Create an optimizer with basic optimizations
    pub fn basic() -> Self {
        Self::new(OptLevel::Basic)
    }

    /// Create an optimizer with no optimizations
    pub fn none() -> Self {
        Self::new(OptLevel::None)
    }

    /// Run all optimization passes on the module
    pub fn optimize(&self, module: &mut IrModule) {
        if self.level == OptLevel::None {
            return;
        }

        // Run constant folding
        let folder = ConstantFolder::new();
        folder.fold(module);

        // Run dead code elimination
        let dce = DeadCodeEliminator::new();
        dce.eliminate(module);

        // Run constant folding again after DCE (may expose more opportunities)
        if self.level == OptLevel::Full {
            folder.fold(module);
        }
    }

    /// Get statistics about optimizations performed
    pub fn stats(&self) -> OptStats {
        OptStats::default()
    }
}

/// Statistics about optimizations performed
#[derive(Debug, Clone, Default)]
pub struct OptStats {
    /// Number of constants folded
    pub constants_folded: usize,
    /// Number of dead instructions eliminated
    pub dead_instructions_removed: usize,
    /// Number of unreachable blocks removed
    pub unreachable_blocks_removed: usize,
}

impl Default for Optimizer {
    fn default() -> Self {
        Self::basic()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optimizer_levels() {
        let opt = Optimizer::none();
        assert_eq!(opt.level, OptLevel::None);

        let opt = Optimizer::basic();
        assert_eq!(opt.level, OptLevel::Basic);
    }
}
