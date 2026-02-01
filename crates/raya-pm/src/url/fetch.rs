//! URL fetching for remote module imports
//!
//! Handles downloading modules from HTTP/HTTPS URLs.

use reqwest::blocking::Client;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::time::Duration;
use thiserror::Error;

/// Errors that can occur during URL fetching
#[derive(Debug, Error)]
pub enum FetchError {
    /// HTTP request failed
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    /// Non-success HTTP status
    #[error("HTTP {status} for URL: {url}")]
    HttpStatus { status: u16, url: String },

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Invalid URL
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// Checksum mismatch
    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    /// Content too large
    #[error("Content too large: {size} bytes (max: {max})")]
    ContentTooLarge { size: u64, max: u64 },
}

/// Maximum size for URL imports (50 MB)
pub const MAX_CONTENT_SIZE: u64 = 50 * 1024 * 1024;

/// HTTP client configuration
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Result of fetching a URL
#[derive(Debug)]
pub struct FetchResult {
    /// Content bytes
    pub content: Vec<u8>,
    /// SHA-256 checksum (hex-encoded)
    pub checksum: String,
    /// Content type (if provided by server)
    pub content_type: Option<String>,
    /// Final URL (after redirects)
    pub final_url: String,
}

/// URL fetcher with caching support
pub struct UrlFetcher {
    client: Client,
    max_size: u64,
}

impl Default for UrlFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl UrlFetcher {
    /// Create a new URL fetcher
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent("raya-pm/0.1.0")
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            max_size: MAX_CONTENT_SIZE,
        }
    }

    /// Create a URL fetcher with custom max size
    pub fn with_max_size(max_size: u64) -> Self {
        let mut fetcher = Self::new();
        fetcher.max_size = max_size;
        fetcher
    }

    /// Fetch content from a URL
    pub fn fetch(&self, url: &str) -> Result<FetchResult, FetchError> {
        // Validate URL
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(FetchError::InvalidUrl(url.to_string()));
        }

        // Make request
        let response = self.client.get(url).send()?;

        // Check status
        let status = response.status();
        if !status.is_success() {
            return Err(FetchError::HttpStatus {
                status: status.as_u16(),
                url: url.to_string(),
            });
        }

        // Check content length
        if let Some(len) = response.content_length() {
            if len > self.max_size {
                return Err(FetchError::ContentTooLarge {
                    size: len,
                    max: self.max_size,
                });
            }
        }

        // Get metadata before consuming response
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let final_url = response.url().to_string();

        // Read content with size limit
        let mut content = Vec::new();
        let mut reader = response.take(self.max_size + 1);
        reader.read_to_end(&mut content)?;

        if content.len() as u64 > self.max_size {
            return Err(FetchError::ContentTooLarge {
                size: content.len() as u64,
                max: self.max_size,
            });
        }

        // Compute checksum
        let checksum = compute_checksum(&content);

        Ok(FetchResult {
            content,
            checksum,
            content_type,
            final_url,
        })
    }

    /// Fetch and verify checksum
    pub fn fetch_verified(&self, url: &str, expected_checksum: &str) -> Result<FetchResult, FetchError> {
        let result = self.fetch(url)?;

        if result.checksum != expected_checksum {
            return Err(FetchError::ChecksumMismatch {
                expected: expected_checksum.to_string(),
                actual: result.checksum,
            });
        }

        Ok(result)
    }

    /// Fetch and save to a file
    pub fn fetch_to_file(&self, url: &str, dest: &Path) -> Result<FetchResult, FetchError> {
        let result = self.fetch(url)?;

        // Ensure parent directory exists
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(dest, &result.content)?;

        Ok(result)
    }
}

/// Compute SHA-256 checksum of bytes
pub fn compute_checksum(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = hasher.finalize();
    hex::encode(hash)
}

/// Compute SHA-256 checksum of a file
pub fn compute_file_checksum(path: &Path) -> Result<String, FetchError> {
    let data = std::fs::read(path)?;
    Ok(compute_checksum(&data))
}

