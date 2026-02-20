//! URL imports module
//!
//! Handles cache lookup for remote modules imported from HTTP/HTTPS URLs.
//! Fetching and caching is now handled by std:pm in Raya.

pub mod cache;

pub use cache::{CachedUrl, UrlCache, UrlCacheError};
