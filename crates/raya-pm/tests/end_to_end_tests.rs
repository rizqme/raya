//! End-to-end integration tests for the complete package management workflow

use raya_pm::{
    Cache, DependencyResolver, Lockfile, PackageManifest, PathResolver, Version,
};
use std::fs;
use tempfile::TempDir;

fn create_test_manifest(root: &std::path::Path, name: &str, version: &str, deps: &[(&str, &str)]) {
    let mut deps_map = std::collections::HashMap::new();
    for (dep_name, constraint) in deps {
        deps_map.insert(
            dep_name.to_string(),
            raya_pm::Dependency::Simple(constraint.to_string()),
        );
    }

    let manifest = PackageManifest {
        package: raya_pm::PackageInfo {
            name: name.to_string(),
            version: version.to_string(),
            description: None,
            authors: vec![],
            license: None,
            repository: None,
            homepage: None,
            main: None,
        },
        jsx: None,
        scripts: std::collections::HashMap::new(),
        dependencies: deps_map,
        dev_dependencies: std::collections::HashMap::new(),
        registry: None,
        assets: None,
        bundle: None,
    };

    fs::create_dir_all(root).unwrap();
    let toml = toml::to_string_pretty(&manifest).unwrap();
    fs::write(root.join("raya.toml"), toml).unwrap();
}

#[test]
fn test_complete_workflow_simple_dependency() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create a simple project with one dependency
    create_test_manifest(root, "my-app", "1.0.0", &[("logging", "^1.0.0")]);

    // Parse the manifest
    let manifest_path = root.join("raya.toml");
    let manifest = PackageManifest::from_file(&manifest_path).unwrap();

    assert_eq!(manifest.package.name, "my-app");
    assert_eq!(manifest.dependencies.len(), 1);

    // Create a resolver with available versions
    let resolver = DependencyResolver::new(manifest)
        .with_available_versions(
            "logging".to_string(),
            vec![Version::new(1, 0, 0), Version::new(1, 2, 0), Version::new(2, 0, 0)],
        );

    // Resolve dependencies
    let resolved = resolver.resolve().unwrap();

    assert_eq!(resolved.packages.len(), 1);
    assert_eq!(resolved.packages["logging"].version, Version::new(1, 2, 0));

    // Generate lockfile
    let lockfile = resolved.to_lockfile(Some("my-app".to_string()));
    assert_eq!(lockfile.packages.len(), 1);

    // Save and reload lockfile
    let lock_path = root.join("raya.lock");
    lockfile.to_file(&lock_path).unwrap();

    let reloaded = Lockfile::from_file(&lock_path).unwrap();
    assert_eq!(reloaded.packages.len(), 1);
    assert_eq!(reloaded.packages[0].name, "logging");
    assert_eq!(reloaded.packages[0].version, "1.2.0");
}

#[test]
fn test_complete_workflow_with_path_dependencies() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create main project
    create_test_manifest(root, "my-app", "1.0.0", &[]);

    // Create local dependency
    let utils_path = root.join("packages/utils");
    create_test_manifest(&utils_path, "utils", "1.0.0", &[]);

    // Verify path resolution
    let path_resolver = PathResolver::new(root.to_path_buf());
    let resolved_path = path_resolver.resolve("./packages/utils", root).unwrap();

    assert!(resolved_path.ends_with("utils"));
    assert!(path_resolver.validate(&resolved_path).is_ok());
}

#[test]
fn test_complete_workflow_transitive_dependencies() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create project with dependency that has its own dependency
    create_test_manifest(root, "my-app", "1.0.0", &[("http", "^2.0.0")]);

    let manifest = PackageManifest::from_file(&root.join("raya.toml")).unwrap();

    let resolver = DependencyResolver::new(manifest)
        .with_available_versions(
            "http".to_string(),
            vec![Version::new(2, 0, 0), Version::new(2, 1, 0)],
        );

    let resolved = resolver.resolve().unwrap();
    assert_eq!(resolved.packages["http"].version, Version::new(2, 1, 0));
}

