# Raya Programming Language

[![CI](https://github.com/rizqme/raya/workflows/CI/badge.svg)](https://github.com/rizqme/raya/actions)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

**Raya** is a statically-typed programming language with TypeScript syntax, featuring a custom bytecode VM with goroutine-style concurrency and a fully static type system with zero runtime type checks.

**[Documentation](https://rizqme.github.io/raya/)** | **[Language Spec](https://rizqme.github.io/raya/language/lang)** | **[VM Architecture](https://rizqme.github.io/raya/runtime/architecture)**

## Features

- **Fully Static Type System** -- All types verified at compile time, zero runtime type checks
- **Goroutine-Style Concurrency** -- Lightweight green threads (Tasks) with work-stealing scheduler
- **TypeScript Syntax** -- Familiar syntax; every valid Raya program is valid TypeScript
- **Monomorphization** -- Generics specialized at compile time (like Rust/C++)
- **Typed Bytecode** -- Type-aware instructions (IADD, FADD, NADD) for unboxed operations
- **Discriminated Unions** -- Type-safe sum types with exhaustiveness checking
- **Sound Type System** -- No `any`, no type assertions, no escape hatches
- **Standard Library** -- 8 modules: logger, math, crypto, time, path, codec, runtime, reflect

## Quick Example

```typescript
import Logger from "std:logger";
import { Time } from "std:time";

type Result<T> =
  | { status: "ok"; value: T }
  | { status: "error"; error: string };

async function fetchData(): Task<Result<number>> {
  return { status: "ok", value: 42 };
}

function main(): void {
  const start = Time.monotonic();
  const task = fetchData();  // Task starts immediately
  const result = await task; // Suspend until complete

  if (result.status == "ok") {
    Logger.info("Success:", result.value);
  } else {
    Logger.error("Error:", result.error);
  }

  Logger.info("Elapsed:", Time.elapsed(start), "ns");
}
```

## Project Structure

```
crates/
├── raya-engine/     # Parser, compiler, VM (932 tests)
├── raya-runtime/    # Binds engine + stdlib (883 tests)
├── raya-stdlib/     # Native stdlib implementations (17 tests)
├── raya-cli/        # CLI tool (raya)
├── raya-pm/         # Package manager (rpkg)
├── raya-sdk/        # Native module FFI types
└── raya-native/     # Proc-macros for native modules
```

## Building

Requires Rust stable (1.70+).

```bash
cargo build --workspace            # Build all
cargo test --workspace             # Run all 1,832 tests
cargo build --release -p raya-cli  # Release binary
```

## Documentation

Full design documentation is available at **[rizqme.github.io/raya/](https://rizqme.github.io/raya/)**:

- [Language Specification](https://rizqme.github.io/raya/language/lang) -- Complete language reference
- [VM Architecture](https://rizqme.github.io/raya/runtime/architecture) -- Execution model
- [Bytecode Reference](https://rizqme.github.io/raya/compiler/opcode) -- Instruction set
- [Standard Library](https://rizqme.github.io/raya/stdlib/stdlib) -- All module APIs
- [Reflection API](https://rizqme.github.io/raya/metaprogramming/reflection) -- 149+ handlers
- [CLI Design](https://rizqme.github.io/raya/tooling/cli) -- Command-line interface

## Design Principles

| Principle | Implementation |
|-----------|----------------|
| **Explicit over implicit** | Discriminated unions, type annotations |
| **Safety over convenience** | No escape hatches, sound type system |
| **Performance through types** | Static types enable typed opcodes, monomorphization |
| **Familiar syntax** | TypeScript-compatible where possible |
| **Predictable semantics** | Well-defined execution model, no prototype chains |

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
