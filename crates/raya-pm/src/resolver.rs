//! Dependency resolution
//!
//! Resolves package dependencies according to semver constraints.

use crate::lockfile::{Lockfile, LockedPackage, Source};
use crate::manifest::{Dependency, PackageManifest};
use crate::semver::{Constraint, Version};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

/// Errors that can occur during dependency resolution
#[derive(Debug, Error)]
pub enum ResolverError {
    /// Failed to parse semver constraint
    #[error("Failed to parse version constraint for {package}: {error}")]
    InvalidConstraint {
        package: String,
        error: crate::semver::SemverError,
    },

    /// No version found that satisfies constraint
    #[error("No version of {package} satisfies constraint {constraint}")]
    NoMatchingVersion { package: String, constraint: String },

    /// Circular dependency detected
    #[error("Circular dependency detected: {0}")]
    CircularDependency(String),

    /// Conflicting version requirements
    #[error("Conflicting requirements for {package}: {constraints:?}")]
    ConflictingRequirements {
        package: String,
        constraints: Vec<String>,
    },

    /// Package not found
    #[error("Package not found: {0}")]
    PackageNotFound(String),
}

/// Resolution strategy for handling conflicts
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConflictStrategy {
    /// Allow multiple major versions (relaxed)
    Relaxed,

    /// Fail on any version conflict (strict)
    Strict,
}

/// Resolved dependencies ready for lockfile generation
#[derive(Debug, Clone)]
pub struct ResolvedDependencies {
    /// Resolved packages (name → version)
    pub packages: HashMap<String, ResolvedPackage>,

    /// Dependency graph (package → dependencies)
    pub graph: HashMap<String, Vec<String>>,
}

/// A resolved package with its version and source
#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: Version,
    pub source: PackageSource,
    pub dependencies: Vec<String>,
}

/// Package source for resolution
#[derive(Debug, Clone, PartialEq)]
pub enum PackageSource {
    /// Registry package
    Registry { url: Option<String> },

    /// Git repository
    Git { url: String, rev: String },

    /// Local path
    Path { path: String },

    /// URL import (direct HTTP/HTTPS)
    Url { url: String },
}

/// Dependency resolver
pub struct DependencyResolver {
    /// Root manifest
    manifest: PackageManifest,

    /// Optional existing lockfile
    lockfile: Option<Lockfile>,

    /// Conflict resolution strategy
    strategy: ConflictStrategy,

    /// Available package versions (for testing/mocking)
    available_versions: HashMap<String, Vec<Version>>,
}

impl DependencyResolver {
    /// Create a new resolver
    pub fn new(manifest: PackageManifest) -> Self {
        Self {
            manifest,
            lockfile: None,
            strategy: ConflictStrategy::Relaxed,
            available_versions: HashMap::new(),
        }
    }

    /// Set the existing lockfile
    pub fn with_lockfile(mut self, lockfile: Lockfile) -> Self {
        self.lockfile = Some(lockfile);
        self
    }

