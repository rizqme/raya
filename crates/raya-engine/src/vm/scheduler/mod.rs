//! Task Scheduler — Unified Reactor Architecture
//!
//! Single reactor thread handles scheduling + event loop. Two worker pools
//! (VM workers for task execution, IO workers for blocking work) do the actual work.

mod pool;
mod reactor;
#[allow(clippy::module_inception)]
mod scheduler;
mod task;

pub use pool::StackPool;
pub use reactor::{IoSubmission, Reactor};
pub use scheduler::{Scheduler, SchedulerLimits, SchedulerStats};
pub use task::{ExceptionHandler, SuspendReason, Task, TaskHandle, TaskId, TaskState};
