---
title: Overview
---

# Raya Design Documentation

> **Raya** is a statically-typed language with TypeScript syntax, compiled to custom bytecode and executed on a multi-threaded VM with goroutine-style concurrency. Fully static type system with zero runtime type checks. 1,832 tests passing.

---

## Implementation Status

| Document | Status | Category |
|----------|--------|----------|
| [Language Spec](./language/lang.md) | Implemented | Language |
| [Numeric Types](./language/numeric-types.md) | Implemented | Language |
| [typeof Design](./language/typeof-design.md) | Implemented | Language |
| [JSON Type](./language/json-type.md) | Implemented | Language |
| [Exception Handling](./language/exception-handling.md) | Implemented | Language |
| [Opcodes](./compiler/opcode.md) | Implemented | Compiler |
| [Compilation Mapping](./compiler/mapping.md) | Implemented | Compiler |
| [File Formats](./compiler/formats.md) | Implemented | Compiler |
| [VM Architecture](./runtime/architecture.md) | Implemented | Runtime |
| [Module System](./runtime/modules.md) | Implemented | Runtime |
| [Built-in Classes](./runtime/builtin-classes.md) | Implemented | Runtime |
| [Standard Library](./stdlib/stdlib.md) | Implemented (8 modules) | Stdlib |
| [Reflection API](./metaprogramming/reflection.md) | Implemented (149+ handlers) | Metaprogramming |
| [Reflect Security](./metaprogramming/reflect-security.md) | Implemented | Metaprogramming |
| [Decorators](./metaprogramming/decorators.md) | Implemented (41 e2e tests) | Metaprogramming |
| [Native ABI](./native/abi.md) | In Progress (M4.9) | Native |
| [Native Bindings](./native/native-bindings.md) | Partially Implemented | Native |
| [Inner VM](./advanced/inner-vm.md) | Partially Implemented (M4.5) | Advanced |
| [Dynamic VM Bootstrap](./advanced/dynamic-vm-bootstrap.md) | Implemented (Reflect Phase 17) | Advanced |
| [Channels](./future/channels.md) | Future | Future |
| [VM Snapshotting](./future/snapshotting.md) | Future | Future |
| [TSX/JSX](./future/tsx.md) | Future | Future |
| [CLI](./tooling/cli.md) | Designed (Scaffolded) | Tooling |

---

## Learning Paths

### New to Raya?

1. [Language Specification](./language/lang.md) -- syntax, types, concurrency model
2. [VM Architecture](./runtime/architecture.md) -- how code executes
3. [Bytecode Reference](./compiler/opcode.md) -- instruction set
4. [Compilation Patterns](./compiler/mapping.md) -- how source becomes bytecode

### Working on the compiler?

1. [Language Spec](./language/lang.md) -- what to compile
2. [Numeric Types](./language/numeric-types.md) -- int/float/number semantics
3. [Opcode Reference](./compiler/opcode.md) -- target instruction set
4. [Compilation Mapping](./compiler/mapping.md) -- translation patterns
5. [File Formats](./compiler/formats.md) -- .raya and .ryb formats

### Working on the VM/runtime?

1. [Architecture](./runtime/architecture.md) -- task scheduler, GC, execution model
2. [Modules](./runtime/modules.md) -- module resolution and loading
3. [Built-in Classes](./runtime/builtin-classes.md) -- Object, Date, Task, Error, etc.
4. [Exception Handling](./language/exception-handling.md) -- try/catch/finally implementation

### Working on the standard library?

1. [Stdlib API Reference](./stdlib/stdlib.md) -- all module APIs
2. [Native ABI](./native/abi.md) -- how stdlib talks to the VM
3. [Native Bindings](./native/native-bindings.md) -- external module system

### Working on metaprogramming?

1. [Reflection API](./metaprogramming/reflection.md) -- 149+ reflection handlers
2. [Reflection Security](./metaprogramming/reflect-security.md) -- permission model
3. [Decorators](./metaprogramming/decorators.md) -- @decorator system
4. [Dynamic Bootstrap](./advanced/dynamic-vm-bootstrap.md) -- runtime code generation

---

## Design Principles

| Principle | Implementation |
|-----------|----------------|
| **Explicit over implicit** | Discriminated unions, type annotations |
| **Safety over convenience** | No escape hatches, sound type system |
| **Performance through types** | Static types enable typed opcodes, monomorphization |
| **Familiar syntax** | TypeScript-compatible where possible |
| **Predictable semantics** | Well-defined execution model |

### Key Design Decisions

- **Fully static type system** -- All types verified at compile time. No `any`, no `instanceof`, no runtime type tags
- **Discriminated unions** -- Required for complex union types; `typeof` for bare primitive unions
- **Monomorphization** -- Generics specialized at compile time (no type erasure)
- **Typed opcodes** -- `IADD` (int), `FADD` (float), `NADD` (number) based on static types
- **Goroutine-style concurrency** -- `async` functions create Tasks (green threads), work-stealing scheduler
- **Single bytecode executor** -- `Interpreter::run()` with frame-based calls, no nested execution

### What's Banned

- `any` type
- Runtime type tags / RTTI
- Type assertions (`as`)
- Type casts
- Non-null assertions (`!`)

---

## Project Structure

```
crates/
├── raya-engine/     # Parser, compiler, VM (932 tests)
├── raya-runtime/    # Binds engine + stdlib via NativeHandler trait (883 tests)
├── raya-stdlib/     # Native stdlib implementations (17 tests)
├── raya-cli/        # CLI tool
├── raya-pm/         # Package manager
├── raya-sdk/        # Native module FFI types
└── raya-native/     # Proc-macros for native modules
```

**Total: 1,832 tests** (932 engine + 883 runtime + 17 stdlib)

---

## Standard Library Modules

| Module | Import | Methods | Milestone |
|--------|--------|---------|-----------|
| Logger | `import Logger from "std:logger"` | debug, info, warn, error | M4.2 |
| Math | `import { Math } from "std:math"` | 22 functions + PI, E | M4.3 |
| Crypto | `import { Crypto } from "std:crypto"` | 12 methods | M4.6 |
| Time | `import { Time } from "std:time"` | 12 methods | M4.7 |
| Path | `import { Path } from "std:path"` | 14 methods | M4.8 |
| Runtime | `import { Compiler, Vm, ... } from "std:runtime"` | 9 exports | M4.5 |
| Reflect | `import * as Reflect from "std:reflect"` | 149+ handlers | M3.8 |

See [stdlib/stdlib.md](./stdlib/stdlib.md) for full API signatures.
