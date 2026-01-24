//! Import resolution for module specifiers
//!
//! Handles parsing and resolving different types of module imports:
//! - Local file imports: `./utils.raya`, `../lib/helper.raya`
//! - Package imports: `logging@1.2.3`, `@org/package@^2.0.0`
//! - URL imports: `https://example.com/module.rbin`

use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur during import resolution
#[derive(Debug, Error)]
pub enum ImportError {
    /// Invalid import specifier format
    #[error("Invalid import specifier: {0}")]
    InvalidSpecifier(String),

    /// Local file not found
    #[error("Local file not found: {0}")]
    FileNotFound(PathBuf),

    /// Package not found in cache or registry
    #[error("Package not found: {0}")]
    PackageNotFound(String),

    /// Invalid URL format
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// Path resolution error
    #[error("Path resolution error: {0}")]
    PathResolution(String),
}

/// Type of import specifier
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportSpec {
    /// Local file import (./utils.raya, ../lib/helper.raya)
    Local(PathBuf),

    /// Package import (logging@1.2.3, @org/package@^2.0.0)
    Package {
        /// Package name (may include scope like @org/package)
        name: String,
        /// Version constraint (e.g., "1.2.3", "^1.0.0", "~2.1.0")
        version: Option<String>,
    },

    /// URL import (https://example.com/module.rbin)
    Url(String),
}

/// Import resolver for module specifiers
#[derive(Debug, Clone)]
pub struct ImportResolver {
    /// Project root directory (for resolving relative paths)
    project_root: PathBuf,
}

