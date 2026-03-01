//! End-to-end regression tests for std import prelude/linking behavior.

use super::harness::compile_with_builtins;
use super::harness::expect_bool_with_builtins;

#[test]
fn test_std_pm_import_smoke() {
    let result = compile_with_builtins(
        r#"
        import pm from "std:pm";
        return pm != null;
        "#,
    );
    assert!(
        result.is_ok(),
        "std:pm import should compile successfully: {:?}",
        result.err()
    );
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

#[test]
fn test_std_env_default_import_member_call_compiles() {
    let result = compile_with_builtins(
        r#"
        import env from "std:env";
        const cwd = env.cwd();
        return cwd.length > 0;
        "#,
    );
    assert!(
        result.is_ok(),
        "env default import member call should compile: {:?}",
        result.err()
    );
}

#[test]
fn test_std_http_named_class_construction_compiles() {
    let result = compile_with_builtins(
        r#"
        import { HttpServer } from "std:http";
        const server = new HttpServer("127.0.0.1", 0);
        return server != null;
        "#,
    );
    assert!(
        result.is_ok(),
        "named class construction should compile: {:?}",
        result.err()
    );
}

#[test]
fn test_class_alias_construction_compiles() {
    let result = compile_with_builtins(
        r#"
        class A {}
        const B = A;
        const x = new B();
        return x != null;
        "#,
    );
    assert!(
        result.is_ok(),
        "class alias construction should compile: {:?}",
        result.err()
    );
}

#[test]
fn test_cast_alias_construction_compiles() {
    let result = compile_with_builtins(
        r#"
        class A {}
        const ns = { A: A };
        const B = (ns.A as A);
        const x = new B();
        return x != null;
        "#,
    );
    assert!(
        result.is_ok(),
        "cast alias construction should compile: {:?}",
        result.err()
    );
}

#[test]
fn test_class_identifier_alias_value_is_not_null() {
    expect_bool_with_builtins(
        r#"
        class A {}
        const B = A;
        return B == null;
        "#,
        false,
    );
}
