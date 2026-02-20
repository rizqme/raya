//! URL cache management
//!
//! Handles looking up cached URL imports in the global cache directory.
//! Fetching and caching is now handled by std:pm in Raya.

use crate::lockfile::Source;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur during URL caching
#[derive(Debug, Error)]
pub enum UrlCacheError {
    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Package not found in cache
    #[error("URL not found in cache: {0}")]
    NotCached(String),
}

/// Cached URL entry
#[derive(Debug, Clone)]
pub struct CachedUrl {
    /// Original URL
    pub url: String,
    /// SHA-256 checksum
    pub checksum: String,
    /// Path to cached content
    pub cache_path: PathBuf,
    /// Extracted package name
    pub name: String,
    /// Extracted version (if available)
    pub version: Option<String>,
}

/// URL cache manager
pub struct UrlCache {
    /// Cache root directory (~/.raya/cache/)
    cache_root: PathBuf,
}

impl UrlCache {
    /// Create a new URL cache manager
    pub fn new(cache_root: PathBuf) -> Self {
        Self { cache_root }
    }

    /// Create URL cache with default cache directory
    pub fn default_cache() -> Self {
        let cache_root = dirs::home_dir()
            .map(|h| h.join(".raya").join("cache"))
            .unwrap_or_else(|| PathBuf::from(".raya").join("cache"));

        Self::new(cache_root)
    }

    /// Get the cache directory for a checksum
    pub fn cache_dir(&self, checksum: &str) -> PathBuf {
        self.cache_root.join(checksum)
    }

    /// Check if a URL is already cached (by checking lockfile)
    pub fn is_cached(&self, url: &str, lockfile: Option<&crate::Lockfile>) -> Option<CachedUrl> {
        let locked = lockfile?.packages.iter().find(|p| {
            matches!(&p.source, Source::Url { url: u } if u == url)
        })?;

        let cache_dir = self.cache_dir(&locked.checksum);

        if !cache_dir.exists() {
            return None;
        }

        Some(CachedUrl {
            url: url.to_string(),
            checksum: locked.checksum.clone(),
            cache_path: cache_dir,
            name: locked.name.clone(),
            version: if locked.version.is_empty() || locked.version == "0.0.0" {
                None
            } else {
                Some(locked.version.clone())
            },
        })
    }

    /// Get the entry point for a cached URL
    pub fn find_entry_point(&self, cached: &CachedUrl) -> Option<PathBuf> {
        let cache_dir = &cached.cache_path;

        // Check for compiled bytecode
        let ryb_path = cache_dir.join("module.ryb");
        if ryb_path.exists() {
            return Some(ryb_path);
        }

        // Check for raya.toml to find main entry
        let manifest_path = cache_dir.join("raya.toml");
        if manifest_path.exists() {
            if let Ok(manifest) = crate::PackageManifest::from_file(&manifest_path) {
                if let Some(main) = manifest.package.main {
                    let entry = cache_dir.join(&main);
                    if entry.exists() {
                        return Some(entry);
                    }
                }
            }
        }

        // Default entry points
        let candidates = [
            cache_dir.join("src/index.raya"),
            cache_dir.join("index.raya"),
            cache_dir.join("src/main.raya"),
            cache_dir.join("main.raya"),
        ];

        for candidate in &candidates {
            if candidate.exists() {
                return Some(candidate.clone());
            }
        }

        None
    }

    /// Remove a URL from the cache
    pub fn remove(&self, checksum: &str) -> Result<(), UrlCacheError> {
        let cache_dir = self.cache_dir(checksum);
        if cache_dir.exists() {
            std::fs::remove_dir_all(&cache_dir)?;
        }
        Ok(())
    }

    /// Get cache root directory
    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cache_dir_path() {
        let temp = TempDir::new().unwrap();
        let cache = UrlCache::new(temp.path().to_path_buf());

        let checksum = "a".repeat(64);
        let dir = cache.cache_dir(&checksum);

        assert_eq!(dir, temp.path().join(&checksum));
    }

    #[test]
    fn test_find_entry_point_ryb() {
        let temp = TempDir::new().unwrap();
        let checksum = "b".repeat(64);
        let cache_dir = temp.path().join(&checksum);
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join("module.ryb"), b"bytecode").unwrap();

        let cache = UrlCache::new(temp.path().to_path_buf());
        let cached = CachedUrl {
            url: "https://example.com/mod".to_string(),
            checksum,
            cache_path: cache_dir.clone(),
            name: "mod".to_string(),
            version: None,
        };

        let entry = cache.find_entry_point(&cached);
        assert_eq!(entry, Some(cache_dir.join("module.ryb")));
    }

    #[test]
    fn test_find_entry_point_index_raya() {
        let temp = TempDir::new().unwrap();
        let checksum = "c".repeat(64);
        let cache_dir = temp.path().join(&checksum);
        let src_dir = cache_dir.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("index.raya"), "export let x = 1;").unwrap();

        let cache = UrlCache::new(temp.path().to_path_buf());
        let cached = CachedUrl {
            url: "https://example.com/mod".to_string(),
            checksum,
            cache_path: cache_dir.clone(),
            name: "mod".to_string(),
            version: None,
        };

        let entry = cache.find_entry_point(&cached);
        assert_eq!(entry, Some(src_dir.join("index.raya")));
    }
}
