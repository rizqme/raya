//! Raya Package Manager Library
//!
//! This crate provides package management functionality for Raya, including:
//! - Module caching (content-addressable storage)
//! - Package manifest parsing (raya.toml)
//! - Lockfile management (raya.lock)
//! - Semver version parsing and constraint matching
//! - Dependency resolution
//! - Local path dependency resolution
//! - Package installation and updates

pub mod cache;
pub mod lockfile;
pub mod manifest;
pub mod path;
pub mod resolver;
pub mod semver;

pub use cache::{Cache, CacheError, ModuleMetadata};
pub use lockfile::{Lockfile, LockfileError, LockedPackage, Source};
pub use manifest::{Dependency, ManifestError, PackageInfo, PackageManifest};
pub use path::{find_project_root, PathError, PathResolver};
pub use resolver::{
    ConflictStrategy, DependencyResolver, PackageSource, ResolvedDependencies, ResolvedPackage,
    ResolverError,
};
pub use semver::{Constraint, SemverError, Version};
