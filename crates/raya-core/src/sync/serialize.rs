//! Mutex serialization for VM snapshotting

use crate::scheduler::TaskId;
use crate::sync::MutexId;
use std::io::{Read, Write};

/// Serialized form of a Mutex for snapshotting
#[derive(Debug, Clone)]
pub struct SerializedMutex {
    /// Mutex ID
    pub mutex_id: MutexId,
    /// Current owner (None if unlocked)
    pub owner: Option<TaskId>,
    /// FIFO wait queue of blocked Tasks
    pub wait_queue: Vec<TaskId>,
}

impl SerializedMutex {
    /// Create a new serialized mutex
    pub fn new(mutex_id: MutexId) -> Self {
        Self {
            mutex_id,
            owner: None,
            wait_queue: Vec::new(),
        }
    }

    /// Encode serialized mutex to binary format
    pub fn encode<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        // Write mutex ID (8 bytes)
        writer.write_all(&self.mutex_id.as_u64().to_le_bytes())?;

        // Write owner (1 byte presence + 8 bytes if present)
        if let Some(owner) = self.owner {
            writer.write_all(&[1u8])?;
            writer.write_all(&owner.as_u64().to_le_bytes())?;
        } else {
            writer.write_all(&[0u8])?;
        }

        // Write wait queue length (4 bytes)
        writer.write_all(&(self.wait_queue.len() as u32).to_le_bytes())?;

        // Write each waiting task ID (8 bytes each)
        for task_id in &self.wait_queue {
            writer.write_all(&task_id.as_u64().to_le_bytes())?;
        }

        Ok(())
    }

    /// Decode serialized mutex from binary format
    pub fn decode<R: Read>(reader: &mut R) -> std::io::Result<Self> {
        // Read mutex ID (8 bytes)
        let mut buf = [0u8; 8];
        reader.read_exact(&mut buf)?;
        let mutex_id = MutexId::from_u64(u64::from_le_bytes(buf));

        // Read owner (1 byte presence + 8 bytes if present)
        let mut presence = [0u8; 1];
        reader.read_exact(&mut presence)?;
        let owner = if presence[0] == 1 {
            reader.read_exact(&mut buf)?;
            Some(TaskId::from_u64(u64::from_le_bytes(buf)))
        } else {
            None
        };

        // Read wait queue length (4 bytes)
        let mut len_buf = [0u8; 4];
        reader.read_exact(&mut len_buf)?;
        let queue_len = u32::from_le_bytes(len_buf) as usize;

        // Read each waiting task ID (8 bytes each)
        let mut wait_queue = Vec::with_capacity(queue_len);
        for _ in 0..queue_len {
            reader.read_exact(&mut buf)?;
            wait_queue.push(TaskId::from_u64(u64::from_le_bytes(buf)));
        }

        Ok(Self {
            mutex_id,
            owner,
            wait_queue,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_serialized_mutex_encode_decode() {
        let id = MutexId::new();
        let owner = Some(TaskId::new());
        let wait_queue = vec![TaskId::new(), TaskId::new(), TaskId::new()];

        let serialized = SerializedMutex {
            mutex_id: id,
            owner,
            wait_queue: wait_queue.clone(),
        };

        let mut buf = Vec::new();
        serialized.encode(&mut buf).unwrap();

        let decoded = SerializedMutex::decode(&mut Cursor::new(&buf)).unwrap();

        assert_eq!(decoded.mutex_id, id);
        assert_eq!(decoded.owner, owner);
        assert_eq!(decoded.wait_queue.len(), 3);
        for (original, decoded) in wait_queue.iter().zip(decoded.wait_queue.iter()) {
            assert_eq!(original, decoded);
        }
    }

    #[test]
    fn test_serialized_mutex_unlocked() {
        let id = MutexId::new();

        let serialized = SerializedMutex {
            mutex_id: id,
            owner: None,
            wait_queue: Vec::new(),
        };

        let mut buf = Vec::new();
        serialized.encode(&mut buf).unwrap();

        let decoded = SerializedMutex::decode(&mut Cursor::new(&buf)).unwrap();

        assert_eq!(decoded.mutex_id, id);
        assert!(decoded.owner.is_none());
        assert_eq!(decoded.wait_queue.len(), 0);
    }

    #[test]
    fn test_mutex_serialize_deserialize() {
        use crate::sync::Mutex;

        let id = MutexId::new();
        let mutex = Mutex::new(id);
        let task1 = TaskId::new();
        let task2 = TaskId::new();

        // Lock the mutex
        mutex.try_lock(task1).unwrap();

        // Try to lock with another task (will be queued)
        let _ = mutex.try_lock(task2);

        // Serialize
        let serialized = mutex.serialize();
        assert_eq!(serialized.mutex_id, id);
        assert_eq!(serialized.owner, Some(task1));
        assert_eq!(serialized.wait_queue.len(), 1);
        assert_eq!(serialized.wait_queue[0], task2);

        // Deserialize
        let restored = Mutex::deserialize(serialized);
        assert_eq!(restored.id(), id);
        assert_eq!(restored.owner(), Some(task1));
        assert!(restored.is_locked());
        assert_eq!(restored.waiting_count(), 1);
    }
}
