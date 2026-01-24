//! Module caching infrastructure
//!
//! Provides content-addressable storage for compiled Raya modules at ~/.raya/cache/

mod metadata;

pub use metadata::ModuleMetadata;

use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur during cache operations
#[derive(Debug, Error)]
pub enum CacheError {
    /// IO error (file operations)
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Cache directory creation failed
    #[error("Failed to create cache directory: {0}")]
    CacheInitError(String),

    /// Module not found in cache
    #[error("Module not found in cache: {0}")]
    ModuleNotFound(String),

    /// Checksum mismatch
    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    /// Metadata error
    #[error("Metadata error: {0}")]
    MetadataError(String),

    /// Invalid hash format
    #[error("Invalid hash format: {0}")]
    InvalidHash(String),
}

/// Content-addressable cache for Raya modules
///
/// Stores compiled .rbin modules at ~/.raya/cache/ indexed by SHA-256 hash.
///
/// Directory structure:
/// ```text
/// ~/.raya/cache/
/// ├── <sha256-hash>/
/// │   ├── module.rbin
/// │   └── metadata.json
/// ├── tmp/
/// └── registry/
/// ```
#[derive(Debug, Clone)]
pub struct Cache {
    /// Root cache directory (~/.raya/cache/)
    root: PathBuf,
}

impl Cache {
    /// Initialize the cache
    ///
    /// Creates the cache directory structure at ~/.raya/cache/ if it doesn't exist.
    ///
    /// # Returns
    /// * `Ok(Cache)` - Successfully initialized cache
    /// * `Err(CacheError)` - Failed to create cache directory
    ///
    /// # Example
    /// ```
    /// # use rpkg::Cache;
    /// let cache = Cache::init().unwrap();
    /// ```
    pub fn init() -> Result<Self, CacheError> {
        let root = Self::cache_dir()?;

        // Create cache directory structure
        fs::create_dir_all(&root)?;
        fs::create_dir_all(root.join("tmp"))?;
        fs::create_dir_all(root.join("registry"))?;

        Ok(Self { root })
    }

    /// Get the cache directory path
    ///
    /// Returns ~/.raya/cache/ or an error if home directory cannot be determined.
    fn cache_dir() -> Result<PathBuf, CacheError> {
        let home = dirs::home_dir().ok_or_else(|| {
            CacheError::CacheInitError("Could not determine home directory".to_string())
        })?;

        Ok(home.join(".raya").join("cache"))
    }

    /// Store a module in the cache
    ///
    /// # Arguments
    /// * `module_bytes` - Raw .rbin module data
    ///
    /// # Returns
    /// * `Ok([u8; 32])` - SHA-256 hash of the stored module
    /// * `Err(CacheError)` - Storage failed
    ///
    /// # Example
    /// ```no_run
    /// # use rpkg::Cache;
    /// # let cache = Cache::init().unwrap();
    /// let module_data = std::fs::read("my_module.rbin").unwrap();
    /// let hash = cache.store(&module_data).unwrap();
    /// println!("Stored with hash: {}", hex::encode(hash));
    /// ```
    pub fn store(&self, module_bytes: &[u8]) -> Result<[u8; 32], CacheError> {
        // Compute SHA-256 hash
        let hash = Sha256::digest(module_bytes);
        let checksum: [u8; 32] = hash.into();
        let hash_str = hex::encode(checksum);

        // Check if already cached
        if self.exists(&checksum) {
            return Ok(checksum);
        }

        // Create module directory
        let module_dir = self.root.join(&hash_str);
        fs::create_dir_all(&module_dir)?;

        // Write to temporary file first (atomic write)
        let tmp_dir = self.root.join("tmp");
        fs::create_dir_all(&tmp_dir)?; // Ensure tmp directory exists
        let tmp_path = tmp_dir.join(format!("{}.tmp", hash_str));
        let mut tmp_file = fs::File::create(&tmp_path)?;
        tmp_file.write_all(module_bytes)?;
        tmp_file.sync_all()?;

        // Move to final location
        let final_path = module_dir.join("module.rbin");
        fs::rename(&tmp_path, &final_path)?;

        Ok(checksum)
    }

    /// Retrieve a module from the cache
    ///
    /// # Arguments
    /// * `hash` - SHA-256 hash of the module
    ///
    /// # Returns
    /// * `Ok(Vec<u8>)` - Module bytes
    /// * `Err(CacheError)` - Module not found or read failed
    ///
    /// # Example
    /// ```no_run
    /// # use rpkg::Cache;
    /// # let cache = Cache::init().unwrap();
    /// # let hash = [0u8; 32];
    /// let module_bytes = cache.retrieve(&hash).unwrap();
    /// ```
    pub fn retrieve(&self, hash: &[u8; 32]) -> Result<Vec<u8>, CacheError> {
        let path = self.module_path(hash);

        if !path.exists() {
            return Err(CacheError::ModuleNotFound(hex::encode(hash)));
        }

        let bytes = fs::read(&path)?;

        // Verify checksum
        let computed_hash = Sha256::digest(&bytes);
        let computed_checksum: [u8; 32] = computed_hash.into();

        if &computed_checksum != hash {
            return Err(CacheError::ChecksumMismatch {
                expected: hex::encode(hash),
                actual: hex::encode(computed_checksum),
            });
        }

        Ok(bytes)
    }

