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
    flags, module_id_from_name, symbol_id_from_name, ClassDebugInfo, ClassDef, ClassReflectionData,
    DebugInfo, Export, FieldReflectionData, Function, FunctionDebugInfo, GenericTemplateInfo,
    Import, JitHint, LineEntry, Metadata, Method, MethodKind, Module, ModuleError, ModuleId,
    MonoDebugEntry, NominalTypeExport, ReflectionData, StaticMethod, StructuralLayoutInfo,
    StructuralShapeInfo, SymbolId, SymbolScope, SymbolType, TemplateSymbolEntry,
    TypeSignatureHash, VERSION,
};
pub use opcode::Opcode;
pub use verify::{verify_module, VerifyError};
