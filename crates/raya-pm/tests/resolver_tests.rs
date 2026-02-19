//! Integration tests for dependency resolution

use raya_pm::{Dependency, DependencyResolver, PackageInfo, PackageManifest, Version};
use std::collections::HashMap;

fn create_manifest(name: &str, version: &str) -> PackageManifest {
    PackageManifest {
        package: PackageInfo {
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
        scripts: HashMap::new(),
        dependencies: HashMap::new(),
        dev_dependencies: HashMap::new(),
        registry: None,
    }
}

#[test]
fn test_resolve_single_dependency() {
    let mut manifest = create_manifest("my-app", "1.0.0");
    manifest.dependencies.insert(
        "logging".to_string(),
        Dependency::Simple("^1.0.0".to_string()),
    );

    let resolver = DependencyResolver::new(manifest).with_available_versions(
        "logging".to_string(),
        vec![
            Version::new(1, 0, 0),
            Version::new(1, 2, 0),
            Version::new(2, 0, 0),
        ],
    );

    let resolved = resolver.resolve().unwrap();

    assert_eq!(resolved.packages.len(), 1);
    let logging = &resolved.packages["logging"];
    assert_eq!(logging.name, "logging");
    assert_eq!(logging.version, Version::new(1, 2, 0)); // Latest compatible
}

#[test]
fn test_resolve_multiple_dependencies() {
    let mut manifest = create_manifest("my-app", "1.0.0");
    manifest.dependencies.insert(
        "logging".to_string(),
        Dependency::Simple("^1.0.0".to_string()),
    );
    manifest.dependencies.insert(
        "http".to_string(),
        Dependency::Simple("~2.1.0".to_string()),
    );

    let resolver = DependencyResolver::new(manifest)
        .with_available_versions(
            "logging".to_string(),
            vec![Version::new(1, 0, 0), Version::new(1, 5, 0)],
        )
        .with_available_versions(
            "http".to_string(),
            vec![Version::new(2, 1, 0), Version::new(2, 1, 5), Version::new(2, 2, 0)],
        );

    let resolved = resolver.resolve().unwrap();

    assert_eq!(resolved.packages.len(), 2);
    assert_eq!(resolved.packages["logging"].version, Version::new(1, 5, 0));
    assert_eq!(resolved.packages["http"].version, Version::new(2, 1, 5)); // Tilde constraint
}

#[test]
fn test_resolve_exact_version() {
    let mut manifest = create_manifest("my-app", "1.0.0");
    manifest.dependencies.insert(
        "utils".to_string(),
        Dependency::Simple("2.3.4".to_string()),
    );

    let resolver = DependencyResolver::new(manifest).with_available_versions(
        "utils".to_string(),
        vec![
            Version::new(2, 3, 3),
            Version::new(2, 3, 4),
            Version::new(2, 3, 5),
        ],
    );

    let resolved = resolver.resolve().unwrap();
    assert_eq!(resolved.packages["utils"].version, Version::new(2, 3, 4));
}

#[test]
fn test_resolve_greater_than_constraint() {
    let mut manifest = create_manifest("my-app", "1.0.0");
    manifest.dependencies.insert(
        "async".to_string(),
        Dependency::Simple(">=3.0.0".to_string()),
    );

    let resolver = DependencyResolver::new(manifest).with_available_versions(
        "async".to_string(),
        vec![
            Version::new(2, 9, 9),
            Version::new(3, 0, 0),
            Version::new(3, 1, 0),
            Version::new(3, 2, 0),
        ],
    );

    let resolved = resolver.resolve().unwrap();
    assert_eq!(resolved.packages["async"].version, Version::new(3, 2, 0)); // Latest
}

#[test]
fn test_resolve_wildcard() {
    let mut manifest = create_manifest("my-app", "1.0.0");
    manifest.dependencies.insert(
        "lib".to_string(),
        Dependency::Simple("1.2.*".to_string()),
    );

    let resolver = DependencyResolver::new(manifest).with_available_versions(
        "lib".to_string(),
        vec![
            Version::new(1, 1, 9),
            Version::new(1, 2, 0),
            Version::new(1, 2, 5),
            Version::new(1, 3, 0),
        ],
    );

    let resolved = resolver.resolve().unwrap();
    assert_eq!(resolved.packages["lib"].version, Version::new(1, 2, 5)); // Latest in 1.2.*
}

#[test]
fn test_no_matching_version_error() {
    let mut manifest = create_manifest("my-app", "1.0.0");
    manifest.dependencies.insert(
        "missing".to_string(),
        Dependency::Simple("^5.0.0".to_string()),
    );

    let resolver = DependencyResolver::new(manifest).with_available_versions(
        "missing".to_string(),
        vec![Version::new(1, 0, 0), Version::new(2, 0, 0)],
    );

    let result = resolver.resolve();
    assert!(result.is_err());
}

#[test]
fn test_package_not_found_error() {
    let mut manifest = create_manifest("my-app", "1.0.0");
    manifest.dependencies.insert(
        "unknown".to_string(),
        Dependency::Simple("^1.0.0".to_string()),
    );

    let resolver = DependencyResolver::new(manifest);
    let result = resolver.resolve();
    assert!(result.is_err());
}

#[test]
fn test_lockfile_generation() {
    let mut manifest = create_manifest("my-app", "1.0.0");
    manifest.dependencies.insert(
        "logging".to_string(),
        Dependency::Simple("^1.0.0".to_string()),
    );
    manifest.dependencies.insert(
        "http".to_string(),
        Dependency::Simple("~2.0.0".to_string()),
    );

    let resolver = DependencyResolver::new(manifest)
        .with_available_versions(
            "logging".to_string(),
            vec![Version::new(1, 0, 0), Version::new(1, 2, 0)],
        )
        .with_available_versions(
            "http".to_string(),
            vec![Version::new(2, 0, 0), Version::new(2, 0, 5)],
        );

    let resolved = resolver.resolve().unwrap();
    let lockfile = resolved.to_lockfile(Some("my-app".to_string()));

    assert_eq!(lockfile.root, Some("my-app".to_string()));
    assert_eq!(lockfile.packages.len(), 2);

    let logging = lockfile.get_package("logging").unwrap();
    assert_eq!(logging.version, "1.2.0");

    let http = lockfile.get_package("http").unwrap();
    assert_eq!(http.version, "2.0.5");
}

#[test]
fn test_dev_dependencies() {
    let mut manifest = create_manifest("my-app", "1.0.0");
    manifest.dependencies.insert(
        "logging".to_string(),
        Dependency::Simple("^1.0.0".to_string()),
    );
    manifest.dev_dependencies.insert(
        "test-utils".to_string(),
        Dependency::Simple("^2.0.0".to_string()),
    );

    let resolver = DependencyResolver::new(manifest)
        .with_available_versions("logging".to_string(), vec![Version::new(1, 0, 0)])
        .with_available_versions("test-utils".to_string(), vec![Version::new(2, 0, 0)]);

    let resolved = resolver.resolve().unwrap();

    assert_eq!(resolved.packages.len(), 2);
    assert!(resolved.packages.contains_key("logging"));
    assert!(resolved.packages.contains_key("test-utils"));
}

#[test]
fn test_caret_with_zero_major() {
    let mut manifest = create_manifest("my-app", "1.0.0");
    manifest.dependencies.insert(
        "experimental".to_string(),
        Dependency::Simple("^0.2.3".to_string()),
    );

    let resolver = DependencyResolver::new(manifest).with_available_versions(
        "experimental".to_string(),
        vec![
            Version::new(0, 2, 3),
            Version::new(0, 2, 4),
            Version::new(0, 2, 9),
            Version::new(0, 3, 0),
        ],
    );

    let resolved = resolver.resolve().unwrap();
    // ^0.2.3 matches >=0.2.3 <0.3.0, so should pick 0.2.9
    assert_eq!(
        resolved.packages["experimental"].version,
        Version::new(0, 2, 9)
    );
}

#[test]
fn test_tilde_constraint_picks_latest_patch() {
    let mut manifest = create_manifest("my-app", "1.0.0");
    manifest.dependencies.insert(
        "stable".to_string(),
        Dependency::Simple("~1.5.0".to_string()),
    );

    let resolver = DependencyResolver::new(manifest).with_available_versions(
        "stable".to_string(),
        vec![
            Version::new(1, 5, 0),
            Version::new(1, 5, 3),
            Version::new(1, 5, 10),
            Version::new(1, 6, 0),
        ],
    );

    let resolved = resolver.resolve().unwrap();
    // ~1.5.0 matches >=1.5.0 <1.6.0, so should pick 1.5.10
    assert_eq!(
        resolved.packages["stable"].version,
        Version::new(1, 5, 10)
    );
}

#[test]
fn test_prerelease_versions_excluded_by_default() {
    let mut manifest = create_manifest("my-app", "1.0.0");
    manifest.dependencies.insert(
        "lib".to_string(),
        Dependency::Simple("^1.0.0".to_string()),
    );

    let resolver = DependencyResolver::new(manifest).with_available_versions(
        "lib".to_string(),
        vec![
            Version::new(1, 0, 0),
            Version::new(1, 1, 0),
            Version::parse("1.2.0-alpha").unwrap(),
        ],
    );

    let resolved = resolver.resolve().unwrap();
    // Should pick 1.1.0, not the prerelease
    assert_eq!(resolved.packages["lib"].version, Version::new(1, 1, 0));
}

#[test]
fn test_complex_resolution() {
    let mut manifest = create_manifest("my-app", "1.0.0");

    // Multiple dependencies with different constraint types
    manifest.dependencies.insert(
        "logging".to_string(),
        Dependency::Simple("^1.2.0".to_string()),
    );
    manifest.dependencies.insert(
        "http".to_string(),
        Dependency::Simple("~2.3.0".to_string()),
    );
    manifest.dependencies.insert(
        "utils".to_string(),
        Dependency::Simple("3.4.5".to_string()),
    );
    manifest.dependencies.insert(
        "async".to_string(),
        Dependency::Simple(">=4.0.0".to_string()),
    );

    let resolver = DependencyResolver::new(manifest)
        .with_available_versions(
            "logging".to_string(),
            vec![Version::new(1, 2, 0), Version::new(1, 5, 0), Version::new(2, 0, 0)],
        )
        .with_available_versions(
            "http".to_string(),
            vec![Version::new(2, 3, 0), Version::new(2, 3, 5), Version::new(2, 4, 0)],
        )
        .with_available_versions(
            "utils".to_string(),
            vec![Version::new(3, 4, 4), Version::new(3, 4, 5), Version::new(3, 4, 6)],
        )
        .with_available_versions(
            "async".to_string(),
            vec![Version::new(3, 9, 9), Version::new(4, 0, 0), Version::new(4, 1, 0)],
        );

    let resolved = resolver.resolve().unwrap();

    assert_eq!(resolved.packages.len(), 4);
    assert_eq!(resolved.packages["logging"].version, Version::new(1, 5, 0)); // ^1.2.0
    assert_eq!(resolved.packages["http"].version, Version::new(2, 3, 5)); // ~2.3.0
    assert_eq!(resolved.packages["utils"].version, Version::new(3, 4, 5)); // exact
    assert_eq!(resolved.packages["async"].version, Version::new(4, 1, 0)); // >=4.0.0
}
