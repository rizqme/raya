//! NativeHandler trait — stdlib dispatch interface
//!
//! Moved from raya-engine to raya-sdk so that stdlib implementations
//! can compile against SDK alone.

use crate::context::NativeContext;
use crate::value::NativeValue;

// ============================================================================
// IO Request / Completion types (for event loop integration)
// ============================================================================

/// IO request submitted by a native handler that needs to suspend.
///
/// When a native handler cannot complete synchronously (e.g., file read,
/// network accept, channel receive), it returns `NativeCallResult::Suspend(IoRequest)`.
/// The VM's event loop processes the request and resumes the task when done.
pub enum IoRequest {
    /// Run blocking work on the IO thread pool (fs ops, process exec, stdin).
    /// The closure runs on a pool thread and returns an `IoCompletion`.
    BlockingWork {
        /// Work to execute on the IO thread pool
        work: Box<dyn FnOnce() -> IoCompletion + Send>,
    },
    /// Non-blocking channel receive — yield until value available or closed
    ChannelReceive {
        /// The channel NativeValue to receive from
        channel: NativeValue,
    },
    /// Non-blocking channel send — yield until space available or closed
    ChannelSend {
        /// The channel NativeValue to send to
        channel: NativeValue,
        /// The value to send
        value: NativeValue,
    },
    /// Accept a connection on a network listener (non-blocking via poller)
    NetAccept {
        /// Listener handle ID
        handle: u64,
    },
    /// Read from a network stream (non-blocking via poller)
    NetRead {
        /// Stream handle ID
        handle: u64,
        /// Maximum bytes to read
        max_bytes: usize,
    },
    /// Write to a network stream (non-blocking via poller)
    NetWrite {
        /// Stream handle ID
        handle: u64,
        /// Data to write
        data: Vec<u8>,
    },
    /// Connect to a remote TCP address (non-blocking via poller)
    NetConnect {
        /// Address to connect to (host:port)
        addr: String,
    },
}

/// Result of a completed IO operation from a pool thread.
///
/// Pool threads don't have access to the GC, so they return raw data.
/// The event loop converts these to GC-allocated `Value`s when resuming tasks.
pub enum IoCompletion {
    /// Raw bytes — event loop allocates as Buffer
    Bytes(Vec<u8>),
    /// Raw string — event loop allocates as RayaString
    String(String),
    /// Primitive value (i32, f64, bool, null) — no allocation needed
    Primitive(NativeValue),
    /// Error — event loop sets exception on task
    Error(String),
}

// ============================================================================
// NativeCallResult
// ============================================================================

/// Result of a native call handler
pub enum NativeCallResult {
    /// Call handled successfully, returned a value
    Value(NativeValue),
    /// Native call ID not recognized by this handler
    Unhandled,
    /// Call failed with an error
    Error(String),
    /// Handler cannot complete synchronously — submit IO request and suspend task.
    /// The event loop processes the request and resumes the task when done.
    Suspend(IoRequest),
}

impl NativeCallResult {
    /// Create a successful result with null value
    #[inline]
    pub fn null() -> Self {
        Self::Value(NativeValue::null())
    }

    /// Create a successful result with an i32 value
    #[inline]
    pub fn i32(val: i32) -> Self {
        Self::Value(NativeValue::i32(val))
    }

    /// Create a successful result with an f64 value
    #[inline]
    pub fn f64(val: f64) -> Self {
        Self::Value(NativeValue::f64(val))
    }

    /// Create a successful result with a bool value
    #[inline]
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
/// All handlers receive a `&dyn NativeContext` providing full VM
/// access (GC, class registry, scheduler) without depending on
/// engine internals.
pub trait NativeHandler: Send + Sync {
    /// Handle a native call
    ///
    /// - `ctx`: VM context (GC, class registry, scheduler access)
    /// - `id`: The native call ID (u16)
    /// - `args`: Array of NativeValue arguments
    ///
    /// Returns `NativeCallResult::Unhandled` if the ID is not recognized.
    fn call(&self, ctx: &dyn NativeContext, id: u16, args: &[NativeValue]) -> NativeCallResult;
}

/// A no-op handler that returns `Unhandled` for all calls
pub struct NoopNativeHandler;

impl NativeHandler for NoopNativeHandler {
    fn call(
        &self,
        _ctx: &dyn NativeContext,
        _id: u16,
        _args: &[NativeValue],
    ) -> NativeCallResult {
        NativeCallResult::Unhandled
    }
}

// ============================================================================
// Native Function Registry (name-based dispatch)
// ============================================================================

use std::collections::HashMap;
use std::sync::Arc;

/// A native function handler (for symbolic name-based dispatch)
pub type NativeHandlerFn = Arc<dyn Fn(&dyn NativeContext, &[NativeValue]) -> NativeCallResult + Send + Sync>;

/// Registry of native functions indexed by symbolic name.
///
/// Used at module load time to resolve symbolic native call names
/// (stored in bytecode) to handler functions. Stdlib modules register
/// their handlers here (e.g., "math.abs", "logger.info").
pub struct NativeFunctionRegistry {
    handlers: HashMap<String, NativeHandlerFn>,
}

impl NativeFunctionRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a native function by name
    pub fn register(
        &mut self,
        name: &str,
        handler: impl Fn(&dyn NativeContext, &[NativeValue]) -> NativeCallResult + Send + Sync + 'static,
    ) {
        self.handlers.insert(name.to_string(), Arc::new(handler));
    }

    /// Get a handler by name (used at link time)
    pub fn get(&self, name: &str) -> Option<NativeHandlerFn> {
        self.handlers.get(name).cloned()
    }

    /// Check if a handler is registered
    pub fn contains(&self, name: &str) -> bool {
        self.handlers.contains_key(name)
    }

    /// Get the number of registered handlers
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}

impl Default for NativeFunctionRegistry {
    fn default() -> Self {
        Self::new()
    }
}
