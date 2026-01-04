# Raya Language & VM Design Documentation

This directory contains the complete specification for the Raya programming language and its virtual machine.

---

## Documents

### [LANG.md](LANG.md) - Language Specification
Complete language reference including syntax, semantics, type system, and examples.

**Key Sections:**
- Lexical structure and operators
- Type system with discriminated unions
- Classes, interfaces, and generics
- Concurrency model (Tasks and async/await)
- Module system
- Memory model and synchronization

### [ARCHITECTURE.md](ARCHITECTURE.md) - VM Architecture
High-level design of the Raya virtual machine runtime.

**Key Features:**
- Task-based execution model (green threads)
- Multi-threaded scheduler with work-stealing
- Automatic CPU core utilization (like Go)
- Memory model and garbage collection
- Mutex implementation

### [OPCODE.md](OPCODE.md) - Bytecode Instruction Set
Complete reference of all VM opcodes.

**Categories:**
- Stack manipulation and constants
- Typed arithmetic (IADD, FADD, NADD)
- Control flow and function calls
- Object and array operations
- Task concurrency (SPAWN, AWAIT)
- Closures and static members

### [MAPPING.md](MAPPING.md) - Language to Bytecode Mapping
Detailed examples showing how each language feature compiles to bytecode.

**Includes:**
- Side-by-side Raya source and bytecode
- Translation strategies for all constructs
- Optimization patterns
- Compiler hints

---

## Key Design Decisions

### 1. Fully Static Type System (Zero Runtime Type Checks)

**Core Principle:** All types are known and verified at compile time. The VM operates on typed values without any runtime type checking.

**What's BANNED:**
- `typeof` operator
- `instanceof` operator
- `any` type
- Type assertions (`as`)
- Type casts
- Non-null assertions (`!`)
- Runtime type tags/RTTI

**What's REQUIRED:**
- Discriminated unions with explicit discriminant fields
- Compile-time type narrowing
- Exhaustiveness checking
- Monomorphization for generics

**How it works:**

1. **Compile-time:**
   - Compiler verifies all types
   - Monomorphizes generic code
   - Generates typed bytecode
   - Ensures exhaustiveness

2. **Runtime:**
   - No type tags on values
   - No type checking opcodes
   - Direct dispatch (vtables for methods)
   - Value-based discrimination only

**Two Patterns:**

1. **Bare Primitive Unions** (automatic):
```ts
// For primitives, compiler handles automatically
type ID = string | number;
let id: ID = 42;

import { match } from "raya:std";
match(id, {
  string: (s) => console.log(`String: ${s}`),
  number: (n) => console.log(`Number: ${n}`)
});
```

2. **Discriminated Unions** (explicit):
```ts
// For complex types, use explicit discriminants
type Value =
  | { kind: "string"; value: string }
  | { kind: "number"; value: number };

match(value, {
  string: (v) => console.log(v.value),
  number: (v) => console.log(v.value)
});
```

**Benefits:**
1. **Minimal runtime overhead** — No type tags/RTTI, only value-based discriminants
   - Bare unions: Boxing overhead (16 bytes per value on 64-bit)
   - Discriminated unions: Zero overhead (fields already exist)
   - No dynamic type checking (typeof/instanceof)
2. **Compile-time safety** — Type errors caught before execution
3. **Exhaustiveness** — Compiler ensures all cases handled
4. **Ergonomic** — Bare unions for simple cases, explicit for complex
5. **Smaller runtime** — No type introspection machinery

### 2. Goroutine-Style Concurrency

**Model:** Green threads (Tasks) scheduled over OS threads

