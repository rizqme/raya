//! VM Snapshotting - Stop-The-World Pause & Resume
//!
//! This module implements safe pause, snapshot, transfer, and resume semantics
//! for the Raya Virtual Machine.

pub mod format;
mod heap;
mod reader;
mod task;
mod writer;

pub use format::{SegmentType, SnapshotChecksum, SnapshotError, SnapshotHeader};
pub use heap::{HeapSnapshot, ObjectId};
pub use reader::SnapshotReader;
pub use task::{BlockedReason, SerializedFrame, SerializedTask};
pub use writer::SnapshotWriter;
