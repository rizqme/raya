# Raya Programming Language

[![CI](https://github.com/rizqme/raya/workflows/CI/badge.svg)](https://github.com/rizqme/raya/actions)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

**Raya** is a statically-typed programming language with TypeScript syntax, featuring a custom virtual machine with goroutine-style concurrency and a fully static type system with zero runtime type checks.

## 🚀 Features

- **Fully Static Type System** - All types verified at compile time, no runtime type checks
- **Goroutine-Style Concurrency** - Lightweight green threads (Tasks) with automatic CPU core utilization
- **VM Snapshotting** - Pause, serialize, and resume entire VM state for migration and debugging
- **Inner VMs** - Nested, isolated VMs with resource limits and capability-based security
- **TypeScript Syntax** - Familiar syntax for millions of developers
- **Monomorphization** - Generic code specialized per concrete type (like Rust/C++)
- **Discriminated Unions** - Type-safe sum types with exhaustiveness checking
- **Sound Type System** - No `any`, no `typeof`, no escape hatches
- **Predictable Runtime** - Clean object model, no prototype chains

## 📚 Documentation

- [Language Specification](design/LANG.md) - Complete language reference
- [VM Architecture](design/ARCHITECTURE.md) - Virtual machine design
- [Bytecode Reference](design/OPCODE.md) - Instruction set documentation
- [VM Snapshotting](design/SNAPSHOTTING.md) - Pause, snapshot, and resume design
- [Inner VMs](design/INNER_VM.md) - Nested VMs with isolation and control
- [File Formats](design/FORMATS.md) - .raya, .rbc, .rlib specifications
- [CLI Design](design/CLI.md) - Unified command-line interface
- [Implementation Plan](plans/PLAN.md) - Development roadmap
- [AI Assistant Guide](CLAUDE.md) - Guide for AI-assisted development

## 🏗️ Project Structure

```
rayavm/
├── crates/
│   ├── raya-core/        # VM runtime (interpreter, GC, scheduler)
│   ├── raya-bytecode/    # Bytecode definitions
│   ├── raya-parser/      # Lexer & Parser
│   ├── raya-types/       # Type system & checker
│   ├── raya-compiler/    # Code generation
│   ├── raya-stdlib/      # Standard library
│   ├── raya-cli/         # Unified CLI tool (raya)
│   └── raya-pm/             # Package manager (legacy)
├── design/                 # Specification documents
└── plans/                  # Implementation roadmap
```

## 📦 File Extensions

Raya uses specific extensions for different artifact types:

- `.raya` - Source code (TypeScript syntax)
- `.rbc` - Compiled bytecode modules
- `.rlib` - Library archives (packages)
- Executables - Standalone bundles with embedded runtime

See [FORMATS.md](design/FORMATS.md) for detailed specifications.

## 🔧 Building

Requires Rust 1.70 or later.

```bash
# Build all crates
cargo build --workspace

# Run tests
cargo test --workspace

# Build release binary
cargo build --release -p raya-cli

# Run benchmarks
cargo bench --workspace

# Install CLI tool
make install
# or: cargo install --path crates/raya-cli
```

## 🎯 Quick Example

```typescript
// main.raya - Raya source file
import { match } from "raya:std";

type Result<T> =
  | { status: "ok"; value: T }
  | { status: "error"; error: string };

async function fetchData(): Task<Result<number>> {
  // async functions always create a Task
  return { status: "ok", value: 42 };
}

function main(): void {
  const task = fetchData();  // Task starts immediately
  const result = await task;  // Suspend until complete

  match(result, {
    ok: (r) => console.log("Success:", r.value),
    error: (r) => console.log("Error:", r.error)
  });
}
```

```bash
# Run directly (compiles to .rbc internally)
raya run main.raya

# Build to bytecode
raya build main.raya
# Creates: dist/main.rbc

# Create standalone executable
raya bundle main.raya -o myapp
# Creates: myapp (with embedded runtime)
```

## 🚦 Project Status

**Current Phase:** Phase 1 - VM Core Implementation

### Completed Milestones ✅
- **1.1**: Project Setup - Rust workspace, dependencies, CI/CD
- **1.2**: Bytecode Definitions - Complete opcode set and module format
- **1.9**: Safepoint Infrastructure - Stop-the-world coordination for GC/snapshots
- **1.10**: Task Scheduler - Goroutine-style work-stealing concurrency
  - ✅ Multi-threaded work-stealing scheduler (crossbeam-deque)
  - ✅ Go-style asynchronous preemption (10ms threshold)
  - ✅ SPAWN/AWAIT opcodes in both VM and worker threads
  - ✅ Nested task spawning support
  - ✅ Resource limits for inner VMs (SchedulerLimits)
  - ✅ 22 comprehensive integration tests

- **1.11**: VM Snapshotting - Stop-the-world pause & resume
  - ✅ Binary snapshot format with SHA-256 checksums
  - ✅ Task state serialization (IP, stack, frames, blocked reasons)
  - ✅ Heap snapshot infrastructure
  - ✅ Snapshot writer/reader with validation
  - ✅ 14 comprehensive integration tests
  - ✅ Value encode/decode for all types

- **1.12**: Synchronization Primitives (Mutex) - Task-aware synchronization
  - ✅ Enhanced Mutex with FIFO wait queue and owner tracking
  - ✅ MutexId and MutexRegistry for global management
  - ✅ Scheduler integration (block_on_mutex, resume_from_mutex)
  - ✅ Mutex serialization for snapshots
  - ✅ MutexGuard with RAII pattern (auto-unlock on drop)
  - ✅ Comprehensive unit and integration tests
  - ✅ Task-level blocking without OS thread blocking

### In Progress ⏳
- **1.3-1.8**: Core VM components (stack, frames, basic execution)

### Pending 📋
- Parser & type checker
- Compiler (bytecode generation)
- Standard library
- CLI tools

See [PLAN.md](plans/PLAN.md) for detailed milestones and [ARCHITECTURE.md](design/ARCHITECTURE.md) for VM design.

## 🤝 Contributing

Contributions are welcome! Please read the design documents first:

1. [design/LANG.md](design/LANG.md) - Understand language semantics
2. [design/ARCHITECTURE.md](design/ARCHITECTURE.md) - VM design
3. [CLAUDE.md](CLAUDE.md) - Development guidelines

## 📖 Design Principles

| Principle | Implementation |
|-----------|----------------|
| **Explicit over implicit** | Discriminated unions, type annotations |
| **Safety over convenience** | No escape hatches, sound type system |
| **Performance through types** | Static types enable optimization |
| **Familiar syntax** | TypeScript-compatible where possible |
| **Predictable semantics** | Well-defined execution model |

## 🔑 Key Differences from TypeScript

- ❌ No `typeof` or `instanceof` - use discriminated unions
- ❌ No `any` type - fully sound type system
- ❌ No type assertions - all types verified
- ✅ Monomorphization for generics (not type erasure)
- ✅ Multi-threaded task scheduler (not single-threaded)
- ✅ Custom VM with typed bytecode (not JavaScript)

## 📄 License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

## 🌟 Inspiration

Raya draws inspiration from:
- **TypeScript** - Syntax and developer experience
- **Go** - Goroutine-style concurrency model
- **Rust** - Sound type system and monomorphization
- **Wasm** - Bytecode design principles

---

**Raya: A strict, concurrent TypeScript subset with a predictable runtime.**
