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

    /// URL imports not yet supported
    #[error("URL imports not yet supported: {0}")]
    UrlNotSupported(String),
}

/// A resolved module with its absolute path and source
#[derive(Debug, Clone)]
pub struct ResolvedModule {
    /// Absolute path to the module file
    pub path: PathBuf,
    /// Whether this was resolved from an index file
    pub is_index: bool,
}

/// Module resolver for import specifiers
#[derive(Debug, Clone)]
pub struct ModuleResolver {
    /// Project root directory
    project_root: PathBuf,
}

impl ModuleResolver {
    /// Create a new module resolver
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
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
    /// * `specifier` - The import specifier (e.g., "./utils", "../lib/helper")
    /// * `from_file` - The file containing the import statement
    ///
    /// # Resolution Order
    /// For `import { x } from "./utils"`:
    /// 1. Try `./utils.raya`
    /// 2. Try `./utils/index.raya`
    pub fn resolve(&self, specifier: &str, from_file: &Path) -> Result<ResolvedModule, ResolveError> {
        // Check for URL imports
        if specifier.starts_with("http://") || specifier.starts_with("https://") {
            return Err(ResolveError::UrlNotSupported(specifier.to_string()));
        }

        // Check for local imports
        if specifier.starts_with("./") || specifier.starts_with("../") {
            return self.resolve_local(specifier, from_file);
        }

        // Package import
        Err(ResolveError::PackageNotSupported(specifier.to_string()))
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
                });
            }

            // Try 2: specifier/index.raya
            let index_path = base_path.join("index.raya");
            tried.push(index_path.clone());
            if index_path.exists() {
                return Ok(ResolvedModule {
                    path: self.canonicalize(&index_path)?,
                    is_index: true,
                });
            }
        } else {
            // Explicit .raya extension
            tried.push(base_path.clone());
            if base_path.exists() {
                return Ok(ResolvedModule {
                    path: self.canonicalize(&base_path)?,
                    is_index: false,
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
        let resolver = ModuleResolver::new(temp_dir.path().to_path_buf());
        (temp_dir, resolver)
    }

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
    fn test_package_import_not_supported() {
        let (temp_dir, resolver) = create_test_project();

        let main_file = temp_dir.path().join("main.raya");
        let result = resolver.resolve("logging", &main_file);

        assert!(matches!(result, Err(ResolveError::PackageNotSupported(_))));
    }

    #[test]
    fn test_url_import_not_supported() {
        let (temp_dir, resolver) = create_test_project();

        let main_file = temp_dir.path().join("main.raya");
        let result = resolver.resolve("https://example.com/mod.ryb", &main_file);

        assert!(matches!(result, Err(ResolveError::UrlNotSupported(_))));
    }
}
