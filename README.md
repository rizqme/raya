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

## Quick Example

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
  const task = fetchData();  // Task starts immediately
  const result = await task; // Suspend until complete

  if (result.status == "ok") {
    logger.info("Success:", result.value);
  } else {
    logger.error("Error:", result.error);
  }

  logger.info("Elapsed:", time.elapsed(start), "ns");
}
```

## Standard Library

### Core Modules (`raya-stdlib`)

| Module | Import | Description |
|--------|--------|-------------|
| Logger | `import logger from "std:logger"` | Structured logging (debug, info, warn, error) |
| Math | `import math from "std:math"` | 22 math functions + PI, E constants |
| Crypto | `import crypto from "std:crypto"` | Hashing, HMAC, random, encoding (12 methods) |
| Time | `import time from "std:time"` | Clocks, sleep, duration conversion (12 methods) |
| Path | `import path from "std:path"` | Path manipulation (14 methods) |
| Stream | `import stream from "std:stream"` | Reactive streams (forward, collect, send, receive) |
| Runtime | `import { Compiler, Vm } from "std:runtime"` | Compilation, VM isolation, permissions |
| Reflect | `import * as Reflect from "std:reflect"` | Reflection API (149+ handlers) |

### System Modules (`raya-stdlib-posix`)

| Module | Import | Description |
|--------|--------|-------------|
| Fs | `import fs from "std:fs"` | File I/O, directory ops, stat, symlinks |
| Net | `import { TcpListener, TcpStream } from "std:net"` | TCP/UDP sockets |
| HTTP | `import { HttpServer } from "std:http"` | HTTP/1.1 server |
| Fetch | `import fetch from "std:fetch"` | HTTP client |
| Env | `import env from "std:env"` | Environment variables |
| Process | `import process from "std:process"` | Process management, exec |
| OS | `import os from "std:os"` | Platform info (arch, cpus, memory) |
| IO | `import io from "std:io"` | stdin/stdout/stderr |

All I/O is synchronous. Async is achieved at the call site with goroutines:

```typescript
import fs from "std:fs";

// Synchronous read
const data = fs.readTextFile("config.json");

// Concurrent reads via goroutines
const t1 = async fs.readTextFile("a.txt");
const t2 = async fs.readTextFile("b.txt");
const a = await t1;
const b = await t2;
```

## Project Structure

```
crates/
├── raya-engine/        # Parser, compiler, VM
├── raya-runtime/       # Binds engine + stdlib (e2e tests)
├── raya-stdlib/        # Core stdlib (logger, math, crypto, time, path, stream)
├── raya-stdlib-posix/  # System stdlib (fs, net, http, fetch, env, process, os, io)
├── raya-cli/           # CLI tool (raya)
├── raya-pm/            # Package manager (rpkg)
├── raya-sdk/           # Native module FFI types
└── raya-native/        # Proc-macros for native modules
```

## Building

Requires Rust stable (1.70+).

```bash
cargo build --workspace            # Build all
cargo test --workspace             # Run all tests
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
