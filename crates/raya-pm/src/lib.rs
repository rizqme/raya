//! Raya Package Manager Library
//!
//! This crate provides package management data types and utilities:
//! - Module caching (content-addressable storage)
//! - Package manifest parsing (raya.toml)
//! - Lockfile management (raya.lock)
//! - Semver version parsing and constraint matching
//! - Local path dependency resolution
//! - URL import caching
//!
//! PM commands (init, install, add, remove, update) are implemented
//! in Raya via the `std:pm` stdlib module.

pub mod cache;
pub mod lockfile;
pub mod manifest;
pub mod path;
pub mod semver;
pub mod url;

pub use cache::{Cache, CacheError, ModuleMetadata};
pub use lockfile::{Lockfile, LockfileError, LockedPackage, Source};
pub use manifest::{
    AssetsConfig, BundleConfig, Dependency, ManifestError, PackageInfo, PackageManifest,
    RegistryConfig,
};
pub use path::{find_project_root, PathError, PathResolver};
pub use semver::{Constraint, SemverError, Version};
pub use url::{CachedUrl, UrlCache, UrlCacheError};
