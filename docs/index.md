---
layout: home

hero:
  text: <span class="raya-name">raya</span> is a programming language with TypeScript syntax and goroutine-style concurrency
  tagline: Compile-time type safety. Lightweight concurrency. Familiar syntax.
  actions:
    - theme: brand
      text: Get Started
      link: /getting-started
    - theme: alt
      text: Quick Install
      link: /getting-started
    - theme: alt
      text: View on GitHub
      link: https://github.com/rizqme/raya

features:
  - icon: 
      src: /icons/zap.svg
    title: Goroutine-Style Concurrency
    details: Lightweight Tasks with work-stealing scheduler. Write async code that looks synchronous. Start thousands of concurrent tasks without OS thread overhead.
    
  - icon:
      src: /icons/target.svg
    title: Fully Static Type System
    details: All types verified at compile time. Zero runtime type checks. No 'any', no escape hatches. Sound type system that catches bugs before they run.
    
  - icon:
      src: /icons/code.svg
    title: TypeScript Syntax
    details: Familiar syntax that TypeScript developers already know. Write code that feels like TypeScript, compiles like Rust.
    
  - icon:
      src: /icons/cpu.svg
    title: Typed Bytecode
    details: Type-aware instructions (IADD, FADD) enable unboxed operations. Monomorphization at compile time like Rust/C++.
    
  - icon:
      src: /icons/link.svg
    title: Discriminated Unions
    details: Type-safe sum types with exhaustiveness checking. Pattern match on discriminant fields for compile-time guarantees.
    
  - icon:
      src: /icons/package.svg
    title: Batteries Included
    details: Core modules (logger, math, crypto, time, path) and system modules (fs, net, http, process). Everything you need to build real applications.
---

## Quick Example

```typescript
import io from "std:io";

// Async functions create lightweight Tasks
async function fetchUser(id: number): Task<string> {
  return `User ${id}`;
}

function main(): void {
  // Tasks start immediately - no explicit spawn
  const tasks = [fetchUser(1), fetchUser(2), fetchUser(3)];
  
  // Await array of tasks - returns array of results
  const users = await tasks;
  
  for (const user of users) {
    io.writeln(user);
  }
}
```

## Why Raya?

<div class="tip custom-block">
<p class="custom-block-title">Early Project</p>

Raya is in active development. APIs may change. Not ready for production. But if you're curious about:
- **TypeScript syntax** without JavaScript baggage
- **Goroutine-style concurrency** without Go's GC pauses  
- **Static types** that enable real optimizations
- **Lower overhead** than dynamically typed languages

...then Raya might be worth watching.
</div>

### Design Principles

| What | How |
|------|-----|
| **Explicit over implicit** | Discriminated unions, type annotations required |
| **Safety over convenience** | No escape hatches, sound type system |
| **Performance through types** | Static types → typed opcodes → unboxed operations |
| **Familiar syntax** | TypeScript-compatible where it makes sense |
| **Predictable semantics** | No prototype chains, no coercion magic |

### Concurrency Model

```typescript
// Synchronous I/O becomes concurrent with async prefix
import fs from "std:fs";

const task1 = async fs.readTextFile("a.txt");  // Starts immediately
const task2 = async fs.readTextFile("b.txt");  // Runs in parallel
const a = await task1;  // Suspend until ready
const b = await task2;
```

- **Tasks** are green threads (like goroutines)
- **async** creates a Task, starts it immediately
- **await** suspends current Task (doesn't block OS thread)
- **Work-stealing scheduler** across CPU cores
- **Nursery allocator** per Task reduces GC contention

### Type System

```typescript
// Discriminated unions with exhaustiveness checking
type State =
  | { kind: "loading" }
  | { kind: "success"; data: string }
  | { kind: "error"; message: string };

function handle(state: State): void {
  if (state.kind == "loading") {
    logger.info("Loading...");
  } else if (state.kind == "success") {
    logger.info(state.data);  // Compiler knows 'data' exists
  } else {
    logger.error(state.message);  // Compiler knows 'message' exists
  }
}
```

- **No `any` type** - all values have known types
- **No runtime type checks** - types erased after compilation  
- **Monomorphization** - generics specialized at compile time
- **Typed opcodes** - IADD for int, FADD for float

## Standard Library

### Core Modules
- **std:logger** - debug, info, warn, error
- **std:math** - 22 functions + PI, E
- **std:crypto** - hashing, HMAC, random, encoding  
- **std:time** - clocks, sleep, durations
- **std:path** - path manipulation
- **std:runtime** - VM, compiler, reflection

### System Modules (POSIX)
- **std:fs** - file I/O, directories
- **std:net** - TCP/UDP sockets
- **std:http** - HTTP/1.1 server
- **std:fetch** - HTTP client
- **std:process** - process management

All I/O is synchronous. Concurrency achieved with Tasks at call site.

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

## Project Status

**What works:**
- ✅ Parser, type checker, compiler
- ✅ Bytecode VM with GC
- ✅ Goroutine-style concurrency (Tasks + scheduler)
- ✅ Reflection API (149+ handlers)
- ✅ Decorators (@class, @method, @field, @parameter)
- ✅ Standard library (14 modules)
- ✅ CLI (run, build, eval, check, repl, pkg)
- ✅ Package manager (raya-pm)

**What's coming:**
- 🚧 JIT compilation (feature-gated, experimental)
- 🚧 AOT compilation (feature-gated, experimental)
- 🚧 LSP server (WIP)
- 🚧 More stdlib modules

**Tests:** 4,121+ tests passing (engine, runtime, stdlib, CLI, PM)

## License

MIT OR Apache-2.0
