//! Path resolution for local dependencies
//!
//! Handles resolution of local filesystem paths for monorepo and local package support.

use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur during path resolution
#[derive(Debug, Error)]
pub enum PathError {
    /// Path does not exist
    #[error("Path does not exist: {0}")]
    PathNotFound(PathBuf),

    /// Path is not a directory
    #[error("Path is not a directory: {0}")]
    NotADirectory(PathBuf),

    /// Missing raya.toml manifest
    #[error("Missing raya.toml in path: {0}")]
    MissingManifest(PathBuf),

    /// I/O error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Invalid path (e.g., absolute when relative expected)
    #[error("Invalid path: {0}")]
    InvalidPath(String),

    /// Path traversal outside project root
    #[error("Path traversal outside project root: {0}")]
    PathTraversal(PathBuf),
}

/// Path dependency resolver
pub struct PathResolver {
    /// Project root directory
    project_root: PathBuf,
}

impl PathResolver {
    /// Create a new path resolver
    pub fn new(project_root: PathBuf) -> Self {
        // Canonicalize the project root to handle symlinks consistently
        let project_root = project_root
            .canonicalize()
            .unwrap_or(project_root);
        Self { project_root }
    }

    /// Resolve a relative path dependency
    ///
    /// # Arguments
    /// * `path` - Relative path from manifest location
    /// * `manifest_dir` - Directory containing the manifest file
    ///
    /// # Returns
    /// Absolute, canonicalized path to the dependency
    pub fn resolve(&self, path: &str, manifest_dir: &Path) -> Result<PathBuf, PathError> {
        // Parse the path
        let dep_path = Path::new(path);

        // Resolve relative to manifest directory
        let mut resolved = manifest_dir.join(dep_path);

        // Canonicalize the path
        resolved = resolved
            .canonicalize()
            .map_err(|_| PathError::PathNotFound(resolved.clone()))?;

        // Verify it's a directory
        if !resolved.is_dir() {
            return Err(PathError::NotADirectory(resolved));
        }

        // Check for raya.toml
        let manifest_path = resolved.join("raya.toml");
        if !manifest_path.exists() {
            return Err(PathError::MissingManifest(resolved));
        }

        // Verify it's within project bounds (prevent path traversal attacks)
        if !resolved.starts_with(&self.project_root) {
            return Err(PathError::PathTraversal(resolved));
        }

        Ok(resolved)
    }

    /// Resolve a path relative to the project root
    pub fn resolve_from_root(&self, path: &str) -> Result<PathBuf, PathError> {
        self.resolve(path, &self.project_root)
    }

    /// Check if a path exists and is a valid package directory
    pub fn validate(&self, path: &Path) -> Result<(), PathError> {
        if !path.exists() {
            return Err(PathError::PathNotFound(path.to_path_buf()));
        }

        if !path.is_dir() {
            return Err(PathError::NotADirectory(path.to_path_buf()));
        }

        let manifest_path = path.join("raya.toml");
        if !manifest_path.exists() {
            return Err(PathError::MissingManifest(path.to_path_buf()));
        }

        Ok(())
    }

    /// Get the manifest path for a package directory
    pub fn manifest_path(&self, package_dir: &Path) -> PathBuf {
        package_dir.join("raya.toml")
    }

    /// Get the source directory for a package
    pub fn source_dir(&self, package_dir: &Path) -> PathBuf {
        package_dir.join("src")
    }

    /// Normalize a path for cross-platform consistency
    pub fn normalize(path: &Path) -> PathBuf {
        let mut components = Vec::new();

        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    components.pop();
                }
                std::path::Component::CurDir => {}
                _ => components.push(component),
            }
        }

        components.iter().collect()
    }

    /// Convert an absolute path to a path relative to the project root
    pub fn make_relative(&self, absolute: &Path) -> Result<PathBuf, PathError> {
        // Canonicalize the input path to handle symlinks consistently
        let absolute = absolute
            .canonicalize()
            .map_err(|_| PathError::PathNotFound(absolute.to_path_buf()))?;

        absolute
            .strip_prefix(&self.project_root)
            .map(|p| p.to_path_buf())
            .map_err(|_| PathError::InvalidPath(format!("{:?} is not within project root", absolute)))
    }
}

