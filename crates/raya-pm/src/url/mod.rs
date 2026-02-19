//! URL imports module
//!
//! Handles fetching and caching remote modules from HTTP/HTTPS URLs.
//!
//! ## Supported URL formats
//!
//! - `https://github.com/user/repo/archive/v1.0.0.tar.gz` - GitHub release archives
//! - `https://pkg.raya.dev/lib@1.0.0` - Raya package registry direct links
//! - `https://example.com/module.tar.gz` - Generic tar.gz archives
//! - `https://example.com/module.ryb` - Direct bytecode files
//!
//! ## Usage
//!
//! ```ignore
//! use raya_pm::url::{UrlCache, UrlFetcher};
//!
//! // Create cache
//! let cache = UrlCache::default_cache();
//!
//! // Fetch and cache a URL
//! let (cached, locked) = cache.fetch_and_cache("https://github.com/user/repo/archive/v1.0.0.tar.gz")?;
//!
//! // Add to lockfile
//! lockfile.add_package(locked);
//!
//! // Find entry point
//! let entry = cache.find_entry_point(&cached);
//! ```
//!
//! ## Cache structure
//!
//! URL imports are cached in `~/.raya/cache/<sha256>/`:
//! ```text
//! ~/.raya/cache/
//! ├── <sha256>/               # Content-addressed by SHA-256 of downloaded content
//! │   ├── module.ryb          # Compiled bytecode (if pre-compiled)
//! │   ├── module.d.raya       # Type definitions (if available)
//! │   ├── raya.toml           # Package manifest (if available)
//! │   ├── src/                # Source files (if archive)
//! │   └── ...
//! ```

pub mod cache;
pub mod fetch;

pub use cache::{CachedUrl, UrlCache, UrlCacheError};
pub use fetch::{
    compute_checksum, compute_file_checksum, extract_package_name, extract_version, FetchError,
    FetchResult, UrlFetcher, MAX_CONTENT_SIZE, REQUEST_TIMEOUT,
};
