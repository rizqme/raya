//! Global registries for task-aware synchronization primitives

use crate::vm::sync::{Mutex, MutexId, Semaphore, SemaphoreId};
use dashmap::DashMap;
use std::sync::Arc;

/// Global registry of all mutexes
///
/// This registry allows looking up mutexes by ID from anywhere in the VM.
/// It's used by the LOCK/UNLOCK opcodes to find the mutex to operate on.
pub struct MutexRegistry {
    /// Map of mutex ID to mutex instance
    mutexes: DashMap<MutexId, Arc<Mutex>>,
}

impl MutexRegistry {
    /// Create a new empty mutex registry
    pub fn new() -> Self {
        Self {
            mutexes: DashMap::new(),
        }
    }

    /// Create a new mutex and register it
    ///
    /// Returns the mutex ID and a reference to the mutex
    pub fn create_mutex(&self) -> (MutexId, Arc<Mutex>) {
        let id = MutexId::new();
        let mutex = Arc::new(Mutex::new(id));
        self.mutexes.insert(id, mutex.clone());
        (id, mutex)
    }

    /// Get a mutex by ID
    pub fn get(&self, id: MutexId) -> Option<Arc<Mutex>> {
        self.mutexes.get(&id).map(|entry| entry.clone())
    }

    /// Remove a mutex from the registry (when dropped)
    pub fn remove(&self, id: MutexId) -> Option<Arc<Mutex>> {
        self.mutexes.remove(&id).map(|(_, mutex)| mutex)
    }

    /// Get the number of registered mutexes
    pub fn count(&self) -> usize {
        self.mutexes.len()
    }

    /// Clear all mutexes (for shutdown)
    pub fn clear(&self) {
        self.mutexes.clear();
    }

    /// Get all mutex IDs (for snapshotting)
    pub fn all_ids(&self) -> Vec<MutexId> {
        self.mutexes.iter().map(|entry| *entry.key()).collect()
    }

    /// Register an existing mutex (for snapshot restoration)
    pub fn register(&self, mutex: Arc<Mutex>) {
        self.mutexes.insert(mutex.id(), mutex);
    }
}

impl Default for MutexRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global registry of all semaphores.
pub struct SemaphoreRegistry {
    semaphores: DashMap<SemaphoreId, Arc<Semaphore>>,
}

impl SemaphoreRegistry {
    /// Create a new empty semaphore registry.
    pub fn new() -> Self {
        Self {
            semaphores: DashMap::new(),
        }
    }

    /// Create a new semaphore and register it.
    pub fn create_semaphore(&self, permits: usize) -> (SemaphoreId, Arc<Semaphore>) {
        let id = SemaphoreId::new();
        let semaphore = Arc::new(Semaphore::new(id, permits));
        self.semaphores.insert(id, semaphore.clone());
        (id, semaphore)
    }

    /// Get a semaphore by ID.
    pub fn get(&self, id: SemaphoreId) -> Option<Arc<Semaphore>> {
        self.semaphores.get(&id).map(|entry| entry.clone())
    }

    /// Remove a semaphore from the registry.
    pub fn remove(&self, id: SemaphoreId) -> Option<Arc<Semaphore>> {
        self.semaphores.remove(&id).map(|(_, semaphore)| semaphore)
    }

    /// Get the number of registered semaphores.
    pub fn count(&self) -> usize {
        self.semaphores.len()
    }

    /// Clear all semaphores.
    pub fn clear(&self) {
        self.semaphores.clear();
    }

    /// Register an existing semaphore (for restoration paths).
    pub fn register(&self, semaphore: Arc<Semaphore>) {
        self.semaphores.insert(semaphore.id(), semaphore);
    }
}

impl Default for SemaphoreRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = MutexRegistry::new();
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_registry_create_mutex() {
        let registry = MutexRegistry::new();

        let (id, mutex) = registry.create_mutex();
        assert_eq!(mutex.id(), id);
        assert_eq!(registry.count(), 1);

        // Should be able to retrieve it
        let retrieved = registry.get(id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id(), id);
    }

    #[test]
    fn test_registry_multiple_mutexes() {
        let registry = MutexRegistry::new();

        let (id1, mutex1) = registry.create_mutex();
        let (id2, mutex2) = registry.create_mutex();

        assert_ne!(id1, id2);
        assert_eq!(registry.count(), 2);

        assert_eq!(registry.get(id1).unwrap().id(), id1);
        assert_eq!(registry.get(id2).unwrap().id(), id2);
    }

    #[test]
    fn test_registry_remove() {
        let registry = MutexRegistry::new();

        let (id, _) = registry.create_mutex();
        assert_eq!(registry.count(), 1);

        let removed = registry.remove(id);
        assert!(removed.is_some());
        assert_eq!(registry.count(), 0);

        // Should not be able to retrieve after removal
        assert!(registry.get(id).is_none());
    }

    #[test]
    fn test_registry_clear() {
        let registry = MutexRegistry::new();

        registry.create_mutex();
        registry.create_mutex();
        registry.create_mutex();

        assert_eq!(registry.count(), 3);

        registry.clear();
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_registry_all_ids() {
        let registry = MutexRegistry::new();

        let (id1, _) = registry.create_mutex();
        let (id2, _) = registry.create_mutex();
        let (id3, _) = registry.create_mutex();

        let all_ids = registry.all_ids();
        assert_eq!(all_ids.len(), 3);
        assert!(all_ids.contains(&id1));
        assert!(all_ids.contains(&id2));
        assert!(all_ids.contains(&id3));
    }

    #[test]
    fn test_registry_register_existing() {
        let registry = MutexRegistry::new();

        let id = MutexId::new();
        let mutex = Arc::new(Mutex::new(id));

        registry.register(mutex.clone());

        assert_eq!(registry.count(), 1);
        assert_eq!(registry.get(id).unwrap().id(), id);
    }
}
