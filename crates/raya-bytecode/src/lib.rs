//! Raya VM Bytecode Definitions
//!
//! This crate provides the core bytecode instruction set, module format,
//! and constant pool structures for the Raya virtual machine.

#![warn(missing_docs)]
#![warn(rust_2018_idioms)]

pub mod opcode;
pub mod module;
pub mod constants;
pub mod verify;
pub mod encoder;

pub use opcode::Opcode;
pub use module::{Module, ModuleError, Function, ClassDef, Method, Metadata};
pub use constants::ConstantPool;
pub use encoder::{BytecodeReader, BytecodeWriter, DecodeError};
pub use verify::{verify_module, VerifyError};
