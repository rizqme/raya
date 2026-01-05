//! Snapshot writer - captures full VM state

use crate::snapshot::format::{
    SegmentHeader, SegmentType, SnapshotChecksum, SnapshotError, SnapshotHeader,
};
use crate::snapshot::heap::HeapSnapshot;
use crate::snapshot::task::SerializedTask;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

/// Snapshot writer - captures full VM state
///
/// Note: This is a simplified implementation for Milestone 1.11.
/// Full VM integration will be added when the VM is complete.
pub struct SnapshotWriter {
    /// Tasks to snapshot
    tasks: Vec<SerializedTask>,

    /// Heap snapshot
    heap: HeapSnapshot,
}

impl SnapshotWriter {
    /// Create a new snapshot writer
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            heap: HeapSnapshot::empty(),
        }
    }

    /// Add a task to the snapshot
    pub fn add_task(&mut self, task: SerializedTask) {
        self.tasks.push(task);
    }

    /// Set the heap snapshot
    pub fn set_heap(&mut self, heap: HeapSnapshot) {
        self.heap = heap;
    }

    /// Write snapshot to file
    pub fn write_to_file(&self, path: impl AsRef<Path>) -> Result<(), SnapshotError> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        self.write_snapshot(&mut writer)?;
        Ok(())
    }

    /// Write snapshot to writer
    pub fn write_snapshot(&self, writer: &mut impl Write) -> Result<(), SnapshotError> {
        // Write header
        let header = SnapshotHeader::new();
        header.encode(writer)?;

        // Write segment count (always 5 segments for now)
        writer.write_all(&5u32.to_le_bytes())?;

        // Collect all segment data
        let mut segment_data = Vec::new();

        // 1. Metadata segment (empty for now)
        self.write_metadata_segment(&mut segment_data)?;

        // 2. Heap segment
        self.write_heap_segment(&mut segment_data)?;

        // 3. Task segment
        self.write_task_segment(&mut segment_data)?;

        // 4. Scheduler segment (empty for now)
        self.write_scheduler_segment(&mut segment_data)?;

        // 5. Sync segment (empty for now)
        self.write_sync_segment(&mut segment_data)?;

        // Write segment data
        writer.write_all(&segment_data)?;

        // Calculate and write checksum
        let checksum = SnapshotChecksum::compute(&segment_data);
        checksum.encode(writer)?;

        Ok(())
    }

    fn write_metadata_segment(&self, writer: &mut Vec<u8>) -> Result<(), SnapshotError> {
        let mut segment_data = Vec::new();

        // Empty metadata for now
        segment_data.write_all(&0u64.to_le_bytes())?; // Module count
        segment_data.write_all(&0u64.to_le_bytes())?; // Function count

        // Write segment header
        let header = SegmentHeader::new(SegmentType::Metadata, segment_data.len() as u64);
        header.encode(writer)?;
        writer.write_all(&segment_data)?;

        Ok(())
    }

    fn write_heap_segment(&self, writer: &mut Vec<u8>) -> Result<(), SnapshotError> {
        let mut segment_data = Vec::new();
        self.heap.encode(&mut segment_data)?;

        let header = SegmentHeader::new(SegmentType::Heap, segment_data.len() as u64);
        header.encode(writer)?;
        writer.write_all(&segment_data)?;

        Ok(())
    }

    fn write_task_segment(&self, writer: &mut Vec<u8>) -> Result<(), SnapshotError> {
        let mut segment_data = Vec::new();

        // Write task count
        segment_data.write_all(&(self.tasks.len() as u64).to_le_bytes())?;

        // Write tasks
        for task in &self.tasks {
            task.encode(&mut segment_data)?;
        }

        let header = SegmentHeader::new(SegmentType::Task, segment_data.len() as u64);
        header.encode(writer)?;
        writer.write_all(&segment_data)?;

        Ok(())
    }

    fn write_scheduler_segment(&self, writer: &mut Vec<u8>) -> Result<(), SnapshotError> {
        let mut segment_data = Vec::new();

        // Empty scheduler state for now
        segment_data.write_all(&0u64.to_le_bytes())?; // Ready queue count

        let header = SegmentHeader::new(SegmentType::Scheduler, segment_data.len() as u64);
        header.encode(writer)?;
        writer.write_all(&segment_data)?;

        Ok(())
    }

    fn write_sync_segment(&self, writer: &mut Vec<u8>) -> Result<(), SnapshotError> {
        let mut segment_data = Vec::new();

        // Empty sync state for now
        segment_data.write_all(&0u64.to_le_bytes())?; // Mutex count

        let header = SegmentHeader::new(SegmentType::Sync, segment_data.len() as u64);
        header.encode(writer)?;
        writer.write_all(&segment_data)?;

        Ok(())
    }
}

impl Default for SnapshotWriter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::TaskId;

    #[test]
    fn test_empty_snapshot() {
        let writer = SnapshotWriter::new();
        let mut buf = Vec::new();
        writer.write_snapshot(&mut buf).unwrap();

        // Should have header + segments + checksum
        assert!(buf.len() > 100);
    }

    #[test]
    fn test_snapshot_with_tasks() {
        let mut writer = SnapshotWriter::new();

        let task1 = SerializedTask::new(TaskId::from_u64(1), 0);
        let task2 = SerializedTask::new(TaskId::from_u64(2), 1);

        writer.add_task(task1);
        writer.add_task(task2);

        let mut buf = Vec::new();
        writer.write_snapshot(&mut buf).unwrap();

        assert!(buf.len() > 100);
    }
}
