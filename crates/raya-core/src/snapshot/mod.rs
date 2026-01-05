//! VM Snapshotting - Stop-The-World Pause & Resume
//!
//! This module implements safe pause, snapshot, transfer, and resume semantics
//! for the Raya Virtual Machine.

pub mod format;
mod heap;
mod task;
mod writer;
mod reader;

pub use format::{SnapshotHeader, SegmentType, SnapshotChecksum, SnapshotError};
pub use heap::{HeapSnapshot, ObjectId};
pub use task::{SerializedTask, SerializedFrame, BlockedReason};
pub use writer::SnapshotWriter;
pub use reader::SnapshotReader;
