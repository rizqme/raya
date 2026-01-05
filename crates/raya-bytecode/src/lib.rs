//! Raya VM Bytecode Definitions
//!
//! This crate provides the core bytecode instruction set, module format,
//! and constant pool structures for the Raya virtual machine.

#![warn(missing_docs)]
#![warn(rust_2018_idioms)]

pub mod constants;
pub mod encoder;
pub mod module;
pub mod opcode;
pub mod verify;

pub use constants::ConstantPool;
pub use encoder::{BytecodeReader, BytecodeWriter, DecodeError};
pub use module::{ClassDef, Function, Metadata, Method, Module, ModuleError};
pub use opcode::Opcode;
pub use verify::{verify_module, VerifyError};
