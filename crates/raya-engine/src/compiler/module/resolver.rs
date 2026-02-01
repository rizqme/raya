//! Module path resolution
//!
//! Handles resolving import specifiers to absolute file paths.

use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur during module resolution
#[derive(Debug, Error, Clone)]
pub enum ResolveError {
    /// Local file not found
    #[error("Module not found: {path} (tried: {tried:?})")]
    ModuleNotFound { path: String, tried: Vec<PathBuf> },

    /// Current file has no parent directory
    #[error("Cannot resolve import: current file has no parent directory")]
    NoParentDirectory,

    /// IO error during resolution
    #[error("IO error: {0}")]
    IoError(String),

    /// Package imports not yet supported
    #[error("Package imports not yet supported: {0}")]
    PackageNotSupported(String),

    /// URL fetch failed
    #[error("Failed to fetch URL '{url}': {error}")]
    UrlFetchError { url: String, error: String },

    /// URL not in lockfile
    #[error("URL not in lockfile (run with --fetch to download): {0}")]
    UrlNotLocked(String),

    /// URL checksum mismatch
    #[error("URL checksum mismatch for '{url}': expected {expected}, got {actual}")]
    UrlChecksumMismatch {
        url: String,
        expected: String,
        actual: String,
    },

    /// Package not found in dependencies
    #[error("Package '{0}' not found in raya.toml dependencies")]
    PackageNotInDependencies(String),

    /// Package not in cache
    #[error("Package '{name}@{version}' not found in cache (run `raya install` first)")]
    PackageNotCached { name: String, version: String },

    /// Package version constraint invalid
    #[error("Invalid version constraint for '{package}': {error}")]
    InvalidVersionConstraint { package: String, error: String },
}

/// A resolved module with its absolute path and source
#[derive(Debug, Clone)]
pub struct ResolvedModule {
    /// Absolute path to the module file
    pub path: PathBuf,
    /// Whether this was resolved from an index file
    pub is_index: bool,
    /// Package source info (if this is a package import)
    pub package_info: Option<ResolvedPackageInfo>,
    /// URL source info (if this is a URL import)
    pub url_info: Option<ResolvedUrlInfo>,
}

impl ResolvedModule {
    /// Get the path to the type definition file (.d.raya)
    /// Returns None for local modules (they have source), Some for cached packages
    pub fn typedef_path(&self) -> Option<PathBuf> {
        if self.package_info.is_none() {
            // Local module - no separate type definitions
            return None;
        }

        // For cached packages, look for module.d.raya alongside module.ryb
        let parent = self.path.parent()?;
        let typedef_path = parent.join("module.d.raya");

        if typedef_path.exists() {
            Some(typedef_path)
        } else {
            None
        }
    }

    /// Check if this module has type definitions available
    pub fn has_typedef(&self) -> bool {
        self.typedef_path().is_some()
    }

    /// Get the path to the package manifest (raya.toml)
    /// Returns None for local modules, Some for cached packages
    pub fn manifest_path(&self) -> Option<PathBuf> {
        if self.package_info.is_none() {
            return None;
        }

        let parent = self.path.parent()?;
        let manifest_path = parent.join("raya.toml");

        if manifest_path.exists() {
            Some(manifest_path)
        } else {
            None
        }
    }

    /// Get the path to the README file
    /// Returns None if no README exists
    pub fn readme_path(&self) -> Option<PathBuf> {
        if self.package_info.is_none() {
            return None;
        }

        let parent = self.path.parent()?;
        let readme_path = parent.join("README.md");

        if readme_path.exists() {
            Some(readme_path)
        } else {
            None
        }
    }

    /// Get the cache directory for this package
    pub fn cache_dir(&self) -> Option<&Path> {
        if self.package_info.is_some() {
            self.path.parent()
        } else {
            None
        }
    }
}

/// Information about a resolved package import
#[derive(Debug, Clone)]
pub struct ResolvedPackageInfo {
    /// Package name
    pub name: String,
    /// Resolved version
    pub version: String,
    /// Cache checksum (SHA-256 hash)
    pub checksum: [u8; 32],
}

/// Information about a resolved URL import
#[derive(Debug, Clone)]
pub struct ResolvedUrlInfo {
    /// Original URL
    pub url: String,
    /// Cache checksum (SHA-256 hash)
    pub checksum: [u8; 32],
}

