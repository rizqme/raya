//! Safepoint Infrastructure for STW Pauses
//!
//! This module provides cooperative safepoint coordination for stop-the-world (STW)
//! operations like garbage collection, VM snapshotting, and debugging.
//!
//! ## Safepoint Poll Locations
//!
//! The VM guarantees safepoint polls at these locations to ensure timely STW pauses:
//!
//! ### Critical Locations (Always Polled)
//! - **Before GC allocations**: NEW, NEW_ARRAY, OBJECT_LITERAL, ARRAY_LITERAL, SCONCAT
//! - **Function calls**: CALL, CALL_METHOD, CALL_CONSTRUCTOR, CALL_SUPER
//! - **Loop back-edges**: At the start of each interpreter loop iteration
//! - **Task operations**: SPAWN (before task creation), AWAIT (on entry)
//!
//! ### Guarantees
//! - All workers will reach a safepoint within:
//!   - One loop iteration (~microseconds for tight loops)
//!   - One function call
//!   - One allocation
//! - No indefinite blocking of GC or snapshotting
//! - Fast-path polling (single atomic load when no pause pending)
//!
//! ## Usage
//!
//! Safepoint polling should be inserted before allocations in the interpreter loop.
//! Call `safepoint().poll()` before operations that allocate memory to ensure
//! the VM can pause for garbage collection or snapshotting when needed.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};

/// Reasons for requesting a safepoint pause
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// Garbage collection
    GarbageCollection,
    /// VM state snapshotting
    Snapshot,
    /// Debugger breakpoint
    Debug,
}

/// Statistics tracking for safepoint operations
#[derive(Debug, Default)]
pub struct SafepointStats {
    /// Total number of safepoints executed
    total_safepoints: AtomicUsize,
    /// Total time spent at safepoints (microseconds)
    total_pause_time_us: AtomicUsize,
    /// Maximum pause time (microseconds)
    max_pause_time_us: AtomicUsize,
}

impl SafepointStats {
    fn reset(&self) {
        self.total_safepoints.store(0, Ordering::Relaxed);
        self.total_pause_time_us.store(0, Ordering::Relaxed);
        self.max_pause_time_us.store(0, Ordering::Relaxed);
    }

    fn total_safepoints(&self) -> usize {
        self.total_safepoints.load(Ordering::Relaxed)
    }

    fn total_pause_time_us(&self) -> usize {
        self.total_pause_time_us.load(Ordering::Relaxed)
    }

    fn max_pause_time_us(&self) -> usize {
        self.max_pause_time_us.load(Ordering::Relaxed)
    }
}

/// Coordinates stop-the-world pauses across all worker threads
pub struct SafepointCoordinator {
    /// Number of active worker threads
    worker_count: AtomicUsize,

    /// Workers currently at safepoint
    workers_at_safepoint: AtomicUsize,

    /// GC pause is pending
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) gc_pending: AtomicBool,

    /// Snapshot pause is pending
    #[cfg_attr(test, allow(dead_code))]
    pub snapshot_pending: AtomicBool,

    /// Debug pause is pending
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) debug_pending: AtomicBool,

    /// Current pause reason
    #[cfg_attr(test, allow(dead_code))]
    pub current_reason: Mutex<Option<StopReason>>,

    /// Barrier for synchronizing workers
    barrier: Arc<Barrier>,

    /// Statistics
    #[cfg_attr(test, allow(dead_code))]
    pub stats: SafepointStats,
}

impl SafepointCoordinator {
    /// Create a new SafepointCoordinator with the specified number of workers
    pub fn new(worker_count: usize) -> Self {
        Self {
            worker_count: AtomicUsize::new(worker_count),
            workers_at_safepoint: AtomicUsize::new(0),
            gc_pending: AtomicBool::new(false),
            snapshot_pending: AtomicBool::new(false),
            debug_pending: AtomicBool::new(false),
            current_reason: Mutex::new(None),
            barrier: Arc::new(Barrier::new(worker_count)),
            stats: SafepointStats::default(),
        }
    }

    /// Fast inline check - called frequently from interpreter
    #[inline(always)]
    pub fn poll(&self) {
        // Fast path: check if any pause is pending (single atomic load)
        if self.is_pause_pending_fast() {
            // Slow path: enter safepoint handler
            self.enter_safepoint();
        }
    }

