//! Integration tests for lockfile management
//!
//! Tests the Lockfile parser with realistic raya.lock files.

use raya_pm::{Lockfile, LockedPackage, Source};

#[test]
fn test_create_empty_lockfile() {
    let lock = Lockfile::new(Some("my-package".to_string()));
    assert_eq!(lock.version, 1);
    assert_eq!(lock.root, Some("my-package".to_string()));
    assert_eq!(lock.packages.len(), 0);
}

#[test]
fn test_minimal_lockfile() {
    let toml = r#"
version = 1

[[packages]]
name = "logging"
version = "1.2.3"
checksum = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
source = { type = "registry" }
"#;

    let lockfile = Lockfile::from_str(toml).unwrap();
    assert_eq!(lockfile.version, 1);
    assert_eq!(lockfile.packages.len(), 1);

    let pkg = &lockfile.packages[0];
    assert_eq!(pkg.name, "logging");
    assert_eq!(pkg.version, "1.2.3");
    assert_eq!(
        pkg.checksum,
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    );
    assert!(pkg.is_registry());
}

#[test]
fn test_lockfile_with_root() {
    let toml = r#"
version = 1
root = "my-app"

[[packages]]
name = "dep"
version = "1.0.0"
checksum = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
source = { type = "registry" }
"#;

    let lockfile = Lockfile::from_str(toml).unwrap();
    assert_eq!(lockfile.root, Some("my-app".to_string()));
}

#[test]
fn test_lockfile_registry_sources() {
    let toml = r#"
version = 1

[[packages]]
name = "pkg1"
version = "1.0.0"
checksum = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
source = { type = "registry" }

[[packages]]
name = "pkg2"
version = "2.0.0"
checksum = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
source = { type = "registry", url = "https://registry.example.com" }
"#;

    let lockfile = Lockfile::from_str(toml).unwrap();
    assert_eq!(lockfile.packages.len(), 2);

    assert!(lockfile.packages[0].is_registry());
    assert!(lockfile.packages[1].is_registry());
}

#[test]
fn test_lockfile_git_sources() {
    let toml = r#"
version = 1

[[packages]]
name = "git-pkg"
version = "1.0.0"
checksum = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
source = { type = "git", url = "https://github.com/user/repo", rev = "abc123def456" }
"#;

    let lockfile = Lockfile::from_str(toml).unwrap();
    let pkg = &lockfile.packages[0];

    assert!(pkg.is_git());
    assert!(!pkg.is_registry());
    assert!(!pkg.is_path());

    match &pkg.source {
        Source::Git { url, rev } => {
            assert_eq!(url, "https://github.com/user/repo");
            assert_eq!(rev, "abc123def456");
        }
        _ => panic!("Expected Git source"),
    }
}

#[test]
fn test_lockfile_path_sources() {
    let toml = r#"
version = 1

[[packages]]
name = "local-pkg"
version = "1.0.0"
checksum = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
source = { type = "path", path = "../local" }
"#;

    let lockfile = Lockfile::from_str(toml).unwrap();
    let pkg = &lockfile.packages[0];

    assert!(pkg.is_path());
    assert!(!pkg.is_git());
    assert!(!pkg.is_registry());

    match &pkg.source {
        Source::Path { path } => {
            assert_eq!(path, "../local");
        }
        _ => panic!("Expected Path source"),
    }
}

#[test]
fn test_lockfile_with_dependencies() {
    let toml = r#"
version = 1

[[packages]]
name = "app"
version = "1.0.0"
checksum = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
source = { type = "registry" }
dependencies = ["logging", "http"]

[[packages]]
name = "logging"
version = "1.2.3"
checksum = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
source = { type = "registry" }

[[packages]]
name = "http"
version = "2.0.0"
checksum = "0000000000000000000000000000000000000000000000000000000000000000"
source = { type = "registry" }
dependencies = ["logging"]
"#;

    let lockfile = Lockfile::from_str(toml).unwrap();
    assert_eq!(lockfile.packages.len(), 3);

    let app = lockfile.get_package("app").unwrap();
    assert_eq!(app.dependencies.len(), 2);
    assert!(app.dependencies.contains(&"logging".to_string()));
    assert!(app.dependencies.contains(&"http".to_string()));

    let http = lockfile.get_package("http").unwrap();
    assert_eq!(http.dependencies.len(), 1);
    assert!(http.dependencies.contains(&"logging".to_string()));
}

