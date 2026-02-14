//! End-to-end tests for the std:path module
//!
//! Tests verify that path manipulation methods compile and execute correctly.

use super::harness::{
    compile_and_run_with_builtins, expect_bool_with_builtins,
    expect_string_with_builtins,
};

// ============================================================================
// Import
// ============================================================================

#[test]
fn test_path_import() {
    let result = compile_and_run_with_builtins(
        r#"
        import path from "std:path";
        let p: string = path.join("a", "b");
        return 1;
    "#,
    );
    assert!(
        result.is_ok(),
        "Path should be importable from std:path: {:?}",
        result.err()
    );
}

// ============================================================================
// Join
// ============================================================================

#[test]
fn test_path_join_basic() {
    expect_string_with_builtins(
        r#"
        import path from "std:path";
        return path.join("a", "b");
    "#,
        "a/b",
    );
}

#[test]
fn test_path_join_absolute() {
    expect_string_with_builtins(
        r#"
        import path from "std:path";
        return path.join("/home", "docs");
    "#,
        "/home/docs",
    );
}

#[test]
fn test_path_join_chain() {
    expect_string_with_builtins(
        r#"
        import path from "std:path";
        return path.join(path.join("a", "b"), "c");
    "#,
        "a/b/c",
    );
}

// ============================================================================
// Normalize
// ============================================================================

#[test]
fn test_path_normalize() {
    expect_string_with_builtins(
        r#"
        import path from "std:path";
        return path.normalize("/foo/bar/../baz");
    "#,
        "/foo/baz",
    );
}

#[test]
fn test_path_normalize_dot() {
    expect_string_with_builtins(
        r#"
        import path from "std:path";
        return path.normalize("foo/./bar");
    "#,
        "foo/bar",
    );
}

// ============================================================================
// Components
// ============================================================================

#[test]
fn test_path_dirname() {
    expect_string_with_builtins(
        r#"
        import path from "std:path";
        return path.dirname("/home/alice/file.txt");
    "#,
        "/home/alice",
    );
}

#[test]
fn test_path_basename() {
    expect_string_with_builtins(
        r#"
        import path from "std:path";
        return path.basename("/home/alice/file.txt");
    "#,
        "file.txt",
    );
}

#[test]
fn test_path_extname() {
    expect_string_with_builtins(
        r#"
        import path from "std:path";
        return path.extname("file.txt");
    "#,
        ".txt",
    );
}

#[test]
fn test_path_extname_none() {
    expect_string_with_builtins(
        r#"
        import path from "std:path";
        return path.extname("Makefile");
    "#,
        "",
    );
}

// ============================================================================
// Absolute / Relative
// ============================================================================

#[test]
fn test_path_is_absolute_true() {
    expect_bool_with_builtins(
        r#"
        import path from "std:path";
        return path.isAbsolute("/foo");
    "#,
        true,
    );
}

#[test]
fn test_path_is_absolute_false() {
    expect_bool_with_builtins(
        r#"
        import path from "std:path";
        return path.isAbsolute("foo");
    "#,
        false,
    );
}

#[test]
fn test_path_resolve() {
    expect_string_with_builtins(
        r#"
        import path from "std:path";
        return path.resolve("/base", "rel");
    "#,
        "/base/rel",
    );
}

#[test]
fn test_path_relative() {
    expect_string_with_builtins(
        r#"
        import path from "std:path";
        return path.relative("/a/b", "/a/b/c/d");
    "#,
        "c/d",
    );
}

#[test]
fn test_path_cwd_not_empty() {
    let result = compile_and_run_with_builtins(
        r#"
        import path from "std:path";
        let c: string = path.cwd();
        if (c.length > 0) {
            return 1;
        }
        return 0;
    "#,
    );
    assert!(result.is_ok(), "path.cwd() should work: {:?}", result.err());
    assert_eq!(result.unwrap().as_i32(), Some(1));
}

// ============================================================================
// OS Constants
// ============================================================================

#[test]
fn test_path_sep() {
    expect_string_with_builtins(
        r#"
        import path from "std:path";
        return path.sep();
    "#,
        "/",
    );
}

#[test]
fn test_path_delimiter() {
    expect_string_with_builtins(
        r#"
        import path from "std:path";
        return path.delimiter();
    "#,
        ":",
    );
}

// ============================================================================
// Pure Raya Utilities
// ============================================================================

#[test]
fn test_path_strip_ext() {
    expect_string_with_builtins(
        r#"
        import path from "std:path";
        return path.stripExt("file.txt");
    "#,
        "file",
    );
}

#[test]
fn test_path_with_ext() {
    expect_string_with_builtins(
        r#"
        import path from "std:path";
        return path.withExt("file.txt", ".md");
    "#,
        "file.md",
    );
}

#[test]
fn test_path_is_relative() {
    expect_bool_with_builtins(
        r#"
        import path from "std:path";
        return path.isRelative("foo/bar");
    "#,
        true,
    );
}

#[test]
fn test_path_is_relative_false() {
    expect_bool_with_builtins(
        r#"
        import path from "std:path";
        return path.isRelative("/foo/bar");
    "#,
        false,
    );
}
