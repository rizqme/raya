//! Bundle format, loading, and VFS
//!
//! Handles the AOT bundle appended to the raya binary:
//! - **format**: binary payload format (trailer, code section, function table, VFS)
//! - **loader**: self-detection, code mmap, function pointer table
//! - **vfs**: virtual filesystem (DiskBacked for dev, Embedded for bundle)

pub mod format;
pub mod loader;
pub mod vfs;

pub use format::{AotTrailer, TRAILER_MAGIC, TRAILER_SIZE};
pub use loader::{AotCodeRegion, BundlePayload};
pub use vfs::{Vfs, VfsEntry};
