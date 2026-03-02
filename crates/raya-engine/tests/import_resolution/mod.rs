//! Integration tests for import resolution
//!
//! Tests the complete import resolution pipeline including path resolution,
//! dependency graph building, and circular dependency detection.

use raya_engine::vm::module::{DependencyGraph, ImportResolver, ImportSpec};
use std::path::PathBuf;

#[test]
fn test_parse_local_import() {
    let resolver = ImportResolver::new(PathBuf::from("/project"));

    // Simple relative import
    let spec = resolver.parse_specifier("./utils.raya").unwrap();
    match spec {
        ImportSpec::Local(path) => {
            assert_eq!(path, PathBuf::from("./utils.raya"));
        }
        _ => panic!("Expected Local import"),
    }

    // Parent directory import
    let spec = resolver.parse_specifier("../lib/helper.raya").unwrap();
    match spec {
        ImportSpec::Local(path) => {
            assert_eq!(path, PathBuf::from("../lib/helper.raya"));
        }
        _ => panic!("Expected Local import"),
    }
}

#[test]
fn test_parse_package_import() {
    let resolver = ImportResolver::new(PathBuf::from("/project"));

    // Simple package
    let spec = resolver.parse_specifier("logging").unwrap();
    match spec {
        ImportSpec::Package { name, version } => {
            assert_eq!(name, "logging");
            assert_eq!(version, None);
        }
        _ => panic!("Expected Package import"),
    }

    // Package with exact version
    let spec = resolver.parse_specifier("logging@1.2.3").unwrap();
    match spec {
        ImportSpec::Package { name, version } => {
            assert_eq!(name, "logging");
            assert_eq!(version, Some("1.2.3".to_string()));
        }
        _ => panic!("Expected Package import"),
    }

    // Package with caret version
    let spec = resolver.parse_specifier("http@^2.0.0").unwrap();
    match spec {
        ImportSpec::Package { name, version } => {
            assert_eq!(name, "http");
            assert_eq!(version, Some("^2.0.0".to_string()));
        }
        _ => panic!("Expected Package import"),
    }

    // Package with tilde version
    let spec = resolver.parse_specifier("async@~1.5.0").unwrap();
    match spec {
        ImportSpec::Package { name, version } => {
            assert_eq!(name, "async");
            assert_eq!(version, Some("~1.5.0".to_string()));
        }
        _ => panic!("Expected Package import"),
    }
}

#[test]
fn test_parse_scoped_package() {
    let resolver = ImportResolver::new(PathBuf::from("/project"));

    // Scoped package without version
    let spec = resolver.parse_specifier("@org/package").unwrap();
    match spec {
        ImportSpec::Package { name, version } => {
            assert_eq!(name, "@org/package");
            assert_eq!(version, None);
        }
        _ => panic!("Expected Package import"),
    }

    // Scoped package with version
    let spec = resolver.parse_specifier("@org/package@1.0.0").unwrap();
    match spec {
        ImportSpec::Package { name, version } => {
            assert_eq!(name, "@org/package");
            assert_eq!(version, Some("1.0.0".to_string()));
        }
        _ => panic!("Expected Package import"),
    }
}

#[test]
fn test_parse_url_import() {
    let resolver = ImportResolver::new(PathBuf::from("/project"));

    // HTTPS URL
    let spec = resolver
        .parse_specifier("https://example.com/module.ryb")
        .unwrap();
    match spec {
        ImportSpec::Url(url) => {
            assert_eq!(url, "https://example.com/module.ryb");
        }
        _ => panic!("Expected URL import"),
    }

    // HTTP URL
    let spec = resolver
        .parse_specifier("http://localhost:8080/test.ryb")
        .unwrap();
    match spec {
        ImportSpec::Url(url) => {
            assert_eq!(url, "http://localhost:8080/test.ryb");
        }
        _ => panic!("Expected URL import"),
    }
}

#[test]
fn test_circular_dependency_detection() {
    let mut graph = DependencyGraph::new();

    // Create a simple cycle: A -> B -> C -> A
    graph.add_dependency("a".to_string(), "b".to_string());
    graph.add_dependency("b".to_string(), "c".to_string());
    graph.add_dependency("c".to_string(), "a".to_string());

    // Detect the cycle
    let cycle = graph.detect_cycle();
    assert!(cycle.is_some(), "Expected to detect circular dependency");

    let cycle_path = cycle.unwrap();
    assert!(
        cycle_path.len() >= 3,
        "Cycle should include at least 3 modules"
    );

    // Verify the cycle contains the modules
    let cycle_str = cycle_path.join("->");
    assert!(
        cycle_str.contains('a') && cycle_str.contains('b') && cycle_str.contains('c'),
        "Cycle should contain a, b, and c"
    );
}

#[test]
fn test_no_circular_dependency() {
    let mut graph = DependencyGraph::new();

    // Create a valid DAG
    graph.add_dependency("main".to_string(), "utils".to_string());
    graph.add_dependency("main".to_string(), "config".to_string());
    graph.add_dependency("utils".to_string(), "helpers".to_string());

    let cycle = graph.detect_cycle();
    assert!(cycle.is_none(), "Should not detect any cycles");
}

