//! Package manifest parsing (raya.toml)
//!
//! Provides structures and parsing for Raya package manifests.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur during manifest parsing
#[derive(Debug, Error)]
pub enum ManifestError {
    /// Failed to read manifest file
    #[error("Failed to read manifest file: {0}")]
    IoError(#[from] std::io::Error),

    /// Failed to parse TOML
    #[error("Failed to parse manifest: {0}")]
    ParseError(#[from] toml::de::Error),

    /// Validation error
    #[error("Invalid manifest: {0}")]
    ValidationError(String),

    /// Missing required field
    #[error("Missing required field: {0}")]
    MissingField(String),
}

/// Package manifest (raya.toml)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackageManifest {
    /// Package metadata
    pub package: PackageInfo,

    /// JSX compilation settings (optional — omit to disable JSX)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jsx: Option<JsxConfig>,

    /// Runtime dependencies
    #[serde(default)]
    pub dependencies: HashMap<String, Dependency>,

    /// Development-only dependencies
    #[serde(default, rename = "dev-dependencies")]
    pub dev_dependencies: HashMap<String, Dependency>,
}

/// JSX compilation configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsxConfig {
    /// Factory function name (default: "createElement")
    #[serde(default = "default_jsx_factory")]
    pub factory: String,

    /// Fragment component name (default: "Fragment")
    #[serde(default = "default_jsx_fragment")]
    pub fragment: String,

    /// Module to auto-import factory from (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub factory_module: Option<String>,

    /// Development mode — adds __source and __self props for debugging
    #[serde(default)]
    pub development: bool,
}

fn default_jsx_factory() -> String {
    "createElement".to_string()
}

fn default_jsx_fragment() -> String {
    "Fragment".to_string()
}

impl Default for JsxConfig {
    fn default() -> Self {
        Self {
            factory: default_jsx_factory(),
            fragment: default_jsx_fragment(),
            factory_module: None,
            development: false,
        }
    }
}

/// Package information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackageInfo {
    /// Package name (must be unique)
    pub name: String,

    /// Semver version
    pub version: String,

    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Authors
    #[serde(default)]
    pub authors: Vec<String>,

    /// License identifier (SPDX)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,

    /// Repository URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,

    /// Homepage URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,

    /// Main entry point (defaults to "src/main.raya")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,
}

/// Dependency specification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Dependency {
    /// Simple version constraint: "^1.2.0"
    Simple(String),

    /// Detailed dependency specification
    Detailed {
        /// Version constraint (for registry packages)
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,

        /// Local path dependency
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,

        /// Git repository URL
        #[serde(skip_serializing_if = "Option::is_none")]
        git: Option<String>,

        /// Git branch
        #[serde(skip_serializing_if = "Option::is_none")]
        branch: Option<String>,

        /// Git tag
        #[serde(skip_serializing_if = "Option::is_none")]
        tag: Option<String>,

        /// Git commit hash
        #[serde(skip_serializing_if = "Option::is_none")]
        rev: Option<String>,
    },
}

impl PackageManifest {
    /// Parse a manifest from a file
    pub fn from_file(path: &Path) -> Result<Self, ManifestError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_str(&content)
    }

    /// Parse a manifest from a string
    pub fn from_str(content: &str) -> Result<Self, ManifestError> {
        let manifest: PackageManifest = toml::from_str(content)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validate the manifest
    pub fn validate(&self) -> Result<(), ManifestError> {
        // Validate package name
        if self.package.name.is_empty() {
            return Err(ManifestError::ValidationError(
                "Package name cannot be empty".to_string(),
            ));
        }

        // Validate package name format (alphanumeric, hyphens, underscores, @/)
        if !is_valid_package_name(&self.package.name) {
            return Err(ManifestError::ValidationError(format!(
                "Invalid package name: {}. Must contain only alphanumeric characters, hyphens, underscores, and optional @org/ prefix",
                self.package.name
            )));
        }

        // Validate version format (basic semver check)
        if !is_valid_version(&self.package.version) {
            return Err(ManifestError::ValidationError(format!(
                "Invalid version: {}. Must be valid semver (e.g., 1.2.3)",
                self.package.version
            )));
        }

        // Validate dependencies
        for (name, dep) in &self.dependencies {
            validate_dependency(name, dep)?;
        }

        for (name, dep) in &self.dev_dependencies {
            validate_dependency(name, dep)?;
        }

        Ok(())
    }

    /// Write manifest to a file
    pub fn to_file(&self, path: &Path) -> Result<(), ManifestError> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| ManifestError::ValidationError(e.to_string()))?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Get all dependencies (runtime + dev)
    pub fn all_dependencies(&self) -> HashMap<String, &Dependency> {
        let mut all = HashMap::new();
        for (name, dep) in &self.dependencies {
            all.insert(name.clone(), dep);
        }
        for (name, dep) in &self.dev_dependencies {
            all.insert(name.clone(), dep);
        }
        all
    }

    /// Get only runtime dependencies
    pub fn runtime_dependencies(&self) -> &HashMap<String, Dependency> {
        &self.dependencies
    }
}

