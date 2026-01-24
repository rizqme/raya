//! Semantic versioning parser and constraint matching
//!
//! Provides semver parsing and version constraint resolution.

use std::cmp::Ordering;
use std::fmt;
use thiserror::Error;

/// Errors that can occur during semver parsing
#[derive(Debug, Error)]
pub enum SemverError {
    /// Invalid version format
    #[error("Invalid version format: {0}")]
    InvalidVersion(String),

    /// Invalid constraint format
    #[error("Invalid constraint format: {0}")]
    InvalidConstraint(String),

    /// Invalid pre-release tag
    #[error("Invalid pre-release tag: {0}")]
    InvalidPrerelease(String),
}

/// Semantic version (MAJOR.MINOR.PATCH)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub prerelease: Option<String>,
    pub build: Option<String>,
}

/// Version constraint
#[derive(Debug, Clone, PartialEq)]
pub enum Constraint {
    /// Exact version (=1.2.3 or 1.2.3)
    Exact(Version),

    /// Caret range (^1.2.3 → >=1.2.3 <2.0.0)
    Caret(Version),

    /// Tilde range (~1.2.3 → >=1.2.3 <1.3.0)
    Tilde(Version),

    /// Greater than (>1.2.3)
    GreaterThan(Version),

    /// Greater than or equal (>=1.2.3)
    GreaterThanOrEqual(Version),

    /// Less than (<1.2.3)
    LessThan(Version),

    /// Less than or equal (<=1.2.3)
    LessThanOrEqual(Version),

    /// Wildcard (1.2.*, 1.*)
    Wildcard(u64, Option<u64>),

    /// Any version (*)
    Any,
}

impl Version {
    /// Parse a version string
    pub fn parse(s: &str) -> Result<Self, SemverError> {
        let s = s.trim();

        // Remove 'v' prefix if present
        let s = s.strip_prefix('v').unwrap_or(s);

        // Split by + to separate build metadata
        let (version_part, build) = if let Some(pos) = s.find('+') {
            let (v, b) = s.split_at(pos);
            (v, Some(b[1..].to_string()))
        } else {
            (s, None)
        };

        // Split by - to separate prerelease
        let (core_version, prerelease) = if let Some(pos) = version_part.find('-') {
            let (v, p) = version_part.split_at(pos);
            (v, Some(p[1..].to_string()))
        } else {
            (version_part, None)
        };

        // Parse MAJOR.MINOR.PATCH
        let parts: Vec<&str> = core_version.split('.').collect();
        if parts.len() != 3 {
            return Err(SemverError::InvalidVersion(format!(
                "Expected MAJOR.MINOR.PATCH, got '{}'",
                s
            )));
        }

        let major = parts[0]
            .parse()
            .map_err(|_| SemverError::InvalidVersion(format!("Invalid major version: {}", parts[0])))?;

        let minor = parts[1]
            .parse()
            .map_err(|_| SemverError::InvalidVersion(format!("Invalid minor version: {}", parts[1])))?;

        let patch = parts[2]
            .parse()
            .map_err(|_| SemverError::InvalidVersion(format!("Invalid patch version: {}", parts[2])))?;

        Ok(Version {
            major,
            minor,
            patch,
            prerelease,
            build,
        })
    }