/// Helper to find the project root by looking for raya.toml
pub fn find_project_root(start_dir: &Path) -> Option<PathBuf> {
    let mut current = start_dir;

    loop {
        let manifest = current.join("raya.toml");
        if manifest.exists() {
            return Some(current.to_path_buf());
        }

        current = current.parent()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_temp_project() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();

        // Create project structure
        fs::write(root.join("raya.toml"), "[package]\nname = \"root\"\nversion = \"1.0.0\"\n")
            .unwrap();

        (temp, root)
    }

    #[test]
    fn test_resolve_relative_path() {
        let (_temp, root) = create_temp_project();

        // Create both the app and utils directories
        let app_dir = root.join("packages").join("app");
        let utils_dir = root.join("packages").join("utils");

        fs::create_dir_all(&app_dir).unwrap();
        fs::create_dir_all(&utils_dir).unwrap();

        fs::write(
            utils_dir.join("raya.toml"),
            "[package]\nname = \"utils\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();

        let resolver = PathResolver::new(root.clone());
        let resolved = resolver.resolve("../utils", &app_dir).unwrap();

        assert!(resolved.ends_with("utils"));
        assert!(resolved.join("raya.toml").exists());
    }

    #[test]
    fn test_resolve_from_root() {
        let (_temp, root) = create_temp_project();

        // Create a local dependency
        let dep_dir = root.join("shared");
        fs::create_dir_all(&dep_dir).unwrap();
        fs::write(
            dep_dir.join("raya.toml"),
            "[package]\nname = \"shared\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();

        let resolver = PathResolver::new(root.clone());
        let resolved = resolver.resolve_from_root("./shared").unwrap();

        assert!(resolved.ends_with("shared"));
    }

    #[test]
    fn test_missing_manifest_error() {
        let (_temp, root) = create_temp_project();

        // Create directory without raya.toml
        let dep_dir = root.join("no-manifest");
        fs::create_dir_all(&dep_dir).unwrap();

        let resolver = PathResolver::new(root.clone());
        let result = resolver.resolve_from_root("./no-manifest");

        assert!(matches!(result, Err(PathError::MissingManifest(_))));
    }

    #[test]
    fn test_path_not_found_error() {
        let (_temp, root) = create_temp_project();

        let resolver = PathResolver::new(root.clone());
        let result = resolver.resolve_from_root("./does-not-exist");

        assert!(matches!(result, Err(PathError::PathNotFound(_))));
    }

    #[test]
    fn test_validate_valid_path() {
        let (_temp, root) = create_temp_project();

        let resolver = PathResolver::new(root.clone());
        assert!(resolver.validate(&root).is_ok());
    }

    #[test]
    fn test_validate_missing_manifest() {
        let (_temp, root) = create_temp_project();

        let no_manifest = root.join("empty");
        fs::create_dir_all(&no_manifest).unwrap();

        let resolver = PathResolver::new(root.clone());
        let result = resolver.validate(&no_manifest);

        assert!(matches!(result, Err(PathError::MissingManifest(_))));
    }

    #[test]
    fn test_normalize_path() {
        let path = Path::new("./foo/../bar/./baz");
        let normalized = PathResolver::normalize(path);

        assert_eq!(normalized, PathBuf::from("bar/baz"));
    }

    #[test]
    fn test_make_relative() {
        let (_temp, root) = create_temp_project();

        let dep_dir = root.join("packages").join("utils");
        fs::create_dir_all(&dep_dir).unwrap();

        let resolver = PathResolver::new(root.clone());
        let relative = resolver.make_relative(&dep_dir).unwrap();

        assert_eq!(relative, PathBuf::from("packages/utils"));
    }

    #[test]
    fn test_find_project_root() {
        let (_temp, root) = create_temp_project();

        let nested = root.join("a").join("b").join("c");
        fs::create_dir_all(&nested).unwrap();

        let found = find_project_root(&nested).unwrap();
        assert_eq!(found, root);
    }

    #[test]
    fn test_manifest_path() {
        let (_temp, root) = create_temp_project();
        let resolver = PathResolver::new(root.clone());

        let manifest = resolver.manifest_path(&root);
        assert_eq!(manifest, root.join("raya.toml"));
    }

    #[test]
    fn test_source_dir() {
        let (_temp, root) = create_temp_project();
        let resolver = PathResolver::new(root.clone());

        let src = resolver.source_dir(&root);
        assert_eq!(src, root.join("src"));
    }
}