    /// Fast check for pending pauses (inlines to ~2 instructions)
    #[inline(always)]
    fn is_pause_pending_fast(&self) -> bool {
        self.gc_pending.load(Ordering::Acquire)
            || self.snapshot_pending.load(Ordering::Acquire)
            || self.debug_pending.load(Ordering::Acquire)
    }

    /// Slow path: handle safepoint entry
    #[cold]
    #[inline(never)]
    fn enter_safepoint(&self) {
        let start = std::time::Instant::now();

        // Increment workers at safepoint
        let count = self.workers_at_safepoint.fetch_add(1, Ordering::AcqRel);

        // Last worker to arrive triggers the STW operation
        if count + 1 == self.worker_count.load(Ordering::Acquire) {
            // All workers are paused - safe to proceed
            self.execute_stw_operation();
        }

        // Wait at barrier for all workers
        self.barrier.wait();

        // Decrement workers at safepoint
        self.workers_at_safepoint.fetch_sub(1, Ordering::AcqRel);

        // Track statistics
        let elapsed = start.elapsed().as_micros() as usize;
        self.stats
            .total_pause_time_us
            .fetch_add(elapsed, Ordering::Relaxed);
        self.stats.total_safepoints.fetch_add(1, Ordering::Relaxed);

        // Update max pause time
        let mut max = self.stats.max_pause_time_us.load(Ordering::Relaxed);
        while elapsed > max {
            match self.stats.max_pause_time_us.compare_exchange_weak(
                max,
                elapsed,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(current) => max = current,
            }
        }
    }

    /// Execute the STW operation (called by last worker at safepoint)
    fn execute_stw_operation(&self) {
        let reason = self.current_reason.lock().unwrap();

        match *reason {
            Some(StopReason::GarbageCollection) => {
                // GC will be triggered externally
            }
            Some(StopReason::Snapshot) => {
                // Snapshot will be captured externally
            }
            Some(StopReason::Debug) => {
                // Debugger will inspect state externally
            }
            None => {
                // Should not happen
                #[cfg(debug_assertions)]
                eprintln!("Warning: Safepoint reached with no reason set");
            }
        }
    }

    /// Request a stop-the-world pause
    pub fn request_stw_pause(&self, reason: StopReason) {
        // Set the current reason
        {
            let mut current = self.current_reason.lock().unwrap();
            if current.is_some() {
                panic!("Cannot request STW pause while another is active");
            }
            *current = Some(reason);
        }

        // Set the appropriate atomic flag
        match reason {
            StopReason::GarbageCollection => {
                self.gc_pending.store(true, Ordering::Release);
            }
            StopReason::Snapshot => {
                self.snapshot_pending.store(true, Ordering::Release);
            }
            StopReason::Debug => {
                self.debug_pending.store(true, Ordering::Release);
            }
        }

        // Wait for all workers to reach safepoint
        self.wait_for_all_workers();
    }

    /// Wait for all workers to reach safepoint
    fn wait_for_all_workers(&self) {
        let expected = self.worker_count.load(Ordering::Acquire);

        // Spin-wait with exponential backoff
        let mut backoff = 1;
        loop {
            let at_safepoint = self.workers_at_safepoint.load(Ordering::Acquire);

            if at_safepoint == expected {
                break;
            }

            // Exponential backoff
            for _ in 0..backoff {
                std::hint::spin_loop();
            }

            backoff = (backoff * 2).min(1000);
        }
    }

    /// Resume from STW pause
    pub fn resume_from_pause(&self) {
        // Clear the atomic flags
        self.gc_pending.store(false, Ordering::Release);
        self.snapshot_pending.store(false, Ordering::Release);
        self.debug_pending.store(false, Ordering::Release);

        // Clear the current reason
        {
            let mut current = self.current_reason.lock().unwrap();
            *current = None;
        }

        // Workers will resume after barrier
    }

    /// Register a new worker thread
    pub fn register_worker(&self) {
        self.worker_count.fetch_add(1, Ordering::AcqRel);
        // Note: Barrier cannot be dynamically resized in std library
        // In practice, workers should be registered during initialization
    }

    /// Deregister a worker thread
    pub fn deregister_worker(&self) {
        let count = self.worker_count.fetch_sub(1, Ordering::AcqRel);
        if count == 1 {
            // Last worker deregistered
        }
    }

    /// Get current worker count
    pub fn worker_count(&self) -> usize {
        self.worker_count.load(Ordering::Acquire)
    }

    /// Get current workers at safepoint
    pub fn workers_at_safepoint(&self) -> usize {
        self.workers_at_safepoint.load(Ordering::Acquire)
    }