    /// Create a new version
    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Version {
            major,
            minor,
            patch,
            prerelease: None,
            build: None,
        }
    }

    /// Check if this is a prerelease version
    pub fn is_prerelease(&self) -> bool {
        self.prerelease.is_some()
    }

    /// Bump major version (resets minor and patch to 0)
    pub fn bump_major(&self) -> Self {
        Version::new(self.major + 1, 0, 0)
    }

    /// Bump minor version (resets patch to 0)
    pub fn bump_minor(&self) -> Self {
        Version::new(self.major, self.minor + 1, 0)
    }

    /// Bump patch version
    pub fn bump_patch(&self) -> Self {
        Version::new(self.major, self.minor, self.patch + 1)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(ref pre) = self.prerelease {
            write!(f, "-{}", pre)?;
        }
        if let Some(ref build) = self.build {
            write!(f, "+{}", build)?;
        }
        Ok(())
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        // Compare major, minor, patch
        match self.major.cmp(&other.major) {
            Ordering::Equal => {}
            ord => return ord,
        }
        match self.minor.cmp(&other.minor) {
            Ordering::Equal => {}
            ord => return ord,
        }
        match self.patch.cmp(&other.patch) {
            Ordering::Equal => {}
            ord => return ord,
        }

        // Compare prerelease (versions with prerelease are less than without)
        match (&self.prerelease, &other.prerelease) {
            (None, None) => Ordering::Equal,
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(a), Some(b)) => a.cmp(b),
        }
    }
}

impl Constraint {
    /// Parse a constraint string
    pub fn parse(s: &str) -> Result<Self, SemverError> {
        let s = s.trim();

        if s == "*" {
            return Ok(Constraint::Any);
        }

        // Check for operators
        if let Some(rest) = s.strip_prefix(">=") {
            let version = Version::parse(rest.trim())?;
            return Ok(Constraint::GreaterThanOrEqual(version));
        }

        if let Some(rest) = s.strip_prefix("<=") {
            let version = Version::parse(rest.trim())?;
            return Ok(Constraint::LessThanOrEqual(version));
        }

        if let Some(rest) = s.strip_prefix('>') {
            let version = Version::parse(rest.trim())?;
            return Ok(Constraint::GreaterThan(version));
        }

        if let Some(rest) = s.strip_prefix('<') {
            let version = Version::parse(rest.trim())?;
            return Ok(Constraint::LessThan(version));
        }

        if let Some(rest) = s.strip_prefix('^') {
            let version = Version::parse(rest.trim())?;
            return Ok(Constraint::Caret(version));
        }

        if let Some(rest) = s.strip_prefix('~') {
            let version = Version::parse(rest.trim())?;
            return Ok(Constraint::Tilde(version));
        }

        if let Some(rest) = s.strip_prefix('=') {
            let version = Version::parse(rest.trim())?;
            return Ok(Constraint::Exact(version));
        }

        // Check for wildcards
        if s.contains('*') {
            return Self::parse_wildcard(s);
        }

        // Default: exact version
        let version = Version::parse(s)?;
        Ok(Constraint::Exact(version))
    }

    /// Parse wildcard constraint (1.2.*, 1.*)
    fn parse_wildcard(s: &str) -> Result<Self, SemverError> {
        let parts: Vec<&str> = s.split('.').collect();

        if parts.len() == 1 && parts[0] == "*" {
            return Ok(Constraint::Any);
        }

        if parts.len() == 2 && parts[1] == "*" {
            let major = parts[0].parse().map_err(|_| {
                SemverError::InvalidConstraint(format!("Invalid wildcard: {}", s))
            })?;
            return Ok(Constraint::Wildcard(major, None));
        }

        if parts.len() == 3 && parts[2] == "*" {
            let major = parts[0].parse().map_err(|_| {
                SemverError::InvalidConstraint(format!("Invalid wildcard: {}", s))
            })?;
            let minor = parts[1].parse().map_err(|_| {
                SemverError::InvalidConstraint(format!("Invalid wildcard: {}", s))
            })?;
            return Ok(Constraint::Wildcard(major, Some(minor)));
        }

        Err(SemverError::InvalidConstraint(format!(
            "Invalid wildcard: {}",
            s
        )))
    }

