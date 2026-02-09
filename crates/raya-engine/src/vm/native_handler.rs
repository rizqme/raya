//! Native call handler trait
//!
//! Defines the interface for external native function implementations.
//! The VM calls into this trait when it encounters a NativeCall opcode
//! for IDs in the stdlib range. This allows the engine to remain
//! independent of any specific stdlib implementation.

/// Result of a native call handler
pub enum NativeCallResult {
    /// Call handled successfully, returned a string value
    String(String),
    /// Call handled successfully, returned a numeric value
    Number(f64),
    /// Call handled successfully, returned an integer value
    Integer(i32),
    /// Call handled successfully, returned a boolean value
    Bool(bool),
    /// Call handled successfully, returned null/void
    Void,
    /// Native call ID not recognized by this handler
    Unhandled,
    /// Call failed with an error
    Error(String),
}

/// Trait for handling native calls from the VM
///
/// Implementors provide native function implementations for specific
/// native call ID ranges. The VM dispatches to registered handlers
/// when executing NativeCall opcodes.
///
/// This trait enables different stdlib implementations for different
/// targets (desktop, embedded, WASM, etc.) without coupling the engine
/// to any specific implementation.
pub trait NativeHandler: Send + Sync {
    /// Handle a native call
    ///
    /// - `id`: The native call ID (u16)
    /// - `args`: String representations of the arguments
    ///
    /// Returns `NativeCallResult::Unhandled` if the ID is not recognized.
    fn call(&self, id: u16, args: &[String]) -> NativeCallResult;
}

/// A no-op handler that returns `Unhandled` for all calls
pub struct NoopNativeHandler;

impl NativeHandler for NoopNativeHandler {
    fn call(&self, _id: u16, _args: &[String]) -> NativeCallResult {
        NativeCallResult::Unhandled
    }
}
