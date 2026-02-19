//! Code cache for JIT-compiled functions
//!
//! Stores compiled native code indexed by (module_id, function_index), with
//! support for invalidation (when a function is recompiled or patched).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use crate::jit::backend::traits::ExecutableCode;
use crate::jit::runtime::trampoline::JitEntryFn;

/// Composite key for the code cache: (module_id, func_index)
///
/// module_id disambiguates functions across different modules that may share
/// the same function index. Assigned by the cache via an atomic counter.
type CacheKey = (u64, u32);

/// Entry in the code cache
pub struct CacheEntry {
    /// The compiled executable code
    pub code: ExecutableCode,
    /// Whether this entry has been invalidated (e.g., function was recompiled)
    pub invalidated: AtomicBool,
}

/// Thread-safe cache of JIT-compiled function code
pub struct CodeCache {
    /// (module_id, func_index) → compiled code
    entries: RwLock<FxHashMap<CacheKey, CacheEntry>>,
    /// Total size of all cached code
    total_code_size: AtomicUsize,
    /// Maximum allowed total code size
    max_size: usize,
    /// Counter for assigning unique module IDs
    next_module_id: AtomicUsize,
    /// Module checksum → module_id mapping (for interpreter lookups)
    module_ids: RwLock<FxHashMap<[u8; 32], u64>>,
}

impl CodeCache {
    /// Create a new code cache with a maximum size limit (in bytes)
    pub fn new(max_size: usize) -> Self {
        CodeCache {
            entries: RwLock::new(FxHashMap::default()),
            total_code_size: AtomicUsize::new(0),
            max_size,
            next_module_id: AtomicUsize::new(0),
            module_ids: RwLock::new(FxHashMap::default()),
        }
    }

    /// Allocate a unique module ID for use as the first part of the cache key
    pub fn allocate_module_id(&self) -> u64 {
        self.next_module_id.fetch_add(1, Ordering::Relaxed) as u64
    }

    /// Register a module by checksum and return its assigned module_id.
    ///
    /// If the module was already registered, returns the existing module_id.
    pub fn register_module(&self, checksum: [u8; 32]) -> u64 {
        let ids = self.module_ids.read();
        if let Some(&id) = ids.get(&checksum) {
            return id;
        }
        drop(ids);

        let id = self.allocate_module_id();
        self.module_ids.write().insert(checksum, id);
        id
    }

    /// Look up the module_id for a given module checksum.
    ///
    /// Returns None if the module has not been JIT-compiled.
    pub fn module_id(&self, checksum: &[u8; 32]) -> Option<u64> {
        self.module_ids.read().get(checksum).copied()
    }

    /// Insert compiled code for a function
    ///
    /// Returns false if the cache is full and the entry was not inserted.
    pub fn insert(&self, module_id: u64, func_index: u32, code: ExecutableCode) -> bool {
        let key = (module_id, func_index);
        let code_size = code.code_size;
        let current = self.total_code_size.load(Ordering::Relaxed);
        if current + code_size > self.max_size {
            return false;
        }

        let mut entries = self.entries.write();
        // Remove old entry size if replacing
        if let Some(old) = entries.remove(&key) {
            self.total_code_size.fetch_sub(old.code.code_size, Ordering::Relaxed);
        }

        self.total_code_size.fetch_add(code_size, Ordering::Relaxed);
        entries.insert(key, CacheEntry {
            code,
            invalidated: AtomicBool::new(false),
        });
        true
    }

    /// Look up the JIT entry function for a (module_id, func_index) pair
    ///
    /// Returns None if the function isn't compiled or has been invalidated.
    pub fn get(&self, module_id: u64, func_index: u32) -> Option<JitEntryFn> {
        let key = (module_id, func_index);
        let entries = self.entries.read();
        let entry = entries.get(&key)?;
        if entry.invalidated.load(Ordering::Acquire) {
            return None;
        }
        // Safety: entry_offset is within code bounds (verified at finalize time)
        let fn_ptr = unsafe { entry.code.code_ptr.add(entry.code.entry_offset) };
        Some(unsafe { std::mem::transmute(fn_ptr) })
    }

