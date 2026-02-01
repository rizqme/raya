//! Registry client for Raya package registry
//!
//! Provides HTTP client for interacting with the raya.dev package registry.

mod api;
mod client;

pub use api::{
    PackageMetadata, PackageVersion, VersionDownload, VersionInfo,
};
pub use client::{RegistryClient, RegistryError};
