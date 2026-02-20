//! Integration tests for package manifest parsing
//!
//! Tests the PackageManifest parser with realistic raya.toml files.

use raya_pm::{Dependency, PackageManifest};
use std::collections::HashMap;

#[test]
fn test_minimal_manifest() {
    let toml = r#"
[package]
name = "minimal"
version = "1.0.0"
"#;

    let manifest = PackageManifest::from_str(toml).unwrap();
    assert_eq!(manifest.package.name, "minimal");
    assert_eq!(manifest.package.version, "1.0.0");
    assert!(manifest.dependencies.is_empty());
    assert!(manifest.dev_dependencies.is_empty());
}

#[test]
fn test_full_package_info() {
    let toml = r#"
[package]
name = "full-package"
version = "2.3.4"
description = "A fully-featured package"
authors = ["Alice <alice@example.com>", "Bob <bob@example.com>"]
license = "MIT"
repository = "https://github.com/user/repo"
homepage = "https://example.com"
main = "src/index.raya"
"#;

    let manifest = PackageManifest::from_str(toml).unwrap();
    assert_eq!(manifest.package.name, "full-package");
    assert_eq!(manifest.package.version, "2.3.4");
    assert_eq!(
        manifest.package.description,
        Some("A fully-featured package".to_string())
    );
    assert_eq!(manifest.package.authors.len(), 2);
    assert_eq!(manifest.package.license, Some("MIT".to_string()));
    assert_eq!(
        manifest.package.repository,
        Some("https://github.com/user/repo".to_string())
    );
    assert_eq!(manifest.package.main, Some("src/index.raya".to_string()));
}

#[test]
fn test_simple_dependencies() {
    let toml = r#"
[package]
name = "app"
version = "1.0.0"

[dependencies]
logging = "^1.2.0"
http = "~2.1.0"
utils = ">=3.0.0"
"#;

    let manifest = PackageManifest::from_str(toml).unwrap();
    assert_eq!(manifest.dependencies.len(), 3);

    assert!(matches!(
        manifest.dependencies.get("logging"),
        Some(Dependency::Simple(v)) if v == "^1.2.0"
    ));

    assert!(matches!(
        manifest.dependencies.get("http"),
        Some(Dependency::Simple(v)) if v == "~2.1.0"
    ));
}

#[test]
fn test_path_dependencies() {
    let toml = r#"
[package]
name = "workspace-member"
version = "1.0.0"

[dependencies]
shared = { path = "../shared" }
utils = { path = "../utils" }
"#;

    let manifest = PackageManifest::from_str(toml).unwrap();
    assert_eq!(manifest.dependencies.len(), 2);

    let shared = &manifest.dependencies["shared"];
    assert!(shared.is_path());
    assert_eq!(shared.path().unwrap().to_str().unwrap(), "../shared");

    let utils = &manifest.dependencies["utils"];
    assert!(utils.is_path());
    assert_eq!(utils.path().unwrap().to_str().unwrap(), "../utils");
}

#[test]
fn test_git_dependencies() {
    let toml = r#"
[package]
name = "app"
version = "1.0.0"

[dependencies]
lib1 = { git = "https://github.com/user/lib1", tag = "v1.0.0" }
lib2 = { git = "https://github.com/user/lib2", branch = "main" }
lib3 = { git = "https://github.com/user/lib3", rev = "abc123def456" }
"#;

    let manifest = PackageManifest::from_str(toml).unwrap();
    assert_eq!(manifest.dependencies.len(), 3);

    let lib1 = &manifest.dependencies["lib1"];
    assert!(lib1.is_git());
    assert_eq!(lib1.git().unwrap(), "https://github.com/user/lib1");

    let lib2 = &manifest.dependencies["lib2"];
    assert!(lib2.is_git());

    let lib3 = &manifest.dependencies["lib3"];
    assert!(lib3.is_git());
}

#[test]
fn test_dev_dependencies() {
    let toml = r#"
[package]
name = "app"
version = "1.0.0"

[dependencies]
logging = "^1.0.0"

[dev-dependencies]
test-utils = "^2.0.0"
benchmark = "~1.5.0"
"#;

    let manifest = PackageManifest::from_str(toml).unwrap();
    assert_eq!(manifest.dependencies.len(), 1);
    assert_eq!(manifest.dev_dependencies.len(), 2);

    assert!(manifest.dev_dependencies.contains_key("test-utils"));
    assert!(manifest.dev_dependencies.contains_key("benchmark"));
}

#[test]
fn test_scoped_packages() {
    let toml = r#"
[package]
name = "@myorg/my-package"
version = "1.0.0"

[dependencies]
"@otherorg/lib" = "^1.0.0"
"@thirdorg/utils" = { version = "~2.0.0" }
"#;

    let manifest = PackageManifest::from_str(toml).unwrap();
    assert_eq!(manifest.package.name, "@myorg/my-package");
    assert_eq!(manifest.dependencies.len(), 2);

    assert!(manifest.dependencies.contains_key("@otherorg/lib"));
    assert!(manifest.dependencies.contains_key("@thirdorg/utils"));
}

