//! JIT IR — SSA-form intermediate representation for native code generation

pub mod types;
pub mod instr;
pub mod builder;
pub mod display;

pub use types::JitType;
pub use instr::{Reg, JitBlockId, JitInstr, JitTerminator, JitBlock, JitFunction};
pub use instr::{DeoptReason, DeoptState, LocalSlot};
