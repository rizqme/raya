//! Install command
//!
//! Installs all dependencies from raya.toml and raya.lock.

use crate::cache::Cache;
use crate::lockfile::Lockfile;
use crate::manifest::{Dependency, PackageManifest};
use crate::path::find_project_root;
use crate::registry::{RegistryClient, RegistryError};
use crate::resolver::{DependencyResolver, PackageSource, ResolverError};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

/// Errors that can occur during installation
#[derive(Debug, Error)]
pub enum InstallError {
    /// No raya.toml found
    #[error("No raya.toml found. Run `raya init` to create a project.")]
    NoManifest,

    /// Manifest error
    #[error("Manifest error: {0}")]
    ManifestError(#[from] crate::manifest::ManifestError),

    /// Lockfile error
    #[error("Lockfile error: {0}")]
    LockfileError(#[from] crate::lockfile::LockfileError),

    /// Resolution error
    #[error("Resolution error: {0}")]
    ResolverError(#[from] ResolverError),

    /// Registry error
    #[error("Registry error: {0}")]
    RegistryError(#[from] RegistryError),

    /// Cache error
    #[error("Cache error: {0}")]
    CacheError(#[from] crate::cache::CacheError),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Install options
#[derive(Debug, Clone, Default)]
pub struct InstallOptions {
    /// Skip dev dependencies
    pub production: bool,

    /// Force re-download even if cached
    pub force: bool,

    /// Update to latest compatible versions
    pub update: bool,
}

/// Install result
#[derive(Debug)]
pub struct InstallResult {
    /// Number of packages installed
    pub installed: usize,

    /// Number of packages from cache
    pub cached: usize,

    /// Number of packages updated
    pub updated: usize,
}

/// Install all dependencies for a project
///
/// Reads raya.toml, resolves dependencies, downloads packages, and updates raya.lock.
pub fn install_dependencies(
    start_dir: Option<&Path>,
    options: InstallOptions,
) -> Result<InstallResult, InstallError> {
    // Find project root
    let project_root = match start_dir {
        Some(dir) => find_project_root(dir).ok_or(InstallError::NoManifest)?,
        None => find_project_root(&std::env::current_dir()?).ok_or(InstallError::NoManifest)?,
    };

    let manifest_path = project_root.join("raya.toml");
    if !manifest_path.exists() {
        return Err(InstallError::NoManifest);
    }

    // Load manifest
    let manifest = PackageManifest::from_file(&manifest_path)?;
    println!("Installing dependencies for {}...", manifest.package.name);

    // Load existing lockfile if present
    let lockfile_path = project_root.join("raya.lock");
    let existing_lockfile = if lockfile_path.exists() && !options.update {
        Some(Lockfile::from_file(&lockfile_path)?)
    } else {
        None
    };

    // Initialize cache
    let cache = Cache::init()?;

    // Initialize registry client
    let registry = RegistryClient::new()?;

    // Collect dependencies to install
    let mut deps_to_install: HashMap<String, &Dependency> = manifest
        .dependencies
        .iter()
        .map(|(k, v)| (k.clone(), v))
        .collect();
    if !options.production {
        deps_to_install.extend(manifest.dev_dependencies.iter().map(|(k, v)| (k.clone(), v)));
    }

    if deps_to_install.is_empty() {
        println!("No dependencies to install.");
        return Ok(InstallResult {
            installed: 0,
            cached: 0,
            updated: 0,
        });
    }

    let mut installed = 0;
    let mut cached = 0;
    let mut updated = 0;

    // Create resolver with available versions from registry
    let mut resolver = DependencyResolver::new(manifest.clone());
    if let Some(ref lock) = existing_lockfile {
        resolver = resolver.with_lockfile(lock.clone());
    }

    // Fetch available versions for each dependency
    for (name, dep) in &deps_to_install {
        // Skip path dependencies (they're compiled from source)
        if dep.is_path() {
            println!("  {} (path dependency)", name);
            continue;
        }

        // Skip git dependencies for now
        if dep.is_git() {
            println!("  {} (git dependency - not yet supported)", name);
            continue;
        }

        // Fetch versions from registry
        match registry.get_versions(name) {
            Ok(versions) => {
                resolver = resolver.with_available_versions(name.to_string(), versions);
            }
            Err(RegistryError::PackageNotFound(_)) => {
                // Check if it's in lockfile (offline mode)
                if let Some(ref lock) = existing_lockfile {
                    if lock.get_package(name).is_some() {
                        println!("  {} (using locked version, registry unavailable)", name);
                        continue;
                    }
                }
                return Err(InstallError::RegistryError(RegistryError::PackageNotFound(
                    name.to_string(),
                )));
            }
            Err(e) => return Err(e.into()),
        }
    }

    // Resolve dependencies
    let resolved = resolver.resolve()?;

    // Download and cache each resolved package
    for (name, pkg) in &resolved.packages {
        match &pkg.source {
            PackageSource::Path { path } => {
                println!("  {} -> {} (path)", name, path);
            }
            PackageSource::Git { url, rev } => {
                println!("  {}@{} -> {} (git)", name, rev, url);
            }
            PackageSource::Url { url } => {
                // URL imports are handled separately via URL cache
                println!("  {} -> {} (url)", name, url);
            }
            PackageSource::Registry { .. } => {
                let version_str = pkg.version.to_string();

                // Check if already in cache
                if let Some(ref lock) = existing_lockfile {
                    if let Some(locked) = lock.get_package(name) {
                        if locked.version == version_str && !options.force {
                            // Check cache
                            if let Ok(hash) = hex::decode(&locked.checksum) {
                                if hash.len() == 32 {
                                    let mut hash_arr = [0u8; 32];
                                    hash_arr.copy_from_slice(&hash);
                                    if cache.exists(&hash_arr) {
                                        println!("  {}@{} (cached)", name, version_str);
                                        cached += 1;
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                }

                // Download from registry
                println!("  Downloading {}@{}...", name, version_str);
                let cache_dir =
                    registry.download_to_cache(name, &version_str, cache.root())?;
                println!("    -> {}", cache_dir.display());

                if options.update {
                    updated += 1;
                } else {
                    installed += 1;
                }
            }
        }
    }

    // Generate and write lockfile
    let new_lockfile = resolved.to_lockfile(Some(manifest.package.name.clone()));

    // Update checksums in lockfile from actual downloads
    let mut lockfile = new_lockfile;
    for pkg in &mut lockfile.packages {
        if pkg.is_registry() {
            // Get actual checksum from version info
            if let Ok(info) = registry.get_version(&pkg.name, &pkg.version) {
                pkg.checksum = info.checksum;
            }
        }
    }

    lockfile.to_file(&lockfile_path)?;
    println!("\nUpdated raya.lock");

    Ok(InstallResult {
        installed,
        cached,
        updated,
    })
}

#[cfg(test)]
mod tests {
    // Integration tests would require a mock registry server
    // Unit tests for individual functions go here
}
