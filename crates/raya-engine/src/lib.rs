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
// Test-only: allow approximate constants (PI/E in tests) and identity ops (clarity)
#![cfg_attr(test, allow(clippy::approx_constant, clippy::identity_op))]
#![cfg_attr(test, allow(unused_variables, unused_imports))]
// ParseError is large by design (carries source spans + messages); boxing would add
// indirection on every error path. Same applies to large AST enum variants.
#![allow(clippy::result_large_err)]
#![allow(clippy::large_enum_variant)]
// Intentional: raw pointer operations in GC/unsafe code
#![allow(clippy::not_unsafe_ptr_arg_deref)]
// Intentional: module structure mirrors the language concepts
#![allow(clippy::module_inception)]

// ============================================================================
// Core Modules
// ============================================================================

/// Parser module: Lexer, parser, types, and type checker
pub mod parser;

/// Shared language semantic profiles and semantic-HIR inspection.
pub mod semantics;

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

/// Offline AOT profiling types, available as real or no-op shims depending on feature flags.
pub mod aot_profile;

/// Profiler module: sampling-based CPU/wall-clock profiling
pub mod profiler;

/// Linter module: AST-based lint analysis
pub mod linter;

/// Builtins module: Precompiled builtin types and signatures (re-exported from vm::builtins)
pub use vm::builtins;

// ============================================================================
// Re-exports from Parser
// ============================================================================

pub use parser::{
    // AST
    ast,
    CheckError,
    // Interner
    Interner,
    LexError,
    // Lexer
    Lexer,
    ParseError,
    // Parser
    Parser,
    Span,
    Symbol,
    SymbolTable,
    TemplatePart,
    Token,
    // Types
    Type,
    // Checker
    TypeChecker,
    TypeContext,
    TypeId,
};

pub use semantics::{
    build_semantic_hir, build_semantic_lowering_plan, BindingKind, BindingOpKind, CallOpKind,
    CallableKind, ConcurrencySemantics, DestructuringPlan, EnvHandle, EnvRecordKind,
    FunctionSemantics, LoopScopePlan, LoweringSemantics, OptimizationProfile,
    ReferenceExprKind, ResolvedIdentifierKind, RuntimeSemanticsBase, SemanticBinding,
    SemanticBindingOp, SemanticCallOp, SemanticCallable, SemanticHirModule,
    SemanticLoweringPlan, SemanticProfile, SemanticReferenceExpr, SemanticResolvedIdentifier,
    SemanticTopLevelCallable, SemanticUpdateOp, SourceKind,
    SuspensionKind, SuspensionPoint, TypingDiscipline, UpdateOpKind,
};

// ============================================================================
// Re-exports from Compiler
// ============================================================================

pub use compiler::{
    // Disassembler
    disassemble_function,
    // IR
    ir,
    // Native IDs
    native_id,
    verify_module,
    // Bytecode
    BytecodeReader,
    BytecodeWriter,
    ClassDef,
    // Code generation
    CodeGenerator,
    CompileError,
    CompileResult,
    // Compiler
    Compiler as BytecodeCompiler,
    ConstantPool,
    DecodeError,
    Export,
    Function,
    Import,
    IrCodeGenerator,
    Metadata,
    Method,
    Module,
    ModuleBuilder,
    ModuleError,
    Opcode,
    SymbolType,
    VerifyError,
};

// ============================================================================
// Re-exports from VM
// ============================================================================

pub use vm::{
    // Builtin native IDs (for VM dispatch)
    builtin as builtin_ids,
    // FFI - Native modules (types from raya-sdk)
    ffi::{
        native_to_value,
        pin_value,
        raya_error_free,
        raya_error_message,
        raya_module_free,
        raya_module_load_bytes,
        raya_module_load_file,
        raya_value_bool,
        raya_value_free,
        raya_value_i32,
        raya_value_null,
        raya_version,
        raya_vm_destroy,
        raya_vm_execute,
        raya_vm_new,
        register_native_module,
        unpin_value,
        // Value conversion
        value_to_native,
        FromRaya,
        // Dynamic library loading
        Library,
        LoadError,
        NativeError,
        NativeFn,
        NativeModule,
        NativeValue,
        RayaError,
        RayaModule as CRayaModule,
        // C API
        RayaVM,
        RayaValue as CRayaValue,
        ToRaya,
    },
    // GC
    gc,
    // JSON
    validate_cast,
    Array,
    // Stack
    CallFrame,
    Class,
    // Class registry
    ClassRegistry,
    ContextRegistry,
    JsonValue,
    // Synchronization
    Mutex,
    MutexError,
    MutexId,
    MutexRegistry,
    Object,
    // Types
    PointerMap,
    RayaString,
    ResourceCounters,
    ResourceLimits,
    // Scheduler
    Scheduler,
    // Snapshots
    SnapshotReader,
    SnapshotWriter,
    Stack,
    StackStats,
    TypeInfo,
    TypeKind,
    TypeRegistry,
    TypeSchema,
    TypeSchemaRegistry,
    VTable,
    // Value and Object model
    Value,
    // VM and execution
    Vm,
    VmContext,
    VmContextId,
    VmError,
    VmOptions,
    VmResult,
};

// ============================================================================
// Re-exports from Builtins
// ============================================================================

pub use builtins::{
    builtin_count, builtin_names, get_all_builtins, get_all_signatures, get_builtin,
    get_builtin_bytecode, get_signatures, BuiltinModule, BuiltinSignatures, ClassSig, FunctionSig,
    MethodSig, PropertySig,
};
