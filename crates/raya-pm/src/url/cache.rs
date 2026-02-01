//! URL cache management
//!
//! Handles caching URL imports in the global cache directory.

use crate::lockfile::{LockedPackage, Lockfile, Source};
use std::path::{Path, PathBuf};
use thiserror::Error;

use super::fetch::{extract_package_name, extract_version, FetchError, UrlFetcher};

/// Errors that can occur during URL caching
#[derive(Debug, Error)]
pub enum UrlCacheError {
    /// Fetch error
    #[error("Failed to fetch URL: {0}")]
    FetchError(#[from] FetchError),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Archive extraction error
    #[error("Failed to extract archive: {0}")]
    ExtractionError(String),

    /// Lockfile error
    #[error("Lockfile error: {0}")]
    LockfileError(#[from] crate::lockfile::LockfileError),

    /// Invalid URL
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

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
    /// URL fetcher
    fetcher: UrlFetcher,
}

impl UrlCache {
    /// Create a new URL cache manager
    pub fn new(cache_root: PathBuf) -> Self {
        Self {
            cache_root,
            fetcher: UrlFetcher::new(),
        }
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
    pub fn is_cached(&self, url: &str, lockfile: Option<&Lockfile>) -> Option<CachedUrl> {
        // Look for URL in lockfile
        let locked = lockfile?.packages.iter().find(|p| {
            matches!(&p.source, Source::Url { url: u } if u == url)
        })?;

        let cache_dir = self.cache_dir(&locked.checksum);

        // Verify cache directory exists with required files
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

    /// Fetch and cache a URL
    ///
    /// Returns the cached entry and optionally a lockfile entry to add.
    pub fn fetch_and_cache(&self, url: &str) -> Result<(CachedUrl, LockedPackage), UrlCacheError> {
        // Fetch the URL
        let result = self.fetcher.fetch(url)?;

        // Get or extract package name
        let name = extract_package_name(url)
            .unwrap_or_else(|| format!("url-{}", &result.checksum[..8]));

        // Get or extract version
        let version = extract_version(url)
            .unwrap_or_else(|| "0.0.0".to_string());

        // Create cache directory
        let cache_dir = self.cache_dir(&result.checksum);
        std::fs::create_dir_all(&cache_dir)?;

        // Determine content type and handle accordingly
        let content_type = result.content_type.as_deref();

        if is_archive_content_type(content_type) || is_archive_url(url) {
            // Extract archive
            self.extract_archive(&result.content, &cache_dir, url)?;
        } else {
            // Assume it's a single file module
            // Try to detect if it's a .ryb file or source
            if url.ends_with(".ryb") {
                std::fs::write(cache_dir.join("module.ryb"), &result.content)?;
            } else {
                // Assume source file, write as main.raya
                std::fs::write(cache_dir.join("main.raya"), &result.content)?;
            }
        }

        // Create locked package entry
        let locked = LockedPackage::new(
            name.clone(),
            version.clone(),
            result.checksum.clone(),
            Source::url(url),
        );

        let cached = CachedUrl {
            url: url.to_string(),
            checksum: result.checksum,
            cache_path: cache_dir,
            name,
            version: Some(version),
        };

        Ok((cached, locked))
    }

    /// Fetch and cache with verification against expected checksum
    pub fn fetch_and_cache_verified(
        &self,
        url: &str,
        expected_checksum: &str,
    ) -> Result<CachedUrl, UrlCacheError> {
        let result = self.fetcher.fetch_verified(url, expected_checksum)?;

        // Get or extract package name
        let name = extract_package_name(url)
            .unwrap_or_else(|| format!("url-{}", &result.checksum[..8]));

        // Get or extract version
        let version = extract_version(url);

        // Create cache directory
        let cache_dir = self.cache_dir(&result.checksum);
        std::fs::create_dir_all(&cache_dir)?;

        // Extract or save content
        let content_type = result.content_type.as_deref();
        if is_archive_content_type(content_type) || is_archive_url(url) {
            self.extract_archive(&result.content, &cache_dir, url)?;
        } else {
            if url.ends_with(".ryb") {
                std::fs::write(cache_dir.join("module.ryb"), &result.content)?;
            } else {
                std::fs::write(cache_dir.join("main.raya"), &result.content)?;
            }
        }

        Ok(CachedUrl {
            url: url.to_string(),
            checksum: result.checksum,
            cache_path: cache_dir,
            name,
            version,
        })
    }

    /// Extract an archive to the cache directory
    fn extract_archive(
        &self,
        content: &[u8],
        dest: &Path,
        url: &str,
    ) -> Result<(), UrlCacheError> {
        use flate2::read::GzDecoder;
        use std::io::Cursor;
        use tar::Archive;

        // Determine archive type
        if url.ends_with(".tar.gz") || url.ends_with(".tgz") {
            // Extract tar.gz
            let cursor = Cursor::new(content);
            let decoder = GzDecoder::new(cursor);
            let mut archive = Archive::new(decoder);

            // Extract all files
            for entry in archive.entries().map_err(|e| {
                UrlCacheError::ExtractionError(format!("Failed to read tar entries: {}", e))
            })? {
                let mut entry = entry.map_err(|e| {
                    UrlCacheError::ExtractionError(format!("Failed to read entry: {}", e))
                })?;

                let entry_path = entry.path().map_err(|e| {
                    UrlCacheError::ExtractionError(format!("Invalid entry path: {}", e))
                })?;

                // Skip the top-level directory if all files are in one
                let components: Vec<_> = entry_path.components().collect();
                let dest_path = if components.len() > 1 {
                    // Skip first component (the archive root directory)
                    let rest: PathBuf = components[1..].iter().collect();
                    dest.join(rest)
                } else {
                    dest.join(&entry_path)
                };

                if entry.header().entry_type().is_dir() {
                    std::fs::create_dir_all(&dest_path)?;
                } else {
                    if let Some(parent) = dest_path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    entry.unpack(&dest_path).map_err(|e| {
                        UrlCacheError::ExtractionError(format!("Failed to extract {}: {}", dest_path.display(), e))
                    })?;
                }
            }

            Ok(())
        } else if url.ends_with(".zip") {
            // For now, return error - we'd need the zip crate
            Err(UrlCacheError::ExtractionError(
                "ZIP archives not yet supported".to_string(),
            ))
        } else {
            Err(UrlCacheError::ExtractionError(format!(
                "Unknown archive format: {}",
                url
            )))
        }
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

/// Check if content type indicates an archive
fn is_archive_content_type(content_type: Option<&str>) -> bool {
    match content_type {
        Some(ct) => {
            ct.contains("application/gzip")
                || ct.contains("application/x-gzip")
                || ct.contains("application/x-tar")
                || ct.contains("application/zip")
                || ct.contains("application/x-compressed")
        }
        None => false,
    }
}

/// Check if URL suggests an archive
fn is_archive_url(url: &str) -> bool {
    url.ends_with(".tar.gz")
        || url.ends_with(".tgz")
        || url.ends_with(".zip")
        || url.ends_with(".tar")
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
    fn test_is_archive_url() {
        assert!(is_archive_url("https://example.com/file.tar.gz"));
        assert!(is_archive_url("https://example.com/file.tgz"));
        assert!(is_archive_url("https://example.com/file.zip"));
        assert!(!is_archive_url("https://example.com/file.ryb"));
        assert!(!is_archive_url("https://example.com/file.raya"));
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
