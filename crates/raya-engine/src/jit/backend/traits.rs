//! Backend-agnostic code generation traits
//!
//! Defines the `CodegenBackend` trait that pluggable backends (Cranelift, LLVM, etc.)
//! implement, along with types for compiled code, relocations, and stack maps.

use crate::jit::ir::instr::JitFunction;

/// Target architecture
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetArch {
    X86_64,
    AArch64,
}

/// Target information
#[derive(Debug, Clone)]
pub struct TargetInfo {
    pub arch: TargetArch,
    pub pointer_size: usize,
}

/// Error during code generation
#[derive(Debug, thiserror::Error)]
pub enum CodegenError {
    #[error("Backend error: {0}")]
    BackendError(String),
    #[error("Unsupported instruction: {0}")]
    UnsupportedInstruction(String),
    #[error("Register allocation failed: {0}")]
    RegisterAllocationFailed(String),
}

/// A relocation target for code patching
#[derive(Debug, Clone)]
pub enum RelocationTarget {
    /// Pointer to a runtime helper function
    RuntimeHelper(RuntimeHelper),
    /// Pointer to another JIT-compiled function
    JitFunction(u32),
    /// Absolute address
    Absolute(usize),
}

/// A relocation entry (address to patch after code is placed in memory)
#[derive(Debug, Clone)]
pub struct Relocation {
    /// Offset in the generated code where the relocation applies
    pub code_offset: usize,
    /// What address to patch in
    pub target: RelocationTarget,
}

/// Well-known runtime helper functions that JIT code calls via trampolines
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeHelper {
    AllocObject,
    AllocArray,
    AllocString,
    SafepointPoll,
    CheckPreemption,
    BoxI32,
    UnboxI32,
    BoxF64,
    UnboxF64,
    BoxBool,
    UnboxBool,
    NativeCallDispatch,
    InterpreterCall,
    ThrowException,
    Deoptimize,
    SpawnTask,
    AwaitTask,
    StringConcat,
    GenericEquals,
}

/// Resolver for symbol addresses at finalization time
pub trait SymbolResolver {
    fn resolve_runtime_helper(&self, helper: RuntimeHelper) -> Option<usize>;
    fn resolve_jit_function(&self, func_index: u32) -> Option<usize>;
}

/// Where a GC pointer lives at a safepoint
#[derive(Debug, Clone)]
pub enum PointerLocation {
    /// In a machine register (register number)
    Register(u8),
    /// At a stack offset from frame pointer
    StackOffset(i32),
}

/// Describes GC-visible pointers at a specific code location (for stack scanning)
#[derive(Debug, Clone)]
pub struct StackMapEntry {
    /// Offset in the generated code
    pub code_offset: usize,
    /// Locations of live GC pointers at this point
    pub live_pointers: Vec<PointerLocation>,
}

/// Information needed to resume the interpreter after deoptimization
#[derive(Debug, Clone)]
pub struct DeoptInfo {
    /// Code offset where deopt can occur
    pub code_offset: usize,
    /// Bytecode offset to resume at
    pub bytecode_offset: usize,
    /// Map from machine locations to local variable indices
    pub register_map: Vec<(PointerLocation, u16)>,
}

/// Context information about the module being compiled
pub struct ModuleContext<'a> {
    /// The bytecode module
    pub module: &'a crate::compiler::bytecode::Module,
    /// Function index being compiled
    pub func_index: u32,
}

/// Compiled machine code (not yet executable — needs relocation patching)
#[derive(Debug)]
pub struct CompiledCode {
    /// Raw machine code bytes
    pub code: Vec<u8>,
    /// Offset of the function entry point within code
    pub entry_offset: usize,
    /// GC stack maps for safepoints
    pub stack_maps: Vec<StackMapEntry>,
    /// Deoptimization state at potential deopt points
    pub deopt_info: Vec<DeoptInfo>,
    /// Relocations to patch
    pub relocations: Vec<Relocation>,
}

/// Executable native code (after relocation and memory mapping)
pub struct ExecutableCode {
    /// Pointer to the executable code
    pub code_ptr: *const u8,
    /// Size of the code region
    pub code_size: usize,
    /// Offset of the entry point
    pub entry_offset: usize,
    /// GC stack maps
    pub stack_maps: Vec<StackMapEntry>,
    /// Deoptimization info
    pub deopt_info: Vec<DeoptInfo>,
}

// Safety: ExecutableCode is Send+Sync because the code_ptr points to
// immutable executable memory that won't change after finalization.
unsafe impl Send for ExecutableCode {}
unsafe impl Sync for ExecutableCode {}

/// The backend-agnostic code generation trait
///
/// Backends implement this to generate native code from JIT IR.
/// The compilation pipeline is:
///   JitFunction → compile_function() → CompiledCode → finalize() → ExecutableCode
pub trait CodegenBackend: Send + Sync {
    /// Backend name (for diagnostics)
    fn name(&self) -> &str;

    /// Compile a JIT IR function to machine code
    fn compile_function(
        &self,
        func: &JitFunction,
        ctx: &ModuleContext,
    ) -> Result<CompiledCode, CodegenError>;

    /// Apply relocations and produce executable code
    fn finalize(
        &self,
        code: &mut CompiledCode,
        resolver: &dyn SymbolResolver,
    ) -> Result<ExecutableCode, CodegenError>;

    /// Return target architecture information
    fn target_info(&self) -> TargetInfo;
}
