//! Raya VM Bytecode Definitions
//!
//! This module provides the core bytecode instruction set, module format,
//! and constant pool structures for the Raya virtual machine.

pub mod constants;
pub mod encoder;
pub mod module;
pub mod opcode;
pub mod verify;

pub use constants::ConstantPool;
pub use encoder::{BytecodeReader, BytecodeWriter, DecodeError};
pub use module::{
    flags, ClassDef, ClassReflectionData, Export, FieldReflectionData, Function, Import, Metadata,
    Method, Module, ModuleError, ReflectionData, SymbolType,
};
pub use opcode::Opcode;
pub use verify::{verify_module, VerifyError};
