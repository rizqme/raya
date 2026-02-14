//! Native call handler trait
//!
//! Defines the interface for external native function implementations.
//! The VM calls into this trait when it encounters a NativeCall opcode
//! for IDs in the stdlib range. This allows the engine to remain
//! independent of any specific stdlib implementation.

use crate::vm::abi::{NativeContext, NativeValue};

/// Result of a native call handler
pub enum NativeCallResult {
    /// Call handled successfully, returned a value
    Value(NativeValue),
    /// Native call ID not recognized by this handler
    Unhandled,
    /// Call failed with an error
    Error(String),
}

impl NativeCallResult {
    /// Create a successful result with null value
    pub fn null() -> Self {
        Self::Value(NativeValue::null())
    }

    /// Create a successful result with an i32 value
    pub fn i32(val: i32) -> Self {
        Self::Value(NativeValue::i32(val))
    }

    /// Create a successful result with an f64 value
    pub fn f64(val: f64) -> Self {
        Self::Value(NativeValue::f64(val))
    }

    /// Create a successful result with a bool value
    pub fn bool(val: bool) -> Self {
        Self::Value(NativeValue::bool(val))
    }
}

/// Trait for handling native calls from the VM
///
/// Implementors provide native function implementations for specific
/// native call ID ranges. The VM dispatches to registered handlers
/// when executing NativeCall opcodes.
///
/// All handlers receive full VM context (GC, class registry, scheduler)
/// and can work with any value type (primitives, strings, buffers, objects).
///
/// This trait enables different stdlib implementations for different
/// targets (desktop, embedded, WASM, etc.) without coupling the engine
/// to any specific implementation.
pub trait NativeHandler: Send + Sync {
    /// Handle a native call with full VM context
    ///
    /// - `ctx`: Context with GC, class registry, scheduler access
    /// - `id`: The native call ID (u16)
    /// - `args`: Array of NativeValue arguments (typed values)
    ///
    /// Returns `NativeCallResult::Unhandled` if the ID is not recognized.
    fn call(&self, ctx: &NativeContext, id: u16, args: &[NativeValue]) -> NativeCallResult;
}

/// A no-op handler that returns `Unhandled` for all calls
pub struct NoopNativeHandler;

impl NativeHandler for NoopNativeHandler {
    fn call(&self, _ctx: &NativeContext, _id: u16, _args: &[NativeValue]) -> NativeCallResult {
        NativeCallResult::Unhandled
    }
}
