//! VM Snapshotting - Stop-The-World Pause & Resume
//!
//! This module implements safe pause, snapshot, transfer, and resume semantics
//! for the Raya Virtual Machine.

pub mod format;
mod heap;
mod reader;
mod task;
mod writer;

pub use format::{
    is_big_endian, is_little_endian, needs_byte_swap, SegmentType, SnapshotChecksum, SnapshotError,
    SnapshotHeader, ENDIANNESS_MARKER, ENDIANNESS_MARKER_SWAPPED,
};
pub use heap::{HeapSnapshot, ObjectId};
pub use reader::SnapshotReader;
pub use task::{BlockedReason, SerializedFrame, SerializedTask};
pub use writer::SnapshotWriter;