    /// Set the conflict resolution strategy
    pub fn with_strategy(mut self, strategy: ConflictStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Set available versions for a package (for testing)
    pub fn with_available_versions(
        mut self,
        package: String,
        versions: Vec<Version>,
    ) -> Self {
        self.available_versions.insert(package, versions);
        self
    }

    /// Resolve all dependencies
    pub fn resolve(&self) -> Result<ResolvedDependencies, ResolverError> {
        let mut resolved = HashMap::new();
        let mut graph = HashMap::new();
        let mut constraints: HashMap<String, Vec<(String, Constraint)>> = HashMap::new();

        // Collect all constraints from manifest
        self.collect_constraints(&self.manifest, &mut constraints)?;

        // Resolve path dependencies first
        for (name, dep) in &self.manifest.dependencies {
            if dep.is_path() {
                let resolved_pkg = self.resolve_path_dependency(name, dep)?;
                graph.insert(name.clone(), resolved_pkg.dependencies.clone());
                resolved.insert(name.clone(), resolved_pkg);
            }
        }

        for (name, dep) in &self.manifest.dev_dependencies {
            if dep.is_path() {
                let resolved_pkg = self.resolve_path_dependency(name, dep)?;
                graph.insert(name.clone(), resolved_pkg.dependencies.clone());
                resolved.insert(name.clone(), resolved_pkg);
            }
        }

        // Resolve version-based dependencies
        for (package, package_constraints) in &constraints {
            let resolved_pkg = self.resolve_package(package, package_constraints)?;
            graph.insert(
                package.clone(),
                resolved_pkg.dependencies.clone(),
            );
            resolved.insert(package.clone(), resolved_pkg);
        }

        // Check for circular dependencies
        self.check_circular_dependencies(&graph)?;

        Ok(ResolvedDependencies {
            packages: resolved,
            graph,
        })
    }

    /// Collect all version constraints for packages
    fn collect_constraints(
        &self,
        manifest: &PackageManifest,
        constraints: &mut HashMap<String, Vec<(String, Constraint)>>,
    ) -> Result<(), ResolverError> {
        // Process runtime dependencies
        for (name, dep) in &manifest.dependencies {
            self.add_constraint(name, dep, &self.manifest.package.name, constraints)?;
        }

        // Process dev dependencies
        for (name, dep) in &manifest.dev_dependencies {
            self.add_constraint(name, dep, &self.manifest.package.name, constraints)?;
        }

        Ok(())
    }

    /// Add a constraint for a package
    fn add_constraint(
        &self,
        package: &str,
        dep: &Dependency,
        from: &str,
        constraints: &mut HashMap<String, Vec<(String, Constraint)>>,
    ) -> Result<(), ResolverError> {
        // Skip path and git dependencies for now (they don't use version constraints)
        if dep.is_path() || dep.is_git() {
            return Ok(());
        }

        if let Some(version_str) = dep.version() {
            let constraint = Constraint::parse(version_str).map_err(|e| {
                ResolverError::InvalidConstraint {
                    package: package.to_string(),
                    error: e,
                }
            })?;

            constraints
                .entry(package.to_string())
                .or_default()
                .push((from.to_string(), constraint));
        }

        Ok(())
    }

    /// Resolve a single package
    fn resolve_package(
        &self,
        package: &str,
        package_constraints: &[(String, Constraint)],
    ) -> Result<ResolvedPackage, ResolverError> {
        // Check if locked version exists and satisfies all constraints
        if let Some(ref lockfile) = self.lockfile {
            if let Some(locked) = lockfile.get_package(package) {
                if let Ok(locked_version) = Version::parse(&locked.version) {
                    if package_constraints
                        .iter()
                        .all(|(_, c)| c.matches(&locked_version))
                    {
                        // Use locked version
                        return Ok(ResolvedPackage {
                            name: package.to_string(),
                            version: locked_version,
                            source: Self::convert_source(&locked.source),
                            dependencies: locked.dependencies.clone(),
                        });
                    }
                }
            }
        }

        // Find compatible version
        let version = self.find_compatible_version(package, package_constraints)?;

        // For now, assume registry source
        Ok(ResolvedPackage {
            name: package.to_string(),
            version,
            source: PackageSource::Registry { url: None },
            dependencies: Vec::new(),
        })
    }

    /// Resolve a path dependency
    fn resolve_path_dependency(
        &self,
        package: &str,
        dep: &Dependency,
    ) -> Result<ResolvedPackage, ResolverError> {
        let path = dep.path().ok_or_else(|| {
            ResolverError::PackageNotFound(format!("{} (not a path dependency)", package))
        })?;

        // For path dependencies, we use version 0.0.0 as placeholder
        // The actual version will be read from the path's raya.toml later
        Ok(ResolvedPackage {
            name: package.to_string(),
            version: crate::semver::Version::new(0, 0, 0),
            source: PackageSource::Path {
                path: path.to_string_lossy().to_string(),
            },
            dependencies: Vec::new(),
        })
    }

    /// Find a version that satisfies all constraints
    fn find_compatible_version(
        &self,
        package: &str,
        constraints: &[(String, Constraint)],
    ) -> Result<Version, ResolverError> {
        // Get available versions
        let available = self
            .available_versions
            .get(package)
            .ok_or_else(|| ResolverError::PackageNotFound(package.to_string()))?;

        // Filter versions that satisfy all constraints
        // Exclude prerelease versions unless explicitly requested
        let mut compatible: Vec<&Version> = available
            .iter()
            .filter(|v| {
                // Check if constraints match
                let matches_constraints = constraints.iter().all(|(_, c)| c.matches(v));

                // Exclude prereleases unless the constraint explicitly allows them
                let allow_prerelease = v.prerelease.is_none();

                matches_constraints && allow_prerelease
            })
            .collect();

        if compatible.is_empty() {
            return Err(ResolverError::NoMatchingVersion {
                package: package.to_string(),
                constraint: constraints
                    .iter()
                    .map(|(_, c)| c.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            });
        }

        // Sort by version (latest first)
        compatible.sort_by(|a, b| b.cmp(a));

        // Return the latest compatible version
        Ok(compatible[0].clone())
    }

    /// Check for circular dependencies
    fn check_circular_dependencies(
        &self,
        graph: &HashMap<String, Vec<String>>,
    ) -> Result<(), ResolverError> {
        let mut visited = HashSet::new();
        let mut stack = HashSet::new();

        for package in graph.keys() {
            if !visited.contains(package) {
                self.check_cycle(package, graph, &mut visited, &mut stack)?;
            }
        }

        Ok(())
    }

    /// Check for cycles using DFS
    fn check_cycle(
        &self,
        package: &str,
        graph: &HashMap<String, Vec<String>>,
        visited: &mut HashSet<String>,
        stack: &mut HashSet<String>,
    ) -> Result<(), ResolverError> {
        visited.insert(package.to_string());
        stack.insert(package.to_string());

        if let Some(deps) = graph.get(package) {
            for dep in deps {
                if !visited.contains(dep) {
                    self.check_cycle(dep, graph, visited, stack)?;
                } else if stack.contains(dep) {
                    return Err(ResolverError::CircularDependency(format!(
                        "{} -> {}",
                        package, dep
                    )));
                }
            }
        }

        stack.remove(package);
        Ok(())
    }

    /// Convert lockfile source to package source
    fn convert_source(source: &Source) -> PackageSource {
        match source {
            Source::Registry { url } => PackageSource::Registry { url: url.clone() },
            Source::Git { url, rev } => PackageSource::Git {
                url: url.clone(),
                rev: rev.clone(),
            },
            Source::Path { path } => PackageSource::Path {
                path: path.clone(),
            },
            Source::Url { url } => PackageSource::Url { url: url.clone() },
        }
    }
}

impl ResolvedDependencies {
    /// Generate a lockfile from resolved dependencies
    pub fn to_lockfile(&self, root: Option<String>) -> Lockfile {
        let mut lockfile = Lockfile::new(root);

        for (name, pkg) in &self.packages {
            let source = match &pkg.source {
                PackageSource::Registry { url } => Source::Registry { url: url.clone() },
                PackageSource::Git { url, rev } => Source::Git {
                    url: url.clone(),
                    rev: rev.clone(),
                },
                PackageSource::Path { path } => Source::Path {
                    path: path.clone(),
                },
                PackageSource::Url { url } => Source::Url { url: url.clone() },
            };

            let mut locked = LockedPackage::new(
                name.clone(),
                pkg.version.to_string(),
                "0".repeat(64), // Placeholder checksum (will be filled by cache)
                source,
            );

            for dep in &pkg.dependencies {
                locked.add_dependency(dep.clone());
            }

            lockfile.add_package(locked);
        }

        lockfile.sort_packages();
        lockfile
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PackageInfo;

    fn create_test_manifest(name: &str) -> PackageManifest {
        PackageManifest {
            package: PackageInfo {
                name: name.to_string(),
                version: "1.0.0".to_string(),
                description: None,
                authors: vec![],
                license: None,
                repository: None,
                homepage: None,
                main: None,
            },
            dependencies: HashMap::new(),
            dev_dependencies: HashMap::new(),
        }
    }

    #[test]
    fn test_resolve_exact_version() {
        let mut manifest = create_test_manifest("app");
        manifest.dependencies.insert(
            "logging".to_string(),
            Dependency::Simple("1.2.3".to_string()),
        );

        let resolver = DependencyResolver::new(manifest).with_available_versions(
            "logging".to_string(),
            vec![
                Version::new(1, 2, 2),
                Version::new(1, 2, 3),
                Version::new(1, 2, 4),
            ],
        );

        let resolved = resolver.resolve().unwrap();
        let logging = &resolved.packages["logging"];
        assert_eq!(logging.version, Version::new(1, 2, 3));
    }

    #[test]
    fn test_resolve_caret_constraint() {
        let mut manifest = create_test_manifest("app");
        manifest.dependencies.insert(
            "http".to_string(),
            Dependency::Simple("^1.2.0".to_string()),
        );

        let resolver = DependencyResolver::new(manifest).with_available_versions(
            "http".to_string(),
            vec![
                Version::new(1, 1, 0),
                Version::new(1, 2, 0),
                Version::new(1, 2, 5),
                Version::new(1, 3, 0),
                Version::new(2, 0, 0),
            ],
        );

        let resolved = resolver.resolve().unwrap();
        let http = &resolved.packages["http"];

        // Should pick latest compatible: 1.3.0
        assert_eq!(http.version, Version::new(1, 3, 0));
    }

    #[test]
    fn test_resolve_tilde_constraint() {
        let mut manifest = create_test_manifest("app");
        manifest.dependencies.insert(
            "utils".to_string(),
            Dependency::Simple("~1.2.3".to_string()),
        );

        let resolver = DependencyResolver::new(manifest).with_available_versions(
            "utils".to_string(),
            vec![
                Version::new(1, 2, 3),
                Version::new(1, 2, 4),
                Version::new(1, 2, 9),
                Version::new(1, 3, 0),
            ],
        );

        let resolved = resolver.resolve().unwrap();
        let utils = &resolved.packages["utils"];

        // Should pick latest in ~1.2.3 range: 1.2.9
        assert_eq!(utils.version, Version::new(1, 2, 9));
    }

    #[test]
    fn test_no_matching_version() {
        let mut manifest = create_test_manifest("app");
        manifest.dependencies.insert(
            "missing".to_string(),
            Dependency::Simple("^2.0.0".to_string()),
        );

        let resolver = DependencyResolver::new(manifest).with_available_versions(
            "missing".to_string(),
            vec![Version::new(1, 0, 0), Version::new(1, 5, 0)],
        );

        let result = resolver.resolve();
        assert!(matches!(result, Err(ResolverError::NoMatchingVersion { .. })));
    }

    #[test]
    fn test_lockfile_generation() {
        let mut manifest = create_test_manifest("my-app");
        manifest.dependencies.insert(
            "logging".to_string(),
            Dependency::Simple("^1.0.0".to_string()),
        );

        let resolver = DependencyResolver::new(manifest).with_available_versions(
            "logging".to_string(),
            vec![Version::new(1, 0, 0), Version::new(1, 2, 0)],
        );

        let resolved = resolver.resolve().unwrap();
        let lockfile = resolved.to_lockfile(Some("my-app".to_string()));

        assert_eq!(lockfile.root, Some("my-app".to_string()));
        assert_eq!(lockfile.packages.len(), 1);

        let pkg = lockfile.get_package("logging").unwrap();
        assert_eq!(pkg.version, "1.2.0");
    }
}
