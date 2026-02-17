//! Raya POSIX Standard Library
//!
//! System-level native implementations for Raya: filesystem, networking,
//! HTTP server/client, environment, process management, OS info, and I/O.
//!
//! All I/O is synchronous. Async is achieved at the call site via
//! Raya's goroutine model: `async fs.readFile(path)`.

#![warn(missing_docs)]

pub mod handles;
pub mod registry;

pub mod env;
pub mod fs;
pub mod io;
pub mod os;
pub mod process;
pub mod net;
pub mod http;
pub mod fetch;

pub use registry::register_posix;
