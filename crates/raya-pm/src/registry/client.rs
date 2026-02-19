//! Registry HTTP client
//!
//! Provides a blocking HTTP client for the raya.dev package registry.

use super::api::{PackageMetadata, VersionInfo};
use crate::semver::Version;
use reqwest::blocking::Client;
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

/// Default registry URL
pub const DEFAULT_REGISTRY: &str = "https://pkg.raya.dev/api/v1";

/// Errors that can occur during registry operations
#[derive(Debug, Error)]
pub enum RegistryError {
    /// HTTP request failed
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    /// Package not found
    #[error("Package not found: {0}")]
    PackageNotFound(String),

    /// Version not found
    #[error("Version {version} not found for package {package}")]
    VersionNotFound { package: String, version: String },

    /// Checksum mismatch
    #[error("Checksum mismatch for {package}@{version}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        package: String,
        version: String,
        expected: String,
        actual: String,
    },

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] io::Error),

    /// JSON parsing error
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// Invalid URL
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// Archive extraction failed
    #[error("Failed to extract archive: {0}")]
    ExtractionError(String),

    /// Registry unavailable
    #[error("Registry unavailable: {0}")]
    Unavailable(String),
}

/// Registry client for interacting with the package registry
pub struct RegistryClient {
    /// HTTP client
    client: Client,

    /// Base URL for the registry
    base_url: String,
}

impl RegistryClient {
    /// Create a new registry client with default URL
    pub fn new() -> Result<Self, RegistryError> {
        Self::with_url(DEFAULT_REGISTRY)
    }

    /// Create a new registry client with a custom URL
    pub fn with_url(base_url: &str) -> Result<Self, RegistryError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .user_agent(format!("raya-pm/{}", env!("CARGO_PKG_VERSION")))
            .build()?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    /// Get package metadata
    ///
    /// GET /packages/{name}
    pub fn get_package(&self, name: &str) -> Result<PackageMetadata, RegistryError> {
        let url = format!("{}/packages/{}", self.base_url, encode_package_name(name));

        let response = self.client.get(&url).send()?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(RegistryError::PackageNotFound(name.to_string()));
        }

        if !response.status().is_success() {
            return Err(RegistryError::Unavailable(format!(
                "Registry returned status {}",
                response.status()
            )));
        }

        let metadata: PackageMetadata = response.json()?;
        Ok(metadata)
    }

    /// Get version information
    ///
    /// GET /packages/{name}/{version}
    pub fn get_version(&self, name: &str, version: &str) -> Result<VersionInfo, RegistryError> {
        let url = format!(
            "{}/packages/{}/{}",
            self.base_url,
            encode_package_name(name),
            version
        );

        let response = self.client.get(&url).send()?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(RegistryError::VersionNotFound {
                package: name.to_string(),
                version: version.to_string(),
            });
        }

        if !response.status().is_success() {
            return Err(RegistryError::Unavailable(format!(
                "Registry returned status {}",
                response.status()
            )));
        }

        let info: VersionInfo = response.json()?;
        Ok(info)
    }

    /// Get all available versions for a package
    pub fn get_versions(&self, name: &str) -> Result<Vec<Version>, RegistryError> {
        let metadata = self.get_package(name)?;

        let mut versions: Vec<Version> = metadata
            .versions
            .iter()
            .filter_map(|v| Version::parse(v).ok())
            .collect();

        // Sort versions (latest first)
        versions.sort_by(|a, b| b.cmp(a));

        Ok(versions)
    }

    /// Download a package to a directory
    ///
    /// Downloads the package archive and extracts it to the target directory.
    /// Returns the SHA-256 checksum of the downloaded archive.
    pub fn download_package(
        &self,
        name: &str,
        version: &str,
        target_dir: &Path,
    ) -> Result<String, RegistryError> {
        // Get version info first
        let info = self.get_version(name, version)?;

        // Download the archive
        let archive_bytes = self.download_url(&info.download.url)?;

        // Verify checksum
        let actual_checksum = hex::encode(Sha256::digest(&archive_bytes));
        if actual_checksum != info.checksum {
            return Err(RegistryError::ChecksumMismatch {
                package: name.to_string(),
                version: version.to_string(),
                expected: info.checksum,
                actual: actual_checksum,
            });
        }

        // Create target directory
        fs::create_dir_all(target_dir)?;

        // Extract the archive
        self.extract_archive(&archive_bytes, target_dir)?;

        Ok(actual_checksum)
    }

    /// Download raw bytes from a URL
    fn download_url(&self, url: &str) -> Result<Vec<u8>, RegistryError> {
        let response = self.client.get(url).send()?;

        if !response.status().is_success() {
            return Err(RegistryError::Unavailable(format!(
                "Download failed with status {}",
                response.status()
            )));
        }

        let bytes = response.bytes()?.to_vec();
        Ok(bytes)
    }

    /// Extract a tar.gz archive to a directory
    fn extract_archive(&self, archive_bytes: &[u8], target_dir: &Path) -> Result<(), RegistryError> {
        use flate2::read::GzDecoder;
        use tar::Archive;

        let decoder = GzDecoder::new(archive_bytes);
        let mut archive = Archive::new(decoder);

        archive
            .unpack(target_dir)
            .map_err(|e| RegistryError::ExtractionError(e.to_string()))?;

        Ok(())
    }

    /// Download and cache a package
    ///
    /// Downloads the package and places it in the global cache at ~/.raya/cache/<checksum>/
    /// The cache directory will contain:
    /// - module.ryb (compiled bytecode)
    /// - module.d.raya (type definitions)
    /// - raya.toml (package manifest)
    /// - README.md (optional)
    pub fn download_to_cache(
        &self,
        name: &str,
        version: &str,
        cache_root: &Path,
    ) -> Result<PathBuf, RegistryError> {
        // Get version info
        let info = self.get_version(name, version)?;

        // Check if already cached
        let cache_dir = cache_root.join(&info.checksum);
        if cache_dir.exists() && cache_dir.join("module.ryb").exists() {
            return Ok(cache_dir);
        }

        // Create a temporary directory for extraction
        let tmp_dir = cache_root.join("tmp").join(format!(
            "{}-{}-{}",
            name.replace('/', "-"),
            version,
            std::process::id()
        ));
        fs::create_dir_all(&tmp_dir)?;

        // Download and extract
        let checksum = self.download_package(name, version, &tmp_dir)?;

        // Move to final cache location
        let final_dir = cache_root.join(&checksum);
        if final_dir.exists() {
            fs::remove_dir_all(&final_dir)?;
        }
        fs::rename(&tmp_dir, &final_dir)?;

        Ok(final_dir)
    }
}

impl Default for RegistryClient {
    fn default() -> Self {
        Self::new().expect("Failed to create registry client")
    }
}

/// Encode a package name for URL path
///
/// Handles scoped packages (@org/name) by encoding the @ and /
fn encode_package_name(name: &str) -> String {
    if name.starts_with('@') {
        // Scoped package: @org/name -> @org%2Fname
        name.replacen('/', "%2F", 1)
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_package_name() {
        assert_eq!(encode_package_name("logging"), "logging");
        assert_eq!(encode_package_name("my-package"), "my-package");
        assert_eq!(encode_package_name("@org/package"), "@org%2Fpackage");
        assert_eq!(encode_package_name("@my-org/my-pkg"), "@my-org%2Fmy-pkg");
    }

    #[test]
    fn test_default_registry_url() {
        assert_eq!(DEFAULT_REGISTRY, "https://pkg.raya.dev/api/v1");
    }
}