impl Dependency {
    /// Get the version constraint (if any)
    pub fn version(&self) -> Option<&str> {
        match self {
            Dependency::Simple(v) => Some(v.as_str()),
            Dependency::Detailed { version, .. } => version.as_deref(),
        }
    }

    /// Get the path (if this is a path dependency)
    pub fn path(&self) -> Option<PathBuf> {
        match self {
            Dependency::Detailed { path: Some(p), .. } => Some(PathBuf::from(p)),
            _ => None,
        }
    }

    /// Get the git URL (if this is a git dependency)
    pub fn git(&self) -> Option<&str> {
        match self {
            Dependency::Detailed { git: Some(g), .. } => Some(g.as_str()),
            _ => None,
        }
    }

    /// Check if this is a path dependency
    pub fn is_path(&self) -> bool {
        matches!(
            self,
            Dependency::Detailed { path: Some(_), .. }
        )
    }

    /// Check if this is a git dependency
    pub fn is_git(&self) -> bool {
        matches!(
            self,
            Dependency::Detailed { git: Some(_), .. }
        )
    }

    /// Check if this is a registry dependency
    pub fn is_registry(&self) -> bool {
        match self {
            Dependency::Simple(_) => true,
            Dependency::Detailed { version: Some(_), path: None, git: None, .. } => true,
            _ => false,
        }
    }
}

/// Validate a package name
fn is_valid_package_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }

    // Handle scoped packages (@org/package)
    if name.starts_with('@') {
        if let Some(slash_pos) = name.find('/') {
            if slash_pos == 1 || slash_pos == name.len() - 1 {
                return false; // @/ or @org/
            }
            let org = &name[1..slash_pos];
            let pkg = &name[slash_pos + 1..];
            return is_valid_name_part(org) && is_valid_name_part(pkg);
        }
        return false; // @ without /
    }

    is_valid_name_part(name)
}

/// Validate a name part (alphanumeric, hyphens, underscores)
fn is_valid_name_part(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

/// Basic semver validation (MAJOR.MINOR.PATCH)
fn is_valid_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 {
        return false;
    }

    parts.iter().all(|p| p.parse::<u32>().is_ok())
}