/// Parsed package specifier
#[derive(Debug, Clone, PartialEq)]
pub struct PackageSpecifier {
    /// Package name (e.g., "logging" or "@org/logging")
    pub name: String,
    /// Optional version constraint (e.g., "1.2.0", "^1.0.0")
    pub version: Option<String>,
}

impl PackageSpecifier {
    /// Parse a package specifier string
    ///
    /// Supports formats:
    /// - `"logging"` - package name only
    /// - `"logging@1.2.0"` - exact version
    /// - `"logging@^1.0.0"` - version constraint
    /// - `"@org/pkg"` - scoped package
    /// - `"@org/pkg@1.0.0"` - scoped package with version
    pub fn parse(specifier: &str) -> Self {
        let specifier = specifier.trim();

        // Handle scoped packages (@org/pkg)
        if specifier.starts_with('@') {
            // Find the second @ for version (if any)
            if let Some(first_slash) = specifier.find('/') {
                let after_slash = &specifier[first_slash + 1..];
                if let Some(at_pos) = after_slash.find('@') {
                    let name = specifier[..first_slash + 1 + at_pos].to_string();
                    let version = after_slash[at_pos + 1..].to_string();
                    return Self {
                        name,
                        version: Some(version),
                    };
                }
            }
            // No version specified for scoped package
            return Self {
                name: specifier.to_string(),
                version: None,
            };
        }

        // Handle regular packages (pkg or pkg@version)
        if let Some(at_pos) = specifier.find('@') {
            Self {
                name: specifier[..at_pos].to_string(),
                version: Some(specifier[at_pos + 1..].to_string()),
            }
        } else {
            Self {
                name: specifier.to_string(),
                version: None,
            }
        }
    }

    /// Check if this specifier has an explicit version
    pub fn has_version(&self) -> bool {
        self.version.is_some()
    }
}

/// Module resolver for import specifiers
#[derive(Debug, Clone)]
pub struct ModuleResolver {
    /// Project root directory
    project_root: PathBuf,
    /// Package resolver (optional, for package imports)
    package_resolver: Option<PackageResolverConfig>,
}

/// Configuration for package resolution
#[derive(Debug, Clone)]
pub struct PackageResolverConfig {
    /// Path to raya.toml
    pub manifest_path: PathBuf,
    /// Path to raya.lock (if exists)
    pub lockfile_path: Option<PathBuf>,
    /// Global cache directory (~/.raya/cache/)
    pub cache_dir: PathBuf,
}

impl PackageResolverConfig {
    /// Create package resolver config from project root
    pub fn from_project_root(project_root: &Path) -> Option<Self> {
        let manifest_path = project_root.join("raya.toml");
        if !manifest_path.exists() {
            return None;
        }

        let lockfile_path = {
            let path = project_root.join("raya.lock");
            if path.exists() { Some(path) } else { None }
        };

        let cache_dir = dirs::home_dir()
            .map(|h| h.join(".raya").join("cache"))
            .unwrap_or_else(|| project_root.join(".raya").join("cache"));

        Some(Self {
            manifest_path,
            lockfile_path,
            cache_dir,
        })
    }
}

impl ModuleResolver {
    /// Create a new module resolver
    pub fn new(project_root: PathBuf) -> Self {
        let package_resolver = PackageResolverConfig::from_project_root(&project_root);
        Self { project_root, package_resolver }
    }

    /// Create a module resolver without package resolution
    pub fn local_only(project_root: PathBuf) -> Self {
        Self { project_root, package_resolver: None }
    }

    /// Create a module resolver with the current directory as project root
    pub fn current_dir() -> Result<Self, ResolveError> {
        let project_root = std::env::current_dir()
            .map_err(|e| ResolveError::IoError(e.to_string()))?;
        Ok(Self::new(project_root))
    }