#[test]
fn test_mixed_dependencies() {
    let toml = r#"
[package]
name = "complex"
version = "1.0.0"

[dependencies]
registry-pkg = "^1.0.0"
local-pkg = { path = "../local" }
git-pkg = { git = "https://github.com/user/repo", tag = "v1.0.0" }
detailed-registry = { version = "~2.0.0" }
"#;

    let manifest = PackageManifest::from_str(toml).unwrap();
    assert_eq!(manifest.dependencies.len(), 4);

    assert!(manifest.dependencies["registry-pkg"].is_registry());
    assert!(manifest.dependencies["local-pkg"].is_path());
    assert!(manifest.dependencies["git-pkg"].is_git());
    assert!(manifest.dependencies["detailed-registry"].is_registry());
}

#[test]
fn test_all_dependencies() {
    let toml = r#"
[package]
name = "app"
version = "1.0.0"

[dependencies]
runtime1 = "^1.0.0"
runtime2 = "^2.0.0"

[dev-dependencies]
dev1 = "^1.0.0"
dev2 = "^2.0.0"
"#;

    let manifest = PackageManifest::from_str(toml).unwrap();
    let all_deps = manifest.all_dependencies();

    assert_eq!(all_deps.len(), 4);
    assert!(all_deps.contains_key("runtime1"));
    assert!(all_deps.contains_key("runtime2"));
    assert!(all_deps.contains_key("dev1"));
    assert!(all_deps.contains_key("dev2"));

    let runtime_deps = manifest.runtime_dependencies();
    assert_eq!(runtime_deps.len(), 2);
}

#[test]
fn test_invalid_empty_package_name() {
    let toml = r#"
[package]
name = ""
version = "1.0.0"
"#;

    let result = PackageManifest::from_str(toml);
    assert!(result.is_err());
}

#[test]
fn test_invalid_package_name_format() {
    let toml = r#"
[package]
name = "my.package"
version = "1.0.0"
"#;

    let result = PackageManifest::from_str(toml);
    assert!(result.is_err());
}

#[test]
fn test_invalid_version() {
    let toml = r#"
[package]
name = "pkg"
version = "1.0"
"#;

    let result = PackageManifest::from_str(toml);
    assert!(result.is_err());
}

#[test]
fn test_invalid_dependency_multiple_sources() {
    let toml = r#"
[package]
name = "pkg"
version = "1.0.0"

[dependencies]
bad = { version = "^1.0.0", path = "../local" }
"#;

    let result = PackageManifest::from_str(toml);
    assert!(result.is_err());
}

#[test]
fn test_invalid_dependency_no_source() {
    let toml = r#"
[package]
name = "pkg"
version = "1.0.0"

[dependencies]
bad = { }
"#;

    let result = PackageManifest::from_str(toml);
    assert!(result.is_err());
}

#[test]
fn test_roundtrip_simple() {
    let original = r#"
[package]
name = "test-pkg"
version = "1.2.3"

[dependencies]
dep1 = "^1.0.0"
"#;

    let manifest = PackageManifest::from_str(original).unwrap();
    let serialized = toml::to_string_pretty(&manifest).unwrap();
    let reparsed = PackageManifest::from_str(&serialized).unwrap();

    assert_eq!(manifest, reparsed);
}

#[test]
fn test_roundtrip_complex() {
    let mut manifest = PackageManifest {
        package: raya_pm::PackageInfo {
            name: "complex-pkg".to_string(),
            version: "2.3.4".to_string(),
            description: Some("Test package".to_string()),
            authors: vec!["Author <author@example.com>".to_string()],
            license: Some("MIT".to_string()),
            repository: Some("https://github.com/user/repo".to_string()),
            homepage: None,
            main: None,
        },
        jsx: None,
        scripts: HashMap::new(),
        dependencies: HashMap::new(),
        dev_dependencies: HashMap::new(),
        registry: None,
        assets: None,
        build: None,
        bundle: None,
        lint: None,
    };

    manifest.dependencies.insert(
        "dep1".to_string(),
        Dependency::Simple("^1.0.0".to_string()),
    );

    manifest.dependencies.insert(
        "dep2".to_string(),
        Dependency::Detailed {
            version: None,
            path: Some("../local".to_string()),
            git: None,
            branch: None,
            tag: None,
            rev: None,
        },
    );

    let serialized = toml::to_string_pretty(&manifest).unwrap();
    let reparsed = PackageManifest::from_str(&serialized).unwrap();

    assert_eq!(manifest, reparsed);
}
