//! Virtual machine execution and context management

mod capabilities;
mod class_registry;
mod context;
mod execution;
mod handlers;
mod interpreter;
mod task_interpreter;
mod lifecycle;
mod marshal;
mod module_registry;
mod native_module_registry;
mod safepoint;
mod shared_state;

pub use capabilities::{Capability, CapabilityError, CapabilityRegistry};
pub use capabilities::{HttpCapability, LogCapability, ReadCapability};
pub use class_registry::ClassRegistry;
pub use context::{
    ContextRegistry, ResourceCounters, ResourceLimits, VmContext, VmContextId, VmOptions,
};
pub use execution::{ExecutionResult, OpcodeResult};
pub use interpreter::Vm;
pub use lifecycle::{
    ContextSnapshot, FrameSnapshot, TaskSnapshot, Vm as InnerVm, VmError, VmSnapshot, VmStats,
};
pub use marshal::{marshal, unmarshal, ForeignHandleManager, MarshalError, MarshalledValue};
pub use module_registry::ModuleRegistry;
pub use native_module_registry::{NativeFn, NativeModule, NativeModuleRegistry};
pub use safepoint::{SafepointCoordinator, StopReason};
pub use shared_state::{SharedVmState, TaskExecutor};
pub use task_interpreter::TaskInterpreter;
