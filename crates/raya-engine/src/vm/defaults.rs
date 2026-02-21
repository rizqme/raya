//! Default constants for VM configuration.
//!
//! Centralizes magic numbers that were previously duplicated across
//! context.rs, shared_state.rs, scheduler.rs, and collector.rs.

/// Default maximum consecutive preemptions before killing a task (infinite loop detection).
pub const DEFAULT_MAX_PREEMPTIONS: u32 = 1000;

/// Default preemption time slice in milliseconds.
pub const DEFAULT_PREEMPT_THRESHOLD_MS: u64 = 10;

/// Default initial GC heap threshold in bytes (1 MB).
pub const DEFAULT_GC_THRESHOLD: usize = 1024 * 1024;

/// JIT adaptive compilation policy check mask.
/// The interpreter checks compilation policy every `(count & MASK) == 0` calls,
/// i.e. every 256 calls with the default mask of 0xFF.
#[cfg(feature = "jit")]
pub const JIT_POLICY_CHECK_MASK: u32 = 0xFF;
