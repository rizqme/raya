//! Lockfile management (raya.lock)
//!
//! Provides structures and parsing for Raya lockfiles.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

/// Errors that can occur during lockfile operations
#[derive(Debug, Error)]
pub enum LockfileError {
    /// Failed to read lockfile
    #[error("Failed to read lockfile: {0}")]
    IoError(#[from] std::io::Error),

    /// Failed to parse TOML
    #[error("Failed to parse lockfile: {0}")]
    ParseError(#[from] toml::de::Error),

    /// Failed to serialize lockfile
    #[error("Failed to serialize lockfile: {0}")]
    SerializeError(String),

    /// Validation error
    #[error("Invalid lockfile: {0}")]
    ValidationError(String),

    /// Checksum mismatch
    #[error("Checksum mismatch for package {package}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        package: String,
        expected: String,
        actual: String,
    },
}

/// Lockfile format version
pub const LOCKFILE_VERSION: u32 = 1;

/// Lockfile (raya.lock)
///
/// Records exact versions and checksums of all dependencies for reproducible builds.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Lockfile {
    /// Lockfile format version
    pub version: u32,

    /// Root package name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,

    /// Locked packages
    #[serde(default)]
    pub packages: Vec<LockedPackage>,
}

/// A locked package with exact version and checksum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedPackage {
    /// Package name
    pub name: String,

    /// Exact version
    pub version: String,

    /// SHA-256 checksum (hex-encoded)
    pub checksum: String,

    /// Package source
    pub source: Source,

    /// Direct dependencies of this package
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
}

/// Package source
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Source {
    /// Registry package
    Registry {
        /// Registry URL (optional, defaults to official registry)
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },

    /// Git repository
    Git {
        /// Git repository URL
        url: String,

        /// Git revision (commit hash)
        rev: String,
    },

    /// Local path
    Path {
        /// Relative or absolute path
        path: String,
    },

    /// URL import (direct HTTP/HTTPS)
    Url {
        /// Original URL
        url: String,
    },
}

impl Lockfile {
    /// Create a new empty lockfile
    pub fn new(root: Option<String>) -> Self {
        Self {
            version: LOCKFILE_VERSION,
            root,
            packages: Vec::new(),
        }
    }

    /// Parse a lockfile from a file
    pub fn from_file(path: &Path) -> Result<Self, LockfileError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_str(&content)
    }

    /// Parse a lockfile from a string
    pub fn from_str(content: &str) -> Result<Self, LockfileError> {
        let lockfile: Lockfile = toml::from_str(content)?;
        lockfile.validate()?;
        Ok(lockfile)
    }

    /// Validate the lockfile
    pub fn validate(&self) -> Result<(), LockfileError> {
        // Check version
        if self.version != LOCKFILE_VERSION {
            return Err(LockfileError::ValidationError(format!(
                "Unsupported lockfile version: {} (expected {})",
                self.version, LOCKFILE_VERSION
            )));
        }

        // Validate each package
        for pkg in &self.packages {
            if pkg.name.is_empty() {
                return Err(LockfileError::ValidationError(
                    "Package name cannot be empty".to_string(),
                ));
            }

            if pkg.version.is_empty() {
                return Err(LockfileError::ValidationError(format!(
                    "Package '{}' has empty version",
                    pkg.name
                )));
            }

            if pkg.checksum.is_empty() {
                return Err(LockfileError::ValidationError(format!(
                    "Package '{}' has empty checksum",
                    pkg.name
                )));
            }

            // Validate checksum is valid hex
            if pkg.checksum.len() != 64 || !pkg.checksum.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(LockfileError::ValidationError(format!(
                    "Package '{}' has invalid checksum (must be 64 hex characters)",
                    pkg.name
                )));
            }
        }

        Ok(())
    }

    /// Write lockfile to a file
    pub fn to_file(&self, path: &Path) -> Result<(), LockfileError> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| LockfileError::SerializeError(e.to_string()))?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Add a locked package
    pub fn add_package(&mut self, package: LockedPackage) {
        // Remove existing package with same name if present
        self.packages.retain(|p| p.name != package.name);
        self.packages.push(package);
    }

    /// Get a locked package by name
    pub fn get_package(&self, name: &str) -> Option<&LockedPackage> {
        self.packages.iter().find(|p| p.name == name)
    }

    /// Get all package names
    pub fn package_names(&self) -> Vec<&str> {
        self.packages.iter().map(|p| p.name.as_str()).collect()
    }

    /// Build a dependency map (package name -> dependencies)
    pub fn dependency_map(&self) -> HashMap<String, Vec<String>> {
        self.packages
            .iter()
            .map(|p| (p.name.clone(), p.dependencies.clone()))
            .collect()
    }

    /// Sort packages by name (for deterministic output)
    pub fn sort_packages(&mut self) {
        self.packages.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// Verify checksums for all packages
    pub fn verify_checksums(&self, verify_fn: impl Fn(&str, &str) -> Result<String, String>) -> Result<(), LockfileError> {
        for pkg in &self.packages {
            match verify_fn(&pkg.name, &pkg.version) {
                Ok(actual_checksum) => {
                    if actual_checksum != pkg.checksum {
                        return Err(LockfileError::ChecksumMismatch {
                            package: pkg.name.clone(),
                            expected: pkg.checksum.clone(),
                            actual: actual_checksum,
                        });
                    }
                }
                Err(e) => {
                    return Err(LockfileError::ValidationError(format!(
                        "Failed to verify checksum for {}: {}",
                        pkg.name, e
                    )));
                }
            }
        }
        Ok(())
    }
}

