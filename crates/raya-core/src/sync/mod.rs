//! Synchronization primitives for Task coordination
//!
//! This module provides goroutine-style synchronization primitives that block
//! at the Task level instead of the OS thread level, allowing efficient
//! multi-Task concurrency.

mod guard;
mod mutex;
mod mutex_id;
mod registry;
mod serialize;

pub use guard::{MutexGuard, OwnedMutexGuard};
pub use mutex::{BlockReason, Mutex, MutexError};
pub use mutex_id::MutexId;
pub use registry::MutexRegistry;
pub use serialize::SerializedMutex;