impl ImportResolver {
    /// Create a new import resolver
    ///
    /// # Arguments
    /// * `project_root` - Root directory of the project (for resolving relative imports)
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }

    /// Parse an import specifier into its type
    ///
    /// # Arguments
    /// * `spec` - The import specifier string
    ///
    /// # Returns
    /// * `Ok(ImportSpec)` - Parsed import specification
    /// * `Err(ImportError)` - Invalid specifier
    ///
    /// # Examples
    /// ```
    /// # use raya_core::module::{ImportResolver, ImportSpec};
    /// # use std::path::PathBuf;
    /// let resolver = ImportResolver::new(PathBuf::from("/project"));
    ///
    /// // Local import
    /// let spec = resolver.parse_specifier("./utils.raya").unwrap();
    /// assert!(matches!(spec, ImportSpec::Local(_)));
    ///
    /// // Package import
    /// let spec = resolver.parse_specifier("logging@1.2.3").unwrap();
    /// assert!(matches!(spec, ImportSpec::Package { .. }));
    ///
    /// // URL import
    /// let spec = resolver.parse_specifier("https://example.com/mod.rbin").unwrap();
    /// assert!(matches!(spec, ImportSpec::Url(_)));
    /// ```
    pub fn parse_specifier(&self, spec: &str) -> Result<ImportSpec, ImportError> {
        // Check for URL import (starts with http:// or https://)
        if spec.starts_with("http://") || spec.starts_with("https://") {
            return Ok(ImportSpec::Url(spec.to_string()));
        }

        // Check for local import (starts with ./ or ../)
        if spec.starts_with("./") || spec.starts_with("../") {
            let path = PathBuf::from(spec);
            return Ok(ImportSpec::Local(path));
        }

        // Otherwise, it's a package import
        // Format: name@version or just name
        // For scoped packages like @org/name@1.0.0, we need to skip the first @ if it's at position 0
        let at_pos = if spec.starts_with('@') {
            // Scoped package - find @ after the first one
            spec[1..].rfind('@').map(|pos| pos + 1)
        } else {
            // Regular package - find any @
            spec.rfind('@')
        };

        if let Some(at_pos) = at_pos {
            // Has version specifier
            let name = spec[..at_pos].to_string();
            let version = spec[at_pos + 1..].to_string();

            if name.is_empty() {
                return Err(ImportError::InvalidSpecifier(
                    "Empty package name".to_string(),
                ));
            }

            Ok(ImportSpec::Package {
                name,
                version: Some(version),
            })
        } else {
            // No version specifier
            if spec.is_empty() {
                return Err(ImportError::InvalidSpecifier(
                    "Empty import specifier".to_string(),
                ));
            }

            Ok(ImportSpec::Package {
                name: spec.to_string(),
                version: None,
            })
        }
    }

    /// Resolve a local import path to an absolute path
    ///
    /// # Arguments
    /// * `path` - Relative path from the import
    /// * `current_file` - Path to the file containing the import
    ///
    /// # Returns
    /// * `Ok(PathBuf)` - Resolved absolute path
    /// * `Err(ImportError)` - Resolution failed
    ///
    /// # Examples
    /// ```no_run
    /// # use raya_core::module::ImportResolver;
    /// # use std::path::PathBuf;
    /// let resolver = ImportResolver::new(PathBuf::from("/project"));
    /// let current = PathBuf::from("/project/src/main.raya");
    ///
    /// let resolved = resolver.resolve_local("./utils.raya", &current).unwrap();
    /// assert_eq!(resolved, PathBuf::from("/project/src/utils.raya"));
    /// ```
    pub fn resolve_local(&self, path: &str, current_file: &Path) -> Result<PathBuf, ImportError> {
        // Get the directory containing the current file
        let current_dir = current_file.parent().ok_or_else(|| {
            ImportError::PathResolution("Current file has no parent directory".to_string())
        })?;

        // Resolve the relative path
        let resolved = current_dir.join(path);

        // Canonicalize to get absolute path and resolve .. components
        resolved
            .canonicalize()
            .map_err(|e| ImportError::PathResolution(format!("Failed to canonicalize path: {}", e)))
    }

    /// Resolve a package import to its location in the cache
    ///
    /// This is a placeholder implementation. In Phase 3, this will:
    /// 1. Check the global cache (~/.raya/cache/)
    /// 2. Look up the package in the registry
    /// 3. Download if needed
    ///
    /// # Arguments
    /// * `name` - Package name
    /// * `version` - Optional version constraint
    ///
    /// # Returns
    /// * `Ok(PathBuf)` - Path to the cached module
    /// * `Err(ImportError)` - Package not found or resolution failed
    pub fn resolve_package(
        &self,
        name: &str,
        version: Option<&str>,
    ) -> Result<PathBuf, ImportError> {
        // TODO: Phase 3 - Implement cache lookup
        // For now, return an error
        Err(ImportError::PackageNotFound(format!(
            "{}{}",
            name,
            version.map(|v| format!("@{}", v)).unwrap_or_default()
        )))
    }

    /// Resolve a URL import to its cached location
    ///
    /// This is a placeholder implementation. In Phase 3, this will:
    /// 1. Check if already cached
    /// 2. Download if needed
    /// 3. Verify checksum
    ///
    /// # Arguments
    /// * `url` - URL to the module
    ///
    /// # Returns
    /// * `Ok(PathBuf)` - Path to the cached module
    /// * `Err(ImportError)` - Download or caching failed
    pub fn resolve_url(&self, url: &str) -> Result<PathBuf, ImportError> {
        // TODO: Phase 3 - Implement URL download and caching
        // For now, return an error
        Err(ImportError::InvalidUrl(format!(
            "URL imports not yet supported: {}",
            url
        )))
    }

    /// Get the project root
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_resolver() -> ImportResolver {
        ImportResolver::new(PathBuf::from("/project"))
    }

    #[test]
    fn test_parse_local_import() {
        let resolver = test_resolver();

        let spec = resolver.parse_specifier("./utils.raya").unwrap();
        assert!(matches!(spec, ImportSpec::Local(_)));

        let spec = resolver.parse_specifier("../lib/helper.raya").unwrap();
        assert!(matches!(spec, ImportSpec::Local(_)));
    }

    #[test]
    fn test_parse_package_import() {
        let resolver = test_resolver();

        // Package with version
        let spec = resolver.parse_specifier("logging@1.2.3").unwrap();
        match spec {
            ImportSpec::Package { name, version } => {
                assert_eq!(name, "logging");
                assert_eq!(version, Some("1.2.3".to_string()));
            }
            _ => panic!("Expected Package import"),
        }

        // Package without version
        let spec = resolver.parse_specifier("logging").unwrap();
        match spec {
            ImportSpec::Package { name, version } => {
                assert_eq!(name, "logging");
                assert_eq!(version, None);
            }
            _ => panic!("Expected Package import"),
        }

        // Scoped package
        let spec = resolver.parse_specifier("@org/package@^2.0.0").unwrap();
        match spec {
            ImportSpec::Package { name, version } => {
                assert_eq!(name, "@org/package");
                assert_eq!(version, Some("^2.0.0".to_string()));
            }
            _ => panic!("Expected Package import"),
        }
    }

    #[test]
    fn test_parse_url_import() {
        let resolver = test_resolver();

        let spec = resolver
            .parse_specifier("https://example.com/module.rbin")
            .unwrap();
        match spec {
            ImportSpec::Url(url) => {
                assert_eq!(url, "https://example.com/module.rbin");
            }
            _ => panic!("Expected URL import"),
        }

        let spec = resolver
            .parse_specifier("http://example.com/module.rbin")
            .unwrap();
        assert!(matches!(spec, ImportSpec::Url(_)));
    }

    #[test]
    fn test_invalid_specifier() {
        let resolver = test_resolver();

        let result = resolver.parse_specifier("");
        assert!(result.is_err());
    }

    #[test]
    fn test_package_name_with_at_symbol() {
        let resolver = test_resolver();

        // Scoped package with @ at start should still work
        let spec = resolver.parse_specifier("@scope/name@1.0.0").unwrap();
        match spec {
            ImportSpec::Package { name, version } => {
                assert_eq!(name, "@scope/name");
                assert_eq!(version, Some("1.0.0".to_string()));
            }
            _ => panic!("Expected Package import"),
        }
    }
}
