//! Heap serialization for snapshots

use std::io::{Read, Write};

/// Stable object ID for snapshot serialization
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct ObjectId(u64);

impl ObjectId {
    /// Create a new object ID
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get the object ID as a u64
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// Heap snapshot containing all allocated objects
///
/// Note: This is a simplified implementation for Milestone 1.11.
/// Full heap serialization will be implemented when GC is complete.
#[derive(Debug)]
pub struct HeapSnapshot {
    /// Object count
    object_count: u64,

    /// Serialized object data
    data: Vec<u8>,
}

impl HeapSnapshot {
    /// Create a new empty heap snapshot
    pub fn new() -> Self {
        Self {
            object_count: 0,
            data: Vec::new(),
        }
    }

    /// Create an empty heap snapshot
    pub fn empty() -> Self {
        Self::new()
    }

    /// Encode heap snapshot to writer
    pub fn encode(&self, writer: &mut impl Write) -> std::io::Result<()> {
        // Write object count
        writer.write_all(&self.object_count.to_le_bytes())?;

        // Write data length
        writer.write_all(&(self.data.len() as u64).to_le_bytes())?;

        // Write data
        writer.write_all(&self.data)?;

        Ok(())
    }

    /// Decode heap snapshot from reader
    pub fn decode(reader: &mut impl Read) -> std::io::Result<Self> {
        let mut buf = [0u8; 8];

        // Read object count
        reader.read_exact(&mut buf)?;
        let object_count = u64::from_le_bytes(buf);

        // Read data length
        reader.read_exact(&mut buf)?;
        let data_len = u64::from_le_bytes(buf) as usize;

        // Read data
        let mut data = vec![0u8; data_len];
        reader.read_exact(&mut data)?;

        Ok(Self { object_count, data })
    }
}

impl Default for HeapSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_id() {
        let id = ObjectId::new(42);
        assert_eq!(id.as_u64(), 42);
    }

    #[test]
    fn test_empty_heap_snapshot() {
        let snapshot = HeapSnapshot::empty();
        let mut buf = Vec::new();
        snapshot.encode(&mut buf).unwrap();

        let decoded = HeapSnapshot::decode(&mut &buf[..]).unwrap();
        assert_eq!(decoded.object_count, 0);
        assert_eq!(decoded.data.len(), 0);
    }

    #[test]
    fn test_heap_snapshot_round_trip() {
        let snapshot = HeapSnapshot {
            object_count: 5,
            data: vec![1, 2, 3, 4, 5],
        };

        let mut buf = Vec::new();
        snapshot.encode(&mut buf).unwrap();

        let decoded = HeapSnapshot::decode(&mut &buf[..]).unwrap();
        assert_eq!(decoded.object_count, 5);
        assert_eq!(decoded.data, vec![1, 2, 3, 4, 5]);
    }
}
