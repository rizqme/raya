//! Backend-agnostic code generation traits and implementations

pub mod cranelift;
pub mod stub;
pub mod traits;

pub use self::cranelift::CraneliftBackend;
pub use stub::StubBackend;
pub use traits::{
    CodegenBackend, CodegenError, CompiledCode, DeoptInfo, ExecutableCode, ModuleContext,
    PointerLocation, Relocation, RelocationTarget, RuntimeHelper, StackMapEntry, SymbolResolver,
    TargetArch, TargetInfo,
};
