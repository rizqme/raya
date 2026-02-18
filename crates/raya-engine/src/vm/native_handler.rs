//! Native call handler trait
//!
//! Re-exports from raya-sdk. Canonical definitions live in the SDK crate.
//! The engine provides `EngineContext` as the concrete `NativeContext` impl.

pub use raya_sdk::{NativeHandler, NativeCallResult, NoopNativeHandler};
