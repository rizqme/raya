//! Thread-safe handle registry for stateful resources
//!
//! Provides numeric handles for resources like sockets, HTTP connections,
//! and process results. Used by net, http, fetch, and process modules.

use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Thread-safe registry mapping numeric handles to values.
///
/// Handles are auto-incrementing u64 IDs. Thread-safe for concurrent access
/// from multiple goroutines.
pub struct HandleRegistry<T> {
    map: DashMap<u64, T>,
    next_id: AtomicU64,
}

impl<T> HandleRegistry<T> {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            map: DashMap::new(),
            next_id: AtomicU64::new(1),
        }
    }

    /// Insert a value and return its handle.
    pub fn insert(&self, value: T) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.map.insert(id, value);
        id
    }

    /// Get a reference to a value by handle.
    pub fn get(&self, id: u64) -> Option<dashmap::mapref::one::Ref<'_, u64, T>> {
        self.map.get(&id)
    }

    /// Get a mutable reference to a value by handle.
    pub fn get_mut(&self, id: u64) -> Option<dashmap::mapref::one::RefMut<'_, u64, T>> {
        self.map.get_mut(&id)
    }

    /// Remove a value by handle, returning it.
    pub fn remove(&self, id: u64) -> Option<(u64, T)> {
        self.map.remove(&id)
    }
}

impl<T> Default for HandleRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}
