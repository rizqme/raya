# CLAUDE.md - AI Assistant Guide for Raya Project

**Last Updated:** 2026-01-04
**Project:** Raya Programming Language & Virtual Machine
**Implementation Language:** Rust (stable)

---

## 🎯 Project Overview

**Raya** is a statically-typed programming language with TypeScript syntax, implemented in Rust. It features a custom virtual machine with goroutine-style concurrency and a fully static type system with zero runtime type checks.

### Quick Facts

- **Language:** Rust (stable toolchain)
- **Type:** Programming language implementation (compiler + VM)
- **Syntax:** TypeScript subset
- **Runtime:** Custom bytecode VM with multi-threaded task scheduler
- **Concurrency:** Goroutine-style green threads (Tasks)
- **Type System:** Fully static, sound, with discriminated unions
- **Status:** Design complete, implementation in progress

---

## 🏗️ Project Structure

```
rayavm/
├── crates/                  # Rust workspace crates
│   ├── raya-core/        # VM runtime (interpreter, GC, scheduler)
│   ├── raya-bytecode/    # Bytecode definitions and encoding
│   ├── raya-parser/      # Lexer & Parser (logos/hand-written)
│   ├── raya-types/       # Type system & type checker
│   ├── raya-compiler/    # Code generation (AST → bytecode)
│   ├── raya-stdlib/      # Standard library (native implementations)
│   ├── raya-cli/         # CLI tool (rayac)
│   └── raya-pm/             # Package manager
├── stdlib/                 # Raya standard library source (.raya files)
├── design/                 # Complete specification documents
│   ├── README.md          # Design overview
│   ├── LANG.md            # Language specification (~2500 lines)
│   ├── ARCHITECTURE.md    # VM architecture
│   ├── OPCODE.md          # Bytecode instruction set
│   ├── MAPPING.md         # Language → bytecode mappings
│   ├── SNAPSHOTTING.md    # VM snapshotting design
│   ├── INNER_VM.md        # Inner VM & controllability
│   └── STDLIB.md          # Standard library design
├── plans/                 # Implementation roadmap
│   └── PLAN.md           # Detailed implementation plan
├── tests/                 # Integration tests
└── examples/              # Example Raya programs
```

### Key Rust Dependencies

- **clap** - CLI argument parsing
- **serde** / **serde_json** - Serialization
- **logos** - Lexer generation (or hand-written alternative)
- **crossbeam** - Work-stealing scheduler for Tasks
- **parking_lot** - Efficient synchronization primitives
- **rustc-hash** - Fast hashing
- **mimalloc** - Fast allocator

---

## 🔑 Critical Design Principles

### 1. **Type System with typeof for Primitives**

**ALLOWED FOR PRIMITIVES:**
- `typeof` operator for bare unions of primitives (`string | number | boolean | null`)
- Control flow-based type narrowing
- Exhaustiveness checking

**ABSOLUTELY BANNED:**
- `instanceof` operator
- `any` type
- Type assertions (`as`)
- Type casts
- Runtime type tags/RTTI (except with `--emit-reflection` flag)

**Two Patterns for Unions:**

**1. Bare Unions (Primitives Only) - Use `typeof`:**
```typescript
// ✅ ALLOWED for primitives
type ID = string | number;
let id: ID = 42;

if (typeof id === "number") {
  console.log(id + 1);  // id is narrowed to number
} else {
  console.log(id.toUpperCase());  // id is narrowed to string
}
```

**2. Discriminated Unions (Complex Types) - Use Discriminants:**
```typescript
// ✅ REQUIRED for objects/classes
type Value =
  | { kind: "string"; value: string }
  | { kind: "number"; value: number };

if (value.kind === "string") {
  console.log(value.value.toUpperCase());
} else {
  console.log(value.value.toFixed(2));
}
```

### 2. **Monomorphization (Like Rust/C++)**

Generics are specialized at compile time:
- Each concrete type instantiation generates a separate function/class
- Zero runtime overhead for generics
- Type-specific optimizations possible

### 3. **Goroutine-Style Concurrency**

