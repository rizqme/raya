//! Timer thread for efficient sleep handling
//!
//! Instead of polling for sleeping tasks, this timer thread efficiently
//! waits for the next wake time using condvar timeouts.

use crate::vm::scheduler::{Task, TaskId, TaskState};
use crossbeam_deque::Injector;
use parking_lot::{Condvar, Mutex};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Instant;

/// Entry in the timer heap
struct SleepEntry {
    /// When to wake this task
    wake_at: Instant,
    /// Task ID to wake
    task_id: TaskId,
    /// Reference to the task (for state checking)
    task: Arc<Task>,
}

// Reverse ordering for min-heap (earliest wake time first)
impl Ord for SleepEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse comparison for min-heap
        other.wake_at.cmp(&self.wake_at)
    }
}

impl PartialOrd for SleepEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for SleepEntry {
    fn eq(&self, other: &Self) -> bool {
        self.wake_at == other.wake_at && self.task_id == other.task_id
    }
}

impl Eq for SleepEntry {}

/// Timer thread state
struct TimerState {
    /// Tasks waiting to wake up, sorted by wake time (min-heap)
    sleeping: BinaryHeap<SleepEntry>,
}

/// Timer thread for efficient sleep handling
pub struct TimerThread {
    /// Internal state protected by mutex
    state: Mutex<TimerState>,
    /// Condvar to wake timer thread when new entry added or shutdown
    notify: Condvar,
    /// Shutdown signal
    shutdown: AtomicBool,
    /// Thread handle
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl TimerThread {
    /// Create a new timer thread
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(TimerState {
                sleeping: BinaryHeap::new(),
            }),
            notify: Condvar::new(),
            shutdown: AtomicBool::new(false),
            handle: Mutex::new(None),
        })
    }

    /// Start the timer thread
    pub fn start(self: &Arc<Self>, injector: Arc<Injector<Arc<Task>>>) {
        let timer = Arc::clone(self);

        let handle = thread::Builder::new()
            .name("raya-timer".to_string())
            .spawn(move || {
                timer.run_loop(injector);
            })
            .expect("Failed to spawn timer thread");

        *self.handle.lock() = Some(handle);
    }

    /// Stop the timer thread
    pub fn stop(&self) {
        self.shutdown.store(true, AtomicOrdering::Release);
        self.notify.notify_one();

        if let Some(handle) = self.handle.lock().take() {
            let start = Instant::now();
            let timeout = std::time::Duration::from_secs(2);
            loop {
                if handle.is_finished() {
                    let _ = handle.join();
                    return;
                }
                if start.elapsed() > timeout {
                    drop(handle);
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }

    /// Register a task to wake at a specific time
    pub fn register(&self, task: Arc<Task>, wake_at: Instant) {
        let task_id = task.id();
        let mut state = self.state.lock();
        state.sleeping.push(SleepEntry {
            wake_at,
            task_id,
            task,
        });
        // Notify timer thread that a new entry was added
        // (it might need to wake earlier than currently scheduled)
        self.notify.notify_one();
    }

    /// Timer thread main loop
    fn run_loop(&self, injector: Arc<Injector<Arc<Task>>>) {
        loop {
            // Check shutdown
            if self.shutdown.load(AtomicOrdering::Acquire) {
                break;
            }

            let mut state = self.state.lock();

            // Re-check shutdown after acquiring lock to close race window:
            // stop() may set shutdown + notify_one between our first check
            // and acquiring the lock, causing the notification to be lost.
            if self.shutdown.load(AtomicOrdering::Acquire) {
                break;
            }

            // Process all tasks that should wake up now
            let now = Instant::now();
            while let Some(entry) = state.sleeping.peek() {
                if entry.wake_at <= now {
                    let entry = state.sleeping.pop().unwrap();

                    // Only wake if task is still suspended (not cancelled)
                    if entry.task.state() == TaskState::Suspended {
                        entry.task.set_state(TaskState::Resumed);
                        entry.task.clear_suspend_reason();
                        injector.push(entry.task);
                    }
                } else {
                    break;
                }
            }

            // Calculate wait time
            if let Some(next) = state.sleeping.peek() {
                let now = Instant::now();
                if next.wake_at > now {
                    let timeout = next.wake_at - now;
                    // Wait with timeout - will wake early if new entry added
                    self.notify.wait_for(&mut state, timeout);
                }
                // Loop again to process
            } else {
                // No sleeping tasks, wait indefinitely for new registrations
                self.notify.wait(&mut state);
            }
        }

        #[cfg(debug_assertions)]
        eprintln!("Timer thread shutting down");
    }

    /// Get the number of sleeping tasks (for debugging/stats)
    pub fn sleeping_count(&self) -> usize {
        self.state.lock().sleeping.len()
    }
}

impl Default for TimerThread {
    fn default() -> Self {
        Self {
            state: Mutex::new(TimerState {
                sleeping: BinaryHeap::new(),
            }),
            notify: Condvar::new(),
            shutdown: AtomicBool::new(false),
            handle: Mutex::new(None),
        }
    }
}

impl Drop for TimerThread {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::{Function, Module, Opcode};
    use std::time::Duration;

    fn create_test_task(id: u64) -> Arc<Task> {
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: format!("task_{}", id),
            param_count: 0,
            local_count: 0,
            code: vec![Opcode::ConstI32 as u8, 42, 0, 0, 0, Opcode::Return as u8],
        });

        Arc::new(Task::new(0, Arc::new(module), None))
    }

    #[test]
    fn test_timer_creation() {
        let timer = TimerThread::new();
        assert_eq!(timer.sleeping_count(), 0);
    }

    #[test]
    fn test_timer_register() {
        let timer = TimerThread::new();
        let task = create_test_task(1);
        task.set_state(TaskState::Suspended);

        let wake_at = Instant::now() + Duration::from_millis(100);
        timer.register(task.clone(), wake_at);

        assert_eq!(timer.sleeping_count(), 1);
    }

    #[test]
    fn test_timer_wakes_task() {
        let timer = TimerThread::new();
        let injector = Arc::new(Injector::new());

        timer.start(injector.clone());

        // Create and register a task
        let task = create_test_task(1);
        task.set_state(TaskState::Suspended);

        let wake_at = Instant::now() + Duration::from_millis(50);
        timer.register(task.clone(), wake_at);

        // Wait for wake
        thread::sleep(Duration::from_millis(100));

        // Task should be in injector
        assert!(!injector.is_empty());

        // Task should be resumed
        assert_eq!(task.state(), TaskState::Resumed);

        timer.stop();
    }

    #[test]
    fn test_timer_multiple_tasks() {
        let timer = TimerThread::new();
        let injector = Arc::new(Injector::new());

        timer.start(injector.clone());

        // Create tasks with different wake times
        let task1 = create_test_task(1);
        let task2 = create_test_task(2);
        let task3 = create_test_task(3);

        task1.set_state(TaskState::Suspended);
        task2.set_state(TaskState::Suspended);
        task3.set_state(TaskState::Suspended);

        let now = Instant::now();
        timer.register(task3.clone(), now + Duration::from_millis(150));
        timer.register(task1.clone(), now + Duration::from_millis(50));
        timer.register(task2.clone(), now + Duration::from_millis(100));

        // Wait for all to wake
        thread::sleep(Duration::from_millis(200));

        // All tasks should be resumed
        assert_eq!(task1.state(), TaskState::Resumed);
        assert_eq!(task2.state(), TaskState::Resumed);
        assert_eq!(task3.state(), TaskState::Resumed);

        timer.stop();
    }

    #[test]
    fn test_timer_shutdown() {
        let timer = TimerThread::new();
        let injector = Arc::new(Injector::new());

        timer.start(injector);

        // Register a task that won't wake before shutdown
        let task = create_test_task(1);
        task.set_state(TaskState::Suspended);
        timer.register(task, Instant::now() + Duration::from_secs(60));

        // Should be able to stop cleanly
        timer.stop();
    }
}
