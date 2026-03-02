//! Integration tests for semver parsing and constraint matching

use raya_pm::{Constraint, Version};

#[test]
fn test_version_parsing() {
    let v = Version::parse("1.2.3").unwrap();
    assert_eq!(v.major, 1);
    assert_eq!(v.minor, 2);
    assert_eq!(v.patch, 3);
    assert_eq!(v.to_string(), "1.2.3");
}

#[test]
fn test_version_with_prerelease() {
    let v = Version::parse("1.2.3-alpha.1").unwrap();
    assert_eq!(v.major, 1);
    assert_eq!(v.minor, 2);
    assert_eq!(v.patch, 3);
    assert_eq!(v.prerelease, Some("alpha.1".to_string()));
    assert!(v.is_prerelease());
}

#[test]
fn test_version_with_build() {
    let v = Version::parse("1.2.3+build.456").unwrap();
    assert_eq!(v.build, Some("build.456".to_string()));
}

#[test]
fn test_version_ordering() {
    let v1 = Version::new(1, 0, 0);
    let v2 = Version::new(1, 1, 0);
    let v3 = Version::new(2, 0, 0);

    assert!(v1 < v2);
    assert!(v2 < v3);
    assert!(v1 < v3);
}

#[test]
fn test_version_equality() {
    let v1 = Version::new(1, 2, 3);
    let v2 = Version::new(1, 2, 3);
    let v3 = Version::new(1, 2, 4);

    assert_eq!(v1, v2);
    assert_ne!(v1, v3);
}

#[test]
fn test_exact_constraint() {
    let c = Constraint::parse("1.2.3").unwrap();

    assert!(c.matches(&Version::new(1, 2, 3)));
    assert!(!c.matches(&Version::new(1, 2, 4)));
    assert!(!c.matches(&Version::new(1, 3, 3)));
}

#[test]
fn test_caret_constraint_major_nonzero() {
    let c = Constraint::parse("^1.2.3").unwrap();

    // Matches: >=1.2.3 <2.0.0
    assert!(c.matches(&Version::new(1, 2, 3)));
    assert!(c.matches(&Version::new(1, 2, 4)));
    assert!(c.matches(&Version::new(1, 3, 0)));
    assert!(c.matches(&Version::new(1, 9, 9)));

    // Does not match
    assert!(!c.matches(&Version::new(1, 2, 2)));
    assert!(!c.matches(&Version::new(1, 1, 9)));
    assert!(!c.matches(&Version::new(2, 0, 0)));
    assert!(!c.matches(&Version::new(0, 9, 9)));
}

#[test]
fn test_caret_constraint_major_zero() {
    let c = Constraint::parse("^0.2.3").unwrap();

    // Matches: >=0.2.3 <0.3.0
    assert!(c.matches(&Version::new(0, 2, 3)));
    assert!(c.matches(&Version::new(0, 2, 4)));
    assert!(c.matches(&Version::new(0, 2, 9)));

    // Does not match
    assert!(!c.matches(&Version::new(0, 2, 2)));
    assert!(!c.matches(&Version::new(0, 3, 0)));
    assert!(!c.matches(&Version::new(1, 0, 0)));
}

#[test]
fn test_caret_constraint_major_minor_zero() {
    let c = Constraint::parse("^0.0.3").unwrap();

    // Matches: >=0.0.3 <0.0.4
    assert!(c.matches(&Version::new(0, 0, 3)));

    // Does not match
    assert!(!c.matches(&Version::new(0, 0, 2)));
    assert!(!c.matches(&Version::new(0, 0, 4)));
    assert!(!c.matches(&Version::new(0, 1, 0)));
}

#[test]
fn test_tilde_constraint() {
    let c = Constraint::parse("~1.2.3").unwrap();

    // Matches: >=1.2.3 <1.3.0
    assert!(c.matches(&Version::new(1, 2, 3)));
    assert!(c.matches(&Version::new(1, 2, 4)));
    assert!(c.matches(&Version::new(1, 2, 9)));

    // Does not match
    assert!(!c.matches(&Version::new(1, 2, 2)));
    assert!(!c.matches(&Version::new(1, 3, 0)));
    assert!(!c.matches(&Version::new(2, 0, 0)));
}

#[test]
fn test_greater_than_constraint() {
    let c = Constraint::parse(">1.2.3").unwrap();

    assert!(c.matches(&Version::new(1, 2, 4)));
    assert!(c.matches(&Version::new(1, 3, 0)));
    assert!(c.matches(&Version::new(2, 0, 0)));

    assert!(!c.matches(&Version::new(1, 2, 3)));
    assert!(!c.matches(&Version::new(1, 2, 2)));
    assert!(!c.matches(&Version::new(1, 1, 9)));
}

#[test]
fn test_greater_than_or_equal_constraint() {
    let c = Constraint::parse(">=1.2.3").unwrap();

    assert!(c.matches(&Version::new(1, 2, 3)));
    assert!(c.matches(&Version::new(1, 2, 4)));
    assert!(c.matches(&Version::new(2, 0, 0)));

    assert!(!c.matches(&Version::new(1, 2, 2)));
}

