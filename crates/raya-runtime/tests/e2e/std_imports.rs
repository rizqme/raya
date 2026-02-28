//! End-to-end regression tests for std import prelude/linking behavior.

use super::harness::compile_with_builtins;

#[test]
fn test_std_pm_import_smoke() {
    let result = compile_with_builtins(
        r#"
        import pm from "std:pm";
        return pm != null;
        "#,
    );
    assert!(result.is_ok(), "std:pm import should compile successfully: {:?}", result.err());
}

#[test]
fn test_mixed_std_imports_execute_without_collision() {
    let result = compile_with_builtins(
        r#"
        import path from "std:path";
        import fs from "std:fs";
        import env from "std:env";
        let base = env.cwd();
        let full = path.join(base, "tmp");
        return fs != null && full.length >= 0;
        "#,
    );
    assert!(
        result.is_ok(),
        "mixed std imports should compile without symbol-collision failures: {:?}",
        result.err()
    );
}

#[test]
fn test_namespace_std_import_executes() {
    let result = compile_with_builtins(
        r#"
        import * as p from "std:path";
        return p.join("a", "b");
        "#,
    );
    assert!(
        result.is_ok(),
        "namespace std import should compile successfully: {:?}",
        result.err()
    );
}
