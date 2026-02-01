//! Registry API types
//!
//! Response types for the raya.dev package registry API.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Package metadata from registry
///
/// Response from GET /packages/{name}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMetadata {
    /// Package name
    pub name: String,

    /// Package description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Package homepage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,

    /// Package repository
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,

    /// License identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,

    /// List of available versions
    pub versions: Vec<String>,

    /// Keywords/tags
    #[serde(default)]
    pub keywords: Vec<String>,

    /// Package owner/maintainers
    #[serde(default)]
    pub maintainers: Vec<String>,

    /// Time when package was created
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    /// Time when package was last updated
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Version information from registry
///
/// Response from GET /packages/{name}/{version}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    /// Package name
    pub name: String,

    /// Version number
    pub version: String,

    /// Package description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// SHA-256 checksum of the package archive
    pub checksum: String,

    /// Download information
    pub download: VersionDownload,

    /// Dependencies (name -> version constraint)
    #[serde(default)]
    pub dependencies: HashMap<String, String>,

    /// Minimum Raya runtime version required
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raya_version: Option<String>,

    /// Time when this version was published
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,

    /// Size of the package archive in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// Download information for a package version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionDownload {
    /// Download URL for the package archive
    pub url: String,

    /// Alternative download URLs (mirrors)
    #[serde(default)]
    pub mirrors: Vec<String>,
}

/// Package version (simplified for version list)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageVersion {
    /// Version number
    pub version: String,

    /// SHA-256 checksum
    pub checksum: String,

    /// Publication time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,

    /// Whether this version is yanked
    #[serde(default)]
    pub yanked: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_package_metadata() {
        let json = r#"{
            "name": "logging",
            "description": "A logging library",
            "versions": ["1.0.0", "1.1.0", "2.0.0"],
            "keywords": ["logging", "utility"],
            "maintainers": ["alice"]
        }"#;

        let metadata: PackageMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(metadata.name, "logging");
        assert_eq!(metadata.versions.len(), 3);
        assert_eq!(metadata.keywords, vec!["logging", "utility"]);
    }

    #[test]
    fn test_deserialize_version_info() {
        let json = r#"{
            "name": "logging",
            "version": "1.0.0",
            "checksum": "abc123",
            "download": {
                "url": "https://pkg.raya.dev/logging/1.0.0/download"
            },
            "dependencies": {
                "utils": "^1.0.0"
            }
        }"#;

        let info: VersionInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.name, "logging");
        assert_eq!(info.version, "1.0.0");
        assert_eq!(info.dependencies.get("utils"), Some(&"^1.0.0".to_string()));
    }
}
