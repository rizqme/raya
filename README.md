# Raya Programming Language

[![CI](https://github.com/rizqme/raya/workflows/CI/badge.svg)](https://github.com/rizqme/raya/actions)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

Raya is a **statically-typed language with TypeScript syntax** and a custom VM built for **predictable performance** and **goroutine-style concurrency**.

If you like TypeScript ergonomics but want stronger compile-time guarantees and a tighter runtime model, Raya is built for that.

**[Documentation](https://raya.land)**

---

## Why software engineers care

- **Runtime for TypeScript-like syntax, without JavaScript legacy baggage**  
  Keep familiar developer ergonomics while avoiding historical JS runtime quirks.
- **TypeScript-like syntax, stricter semantics**  
  Familiar syntax with no `any`, no runtime type assertions, and no hidden coercion.
- **Concurrency as a first-class model**  
  Lightweight `Task`s, immediate start with `async`, cooperative `await`, work-stealing scheduler.
- **Compile-time specialization**  
  Monomorphization and typed bytecode (`IADD`, `FADD`, etc.) for predictable execution paths.
- **No runtime type checks in hot code**  
  Type safety is enforced at compile time.

---

## Quick example

```typescript
import logger from "std:logger";
import time from "std:time";

type Result<T> =
  | { status: "ok"; value: T }
  | { status: "error"; error: string };

async function fetchData(): Task<Result<number>> {
  return { status: "ok", value: 42 };
}

function main(): void {
  const start = time.monotonic();

  const task = fetchData();     // Starts immediately
  const result = await task;    // Suspends current task

  if (result.status == "ok") {
    logger.info("value:", result.value);
  } else {
    logger.error("error:", result.error);
  }

  logger.info("elapsed(ns):", time.elapsed(start));
}
```

---

## 60-second local run

```bash
curl -fsSL https://raya.land/install.sh | sh
raya --help
raya check examples/hello.raya
raya eval "1 + 2 * 3"
```

---

## Architecture at a glance

```text
source (.raya)
  -> parser + type checker
  -> typed IR
  -> typed bytecode
  -> VM interpreter (and optional JIT/AOT paths)
```

Project layout:

```text
crates/
├── raya-engine/        # Parser, type checker, compiler, VM core
├── raya-runtime/       # Runtime API + stdlib integration + e2e tests
├── raya-stdlib/        # Cross-platform modules (logger, math, crypto, time, ...)
├── raya-stdlib-posix/  # POSIX modules (fs, net, http, fetch, process, ...)
├── raya-cli/           # CLI binary (raya)
├── raya-pm/            # Package manager primitives
├── raya-sdk/           # Native module FFI surface
└── raya-native/        # Proc-macros for native modules
```

---

## Standard library highlights

- **Core**: `std:logger`, `std:math`, `std:crypto`, `std:time`, `std:path`, `std:stream`, `std:runtime`, `std:reflect`
- **POSIX**: `std:fs`, `std:net`, `std:http`, `std:fetch`, `std:env`, `std:process`, `std:os`, `std:io`

> Note: stdlib I/O calls are synchronous by design; concurrency comes from running calls in `Task`s.

---

## Current maturity

Raya is active and moving quickly.

- Core language, VM, and CLI are implemented and tested.
- Standard library is broad and still expanding.
- JIT/AOT paths exist and are feature-gated.
- API and behavior may still evolve as milestones complete.

If you’re evaluating for production, track changes closely and pin versions.

---

## Build & test

Requires Rust stable.

```bash
cargo build --workspace
cargo test --workspace
cargo build --release -p raya-cli
```

---

## Design principles

- **Explicit over implicit**
- **Safety over convenience**
- **Performance through static types**
- **Familiar syntax, predictable runtime semantics**

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
