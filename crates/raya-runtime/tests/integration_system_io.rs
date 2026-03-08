//! raya-runtime integration tests — system, io, networking, and stdlib modules.

#[path = "e2e/harness.rs"]
mod harness;
pub use harness::*;
#[path = "e2e/archive.rs"]
mod archive;
#[path = "e2e/crypto.rs"]
mod crypto;
#[path = "e2e/dns.rs"]
mod dns;
#[path = "e2e/encoding.rs"]
mod encoding;
#[path = "e2e/env.rs"]
mod env;
#[path = "e2e/fetch.rs"]
mod fetch;
#[path = "e2e/fs.rs"]
mod fs;
#[path = "e2e/glob.rs"]
mod glob;
#[path = "e2e/hardening.rs"]
mod hardening;
#[path = "e2e/http.rs"]
mod http;
#[path = "e2e/io.rs"]
mod io;
#[path = "e2e/json.rs"]
mod json;
#[path = "e2e/logger.rs"]
mod logger;
#[path = "e2e/net.rs"]
mod net;
#[path = "e2e/node_stdlib.rs"]
mod node_stdlib;
#[path = "e2e/os.rs"]
mod os;
#[path = "e2e/path.rs"]
mod path;
#[path = "e2e/process.rs"]
mod process;
#[path = "e2e/semver.rs"]
mod semver;
#[path = "e2e/std_imports.rs"]
mod std_imports;
#[path = "e2e/url.rs"]
mod url;
