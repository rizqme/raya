//! Task Scheduler - Work-Stealing Concurrency
//!
//! This module implements the goroutine-style work-stealing task scheduler
//! for Raya's async/await concurrency model with Go-style asynchronous preemption.

mod deque;
mod preempt;
#[allow(clippy::module_inception)]
mod scheduler;
mod task;
mod timer;
mod worker;

pub use deque::WorkerDeque;
pub use preempt::{PreemptMonitor, DEFAULT_PREEMPT_THRESHOLD};
pub use scheduler::{Scheduler, SchedulerLimits, SchedulerStats};
pub use task::{ExceptionHandler, SuspendReason, Task, TaskHandle, TaskId, TaskState};
pub use timer::TimerThread;
pub use worker::Worker;
