//! Profiling counters for hot function detection
//!
//! Atomic counters that the interpreter increments at function entry
//! and backward jumps to identify hot functions for JIT compilation.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Profiling counters for a single function
pub struct FunctionProfile {
    /// Number of times this function has been called
    pub call_count: AtomicU32,
    /// Number of times backward jumps (loops) have been taken
    pub loop_count: AtomicU32,
    /// Whether compilation is currently in progress
    pub compiling: AtomicBool,
    /// Whether JIT-compiled code is available
    pub jit_available: AtomicBool,
}

impl FunctionProfile {
    /// Create a new profile with zero counters
    pub fn new() -> Self {
        FunctionProfile {
            call_count: AtomicU32::new(0),
            loop_count: AtomicU32::new(0),
            compiling: AtomicBool::new(false),
            jit_available: AtomicBool::new(false),
        }
    }

    /// Record a function call, return new count
    pub fn record_call(&self) -> u32 {
        self.call_count.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Record a loop iteration (backward jump), return new count
    pub fn record_loop(&self) -> u32 {
        self.loop_count.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Try to claim this function for compilation (CAS on `compiling` flag)
    /// Returns true if we successfully claimed it
    pub fn try_start_compile(&self) -> bool {
        self.compiling
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    }

    /// Mark compilation as complete and JIT code as available
    pub fn finish_compile(&self) {
        self.jit_available.store(true, Ordering::Release);
        self.compiling.store(false, Ordering::Release);
    }

    /// Check if JIT code is available
    pub fn is_jit_available(&self) -> bool {
        self.jit_available.load(Ordering::Acquire)
    }
}

impl Default for FunctionProfile {
    fn default() -> Self {
        Self::new()
    }
}

/// Profiling data for all functions in a module
pub struct ModuleProfile {
    /// Per-function profiling counters
    pub functions: Vec<FunctionProfile>,
}

impl ModuleProfile {
    /// Create a profile for a module with the given number of functions
    pub fn new(function_count: usize) -> Self {
        let mut functions = Vec::with_capacity(function_count);
        for _ in 0..function_count {
            functions.push(FunctionProfile::new());
        }
        ModuleProfile { functions }
    }

    /// Record a call to function `func_id`, return new call count
    pub fn record_call(&self, func_id: usize) -> u32 {
        if func_id < self.functions.len() {
            self.functions[func_id].record_call()
        } else {
            0
        }
    }

    /// Record a loop iteration in function `func_id`, return new loop count
    pub fn record_loop(&self, func_id: usize) -> u32 {
        if func_id < self.functions.len() {
            self.functions[func_id].record_loop()
        } else {
            0
        }
    }

    /// Get the profile for a specific function
    pub fn get(&self, func_id: usize) -> Option<&FunctionProfile> {
        self.functions.get(func_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_profile_counters() {
        let profile = FunctionProfile::new();
        assert_eq!(profile.record_call(), 1);
        assert_eq!(profile.record_call(), 2);
        assert_eq!(profile.record_call(), 3);
        assert_eq!(profile.record_loop(), 1);
        assert_eq!(profile.record_loop(), 2);
    }

    #[test]
    fn test_compilation_lifecycle() {
        let profile = FunctionProfile::new();
        assert!(!profile.is_jit_available());

        // Claim for compilation
        assert!(profile.try_start_compile());
        // Can't claim again while compiling
        assert!(!profile.try_start_compile());

        // Finish compilation
        profile.finish_compile();
        assert!(profile.is_jit_available());

        // Can claim again for recompilation
        assert!(profile.try_start_compile());
    }

    #[test]
    fn test_module_profile() {
        let profile = ModuleProfile::new(3);
        assert_eq!(profile.functions.len(), 3);

        assert_eq!(profile.record_call(0), 1);
        assert_eq!(profile.record_call(0), 2);
        assert_eq!(profile.record_call(1), 1);
        assert_eq!(profile.record_loop(2), 1);

        // Out-of-bounds returns 0
        assert_eq!(profile.record_call(99), 0);
    }
}
