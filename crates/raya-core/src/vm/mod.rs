//! Virtual machine execution and context management

mod class_registry;
mod context;
mod interpreter;

pub use class_registry::ClassRegistry;
pub use context::{
    ContextRegistry, ResourceCounters, ResourceLimits, VmContext, VmContextId, VmOptions,
};
pub use interpreter::Vm;
