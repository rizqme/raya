//! Raya Runtime
//!
//! Binds the Raya engine with the standard library implementation.
//! Re-exports `StdNativeHandler` from `raya-stdlib` for backward compatibility.

pub use raya_stdlib::StdNativeHandler;
pub use raya_stdlib_posix::register_posix;
