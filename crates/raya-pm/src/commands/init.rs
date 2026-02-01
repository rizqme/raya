//! Project initialization command
//!
//! Creates a new Raya project with raya.toml.

use crate::manifest::{PackageInfo, PackageManifest};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use thiserror::Error;

/// Errors that can occur during project initialization
#[derive(Debug, Error)]
pub enum InitError {
    /// Project already exists
    #[error("Project already exists: raya.toml found in {0}")]
    AlreadyExists(String),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Manifest error
    #[error("Manifest error: {0}")]
    ManifestError(#[from] crate::manifest::ManifestError),
}

/// Initialize a new Raya project
///
/// Creates a raya.toml file in the specified directory.
pub fn init_project(dir: &Path, name: Option<&str>) -> Result<(), InitError> {
    let manifest_path = dir.join("raya.toml");

    // Check if project already exists
    if manifest_path.exists() {
        return Err(InitError::AlreadyExists(dir.display().to_string()));
    }

    // Derive package name from directory name if not provided
    let package_name = name
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            dir.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("my-package")
                .to_string()
        });

    // Create manifest
    let manifest = PackageManifest {
        package: PackageInfo {
            name: package_name.clone(),
            version: "0.1.0".to_string(),
            description: Some(format!("A Raya project: {}", package_name)),
            authors: Vec::new(),
            license: Some("MIT".to_string()),
            repository: None,
            homepage: None,
            main: Some("src/main.raya".to_string()),
        },
        dependencies: HashMap::new(),
        dev_dependencies: HashMap::new(),
    };

    // Create directory if it doesn't exist
    fs::create_dir_all(dir)?;

    // Write manifest
    manifest.to_file(&manifest_path)?;

    // Create src directory with main.raya
    let src_dir = dir.join("src");
    fs::create_dir_all(&src_dir)?;

    let main_path = src_dir.join("main.raya");
    if !main_path.exists() {
        fs::write(
            &main_path,
            r#"// Main entry point

function main(): void {
    console.log("Hello, Raya!");
}

main();
"#,
        )?;
    }

    println!("Created new Raya project: {}", package_name);
    println!("  - raya.toml");
    println!("  - src/main.raya");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_project() {
        let temp_dir = tempfile::tempdir().unwrap();
        let project_dir = temp_dir.path().join("test-project");

        init_project(&project_dir, Some("my-test")).unwrap();

        // Check files exist
        assert!(project_dir.join("raya.toml").exists());
        assert!(project_dir.join("src/main.raya").exists());

        // Check manifest content
        let manifest = PackageManifest::from_file(&project_dir.join("raya.toml")).unwrap();
        assert_eq!(manifest.package.name, "my-test");
        assert_eq!(manifest.package.version, "0.1.0");
    }

    #[test]
    fn test_init_already_exists() {
        let temp_dir = tempfile::tempdir().unwrap();
        let project_dir = temp_dir.path().join("existing");

        // First init should succeed
        init_project(&project_dir, None).unwrap();

        // Second init should fail
        let result = init_project(&project_dir, None);
        assert!(matches!(result, Err(InitError::AlreadyExists(_))));
    }
}
