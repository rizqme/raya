//! Add command
//!
//! Adds a new dependency to raya.toml and installs it.

use crate::manifest::{Dependency, PackageManifest};
use crate::path::find_project_root;
use crate::registry::{RegistryClient, RegistryError};
use crate::semver::{Constraint, Version};
use std::path::Path;
use thiserror::Error;

/// Errors that can occur when adding a package
#[derive(Debug, Error)]
pub enum AddError {
    /// No raya.toml found
    #[error("No raya.toml found. Run `rpkg init` to create a project.")]
    NoManifest,

    /// Manifest error
    #[error("Manifest error: {0}")]
    ManifestError(#[from] crate::manifest::ManifestError),

    /// Registry error
    #[error("Registry error: {0}")]
    RegistryError(#[from] RegistryError),

    /// Invalid package specifier
    #[error("Invalid package specifier: {0}")]
    InvalidSpecifier(String),

    /// Semver error
    #[error("Semver error: {0}")]
    SemverError(#[from] crate::semver::SemverError),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Install error
    #[error("Install error: {0}")]
    InstallError(#[from] super::install::InstallError),
}

/// Options for adding a package
#[derive(Debug, Clone, Default)]
pub struct AddOptions {
    /// Add as dev dependency
    pub dev: bool,

    /// Exact version (no caret prefix)
    pub exact: bool,

    /// Skip installation after adding
    pub no_install: bool,
}

/// Parsed package specifier
#[derive(Debug)]
struct PackageSpecifier {
    /// Package name
    name: String,

    /// Version constraint (if specified)
    version: Option<String>,
}

impl PackageSpecifier {
    /// Parse a package specifier string
    ///
    /// Formats:
    /// - `package` - latest version
    /// - `package@1.2.3` - exact version
    /// - `package@^1.2.0` - version constraint
    fn parse(spec: &str) -> Result<Self, AddError> {
        let spec = spec.trim();

        if spec.is_empty() {
            return Err(AddError::InvalidSpecifier("empty specifier".to_string()));
        }

        // Handle scoped packages (@org/name@version)
        if spec.starts_with('@') {
            // Find the second @ (version separator)
            let rest = &spec[1..];
            if let Some(at_pos) = rest.find('@') {
                let name = &spec[..at_pos + 1];
                let version = &rest[at_pos + 1..];
                return Ok(PackageSpecifier {
                    name: name.to_string(),
                    version: Some(version.to_string()),
                });
            }
            // No version specified
            return Ok(PackageSpecifier {
                name: spec.to_string(),
                version: None,
            });
        }

        // Regular package
        if let Some(at_pos) = spec.find('@') {
            let name = &spec[..at_pos];
            let version = &spec[at_pos + 1..];

            if name.is_empty() {
                return Err(AddError::InvalidSpecifier(format!(
                    "empty package name in '{}'",
                    spec
                )));
            }

            if version.is_empty() {
                return Err(AddError::InvalidSpecifier(format!(
                    "empty version in '{}'",
                    spec
                )));
            }

            Ok(PackageSpecifier {
                name: name.to_string(),
                version: Some(version.to_string()),
            })
        } else {
            Ok(PackageSpecifier {
                name: spec.to_string(),
                version: None,
            })
        }
    }
}

/// Add a package to the project
///
/// Adds the package to raya.toml and optionally installs it.
pub fn add_package(
    package_spec: &str,
    start_dir: Option<&Path>,
    options: AddOptions,
) -> Result<(), AddError> {
    // Find project root
    let project_root = match start_dir {
        Some(dir) => find_project_root(dir).ok_or(AddError::NoManifest)?,
        None => find_project_root(&std::env::current_dir()?).ok_or(AddError::NoManifest)?,
    };

    let manifest_path = project_root.join("raya.toml");
    if !manifest_path.exists() {
        return Err(AddError::NoManifest);
    }

    // Parse package specifier
    let spec = PackageSpecifier::parse(package_spec)?;
    println!("Adding {}...", spec.name);

    // Load manifest
    let mut manifest = PackageManifest::from_file(&manifest_path)?;

    // Determine version constraint
    let version_constraint = if let Some(v) = spec.version {
        // Validate the version/constraint
        if v.starts_with('^') || v.starts_with('~') || v.starts_with('>') || v.starts_with('<') {
            // Already a constraint
            Constraint::parse(&v)?;
            v
        } else {
            // Treat as exact version
            Version::parse(&v)?;
            if options.exact {
                v
            } else {
                format!("^{}", v)
            }
        }
    } else {
        // Fetch latest version from registry
        let registry = RegistryClient::new()?;
        let versions = registry.get_versions(&spec.name)?;

        if versions.is_empty() {
            return Err(AddError::RegistryError(RegistryError::PackageNotFound(
                spec.name.clone(),
            )));
        }

        // Get latest non-prerelease version
        let latest = versions
            .iter()
            .find(|v| v.prerelease.is_none())
            .or(versions.first())
            .ok_or_else(|| RegistryError::PackageNotFound(spec.name.clone()))?;

        if options.exact {
            latest.to_string()
        } else {
            format!("^{}", latest)
        }
    };

    // Add to appropriate dependency section
    let dep = Dependency::Simple(version_constraint.clone());

    if options.dev {
        manifest.dev_dependencies.insert(spec.name.clone(), dep);
        println!("  Added {} = \"{}\" to dev-dependencies", spec.name, version_constraint);
    } else {
        manifest.dependencies.insert(spec.name.clone(), dep);
        println!("  Added {} = \"{}\" to dependencies", spec.name, version_constraint);
    }

    // Write updated manifest
    manifest.to_file(&manifest_path)?;

    // Install if requested
    if !options.no_install {
        println!("\nInstalling...");
        super::install::install_dependencies(
            Some(&project_root),
            super::install::InstallOptions::default(),
        )?;
    }

    Ok(())
}

/// Remove a package from the project
pub fn remove_package(
    package_name: &str,
    start_dir: Option<&Path>,
) -> Result<(), AddError> {
    // Find project root
    let project_root = match start_dir {
        Some(dir) => find_project_root(dir).ok_or(AddError::NoManifest)?,
        None => find_project_root(&std::env::current_dir()?).ok_or(AddError::NoManifest)?,
    };

    let manifest_path = project_root.join("raya.toml");
    if !manifest_path.exists() {
        return Err(AddError::NoManifest);
    }

    // Load manifest
    let mut manifest = PackageManifest::from_file(&manifest_path)?;

    // Remove from dependencies
    let removed_from_deps = manifest.dependencies.remove(package_name).is_some();
    let removed_from_dev = manifest.dev_dependencies.remove(package_name).is_some();

    if !removed_from_deps && !removed_from_dev {
        println!("Package '{}' not found in dependencies", package_name);
        return Ok(());
    }

    // Write updated manifest
    manifest.to_file(&manifest_path)?;

    if removed_from_deps {
        println!("Removed {} from dependencies", package_name);
    }
    if removed_from_dev {
        println!("Removed {} from dev-dependencies", package_name);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_specifier() {
        let spec = PackageSpecifier::parse("logging").unwrap();
        assert_eq!(spec.name, "logging");
        assert!(spec.version.is_none());
    }

    #[test]
    fn test_parse_versioned_specifier() {
        let spec = PackageSpecifier::parse("logging@1.2.3").unwrap();
        assert_eq!(spec.name, "logging");
        assert_eq!(spec.version, Some("1.2.3".to_string()));
    }

    #[test]
    fn test_parse_constraint_specifier() {
        let spec = PackageSpecifier::parse("logging@^1.2.0").unwrap();
        assert_eq!(spec.name, "logging");
        assert_eq!(spec.version, Some("^1.2.0".to_string()));
    }

    #[test]
    fn test_parse_scoped_package() {
        let spec = PackageSpecifier::parse("@org/package").unwrap();
        assert_eq!(spec.name, "@org/package");
        assert!(spec.version.is_none());
    }

    #[test]
    fn test_parse_scoped_package_with_version() {
        let spec = PackageSpecifier::parse("@org/package@1.0.0").unwrap();
        assert_eq!(spec.name, "@org/package");
        assert_eq!(spec.version, Some("1.0.0".to_string()));
    }

    #[test]
    fn test_parse_empty_specifier() {
        let result = PackageSpecifier::parse("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_missing_name() {
        let result = PackageSpecifier::parse("@1.0.0");
        // This is parsed as a scoped package without version
        assert!(result.is_ok());
    }
}