#[test]
fn test_add_package() {
    let mut lock = Lockfile::new(None);

    let pkg1 = LockedPackage::new(
        "pkg1".to_string(),
        "1.0.0".to_string(),
        "a".repeat(64),
        Source::registry(None),
    );

    lock.add_package(pkg1);
    assert_eq!(lock.packages.len(), 1);

    // Adding different package
    let pkg2 = LockedPackage::new(
        "pkg2".to_string(),
        "2.0.0".to_string(),
        "b".repeat(64),
        Source::registry(None),
    );

    lock.add_package(pkg2);
    assert_eq!(lock.packages.len(), 2);

    // Replacing pkg1
    let pkg1_v2 = LockedPackage::new(
        "pkg1".to_string(),
        "1.1.0".to_string(),
        "c".repeat(64),
        Source::registry(None),
    );

    lock.add_package(pkg1_v2);
    assert_eq!(lock.packages.len(), 2); // Still 2 packages

    let updated = lock.get_package("pkg1").unwrap();
    assert_eq!(updated.version, "1.1.0");
}

#[test]
fn test_get_package() {
    let mut lock = Lockfile::new(None);

    lock.add_package(LockedPackage::new(
        "exists".to_string(),
        "1.0.0".to_string(),
        "a".repeat(64),
        Source::registry(None),
    ));

    assert!(lock.get_package("exists").is_some());
    assert!(lock.get_package("missing").is_none());
}

#[test]
fn test_package_names() {
    let mut lock = Lockfile::new(None);

    lock.add_package(LockedPackage::new(
        "pkg1".to_string(),
        "1.0.0".to_string(),
        "a".repeat(64),
        Source::registry(None),
    ));

    lock.add_package(LockedPackage::new(
        "pkg2".to_string(),
        "2.0.0".to_string(),
        "b".repeat(64),
        Source::registry(None),
    ));

    let names = lock.package_names();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"pkg1"));
    assert!(names.contains(&"pkg2"));
}

#[test]
fn test_dependency_map() {
    let mut lock = Lockfile::new(None);

    let mut pkg1 = LockedPackage::new(
        "pkg1".to_string(),
        "1.0.0".to_string(),
        "a".repeat(64),
        Source::registry(None),
    );
    pkg1.add_dependency("dep1".to_string());
    pkg1.add_dependency("dep2".to_string());

    let mut pkg2 = LockedPackage::new(
        "pkg2".to_string(),
        "2.0.0".to_string(),
        "b".repeat(64),
        Source::registry(None),
    );
    pkg2.add_dependency("dep1".to_string());

    lock.add_package(pkg1);
    lock.add_package(pkg2);

    let dep_map = lock.dependency_map();
    assert_eq!(dep_map.len(), 2);
    assert_eq!(dep_map["pkg1"].len(), 2);
    assert_eq!(dep_map["pkg2"].len(), 1);
}

#[test]
fn test_sort_packages() {
    let mut lock = Lockfile::new(None);

    lock.add_package(LockedPackage::new(
        "zebra".to_string(),
        "1.0.0".to_string(),
        "a".repeat(64),
        Source::registry(None),
    ));

    lock.add_package(LockedPackage::new(
        "alpha".to_string(),
        "1.0.0".to_string(),
        "b".repeat(64),
        Source::registry(None),
    ));

    lock.add_package(LockedPackage::new(
        "middle".to_string(),
        "1.0.0".to_string(),
        "c".repeat(64),
        Source::registry(None),
    ));

    lock.sort_packages();

    assert_eq!(lock.packages[0].name, "alpha");
    assert_eq!(lock.packages[1].name, "middle");
    assert_eq!(lock.packages[2].name, "zebra");
}

#[test]
fn test_invalid_lockfile_version() {
    let toml = r#"
version = 999

[[packages]]
name = "pkg"
version = "1.0.0"
checksum = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
source = { type = "registry" }
"#;

    let result = Lockfile::from_str(toml);
    assert!(result.is_err());
}

#[test]
fn test_invalid_checksum_length() {
    let toml = r#"
version = 1

[[packages]]
name = "bad"
version = "1.0.0"
checksum = "tooshort"
source = { type = "registry" }
"#;

    let result = Lockfile::from_str(toml);
    assert!(result.is_err());
}

#[test]
fn test_invalid_checksum_chars() {
    let toml = r#"
version = 1

[[packages]]
name = "bad"
version = "1.0.0"
checksum = "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"
source = { type = "registry" }
"#;

    let result = Lockfile::from_str(toml);
    assert!(result.is_err());
}

