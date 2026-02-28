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

pub mod tls;

pub mod archive;
pub mod dns;
pub mod env;
pub mod fetch;
pub mod fs;
pub mod glob_mod;
pub mod http;
pub mod http2;
pub mod io;
pub mod net;
pub mod os;
pub mod process;
pub mod readline;
pub mod sqlite;
pub mod terminal;
pub mod watch;
pub mod ws;

pub use registry::register_posix;
