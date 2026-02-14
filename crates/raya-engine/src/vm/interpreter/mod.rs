//! Virtual machine execution and context management

mod capabilities;
mod class_registry;
mod context;
mod core;
mod exec_context;
mod execution;
mod lifecycle;
mod marshal;
mod module_registry;
mod native_module_registry;
pub mod opcodes;
mod safepoint;
mod shared_state;
mod vm_facade;

pub use capabilities::{Capability, CapabilityError, CapabilityRegistry};
pub use capabilities::{HttpCapability, LogCapability, ReadCapability};
pub use class_registry::ClassRegistry;
pub use context::{
    ContextRegistry, ResourceCounters, ResourceLimits, VmContext, VmContextId, VmOptions,
};
pub use core::Interpreter;
pub use exec_context::{AsyncContext, ExecutionContext, SyncContext};
pub use execution::{ControlFlow, ExecutionResult, OpcodeResult};
pub use lifecycle::{
    ContextSnapshot, FrameSnapshot, TaskSnapshot, Vm as InnerVm, VmError, VmSnapshot, VmStats,
};
pub use marshal::{marshal, unmarshal, ForeignHandleManager, MarshalError, MarshalledValue};
pub use module_registry::ModuleRegistry;
pub use native_module_registry::{NativeFn, NativeModule, NativeModuleRegistry};
pub use safepoint::{SafepointCoordinator, StopReason};
pub use shared_state::{SharedVmState, TaskExecutor};
pub use vm_facade::Vm;