    /// Check if a version satisfies this constraint
    pub fn matches(&self, version: &Version) -> bool {
        match self {
            Constraint::Any => true,

            Constraint::Exact(v) => {
                version.major == v.major
                    && version.minor == v.minor
                    && version.patch == v.patch
                    && version.prerelease == v.prerelease
            }

            Constraint::Caret(v) => {
                // ^1.2.3 := >=1.2.3 <2.0.0
                // ^0.2.3 := >=0.2.3 <0.3.0
                // ^0.0.3 := >=0.0.3 <0.0.4
                if v.major > 0 {
                    version >= v && version.major == v.major
                } else if v.minor > 0 {
                    version >= v && version.major == 0 && version.minor == v.minor
                } else {
                    version >= v
                        && version.major == 0
                        && version.minor == 0
                        && version.patch == v.patch
                }
            }

            Constraint::Tilde(v) => {
                // ~1.2.3 := >=1.2.3 <1.3.0
                version >= v && version.major == v.major && version.minor == v.minor
            }

            Constraint::GreaterThan(v) => version > v,
            Constraint::GreaterThanOrEqual(v) => version >= v,
            Constraint::LessThan(v) => version < v,
            Constraint::LessThanOrEqual(v) => version <= v,

            Constraint::Wildcard(major, minor) => {
                if let Some(m) = minor {
                    version.major == *major && version.minor == *m
                } else {
                    version.major == *major
                }
            }
        }
    }

    /// Get the minimum version that satisfies this constraint
    pub fn min_version(&self) -> Option<Version> {
        match self {
            Constraint::Exact(v)
            | Constraint::Caret(v)
            | Constraint::Tilde(v)
            | Constraint::GreaterThanOrEqual(v) => Some(v.clone()),

            Constraint::GreaterThan(v) => Some(v.bump_patch()),

            Constraint::Wildcard(major, minor) => {
                if let Some(m) = minor {
                    Some(Version::new(*major, *m, 0))
                } else {
                    Some(Version::new(*major, 0, 0))
                }
            }

            Constraint::Any | Constraint::LessThan(_) | Constraint::LessThanOrEqual(_) => None,
        }
    }
}

