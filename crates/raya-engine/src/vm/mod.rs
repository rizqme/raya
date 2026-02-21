//! Raya VM Core Runtime
//!
//! This crate provides the virtual machine runtime including:
//! - Bytecode interpreter
//! - Task scheduler (goroutine-style green threads)
//! - Garbage collector
//! - Object model and memory management
//! - Synchronization primitives (Mutex)

// Note: clippy allows are configured at the crate level in lib.rs

pub mod abi;
pub mod builtin;
pub mod defaults;
pub mod builtins;
pub mod ffi;
pub mod gc;
pub mod json;
pub mod module;
pub mod native_handler;
pub mod native_registry;
pub mod object;
pub mod reflect;
pub mod scheduler;
pub mod snapshot;
pub mod stack;
pub mod sync;
pub mod types;
pub mod value;
pub mod interpreter;

// Re-export SDK types (canonical definitions live in raya-sdk)
pub use raya_sdk::{
    NativeValue, NativeContext, NativeHandler, NativeCallResult, NoopNativeHandler,
    NativeArray, NativeObject, ObjectSchema, NativeClass, NativeFunction, NativeMethod, NativeTask,
    AbiResult, ClassInfo, NativeError, FromNativeObject, ToNativeObject,
};

// Re-export engine-specific ABI types
pub use abi::{
    EngineContext,
    value_to_native, native_to_value,
    // Backward-compatible free functions
    array_allocate, array_get, array_length, buffer_allocate, buffer_read_bytes, class_get_info,
    object_allocate, object_class_id, object_get_field, object_set_field, string_allocate,
    string_read, task_cancel, task_is_done, task_spawn,
};

pub use json::{validate_cast, JsonValue, TypeKind, TypeSchema, TypeSchemaRegistry};
pub use native_registry::{NativeFn, NativeFunctionRegistry, ResolvedNatives};
pub use object::{Array, BoundMethod, Class, Object, RayaString, VTable};
pub use scheduler::Scheduler;
pub use snapshot::{SnapshotReader, SnapshotWriter};
pub use stack::{CallFrame, Stack, StackStats};
pub use sync::{Mutex, MutexError, MutexId, MutexRegistry};
pub use types::{PointerMap, TypeInfo, TypeRegistry};
pub use value::Value;
pub use interpreter::{
    ClassRegistry, ContextRegistry, ResourceCounters, ResourceLimits, Vm, VmContext, VmContextId,
    VmOptions, VmStats,
};

/// VM execution errors
#[derive(Debug, thiserror::Error)]
pub enum VmError {
    /// Stack overflow
    #[error("Stack overflow")]
    StackOverflow,

    /// Stack underflow
    #[error("Stack underflow")]
    StackUnderflow,

    /// Invalid opcode
    #[error("Invalid opcode: {0}")]
    InvalidOpcode(u8),

    /// Null pointer exception
    #[error("Null pointer exception")]
    NullPointer,

    /// Type error
    #[error("Type error: {0}")]
    TypeError(String),

    /// Runtime error
    #[error("Runtime error: {0}")]
    RuntimeError(String),

    /// Snapshot error
    #[error("Snapshot error: {0}")]
    SnapshotError(String),

    /// IO error (file not found, permission denied, etc.)
    #[error("IO error: {0}")]
    IoError(String),

    /// Invalid binary format
    #[error("Invalid binary format: {0}")]
    InvalidBinaryFormat(String),

    /// Checksum mismatch (module integrity verification failed)
    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch {
        /// Expected checksum value
        expected: String,
        /// Actual checksum value
        actual: String,
    },

    /// Task preempted (should be rescheduled, not failed)
    #[error("Task preempted")]
    TaskPreempted,

    /// Task suspended waiting for another task (yield to allow other tasks to run)
    #[error("Task suspended")]
    Suspended,
}

/// VM execution result
pub type VmResult<T> = Result<T, VmError>;
