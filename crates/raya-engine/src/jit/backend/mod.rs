//! Backend-agnostic code generation traits and implementations

pub mod traits;
pub mod stub;
pub mod cranelift;

pub use traits::{
    CodegenBackend, CodegenError, CompiledCode, ExecutableCode,
    ModuleContext, Relocation, RelocationTarget, RuntimeHelper,
    StackMapEntry, PointerLocation, DeoptInfo, SymbolResolver,
    TargetInfo, TargetArch,
};
pub use stub::StubBackend;
pub use self::cranelift::CraneliftBackend;
