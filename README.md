<div align="center">
  <img src="raya-logo.svg" alt="Raya Logo" width="200"/>

  <h1>Raya Programming Language</h1>

  [![CI](https://github.com/rizqme/raya/actions/workflows/ci.yml/badge.svg)](https://github.com/rizqme/raya/actions/workflows/ci.yml)
  [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

  <p>
    A <strong>statically typed language with TypeScript syntax</strong> powered by a 
    <strong>modern VM</strong> designed for <strong>predictable performance</strong> 
    and <strong>native multi-core concurrency</strong>.
  </p>
  <div>📚 Documentation: [https://raya.land](https://raya.land)</div>
</div>

---

# Raya Programming Language

Raya is a **statically typed language with TypeScript syntax** powered by a **custom virtual machine** built for modern hardware.

TypeScript today compiles to JavaScript — meaning your types are erased and your runtime behavior is still governed by JavaScript semantics and its single-threaded event loop model.

Raya keeps the **ergonomics and expressiveness of TypeScript**, while replacing the runtime entirely.

---

## Why Raya Exists

Raya is built for engineers who:

- Like **TypeScript’s syntax and developer experience**
- Want **stronger compile-time guarantees**
- Need **predictable runtime behavior**
- Care about **real concurrency on multi-core systems**
- Prefer a **clean execution model without JavaScript legacy constraints**

---

## Core Ideas

### **TypeScript Ergonomics — Without JavaScript**

Raya uses familiar TypeScript-like syntax, but:

- No JavaScript compatibility layer
- No `any`
- No implicit coercions
- No runtime type assertions in hot paths

Types are **real semantic constructs**, not erased hints.

---

### **Concurrency as a First-Class Runtime Feature**

Async functions create **lightweight Tasks**:

- Tasks start immediately
- Minimal scheduling overhead
- Cooperative `await`
- Work-stealing scheduler
- Designed for multi-core execution

Concurrency is a **runtime primitive**, not an afterthought.

---

### **Static Types That Shape Execution**

Raya compiles into:

- Typed IR
- Typed bytecode (`IADD`, `FADD`, etc.)

This enables:

- Predictable execution paths
- No dynamic type checks in hot code
- Better optimization opportunities (JIT / AOT)

---

## Installation

```bash
curl -fsSL https://raya.land/install.sh | sh
```

**Build from source:**

```bash
git clone https://github.com/rizqme/raya.git
cd raya
cargo build --release -p raya-cli
```

---

## Quick Example

```ts
import io from "std:io";

// Async functions create lightweight Tasks
async function fetchUser(id: number): Task<string> {
  return `User ${id}`;
}

async function main(): Task<void> {
  // Tasks start immediately
  const tasks = [fetchUser(1), fetchUser(2), fetchUser(3)];

  // Await multiple tasks
  const users = await tasks;

  for (const user of users) {
    io.writeln(user);
  }
}
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

Runtime concurrency uses a **reactor model**:

- **I/O threads** run the reactor/event loop for polling and wakeups
- **Task worker threads** execute scheduled language tasks (work-stealing)

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

## Design principles

- **Explicit over implicit**
- **Safety over convenience**
- **Performance through static types**
- **Familiar syntax, predictable runtime semantics**

---

## Author & License

Author: Ahmad Rizqi Meydiarso
MIT License ([LICENSE](LICENSE))
