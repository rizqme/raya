//! Backend-agnostic code generation traits and implementations

pub mod cranelift;
pub mod traits;

pub use self::cranelift::CraneliftBackend;
pub use traits::{
    CodegenBackend, CodegenError, CompiledCode, ExecutableCode, ModuleContext, PointerLocation,
    Relocation, RelocationTarget, RuntimeHelper, StackMapEntry, SymbolResolver, TargetArch,
    TargetInfo,
};
