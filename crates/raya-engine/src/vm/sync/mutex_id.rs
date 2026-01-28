//! Unique identifier for mutexes

use std::sync::atomic::{AtomicU64, Ordering};

/// Unique identifier for a Mutex
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct MutexId(u64);

static NEXT_MUTEX_ID: AtomicU64 = AtomicU64::new(1);

impl MutexId {
    /// Generate a new unique MutexId
    pub fn new() -> Self {
        MutexId(NEXT_MUTEX_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Get the numeric ID value
    pub fn as_u64(self) -> u64 {
        self.0
    }

    /// Create a MutexId from a u64 value (for deserialization)
    pub fn from_u64(id: u64) -> Self {
        MutexId(id)
    }
}

impl Default for MutexId {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mutex_id_uniqueness() {
        let id1 = MutexId::new();
        let id2 = MutexId::new();
        assert_ne!(id1, id2);
        assert!(id2.as_u64() > id1.as_u64());
    }

    #[test]
    fn test_mutex_id_from_u64() {
        let id = MutexId::from_u64(42);
        assert_eq!(id.as_u64(), 42);
    }

    #[test]
    fn test_mutex_id_default() {
        let id = MutexId::default();
        assert!(id.as_u64() > 0);
    }
}