    /// Resolve an import specifier to an absolute path
    ///
    /// # Arguments
    /// * `specifier` - The import specifier (e.g., "./utils", "../lib/helper", "logging", "https://...")
    /// * `from_file` - The file containing the import statement
    ///
    /// # Resolution Order
    /// For `import { x } from "./utils"`:
    /// 1. Try `./utils.raya`
    /// 2. Try `./utils/index.raya`
    ///
    /// For `import { x } from "logging"`:
    /// 1. Check raya.toml for dependency
    /// 2. Check raya.lock for exact version
    /// 3. Look up in ~/.raya/cache/
    ///
    /// For `import { x } from "https://..."`:
    /// 1. Check raya.lock for cached hash
    /// 2. Look up in ~/.raya/cache/<hash>/
    ///
    /// For `import { x } from "my-local-pkg"` (with path dependency in raya.toml):
    /// 1. Check raya.toml for `my-local-pkg = { path = "./local/path" }`
    /// 2. Resolve to local package entry point
    pub fn resolve(&self, specifier: &str, from_file: &Path) -> Result<ResolvedModule, ResolveError> {
        // Check for HTTP/HTTPS URL imports
        if specifier.starts_with("http://") || specifier.starts_with("https://") {
            return self.resolve_url(specifier);
        }

        // Check for local imports
        if specifier.starts_with("./") || specifier.starts_with("../") {
            return self.resolve_local(specifier, from_file);
        }

        // Package import (includes path dependencies from raya.toml)
        self.resolve_package(specifier)
    }

    /// Resolve a URL import
    fn resolve_url(&self, url: &str) -> Result<ResolvedModule, ResolveError> {
        // Check if package resolver is available (needed for lockfile access)
        let config = self.package_resolver.as_ref()
            .ok_or_else(|| ResolveError::UrlNotLocked(url.to_string()))?;

        // Load lockfile to find cached URL
        let lockfile = config.lockfile_path.as_ref()
            .and_then(|p| raya_pm::Lockfile::from_file(p).ok());

        let lockfile = lockfile
            .ok_or_else(|| ResolveError::UrlNotLocked(url.to_string()))?;

        // Find the URL in lockfile
        let locked = lockfile.packages.iter()
            .find(|p| matches!(&p.source, raya_pm::Source::Url { url: u } if u == url))
            .ok_or_else(|| ResolveError::UrlNotLocked(url.to_string()))?;

        // Convert checksum
        let checksum_bytes = hex::decode(&locked.checksum)
            .map_err(|_| ResolveError::IoError(format!("Invalid checksum format: {}", locked.checksum)))?;

        let mut checksum_arr = [0u8; 32];
        if checksum_bytes.len() != 32 {
            return Err(ResolveError::IoError(format!("Invalid checksum length: {}", locked.checksum.len())));
        }
        checksum_arr.copy_from_slice(&checksum_bytes);

        // Look up in cache
        let cache_dir = config.cache_dir.join(&locked.checksum);

        // Find entry point in cache
        let entry_point = self.find_url_entry_point(&cache_dir)
            .ok_or_else(|| ResolveError::IoError(format!(
                "URL '{}' is cached but has no entry point",
                url
            )))?;

        Ok(ResolvedModule {
            path: entry_point,
            is_index: false,
            package_info: None,
            url_info: Some(ResolvedUrlInfo {
                url: url.to_string(),
                checksum: checksum_arr,
            }),
        })
    }

    /// Find entry point for a cached URL
    fn find_url_entry_point(&self, cache_dir: &Path) -> Option<PathBuf> {
        if !cache_dir.exists() {
            return None;
        }

        // Check for compiled bytecode
        let ryb_path = cache_dir.join("module.ryb");
        if ryb_path.exists() {
            return Some(ryb_path);
        }

        // Check for raya.toml to find main entry
        let manifest_path = cache_dir.join("raya.toml");
        if manifest_path.exists() {
            if let Ok(manifest) = raya_pm::PackageManifest::from_file(&manifest_path) {
                if let Some(main) = manifest.package.main {
                    let entry = cache_dir.join(&main);
                    if entry.exists() {
                        return Some(entry);
                    }
                }
            }
        }

        // Default entry points
        let candidates = [
            cache_dir.join("src/index.raya"),
            cache_dir.join("index.raya"),
            cache_dir.join("src/main.raya"),
            cache_dir.join("main.raya"),
        ];

        for candidate in &candidates {
            if candidate.exists() {
                return Some(candidate.clone());
            }
        }

        None
    }

    /// Resolve a package import
    fn resolve_package(&self, specifier: &str) -> Result<ResolvedModule, ResolveError> {
        let pkg = PackageSpecifier::parse(specifier);

        // Check if package resolver is available
        let config = self.package_resolver.as_ref()
            .ok_or_else(|| ResolveError::PackageNotSupported(
                format!("{} (no raya.toml found)", specifier)
            ))?;

        // Try to resolve from manifest and lockfile
        self.resolve_package_from_config(&pkg, config)
    }