    /// Check if a module exists in the cache
    ///
    /// # Arguments
    /// * `hash` - SHA-256 hash of the module
    ///
    /// # Returns
    /// * `true` - Module exists in cache
    /// * `false` - Module not found
    ///
    /// # Example
    /// ```no_run
    /// # use rpkg::Cache;
    /// # let cache = Cache::init().unwrap();
    /// # let hash = [0u8; 32];
    /// if cache.exists(&hash) {
    ///     println!("Module is cached");
    /// }
    /// ```
    pub fn exists(&self, hash: &[u8; 32]) -> bool {
        self.module_path(hash).exists()
    }

    /// Get the path to a cached module
    ///
    /// # Arguments
    /// * `hash` - SHA-256 hash of the module
    ///
    /// # Returns
    /// * `PathBuf` - Path to the module.rbin file
    ///
    /// # Example
    /// ```no_run
    /// # use rpkg::Cache;
    /// # let cache = Cache::init().unwrap();
    /// # let hash = [0u8; 32];
    /// let path = cache.module_path(&hash);
    /// println!("Module path: {}", path.display());
    /// ```
    pub fn module_path(&self, hash: &[u8; 32]) -> PathBuf {
        let hash_str = hex::encode(hash);
        self.root.join(&hash_str).join("module.rbin")
    }

    /// Get the path to a module's metadata file
    ///
    /// # Arguments
    /// * `hash` - SHA-256 hash of the module
    ///
    /// # Returns
    /// * `PathBuf` - Path to the metadata.json file
    pub fn metadata_path(&self, hash: &[u8; 32]) -> PathBuf {
        let hash_str = hex::encode(hash);
        self.root.join(&hash_str).join("metadata.json")
    }

    /// Store metadata for a cached module
    ///
    /// # Arguments
    /// * `hash` - SHA-256 hash of the module
    /// * `metadata` - Module metadata to store
    ///
    /// # Returns
    /// * `Ok(())` - Metadata stored successfully
    /// * `Err(CacheError)` - Storage failed
    pub fn store_metadata(
        &self,
        hash: &[u8; 32],
        metadata: &ModuleMetadata,
    ) -> Result<(), CacheError> {
        let path = self.metadata_path(hash);
        metadata
            .save(&path)
            .map_err(|e| CacheError::MetadataError(e.to_string()))
    }

    /// Load metadata for a cached module
    ///
    /// # Arguments
    /// * `hash` - SHA-256 hash of the module
    ///
    /// # Returns
    /// * `Ok(ModuleMetadata)` - Loaded metadata
    /// * `Err(CacheError)` - Load failed
    pub fn load_metadata(&self, hash: &[u8; 32]) -> Result<ModuleMetadata, CacheError> {
        let path = self.metadata_path(hash);
        ModuleMetadata::load(&path).map_err(|e| CacheError::MetadataError(e.to_string()))
    }

    /// Get the cache root directory
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Clear the entire cache
    ///
    /// **Warning:** This deletes all cached modules!
    ///
    /// # Returns
    /// * `Ok(())` - Cache cleared successfully
    /// * `Err(CacheError)` - Failed to clear cache
    pub fn clear(&self) -> Result<(), CacheError> {
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() && path.file_name() != Some("tmp".as_ref()) {
                fs::remove_dir_all(&path)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_init() {
        let cache = Cache::init();
        assert!(cache.is_ok());
    }

    #[test]
    fn test_store_and_retrieve() {
        let cache = Cache::init().unwrap();
        let test_data = b"test module data";

        // Store
        let hash = cache.store(test_data).unwrap();

        // Verify it exists
        assert!(cache.exists(&hash));

        // Retrieve
        let retrieved = cache.retrieve(&hash).unwrap();
        assert_eq!(retrieved, test_data);
    }

    #[test]
    fn test_duplicate_store() {
        let cache = Cache::init().unwrap();
        let test_data = b"test module data";

        // Store twice
        let hash1 = cache.store(test_data).unwrap();
        let hash2 = cache.store(test_data).unwrap();

        // Should have same hash
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_module_not_found() {
        let cache = Cache::init().unwrap();
        let nonexistent_hash = [0u8; 32];

        let result = cache.retrieve(&nonexistent_hash);
        assert!(result.is_err());
        assert!(matches!(result, Err(CacheError::ModuleNotFound(_))));
    }

    #[test]
    fn test_exists() {
        let cache = Cache::init().unwrap();
        let test_data = b"test module data";

        let hash = cache.store(test_data).unwrap();
        assert!(cache.exists(&hash));

        let nonexistent = [0u8; 32];
        assert!(!cache.exists(&nonexistent));
    }
}
