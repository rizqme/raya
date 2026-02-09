# CLAUDE.md - Raya Project

**Raya** is a statically-typed language with TypeScript syntax, implemented in Rust. Custom bytecode VM with goroutine-style concurrency. Fully static type system with zero runtime type checks.

---

## ⚠️ Documentation Maintenance (MANDATORY)

**After EVERY turn, update relevant documentation:**

1. **Milestone files** (`plans/milestone-*.md`) - Mark tasks `[x]`, update status
2. **PLAN.md** (`plans/PLAN.md`) - Update overall progress and current focus
3. **Hierarchical CLAUDE.md** (`crates/**/CLAUDE.md`) - Keep concise, key info only
4. **Design docs** (`design/*.md`) - If behavior/API changes
5. **This file** - Update status section if milestone progress changes

---

## Current Status

**Complete:** Milestones 3.4-3.6, 3.9, Milestone 3.7 Phases 2-6 (module system)
**Milestone 3.8 (Reflection API):** Core implementation complete
- Phases 1-17 handlers done (metadata, introspection, proxies, dynamic classes, bytecode generation, permissions, VM bootstrap)
- Phases 12, 18 blocked (framework tests, performance validation) - waiting for compiler `std:` import support
- 141+ reflect unit tests passing

**Milestone 3.9 (Decorators):** Complete (41/41 e2e tests)
- Type aliases registered (`ClassDecorator<T>`, `MethodDecorator<F>`, `ParameterDecorator<T>`, etc.)
- `Class<T>` interface available
- Phase 3 code generation complete with decorator lowering
- Phase 4 runtime infrastructure complete (`DecoratorRegistry`, wrappers)
- Phase 5 integration tests: 41/41 passing
  - Class, method, field, parameter decorators work
  - Decorator factories work (including chained `@Factory("a")("b")`)
  - Framework patterns (HTTP routing, DI, validation, ORM) work
  - Inheritance with decorators works

**Milestone 4.1 (Core Types):** Complete
- Date: All 20 handlers implemented (getters, setters, formatting, parse)
- Object: hashCode, equals, toString handlers implemented
- Task: isDone, isCancelled handlers implemented (via `self.tasks` RwLock)
- Error: ERROR_STACK handler implemented (placeholder — needs stack capture at throw time)
- Number: toFixed, toPrecision, toString(radix) — native IDs, compiler lowering, VM handlers all complete
- String-RegExp bridge verified, RegExp.replaceWith implemented with callback support
- 34 new e2e tests added

**Milestone 4.2 (std:logger):** Complete
- ✅ Phase 1: Infrastructure complete
  - `export default` support (parser, binder, typedef, visitor)
  - `std:` module registry with source embedding
  - `NativeHandler` trait for stdlib decoupling
  - `raya-runtime` crate binds engine + stdlib
  - Logger.raya with debug/info/warn/error methods
- ✅ Phase 2: Console global removed
  - design/STDLIB.md updated (console → logger reference)
  - Reflect bootstrap: CONSOLE_LOG → LOGGER_INFO
  - CLAUDE.md files updated (stdlib, VM)
- ✅ Phase 3: Documentation migration complete
  - 36 files migrated: all design/*.md, plans/*.md, crate files, root files
  - console.log → logger.info (87 in LANG.md, 16 in MAPPING.md, 18 in REFLECTION.md)
  - console.error → logger.error, console.warn → logger.warn
  - Zero remaining console.* references (verified via grep)
- ✅ Phase 4: E2E tests moved to raya-runtime
  - All 594 e2e tests moved from raya-engine to raya-runtime
  - Tests run with StdNativeHandler integration
- Tests: 827 engine lib, 596 runtime (594 e2e + 2 unit), 4 stdlib (1,427 total)

**Milestone 4.3 (std:math):** Not Started
- Default export: `import math from "std:math"` → `math.abs()`, `math.floor()`, `math.PI`, etc.
- 20 methods + 2 constants, native IDs, type checker, compiler lowering, VM handlers

**Tests:** 1,427 total (827 engine lib, 596 runtime, 4 stdlib)

See [plans/milestone-3.8.md](plans/milestone-3.8.md), [plans/milestone-3.9.md](plans/milestone-3.9.md), [plans/milestone-4.1.md](plans/milestone-4.1.md), [plans/milestone-4.2.md](plans/milestone-4.2.md), and [plans/milestone-4.3.md](plans/milestone-4.3.md) for details.

---

## Critical Design Rules

### Type System
- `typeof` for primitive unions (`string | number | boolean | null`)
- `instanceof` for class type checking
- Discriminated unions for complex types (required discriminant field)
- **BANNED:** `any` type, runtime type tags/RTTI

### Concurrency
- `async` functions create Tasks (green threads), start immediately
- `await` suspends current Task (doesn't block OS thread)
- Work-stealing scheduler across CPU cores

### Compilation
- Monomorphization (generics specialized at compile time)
- Typed opcodes: `IADD` (int), `FADD` (float), `NADD` (number)
- No runtime type checking overhead

---

## Key Documents

| Document | Purpose |
|----------|---------|
| [design/LANG.md](design/LANG.md) | Language specification |
| [design/ARCHITECTURE.md](design/ARCHITECTURE.md) | VM architecture |
| [design/OPCODE.md](design/OPCODE.md) | Bytecode instructions |
| [design/MAPPING.md](design/MAPPING.md) | Compilation patterns |
| [plans/milestone-3.7.md](plans/milestone-3.7.md) | Module system |
| [plans/milestone-3.8.md](plans/milestone-3.8.md) | Reflection API |
| [plans/milestone-3.9.md](plans/milestone-3.9.md) | Decorators (uses Reflection) |
| [plans/milestone-4.1.md](plans/milestone-4.1.md) | Core Types (built-in functions) |
| [plans/milestone-4.2.md](plans/milestone-4.2.md) | std:logger (replaces logger.info) |
| [plans/milestone-4.3.md](plans/milestone-4.3.md) | std:math (20 methods + PI, E) |

---

## Project Structure

```
crates/
├── raya-engine/     # Parser, compiler, VM (main crate)
├── raya-runtime/    # Binds engine + stdlib via NativeHandler trait
├── raya-stdlib/     # Native stdlib implementations (logger, etc.)
├── raya-cli/        # CLI tool
├── raya-pm/         # Package manager (rpkg)
├── raya-sdk/        # Native module FFI types
└── raya-native/     # Proc-macros for native modules

design/              # Specifications
plans/               # Implementation roadmap
```

Each crate has its own `CLAUDE.md` with module-specific details.

---

## Build & Test

```bash
cargo build                    # Build all
cargo test                     # Run all tests
cargo test -p raya-engine      # Engine tests only
cargo test -p rpkg             # Package manager tests
```

---

## Quick Reference

| Question | Answer |
|----------|--------|
| Use `typeof`? | ✅ For primitive unions only |
| Use `instanceof`? | ✅ For class type checking |
| Runtime type checks? | ❌ Never (compile-time only) |
| Generic erasure? | ❌ No, use monomorphization |
| Concurrency model? | Goroutine-style Tasks |
| Implementation language? | Rust (stable) |
