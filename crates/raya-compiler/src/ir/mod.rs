//! Intermediate Representation (IR) for Raya
//!
//! The IR serves as a simplified representation between the type-checked AST and
//! bytecode generation. It uses Three-Address Code (TAC) with Basic Blocks.
//!
//! # Structure
//!
//! - `IrModule` - Top-level container for a compiled module
//! - `IrFunction` - A function with parameters, locals, and basic blocks
//! - `BasicBlock` - A sequence of instructions with a single entry and exit
//! - `IrInstr` - Three-address code instructions
//! - `Register` - Virtual registers with type information

pub mod block;
pub mod function;
pub mod instr;
pub mod module;
pub mod pretty;
pub mod value;

pub use block::{BasicBlock, BasicBlockId, Terminator};
pub use function::IrFunction;
pub use instr::{BinaryOp, ClassId, FunctionId, IrInstr, StringCompareMode, UnaryOp};
pub use module::{IrClass, IrField, IrModule};
pub use pretty::PrettyPrint;
pub use value::{IrConstant, IrValue, Register, RegisterId, ValueOrigin};
