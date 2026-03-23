//! Virtual machine execution and context management

mod capabilities;
mod class_registry;
mod context;
mod core;
pub mod debug_state;
pub(crate) mod execution;
mod handlers;
mod marshal;
mod module_registry;
mod native_module_registry;
mod opcodes;
mod safepoint;
mod shared_state;
mod vm_facade;

pub use capabilities::{Capability, CapabilityError, CapabilityRegistry};
pub use capabilities::{HttpCapability, LogCapability, ReadCapability};
pub use class_registry::{ClassRegistry, RuntimeLayoutRegistry};
pub use context::{
    ContextRegistry, ResourceCounters, ResourceLimits, VmContext, VmContextId, VmOptions,
};
pub use core::Interpreter;
pub use debug_state::DebugState;
pub use execution::{ControlFlow, ExecutionFrame, ExecutionResult, OpcodeResult, ReturnAction};
pub use marshal::{marshal, unmarshal, ForeignHandleManager, MarshalError, MarshalledValue};
pub use module_registry::ModuleRegistry;
pub use native_module_registry::{NativeFn, NativeModule, NativeModuleRegistry};
pub use safepoint::{SafepointCoordinator, StopReason};
#[cfg(feature = "jit")]
pub use shared_state::{JitTelemetry, JitTelemetrySnapshot};
pub use shared_state::{
    JsGlobalBindingRecord, ModuleRuntimeLayout, PromiseMicrotask, PropertyKeyRegistry,
    RuntimeTypeHandleRegistry, ShapeAdapter, SharedVmState, StructuralAdapterKey,
    StructuralSlotBinding,
};
pub use vm_facade::{Vm, VmStats};