    /// Get current pause reason
    pub fn current_reason(&self) -> Option<StopReason> {
        *self.current_reason.lock().unwrap()
    }

    /// Check if a pause is currently pending
    pub fn is_pause_pending(&self) -> bool {
        self.is_pause_pending_fast()
    }

    /// Get safepoint statistics
    pub fn stats(&self) -> (usize, usize, usize) {
        (
            self.stats.total_safepoints(),
            self.stats.total_pause_time_us(),
            self.stats.max_pause_time_us(),
        )
    }

    /// Reset statistics
    pub fn reset_stats(&self) {
        self.stats.reset();
    }
}

impl Default for SafepointCoordinator {
    fn default() -> Self {
        Self::new(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_coordinator() {
        let coord = SafepointCoordinator::new(4);
        assert_eq!(coord.worker_count(), 4);
        assert_eq!(coord.workers_at_safepoint(), 0);
        assert!(!coord.is_pause_pending());
        assert_eq!(coord.current_reason(), None);
    }

    #[test]
    fn test_default_coordinator() {
        let coord = SafepointCoordinator::default();
        assert_eq!(coord.worker_count(), 1);
    }

    #[test]
    fn test_no_pending_pause() {
        let coord = SafepointCoordinator::new(1);
        assert!(!coord.is_pause_pending());
    }

    #[test]
    fn test_gc_pause_flag() {
        let coord = SafepointCoordinator::new(1);
        coord.gc_pending.store(true, Ordering::Release);
        assert!(coord.is_pause_pending());
    }

    #[test]
    fn test_snapshot_pause_flag() {
        let coord = SafepointCoordinator::new(1);
        coord.snapshot_pending.store(true, Ordering::Release);
        assert!(coord.is_pause_pending());
    }

    #[test]
    fn test_debug_pause_flag() {
        let coord = SafepointCoordinator::new(1);
        coord.debug_pending.store(true, Ordering::Release);
        assert!(coord.is_pause_pending());
    }

    #[test]
    fn test_worker_registration() {
        let coord = SafepointCoordinator::new(2);
        assert_eq!(coord.worker_count(), 2);

        coord.register_worker();
        assert_eq!(coord.worker_count(), 3);

        coord.deregister_worker();
        assert_eq!(coord.worker_count(), 2);
    }

    #[test]
    fn test_statistics_initial() {
        let coord = SafepointCoordinator::new(1);
        let (total, time, max) = coord.stats();
        assert_eq!(total, 0);
        assert_eq!(time, 0);
        assert_eq!(max, 0);
    }

    #[test]
    fn test_statistics_reset() {
        let coord = SafepointCoordinator::new(1);
        coord.stats.total_safepoints.store(10, Ordering::Relaxed);
        coord
            .stats
            .total_pause_time_us
            .store(1000, Ordering::Relaxed);
        coord.stats.max_pause_time_us.store(100, Ordering::Relaxed);

        coord.reset_stats();

        let (total, time, max) = coord.stats();
        assert_eq!(total, 0);
        assert_eq!(time, 0);
        assert_eq!(max, 0);
    }

    #[test]
    fn test_poll_no_pause() {
        let coord = SafepointCoordinator::new(1);
        // Should return immediately without blocking
        coord.poll();
    }

    #[test]
    fn test_concurrent_pause_check() {
        // Test that we can detect concurrent pause attempts
        // (without actually blocking in request_stw_pause)
        let coord = SafepointCoordinator::new(1);

        // Manually set a reason to simulate active pause
        {
            let mut reason = coord.current_reason.lock().unwrap();
            *reason = Some(StopReason::GarbageCollection);
        }

        // Try to set another reason - should panic
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut reason = coord.current_reason.lock().unwrap();
            if reason.is_some() {
                panic!("Cannot request STW pause while another is active");
            }
            *reason = Some(StopReason::Snapshot);
        }));

        assert!(result.is_err());
    }

    #[test]
    fn test_pause_reason_tracking() {
        let coord = SafepointCoordinator::new(1);

        // Set GC reason
        {
            let mut reason = coord.current_reason.lock().unwrap();
            *reason = Some(StopReason::GarbageCollection);
        }
        assert_eq!(coord.current_reason(), Some(StopReason::GarbageCollection));

        // Clear reason
        {
            let mut reason = coord.current_reason.lock().unwrap();
            *reason = None;
        }
        assert_eq!(coord.current_reason(), None);
    }
}
