//! Virtual machine execution and context management

mod capabilities;
mod class_registry;
mod context;
mod interpreter;
mod lifecycle;
mod marshal;
mod safepoint;

pub use capabilities::{Capability, CapabilityError, CapabilityRegistry};
pub use capabilities::{HttpCapability, LogCapability, ReadCapability};
pub use class_registry::ClassRegistry;
pub use context::{
    ContextRegistry, ResourceCounters, ResourceLimits, VmContext, VmContextId, VmOptions,
};
pub use interpreter::Vm;
pub use lifecycle::{
    ContextSnapshot, FrameSnapshot, TaskSnapshot, VmError, VmSnapshot, VmStats,
    Vm as InnerVm,
};
pub use marshal::{marshal, unmarshal, ForeignHandleManager, MarshalError, MarshalledValue};
pub use safepoint::{SafepointCoordinator, StopReason};
