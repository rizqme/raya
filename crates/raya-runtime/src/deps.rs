//! Dependency resolution from raya.toml or package.json manifests.

use raya_pm::{Dependency, Lockfile, PackageManifest};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::RuntimeError;
use crate::loader;
use crate::CompiledModule;
use raya_engine::semantics::SemanticProfile;

/// Load all dependencies declared in a package manifest.
///
/// Resolves each dependency by type:
/// - Path dependencies: compiled from source or loaded as .ryb
/// - URL/git dependencies: loaded from cache (must be pre-installed)
/// - Registry packages: loaded from .raya/packages/ (preferred) or raya_packages/ (legacy)
pub fn load_dependencies(
    manifest: &PackageManifest,
    manifest_dir: &Path,
) -> Result<Vec<CompiledModule>, RuntimeError> {
    let mut deps = Vec::new();
    let lock_profiles = load_lockfile_semantic_profiles(manifest_dir);

    for (name, dep) in &manifest.dependencies {
        let module = load_dependency(name, dep, manifest_dir, lock_profiles.get(name).copied())?;
        deps.push(module);
    }

    Ok(deps)
}

/// Load dependencies declared in package.json (dependencies only).
pub fn load_dependencies_from_package_json(
    manifest_dir: &Path,
) -> Result<Vec<CompiledModule>, RuntimeError> {
    let package_json_path = manifest_dir.join("package.json");
    let content = std::fs::read_to_string(&package_json_path).map_err(|e| {
        RuntimeError::Dependency(format!(
            "Failed to read {}: {}",
            package_json_path.display(),
            e
        ))
    })?;
    let value: JsonValue = serde_json::from_str(&content).map_err(|e| {
        RuntimeError::Dependency(format!(
            "Failed to parse {}: {}",
            package_json_path.display(),
            e
        ))
    })?;

    let mut deps = Vec::new();
    let lock_profiles = load_lockfile_semantic_profiles(manifest_dir);
    if let Some(obj) = value.get("dependencies").and_then(|v| v.as_object()) {
        for name in obj.keys() {
            deps.push(load_registry_dep(
                name,
                manifest_dir,
                lock_profiles.get(name).copied(),
            )?);
        }
    }
    Ok(deps)
}

fn parse_semantic_profile(raw: &str) -> Option<SemanticProfile> {
    match raw {
        "raya" => Some(SemanticProfile::raya()),
        "ts" => Some(SemanticProfile::ts_strict()),
        "js" => Some(SemanticProfile::js()),
        _ => None,
    }
}

fn load_lockfile_semantic_profiles(manifest_dir: &Path) -> HashMap<String, SemanticProfile> {
    let mut out = HashMap::new();
    let lock_path = manifest_dir.join("raya.lock");
    if !lock_path.exists() {
        return out;
    }
    let lock = match Lockfile::from_file(&lock_path) {
        Ok(v) => v,
        Err(_) => return out,
    };
    for pkg in lock.packages {
        if let Some(profile) = pkg
            .semantic_profile
            .as_deref()
            .and_then(parse_semantic_profile)
        {
            out.insert(pkg.name, profile);
        }
    }
    out
}

fn load_dependency(
    name: &str,
    dep: &Dependency,
    manifest_dir: &Path,
    forced_profile: Option<SemanticProfile>,
) -> Result<CompiledModule, RuntimeError> {
    match dep {
        Dependency::Simple(_version) => load_registry_dep(name, manifest_dir, forced_profile),
        Dependency::Detailed {
            path: Some(path), ..
        } => load_path_dep(name, path, manifest_dir, forced_profile),
        Dependency::Detailed { git: Some(url), .. } => load_url_dep(name, url, forced_profile),
        Dependency::Detailed {
            version: Some(_), ..
        } => load_registry_dep(name, manifest_dir, forced_profile),
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
    forced_profile: Option<SemanticProfile>,
) -> Result<CompiledModule, RuntimeError> {
    let dep_dir = manifest_dir.join(path);
    let dep_dir = dep_dir.canonicalize().map_err(|_| {
        RuntimeError::Dependency(format!(
            "Path dependency '{}' not found at: {}",
            name,
            manifest_dir.join(path).display(),
        ))
    })?;

    loader::load_package_dir_with_profile_pub(&dep_dir, name, forced_profile)
}

/// Load a URL/git dependency from cache.
fn load_url_dep(
    name: &str,
    url: &str,
    forced_profile: Option<SemanticProfile>,
) -> Result<CompiledModule, RuntimeError> {
    // Check raya_pm URL cache
    let cache = raya_pm::UrlCache::default_cache();
    if let Some(cached) = cache.is_cached(url, None) {
        if let Some(entry) = cache.find_entry_point(&cached) {
            return loader::load_entry_point_with_profile_pub(&entry, forced_profile);
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

/// Load a registry package from local project cache or global cache.
fn load_registry_dep(
    name: &str,
    manifest_dir: &Path,
    forced_profile: Option<SemanticProfile>,
) -> Result<CompiledModule, RuntimeError> {
    // 1. Project-local: .raya/packages/{name}/
    let local = manifest_dir.join(".raya").join("packages").join(name);
    if local.exists() {
        return loader::load_package_dir_with_profile_pub(&local, name, forced_profile);
    }

    // 2. Legacy project-local: raya_packages/{name}/
    let local = manifest_dir.join("raya_packages").join(name);
    if local.exists() {
        return loader::load_package_dir_with_profile_pub(&local, name, forced_profile);
    }

    // 3. Global: ~/.raya/packages/{name}/
    if let Some(home) = dirs::home_dir() {
        let global = home.join(".raya").join("packages").join(name);
        if global.exists() {
            return loader::load_package_dir_with_profile_pub(&global, name, forced_profile);
        }
    }

    Err(RuntimeError::Dependency(format!(
        "Package '{}' not installed.\nRun 'raya install' first.",
        name,
    )))
}

/// Find the project root by walking up from a path to find package.json or raya.toml.
pub fn find_manifest_dir(start: &Path) -> Option<PathBuf> {
    let mut dir = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        if dir.join("package.json").exists() || dir.join("raya.toml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parses_lockfile_semantic_profile_strings() {
        assert_eq!(
            parse_semantic_profile("raya"),
            Some(SemanticProfile::raya())
        );
        assert_eq!(
            parse_semantic_profile("ts"),
            Some(SemanticProfile::ts_strict())
        );
        assert_eq!(parse_semantic_profile("js"), Some(SemanticProfile::js()));
        assert_eq!(parse_semantic_profile("unknown"), None);
    }

    #[test]
    fn finds_manifest_directory_from_nested_file() {
        let temp = TempDir::new().expect("temp dir");
        let project = temp.path().join("project");
        let src = project.join("src");
        std::fs::create_dir_all(&src).expect("create src");
        std::fs::write(project.join("raya.toml"), "[package]\nname = \"demo\"\n")
            .expect("write manifest");
        let nested_file = src.join("main.raya");
        std::fs::write(&nested_file, "return 1;").expect("write file");

        let found = find_manifest_dir(&nested_file).expect("manifest dir");
        assert_eq!(found, project);
    }
}
