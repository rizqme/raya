//! AOT (Ahead-of-Time) native compilation for Raya
//!
//! Compiles Raya source or bytecode directly to native machine code via Cranelift.
//! The compiled code runs on the existing scheduler with full concurrency support.
//!
//! Two compilation paths:
//! - **Path A** (source): IR → state machine → Cranelift (full optimization)
//! - **Path B** (bytecode): .ryb → JIT lifter → state machine → Cranelift (for dependencies)
//!
//! Output is appended to the raya binary as a payload and loaded into executable
//! memory at startup via mmap.

pub mod abi;
pub mod analysis;
pub mod bytecode_adapter;
pub mod codegen;
pub mod executor;
pub mod frame;
pub mod helpers;
pub mod ir_adapter;
pub mod linker;
pub mod lowering;
pub mod profile;
pub mod statemachine;
pub mod traits;

pub use codegen::{
    compile_functions, compile_functions_with_profile, create_native_isa, AotBundle,
    AotModuleInput, CompilableFunction, FuncTableEntry, GlobalFuncId,
};
pub use executor::{allocate_initial_frame, build_task_context, run_aot_function, AotRunResult};
pub use frame::{AotEntryFn, AotFrame, AotHelperTable, AotTaskContext, SuspendReason, AOT_SUSPEND};
pub use helpers::{
    clear_registered_aot_functions, dispatch_registered_aot_entry,
    install_registered_aot_functions, InstalledAotFunctionRegistry, RegisteredAotClone,
    RegisteredAotFunctionEntry,
};
pub use ir_adapter::IrFunctionAdapter;
pub use linker::AotLinker;
pub use profile::{AotProfileCollector, AotProfileData, AotSiteKind};
pub use traits::{compile_to_state_machine, AotCompilable, AotError};