    /// Resolve package using config (manifest, lockfile, cache)
    fn resolve_package_from_config(
        &self,
        pkg: &PackageSpecifier,
        config: &PackageResolverConfig,
    ) -> Result<ResolvedModule, ResolveError> {
        use raya_pm::{PackageManifest, Lockfile};

        // 1. Load manifest and check if package is a dependency
        let manifest = PackageManifest::from_file(&config.manifest_path)
            .map_err(|e| ResolveError::IoError(format!("Failed to read raya.toml: {}", e)))?;

        let dep = manifest.dependencies.get(&pkg.name)
            .or_else(|| manifest.dev_dependencies.get(&pkg.name))
            .ok_or_else(|| ResolveError::PackageNotInDependencies(pkg.name.clone()))?;

        // 2. Check if this is a path dependency (compile from source)
        if let Some(path) = dep.path() {
            let full_path = self.project_root.join(&path);
            let entry_point = self.find_package_entry_point(&full_path)?;
            return Ok(ResolvedModule {
                path: entry_point,
                is_index: false,
                package_info: None, // Local path, no cache info
                url_info: None,
                });
        }

        // 3. Load lockfile to get exact version and checksum
        let lockfile = config.lockfile_path.as_ref()
            .and_then(|p| Lockfile::from_file(p).ok());

        let (version, checksum) = if let Some(ref lock) = lockfile {
            if let Some(locked) = lock.get_package(&pkg.name) {
                (locked.version.clone(), locked.checksum.clone())
            } else {
                return Err(ResolveError::PackageNotCached {
                    name: pkg.name.clone(),
                    version: pkg.version.clone().unwrap_or_else(|| "latest".to_string()),
                });
            }
        } else {
            return Err(ResolveError::PackageNotCached {
                name: pkg.name.clone(),
                version: pkg.version.clone().unwrap_or_else(|| "latest".to_string()),
            });
        };

        // 4. Look up in cache
        let checksum_bytes = hex::decode(&checksum)
            .map_err(|_| ResolveError::IoError(format!("Invalid checksum format: {}", checksum)))?;

        let mut checksum_arr = [0u8; 32];
        if checksum_bytes.len() != 32 {
            return Err(ResolveError::IoError(format!("Invalid checksum length: {}", checksum.len())));
        }
        checksum_arr.copy_from_slice(&checksum_bytes);

        let cache_package_dir = config.cache_dir.join(&checksum);
        let module_path = cache_package_dir.join("module.ryb");

        // Verify required files exist
        if !module_path.exists() {
            return Err(ResolveError::PackageNotCached {
                name: pkg.name.clone(),
                version: version.clone(),
            });
        }

        // Check for required type definitions
        let typedef_path = cache_package_dir.join("module.d.raya");
        if !typedef_path.exists() {
            return Err(ResolveError::IoError(format!(
                "Package '{}@{}' is missing type definitions (module.d.raya)",
                pkg.name, version
            )));
        }

        // Check for required manifest
        let manifest_path = cache_package_dir.join("raya.toml");
        if !manifest_path.exists() {
            return Err(ResolveError::IoError(format!(
                "Package '{}@{}' is missing manifest (raya.toml)",
                pkg.name, version
            )));
        }

        Ok(ResolvedModule {
            path: module_path,
            is_index: false,
            package_info: Some(ResolvedPackageInfo {
                name: pkg.name.clone(),
                version,
                checksum: checksum_arr,
            }),
            url_info: None,
        })
    }

    /// Find the entry point for a local package
    fn find_package_entry_point(&self, package_path: &Path) -> Result<PathBuf, ResolveError> {
        // Check for raya.toml to find main entry point
        let manifest_path = package_path.join("raya.toml");
        if manifest_path.exists() {
            if let Ok(manifest) = raya_pm::PackageManifest::from_file(&manifest_path) {
                if let Some(main) = manifest.package.main {
                    let entry = package_path.join(&main);
                    if entry.exists() {
                        return self.canonicalize(&entry);
                    }
                }
            }
        }

        // Default: src/index.raya or index.raya
        let candidates = [
            package_path.join("src/index.raya"),
            package_path.join("index.raya"),
            package_path.join("src/main.raya"),
            package_path.join("main.raya"),
        ];

        for candidate in &candidates {
            if candidate.exists() {
                return self.canonicalize(candidate);
            }
        }

        Err(ResolveError::ModuleNotFound {
            path: format!("{} (no entry point)", package_path.display()),
            tried: candidates.to_vec(),
        })
    }

