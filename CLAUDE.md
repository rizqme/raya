# CLAUDE.md - AI Assistant Guide for Raya Project

**Last Updated:** 2026-01-28
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
raya/
├── crates/                    # Rust workspace crates
│   ├── raya-core/            # VM runtime (interpreter, GC, scheduler)
│   ├── raya-compiler/        # Code generation (AST → IR → bytecode)
│   ├── raya-parser/          # Lexer, parser, type checker
│   ├── raya-builtins/        # Built-in class definitions (.raya files)
│   └── raya-cli/             # CLI tool (rayac) [planned]
├── design/                    # Complete specification documents
│   ├── README.md             # Design overview
│   ├── LANG.md               # Language specification (~2500 lines)
│   ├── ARCHITECTURE.md       # VM architecture
│   ├── OPCODE.md             # Bytecode instruction set
│   ├── MAPPING.md            # Language → bytecode mappings
│   ├── SNAPSHOTTING.md       # VM snapshotting design
│   ├── INNER_VM.md           # Inner VM & controllability
│   ├── BUILTIN_CLASSES.md    # Built-in type definitions
│   └── CHANNELS.md           # Channel semantics
├── plans/                     # Implementation roadmap
│   ├── milestone-3.4.md      # Classes & Concurrency (complete)
│   └── milestone-3.5.md      # Built-in Types (in progress)
├── tests/                     # Integration tests
└── examples/                  # Example Raya programs
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

**ALLOWED FOR CLASSES:**
- `instanceof` operator for class type checking
- Type assertions (`as`) for safe type casting

**ABSOLUTELY BANNED:**
- `any` type
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

### 7. [design/BUILTIN_CLASSES.md](design/BUILTIN_CLASSES.md) - Built-in Types
- Object base class
- Collection types (Map, Set, Buffer)
- Concurrency primitives (Mutex, Channel, Task)
- Error class hierarchy

### 8. [plans/milestone-3.5.md](plans/milestone-3.5.md) - Current Milestone
- Built-in types implementation
- Native call system
- Primitive method dispatch

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

3. **Misuse type operators**
   - `typeof` is for primitives only (not classes)
   - `instanceof` is for classes only (not primitives)
   - Use discriminated unions for complex object unions

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
- **Design Documents:** LANG.md, ARCHITECTURE.md, OPCODE.md, MAPPING.md, SNAPSHOTTING.md, INNER_VM.md
- **Milestone 3.4:** Full class support with inheritance, async/await, exception handling
- **Milestone 3.5 Phase 1-3:** Compiler intrinsics, type operators, Object base class

### ⏳ In Progress (Milestone 3.5):
- **Phase 4:** Hardcoded primitive methods (string/array methods largely complete)
- **Phase 5-11:** Remaining built-in classes (Mutex, Channel, Map, Set, etc.)

### 📊 Test Coverage:
- **358 e2e tests passing** (arrays, strings, classes, concurrency, exceptions)
- String methods: charAt, substring, toUpperCase, toLowerCase, trim, indexOf, includes, startsWith, endsWith, split, replace
- Array methods: push, pop, shift, unshift, indexOf, includes, slice, concat, reverse, join, forEach, filter, find, findIndex, every, some

See [plans/milestone-3.5.md](plans/milestone-3.5.md) for current progress.

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

### Built-in Types System

**Primitives** (hardcoded in compiler):
- `number`, `boolean`, `null`, `string`, `Array<T>`
- Methods emit opcodes or native calls directly
- Cannot be extended by users

**Classes** (defined in raya-builtins/*.raya):
- `Object`, `Mutex`, `Task<T>`, `Channel<T>`, `Error`, `Buffer`, `Map<K,V>`, `Set<T>`, `Date`
- Use `__OPCODE_*` and `__NATIVE_CALL` intrinsics
- Can be extended by users

**Native Call IDs** (in `crates/raya-compiler/src/native_id.rs`):
- Object: 0x00xx
- Array: 0x01xx
- String: 0x02xx
- Mutex: 0x03xx
- Channel: 0x05xx
- Buffer: 0x07xx
- Map: 0x08xx
- Set: 0x09xx
- Date: 0x0Bxx

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

# Run all tests
cargo test

# Run e2e tests only (fastest feedback)
cargo test -p raya-compiler --test e2e_tests

# Run specific test
cargo test -p raya-compiler --test e2e_tests test_array_filter

# Build with release optimizations
cargo build --release
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

**Question:** "Can I use `instanceof` in Raya code?"
**Answer:** ✅ YES, for class type checking:
```typescript
if (obj instanceof MyClass) { /* obj is narrowed to MyClass */ }
```

**Question:** "Can I use `as` for type casting?"
**Answer:** ✅ YES, for safe type assertions:
```typescript
let specific = general as SpecificType;
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
