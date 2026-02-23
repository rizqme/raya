//! Raya Standard Library
//!
//! Native implementations for standard library functions.
//! Includes the `StdNativeHandler` that routes native call IDs
//! to their Rust implementations.

#![warn(missing_docs)]

pub mod compress;
pub mod crypto;
pub mod encoding;
pub mod handler;
pub mod json_toml;
pub mod logger;
pub mod math;
pub mod path;
pub mod registry;
pub mod semver_mod;
pub mod stream;
pub mod template;
pub mod test;
pub mod url;

pub use handler::StdNativeHandler;
pub use registry::register_stdlib;