impl LockedPackage {
    /// Create a new locked package
    pub fn new(name: String, version: String, checksum: String, source: Source) -> Self {
        Self {
            name,
            version,
            checksum,
            source,
            dependencies: Vec::new(),
        }
    }

    /// Add a dependency
    pub fn add_dependency(&mut self, dep: String) {
        if !self.dependencies.contains(&dep) {
            self.dependencies.push(dep);
        }
    }

    /// Check if this is a registry package
    pub fn is_registry(&self) -> bool {
        matches!(self.source, Source::Registry { .. })
    }

    /// Check if this is a git package
    pub fn is_git(&self) -> bool {
        matches!(self.source, Source::Git { .. })
    }

    /// Check if this is a path package
    pub fn is_path(&self) -> bool {
        matches!(self.source, Source::Path { .. })
    }

    /// Check if this is a URL package
    pub fn is_url(&self) -> bool {
        matches!(self.source, Source::Url { .. })
    }
}

impl Source {
    /// Create a registry source
    pub fn registry(url: Option<String>) -> Self {
        Source::Registry { url }
    }

    /// Create a git source
    pub fn git(url: String, rev: String) -> Self {
        Source::Git { url, rev }
    }

    /// Create a path source
    pub fn path(path: impl Into<String>) -> Self {
        Source::Path { path: path.into() }
    }

