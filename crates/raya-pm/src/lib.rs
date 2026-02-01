//! Raya Package Manager Library
//!
//! This crate provides package management functionality for Raya, including:
//! - Module caching (content-addressable storage)
//! - Package manifest parsing (raya.toml)
//! - Lockfile management (raya.lock)
//! - Semver version parsing and constraint matching
//! - Dependency resolution
//! - Local path dependency resolution
//! - Package registry client
//! - Package installation and updates
//! - URL imports (direct HTTP/HTTPS)

pub mod cache;
pub mod commands;
pub mod lockfile;
pub mod manifest;
pub mod path;
pub mod registry;
pub mod resolver;
pub mod semver;
pub mod url;

pub use cache::{Cache, CacheError, ModuleMetadata};
pub use commands::{add_package, init_project, install_dependencies};
pub use lockfile::{Lockfile, LockfileError, LockedPackage, Source};
pub use manifest::{Dependency, ManifestError, PackageInfo, PackageManifest};
pub use path::{find_project_root, PathError, PathResolver};
pub use registry::{PackageMetadata, RegistryClient, RegistryError, VersionInfo};
pub use resolver::{
    ConflictStrategy, DependencyResolver, PackageSource, ResolvedDependencies, ResolvedPackage,
    ResolverError,
};
pub use semver::{Constraint, SemverError, Version};
pub use url::{CachedUrl, UrlCache, UrlCacheError, UrlFetcher};
