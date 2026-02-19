//! Dependency resolution from raya.toml manifests.

use raya_pm::{Dependency, PackageManifest};
use std::path::{Path, PathBuf};

use crate::error::RuntimeError;
use crate::loader;
use crate::CompiledModule;

/// Load all dependencies declared in a package manifest.
///
/// Resolves each dependency by type:
/// - Path dependencies: compiled from source or loaded as .ryb
/// - URL/git dependencies: loaded from cache (must be pre-installed)
/// - Registry packages: loaded from raya_packages/ (must be pre-installed)
pub fn load_dependencies(
    manifest: &PackageManifest,
    manifest_dir: &Path,
) -> Result<Vec<CompiledModule>, RuntimeError> {
    let mut deps = Vec::new();

    for (name, dep) in &manifest.dependencies {
        let module = load_dependency(name, dep, manifest_dir)?;
        deps.push(module);
    }

    Ok(deps)
}

fn load_dependency(
    name: &str,
    dep: &Dependency,
    manifest_dir: &Path,
) -> Result<CompiledModule, RuntimeError> {
    match dep {
        Dependency::Simple(_version) => load_registry_dep(name, manifest_dir),
        Dependency::Detailed {
            path: Some(path), ..
        } => load_path_dep(name, path, manifest_dir),
        Dependency::Detailed {
            git: Some(url), ..
        } => load_url_dep(name, url),
        Dependency::Detailed { version: Some(_), .. } => load_registry_dep(name, manifest_dir),
        _ => Err(RuntimeError::Dependency(format!(
            "Dependency '{}' has no path, git, or version specified.",
            name
        ))),
    }
}

/// Load a local path dependency.
fn load_path_dep(
    name: &str,
    path: &str,
    manifest_dir: &Path,
) -> Result<CompiledModule, RuntimeError> {
    let dep_dir = manifest_dir.join(path);
    let dep_dir = dep_dir.canonicalize().map_err(|_| {
        RuntimeError::Dependency(format!(
            "Path dependency '{}' not found at: {}",
            name,
            manifest_dir.join(path).display(),
        ))
    })?;

    loader::load_package_dir_pub(&dep_dir, name)
}

/// Load a URL/git dependency from cache.
fn load_url_dep(name: &str, url: &str) -> Result<CompiledModule, RuntimeError> {
    // Check raya_pm URL cache
    let cache = raya_pm::UrlCache::default_cache();
    if let Some(cached) = cache.is_cached(url, None) {
        if let Some(entry) = cache.find_entry_point(&cached) {
            return loader::load_entry_point_pub(&entry);
        }
    }

    // Check ~/.raya/cache/urls/ as fallback
    if let Some(home) = dirs::home_dir() {
        let cache_dir = home.join(".raya").join("cache").join("urls");
        if cache_dir.exists() {
            // Look for {name}.ryb in any cached directory
            if let Ok(entries) = std::fs::read_dir(&cache_dir) {
                for entry in entries.flatten() {
                    let ryb = entry.path().join(format!("{}.ryb", name));
                    if ryb.exists() {
                        return loader::load_bytecode_file(&ryb);
                    }
                }
            }
        }
    }

    Err(RuntimeError::Dependency(format!(
        "Dependency '{}' (git: {}) not installed.\nRun 'raya install' first.",
        name, url,
    )))
}

/// Load a registry package from raya_packages/ or global cache.
fn load_registry_dep(
    name: &str,
    manifest_dir: &Path,
) -> Result<CompiledModule, RuntimeError> {
    // 1. Project-local: raya_packages/{name}/
    let local = manifest_dir.join("raya_packages").join(name);
    if local.exists() {
        return loader::load_package_dir_pub(&local, name);
    }

    // 2. Global: ~/.raya/packages/{name}/
    if let Some(home) = dirs::home_dir() {
        let global = home.join(".raya").join("packages").join(name);
        if global.exists() {
            return loader::load_package_dir_pub(&global, name);
        }
    }

    Err(RuntimeError::Dependency(format!(
        "Package '{}' not installed.\nRun 'raya install' first.",
        name,
    )))
}

/// Find the project root by walking up from a path to find raya.toml.
pub fn find_manifest_dir(start: &Path) -> Option<PathBuf> {
    let mut dir = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        if dir.join("raya.toml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}