/// Validate a dependency specification
fn validate_dependency(name: &str, dep: &Dependency) -> Result<(), ManifestError> {
    if name.is_empty() {
        return Err(ManifestError::ValidationError(
            "Dependency name cannot be empty".to_string(),
        ));
    }

    match dep {
        Dependency::Simple(version) => {
            if version.is_empty() {
                return Err(ManifestError::ValidationError(format!(
                    "Dependency '{}' has empty version",
                    name
                )));
            }
        }
        Dependency::Detailed {
            version,
            path,
            git,
            ..
        } => {
            // Must have at least one source specified
            if version.is_none() && path.is_none() && git.is_none() {
                return Err(ManifestError::ValidationError(format!(
                    "Dependency '{}' must specify version, path, or git",
                    name
                )));
            }

            // Cannot have multiple sources
            let source_count =
                [version.is_some(), path.is_some(), git.is_some()]
                    .iter()
                    .filter(|&&x| x)
                    .count();

            if source_count > 1 {
                return Err(ManifestError::ValidationError(format!(
                    "Dependency '{}' cannot specify multiple sources (version, path, git)",
                    name
                )));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_manifest() {
        let toml = r#"
[package]
name = "my-package"
version = "1.0.0"
description = "A test package"
authors = ["Alice <alice@example.com>"]
license = "MIT"

[dependencies]
logging = "^1.2.0"
http = "~2.1.0"
"#;

        let manifest = PackageManifest::from_str(toml).unwrap();
        assert_eq!(manifest.package.name, "my-package");
        assert_eq!(manifest.package.version, "1.0.0");
        assert_eq!(manifest.dependencies.len(), 2);
    }

    #[test]
    fn test_parse_scoped_package() {
        let toml = r#"
[package]
name = "@org/my-package"
version = "2.3.4"
"#;

        let manifest = PackageManifest::from_str(toml).unwrap();
        assert_eq!(manifest.package.name, "@org/my-package");
    }

    #[test]
    fn test_parse_path_dependency() {
        let toml = r#"
[package]
name = "main"
version = "1.0.0"

[dependencies]
utils = { path = "../utils" }
"#;

        let manifest = PackageManifest::from_str(toml).unwrap();
        let utils_dep = &manifest.dependencies["utils"];
        assert!(utils_dep.is_path());
        assert_eq!(utils_dep.path(), Some(PathBuf::from("../utils")));
    }

    #[test]
    fn test_parse_git_dependency() {
        let toml = r#"
[package]
name = "main"
version = "1.0.0"

[dependencies]
lib = { git = "https://github.com/user/repo", tag = "v1.0.0" }
"#;

        let manifest = PackageManifest::from_str(toml).unwrap();
        let lib_dep = &manifest.dependencies["lib"];
        assert!(lib_dep.is_git());
        assert_eq!(lib_dep.git(), Some("https://github.com/user/repo"));
    }

    #[test]
    fn test_validate_package_name() {
        assert!(is_valid_package_name("my-package"));
        assert!(is_valid_package_name("my_package"));
        assert!(is_valid_package_name("package123"));
        assert!(is_valid_package_name("@org/package"));
        assert!(is_valid_package_name("@my-org/my-package"));

        assert!(!is_valid_package_name(""));
        assert!(!is_valid_package_name("@"));
        assert!(!is_valid_package_name("@/"));
        assert!(!is_valid_package_name("@org/"));
        assert!(!is_valid_package_name("@/package"));
        assert!(!is_valid_package_name("my package"));
        assert!(!is_valid_package_name("my.package"));
    }

    #[test]
    fn test_validate_version() {
        assert!(is_valid_version("1.0.0"));
        assert!(is_valid_version("0.1.2"));
        assert!(is_valid_version("10.20.30"));

        assert!(!is_valid_version("1.0"));
        assert!(!is_valid_version("1"));
        assert!(!is_valid_version("1.0.0.0"));
        assert!(!is_valid_version("v1.0.0"));
        assert!(!is_valid_version(""));
    }

    #[test]
    fn test_invalid_manifest_empty_name() {
        let toml = r#"
[package]
name = ""
version = "1.0.0"
"#;

        let result = PackageManifest::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_manifest_bad_version() {
        let toml = r#"
[package]
name = "pkg"
version = "1.0"
"#;

        let result = PackageManifest::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_dependency_multiple_sources_error() {
        let toml = r#"
[package]
name = "pkg"
version = "1.0.0"

[dependencies]
bad = { version = "^1.0.0", path = "../local" }
"#;

        let result = PackageManifest::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_jsx_config() {
        let toml = r#"
[package]
name = "my-app"
version = "1.0.0"

[jsx]
factory = "h"
fragment = "F"
factory_module = "preact"
development = true
"#;

        let manifest = PackageManifest::from_str(toml).unwrap();
        let jsx = manifest.jsx.unwrap();
        assert_eq!(jsx.factory, "h");
        assert_eq!(jsx.fragment, "F");
        assert_eq!(jsx.factory_module, Some("preact".to_string()));
        assert!(jsx.development);
    }

    #[test]
    fn test_parse_jsx_config_defaults() {
        let toml = r#"
[package]
name = "my-app"
version = "1.0.0"

[jsx]
"#;

        let manifest = PackageManifest::from_str(toml).unwrap();
        let jsx = manifest.jsx.unwrap();
        assert_eq!(jsx.factory, "createElement");
        assert_eq!(jsx.fragment, "Fragment");
        assert_eq!(jsx.factory_module, None);
        assert!(!jsx.development);
    }

    #[test]
    fn test_parse_no_jsx_config() {
        let toml = r#"
[package]
name = "my-app"
version = "1.0.0"
"#;

        let manifest = PackageManifest::from_str(toml).unwrap();
        assert!(manifest.jsx.is_none());
    }

    #[test]
    fn test_jsx_config_round_trip() {
        let config = JsxConfig {
            factory: "h".to_string(),
            fragment: "Fragment".to_string(),
            factory_module: Some("preact".to_string()),
            development: true,
        };
        let manifest = PackageManifest {
            package: PackageInfo {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                authors: Vec::new(),
                license: None,
                repository: None,
                homepage: None,
                main: None,
            },
            jsx: Some(config.clone()),
            dependencies: HashMap::new(),
            dev_dependencies: HashMap::new(),
        };

        let serialized = toml::to_string_pretty(&manifest).unwrap();
        let deserialized: PackageManifest = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.jsx.unwrap(), config);
    }
}