    /// Invalidate a cached function (e.g., when deoptimizing)
    pub fn invalidate(&self, module_id: u64, func_index: u32) {
        let key = (module_id, func_index);
        let entries = self.entries.read();
        if let Some(entry) = entries.get(&key) {
            entry.invalidated.store(true, Ordering::Release);
        }
    }

    /// Check if a function has been compiled and is valid
    pub fn contains(&self, module_id: u64, func_index: u32) -> bool {
        let key = (module_id, func_index);
        let entries = self.entries.read();
        entries.get(&key)
            .map(|e| !e.invalidated.load(Ordering::Acquire))
            .unwrap_or(false)
    }

    /// Total size of cached code
    pub fn total_size(&self) -> usize {
        self.total_code_size.load(Ordering::Relaxed)
    }

    /// Number of cached functions (including invalidated)
    pub fn entry_count(&self) -> usize {
        self.entries.read().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_dummy_code(size: usize) -> ExecutableCode {
        // Safety: this is test code — we never execute these pointers
        ExecutableCode {
            code_ptr: std::ptr::null(),
            code_size: size,
            entry_offset: 0,
            stack_maps: vec![],
            deopt_info: vec![],
        }
    }

    #[test]
    fn test_insert_and_contains() {
        let cache = CodeCache::new(1024);
        let mid = cache.allocate_module_id();
        assert!(!cache.contains(mid, 0));

        let inserted = cache.insert(mid, 0, make_dummy_code(100));
        assert!(inserted);
        assert!(cache.contains(mid, 0));
        assert_eq!(cache.total_size(), 100);
        assert_eq!(cache.entry_count(), 1);
    }

    #[test]
    fn test_invalidate() {
        let cache = CodeCache::new(1024);
        let mid = cache.allocate_module_id();
        cache.insert(mid, 0, make_dummy_code(100));
        assert!(cache.contains(mid, 0));

        cache.invalidate(mid, 0);
        assert!(!cache.contains(mid, 0));
        // Entry still exists (just invalidated)
        assert_eq!(cache.entry_count(), 1);
    }

    #[test]
    fn test_cache_full() {
        let cache = CodeCache::new(200);
        let mid = cache.allocate_module_id();
        assert!(cache.insert(mid, 0, make_dummy_code(100)));
        assert!(cache.insert(mid, 1, make_dummy_code(100)));
        // Cache is now full (200/200)
        assert!(!cache.insert(mid, 2, make_dummy_code(100)));
        assert_eq!(cache.entry_count(), 2);
    }

    #[test]
    fn test_replace_entry() {
        let cache = CodeCache::new(1024);
        let mid = cache.allocate_module_id();
        cache.insert(mid, 0, make_dummy_code(100));
        assert_eq!(cache.total_size(), 100);

        // Replace with larger code
        cache.insert(mid, 0, make_dummy_code(200));
        assert_eq!(cache.total_size(), 200);
        assert_eq!(cache.entry_count(), 1);
    }

    #[test]
    fn test_different_modules_same_func_index() {
        let cache = CodeCache::new(1024);
        let mid1 = cache.allocate_module_id();
        let mid2 = cache.allocate_module_id();

        cache.insert(mid1, 0, make_dummy_code(100));
        cache.insert(mid2, 0, make_dummy_code(100));

        assert!(cache.contains(mid1, 0));
        assert!(cache.contains(mid2, 0));
        assert_eq!(cache.entry_count(), 2);

        // Invalidate only module 1's function
        cache.invalidate(mid1, 0);
        assert!(!cache.contains(mid1, 0));
        assert!(cache.contains(mid2, 0));
    }

    #[test]
    fn test_allocate_module_id_increments() {
        let cache = CodeCache::new(1024);
        let id1 = cache.allocate_module_id();
        let id2 = cache.allocate_module_id();
        let id3 = cache.allocate_module_id();
        assert_eq!(id1, 0);
        assert_eq!(id2, 1);
        assert_eq!(id3, 2);
    }
}