#[test]
fn test_empty_package_name() {
    let toml = r#"
version = 1

[[packages]]
name = ""
version = "1.0.0"
checksum = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
source = { type = "registry" }
"#;

    let result = Lockfile::from_str(toml);
    assert!(result.is_err());
}

#[test]
fn test_roundtrip() {
    let mut lock = Lockfile::new(Some("root".to_string()));

    let mut pkg1 = LockedPackage::new(
        "dep1".to_string(),
        "1.0.0".to_string(),
        "a".repeat(64),
        Source::registry(None),
    );
    pkg1.add_dependency("dep2".to_string());

    lock.add_package(pkg1);

    lock.add_package(LockedPackage::new(
        "dep2".to_string(),
        "2.0.0".to_string(),
        "b".repeat(64),
        Source::git("https://github.com/user/repo".to_string(), "abc123".to_string()),
    ));

    lock.add_package(LockedPackage::new(
        "dep3".to_string(),
        "3.0.0".to_string(),
        "c".repeat(64),
        Source::path("../local"),
    ));

    lock.sort_packages();

    let serialized = toml::to_string_pretty(&lock).unwrap();
    let parsed = Lockfile::from_str(&serialized).unwrap();

    assert_eq!(lock, parsed);
}

#[test]
fn test_locked_package_add_dependency() {
    let mut pkg = LockedPackage::new(
        "pkg".to_string(),
        "1.0.0".to_string(),
        "a".repeat(64),
        Source::registry(None),
    );

    pkg.add_dependency("dep1".to_string());
    assert_eq!(pkg.dependencies.len(), 1);

    pkg.add_dependency("dep2".to_string());
    assert_eq!(pkg.dependencies.len(), 2);

    // Adding duplicate should not increase count
    pkg.add_dependency("dep1".to_string());
    assert_eq!(pkg.dependencies.len(), 2);
}

#[test]
fn test_source_constructors() {
    let registry = Source::registry(None);
    assert!(matches!(registry, Source::Registry { url: None }));

    let registry_url = Source::registry(Some("https://registry.example.com".to_string()));
    assert!(matches!(
        registry_url,
        Source::Registry {
            url: Some(ref u)
        } if u == "https://registry.example.com"
    ));

    let git = Source::git("https://github.com/user/repo".to_string(), "abc123".to_string());
    assert!(matches!(
        git,
        Source::Git {
            ref url,
            ref rev
        } if url == "https://github.com/user/repo" && rev == "abc123"
    ));

    let path = Source::path("../local");
    assert!(matches!(
        path,
        Source::Path { ref path } if path == "../local"
    ));
}

#[test]
fn test_complex_lockfile() {
    let toml = r#"
version = 1
root = "my-app"

[[packages]]
name = "my-app"
version = "1.0.0"
checksum = "1111111111111111111111111111111111111111111111111111111111111111"
source = { type = "path", path = "." }
dependencies = ["logging", "http", "utils"]

[[packages]]
name = "logging"
version = "1.2.3"
checksum = "2222222222222222222222222222222222222222222222222222222222222222"
source = { type = "registry" }

[[packages]]
name = "http"
version = "2.1.0"
checksum = "3333333333333333333333333333333333333333333333333333333333333333"
source = { type = "git", url = "https://github.com/org/http", rev = "main-abc123" }
dependencies = ["logging"]

[[packages]]
name = "utils"
version = "0.5.0"
checksum = "4444444444444444444444444444444444444444444444444444444444444444"
source = { type = "path", path = "../utils" }
dependencies = ["logging"]
"#;

    let lockfile = Lockfile::from_str(toml).unwrap();
    assert_eq!(lockfile.root, Some("my-app".to_string()));
    assert_eq!(lockfile.packages.len(), 4);

    let app = lockfile.get_package("my-app").unwrap();
    assert_eq!(app.dependencies.len(), 3);
    assert!(app.is_path());

    let logging = lockfile.get_package("logging").unwrap();
    assert!(logging.dependencies.is_empty());
    assert!(logging.is_registry());

    let http = lockfile.get_package("http").unwrap();
    assert_eq!(http.dependencies.len(), 1);
    assert!(http.is_git());

    let utils = lockfile.get_package("utils").unwrap();
    assert_eq!(utils.dependencies.len(), 1);
    assert!(utils.is_path());
}
