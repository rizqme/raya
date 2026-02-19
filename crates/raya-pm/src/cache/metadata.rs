//! Module metadata storage
//!
//! Stores metadata about cached modules including dependencies, version info, etc.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use thiserror::Error;

/// Errors that can occur during metadata operations
#[derive(Debug, Error)]
pub enum MetadataError {
    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
}

/// Metadata for a cached module
///
/// Stores information about a module that helps with:
/// - Dependency resolution
/// - Version management
/// - Module discovery
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModuleMetadata {
    /// Module name
    pub name: String,

    /// Module version (semver string)
    pub version: String,

    /// SHA-256 checksum (hex-encoded)
    pub checksum: String,

    /// Dependencies: name -> version constraint
    pub dependencies: HashMap<String, String>,

    /// Module description (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Module author (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,

    /// Module license (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,

    /// Timestamp when cached (Unix timestamp)
    pub cached_at: u64,
}

impl ModuleMetadata {
    /// Create new module metadata
    ///
    /// # Arguments
    /// * `name` - Module name
    /// * `version` - Module version (semver)
    /// * `checksum` - SHA-256 checksum (hex-encoded)
    ///
    /// # Returns
    /// * `ModuleMetadata` - New metadata instance
    ///
    /// # Example
    /// ```
    /// # use raya_pm::ModuleMetadata;
    /// use std::collections::HashMap;
    ///
    /// let metadata = ModuleMetadata::new(
    ///     "my-module".to_string(),
    ///     "1.0.0".to_string(),
    ///     "abc123...".to_string(),
    /// );
    /// ```
    pub fn new(name: String, version: String, checksum: String) -> Self {
        Self {
            name,
            version,
            checksum,
            dependencies: HashMap::new(),
            description: None,
            author: None,
            license: None,
            cached_at: Self::current_timestamp(),
        }
    }

    /// Add a dependency
    ///
    /// # Arguments
    /// * `name` - Dependency name
    /// * `version_constraint` - Version constraint (e.g., "^1.0.0")
    pub fn add_dependency(&mut self, name: String, version_constraint: String) {
        self.dependencies.insert(name, version_constraint);
    }

    /// Set optional fields
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    pub fn with_author(mut self, author: String) -> Self {
        self.author = Some(author);
        self
    }

    pub fn with_license(mut self, license: String) -> Self {
        self.license = Some(license);
        self
    }

    /// Load metadata from a JSON file
    ///
    /// # Arguments
    /// * `path` - Path to metadata.json file
    ///
    /// # Returns
    /// * `Ok(ModuleMetadata)` - Loaded metadata
    /// * `Err(MetadataError)` - Load failed
    ///
    /// # Example
    /// ```no_run
    /// # use raya_pm::ModuleMetadata;
    /// # use std::path::Path;
    /// let metadata = ModuleMetadata::load(Path::new("metadata.json")).unwrap();
    /// ```
    pub fn load(path: &Path) -> Result<Self, MetadataError> {
        let contents = fs::read_to_string(path)?;
        let metadata = serde_json::from_str(&contents)?;
        Ok(metadata)
    }

    /// Save metadata to a JSON file
    ///
    /// # Arguments
    /// * `path` - Path to save metadata.json
    ///
    /// # Returns
    /// * `Ok(())` - Saved successfully
    /// * `Err(MetadataError)` - Save failed
    ///
    /// # Example
    /// ```no_run
    /// # use raya_pm::ModuleMetadata;
    /// # use std::path::Path;
    /// # let metadata = ModuleMetadata::new("test".into(), "1.0.0".into(), "abc".into());
    /// metadata.save(Path::new("metadata.json")).unwrap();
    /// ```
    pub fn save(&self, path: &Path) -> Result<(), MetadataError> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Get current Unix timestamp
    fn current_timestamp() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_creation() {
        let metadata = ModuleMetadata::new(
            "test-module".to_string(),
            "1.0.0".to_string(),
            "abc123".to_string(),
        );

        assert_eq!(metadata.name, "test-module");
        assert_eq!(metadata.version, "1.0.0");
        assert_eq!(metadata.checksum, "abc123");
        assert!(metadata.dependencies.is_empty());
    }

    #[test]
    fn test_add_dependency() {
        let mut metadata = ModuleMetadata::new(
            "test-module".to_string(),
            "1.0.0".to_string(),
            "abc123".to_string(),
        );

        metadata.add_dependency("dep1".to_string(), "^1.0.0".to_string());
        metadata.add_dependency("dep2".to_string(), "~2.1.0".to_string());

        assert_eq!(metadata.dependencies.len(), 2);
        assert_eq!(metadata.dependencies.get("dep1"), Some(&"^1.0.0".to_string()));
        assert_eq!(metadata.dependencies.get("dep2"), Some(&"~2.1.0".to_string()));
    }

    #[test]
    fn test_builder_methods() {
        let metadata = ModuleMetadata::new(
            "test".to_string(),
            "1.0.0".to_string(),
            "abc".to_string(),
        )
        .with_description("A test module".to_string())
        .with_author("Test Author".to_string())
        .with_license("MIT".to_string());

        assert_eq!(metadata.description, Some("A test module".to_string()));
        assert_eq!(metadata.author, Some("Test Author".to_string()));
        assert_eq!(metadata.license, Some("MIT".to_string()));
    }

    #[test]
    fn test_serialization() {
        let metadata = ModuleMetadata::new(
            "test".to_string(),
            "1.0.0".to_string(),
            "abc123".to_string(),
        );

        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: ModuleMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(metadata, deserialized);
    }

    #[test]
    fn test_timestamp() {
        let metadata = ModuleMetadata::new(
            "test".to_string(),
            "1.0.0".to_string(),
            "abc".to_string(),
        );

        // Should have a reasonable timestamp (not 0)
        assert!(metadata.cached_at > 0);
    }
}