    /// Create a URL source
    pub fn url(url: impl Into<String>) -> Self {
        Source::Url { url: url.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_empty_lockfile() {
        let lock = Lockfile::new(Some("my-package".to_string()));
        assert_eq!(lock.version, LOCKFILE_VERSION);
        assert_eq!(lock.root, Some("my-package".to_string()));
        assert_eq!(lock.packages.len(), 0);
    }

    #[test]
    fn test_parse_lockfile() {
        let toml = r#"
version = 1
root = "my-package"

[[packages]]
name = "logging"
version = "1.2.3"
checksum = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
source = { type = "registry" }

[[packages]]
name = "utils"
version = "2.0.0"
checksum = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
source = { type = "path", path = "../utils" }
dependencies = ["logging"]
"#;

        let lockfile = Lockfile::from_str(toml).unwrap();
        assert_eq!(lockfile.packages.len(), 2);
        assert_eq!(lockfile.packages[0].name, "logging");
        assert_eq!(lockfile.packages[1].name, "utils");
        assert_eq!(lockfile.packages[1].dependencies, vec!["logging"]);
    }

    #[test]
    fn test_add_package() {
        let mut lock = Lockfile::new(None);

        let pkg = LockedPackage::new(
            "test".to_string(),
            "1.0.0".to_string(),
            "a".repeat(64),
            Source::registry(None),
        );

        lock.add_package(pkg.clone());
        assert_eq!(lock.packages.len(), 1);

        // Adding again should replace
        lock.add_package(pkg);
        assert_eq!(lock.packages.len(), 1);
    }

    #[test]
    fn test_get_package() {
        let mut lock = Lockfile::new(None);

        let pkg = LockedPackage::new(
            "test".to_string(),
            "1.0.0".to_string(),
            "a".repeat(64),
            Source::registry(None),
        );

        lock.add_package(pkg);

        assert!(lock.get_package("test").is_some());
        assert!(lock.get_package("missing").is_none());
    }

    #[test]
    fn test_sort_packages() {
        let mut lock = Lockfile::new(None);

        lock.add_package(LockedPackage::new(
            "zebra".to_string(),
            "1.0.0".to_string(),
            "a".repeat(64),
            Source::registry(None),
        ));

        lock.add_package(LockedPackage::new(
            "alpha".to_string(),
            "1.0.0".to_string(),
            "b".repeat(64),
            Source::registry(None),
        ));

        lock.sort_packages();

        assert_eq!(lock.packages[0].name, "alpha");
        assert_eq!(lock.packages[1].name, "zebra");
    }

    #[test]
    fn test_invalid_checksum() {
        let toml = r#"
version = 1

[[packages]]
name = "bad"
version = "1.0.0"
checksum = "tooshort"
source = { type = "registry" }
"#;

        let result = Lockfile::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_roundtrip() {
        let mut lock = Lockfile::new(Some("root-pkg".to_string()));

        lock.add_package(LockedPackage::new(
            "dep1".to_string(),
            "1.0.0".to_string(),
            "a".repeat(64),
            Source::registry(None),
        ));

        lock.add_package(LockedPackage::new(
            "dep2".to_string(),
            "2.0.0".to_string(),
            "b".repeat(64),
            Source::git("https://github.com/user/repo".to_string(), "abc123".to_string()),
        ));

        lock.sort_packages();

        let toml = toml::to_string_pretty(&lock).unwrap();
        let parsed = Lockfile::from_str(&toml).unwrap();

        assert_eq!(lock, parsed);
    }

    #[test]
    fn test_source_types() {
        let registry_pkg = LockedPackage::new(
            "reg".to_string(),
            "1.0.0".to_string(),
            "a".repeat(64),
            Source::registry(None),
        );
        assert!(registry_pkg.is_registry());
        assert!(!registry_pkg.is_git());
        assert!(!registry_pkg.is_path());
        assert!(!registry_pkg.is_url());

        let git_pkg = LockedPackage::new(
            "git".to_string(),
            "1.0.0".to_string(),
            "b".repeat(64),
            Source::git("url".to_string(), "rev".to_string()),
        );
        assert!(!git_pkg.is_registry());
        assert!(git_pkg.is_git());
        assert!(!git_pkg.is_path());
        assert!(!git_pkg.is_url());

        let path_pkg = LockedPackage::new(
            "path".to_string(),
            "1.0.0".to_string(),
            "c".repeat(64),
            Source::path("../local"),
        );
        assert!(!path_pkg.is_registry());
        assert!(!path_pkg.is_git());
        assert!(path_pkg.is_path());
        assert!(!path_pkg.is_url());

        let url_pkg = LockedPackage::new(
            "url".to_string(),
            "1.0.0".to_string(),
            "d".repeat(64),
            Source::url("https://example.com/mod.tar.gz"),
        );
        assert!(!url_pkg.is_registry());
        assert!(!url_pkg.is_git());
        assert!(!url_pkg.is_path());
        assert!(url_pkg.is_url());
    }

    #[test]
    fn test_parse_url_source() {
        let toml = r#"
version = 1
root = "my-package"

[[packages]]
name = "remote-lib"
version = "1.0.0"
checksum = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
source = { type = "url", url = "https://github.com/user/repo/archive/v1.0.0.tar.gz" }
"#;

        let lockfile = Lockfile::from_str(toml).unwrap();
        assert_eq!(lockfile.packages.len(), 1);
        assert_eq!(lockfile.packages[0].name, "remote-lib");
        assert!(lockfile.packages[0].is_url());

        if let Source::Url { url } = &lockfile.packages[0].source {
            assert_eq!(url, "https://github.com/user/repo/archive/v1.0.0.tar.gz");
        } else {
            panic!("Expected URL source");
        }
    }
}
