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

pub mod frame;
pub mod analysis;
pub mod statemachine;
pub mod traits;
pub mod ir_adapter;
pub mod lowering;
pub mod abi;
pub mod codegen;
pub mod bytecode_adapter;
pub mod linker;
pub mod helpers;
pub mod executor;

pub use frame::{AotFrame, AotTaskContext, AotHelperTable, AotEntryFn, SuspendReason, AOT_SUSPEND};
pub use traits::{AotCompilable, AotError, compile_to_state_machine};
pub use codegen::{AotBundle, AotModuleInput, FuncTableEntry, GlobalFuncId, CompilableFunction, compile_functions, create_native_isa};
pub use ir_adapter::IrFunctionAdapter;
pub use linker::AotLinker;
pub use executor::{run_aot_function, AotRunResult};
