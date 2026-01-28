//! Snapshot reader - restores VM state from snapshot

use crate::vm::snapshot::format::{
    SegmentHeader, SegmentType, SnapshotChecksum, SnapshotError, SnapshotHeader,
};
use crate::vm::snapshot::heap::HeapSnapshot;
use crate::vm::snapshot::task::SerializedTask;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// Snapshot reader - restores VM state from snapshot
pub struct SnapshotReader {
    header: SnapshotHeader,
    tasks: Vec<SerializedTask>,
    heap: HeapSnapshot,
    needs_byte_swap: bool,
}

impl SnapshotReader {
    /// Load snapshot from file
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, SnapshotError> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        Self::from_reader(&mut reader)
    }

    /// Load snapshot from reader
    pub fn from_reader(reader: &mut impl Read) -> Result<Self, SnapshotError> {
        use crate::vm::snapshot::format::byteswap;

        // Read and validate header
        let header = SnapshotHeader::decode(reader)?;
        let needs_byte_swap = header.validate()?;

        // Read segment count
        let mut buf = [0u8; 4];
        reader.read_exact(&mut buf)?;
        let segment_count = byteswap::swap_u32(u32::from_le_bytes(buf), needs_byte_swap) as usize;

        // Read all segments and collect complete segment data for checksum
        let mut segments = Vec::new();
        let mut all_segment_bytes = Vec::new();

        for _ in 0..segment_count {
            let seg_header = SegmentHeader::decode(reader)?;

            let segment_type = match seg_header.segment_type {
                1 => SegmentType::Metadata,
                2 => SegmentType::Heap,
                3 => SegmentType::Task,
                4 => SegmentType::Scheduler,
                5 => SegmentType::Sync,
                _ => return Err(SnapshotError::CorruptedData),
            };

            // Re-encode the segment header for checksum
            seg_header.encode(&mut all_segment_bytes)?;

            // Read segment data
            let mut data = vec![0u8; seg_header.length as usize];
            reader.read_exact(&mut data)?;

            // Add data to checksum bytes
            all_segment_bytes.extend_from_slice(&data);

            segments.push((segment_type, data));
        }

        // Verify checksum
        let checksum = SnapshotChecksum::decode(reader)?;

        if !checksum.verify(&all_segment_bytes) {
            return Err(SnapshotError::ChecksumMismatch);
        }

        // Parse segments
        let mut tasks = Vec::new();
        let mut heap = HeapSnapshot::empty();

        for (segment_type, data) in &segments {
            match segment_type {
                SegmentType::Metadata => {
                    // Skip metadata for now
                }
                SegmentType::Heap => {
                    heap = HeapSnapshot::decode(&mut &data[..], needs_byte_swap)?;
                }
                SegmentType::Task => {
                    tasks = Self::parse_task_segment(data, needs_byte_swap)?;
                }
                SegmentType::Scheduler => {
                    // Skip scheduler for now
                }
                SegmentType::Sync => {
                    // Skip sync for now
                }
            }
        }

        Ok(Self {
            header,
            tasks,
            heap,
            needs_byte_swap,
        })
    }

    fn parse_task_segment(
        data: &[u8],
        needs_byte_swap: bool,
    ) -> Result<Vec<SerializedTask>, SnapshotError> {
        use crate::vm::snapshot::format::byteswap;

        let mut reader = data;
        let mut buf = [0u8; 8];

        // Read task count
        reader.read_exact(&mut buf)?;
        let task_count = byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap) as usize;

        // Read tasks
        let mut tasks = Vec::with_capacity(task_count);
        for _ in 0..task_count {
            tasks.push(SerializedTask::decode(&mut reader, needs_byte_swap)?);
        }

        Ok(tasks)
    }

    /// Get the snapshot header
    pub fn header(&self) -> &SnapshotHeader {
        &self.header
    }

    /// Get the tasks
    pub fn tasks(&self) -> &[SerializedTask] {
        &self.tasks
    }

    /// Get the heap snapshot
    pub fn heap(&self) -> &HeapSnapshot {
        &self.heap
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::scheduler::TaskId;
    use crate::vm::snapshot::writer::SnapshotWriter;

    #[test]
    fn test_snapshot_round_trip() {
        // Create a snapshot
        let mut writer = SnapshotWriter::new();
        let task = SerializedTask::new(TaskId::from_u64(42), 10);
        writer.add_task(task);

        let mut buf = Vec::new();
        writer.write_snapshot(&mut buf).unwrap();

        // Read it back
        let reader = SnapshotReader::from_reader(&mut &buf[..]).unwrap();
        assert_eq!(reader.tasks().len(), 1);
        assert_eq!(reader.tasks()[0].task_id.as_u64(), 42);
        assert_eq!(reader.tasks()[0].function_index, 10);
    }

    #[test]
    fn test_invalid_magic() {
        let mut buf = vec![0u8; 100];
        // Wrong magic number
        buf[0..8].copy_from_slice(&0u64.to_le_bytes());

        let result = SnapshotReader::from_reader(&mut &buf[..]);
        assert!(matches!(result, Err(SnapshotError::InvalidMagic)));
    }

    #[test]
    fn test_checksum_mismatch() {
        // Create a valid snapshot
        let writer = SnapshotWriter::new();
        let mut buf = Vec::new();
        writer.write_snapshot(&mut buf).unwrap();

        // Corrupt the checksum itself (last 32 bytes)
        if buf.len() > 32 {
            let len = buf.len();
            buf[len - 1] ^= 0xFF; // Flip bits in the checksum
        }

        // Should fail checksum validation
        let result = SnapshotReader::from_reader(&mut &buf[..]);
        assert!(matches!(result, Err(SnapshotError::ChecksumMismatch)));
    }
}
