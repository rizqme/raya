//! Raya Language Engine
//!
//! This crate provides the complete Raya language implementation:
//! - **Parser**: Lexer, parser, and type checker (`parser` module)
//! - **Compiler**: IR, optimizations, and bytecode generation (`compiler` module)
//! - **VM**: Interpreter, scheduler, GC, and runtime (`vm` module)
//! - **Builtins**: Precompiled builtin types and signatures (`builtins` module)
//!
//! # Example
//!
//! ```rust,ignore
//! use raya_engine::{Parser, Compiler, Vm};
//!
//! let source = r#"
//!     function main(): number {
//!         return 42;
//!     }
//! "#;
//!
//! // Parse
//! let parser = Parser::new(source).unwrap();
//! let (module, interner) = parser.parse().unwrap();
//!
//! // Compile
//! let compiler = Compiler::new(TypeContext::new(), &interner);
//! let bytecode = compiler.compile_via_ir(&module).unwrap();
//!
//! // Execute
//! let mut vm = Vm::new();
//! let result = vm.execute(&bytecode);
//! ```

#![warn(missing_docs)]
#![warn(rust_2018_idioms)]
// TODO: Fix these clippy warnings properly
#![cfg_attr(test, allow(clippy::approx_constant))]
#![cfg_attr(test, allow(clippy::identity_op))]
#![cfg_attr(test, allow(clippy::unnecessary_cast))]
#![cfg_attr(test, allow(clippy::single_char_add_str))]
#![cfg_attr(test, allow(unused_variables))]
#![cfg_attr(test, allow(unused_imports))]
#![allow(clippy::approx_constant)]
#![allow(clippy::identity_op)]
#![allow(clippy::derivable_impls)]
#![allow(clippy::arc_with_non_send_sync)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::unnecessary_lazy_evaluations)]
#![allow(clippy::needless_return)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(ambiguous_wide_pointer_comparisons)]

// ============================================================================
// Core Modules
// ============================================================================

/// Parser module: Lexer, parser, types, and type checker
pub mod parser;

/// Compiler module: IR, optimizations, and bytecode generation
pub mod compiler;

/// VM module: Interpreter, scheduler, GC, and runtime
pub mod vm;

/// JIT compilation module (optional, behind "jit" feature)
#[cfg(feature = "jit")]
pub mod jit;

/// AOT (Ahead-of-Time) compilation module (optional, behind "aot" feature)
#[cfg(feature = "aot")]
pub mod aot;

/// Builtins module: Precompiled builtin types and signatures (re-exported from vm::builtins)
pub use vm::builtins;

// ============================================================================
// Re-exports from Parser
// ============================================================================

pub use parser::{
    // Lexer
    Lexer, LexError, Token, Span, TemplatePart,
    // Parser
    Parser, ParseError,
    // Interner
    Interner, Symbol,
    // Types
    Type, TypeId, TypeContext,
    // Checker
    TypeChecker, CheckError, SymbolTable,
    // AST
    ast,
};

// ============================================================================
// Re-exports from Compiler
// ============================================================================

pub use compiler::{
    // Bytecode
    BytecodeReader, BytecodeWriter, ClassDef, ConstantPool, DecodeError, Export, Function,
    Import, Metadata, Method, Module, ModuleError, Opcode, SymbolType, VerifyError, verify_module,
    // IR
    ir,
    // Code generation
    CodeGenerator, IrCodeGenerator, ModuleBuilder,
    // Compiler
    Compiler as BytecodeCompiler, CompileError, CompileResult,
    // Disassembler
    disassemble_function,
    // Native IDs
    native_id,
};

// ============================================================================
// Re-exports from VM
// ============================================================================

pub use vm::{
    // Value and Object model
    Value, Object, Array, Class, RayaString, VTable,
    // VM and execution
    Vm, VmContext, VmContextId, VmOptions, VmError, VmResult,
    // Scheduler
    Scheduler,
    // Stack
    CallFrame, Stack, StackStats,
    // GC
    gc,
    // Synchronization
    Mutex, MutexError, MutexId, MutexRegistry,
    // Snapshots
    SnapshotReader, SnapshotWriter,
    // Types
    PointerMap, TypeInfo, TypeRegistry,
    // JSON
    validate_cast, JsonValue, TypeKind, TypeSchema, TypeSchemaRegistry,
    // Class registry
    ClassRegistry, ContextRegistry, ResourceCounters, ResourceLimits,
    // Builtin native IDs (for VM dispatch)
    builtin as builtin_ids,
    // FFI - Native modules (types from raya-sdk)
    ffi::{
        FromRaya, NativeError, NativeFn, NativeModule, NativeValue, ToRaya,
        pin_value, unpin_value, register_native_module,
        // Value conversion
        value_to_native, native_to_value,
        // Dynamic library loading
        Library, LoadError,
        // C API
        RayaVM, RayaValue as CRayaValue, RayaModule as CRayaModule, RayaError,
        raya_vm_new, raya_vm_destroy, raya_vm_execute,
        raya_module_load_file, raya_module_load_bytes, raya_module_free,
        raya_value_null, raya_value_bool, raya_value_i32, raya_value_free,
        raya_error_message, raya_error_free, raya_version,
    },
};

// ============================================================================
// Re-exports from Builtins
// ============================================================================

pub use builtins::{
    BuiltinModule, BuiltinSignatures, ClassSig, MethodSig, PropertySig, FunctionSig,
    get_all_builtins, get_builtin, get_builtin_bytecode, builtin_names, builtin_count,
    get_all_signatures, get_signatures,
};
