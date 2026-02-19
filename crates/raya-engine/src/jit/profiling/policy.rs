//! Compilation policy — decides when to JIT-compile a function
//!
//! Uses call counts and loop counts from profiling to determine
//! when a function is "hot enough" to warrant compilation.

use super::counters::FunctionProfile;

/// Configuration for when to trigger JIT compilation
#[derive(Debug, Clone)]
pub struct CompilationPolicy {
    /// Call count threshold before compiling (default: 1000)
    pub call_threshold: u32,
    /// Loop iteration threshold before compiling (default: 10_000)
    pub loop_threshold: u32,
    /// Maximum bytecode size to compile (skip very large functions)
    pub max_function_size: usize,
}

impl CompilationPolicy {
    /// Create a policy with default thresholds
    pub fn new() -> Self {
        CompilationPolicy {
            call_threshold: 1000,
            loop_threshold: 10_000,
            max_function_size: 4096,
        }
    }

    /// Check if a function should be compiled based on its profile and code size
    pub fn should_compile(&self, profile: &FunctionProfile, code_size: usize) -> bool {
        // Don't compile if already available or in progress
        if profile.is_jit_available() || profile.compiling.load(std::sync::atomic::Ordering::Relaxed) {
            return false;
        }

        // Don't compile very large functions
        if code_size > self.max_function_size {
            return false;
        }

        let calls = profile.call_count.load(std::sync::atomic::Ordering::Relaxed);
        let loops = profile.loop_count.load(std::sync::atomic::Ordering::Relaxed);

        calls >= self.call_threshold || loops >= self.loop_threshold
    }
}

impl Default for CompilationPolicy {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::profiling::counters::FunctionProfile;

    #[test]
    fn test_default_policy() {
        let policy = CompilationPolicy::new();
        assert_eq!(policy.call_threshold, 1000);
        assert_eq!(policy.loop_threshold, 10_000);
        assert_eq!(policy.max_function_size, 4096);
    }

    #[test]
    fn test_below_threshold() {
        let policy = CompilationPolicy::new();
        let profile = FunctionProfile::new();

        // Not enough calls
        for _ in 0..999 {
            profile.record_call();
        }
        assert!(!policy.should_compile(&profile, 100));
    }

    #[test]
    fn test_call_threshold_reached() {
        let policy = CompilationPolicy::new();
        let profile = FunctionProfile::new();

        for _ in 0..1000 {
            profile.record_call();
        }
        assert!(policy.should_compile(&profile, 100));
    }

    #[test]
    fn test_loop_threshold_reached() {
        let policy = CompilationPolicy::new();
        let profile = FunctionProfile::new();

        for _ in 0..10_000 {
            profile.record_loop();
        }
        assert!(policy.should_compile(&profile, 100));
    }

    #[test]
    fn test_too_large() {
        let policy = CompilationPolicy::new();
        let profile = FunctionProfile::new();

        for _ in 0..2000 {
            profile.record_call();
        }
        // Hot enough but too large
        assert!(!policy.should_compile(&profile, 5000));
    }

    #[test]
    fn test_already_compiled() {
        let policy = CompilationPolicy::new();
        let profile = FunctionProfile::new();

        for _ in 0..2000 {
            profile.record_call();
        }
        profile.finish_compile(); // Mark as compiled

        assert!(!policy.should_compile(&profile, 100));
    }

    #[test]
    fn test_currently_compiling() {
        let policy = CompilationPolicy::new();
        let profile = FunctionProfile::new();

        for _ in 0..2000 {
            profile.record_call();
        }
        profile.try_start_compile();

        assert!(!policy.should_compile(&profile, 100));
    }
}
