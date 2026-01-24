//! Virtual machine execution and context management

mod capabilities;
mod class_registry;
mod context;
mod interpreter;
mod lifecycle;
mod marshal;
mod module_registry;
mod native_module_registry;
mod safepoint;

pub use capabilities::{Capability, CapabilityError, CapabilityRegistry};
pub use capabilities::{HttpCapability, LogCapability, ReadCapability};
pub use class_registry::ClassRegistry;
pub use context::{
    ContextRegistry, ResourceCounters, ResourceLimits, VmContext, VmContextId, VmOptions,
};
pub use interpreter::Vm;
pub use lifecycle::{
    ContextSnapshot, FrameSnapshot, TaskSnapshot, Vm as InnerVm, VmError, VmSnapshot, VmStats,
};
pub use marshal::{marshal, unmarshal, ForeignHandleManager, MarshalError, MarshalledValue};
pub use module_registry::ModuleRegistry;
pub use native_module_registry::{NativeFn, NativeModule, NativeModuleRegistry};
pub use safepoint::{SafepointCoordinator, StopReason};
