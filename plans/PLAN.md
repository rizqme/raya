# Raya Implementation Plan

This document outlines the complete implementation roadmap for the Raya programming language and virtual machine, written in Rust.

---

## Table of Contents

1. [Overview](#overview)
2. [Phase 1: VM Core](#phase-1-vm-core)
3. [Phase 2: Parser & Type Checker](#phase-2-parser--type-checker)
4. [Phase 3: Compiler & Code Generation](#phase-3-compiler--code-generation)
5. [Phase 4: Standard Library](#phase-4-standard-library)
6. [Phase 5: Package Manager](#phase-5-package-manager)
7. [Phase 6: Testing System](#phase-6-testing-system)
8. [Phase 7: Tooling & Developer Experience](#phase-7-tooling--developer-experience)
9. [Milestones](#milestones)

---

## Overview

**Technology Stack:**
- **Language**: Rust (stable)
- **Target**: Native binary with embedded VM
- **Architecture**: Interpreter-based VM with future JIT support

**Project Structure:**
```
rayavm/
├── crates/
│   ├── raya-core/        # VM runtime
│   ├── raya-bytecode/    # Bytecode definitions
│   ├── raya-parser/      # Lexer & Parser
│   ├── raya-types/       # Type system
│   ├── raya-compiler/    # Code generation
│   ├── raya-stdlib/      # Standard library
│   ├── raya-cli/         # CLI tool (rayac)
│   └── raya-pm/             # Package manager
├── stdlib/                 # Raya standard library source
├── tests/                  # Integration tests
├── examples/               # Example programs
├── design/                 # Specification docs
└── plans/                  # Implementation plans
```

**Dependencies:**
- `clap` - CLI argument parsing
- `serde` / `serde_json` - Serialization
- `logos` - Lexer generation
- `lalrpop` - Parser generation (alternative)
- `crossbeam` - Work-stealing scheduler
- `parking_lot` - Efficient synchronization
- `rustc-hash` - Fast hashing
- `mimalloc` - Fast allocator

---

## Phase 1: VM Core

**Goal:** Build a functional bytecode interpreter with garbage collection and task scheduling.

### 1.1 Project Setup

**Tasks:**
- [x] Initialize Rust workspace
- [ ] Set up crate structure
- [ ] Configure CI/CD (GitHub Actions)
- [ ] Set up benchmarking infrastructure

**Files:**
```
Cargo.toml (workspace)
crates/raya-bytecode/Cargo.toml
crates/raya-core/Cargo.toml
```

### 1.2 Bytecode Definitions

**Crate:** `raya-bytecode`

**Tasks:**
- [ ] Define `Opcode` enum (all opcodes from OPCODE.md)
- [ ] Implement bytecode encoding/decoding
- [ ] Create bytecode module format
- [ ] Add constant pool structure
- [ ] Implement bytecode verification

**Files:**
```rust
// crates/raya-bytecode/src/lib.rs
pub mod opcode;
pub mod module;
pub mod constants;
pub mod verify;

// crates/raya-bytecode/src/opcode.rs
#[repr(u8)]
pub enum Opcode {
    Nop = 0x00,
    ConstI32 = 0x01,
    ConstF64 = 0x02,
    // ... all opcodes
}

// crates/raya-bytecode/src/module.rs
pub struct Module {
    pub magic: [u8; 4],      // "RAYA"
    pub version: u32,
    pub constants: ConstantPool,
    pub functions: Vec<Function>,
    pub classes: Vec<ClassDef>,
    pub metadata: Metadata,
}
```

**Reference:** `design/OPCODE.md`

### 1.3 Memory Management & GC

**Crate:** `raya-core`

**Tasks:**
- [ ] Implement value representation (tagged pointers)
- [ ] Build heap allocator
- [ ] Implement mark-sweep garbage collector
- [ ] Add GC root tracking
- [ ] Optimize for generational GC (later)

**Files:**
```rust
// crates/raya-core/src/value.rs
#[repr(C)]
pub enum Value {
    Number(f64),
    Integer(i32),
    Boolean(bool),
    Null,
    Object(GcPtr<Object>),
    String(GcPtr<String>),
}

// crates/raya-core/src/gc.rs
pub struct Gc {
    heap: Heap,
    roots: Vec<GcPtr<Value>>,
    allocated: usize,
    threshold: usize,
}

impl Gc {
    pub fn collect(&mut self);
    pub fn mark(&self, ptr: GcPtr<Value>);
    pub fn sweep(&mut self);
}
```

**Reference:** `design/ARCHITECTURE.md` Section 5

### 1.4 Stack & Frame Management

**Tasks:**
- [ ] Implement operand stack
- [ ] Create call frame structure
- [ ] Add stack overflow protection
- [ ] Implement function call mechanism

**Files:**
```rust
// crates/raya-core/src/stack.rs
pub struct Stack {
    slots: Vec<Value>,
    frames: Vec<CallFrame>,
    sp: usize,  // Stack pointer
    fp: usize,  // Frame pointer
}

pub struct CallFrame {
    function: FunctionRef,
    ip: usize,          // Instruction pointer
    base_pointer: usize,
    local_count: usize,
}
```

**Reference:** `design/ARCHITECTURE.md` Section 3

### 1.5 Bytecode Interpreter

**Tasks:**
- [ ] Build instruction dispatch loop
- [ ] Implement all arithmetic opcodes (IADD, FADD, NADD, etc.)
- [ ] Implement control flow (JMP, JMP_IF_TRUE, etc.)
- [ ] Implement function calls (CALL, RETURN)
- [ ] Add error handling (THROW, TRAP)
- [ ] Optimize dispatch (computed goto, threaded code)

**Files:**
```rust
// crates/raya-core/src/vm.rs
pub struct Vm {
    stack: Stack,
    gc: Gc,
    scheduler: Scheduler,
    globals: HashMap<String, Value>,
}

impl Vm {
    pub fn execute(&mut self, module: &Module) -> Result<Value, VmError>;

    fn dispatch(&mut self, opcode: Opcode) -> Result<(), VmError> {
        match opcode {
            Opcode::ConstI32 => self.op_const_i32(),
            Opcode::Iadd => self.op_iadd(),
            Opcode::Call => self.op_call(),
            // ... all opcodes
        }
    }
}
```

**Reference:** `design/OPCODE.md` Sections 3, 7

### 1.6 Object Model

**Tasks:**
- [ ] Implement object allocation
- [ ] Add field access (LOAD_FIELD, STORE_FIELD)
- [ ] Build vtable system for method dispatch
- [ ] Implement class metadata
- [ ] Add array operations

**Files:**
```rust
// crates/raya-core/src/object.rs
pub struct Object {
    class: ClassRef,
    fields: Vec<Value>,
}

pub struct Class {
    name: String,
    field_count: usize,
    methods: Vec<Method>,
    vtable: VTable,
}

pub struct VTable {
    entries: Vec<FunctionRef>,
}
```

**Reference:** `design/LANG.md` Section 8, `design/ARCHITECTURE.md` Section 2

### 1.7 Task Scheduler

**Tasks:**
- [ ] Implement Task structure
- [ ] Build work-stealing deque
- [ ] Create worker thread pool
- [ ] Add task spawning (SPAWN opcode)
- [ ] Implement await mechanism (AWAIT opcode)
- [ ] Add task completion tracking

**Files:**
```rust
// crates/raya-core/src/scheduler.rs
use crossbeam::deque::{Worker, Stealer};
use parking_lot::Mutex;

pub struct Scheduler {
    workers: Vec<WorkerThread>,
    global_queue: Arc<Mutex<VecDeque<TaskId>>>,
    tasks: HashMap<TaskId, Task>,
}

pub struct WorkerThread {
    id: WorkerId,
    local_queue: Worker<TaskId>,
    stealers: Vec<Stealer<TaskId>>,
}

pub struct Task {
    id: TaskId,
    state: TaskState,
    stack: Stack,
    result: Option<Value>,
    waiters: Vec<TaskId>,
}

pub enum TaskState {
    Ready,
    Running,
    Suspended,
    Completed,
}
```

**Reference:** `design/ARCHITECTURE.md` Section 4

### 1.8 Synchronization

**Tasks:**
- [ ] Implement Mutex type
- [ ] Add MUTEX_LOCK / MUTEX_UNLOCK opcodes
- [ ] Ensure no await in critical sections (compile-time check)
- [ ] Add deadlock detection (debug mode)

**Files:**
```rust
// crates/raya-core/src/sync.rs
pub struct RayaMutex {
    inner: parking_lot::Mutex<()>,
    owner: Option<TaskId>,
}

impl RayaMutex {
    pub fn lock(&mut self, task: TaskId) -> Result<(), VmError>;
    pub fn unlock(&mut self, task: TaskId) -> Result<(), VmError>;
}
```

**Reference:** `design/LANG.md` Section 15

### 1.9 Testing

**Tasks:**
- [ ] Write unit tests for each opcode
- [ ] Create integration tests for bytecode execution
- [ ] Add GC stress tests
- [ ] Test concurrent task execution
- [ ] Benchmark performance

**Files:**
```
crates/raya-core/tests/
├── opcodes.rs
├── gc.rs
├── tasks.rs
└── integration.rs
```

---

## Phase 2: Parser & Type Checker

**Goal:** Parse Raya source code and perform sound type checking.

### 2.1 Lexer

**Crate:** `raya-parser`

**Tasks:**
- [ ] Define token types
- [ ] Implement lexer using `logos` or hand-written
- [ ] Handle keywords, identifiers, literals
- [ ] Track source locations for error reporting
- [ ] Support string templates

**Files:**
```rust
// crates/raya-parser/src/lexer.rs
use logos::Logos;

#[derive(Logos, Debug, PartialEq)]
pub enum Token {
    #[token("function")]
    Function,

    #[token("let")]
    Let,

    #[token("const")]
    Const,

    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
    Identifier,

    #[regex(r"\d+")]
    IntLiteral,

    // ... all tokens
}

pub struct Lexer<'a> {
    source: &'a str,
    tokens: Vec<(Token, Span)>,
}
```

**Reference:** `design/LANG.md` Section 2

### 2.2 AST Definition

**Tasks:**
- [ ] Define AST node types
- [ ] Implement visitor pattern
- [ ] Add source span tracking
- [ ] Create pretty-printer for debugging

**Files:**
```rust
// crates/raya-parser/src/ast.rs
pub struct Module {
    pub statements: Vec<Statement>,
    pub span: Span,
}

pub enum Statement {
    FunctionDecl(FunctionDecl),
    ClassDecl(ClassDecl),
    LetDecl(LetDecl),
    Expression(Expression),
}

pub enum Expression {
    Literal(Literal),
    Identifier(String),
    BinaryOp { op: BinOp, left: Box<Expr>, right: Box<Expr> },
    Call { callee: Box<Expr>, args: Vec<Expr> },
    // ... all expression types
}

pub enum Type {
    Number,
    String,
    Boolean,
    Null,
    Union(Vec<Type>),
    Function(FunctionType),
    Class(ClassType),
    Interface(InterfaceType),
    // ...
}
```

**Reference:** `design/LANG.md` All sections

### 2.3 Parser

**Tasks:**
- [ ] Implement recursive descent parser
- [ ] Handle operator precedence
- [ ] Parse function declarations
- [ ] Parse class declarations
- [ ] Parse type annotations
- [ ] Provide helpful error messages

**Files:**
```rust
// crates/raya-parser/src/parser.rs
pub struct Parser<'a> {
    lexer: Lexer<'a>,
    current: usize,
    errors: Vec<ParseError>,
}

impl<'a> Parser<'a> {
    pub fn parse_module(&mut self) -> Result<Module, Vec<ParseError>>;

    fn parse_statement(&mut self) -> Result<Statement, ParseError>;
    fn parse_expression(&mut self) -> Result<Expression, ParseError>;
    fn parse_type(&mut self) -> Result<Type, ParseError>;

    // Precedence climbing for binary operators
    fn parse_binary_expr(&mut self, min_prec: u8) -> Result<Expression, ParseError>;
}
```

**Reference:** `design/LANG.md` Sections 6, 7, 8

### 2.4 Type System

**Crate:** `raya-types`

**Tasks:**
- [ ] Implement type representation
- [ ] Build type inference engine
- [ ] Add subtyping rules
- [ ] Implement discriminated union checking
- [ ] Track type parameters for generics

**Files:**
```rust
// crates/raya-types/src/lib.rs
pub mod types;
pub mod inference;
pub mod unify;
pub mod subtyping;

// crates/raya-types/src/types.rs
pub enum Type {
    Primitive(PrimitiveType),
    Union(UnionType),
    Function(FunctionType),
    Class(ClassType),
    Interface(InterfaceType),
    TypeVar(TypeVar),  // For inference
    Generic(GenericType),
}

pub struct UnionType {
    pub variants: Vec<Type>,
    pub discriminant: Option<DiscriminantInfo>,
}

pub struct DiscriminantInfo {
    pub field: String,
    pub values: HashMap<String, Type>,
}
```

**Reference:** `design/LANG.md` Section 4

### 2.5 Type Checker

**Tasks:**
- [ ] Build symbol table
- [ ] Implement type checking for expressions
- [ ] Check function signatures
- [ ] Validate class definitions
- [ ] Enforce discriminated unions
- [ ] Check exhaustiveness
- [ ] Ban `typeof`, `instanceof`, `any`

**Files:**
```rust
// crates/raya-types/src/checker.rs
pub struct TypeChecker {
    symbols: SymbolTable,
    errors: Vec<TypeError>,
    current_scope: ScopeId,
}

impl TypeChecker {
    pub fn check_module(&mut self, module: &Module) -> Result<TypedModule, Vec<TypeError>>;

    fn check_statement(&mut self, stmt: &Statement) -> Result<TypedStatement, TypeError>;
    fn check_expression(&mut self, expr: &Expression) -> Result<(TypedExpression, Type), TypeError>;

    fn check_discriminated_union(&self, union: &UnionType) -> Result<(), TypeError>;
    fn check_exhaustiveness(&self, union: &UnionType, cases: &[String]) -> Result<(), TypeError>;
}

pub struct SymbolTable {
    scopes: Vec<Scope>,
    symbols: HashMap<String, Symbol>,
}

pub struct Symbol {
    name: String,
    ty: Type,
    kind: SymbolKind,
    span: Span,
}

pub enum SymbolKind {
    Variable,
    Function,
    Class,
    Interface,
    TypeAlias,
}
```

**Reference:** `design/LANG.md` Sections 4.7, 13A

### 2.6 Discriminant Inference

**Tasks:**
- [ ] Implement discriminant field detection
- [ ] Use priority order (kind > type > tag > variant > alphabetical)
- [ ] Validate all variants have common discriminant
- [ ] Generate compile errors for ambiguous unions

**Files:**
```rust
// crates/raya-types/src/discriminant.rs
pub struct DiscriminantInference;

impl DiscriminantInference {
    pub fn infer(union: &UnionType) -> Result<String, TypeError> {
        // Algorithm from LANG.md Section 17.6
        let common_fields = self.find_common_literal_fields(union);

        if common_fields.is_empty() {
            return Err(TypeError::NoDiscriminant);
        }

        // Priority: kind > type > tag > variant > alphabetical
        if common_fields.contains("kind") {
            return Ok("kind".to_string());
        }
        // ... etc
    }
}
```

**Reference:** `design/LANG.md` Section 17.6

### 2.7 Bare Union Transformation

**Tasks:**
- [ ] Detect bare primitive unions (`string | number`)
- [ ] Transform to `{ $type, $value }` representation
- [ ] Insert boxing/unboxing code automatically
- [ ] Prevent user access to `$type` and `$value`

**Files:**
```rust
// crates/raya-types/src/bare_union.rs
pub struct BareUnionTransform;

impl BareUnionTransform {
    pub fn transform(ty: &Type) -> Option<Type> {
        if let Type::Union(union) = ty {
            if self.is_bare_primitive_union(union) {
                return Some(self.create_boxed_union(union));
            }
        }
        None
    }

    fn is_bare_primitive_union(&self, union: &UnionType) -> bool {
        union.variants.iter().all(|v| matches!(v,
            Type::Primitive(PrimitiveType::String |
                           PrimitiveType::Number |
                           PrimitiveType::Boolean |
                           PrimitiveType::Null)
        ))
    }
}
```

**Reference:** `design/LANG.md` Section 4.3

### 2.8 Error Reporting

**Tasks:**
- [ ] Create helpful error messages
- [ ] Show source code context
- [ ] Suggest fixes (e.g., "use discriminated union instead of typeof")
- [ ] Support multiple error formats (human, JSON)

**Files:**
```rust
// crates/raya-parser/src/error.rs
pub struct ParseError {
    pub kind: ErrorKind,
    pub span: Span,
    pub message: String,
    pub suggestion: Option<String>,
}

impl ParseError {
    pub fn format(&self, source: &str) -> String {
        // Pretty-print with source context
    }
}
```

---

## Phase 3: Compiler & Code Generation

**Goal:** Translate typed AST to bytecode.

### 3.1 IR (Intermediate Representation)

**Crate:** `raya-compiler`

**Tasks:**
- [ ] Design IR structure (SSA form or three-address code)
- [ ] Lower AST to IR
- [ ] Implement basic optimizations (constant folding, DCE)
- [ ] Add type information to IR

**Files:**
```rust
// crates/raya-compiler/src/ir.rs
pub enum IrInstr {
    Assign { dest: Register, value: IrValue },
    BinaryOp { dest: Register, op: BinOp, left: Register, right: Register },
    Call { dest: Option<Register>, func: FunctionId, args: Vec<Register> },
    Jump { target: BasicBlockId },
    Branch { cond: Register, then_block: BasicBlockId, else_block: BasicBlockId },
    Return { value: Option<Register> },
}

pub struct BasicBlock {
    id: BasicBlockId,
    instructions: Vec<IrInstr>,
    terminator: Terminator,
}
```

### 3.2 Monomorphization

**Tasks:**
- [ ] Collect all generic instantiations
- [ ] Generate specialized versions of generic functions
- [ ] Generate specialized versions of generic classes
- [ ] Track monomorphized types

**Files:**
```rust
// crates/raya-compiler/src/monomorphize.rs
pub struct Monomorphizer {
    instantiations: HashMap<(FunctionId, Vec<Type>), FunctionId>,
}

impl Monomorphizer {
    pub fn monomorphize(&mut self, module: &TypedModule) -> MonomorphizedModule {
        // Replace all generic types with concrete types
        // Generate specialized functions/classes
    }
}
```

**Reference:** `design/LANG.md` Section 13.7

### 3.3 Code Generation

**Tasks:**
- [ ] Implement bytecode emitter
- [ ] Generate code for all expression types
- [ ] Handle control flow (if, while, switch)
- [ ] Emit function prologues/epilogues
- [ ] Generate vtables for classes
- [ ] Emit closures with captured variables

**Files:**
```rust
// crates/raya-compiler/src/codegen.rs
pub struct CodeGenerator {
    module: Module,
    current_function: Option<FunctionId>,
    bytecode: Vec<u8>,
    constant_pool: ConstantPool,
}

impl CodeGenerator {
    pub fn generate(&mut self, ir_module: &IrModule) -> Module;

    fn emit_opcode(&mut self, opcode: Opcode);
    fn emit_u32(&mut self, value: u32);
    fn add_constant(&mut self, constant: Constant) -> u32;

    fn generate_function(&mut self, func: &IrFunction);
    fn generate_expression(&mut self, expr: &IrExpr);
}
```

**Reference:** `design/MAPPING.md` All sections

### 3.4 Match Inlining

**Tasks:**
- [ ] Detect `match()` calls
- [ ] Inline match logic directly
- [ ] Generate switch-based bytecode for discriminants
- [ ] Optimize for exhaustiveness (no unreachable trap)

**Files:**
```rust
// crates/raya-compiler/src/match_inline.rs
pub struct MatchInliner;

impl MatchInliner {
    pub fn inline_match(&self, call: &CallExpr) -> Option<InlinedMatch> {
        // Check if this is a match() call
        // Generate inline bytecode for switch on discriminant
        // See MAPPING.md Section 15.5, 15.6
    }
}
```

**Reference:** `design/MAPPING.md` Sections 15.5, 15.6

### 3.5 JSON Codegen

**Tasks:**
- [ ] Detect `JSON.encode()` and `JSON.decode<T>()` calls
- [ ] Generate specialized encoder/decoder functions
- [ ] Handle bare unions in JSON
- [ ] Emit validation code for decoders

**Files:**
```rust
// crates/raya-compiler/src/json_codegen.rs
pub struct JsonCodegen;

impl JsonCodegen {
    pub fn generate_encoder(&self, ty: &Type) -> FunctionId;
    pub fn generate_decoder(&self, ty: &Type) -> FunctionId;
}
```

**Reference:** `design/LANG.md` Section 17.7

### 3.6 Module Compilation

**Tasks:**
- [ ] Resolve module dependencies
- [ ] Handle standard library modules (`raya:std`, `raya:json`)
- [ ] Support relative and absolute imports
- [ ] Detect circular dependencies (error)

**Files:**
```rust
// crates/raya-compiler/src/module_resolver.rs
pub struct ModuleResolver {
    resolved: HashMap<PathBuf, ModuleId>,
    stdlib: StdlibModules,
}

impl ModuleResolver {
    pub fn resolve(&mut self, import: &str, from: &Path) -> Result<ModuleId, ResolveError>;
}
```

**Reference:** `design/LANG.md` Section 16.8

### 3.7 Optimization

**Tasks:**
- [ ] Constant folding
- [ ] Dead code elimination
- [ ] Inline small functions
- [ ] Optimize typed arithmetic (IADD vs FADD vs NADD)
- [ ] Remove redundant type checks

**Files:**
```rust
// crates/raya-compiler/src/optimize.rs
pub struct Optimizer;

impl Optimizer {
    pub fn optimize(&self, ir: &mut IrModule) {
        self.constant_folding(ir);
        self.dead_code_elimination(ir);
        self.inline_functions(ir);
    }
}
```

### 3.8 Testing

**Tasks:**
- [ ] Write tests for each language construct
- [ ] Test monomorphization
- [ ] Test match inlining
- [ ] Test JSON codegen
- [ ] Compare output with expected bytecode

**Files:**
```
crates/raya-compiler/tests/
├── functions.rs
├── classes.rs
├── generics.rs
├── unions.rs
└── modules.rs
```

---

## Phase 4: Standard Library

**Goal:** Implement core runtime functionality.

### 4.1 Core Types

**Location:** `stdlib/core.raya`

**Tasks:**
- [ ] Implement `Error` class
- [ ] Define `Result<T, E>` type
- [ ] Define `Task<T>` interface
- [ ] Add `PromiseLike<T>` compatibility

**Files:**
```typescript
// stdlib/core.raya
export class Error {
  constructor(public message: string) {}
  stack?: string;
}

export type Result<T, E> =
  | { status: "ok"; value: T }
  | { status: "error"; error: E };

export interface Task<T> extends PromiseLike<T> {
  // No additional methods
}
```

**Reference:** `design/STDLIB.md` Section 1

### 4.2 raya:std Module

**Location:** `stdlib/std.raya`

**Tasks:**
- [ ] Implement `match()` function (compile-time magic)
- [ ] Implement `sleep()` (native)
- [ ] Implement `all()` for task aggregation
- [ ] Implement `race()` for task racing

**Files:**
```typescript
// stdlib/std.raya
export function match<T, R>(
  value: T,
  handlers: MatchHandlers<T, R>
): R {
  // Compiler intrinsic - replaced during compilation
  throw new Error("match() should be inlined by compiler");
}

// Native implementations
declare function sleep(ms: number): Task<void>;
declare function all<T>(tasks: Task<T>[]): Task<T[]>;
declare function race<T>(tasks: Task<T>[]): Task<T>;
```

**Native Implementation:**
```rust
// crates/raya-stdlib/src/std.rs
pub fn sleep(vm: &mut Vm, ms: f64) -> Result<TaskId, VmError> {
    let task = vm.scheduler.spawn_delayed(Duration::from_millis(ms as u64));
    Ok(task)
}

pub fn all(vm: &mut Vm, tasks: Vec<TaskId>) -> Result<TaskId, VmError> {
    let task = vm.scheduler.all(tasks);
    Ok(task)
}
```

**Reference:** `design/STDLIB.md` Section 2

### 4.3 raya:json Module

**Location:** `stdlib/json.raya`

**Tasks:**
- [ ] Define `JSON` class with `encode()` and `decode()`
- [ ] Both are compiler intrinsics
- [ ] Actual implementation generated per-type

**Files:**
```typescript
// stdlib/json.raya
export class JSON {
  static encode<T>(value: T): Result<string, Error> {
    // Compiler generates specialized encoder
    throw new Error("JSON.encode() should be replaced by compiler");
  }

  static decode<T>(input: string): Result<T, Error> {
    // Compiler generates specialized decoder
    throw new Error("JSON.decode() should be replaced by compiler");
  }
}
```

**Reference:** `design/STDLIB.md` Section 3

### 4.4 raya:json/internal Module

**Tasks:**
- [ ] Implement `JsonValue` type
- [ ] Implement `parseJson()` native function
- [ ] Build JSON parser in Rust

**Files:**
```typescript
// stdlib/json_internal.raya
export type JsonValue =
  | { kind: "null" }
  | { kind: "boolean"; value: boolean }
  | { kind: "number"; value: number }
  | { kind: "string"; value: string }
  | { kind: "array"; value: JsonValue[] }
  | { kind: "object"; value: Map<string, JsonValue> };

declare function parseJson(input: string): Result<JsonValue, Error>;
```

```rust
// crates/raya-stdlib/src/json.rs
pub fn parse_json(input: &str) -> Result<Value, VmError> {
    // Use serde_json or custom parser
    // Convert to Raya JsonValue representation
}
```

**Reference:** `design/STDLIB.md` Section 4

### 4.5 raya:reflect Module (Optional)

**Tasks:**
- [ ] Implement reflection API when `--emit-reflection` flag is set
- [ ] Add `REFLECT_*` opcodes
- [ ] Embed type metadata in bytecode
- [ ] Implement all reflection functions

**Files:**
```rust
// crates/raya-core/src/reflect.rs
#[cfg(feature = "reflection")]
pub mod reflect {
    pub fn type_of(vm: &Vm, value: Value) -> TypeInfo { /* ... */ }
    pub fn type_info<T>() -> TypeInfo { /* ... */ }
    pub fn get_property(obj: GcPtr<Object>, name: &str) -> Option<Value> { /* ... */ }
    // ... all reflection functions
}
```

**Reference:** `design/STDLIB.md` Section 5, `design/LANG.md` Section 18

### 4.6 Built-in Types

**Tasks:**
- [ ] Implement String methods (native)
- [ ] Implement Number methods (native)
- [ ] Implement Array methods (native)
- [ ] Implement Map class (native)
- [ ] Implement Set class (native)
- [ ] Implement Mutex class (native)

**Files:**
```rust
// crates/raya-stdlib/src/string.rs
pub fn string_to_upper_case(s: &str) -> String {
    s.to_uppercase()
}

pub fn string_substring(s: &str, start: usize, end: Option<usize>) -> String {
    // ...
}

// crates/raya-stdlib/src/array.rs
pub fn array_push(arr: &mut Vec<Value>, item: Value) {
    arr.push(item);
}

pub fn array_map(vm: &mut Vm, arr: &[Value], f: FunctionRef) -> Result<Vec<Value>, VmError> {
    // ...
}
```

**Reference:** `design/STDLIB.md` Section 7

### 4.7 Console API

**Tasks:**
- [ ] Implement `console.log()` (native)
- [ ] Implement `console.error()` (native)
- [ ] Implement `console.warn()` and `console.info()` (aliases)

**Files:**
```rust
// crates/raya-stdlib/src/console.rs
pub fn console_log(args: &[Value]) {
    for arg in args {
        print!("{} ", arg.to_string());
    }
    println!();
}

pub fn console_error(args: &[Value]) {
    for arg in args {
        eprint!("{} ", arg.to_string());
    }
    eprintln!();
}
```

**Reference:** `design/STDLIB.md` Section 6

### 4.8 Testing

**Tasks:**
- [ ] Test each stdlib function
- [ ] Test task utilities (sleep, all, race)
- [ ] Test JSON parsing and encoding
- [ ] Benchmark stdlib performance

---

## Phase 5: Package Manager

**Goal:** Create `raya-pm` for managing Raya packages.

### 5.1 Package Format

**Tasks:**
- [ ] Define `package.json` format (or `raya.toml`)
- [ ] Support semantic versioning
- [ ] Define dependency specification
- [ ] Add metadata (author, license, etc.)

**Files:**
```toml
# raya.toml
[package]
name = "my-package"
version = "1.0.0"
authors = ["Your Name <you@example.com>"]
license = "MIT"
description = "A sample Raya package"

[dependencies]
http = "2.1.0"
json = "1.0.0"

[dev-dependencies]
test-framework = "0.5.0"
```

### 5.2 Package Registry

**Tasks:**
- [ ] Design registry API
- [ ] Implement local package cache
- [ ] Support git dependencies
- [ ] Add lock file (`raya.lock`)

**Files:**
```rust
// crates/raya-pm/src/registry.rs
pub struct Registry {
    url: String,
    cache: PathBuf,
}

impl Registry {
    pub fn fetch(&self, package: &str, version: &str) -> Result<Package, RegistryError>;
    pub fn search(&self, query: &str) -> Result<Vec<PackageInfo>, RegistryError>;
}
```

### 5.3 CLI Commands

**Crate:** `raya-pm`

**Tasks:**
- [ ] `raya-pm init` - Initialize new project
- [ ] `raya-pm install` - Install dependencies
- [ ] `raya-pm add <package>` - Add dependency
- [ ] `raya-pm remove <package>` - Remove dependency
- [ ] `raya-pm publish` - Publish to registry
- [ ] `raya-pm search <query>` - Search packages

**Files:**
```rust
// crates/raya-pm/src/main.rs
use clap::{Parser, Subcommand};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Init,
    Install,
    Add { package: String },
    Remove { package: String },
    Publish,
    Search { query: String },
}
```

### 5.4 Dependency Resolution

**Tasks:**
- [ ] Implement SAT-based dependency resolver
- [ ] Handle version conflicts
- [ ] Generate lock file
- [ ] Support workspace projects

**Files:**
```rust
// crates/raya-pm/src/resolver.rs
pub struct DependencyResolver {
    packages: HashMap<String, Vec<Version>>,
}

impl DependencyResolver {
    pub fn resolve(&self, deps: &[Dependency]) -> Result<ResolvedDeps, ResolveError>;
}
```

### 5.5 Testing

**Tasks:**
- [ ] Test dependency resolution
- [ ] Test package installation
- [ ] Test lock file generation
- [ ] Integration tests with real packages

---

## Phase 6: Testing System

**Goal:** Build a test framework for Raya programs.

### 6.1 Test Framework Design

**Tasks:**
- [ ] Define test function syntax
- [ ] Support `describe` and `it` blocks
- [ ] Add assertions (`assert`, `assertEqual`, etc.)
- [ ] Support async tests

**Example:**
```typescript
// example.test.raya
import { describe, it, assert } from "raya:test";

describe("Math operations", () => {
  it("should add numbers correctly", () => {
    assert(1 + 1 === 2);
  });

  it("should handle async operations", async () => {
    const result = await fetchData();
    assert(result !== null);
  });
});
```

### 6.2 Test Runner

**Crate:** `raya-test`

**Tasks:**
- [ ] Discover test files
- [ ] Execute tests in parallel
- [ ] Report results (pass/fail/skip)
- [ ] Generate coverage reports
- [ ] Support watch mode

**Files:**
```rust
// crates/raya-test/src/runner.rs
pub struct TestRunner {
    tests: Vec<Test>,
    reporter: Box<dyn Reporter>,
}

impl TestRunner {
    pub fn run(&mut self) -> TestResults {
        // Execute all tests
        // Collect results
    }
}

pub struct TestResults {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub duration: Duration,
}
```

### 6.3 Assertions

**Location:** `stdlib/test.raya`

**Tasks:**
- [ ] Implement `assert()`
- [ ] Implement `assertEqual()`
- [ ] Implement `assertThrows()`
- [ ] Implement `assertAsync()`

**Files:**
```typescript
// stdlib/test.raya
export function assert(condition: boolean, message?: string): void {
  if (!condition) {
    throw new Error(message || "Assertion failed");
  }
}

export function assertEqual<T>(actual: T, expected: T, message?: string): void {
  if (actual !== expected) {
    throw new Error(message || `Expected ${expected}, got ${actual}`);
  }
}
```

### 6.4 Mocking & Stubbing

**Tasks:**
- [ ] Add basic mocking capabilities
- [ ] Support function spies
- [ ] Track function calls

**Files:**
```typescript
// stdlib/test.raya
export class Mock<T> {
  calls: any[][] = [];

  create(fn: T): T {
    // Return wrapped function that tracks calls
  }
}
```

### 6.5 Coverage

**Tasks:**
- [ ] Instrument bytecode for coverage
- [ ] Track line execution
- [ ] Generate coverage reports (HTML, JSON)

**Files:**
```rust
// crates/raya-test/src/coverage.rs
pub struct CoverageTracker {
    lines: HashMap<FileId, HashSet<usize>>,
}

impl CoverageTracker {
    pub fn record_line(&mut self, file: FileId, line: usize);
    pub fn generate_report(&self) -> CoverageReport;
}
```

### 6.6 Testing

**Tasks:**
- [ ] Test the test framework itself
- [ ] Write example tests
- [ ] Benchmark test execution performance

---

## Phase 7: Tooling & Developer Experience

**Goal:** Build developer tools for productivity.

### 7.1 CLI Tool (rayac)

**Crate:** `raya-cli`

**Tasks:**
- [ ] `rayac compile <file>` - Compile to bytecode
- [ ] `rayac run <file>` - Compile and execute
- [ ] `rayac check <file>` - Type check only
- [ ] `rayac build` - Build project
- [ ] `rayac test` - Run tests
- [ ] `rayac fmt` - Format code
- [ ] `rayac --version` - Show version

**Files:**
```rust
// crates/raya-cli/src/main.rs
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rayac")]
#[command(about = "Raya compiler and toolchain")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Compile { file: PathBuf },
    Run { file: PathBuf, args: Vec<String> },
    Check { file: PathBuf },
    Build,
    Test,
    Fmt { files: Vec<PathBuf> },
}
```

### 7.2 REPL

**Tasks:**
- [ ] Build interactive REPL
- [ ] Support multi-line input
- [ ] Add tab completion
- [ ] Show type information
- [ ] History and editing support

**Files:**
```rust
// crates/raya-cli/src/repl.rs
use rustyline::Editor;

pub struct Repl {
    vm: Vm,
    editor: Editor<()>,
}

impl Repl {
    pub fn run(&mut self) {
        loop {
            let line = self.editor.readline("raya> ");
            // Parse, type check, compile, execute
        }
    }
}
```

### 7.3 Code Formatter

**Crate:** `raya-fmt`

**Tasks:**
- [ ] Implement AST-based formatter
- [ ] Support configuration file
- [ ] Match common style guides (Prettier-like)

**Files:**
```rust
// crates/raya-fmt/src/lib.rs
pub struct Formatter {
    config: FormatConfig,
}

impl Formatter {
    pub fn format(&self, ast: &Module) -> String {
        // Pretty-print AST
    }
}
```

### 7.4 Language Server (LSP)

**Crate:** `raya-lsp`

**Tasks:**
- [ ] Implement LSP protocol
- [ ] Add diagnostics (errors, warnings)
- [ ] Add auto-completion
- [ ] Add go-to-definition
- [ ] Add hover information
- [ ] Add rename refactoring

**Files:**
```rust
// crates/raya-lsp/src/main.rs
use tower_lsp::{Server, LspService};

struct RayaLanguageServer {
    // ...
}

#[tower_lsp::async_trait]
impl LanguageServer for RayaLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult>;
    async fn did_open(&self, params: DidOpenTextDocumentParams);
    async fn completion(&self, params: CompletionParams) -> Result<CompletionResponse>;
    // ... all LSP methods
}
```

### 7.5 Debugger

**Tasks:**
- [ ] Add bytecode debugging support
- [ ] Support breakpoints
- [ ] Add step-through execution
- [ ] Inspect variables and stack
- [ ] Integrate with DAP (Debug Adapter Protocol)

**Files:**
```rust
// crates/raya-debugger/src/lib.rs
pub struct Debugger {
    vm: Vm,
    breakpoints: HashSet<(FunctionId, usize)>,
}

impl Debugger {
    pub fn set_breakpoint(&mut self, location: Location);
    pub fn step(&mut self);
    pub fn continue_execution(&mut self);
    pub fn inspect_variable(&self, name: &str) -> Option<Value>;
}
```

### 7.6 Documentation Generator

**Tasks:**
- [ ] Parse doc comments
- [ ] Generate HTML documentation
- [ ] Support markdown in comments
- [ ] Create API reference

**Files:**
```rust
// crates/raya-doc/src/lib.rs
pub struct DocGenerator;

impl DocGenerator {
    pub fn generate(&self, module: &TypedModule) -> Documentation {
        // Extract doc comments
        // Generate HTML
    }
}
```

---

## Milestones

### Milestone 1: Hello World (Weeks 1-4)
- [x] Project setup
- [ ] Basic bytecode interpreter
- [ ] Simple lexer and parser
- [ ] Minimal type checker
- [ ] Compile and run "Hello, World!"

**Goal:** Execute a simple Raya program.

```typescript
function main(): void {
  console.log("Hello, World!");
}
```

### Milestone 2: Core Features (Weeks 5-12)
- [ ] Full expression support
- [ ] Functions and closures
- [ ] Classes and objects
- [ ] Basic type checking
- [ ] Garbage collection

**Goal:** Run non-trivial programs with functions and objects.

### Milestone 3: Type System (Weeks 13-20)
- [ ] Complete type inference
- [ ] Discriminated unions
- [ ] Exhaustiveness checking
- [ ] Bare union transformation
- [ ] Generics and monomorphization

**Goal:** Enforce sound type safety.

### Milestone 4: Concurrency (Weeks 21-28)
- [ ] Task scheduler
- [ ] Work-stealing
- [ ] Async/await
- [ ] Mutex support
- [ ] Task utilities (sleep, all, race)

**Goal:** Run concurrent programs efficiently.

### Milestone 5: Standard Library (Weeks 29-32)
- [ ] Core types
- [ ] raya:std module
- [ ] raya:json module
- [ ] Built-in type methods
- [ ] Console API

**Goal:** Provide essential runtime functionality.

### Milestone 6: Tooling (Weeks 33-40)
- [ ] CLI tool (rayac)
- [ ] Package manager (raya-pm)
- [ ] Test framework
- [ ] REPL
- [ ] Code formatter

**Goal:** Productive developer experience.

### Milestone 7: Advanced Features (Weeks 41-48)
- [ ] LSP server
- [ ] Debugger
- [ ] Documentation generator
- [ ] Optimization passes
- [ ] Reflection (optional)
- [ ] VM Snapshotting (pause, snapshot, resume)
- [ ] Inner VMs (nested VmContexts with isolation and control)

**Goal:** Complete development environment.

### Milestone 8: Production Ready (Weeks 49-52)
- [ ] Performance optimization
- [ ] Security audit
- [ ] Documentation
- [ ] Example projects
- [ ] Public release

**Goal:** Stable 1.0 release.

---

## Dependencies Graph

```
raya-bytecode
    ↓
raya-core → raya-stdlib
    ↓              ↓
raya-types   raya-test
    ↓              ↓
raya-parser      ↓
    ↓              ↓
raya-compiler    ↓
    ↓              ↓
raya-cli ←-------┘
    ↓
raya-lsp
```

---

## Next Steps

1. **Set up project structure** - Create all crates
2. **Implement bytecode definitions** - Complete `raya-bytecode`
3. **Build interpreter core** - Start with `raya-core`
4. **Test with hand-written bytecode** - Validate VM works
5. **Build lexer and parser** - Start `raya-parser`
6. **Implement type checker** - Complete `raya-types`
7. **Continue with compilation pipeline** - Work on `raya-compiler`

---

**Status:** Planning Complete
**Version:** v0.1 (Implementation Plan)
**Last Updated:** 2026-01-04