**Key Features:**
- `async` functions always create a new Task
- Tasks start immediately when function is called
- `await` suspends current Task (doesn't block OS thread)
- Automatic work-stealing across all CPU cores

**Example:**
```ts
async function worker(): Task<number> {
  return 42;
}

const task = worker();  // Task starts NOW
const result = await task;  // Suspend until complete
```

### 3. Monomorphization for Generics

**Strategy:** Generic code is specialized for each concrete type at compile time.

**Benefits:**
- Zero runtime overhead for generics
- Type-specific optimizations
- Better inlining opportunities
- No generic dispatch mechanism needed

**Example:**
```ts
function identity<T>(x: T): T { return x; }

let a = identity(42);       // Compiles to identity_number
let b = identity("hello");  // Compiles to identity_string

// Compiler generates:
// function identity_number(x: number): number { return x; }
// function identity_string(x: string): string { return x; }
```

**Classes:**
```ts
let numBox = new Box<number>(42);
let strBox = new Box<string>("hi");

// Generates distinct classes: Box_number and Box_string
```

### 4. Predictable Runtime (Type-Erased)

**Guarantees:**
- **No type tags** — Values don't carry runtime type information
- **Fixed layouts** — Object structure determined at compile time
- **Vtable dispatch** — Methods resolved via class metadata, not type queries
- **Atomic access** — Single-variable reads/writes are atomic
- **Clear memory model** — Happens-before relationships via Tasks and Mutexes

**Key insight:** Once compiled, the VM doesn't know about types. It trusts the compiler verified everything.

### 5. Type-Driven Performance

**Compiler uses types for optimization:**
- **Typed opcodes** — IADD vs FADD vs NADD based on static types
- **Unboxed primitives** — Numbers, booleans stored directly (no boxing)
- **Specialized layouts** — Arrays of numbers use packed buffers
- **Monomorphization** — Generic code specialized, enabling inlining
- **GC optimization** — Pointer maps from type metadata (precise GC)

---

## Design Philosophy

| Principle | Implementation |
|-----------|----------------|
| **Explicit over implicit** | Discriminated unions, type annotations |
| **Safety over convenience** | No escape hatches, sound type system |
| **Performance through types** | Static types enable optimization |
| **Familiar syntax** | TypeScript-compatible where possible |
| **Predictable semantics** | Well-defined execution model |

---

## Concurrency Primitives

### Tasks
- Lightweight green threads
- Scheduled over OS thread pool
- Created via `async` functions
- Managed by work-stealing scheduler

### Synchronization
- **Mutex** for mutual exclusion
- Atomic single-variable access
- No `await` in critical sections (enforced)

### Memory Model
- Sequential consistency within Task
- Happens-before via Task completion
- Happens-before via Mutex lock/unlock

---

## Type System Highlights

### Discriminated Unions (Required)
```ts
type Result<T> =
  | { status: "ok"; value: T }
  | { status: "err"; error: string };
```

### Structural Typing (Interfaces)
```ts
interface Point {
  x: number;
  y: number;
}
// Any compatible object satisfies interface
```

### Nominal Typing (Classes)
```ts
class Point { constructor(public x: number, public y: number) {} }
class Vector { constructor(public x: number, public y: number) {} }
// Point and Vector are distinct types despite same structure
```

### Generics
```ts
class Box<T> {
  constructor(public value: T) {}
}
```

---

## Module System

- **ES6 modules** (static imports/exports)
- **Named exports only** (no default exports)
- **Compile-time resolution**
- **No circular dependency issues**

---

## Standard Library (Minimal)

- **Console**: `console.log`, `console.error`
- **Math**: Basic math operations
- **String**: Standard string methods
- **Array**: Standard array methods
- **Task utilities**: `sleep`, `all`, `race`
- **Pattern matching**: `match()` utility for all union types (see LANG.md Section 17.6)
- **JSON**: Compile-time `encode`/`decode` (see LANG.md Section 17.7)

---

## Compilation Pipeline

1. **Parse** — Source to AST
2. **Type Check** — Validate all types, enforce discriminated unions
3. **Code Generation** — Generate JSON encoders/decoders for types used with `JSON.encode()`/`JSON.decode<T>()`
4. **Lower** — AST to typed IR
5. **Optimize** — Type-based optimizations
6. **Emit** — IR to typed bytecode
7. **Verify** — Bytecode verification
8. **Execute** — VM interprets bytecode

---

## Optional Reflection System

Raya provides an **optional reflection capability** for advanced use cases:

**When Enabled (`--emit-reflection`):**
- Type metadata embedded in bytecode
- `Reflect` API available for runtime type introspection
- Enables TypeScript compatibility shims
- Useful for serialization, debugging, and interop

**Use Cases:**
- **TypeScript Compatibility**: Build shims that implement `typeof`/`instanceof` via reflection
- **Debugging**: Runtime inspection of types and properties
- **Interoperability**: Bridge to dynamic languages or external systems

**Note:** JSON serialization uses compile-time code generation by default (see LANG.md Section 17.7), not reflection

**Performance:**
- +10-30% binary size (metadata only)
- No impact on normal code execution
- Reflection API calls have overhead
- Recommended for dev builds and interop layers only

**Example:**
```ts
import * as Reflect from "raya:reflect";

// Get type information
const typeInfo = Reflect.typeOf(value);
console.log(typeInfo.name); // "User"

// Build TypeScript compat shim
export function typeof(value: any): string {
  const type = Reflect.typeOf(value);
  return type.kind === "primitive" ? type.name : "object";
}
```

See [LANG.md Section 18](LANG.md#18-optional-reflection-system) for full API documentation.

---

## Future Extensions

### Potential Features
- JIT compilation for hot code
- Channels (Go-style) for communication
- SIMD operations
- Atomic operations API
- Advanced type features (conditional types, mapped types)
- Abstract classes
- Access modifiers (private, protected)

### Not Planned
- Prototype manipulation
- Dynamic `eval`
- `any` type
- Built-in runtime type introspection (use optional reflection instead)

---

## Comparison with TypeScript

| Feature | TypeScript | Raya |
|---------|-----------|------|
| Syntax | Full TS syntax | Strict subset |
| Type safety | Opt-in, unsound | Always enforced, sound |
| Type checking | Compile-time only | Compile-time only |
| Type information | Erased | Erased + used for optimization |
| `any` type | Allowed | **Banned** |
| `typeof` | Allowed (runtime check) | **Banned** (compile-time only) |
| `instanceof` | Allowed (runtime check) | **Banned** (compile-time only) |
| Type assertions | Allowed (unsound) | **Banned** |
| Union narrowing | `typeof`/`instanceof` | Discriminated unions only |
| Generics | Type erasure | **Monomorphization** |
| Runtime | JavaScript (dynamic) | Custom VM (typed bytecode) |
| Concurrency | Promises (single-threaded) | Tasks (multi-threaded) |
| Objects | Prototype-based | Class-based, fixed layouts |
| Method dispatch | Property lookup | Vtable dispatch |
| Type tags | None | None (fully erased) |

---

## Implementation Status

**Current Version:** v0.5 (Specification)

**Status:**
- ✅ Language specification complete
- ✅ VM architecture designed
- ✅ Opcode set defined
- ✅ Language-to-bytecode mapping documented
- ⏳ Compiler implementation (pending)
- ⏳ VM implementation (pending)
- ⏳ Standard library (pending)

---

## Getting Started (for Implementers)

### 1. Read the Specification
Start with [LANG.md](LANG.md) to understand language semantics.

### 2. Understand the VM
Read [ARCHITECTURE.md](ARCHITECTURE.md) for runtime design.

### 3. Study Opcodes
Review [OPCODE.md](OPCODE.md) for instruction set.

### 4. Review Mappings
Study [MAPPING.md](MAPPING.md) for compilation patterns.

### 5. Implement
Begin with:
- Lexer and parser
- Type checker (with discriminated union enforcement)
- Bytecode emitter
- VM core (interpreter)
- Task scheduler
- Garbage collector

---

## Design Rationale

### Why ban `typeof` and `instanceof`?

**Problem with runtime type checks:**
- Bypass static type safety
- Encourage defensive programming
- Runtime overhead
- Poor IDE/tooling support
- Implicit design decisions

**Benefits of discriminated unions:**
- Compile-time exhaustiveness checking
- Self-documenting code
- Zero runtime cost
- Better refactoring support
- Forces explicit modeling of variants

### Why goroutine-style concurrency?

**Advantages:**
- Lightweight tasks (thousands possible)
- Automatic parallelism across cores
- Simple mental model
- Structured concurrency
- No callback hell

### Why TypeScript syntax?

**Reasons:**
- Familiar to millions of developers
- Excellent tooling ecosystem
- Clear, readable syntax
- Gradual migration path from TS

---

## Contributing

This is a specification document. For implementation contributions:

1. Follow the language spec exactly
2. Document deviations with rationale
3. Maintain type safety guarantees
4. Preserve concurrency semantics

---

## License

TBD

---

**Raya: A strict, concurrent TypeScript subset with a predictable runtime.**