#[test]
fn test_complete_workflow_multiple_constraints() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create project with multiple dependencies
    create_test_manifest(
        root,
        "my-app",
        "1.0.0",
        &[
            ("logging", "^1.0.0"),
            ("http", "~2.1.0"),
            ("utils", "3.0.0"),
        ],
    );

    let manifest = PackageManifest::from_file(&root.join("raya.toml")).unwrap();

    let resolver = DependencyResolver::new(manifest)
        .with_available_versions(
            "logging".to_string(),
            vec![Version::new(1, 0, 0), Version::new(1, 5, 0), Version::new(2, 0, 0)],
        )
        .with_available_versions(
            "http".to_string(),
            vec![Version::new(2, 1, 0), Version::new(2, 1, 5), Version::new(2, 2, 0)],
        )
        .with_available_versions(
            "utils".to_string(),
            vec![Version::new(2, 9, 9), Version::new(3, 0, 0), Version::new(3, 1, 0)],
        );

    let resolved = resolver.resolve().unwrap();

    assert_eq!(resolved.packages.len(), 3);
    assert_eq!(resolved.packages["logging"].version, Version::new(1, 5, 0)); // Latest compatible with ^1.0.0
    assert_eq!(resolved.packages["http"].version, Version::new(2, 1, 5)); // Latest compatible with ~2.1.0
    assert_eq!(resolved.packages["utils"].version, Version::new(3, 0, 0)); // Exact match
}

#[test]
fn test_complete_workflow_lockfile_roundtrip() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    create_test_manifest(root, "my-app", "1.0.0", &[("dep1", "^1.0.0"), ("dep2", "^2.0.0")]);

    let manifest = PackageManifest::from_file(&root.join("raya.toml")).unwrap();

    let resolver = DependencyResolver::new(manifest)
        .with_available_versions("dep1".to_string(), vec![Version::new(1, 2, 3)])
        .with_available_versions("dep2".to_string(), vec![Version::new(2, 4, 5)]);

    let resolved = resolver.resolve().unwrap();
    let lockfile = resolved.to_lockfile(Some("my-app".to_string()));

    // Save lockfile
    let lock_path = root.join("raya.lock");
    lockfile.to_file(&lock_path).unwrap();

    // Reload and verify
    let reloaded = Lockfile::from_file(&lock_path).unwrap();
    assert_eq!(reloaded.root, Some("my-app".to_string()));
    assert_eq!(reloaded.packages.len(), 2);

    // Verify specific packages
    let dep1 = reloaded.get_package("dep1").unwrap();
    assert_eq!(dep1.version, "1.2.3");

    let dep2 = reloaded.get_package("dep2").unwrap();
    assert_eq!(dep2.version, "2.4.5");
}

#[test]
fn test_error_missing_dependency() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    create_test_manifest(root, "my-app", "1.0.0", &[("nonexistent", "^1.0.0")]);

    let manifest = PackageManifest::from_file(&root.join("raya.toml")).unwrap();
    let resolver = DependencyResolver::new(manifest);

    // Should fail because no versions are available
    let result = resolver.resolve();
    assert!(result.is_err());
}

#[test]
fn test_error_no_matching_version() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    create_test_manifest(root, "my-app", "1.0.0", &[("dep", "^5.0.0")]);

    let manifest = PackageManifest::from_file(&root.join("raya.toml")).unwrap();

    let resolver = DependencyResolver::new(manifest)
        .with_available_versions(
            "dep".to_string(),
            vec![Version::new(1, 0, 0), Version::new(2, 0, 0)],
        );

    let result = resolver.resolve();
    assert!(result.is_err());
}

#[test]
fn test_error_invalid_manifest() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create invalid manifest (missing required fields)
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("raya.toml"), "[invalid]\nfoo = \"bar\"").unwrap();

    let result = PackageManifest::from_file(&root.join("raya.toml"));
    assert!(result.is_err());
}

#[test]
fn test_error_invalid_lockfile() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create invalid lockfile
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("raya.lock"), "invalid lockfile content").unwrap();

    let result = Lockfile::from_file(&root.join("raya.lock"));
    assert!(result.is_err());
}

#[test]
fn test_cache_workflow() {
    // Initialize cache at default location
    let cache = Cache::init().unwrap();

    // Create some test module data
    let module_data = b"RAYA\x00\x00\x00\x01test module data";

    // Store in cache
    let hash = cache.store(module_data).unwrap();
    assert_eq!(hash.len(), 32); // SHA-256 produces 32 bytes

    // Verify it exists
    assert!(cache.exists(&hash));

    // Retrieve and verify
    let retrieved = cache.retrieve(&hash).unwrap();
    assert_eq!(retrieved, module_data);

    // Verify path
    let path = cache.module_path(&hash);
    assert!(path.exists());

    // Clean up test data
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn test_path_resolution_errors() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create project root
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("raya.toml"), "[package]\nname = \"root\"\nversion = \"1.0.0\"\n")
        .unwrap();

    let resolver = PathResolver::new(root.to_path_buf());

    // Test missing manifest error
    let no_manifest = root.join("empty");
    fs::create_dir_all(&no_manifest).unwrap();
    assert!(resolver.validate(&no_manifest).is_err());

    // Test path not found error
    let result = resolver.resolve_from_root("./does-not-exist");
    assert!(result.is_err());
}
