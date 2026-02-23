//! End-to-end tests for std:semver module

use super::harness::*;

#[test]
fn test_semver_valid_and_compare() {
    expect_bool_with_builtins(
        r#"
        import semver from "std:semver";
        return semver.valid("1.2.3") && !semver.valid("1.2") && semver.compare("1.2.3", "1.2.4") < 0;
    "#,
        true,
    );
}

#[test]
fn test_semver_parse_components() {
    expect_bool_with_builtins(
        r#"
        import semver from "std:semver";
        semver.parse("2.5.9-beta.1+build.7");
        return true;
    "#,
        true,
    );
}

#[test]
fn test_semver_satisfies_ranges() {
    expect_bool_with_builtins(
        r#"
        import semver from "std:semver";
        return semver.satisfies("1.4.2", ">=1.0.0, <2.0.0") &&
               semver.satisfies("1.4.2", "^1.2.0") &&
               !semver.satisfies("2.0.0", "^1.2.0");
    "#,
        true,
    );
}
