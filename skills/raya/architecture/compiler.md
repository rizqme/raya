# Compilation Pipeline

Raya's compiler transforms source code through multiple stages, each with specific responsibilities.

## Pipeline Overview

```
Source (.raya)
    ↓ [Lexer]
Tokens
    ↓ [Parser]
AST (Abstract Syntax Tree)
    ↓ [Binder]
Bound AST (symbols resolved)
    ↓ [Type Checker]
Typed AST (types verified)
    ↓ [IR Generator]
IR (Three-address code)
    ↓ [Monomorphizer]
Specialized IR (generics expanded)
    ↓ [Optimizer]
Optimized IR
    ↓ [Codegen]
Bytecode Module
    ↓ [Serializer]
.ryb file
```

## Stage 1: Lexical Analysis

**Location:** `crates/raya-engine/src/parser/lexer.rs`

**Purpose:** Convert source text to token stream

**Tokens:**
- Keywords: `function`, `class`, `if`, `for`, `async`, etc.
- Identifiers: `myVar`, `MyClass`
- Literals: `42`, `3.14`, `"string"`
- Operators: `+`, `-`, `*`, `/`, `==`, etc.
- Punctuation: `{`, `}`, `(`, `)`, `;`, `,`

**Features:**
- Hand-written (not generated)
- Fast single-pass
- Error recovery
- Source location tracking

## Stage 2: Syntax Analysis

**Location:** `crates/raya-engine/src/parser/`

**Purpose:** Build Abstract Syntax Tree

**Parser Strategy:** Recursive descent with operator precedence

**AST Nodes:**
```rust
pub enum Stmt {
    Function(FunctionDecl),
    Class(ClassDecl),
    Let(LetStmt),
    If(IfStmt),
    For(ForStmt),
    Return(ReturnStmt),
    // ...
}

pub enum Expr {
    Binary(BinaryExpr),
    Call(CallExpr),
    Member(MemberExpr),
    Literal(LiteralExpr),
    // ...
}
```

**Error Handling:**
- Rich diagnostics with `codespan`
- Multiple errors per pass
- Contextual error messages

## Stage 3: Binding

**Location:** `crates/raya-engine/src/compiler/bind/`

**Purpose:** Resolve symbol references

**Tasks:**
- Build symbol tables
- Resolve imports
- Check name shadowing
- Detect undeclared variables

**Scope Rules:**
- Block-scoped (not function-scoped)
- Lexical closure capture
- No hoisting

## Stage 4: Type Checking

**Location:** `crates/raya-engine/src/parser/checker/`

**Purpose:** Verify type correctness

**Features:**
- Type inference
- Type narrowing (control flow analysis)
- Generic constraint checking
- Interface implementation verification
- Discriminated union exhaustiveness

**Type Narrowing:**
```typescript
function process(x: string | int): void {
  if (typeof x == "string") {
    // x is string here
    x.toUpperCase();
  } else {
    // x is int here
    x * 2;
  }
}
```

**Errors Caught:**
- Type mismatches
- Missing properties
- Invalid operations
- Non-exhaustive pattern matching
- Generic constraint violations

## Stage 5: IR Generation

**Location:** `crates/raya-engine/src/compiler/lower/`

**Purpose:** Lower AST to three-address code IR

**IR Form:**
```rust
pub enum IrInstr {
    Assign { dest: Local, value: IrValue },
    BinOp { dest: Local, op: BinOp, left: IrValue, right: IrValue },
    Call { dest: Local, func: IrValue, args: Vec<IrValue> },
    Return { value: Option<IrValue> },
    // ...
}
```

**Example:**
```typescript
// Source
const x = a + b * c;

// IR
%t1 = mul %b, %c
%x = add %a, %t1
```

**Benefits:**
- Simpler to analyze than AST
- Easier to optimize
- Closer to bytecode

## Stage 6: Monomorphization

**Location:** `crates/raya-engine/src/compiler/monomorphize/`

**Purpose:** Specialize generic functions and classes

**Process:**
1. Find all generic instantiations
2. For each instantiation, create specialized copy
3. Replace type parameters with concrete types
4. Generate typed opcodes (IADD vs FADD)

**Example:**
```typescript
// Source
function identity<T>(x: T): T { return x; }

const a = identity<int>(42);
const b = identity<string>("hi");

// Monomorphized
function identity_int(x: int): int { return x; }
function identity_string(x: string): string { return x; }
```

**Trade-offs:**
- **Pros:** Fast execution, no boxing, typed opcodes
- **Cons:** Larger binary size