#[test]
fn test_topological_sort() {
    let mut graph = DependencyGraph::new();

    // Build dependency graph:
    //   main -> utils -> helpers
    //   main -> config
    graph.add_dependency("main".to_string(), "utils".to_string());
    graph.add_dependency("utils".to_string(), "helpers".to_string());
    graph.add_dependency("main".to_string(), "config".to_string());

    let sorted = graph.topological_sort().unwrap();

    // Find positions
    let helpers_idx = sorted
        .iter()
        .position(|m| m == "helpers")
        .expect("helpers should be in sorted list");
    let utils_idx = sorted
        .iter()
        .position(|m| m == "utils")
        .expect("utils should be in sorted list");
    let main_idx = sorted
        .iter()
        .position(|m| m == "main")
        .expect("main should be in sorted list");
    let config_idx = sorted
        .iter()
        .position(|m| m == "config")
        .expect("config should be in sorted list");

    // Verify dependencies come before dependents
    assert!(helpers_idx < utils_idx, "helpers should come before utils");
    assert!(utils_idx < main_idx, "utils should come before main");
    assert!(config_idx < main_idx, "config should come before main");
}

#[test]
fn test_topological_sort_with_cycle() {
    let mut graph = DependencyGraph::new();

    // Create a cycle
    graph.add_dependency("a".to_string(), "b".to_string());
    graph.add_dependency("b".to_string(), "c".to_string());
    graph.add_dependency("c".to_string(), "a".to_string());

    let result = graph.topological_sort();
    assert!(
        result.is_err(),
        "Topological sort should fail with circular dependencies"
    );
}

#[test]
fn test_diamond_dependency() {
    let mut graph = DependencyGraph::new();

    // Create diamond dependency:
    //      a
    //     / \
    //    b   c
    //     \ /
    //      d
    graph.add_dependency("a".to_string(), "b".to_string());
    graph.add_dependency("a".to_string(), "c".to_string());
    graph.add_dependency("b".to_string(), "d".to_string());
    graph.add_dependency("c".to_string(), "d".to_string());

    // Should not detect a cycle
    let cycle = graph.detect_cycle();
    assert!(cycle.is_none(), "Diamond dependency is not a cycle");

    // Topological sort should work
    let sorted = graph.topological_sort().unwrap();
    assert_eq!(sorted.len(), 4);

    // d should come before both b and c
    let d_idx = sorted.iter().position(|m| m == "d").unwrap();
    let b_idx = sorted.iter().position(|m| m == "b").unwrap();
    let c_idx = sorted.iter().position(|m| m == "c").unwrap();
    let a_idx = sorted.iter().position(|m| m == "a").unwrap();

    assert!(d_idx < b_idx, "d should come before b");
    assert!(d_idx < c_idx, "d should come before c");
    assert!(b_idx < a_idx, "b should come before a");
    assert!(c_idx < a_idx, "c should come before a");
}

#[test]
fn test_complex_dependency_graph() {
    let mut graph = DependencyGraph::new();

    // Complex graph with multiple levels
    graph.add_dependency("app".to_string(), "router".to_string());
    graph.add_dependency("app".to_string(), "store".to_string());
    graph.add_dependency("router".to_string(), "utils".to_string());
    graph.add_dependency("store".to_string(), "utils".to_string());
    graph.add_dependency("store".to_string(), "api".to_string());
    graph.add_dependency("api".to_string(), "http".to_string());
    graph.add_dependency("utils".to_string(), "logging".to_string());

    let sorted = graph.topological_sort().unwrap();

    // Verify basic ordering constraints
    let logging_idx = sorted.iter().position(|m| m == "logging").unwrap();
    let utils_idx = sorted.iter().position(|m| m == "utils").unwrap();
    let http_idx = sorted.iter().position(|m| m == "http").unwrap();
    let api_idx = sorted.iter().position(|m| m == "api").unwrap();
    let store_idx = sorted.iter().position(|m| m == "store").unwrap();
    let app_idx = sorted.iter().position(|m| m == "app").unwrap();

    assert!(logging_idx < utils_idx);
    assert!(utils_idx < store_idx);
    assert!(http_idx < api_idx);
    assert!(api_idx < store_idx);
    assert!(store_idx < app_idx);
}

#[test]
fn test_self_dependency() {
    let mut graph = DependencyGraph::new();

    // Module depends on itself
    graph.add_dependency("module".to_string(), "module".to_string());

    let cycle = graph.detect_cycle();
    assert!(
        cycle.is_some(),
        "Self-dependency should be detected as a cycle"
    );
}

#[test]
fn test_empty_graph() {
    let graph = DependencyGraph::new();

    let cycle = graph.detect_cycle();
    assert!(cycle.is_none(), "Empty graph has no cycles");

    let sorted = graph.topological_sort().unwrap();
    assert_eq!(sorted.len(), 0, "Empty graph produces empty sorted list");
}

#[test]
fn test_independent_modules() {
    let mut graph = DependencyGraph::new();

    // Three modules with no dependencies between them
    graph.add_module("a".to_string());
    graph.add_module("b".to_string());
    graph.add_module("c".to_string());

    let cycle = graph.detect_cycle();
    assert!(cycle.is_none(), "Independent modules have no cycles");

    let sorted = graph.topological_sort().unwrap();
    assert_eq!(sorted.len(), 3, "Should contain all three modules");
}
