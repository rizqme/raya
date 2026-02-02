//! Module cache for compiled modules
//!
//! Provides in-memory caching of:
//! - Parsed ASTs
//! - Type-checked modules
//! - Compiled bytecode modules

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::compiler::bytecode::Module as BytecodeModule;
use crate::parser::ast::Module as AstModule;
use crate::parser::TypeContext;

/// Cached module entry
#[derive(Debug, Clone)]
pub struct CachedModule {
    /// Path to the source file
    pub path: PathBuf,
    /// Modification time when cached
    pub mtime: Option<SystemTime>,
    /// Compiled bytecode module
    pub bytecode: BytecodeModule,
}

/// Cache entry for AST
#[derive(Debug)]
pub struct CachedAst {
    /// Path to the source file
    pub path: PathBuf,
    /// Modification time when cached
    pub mtime: Option<SystemTime>,
    /// Parsed AST
    pub ast: AstModule,
    /// Type context from type checking
    pub type_ctx: TypeContext,
}

/// Module cache for compiled modules
#[derive(Debug, Default)]
pub struct ModuleCache {
    /// Compiled bytecode modules by path
    bytecode_cache: HashMap<PathBuf, CachedModule>,
    /// Cache hit/miss statistics
    hits: usize,
    misses: usize,
}

impl ModuleCache {
    /// Create a new empty module cache
    pub fn new() -> Self {
        Self::default()
    }

    /// Get a compiled module from the cache
    ///
    /// Returns `None` if not in cache or if the source file has been modified.
    pub fn get(&mut self, path: &PathBuf) -> Option<&CachedModule> {
        // First check if entry exists and is valid
        let is_valid = self.bytecode_cache.get(path)
            .map(|cached| self.is_valid(cached))
            .unwrap_or(false);

        if is_valid {
            self.hits += 1;
            return self.bytecode_cache.get(path);
        }

        // Entry doesn't exist or is stale
        if self.bytecode_cache.contains_key(path) {
            // Invalidate stale cache entry
            self.bytecode_cache.remove(path);
        }

        self.misses += 1;
        None
    }

    /// Insert a compiled module into the cache
    pub fn insert(&mut self, path: PathBuf, bytecode: BytecodeModule) {
        let mtime = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok();

        self.bytecode_cache.insert(path.clone(), CachedModule {
            path,
            mtime,
            bytecode,
        });
    }

    /// Check if a cached entry is still valid
    fn is_valid(&self, cached: &CachedModule) -> bool {
        match cached.mtime {
            Some(cached_mtime) => {
                // Check if file has been modified
                if let Ok(metadata) = std::fs::metadata(&cached.path) {
                    if let Ok(current_mtime) = metadata.modified() {
                        return current_mtime == cached_mtime;
                    }
                }
                // If we can't check, assume invalid
                false
            }
            None => {
                // No mtime recorded, check if file exists
                cached.path.exists()
            }
        }
    }

    /// Remove a module from the cache
    pub fn invalidate(&mut self, path: &PathBuf) {
        self.bytecode_cache.remove(path);
    }

    /// Clear the entire cache
    pub fn clear(&mut self) {
        self.bytecode_cache.clear();
        self.hits = 0;
        self.misses = 0;
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.bytecode_cache.len(),
            hits: self.hits,
            misses: self.misses,
        }
    }

    /// Check if a path is in the cache (regardless of validity)
    pub fn contains(&self, path: &PathBuf) -> bool {
        self.bytecode_cache.contains_key(path)
    }

    /// Get the number of cached modules
    pub fn len(&self) -> usize {
        self.bytecode_cache.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.bytecode_cache.is_empty()
    }

    /// Get all cached module paths
    pub fn cached_paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.bytecode_cache.keys()
    }
}

/// Cache statistics
#[derive(Debug, Clone, Copy)]
pub struct CacheStats {
    /// Number of cached entries
    pub entries: usize,
    /// Number of cache hits
    pub hits: usize,
    /// Number of cache misses
    pub misses: usize,
}

impl CacheStats {
    /// Get cache hit ratio (0.0 to 1.0)
    pub fn hit_ratio(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_module() -> BytecodeModule {
        BytecodeModule {
            magic: *b"RAYA",
            version: 1,
            flags: 0,
            checksum: [0u8; 32],
            constants: crate::compiler::bytecode::ConstantPool::default(),
            functions: vec![],
            classes: vec![],
            imports: vec![],
            exports: vec![],
            metadata: crate::compiler::bytecode::Metadata {
                name: "test".to_string(),
                source_file: None,
            },
            reflection: None,
            debug_info: None,
        }
    }

    #[test]
    fn test_insert_and_get() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test.raya");
        fs::write(&path, "let x = 1;").unwrap();

        let mut cache = ModuleCache::new();
        cache.insert(path.clone(), create_test_module());

        let cached = cache.get(&path);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().path, path);
    }

    #[test]
    fn test_cache_miss() {
        let mut cache = ModuleCache::new();
        let path = PathBuf::from("/nonexistent/test.raya");

        let cached = cache.get(&path);
        assert!(cached.is_none());

        let stats = cache.stats();
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hits, 0);
    }

    #[test]
    fn test_invalidate() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test.raya");
        fs::write(&path, "let x = 1;").unwrap();

        let mut cache = ModuleCache::new();
        cache.insert(path.clone(), create_test_module());

        assert!(cache.contains(&path));
        cache.invalidate(&path);
        assert!(!cache.contains(&path));
    }

    #[test]
    fn test_clear() {
        let temp_dir = TempDir::new().unwrap();
        let path1 = temp_dir.path().join("test1.raya");
        let path2 = temp_dir.path().join("test2.raya");
        fs::write(&path1, "let x = 1;").unwrap();
        fs::write(&path2, "let y = 2;").unwrap();

        let mut cache = ModuleCache::new();
        cache.insert(path1.clone(), create_test_module());
        cache.insert(path2.clone(), create_test_module());

        assert_eq!(cache.len(), 2);
        cache.clear();
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_stale_cache_invalidation() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test.raya");
        fs::write(&path, "let x = 1;").unwrap();

        let mut cache = ModuleCache::new();
        cache.insert(path.clone(), create_test_module());

        // Modify the file
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&path, "let x = 2;").unwrap();

        // Cache should be invalidated
        let cached = cache.get(&path);
        assert!(cached.is_none());
    }

    #[test]
    fn test_cache_stats() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test.raya");
        fs::write(&path, "let x = 1;").unwrap();

        let mut cache = ModuleCache::new();
        cache.insert(path.clone(), create_test_module());

        // One miss
        let _ = cache.get(&PathBuf::from("/nonexistent.raya"));

        // Two hits
        let _ = cache.get(&path);
        let _ = cache.get(&path);

        let stats = cache.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_ratio() - 0.666).abs() < 0.01);
    }
}