#[test]
fn test_less_than_constraint() {
    let c = Constraint::parse("<2.0.0").unwrap();

    assert!(c.matches(&Version::new(1, 9, 9)));
    assert!(c.matches(&Version::new(1, 0, 0)));
    assert!(c.matches(&Version::new(0, 1, 0)));

    assert!(!c.matches(&Version::new(2, 0, 0)));
    assert!(!c.matches(&Version::new(2, 1, 0)));
}

#[test]
fn test_less_than_or_equal_constraint() {
    let c = Constraint::parse("<=2.0.0").unwrap();

    assert!(c.matches(&Version::new(2, 0, 0)));
    assert!(c.matches(&Version::new(1, 9, 9)));

    assert!(!c.matches(&Version::new(2, 0, 1)));
}

#[test]
fn test_wildcard_patch() {
    let c = Constraint::parse("1.2.*").unwrap();

    assert!(c.matches(&Version::new(1, 2, 0)));
    assert!(c.matches(&Version::new(1, 2, 5)));
    assert!(c.matches(&Version::new(1, 2, 999)));

    assert!(!c.matches(&Version::new(1, 3, 0)));
    assert!(!c.matches(&Version::new(2, 2, 0)));
}

#[test]
fn test_wildcard_minor() {
    let c = Constraint::parse("1.*").unwrap();

    assert!(c.matches(&Version::new(1, 0, 0)));
    assert!(c.matches(&Version::new(1, 5, 3)));
    assert!(c.matches(&Version::new(1, 99, 99)));

    assert!(!c.matches(&Version::new(2, 0, 0)));
    assert!(!c.matches(&Version::new(0, 9, 9)));
}

#[test]
fn test_any_constraint() {
    let c = Constraint::parse("*").unwrap();

    assert!(c.matches(&Version::new(0, 0, 1)));
    assert!(c.matches(&Version::new(1, 2, 3)));
    assert!(c.matches(&Version::new(999, 999, 999)));
}

#[test]
fn test_constraint_with_equals_prefix() {
    let c = Constraint::parse("=1.2.3").unwrap();

    assert!(c.matches(&Version::new(1, 2, 3)));
    assert!(!c.matches(&Version::new(1, 2, 4)));
}

#[test]
fn test_min_version() {
    assert_eq!(
        Constraint::parse("^1.2.3").unwrap().min_version(),
        Some(Version::new(1, 2, 3))
    );

    assert_eq!(
        Constraint::parse("~1.2.3").unwrap().min_version(),
        Some(Version::new(1, 2, 3))
    );

    assert_eq!(
        Constraint::parse(">=1.2.3").unwrap().min_version(),
        Some(Version::new(1, 2, 3))
    );

    assert_eq!(
        Constraint::parse(">1.2.3").unwrap().min_version(),
        Some(Version::new(1, 2, 4))
    );

    assert_eq!(
        Constraint::parse("1.2.*").unwrap().min_version(),
        Some(Version::new(1, 2, 0))
    );

    assert_eq!(Constraint::parse("*").unwrap().min_version(), None);
}

#[test]
fn test_version_bumping() {
    let v = Version::new(1, 2, 3);

    assert_eq!(v.bump_major(), Version::new(2, 0, 0));
    assert_eq!(v.bump_minor(), Version::new(1, 3, 0));
    assert_eq!(v.bump_patch(), Version::new(1, 2, 4));
}

#[test]
fn test_complex_versions() {
    let v1 = Version::parse("v1.2.3-alpha.1+build.123").unwrap();
    assert_eq!(v1.major, 1);
    assert_eq!(v1.prerelease, Some("alpha.1".to_string()));
    assert_eq!(v1.build, Some("build.123".to_string()));

    let v2 = Version::parse("2.0.0-beta").unwrap();
    assert_eq!(v2.major, 2);
    assert_eq!(v2.prerelease, Some("beta".to_string()));
    assert!(v2.build.is_none());
}

#[test]
fn test_prerelease_ordering() {
    let v1 = Version::parse("1.0.0-alpha").unwrap();
    let v2 = Version::parse("1.0.0-beta").unwrap();
    let v3 = Version::parse("1.0.0").unwrap();

    // Prerelease versions are less than release versions
    assert!(v1 < v3);
    assert!(v2 < v3);

    // Prerelease comparison is lexicographic
    assert!(v1 < v2);
}

#[test]
fn test_invalid_version() {
    assert!(Version::parse("1.2").is_err());
    assert!(Version::parse("1").is_err());
    assert!(Version::parse("a.b.c").is_err());
    assert!(Version::parse("").is_err());
}

#[test]
fn test_invalid_constraint() {
    assert!(Constraint::parse("").is_err());
    assert!(Constraint::parse("invalid").is_err());
}
