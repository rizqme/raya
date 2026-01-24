//! Integration tests for path dependency resolution

use rpkg::{find_project_root, PathResolver};
use std::fs;
use tempfile::TempDir;

fn create_package(root: &std::path::Path, name: &str, version: &str) {
    fs::create_dir_all(root).unwrap();
    let manifest = format!(
        r#"[package]
name = "{}"
version = "{}"
"#,
        name, version
    );
    fs::write(root.join("raya.toml"), manifest).unwrap();
}

#[test]
fn test_resolve_relative_path() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create project structure
    create_package(root, "root", "1.0.0");
    create_package(&root.join("packages/app"), "app", "1.0.0");
    create_package(&root.join("packages/utils"), "utils", "1.0.0");

    let resolver = PathResolver::new(root.to_path_buf());
    let resolved = resolver
        .resolve("../utils", &root.join("packages/app"))
        .unwrap();

    assert!(resolved.ends_with("utils"));
    assert!(resolved.join("raya.toml").exists());
}

#[test]
fn test_resolve_from_root() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    create_package(root, "root", "1.0.0");
    create_package(&root.join("shared"), "shared", "1.0.0");

    let resolver = PathResolver::new(root.to_path_buf());
    let resolved = resolver.resolve_from_root("./shared").unwrap();

    assert!(resolved.ends_with("shared"));
}

#[test]
fn test_missing_manifest_error() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    create_package(root, "root", "1.0.0");

    // Create directory without raya.toml
    fs::create_dir_all(root.join("no-manifest")).unwrap();

    let resolver = PathResolver::new(root.to_path_buf());
    let result = resolver.resolve_from_root("./no-manifest");

    assert!(result.is_err());
}

#[test]
fn test_path_not_found_error() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    create_package(root, "root", "1.0.0");

    let resolver = PathResolver::new(root.to_path_buf());
    let result = resolver.resolve_from_root("./does-not-exist");

    assert!(result.is_err());
}

#[test]
fn test_validate_valid_path() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    create_package(root, "root", "1.0.0");

    let resolver = PathResolver::new(root.to_path_buf());
    assert!(resolver.validate(root).is_ok());
}

#[test]
fn test_validate_missing_manifest() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    create_package(root, "root", "1.0.0");

    let no_manifest = root.join("empty");
    fs::create_dir_all(&no_manifest).unwrap();

    let resolver = PathResolver::new(root.to_path_buf());
    let result = resolver.validate(&no_manifest);

    assert!(result.is_err());
}

#[test]
fn test_find_project_root() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    create_package(root, "root", "1.0.0");

    let nested = root.join("a/b/c");
    fs::create_dir_all(&nested).unwrap();

    let found = find_project_root(&nested).unwrap();
    assert_eq!(found, root);
}

#[test]
fn test_find_project_root_not_found() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // No raya.toml anywhere
    fs::create_dir_all(root).unwrap();

    let result = find_project_root(root);
    assert!(result.is_none());
}

#[test]
fn test_manifest_path() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    create_package(root, "root", "1.0.0");

    let resolver = PathResolver::new(root.to_path_buf());
    let manifest = resolver.manifest_path(root);

    assert_eq!(manifest, root.join("raya.toml"));
    assert!(manifest.exists());
}

#[test]
fn test_source_dir() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    create_package(root, "root", "1.0.0");

    let resolver = PathResolver::new(root.to_path_buf());
    let src = resolver.source_dir(root);

    assert_eq!(src, root.join("src"));
}

#[test]
fn test_make_relative() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    create_package(root, "root", "1.0.0");

    let dep_dir = root.join("packages/utils");
    fs::create_dir_all(&dep_dir).unwrap();

    let resolver = PathResolver::new(root.to_path_buf());
    let relative = resolver.make_relative(&dep_dir).unwrap();

    assert_eq!(relative, std::path::PathBuf::from("packages/utils"));
}

#[test]
fn test_nested_path_dependencies() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create nested structure
    create_package(root, "root", "1.0.0");
    create_package(&root.join("packages/a"), "a", "1.0.0");
    create_package(&root.join("packages/b"), "b", "1.0.0");
    create_package(&root.join("shared/utils"), "utils", "1.0.0");

    let resolver = PathResolver::new(root.to_path_buf());

    // Resolve from packages/a to packages/b
    let b_path = resolver.resolve("../b", &root.join("packages/a")).unwrap();
    assert!(b_path.ends_with("b"));

    // Resolve from packages/a to shared/utils
    let utils_path = resolver
        .resolve("../../shared/utils", &root.join("packages/a"))
        .unwrap();
    assert!(utils_path.ends_with("utils"));
}

#[test]
fn test_absolute_path_resolution() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    create_package(root, "root", "1.0.0");
    create_package(&root.join("lib"), "lib", "1.0.0");

    let resolver = PathResolver::new(root.to_path_buf());

    // Resolve absolute path
    let lib_absolute = root.join("lib");
    let resolved = resolver
        .resolve(
            lib_absolute.to_str().unwrap(),
            &root,
        )
        .unwrap();

    assert!(resolved.ends_with("lib"));
}
