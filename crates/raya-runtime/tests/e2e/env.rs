//! E2E tests for std:env module

use super::harness::*;

#[test]
fn test_env_cwd() {
    // cwd() should return a non-empty string
    expect_bool_with_builtins(r#"
        import env from "std:env";
        const dir: string = env.cwd();
        return dir.length > 0;
    "#, true);
}

#[test]
fn test_env_home() {
    // home() should return a non-empty string
    expect_bool_with_builtins(r#"
        import env from "std:env";
        const dir: string = env.home();
        return dir.length > 0;
    "#, true);
}

#[test]
fn test_env_set_get() {
    expect_string_with_builtins(r#"
        import env from "std:env";
        env.set("RAYA_TEST_VAR", "hello_raya");
        return env.get("RAYA_TEST_VAR");
    "#, "hello_raya");
}

#[test]
fn test_env_has() {
    expect_bool_with_builtins(r#"
        import env from "std:env";
        env.set("RAYA_TEST_HAS", "1");
        return env.has("RAYA_TEST_HAS");
    "#, true);
}

#[test]
fn test_env_has_missing() {
    expect_bool_with_builtins(r#"
        import env from "std:env";
        return env.has("RAYA_NONEXISTENT_VAR_12345");
    "#, false);
}

#[test]
fn test_env_remove() {
    expect_bool_with_builtins(r#"
        import env from "std:env";
        env.set("RAYA_TEST_DEL", "gone");
        env.remove("RAYA_TEST_DEL");
        return env.has("RAYA_TEST_DEL");
    "#, false);
}

#[test]
fn test_env_all() {
    // all() returns alternating key-value pairs, so length is even
    expect_bool_with_builtins(r#"
        import env from "std:env";
        env.set("RAYA_TEST_ALL", "val");
        const pairs: string[] = env.all();
        return pairs.length > 0;
    "#, true);
}