    /// Check if package resolution is available
    pub fn has_package_resolver(&self) -> bool {
        self.package_resolver.is_some()
    }

    /// Resolve a local import (./path or ../path)
    fn resolve_local(&self, specifier: &str, from_file: &Path) -> Result<ResolvedModule, ResolveError> {
        let from_dir = from_file.parent()
            .ok_or(ResolveError::NoParentDirectory)?;

        let mut tried = Vec::new();

        // Build the base path
        let base_path = from_dir.join(specifier);

        // Try 1: specifier.raya (if no extension)
        if !specifier.ends_with(".raya") {
            let with_ext = base_path.with_extension("raya");
            tried.push(with_ext.clone());
            if with_ext.exists() {
                return Ok(ResolvedModule {
                    path: self.canonicalize(&with_ext)?,
                    is_index: false,
                    package_info: None,
                    url_info: None,
                        });
            }

            // Try 2: specifier/index.raya
            let index_path = base_path.join("index.raya");
            tried.push(index_path.clone());
            if index_path.exists() {
                return Ok(ResolvedModule {
                    path: self.canonicalize(&index_path)?,
                    is_index: true,
                    package_info: None,
                    url_info: None,
                        });
            }
        } else {
            // Explicit .raya extension
            tried.push(base_path.clone());
            if base_path.exists() {
                return Ok(ResolvedModule {
                    path: self.canonicalize(&base_path)?,
                    is_index: false,
                    package_info: None,
                    url_info: None,
                        });
            }
        }

        Err(ResolveError::ModuleNotFound {
            path: specifier.to_string(),
            tried,
        })
    }

    /// Canonicalize a path to absolute form
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, ResolveError> {
        path.canonicalize()
            .map_err(|e| ResolveError::IoError(format!("Failed to canonicalize {}: {}", path.display(), e)))
    }

    /// Get the project root
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_project() -> (TempDir, ModuleResolver) {
        let temp_dir = TempDir::new().unwrap();
        let resolver = ModuleResolver::local_only(temp_dir.path().to_path_buf());
        (temp_dir, resolver)
    }

    // ==========================================================
    // PackageSpecifier parsing tests
    // ==========================================================

    #[test]
    fn test_parse_simple_package() {
        let spec = PackageSpecifier::parse("logging");
        assert_eq!(spec.name, "logging");
        assert_eq!(spec.version, None);
    }

    #[test]
    fn test_parse_package_with_exact_version() {
        let spec = PackageSpecifier::parse("logging@1.2.3");
        assert_eq!(spec.name, "logging");
        assert_eq!(spec.version, Some("1.2.3".to_string()));
    }

    #[test]
    fn test_parse_package_with_caret_version() {
        let spec = PackageSpecifier::parse("http@^2.0.0");
        assert_eq!(spec.name, "http");
        assert_eq!(spec.version, Some("^2.0.0".to_string()));
    }

    #[test]
    fn test_parse_package_with_tilde_version() {
        let spec = PackageSpecifier::parse("utils@~1.5.0");
        assert_eq!(spec.name, "utils");
        assert_eq!(spec.version, Some("~1.5.0".to_string()));
    }

    #[test]
    fn test_parse_scoped_package() {
        let spec = PackageSpecifier::parse("@org/my-package");
        assert_eq!(spec.name, "@org/my-package");
        assert_eq!(spec.version, None);
    }

    #[test]
    fn test_parse_scoped_package_with_version() {
        let spec = PackageSpecifier::parse("@raya/stdlib@1.0.0");
        assert_eq!(spec.name, "@raya/stdlib");
        assert_eq!(spec.version, Some("1.0.0".to_string()));
    }

    #[test]
    fn test_parse_scoped_package_with_caret() {
        let spec = PackageSpecifier::parse("@company/internal@^3.0.0");
        assert_eq!(spec.name, "@company/internal");
        assert_eq!(spec.version, Some("^3.0.0".to_string()));
    }