- `async` functions **always** create a new Task (green thread)
- Tasks start **immediately** when async function is called
- `await` **suspends** the current Task (doesn't block OS thread)
- Work-stealing scheduler across all CPU cores (automatic)
- Default workers = CPU core count (configurable via `RAYA_NUM_THREADS`)

### 4. **Type-Driven Performance**

The compiler uses types to optimize:
- **Typed opcodes:** `IADD` (integer), `FADD` (float), `NADD` (number)
- **Unboxed primitives:** Numbers, booleans stored directly
- **Monomorphization:** Generic code specialized per type
- **No type checking overhead:** All verification at compile time

---

## 📚 Essential Reading for AI Assistants

When working on this project, **always reference these documents**:

### 1. [design/LANG.md](design/LANG.md) - Language Specification (~2500 lines)
- Complete syntax and semantics
- Type system rules
- Discriminated unions (Section 4.7)
- Generics and monomorphization (Section 13)
- Concurrency model (Section 14)
- Banned features (Section 19)

### 2. [design/ARCHITECTURE.md](design/ARCHITECTURE.md) - VM Design
- Task scheduler (work-stealing)
- Memory model
- Garbage collection
- Object representation
- Mutex implementation

### 3. [design/OPCODE.md](design/OPCODE.md) - Bytecode Instructions
- All VM opcodes (25+ categories)
- Typed arithmetic opcodes
- Task operations (`SPAWN`, `AWAIT`)
- Control flow instructions

### 4. [design/MAPPING.md](design/MAPPING.md) - Compilation Examples
- How each language construct compiles to bytecode
- Side-by-side examples
- Optimization patterns

### 5. [design/SNAPSHOTTING.md](design/SNAPSHOTTING.md) - VM Snapshotting
- Stop-the-world snapshotting protocol
- Safepoint-based task suspension
- Snapshot format and resume semantics
- Multi-context snapshotting support

### 6. [design/INNER_VM.md](design/INNER_VM.md) - Inner VMs
- Nested VmContexts with isolation
- Resource limits and enforcement
- Capability-based security model
- Data marshalling and fair scheduling

### 7. [plans/PLAN.md](plans/PLAN.md) - Implementation Roadmap
- Rust crate structure
- Phase-by-phase implementation plan
- File organization
- Testing strategy

---

## 🚨 Common Pitfalls to Avoid

### ❌ DON'T:

1. **Add runtime type checking**
   - The entire design philosophy is compile-time-only types
   - No type tags, no RTTI (unless `--emit-reflection` is enabled)

2. **Use JavaScript semantics**
   - Raya is NOT JavaScript
   - No prototype chains, no dynamic property access (except via reflection)
   - Fixed object layouts determined at compile time

3. **Suggest `typeof` or `instanceof`**
   - These are explicitly banned
   - Always use discriminated unions instead

4. **Ignore the Rust implementation**
   - This is a **Rust project** - follow Rust idioms
   - Use Rust's type system to enforce invariants
   - Leverage Rust's ownership for safety

5. **Skip reading the design docs**
   - The design is very specific and intentional
   - Many TypeScript features are explicitly excluded

### ✅ DO:

1. **Follow the specification exactly**
   - The design docs are the source of truth
   - Raya is a strict subset of TypeScript

2. **Use Rust best practices**
   - Idiomatic Rust code
   - Zero-copy where possible
   - Strong typing in the compiler/VM

3. **Enforce compile-time guarantees**
   - Type checking is exhaustive
   - All unions must be discriminated
   - Exhaustiveness checking is mandatory

4. **Think about performance**
   - Types enable optimization
   - Monomorphization is key
   - Unboxed primitives matter

---

## 🎯 Current Implementation Status

### ✅ Complete:
- Language specification (LANG.md)
- VM architecture design (ARCHITECTURE.md)
- Opcode set definition (OPCODE.md)
- Language-to-bytecode mappings (MAPPING.md)
- VM snapshotting design (SNAPSHOTTING.md)
- Inner VM design (INNER_VM.md)
- Implementation plan (PLAN.md)
- Milestone 1.2: Bytecode definitions and encoding

### ⏳ In Progress:
- Rust workspace setup
- VM core implementation
- Parser and type checker
- Compiler (bytecode generation)
- Standard library

See [plans/PLAN.md](plans/PLAN.md) for detailed milestones.

---

## 🛠️ Working with the Codebase

### When Implementing Features:

1. **Check the specification first**
   - Read relevant sections of LANG.md
   - Understand the design intent

2. **Review the architecture**
   - Check ARCHITECTURE.md for VM behavior
   - Check OPCODE.md for instruction details
   - Check MAPPING.md for compilation patterns
   - Check SNAPSHOTTING.md for pause/resume design
   - Check INNER_VM.md for nested VM isolation

3. **Follow the implementation plan**
   - PLAN.md has detailed task breakdowns
   - Each phase builds on previous phases

4. **Write Rust code that mirrors the design**
   - Use strong typing
   - Enforce invariants with Rust's type system
   - Add comprehensive tests

### When Writing Bytecode Generation:

- Reference MAPPING.md for exact patterns
- Use typed opcodes based on static types
- Implement monomorphization for generics
- Inline `match()` calls (compile-time magic)

### When Implementing the VM:

- Reference ARCHITECTURE.md for scheduler design
- Use work-stealing deques (crossbeam)
- Implement atomic single-variable access
- Follow the memory model exactly

---

## 📖 Key Concepts Reference

### Discriminated Unions

Every union type must have a discriminant field:

```typescript
// Compiler infers "status" as discriminant
type Result<T, E> =
  | { status: "ok"; value: T }
  | { status: "error"; error: E };
```

Priority for discriminant inference: `kind > type > tag > variant > alphabetical`

### Bare Unions (typeof for Primitives)

Primitive-only unions use `typeof` for type narrowing:

```typescript
type ID = string | number;
let id: ID = 42;

// Use typeof for type narrowing
if (typeof id === "number") {
  console.log(id + 1);  // id is number here
} else {
  console.log(id.toUpperCase());  // id is string here
}

// Values stored directly, no boxing
// Runtime: Value::i32(42) or Value::string_ptr(...)
```

### Tasks vs Promises

```typescript
// async function ALWAYS creates a Task
async function work(): Task<number> {
  return 42;
}

const task = work();  // Task starts NOW (not lazy)
const result = await task;  // Suspend current Task
```

### Monomorphization Example

```rust
// Source:
function identity<T>(x: T): T { return x; }
let a = identity(42);

// Compiler generates:
// fn identity_number(x: number) -> number { x }
// fn identity_string(x: string) -> string { x }
```

---

## 🧪 Testing Strategy

- **Unit tests:** Each Rust crate has its own tests
- **Integration tests:** In `tests/` directory
- **Bytecode tests:** Verify opcode execution
- **Type checker tests:** Ensure sound type checking
- **Concurrency tests:** Task scheduler stress tests
- **Examples:** Real programs in `examples/`

---

## 🔧 Build Commands

```bash
# Build all crates
cargo build

# Run tests
cargo test

# Build CLI
cargo build --release -p raya-cli

# Run Raya program (once implemented)
cargo run -p raya-cli -- run program.raya
```

---

## 📝 Code Style Guidelines

### Rust Code:
- Follow standard Rust style (rustfmt)
- Use descriptive names
- Document public APIs
- Add inline comments for complex logic
- Prefer `Result<T, E>` for error handling

### Raya Code (examples/stdlib):
- Follow TypeScript conventions
- Always use type annotations
- Use discriminated unions for sum types
- Document public APIs with TSDoc comments

---

## 🌟 Design Philosophy Summary

| Principle | Implementation |
|-----------|----------------|
| **Explicit over implicit** | Discriminated unions, type annotations |
| **Safety over convenience** | No escape hatches, sound type system |
| **Performance through types** | Static types enable optimization |
| **Familiar syntax** | TypeScript-compatible where possible |
| **Predictable semantics** | Well-defined execution model |

---

## 🔗 Additional Resources

- **Raya vs TypeScript:** See [design/README.md](design/README.md)
- **Concurrency Model:** Like Go's goroutines - see Section 14 of LANG.md
- **Memory Model:** Simple happens-before rules - see Section 21 of LANG.md
- **Optional Reflection:** Available with `--emit-reflection` - see Section 18 of LANG.md

---

## ✨ Quick Decision Guide for AI Assistants

**Question:** "Can I use `typeof` in Raya code?"
**Answer:** ✅ YES, for bare unions of primitives (`string | number | boolean | null`). Use discriminated unions for complex types.

**Question:** "How do I handle primitive unions?"
**Answer:** ✅ Use `typeof` for type narrowing:
```typescript
type ID = string | number;
if (typeof id === "number") { /* ... */ }
```

**Question:** "How do I handle object/class unions?"
**Answer:** ✅ Discriminated unions with explicit discriminant fields:
```typescript
type Value = { kind: "str"; value: string } | { kind: "num"; value: number };
if (value.kind === "str") { /* ... */ }
```

**Question:** "Should I add runtime type checking for complex types?"
**Answer:** ❌ NO. All complex types use compile-time discriminants (unless using optional reflection).

**Question:** "Should generic code be type-erased?"
**Answer:** ❌ NO. Use monomorphization (like Rust/C++).

**Question:** "How does concurrency work?"
**Answer:** ✅ Goroutine-style: async functions create Tasks, await suspends Tasks.

**Question:** "What language is this implemented in?"
**Answer:** ✅ **Rust** (stable toolchain).

**Question:** "Can I add a TypeScript feature?"
**Answer:** ⚠️ Only if it's compatible with Raya's design (check LANG.md Section 19).

---

## 🎓 Learning Path for New Contributors

1. Read [design/README.md](design/README.md) - 15 minutes
2. Skim [design/LANG.md](design/LANG.md) - 30 minutes
3. Read [design/ARCHITECTURE.md](design/ARCHITECTURE.md) - 20 minutes
4. Review [plans/PLAN.md](plans/PLAN.md) - 20 minutes
5. Look at examples in [design/MAPPING.md](design/MAPPING.md) - 15 minutes

**Total:** ~90 minutes to understand the full design

---

**Remember:** Raya is not JavaScript, not quite TypeScript, but a carefully designed strict subset with a predictable runtime. When in doubt, check the design docs!
