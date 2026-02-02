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

**Complete:** Milestones 3.4-3.6, Milestone 3.7 Phases 2-6 (module system)
**In Progress:**
- Milestone 3.7 Phase 1 (JSON intrinsics)
- Milestone 3.8 Phases 1-9 complete (Reflect API with proxy support)
**Tests:** 720+ passing (raya-engine)

See [plans/milestone-3.7.md](plans/milestone-3.7.md) and [plans/milestone-3.8.md](plans/milestone-3.8.md) for details.

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
| [plans/milestone-3.7.md](plans/milestone-3.7.md) | Current milestone |

---

## Project Structure

```
crates/
├── raya-engine/     # Parser, compiler, VM (main crate)
├── raya-cli/        # CLI tool
├── raya-pm/         # Package manager (rpkg)
├── raya-sdk/        # Native module FFI types
├── raya-native/     # Proc-macros for native modules
└── raya-stdlib/     # Native stdlib implementations

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
