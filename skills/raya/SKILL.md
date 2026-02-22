---
slug: raya
name: Raya Language Expert
description: Comprehensive knowledge of the Raya programming language, its architecture, and development practices
version: 1.0.0
tags:
  - programming-language
  - compiler
  - runtime
  - typescript-syntax
  - concurrency
  - static-typing
---

# Raya Language Expert

A comprehensive knowledge base for **Raya** — a statically-typed programming language with TypeScript syntax, featuring a custom bytecode VM with goroutine-style concurrency and zero runtime type checks.

## What is Raya?

Raya is a modern programming language that combines:
- **TypeScript-compatible syntax** for familiarity
- **Fully static type system** with zero runtime type checks
- **Goroutine-style concurrency** with lightweight green threads
- **Custom bytecode VM** with typed opcodes
- **Monomorphization** for generics (like Rust/C++)
- **Sound type system** with no escape hatches

## Quick Facts

| Aspect | Detail |
|--------|--------|
| **Implementation** | Rust (stable 1.70+) |
| **Type System** | Fully static, sound, no `any` type |
| **Concurrency** | Goroutine-style Tasks with work-stealing scheduler |
| **Generics** | Monomorphization (compile-time specialization) |
| **I/O Model** | Synchronous APIs + goroutines for concurrency |
| **Tests** | 4,121+ tests (0 ignored) |

## Skill Contents

### 📚 Language

- [**Type System**](language/type-system.md) - Type checking rules, discriminated unions, generics
- [**Concurrency**](language/concurrency.md) - Tasks, async/await, work-stealing scheduler
- [**Syntax**](language/syntax.md) - Import system, declarations, control flow
- [**Examples**](language/examples.md) - Complete working programs

### 🏗️ Architecture

- [**Overview**](architecture/overview.md) - High-level system design
- [**Crates**](architecture/crates.md) - 8-crate structure and dependencies
- [**Compiler**](architecture/compiler.md) - Parser → IR → Bytecode pipeline
- [**Virtual Machine**](architecture/vm.md) - Interpreter, scheduler, GC
- [**JIT/AOT**](architecture/jit-aot.md) - Native compilation features

### 📦 Standard Library

- [**Overview**](stdlib/overview.md) - Philosophy and I/O patterns
- [**Cross-Platform**](stdlib/cross-platform.md) - 14 modules (logger, math, crypto, etc.)
- [**POSIX**](stdlib/posix.md) - 15 system modules (fs, net, http, etc.)
- [**Native IDs**](stdlib/native-ids.md) - ID ranges and dispatch mechanism

### 🛠️ CLI & Tools

- [**Commands**](cli/commands.md) - run, build, check, eval, repl, bundle
- [**Package Manager**](cli/package-manager.md) - Dependency management workflow
- [**REPL**](cli/repl.md) - Interactive shell usage

### 💻 Development

- [**Workflow**](development/workflow.md) - Worktree workflow, build commands
- [**Testing**](development/testing.md) - Test infrastructure (4,121+ tests)
- [**Adding Modules**](development/adding-modules.md) - How to add stdlib modules
- [**Project Status**](development/status.md) - Completed and in-progress work

### 📖 Reference

- [**Quick Reference**](reference/quick-reference.md) - FAQ and cheat sheet
- [**Common Tasks**](reference/common-tasks.md) - Recipe-style task guide
- [**Documentation**](reference/documentation.md) - Links to all design docs

## Getting Started

### For Users
1. Read [Language Syntax](language/syntax.md)
2. Review [Standard Library Overview](stdlib/overview.md)
3. Check [CLI Commands](cli/commands.md)

### For Contributors
1. Understand [Architecture Overview](architecture/overview.md)
2. Review [Development Workflow](development/workflow.md)
3. Check [Project Status](development/status.md)

### For Language Learners
1. Read [Type System](language/type-system.md)
2. Study [Examples](language/examples.md)
3. Explore [Concurrency Model](language/concurrency.md)

## Design Principles

| Principle | Implementation |
|-----------|----------------|
| **Explicit over implicit** | Discriminated unions, required type annotations |
| **Safety over convenience** | No escape hatches, sound type system |
| **Performance through types** | Typed opcodes, monomorphization |
| **Familiar syntax** | TypeScript-compatible where possible |
| **Predictable semantics** | Well-defined execution model |

---

**Version:** 1.0.0  
**Last Updated:** 2026-02-22  
**Repository:** [github.com/rayalang/rayavm](https://github.com/rayalang/rayavm)
