//! Asynchronous preemption monitor (like Go's sysmon)
//!
//! This module implements Go-style asynchronous preemption to prevent
//! long-running tasks from monopolizing worker threads.

use crate::scheduler::{Task, TaskId};
use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Default preemption threshold (like Go's 10ms)
pub const DEFAULT_PREEMPT_THRESHOLD: Duration = Duration::from_millis(10);

/// Preemption monitor (like Go's sysmon goroutine)
pub struct PreemptMonitor {
    /// All active tasks
    tasks: Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,

    /// Preemption threshold (how long before requesting preemption)
    threshold: Duration,

    /// Monitor thread handle
    handle: Option<thread::JoinHandle<()>>,

    /// Shutdown signal
    shutdown: Arc<AtomicBool>,
}

impl PreemptMonitor {
    /// Create a new preemption monitor
    pub fn new(tasks: Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>, threshold: Duration) -> Self {
        Self {
            tasks,
            threshold,
            handle: None,
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the monitoring thread
    pub fn start(&mut self) {
        let tasks = self.tasks.clone();
        let threshold = self.threshold;
        let shutdown = self.shutdown.clone();

        let handle = thread::Builder::new()
            .name("raya-preempt-monitor".to_string())
            .spawn(move || {
                PreemptMonitor::monitor_loop(tasks, threshold, shutdown);
            })
            .expect("Failed to spawn preemption monitor thread");

        self.handle = Some(handle);
    }

    /// Monitoring loop (like Go's sysmon)
    fn monitor_loop(
        tasks: Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,
        threshold: Duration,
        shutdown: Arc<AtomicBool>,
    ) {
        // Check every 1ms (like Go's sysmon polls frequently)
        let check_interval = Duration::from_millis(1);

        loop {
            // Check shutdown signal
            if shutdown.load(Ordering::Acquire) {
                break;
            }

            // Get current time
            let now = Instant::now();

            // Check all running tasks
            {
                let tasks_guard = tasks.read();
                for task in tasks_guard.values() {
                    // Only check running tasks
                    if task.state() != crate::scheduler::TaskState::Running {
                        continue;
                    }

                    // Check if task has been running too long
                    if let Some(start_time) = task.start_time() {
                        let elapsed = now.duration_since(start_time);

                        if elapsed >= threshold {
                            // Request asynchronous preemption
                            task.request_preempt();

                            #[cfg(debug_assertions)]
                            eprintln!(
                                "Preemption requested for task {} (running for {:?})",
                                task.id().as_u64(),
                                elapsed
                            );
                        }
                    }
                }
            }

            // Sleep briefly before next check
            thread::sleep(check_interval);
        }

        #[cfg(debug_assertions)]
        eprintln!("Preemption monitor shutting down");
    }

    /// Stop the monitoring thread
    pub fn stop(&mut self) {
        self.shutdown.store(true, Ordering::Release);

        if let Some(handle) = self.handle.take() {
            handle
                .join()
                .expect("Failed to join preemption monitor thread");
        }
    }

    /// Check if the monitor is running
    pub fn is_running(&self) -> bool {
        self.handle.is_some() && !self.shutdown.load(Ordering::Acquire)
    }
}

impl Drop for PreemptMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raya_bytecode::{Function, Module, Opcode};

    fn create_test_task() -> Arc<Task> {
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "test".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![Opcode::Return as u8],
        });

        Arc::new(Task::new(0, Arc::new(module), None))
    }

    #[test]
    fn test_preempt_monitor_creation() {
        let tasks = Arc::new(RwLock::new(FxHashMap::default()));
        let monitor = PreemptMonitor::new(tasks, DEFAULT_PREEMPT_THRESHOLD);

        assert!(!monitor.is_running());
    }

    #[test]
    fn test_preempt_monitor_start_stop() {
        let tasks = Arc::new(RwLock::new(FxHashMap::default()));
        let mut monitor = PreemptMonitor::new(tasks, DEFAULT_PREEMPT_THRESHOLD);

        monitor.start();
        assert!(monitor.is_running());

        thread::sleep(Duration::from_millis(10));

        monitor.stop();
        assert!(!monitor.is_running());
    }

    #[test]
    fn test_preemption_request() {
        let tasks = Arc::new(RwLock::new(FxHashMap::default()));
        let task = create_test_task();
        let task_id = task.id();

        // Mark task as running with old start time
        task.set_state(crate::scheduler::TaskState::Running);
        task.set_start_time(Instant::now() - Duration::from_millis(20));

        tasks.write().insert(task_id, task.clone());

        // Start monitor with short threshold
        let mut monitor = PreemptMonitor::new(tasks.clone(), Duration::from_millis(5));
        monitor.start();

        // Wait for monitor to detect and request preemption
        thread::sleep(Duration::from_millis(10));

        // Check that preemption was requested
        assert!(task.is_preempt_requested());

        monitor.stop();
    }

    #[test]
    fn test_no_preemption_for_recent_task() {
        let tasks = Arc::new(RwLock::new(FxHashMap::default()));
        let task = create_test_task();
        let task_id = task.id();

        // Mark task as running with recent start time
        task.set_state(crate::scheduler::TaskState::Running);
        task.set_start_time(Instant::now());

        tasks.write().insert(task_id, task.clone());

        // Start monitor
        let mut monitor = PreemptMonitor::new(tasks.clone(), DEFAULT_PREEMPT_THRESHOLD);
        monitor.start();

        // Wait briefly
        thread::sleep(Duration::from_millis(5));

        // Task should not be preempted yet
        assert!(!task.is_preempt_requested());

        monitor.stop();
    }
}