    #[test]
    fn test_parse_package_with_whitespace() {
        let spec = PackageSpecifier::parse("  logging@1.0.0  ");
        assert_eq!(spec.name, "logging");
        assert_eq!(spec.version, Some("1.0.0".to_string()));
    }

    // ==========================================================
    // Local import resolution tests
    // ==========================================================

    #[test]
    fn test_resolve_local_with_extension() {
        let (temp_dir, resolver) = create_test_project();

        // Create src/main.raya and src/utils.raya
        let src_dir = temp_dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("main.raya"), "import { foo } from \"./utils\";").unwrap();
        fs::write(src_dir.join("utils.raya"), "export function foo() {}").unwrap();

        let main_file = src_dir.join("main.raya");
        let resolved = resolver.resolve("./utils", &main_file).unwrap();

        assert_eq!(resolved.path, src_dir.join("utils.raya").canonicalize().unwrap());
        assert!(!resolved.is_index);
        assert!(resolved.package_info.is_none());
    }

    #[test]
    fn test_resolve_local_index_file() {
        let (temp_dir, resolver) = create_test_project();

        // Create src/main.raya and src/lib/index.raya
        let src_dir = temp_dir.path().join("src");
        let lib_dir = src_dir.join("lib");
        fs::create_dir_all(&lib_dir).unwrap();
        fs::write(src_dir.join("main.raya"), "import { bar } from \"./lib\";").unwrap();
        fs::write(lib_dir.join("index.raya"), "export function bar() {}").unwrap();

        let main_file = src_dir.join("main.raya");
        let resolved = resolver.resolve("./lib", &main_file).unwrap();

        assert_eq!(resolved.path, lib_dir.join("index.raya").canonicalize().unwrap());
        assert!(resolved.is_index);
    }

    #[test]
    fn test_resolve_parent_directory() {
        let (temp_dir, resolver) = create_test_project();

        // Create src/nested/module.raya and src/shared.raya
        let src_dir = temp_dir.path().join("src");
        let nested_dir = src_dir.join("nested");
        fs::create_dir_all(&nested_dir).unwrap();
        fs::write(nested_dir.join("module.raya"), "import { x } from \"../shared\";").unwrap();
        fs::write(src_dir.join("shared.raya"), "export const x = 42;").unwrap();

        let module_file = nested_dir.join("module.raya");
        let resolved = resolver.resolve("../shared", &module_file).unwrap();

        assert_eq!(resolved.path, src_dir.join("shared.raya").canonicalize().unwrap());
    }

    #[test]
    fn test_resolve_module_not_found() {
        let (temp_dir, resolver) = create_test_project();

        let src_dir = temp_dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("main.raya"), "import { x } from \"./missing\";").unwrap();

        let main_file = src_dir.join("main.raya");
        let result = resolver.resolve("./missing", &main_file);

        assert!(matches!(result, Err(ResolveError::ModuleNotFound { .. })));
    }

    #[test]
    fn test_package_import_without_manifest() {
        let (temp_dir, resolver) = create_test_project();

        let main_file = temp_dir.path().join("main.raya");
        let result = resolver.resolve("logging", &main_file);

        // Without raya.toml, package imports are not supported
        assert!(matches!(result, Err(ResolveError::PackageNotSupported(_))));
    }

    #[test]
    fn test_url_import_not_locked() {
        let (temp_dir, resolver) = create_test_project();

        let main_file = temp_dir.path().join("main.raya");
        let result = resolver.resolve("https://example.com/mod.ryb", &main_file);

        // Without lockfile, URL imports fail with UrlNotLocked
        assert!(matches!(result, Err(ResolveError::UrlNotLocked(_))));
    }

    #[test]
    fn test_url_import_from_lockfile() {
        let temp_dir = TempDir::new().unwrap();

        // Create manifest
        let manifest = r#"
[package]
name = "test-project"
version = "1.0.0"
"#;
        fs::write(temp_dir.path().join("raya.toml"), manifest).unwrap();

        // Create lockfile with URL entry
        let checksum = "a".repeat(64);
        let lockfile = format!(r#"
version = 1

[[packages]]
name = "remote-lib"
version = "1.0.0"
checksum = "{}"
source = {{ type = "url", url = "https://example.com/lib.tar.gz" }}
"#, checksum);
        fs::write(temp_dir.path().join("raya.lock"), lockfile).unwrap();

        // Create cache directory with entry point
        let cache_dir = dirs::home_dir()
            .map(|h| h.join(".raya").join("cache"))
            .unwrap_or_else(|| temp_dir.path().join(".raya").join("cache"))
            .join(&checksum);
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(cache_dir.join("main.raya"), "export let x = 42;").unwrap();

        let resolver = ModuleResolver::new(temp_dir.path().to_path_buf());

        let main_file = temp_dir.path().join("main.raya");
        let result = resolver.resolve("https://example.com/lib.tar.gz", &main_file);

        // Should resolve to the cached entry point
        assert!(result.is_ok(), "Expected Ok, got {:?}", result);
        let resolved = result.unwrap();
        assert!(resolved.url_info.is_some());
        assert_eq!(resolved.url_info.as_ref().unwrap().url, "https://example.com/lib.tar.gz");
        assert!(resolved.path.ends_with("main.raya"));

        // Clean up cache directory
        let _ = fs::remove_dir_all(&cache_dir);
    }

    // ==========================================================
    // Package import resolution tests
    // ==========================================================

    #[test]
    fn test_package_import_path_dependency() {
        let temp_dir = TempDir::new().unwrap();

        // Create main project with raya.toml
        let manifest = r#"
[package]
name = "main-project"
version = "1.0.0"

[dependencies]
utils = { path = "./utils" }
"#;
        fs::write(temp_dir.path().join("raya.toml"), manifest).unwrap();

        // Create utils package
        let utils_dir = temp_dir.path().join("utils");
        fs::create_dir_all(&utils_dir).unwrap();
        let utils_manifest = r#"
[package]
name = "utils"
version = "1.0.0"
main = "src/index.raya"
"#;
        fs::write(utils_dir.join("raya.toml"), utils_manifest).unwrap();
        fs::create_dir_all(utils_dir.join("src")).unwrap();
        fs::write(utils_dir.join("src/index.raya"), "export let x = 42;").unwrap();

        // Create resolver with package support
        let resolver = ModuleResolver::new(temp_dir.path().to_path_buf());

        let main_file = temp_dir.path().join("main.raya");
        let resolved = resolver.resolve("utils", &main_file).unwrap();

        assert!(resolved.path.ends_with("src/index.raya"));
        assert!(resolved.package_info.is_none()); // Path deps don't have cache info
    }

    #[test]
    fn test_package_import_not_in_dependencies() {
        let temp_dir = TempDir::new().unwrap();

        // Create project with raya.toml but without the requested dependency
        let manifest = r#"
[package]
name = "main-project"
version = "1.0.0"

[dependencies]
"#;
        fs::write(temp_dir.path().join("raya.toml"), manifest).unwrap();

        let resolver = ModuleResolver::new(temp_dir.path().to_path_buf());

        let main_file = temp_dir.path().join("main.raya");
        let result = resolver.resolve("logging", &main_file);

        assert!(matches!(result, Err(ResolveError::PackageNotInDependencies(_))));
    }

    #[test]
    fn test_package_resolver_config() {
        let temp_dir = TempDir::new().unwrap();

        // Without raya.toml
        let config = PackageResolverConfig::from_project_root(temp_dir.path());
        assert!(config.is_none());

        // With raya.toml
        fs::write(temp_dir.path().join("raya.toml"), "[package]\nname = \"test\"\nversion = \"1.0.0\"").unwrap();
        let config = PackageResolverConfig::from_project_root(temp_dir.path());
        assert!(config.is_some());

        let config = config.unwrap();
        assert_eq!(config.manifest_path, temp_dir.path().join("raya.toml"));
        assert!(config.lockfile_path.is_none());
    }

    #[test]
    fn test_package_resolver_config_with_lockfile() {
        let temp_dir = TempDir::new().unwrap();

        fs::write(temp_dir.path().join("raya.toml"), "[package]\nname = \"test\"\nversion = \"1.0.0\"").unwrap();
        fs::write(temp_dir.path().join("raya.lock"), "version = 1\n").unwrap();

        let config = PackageResolverConfig::from_project_root(temp_dir.path()).unwrap();
        assert!(config.lockfile_path.is_some());
    }

    // ==========================================================
    // Path dependency tests (local folder → package name mapping)
    // ==========================================================

    #[test]
    fn test_path_dependency_absolute_path() {
        let temp_dir = TempDir::new().unwrap();

        // Create a local lib package outside the project
        let lib_dir = temp_dir.path().join("external-libs").join("my-lib");
        fs::create_dir_all(lib_dir.join("src")).unwrap();
        fs::write(lib_dir.join("src/index.raya"), "export let x = 42;").unwrap();

        // Create main project with path dependency using absolute path
        let project_dir = temp_dir.path().join("my-project");
        fs::create_dir_all(&project_dir).unwrap();

        let manifest = format!(r#"
[package]
name = "my-project"
version = "1.0.0"

[dependencies]
my-lib = {{ path = "{}" }}
"#, lib_dir.display());
        fs::write(project_dir.join("raya.toml"), manifest).unwrap();

        let resolver = ModuleResolver::new(project_dir.clone());

        let main_file = project_dir.join("main.raya");
        let result = resolver.resolve("my-lib", &main_file);

        assert!(result.is_ok(), "Expected Ok, got {:?}", result);
        let resolved = result.unwrap();
        assert!(resolved.path.ends_with("src/index.raya"));
        assert!(resolved.package_info.is_none()); // Path deps don't have cache info
    }

    #[test]
    fn test_path_dependency_with_custom_main() {
        let temp_dir = TempDir::new().unwrap();

        // Create a local lib with custom entry point
        let lib_dir = temp_dir.path().join("libs").join("custom-lib");
        fs::create_dir_all(lib_dir.join("lib")).unwrap();

        let lib_manifest = r#"
[package]
name = "custom-lib"
version = "1.0.0"
main = "lib/entry.raya"
"#;
        fs::write(lib_dir.join("raya.toml"), lib_manifest).unwrap();
        fs::write(lib_dir.join("lib/entry.raya"), "export function helper() {}").unwrap();

        // Create main project
        let project_dir = temp_dir.path().join("project");
        fs::create_dir_all(&project_dir).unwrap();

        let manifest = r#"
[package]
name = "project"
version = "1.0.0"

[dependencies]
custom-lib = { path = "../libs/custom-lib" }
"#;
        fs::write(project_dir.join("raya.toml"), manifest).unwrap();

        let resolver = ModuleResolver::new(project_dir.clone());

        let main_file = project_dir.join("main.raya");
        let result = resolver.resolve("custom-lib", &main_file);

        assert!(result.is_ok(), "Expected Ok, got {:?}", result);
        let resolved = result.unwrap();
        assert!(resolved.path.ends_with("lib/entry.raya"));
    }

    #[test]
    fn test_multiple_path_dependencies() {
        let temp_dir = TempDir::new().unwrap();

        // Create libs directory with multiple packages
        let libs_dir = temp_dir.path().join("libs");

        // lib-a
        let lib_a = libs_dir.join("lib-a");
        fs::create_dir_all(&lib_a).unwrap();
        fs::write(lib_a.join("index.raya"), "export let a = 1;").unwrap();

        // lib-b
        let lib_b = libs_dir.join("lib-b");
        fs::create_dir_all(&lib_b).unwrap();
        fs::write(lib_b.join("index.raya"), "export let b = 2;").unwrap();

        // Create main project with both dependencies
        let project_dir = temp_dir.path().join("project");
        fs::create_dir_all(&project_dir).unwrap();

        let manifest = r#"
[package]
name = "project"
version = "1.0.0"

[dependencies]
lib-a = { path = "../libs/lib-a" }
lib-b = { path = "../libs/lib-b" }
"#;
        fs::write(project_dir.join("raya.toml"), manifest).unwrap();

        let resolver = ModuleResolver::new(project_dir.clone());
        let main_file = project_dir.join("main.raya");

        // Test lib-a
        let result_a = resolver.resolve("lib-a", &main_file);
        assert!(result_a.is_ok(), "Expected Ok for lib-a, got {:?}", result_a);
        assert!(result_a.unwrap().path.ends_with("index.raya"));

        // Test lib-b
        let result_b = resolver.resolve("lib-b", &main_file);
        assert!(result_b.is_ok(), "Expected Ok for lib-b, got {:?}", result_b);
        assert!(result_b.unwrap().path.ends_with("index.raya"));
    }
}
