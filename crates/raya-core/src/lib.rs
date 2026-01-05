//! Raya VM Core Runtime
//!
//! This crate provides the virtual machine runtime including:
//! - Bytecode interpreter
//! - Task scheduler (goroutine-style green threads)
//! - Garbage collector
//! - Object model and memory management
//! - Synchronization primitives (Mutex)

#![warn(missing_docs)]
#![warn(rust_2018_idioms)]

pub mod value;
pub mod types;
pub mod gc;
pub mod stack;
pub mod vm;
pub mod scheduler;
pub mod object;
pub mod sync;
pub mod json;

pub use value::Value;
pub use types::{PointerMap, TypeInfo, TypeRegistry};
pub use stack::{Stack, CallFrame, StackStats};
pub use vm::{Vm, VmContext, VmContextId, VmOptions, ResourceLimits, ResourceCounters, ContextRegistry, ClassRegistry};
pub use scheduler::Scheduler;
pub use object::{Object, Class, VTable, Array, RayaString};
pub use json::{JsonValue, TypeKind, TypeSchema, TypeSchemaRegistry, validate_cast};

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
}

/// VM execution result
pub type VmResult<T> = Result<T, VmError>;