/// Parse a URL to extract package name from path
///
/// Supports patterns:
/// - `https://github.com/user/repo/archive/v1.0.0.tar.gz` -> "repo"
/// - `https://pkg.raya.dev/lib@1.0.0` -> "lib"
/// - `https://example.com/path/to/module.tar.gz` -> "module"
pub fn extract_package_name(url: &str) -> Option<String> {
    let url_obj = url::Url::parse(url).ok()?;
    let path = url_obj.path();

    // Try GitHub-style URLs
    // /user/repo/archive/vX.Y.Z.tar.gz
    if let Some(idx) = path.find("/archive/") {
        let before_archive = &path[..idx];
        return before_archive.rsplit('/').next().map(String::from);
    }

    // Try pkg.raya.dev style: /lib@1.0.0 or /lib
    let last_segment = path.rsplit('/').next()?;

    // Remove version suffix if present
    let name = if let Some(at_idx) = last_segment.find('@') {
        &last_segment[..at_idx]
    } else {
        last_segment
    };

    // Remove file extension
    let name = name.strip_suffix(".tar.gz")
        .or_else(|| name.strip_suffix(".tgz"))
        .or_else(|| name.strip_suffix(".zip"))
        .unwrap_or(name);

    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Parse version from URL
///
/// Supports patterns:
/// - `https://github.com/user/repo/archive/v1.0.0.tar.gz` -> "1.0.0"
/// - `https://pkg.raya.dev/lib@1.0.0` -> "1.0.0"
pub fn extract_version(url: &str) -> Option<String> {
    let url_obj = url::Url::parse(url).ok()?;
    let path = url_obj.path();

    // Try GitHub-style: /archive/vX.Y.Z.tar.gz
    if let Some(idx) = path.find("/archive/") {
        let after_archive = &path[idx + 9..]; // Skip "/archive/"
        let version = after_archive
            .strip_prefix('v')
            .unwrap_or(after_archive);
        let version = version.strip_suffix(".tar.gz")
            .or_else(|| version.strip_suffix(".tgz"))
            .or_else(|| version.strip_suffix(".zip"))
            .unwrap_or(version);
        return Some(version.to_string());
    }

    // Try @version style: lib@1.0.0
    let last_segment = path.rsplit('/').next()?;
    if let Some(at_idx) = last_segment.find('@') {
        let version = &last_segment[at_idx + 1..];
        return Some(version.to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_checksum() {
        let data = b"hello world";
        let checksum = compute_checksum(data);
        assert_eq!(checksum.len(), 64);
        assert!(checksum.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_extract_package_name_github() {
        assert_eq!(
            extract_package_name("https://github.com/user/my-lib/archive/v1.0.0.tar.gz"),
            Some("my-lib".to_string())
        );
    }

    #[test]
    fn test_extract_package_name_raya_dev() {
        assert_eq!(
            extract_package_name("https://pkg.raya.dev/logging@1.0.0"),
            Some("logging".to_string())
        );
    }

    #[test]
    fn test_extract_package_name_generic() {
        assert_eq!(
            extract_package_name("https://example.com/path/to/utils.tar.gz"),
            Some("utils".to_string())
        );
    }

    #[test]
    fn test_extract_version_github() {
        assert_eq!(
            extract_version("https://github.com/user/repo/archive/v1.2.3.tar.gz"),
            Some("1.2.3".to_string())
        );
    }

    #[test]
    fn test_extract_version_at_style() {
        assert_eq!(
            extract_version("https://pkg.raya.dev/lib@2.0.0"),
            Some("2.0.0".to_string())
        );
    }

    #[test]
    fn test_invalid_url() {
        let fetcher = UrlFetcher::new();
        let result = fetcher.fetch("not-a-url");
        assert!(matches!(result, Err(FetchError::InvalidUrl(_))));
    }
}