## Stage 7: Optimization

**Location:** `crates/raya-engine/src/compiler/optimize/`

**Passes:**

### Constant Folding
```typescript
// Before
const x = 2 + 3 * 4;

// After
const x = 14;
```

### Dead Code Elimination (DCE)
```typescript
// Before
const x = compute();  // x never used
return 42;

// After
return 42;
```

### Inlining
```typescript
// Before
function double(x: int): int { return x * 2; }
const y = double(10);

// After
const y = 10 * 2;
```

### Loop Optimization
- Loop-invariant code motion
- Strength reduction
- Loop unrolling (small loops)

## Stage 8: Bytecode Generation

**Location:** `crates/raya-engine/src/compiler/codegen/`

**Purpose:** Generate typed bytecode

**Opcode Categories:**
- **Stack:** Push, Pop, Dup
- **Arithmetic:** IAdd, FAdd, NAdd (typed!)
- **Control:** Jump, JumpIf, Call, Return
- **Objects:** NewObject, GetField, SetField
- **Arrays:** NewArray, GetIndex, SetIndex
- **Tasks:** Spawn, Await

**Example:**
```typescript
// Source
const x = a + b;

// Bytecode (int)
LoadLocal a
LoadLocal b
IAdd          // Int addition
StoreLocal x

// Bytecode (number)
LoadLocal a
LoadLocal b
FAdd          // Float addition
StoreLocal x
```

**Module Format:**
```rust
pub struct Module {
    pub functions: Vec<Function>,
    pub classes: Vec<ClassDef>,
    pub globals: Vec<Global>,
    pub constants: Vec<Constant>,
    pub bytecode: Vec<u8>,
}
```

## Stage 9: Serialization

**Location:** `crates/raya-engine/src/compiler/bytecode/`

**Purpose:** Write to .ryb file

**Format:**
```
[magic: 8 bytes]
[version: 2 bytes]
[module_name_len: 2 bytes]
[module_name: utf8]
[num_functions: 4 bytes]
[functions: ...]
[num_classes: 4 bytes]
[classes: ...]
[bytecode_len: 4 bytes]
[bytecode: bytes]
```

**Features:**
- Compact binary format
- Fast deserialization
- Version checking
- Integrity validation

## Compiler Options

```rust
pub struct CompilerOptions {
    pub optimize_level: u8,      // 0-3
    pub inline_threshold: usize, // Max inline size
    pub warnings: WarningLevel,  // Strict/Normal/Off
    pub debug_info: bool,        // Include debug symbols
}
```

## Type-Aware Code Generation

Raya generates **typed opcodes** based on type information:

| Type | Add | Mul | Compare |
|------|-----|-----|---------|
| `int` | `IAdd` | `IMul` | `ILt`, `IEq` |
| `number` | `FAdd` | `FMul` | `FLt`, `FEq` |
| Generic | `NAdd` | `NMul` | `NLt`, `NEq` |

Benefits:
- No runtime type checks
- Unboxed arithmetic
- Direct memory access
- Better cache locality

## Module Compilation

### Single File

```bash
raya build main.raya -o main.ryb
```

Process:
1. Parse `main.raya`
2. Compile through pipeline
3. Write `main.ryb`

### Multi-File Project

```bash
raya build --project
```

Process:
1. Parse `raya.toml` manifest
2. Resolve dependencies
3. Compile each module
4. Link modules
5. Write output files

## Compiler Intrinsics

Some operations are lowered to special compiler intrinsics:

- `String.replaceWith` → Inline loop + CallClosure
- `Array.map` → Specialized loop (if lambda is simple)
- Math operations → Native calls with known IDs

## Error Reporting

The compiler uses `codespan` for rich diagnostics:

```
error[E0308]: type mismatch
  ┌─ main.raya:10:15
  │
10 │     const x: int = "hello";
  │              ^^^   ^^^^^^^ expected int, found string
  │              │
  │              expected due to this type
```

## Performance

| Phase | Time (1000 LOC) | Memory |
|-------|----------------|--------|
| Lexing | ~5ms | ~100KB |
| Parsing | ~15ms | ~1MB |
| Type Checking | ~20ms | ~2MB |
| IR Generation | ~10ms | ~500KB |
| Optimization | ~30ms | ~1MB |
| Codegen | ~10ms | ~200KB |
| **Total** | **~90ms** | **~5MB** |

## Related

- [Overview](overview.md) - Architecture overview
- [VM](vm.md) - Bytecode execution
- [JIT/AOT](jit-aot.md) - Native compilation
