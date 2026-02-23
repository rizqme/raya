//! JIT IR — SSA-form intermediate representation for native code generation

pub mod builder;
pub mod display;
pub mod instr;
pub mod types;

pub use instr::{DeoptReason, DeoptState, LocalSlot};
pub use instr::{JitBlock, JitBlockId, JitFunction, JitInstr, JitTerminator, Reg};
pub use types::JitType;
