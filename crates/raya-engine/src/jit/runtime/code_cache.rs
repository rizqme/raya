//! Code cache for JIT-compiled functions
//!
//! Stores compiled native code indexed by function ID, with support for
//! invalidation (when a function is recompiled or patched).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use crate::jit::backend::traits::ExecutableCode;
use crate::jit::runtime::trampoline::JitEntryFn;

/// Entry in the code cache
pub struct CacheEntry {
    /// The compiled executable code
    pub code: ExecutableCode,
    /// Whether this entry has been invalidated (e.g., function was recompiled)
    pub invalidated: AtomicBool,
}

/// Thread-safe cache of JIT-compiled function code
pub struct CodeCache {
    /// Function index → compiled code
    entries: RwLock<FxHashMap<u32, CacheEntry>>,
    /// Total size of all cached code
    total_code_size: AtomicUsize,
    /// Maximum allowed total code size
    max_size: usize,
}

impl CodeCache {
    /// Create a new code cache with a maximum size limit (in bytes)
    pub fn new(max_size: usize) -> Self {
        CodeCache {
            entries: RwLock::new(FxHashMap::default()),
            total_code_size: AtomicUsize::new(0),
            max_size,
        }
    }

    /// Insert compiled code for a function
    ///
    /// Returns false if the cache is full and the entry was not inserted.
    pub fn insert(&self, func_index: u32, code: ExecutableCode) -> bool {
        let code_size = code.code_size;
        let current = self.total_code_size.load(Ordering::Relaxed);
        if current + code_size > self.max_size {
            return false;
        }

        let mut entries = self.entries.write();
        // Remove old entry size if replacing
        if let Some(old) = entries.remove(&func_index) {
            self.total_code_size.fetch_sub(old.code.code_size, Ordering::Relaxed);
        }

        self.total_code_size.fetch_add(code_size, Ordering::Relaxed);
        entries.insert(func_index, CacheEntry {
            code,
            invalidated: AtomicBool::new(false),
        });
        true
    }

    /// Look up the JIT entry function for a function index
    ///
    /// Returns None if the function isn't compiled or has been invalidated.
    pub fn get(&self, func_index: u32) -> Option<JitEntryFn> {
        let entries = self.entries.read();
        let entry = entries.get(&func_index)?;
        if entry.invalidated.load(Ordering::Acquire) {
            return None;
        }
        // Safety: entry_offset is within code bounds (verified at finalize time)
        let fn_ptr = unsafe { entry.code.code_ptr.add(entry.code.entry_offset) };
        Some(unsafe { std::mem::transmute(fn_ptr) })
    }

    /// Invalidate a cached function (e.g., when deoptimizing)
    pub fn invalidate(&self, func_index: u32) {
        let entries = self.entries.read();
        if let Some(entry) = entries.get(&func_index) {
            entry.invalidated.store(true, Ordering::Release);
        }
    }

    /// Check if a function has been compiled and is valid
    pub fn contains(&self, func_index: u32) -> bool {
        let entries = self.entries.read();
        entries.get(&func_index)
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
        assert!(!cache.contains(0));

        let inserted = cache.insert(0, make_dummy_code(100));
        assert!(inserted);
        assert!(cache.contains(0));
        assert_eq!(cache.total_size(), 100);
        assert_eq!(cache.entry_count(), 1);
    }

    #[test]
    fn test_invalidate() {
        let cache = CodeCache::new(1024);
        cache.insert(0, make_dummy_code(100));
        assert!(cache.contains(0));

        cache.invalidate(0);
        assert!(!cache.contains(0));
        // Entry still exists (just invalidated)
        assert_eq!(cache.entry_count(), 1);
    }

    #[test]
    fn test_cache_full() {
        let cache = CodeCache::new(200);
        assert!(cache.insert(0, make_dummy_code(100)));
        assert!(cache.insert(1, make_dummy_code(100)));
        // Cache is now full (200/200)
        assert!(!cache.insert(2, make_dummy_code(100)));
        assert_eq!(cache.entry_count(), 2);
    }

    #[test]
    fn test_replace_entry() {
        let cache = CodeCache::new(1024);
        cache.insert(0, make_dummy_code(100));
        assert_eq!(cache.total_size(), 100);

        // Replace with larger code
        cache.insert(0, make_dummy_code(200));
        assert_eq!(cache.total_size(), 200);
        assert_eq!(cache.entry_count(), 1);
    }
}
