//! End-to-end regression tests for std import binary-link behavior.

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
fn test_namespace_std_math_shape_access_executes() {
    expect_bool_with_builtins(
        r#"
        import * as mathNs from "std:math";
        return mathNs.PI > 3;
        "#,
        true,
    );
}

#[test]
fn test_named_std_math_constant_import_executes() {
    expect_bool_with_builtins(
        r#"
        import { PI } from "std:math";
        return PI > 3;
        "#,
        true,
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

#[test]
fn test_module_scope_import_shadowing_rejected() {
    let result = compile_with_builtins(
        r#"
        import env from "std:env";
        const env = 1;
        return env;
        "#,
    );
    assert!(
        result.is_err(),
        "module-scope shadowing of import binding must be rejected"
    );
}

#[test]
fn test_inner_scope_can_shadow_import_binding() {
    expect_bool_with_builtins(
        r#"
        import env from "std:env";
        function f(): number {
            const env = 1;
            return env;
        }
        return f() == 1;
        "#,
        true,
    );
}

#[test]
fn test_default_import_identity_preserved_across_bindings() {
    expect_bool_with_builtins(
        r#"
        import math from "std:math";
        import mathAgain from "std:math";
        return math == mathAgain;
        "#,
        true,
    );
}

#[test]
fn test_default_import_cast_preserves_identity() {
    expect_bool_with_builtins(
        r#"
        import math from "std:math";
        type MathLike = { PI: number; floor: (x: number) => number };
        const casted = (math as MathLike);
        return casted == math;
        "#,
        true,
    );
}

#[test]
fn test_default_import_structural_cast_uses_shape_adapter_without_view_binding() {
    expect_bool_with_builtins(
        r#"
        import math from "std:math";
        type MathLike = { PI: number; floor: (x: number) => number };
        const casted = (math as MathLike);
        return casted == math && casted.PI > 3 && casted.floor(1.9) == 1;
        "#,
        true,
    );
}

#[test]
fn test_union_cast_of_import_preserves_identity() {
    expect_bool_with_builtins(
        r#"
        import math from "std:math";
        type MathLike = { PI: number; floor: (x: number) => number };
        type AltLike = { PI: number; ceil: (x: number) => number };
        type MathUnion = MathLike | AltLike;
        const unioned = (math as MathUnion);
        const narrowed = (unioned as MathLike);
        return narrowed == math;
        "#,
        true,
    );
}

#[test]
fn test_discriminated_union_cast_preserves_identity() {
    expect_bool_with_builtins(
        r#"
        type Circle = { kind: "circle"; area: () => number };
        type Square = { kind: "square"; area: () => number };
        type Shape = Circle | Square;
        const circle = { kind: "circle", area: () => 42 };
        const shape = (circle as Shape);
        const narrowed = (shape as Circle);
        return narrowed == circle && narrowed.area() == 42;
        "#,
        true,
    );
}