impl fmt::Display for Constraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Constraint::Any => write!(f, "*"),
            Constraint::Exact(v) => write!(f, "{}", v),
            Constraint::Caret(v) => write!(f, "^{}", v),
            Constraint::Tilde(v) => write!(f, "~{}", v),
            Constraint::GreaterThan(v) => write!(f, ">{}", v),
            Constraint::GreaterThanOrEqual(v) => write!(f, ">={}", v),
            Constraint::LessThan(v) => write!(f, "<{}", v),
            Constraint::LessThanOrEqual(v) => write!(f, "<={}", v),
            Constraint::Wildcard(major, Some(minor)) => write!(f, "{}.{}.*", major, minor),
            Constraint::Wildcard(major, None) => write!(f, "{}.*", major),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version() {
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
        assert!(v.prerelease.is_none());
        assert!(v.build.is_none());
    }

    #[test]
    fn test_parse_version_with_v_prefix() {
        let v = Version::parse("v1.2.3").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
    }

    #[test]
    fn test_parse_version_with_prerelease() {
        let v = Version::parse("1.2.3-alpha.1").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.prerelease, Some("alpha.1".to_string()));
    }

    #[test]
    fn test_parse_version_with_build() {
        let v = Version::parse("1.2.3+build.123").unwrap();
        assert_eq!(v.build, Some("build.123".to_string()));
    }

    #[test]
    fn test_version_ordering() {
        assert!(Version::new(1, 0, 0) < Version::new(2, 0, 0));
        assert!(Version::new(1, 2, 0) < Version::new(1, 3, 0));
        assert!(Version::new(1, 2, 3) < Version::new(1, 2, 4));
        assert!(Version::new(1, 2, 3) == Version::new(1, 2, 3));
    }

    #[test]
    fn test_parse_exact_constraint() {
        let c = Constraint::parse("1.2.3").unwrap();
        assert!(matches!(c, Constraint::Exact(_)));
    }

    #[test]
    fn test_parse_caret_constraint() {
        let c = Constraint::parse("^1.2.3").unwrap();
        assert!(matches!(c, Constraint::Caret(_)));
    }

    #[test]
    fn test_parse_tilde_constraint() {
        let c = Constraint::parse("~1.2.3").unwrap();
        assert!(matches!(c, Constraint::Tilde(_)));
    }

    #[test]
    fn test_exact_match() {
        let c = Constraint::parse("1.2.3").unwrap();
        assert!(c.matches(&Version::new(1, 2, 3)));
        assert!(!c.matches(&Version::new(1, 2, 4)));
        assert!(!c.matches(&Version::new(1, 3, 3)));
    }

    #[test]
    fn test_caret_match() {
        let c = Constraint::parse("^1.2.3").unwrap();

        // Should match 1.2.3 to <2.0.0
        assert!(c.matches(&Version::new(1, 2, 3)));
        assert!(c.matches(&Version::new(1, 2, 4)));
        assert!(c.matches(&Version::new(1, 3, 0)));
        assert!(c.matches(&Version::new(1, 9, 9)));

        // Should not match
        assert!(!c.matches(&Version::new(1, 2, 2)));
        assert!(!c.matches(&Version::new(2, 0, 0)));
    }

    #[test]
    fn test_caret_match_zero_major() {
        let c = Constraint::parse("^0.2.3").unwrap();

        // Should match 0.2.3 to <0.3.0
        assert!(c.matches(&Version::new(0, 2, 3)));
        assert!(c.matches(&Version::new(0, 2, 4)));

        // Should not match
        assert!(!c.matches(&Version::new(0, 2, 2)));
        assert!(!c.matches(&Version::new(0, 3, 0)));
        assert!(!c.matches(&Version::new(1, 0, 0)));
    }

    #[test]
    fn test_tilde_match() {
        let c = Constraint::parse("~1.2.3").unwrap();

        // Should match 1.2.3 to <1.3.0
        assert!(c.matches(&Version::new(1, 2, 3)));
        assert!(c.matches(&Version::new(1, 2, 4)));
        assert!(c.matches(&Version::new(1, 2, 9)));

        // Should not match
        assert!(!c.matches(&Version::new(1, 2, 2)));
        assert!(!c.matches(&Version::new(1, 3, 0)));
        assert!(!c.matches(&Version::new(2, 0, 0)));
    }

    #[test]
    fn test_greater_than() {
        let c = Constraint::parse(">1.2.3").unwrap();

        assert!(c.matches(&Version::new(1, 2, 4)));
        assert!(c.matches(&Version::new(1, 3, 0)));
        assert!(c.matches(&Version::new(2, 0, 0)));

        assert!(!c.matches(&Version::new(1, 2, 3)));
        assert!(!c.matches(&Version::new(1, 2, 2)));
    }

    #[test]
    fn test_wildcard() {
        let c = Constraint::parse("1.2.*").unwrap();

        assert!(c.matches(&Version::new(1, 2, 0)));
        assert!(c.matches(&Version::new(1, 2, 5)));
        assert!(c.matches(&Version::new(1, 2, 999)));

        assert!(!c.matches(&Version::new(1, 3, 0)));
        assert!(!c.matches(&Version::new(2, 2, 0)));
    }

    #[test]
    fn test_any_constraint() {
        let c = Constraint::parse("*").unwrap();

        assert!(c.matches(&Version::new(0, 0, 1)));
        assert!(c.matches(&Version::new(1, 2, 3)));
        assert!(c.matches(&Version::new(999, 999, 999)));
    }

    #[test]
    fn test_min_version() {
        let c = Constraint::parse("^1.2.3").unwrap();
        assert_eq!(c.min_version(), Some(Version::new(1, 2, 3)));

        let c = Constraint::parse("~1.2.3").unwrap();
        assert_eq!(c.min_version(), Some(Version::new(1, 2, 3)));

        let c = Constraint::parse(">=1.2.3").unwrap();
        assert_eq!(c.min_version(), Some(Version::new(1, 2, 3)));

        let c = Constraint::parse(">1.2.3").unwrap();
        assert_eq!(c.min_version(), Some(Version::new(1, 2, 4)));
    }
}
