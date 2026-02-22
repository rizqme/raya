# Documentation Links

Complete index of Raya documentation.

## Design Documents

### Language

| Document | Path | Purpose |
|----------|------|---------|
| Language Specification | `docs/language/lang.md` | Complete language reference |
| Type System | `docs/language/typeof-design.md` | typeof operator design |
| Exception Handling | `docs/language/exception-handling.md` | Error handling semantics |
| JSON Type | `docs/language/json-type.md` | JSON integration |
| Numeric Types | `docs/language/numeric-types.md` | int vs number |

### Runtime

| Document | Path | Purpose |
|----------|------|---------|
| VM Architecture | `docs/runtime/architecture.md` | Execution model |
| Builtin Classes | `docs/runtime/builtin-classes.md` | Array, String, Object |
| Modules | `docs/runtime/modules.md` | Module loading |

### Compiler

| Document | Path | Purpose |
|----------|------|---------|
| Opcode Reference | `docs/compiler/opcode.md` | Bytecode instructions |
| Compilation Mapping | `docs/compiler/mapping.md` | AST → Bytecode patterns |
| Formats | `docs/compiler/formats.md` | File formats (.ryb, .bundle) |

### Standard Library

| Document | Path | Purpose |
|----------|------|---------|
| Stdlib Overview | `docs/stdlib/stdlib.md` | All stdlib modules |
| I/O Model | `docs/stdlib/io.md` | I/O patterns |

### Tooling

| Document | Path | Purpose |
|----------|------|---------|
| CLI Design | `docs/tooling/cli.md` | Command-line interface |

### Native Modules

| Document | Path | Purpose |
|----------|------|---------|
| Native ABI | `docs/native/abi.md` | NativeHandler interface |

## Milestone Plans

### Completed

- `plans/milestone-3.7.md` - Module system
- `plans/milestone-3.8.md` - Reflection API
- `plans/milestone-3.9.md` - Decorators
- `plans/milestone-4.1.md` - Core Types
- `plans/milestone-4.2.md` - std:logger
- `plans/milestone-4.3.md` - std:math
- `plans/milestone-4.6.md` - std:crypto
- `plans/milestone-4.7.md` - std:time
- `plans/milestone-4.8.md` - std:path
- `plans/milestone-4.10.md` - VM Unification

### In Progress

- `plans/milestone-4.5.md` - std:runtime (Phases 1-8 done)
- `plans/milestone-4.9.md` - Native ABI Refactor (Phases 1-2 done)

### Not Started

- `plans/milestone-4.4.md` - std:reflect

## Crate Documentation

### Root

- `CLAUDE.md` - Project overview, status, conventions
- `README.md` - Public README

### Crates

- `crates/CLAUDE.md` - Crates overview
- `crates/raya-engine/CLAUDE.md` - Engine internals
- `crates/raya-runtime/CLAUDE.md` - Runtime API
- `crates/raya-stdlib/CLAUDE.md` - Cross-platform stdlib
- `crates/raya-stdlib-posix/CLAUDE.md` - POSIX stdlib
- `crates/raya-cli/CLAUDE.md` - CLI implementation
- `crates/raya-pm/CLAUDE.md` - Package manager
- `crates/raya-sdk/CLAUDE.md` - FFI types
- `crates/raya-native/CLAUDE.md` - Proc-macros
- `crates/raya-lsp/CLAUDE.md` - Language server

### Engine Modules

- `crates/raya-engine/src/parser/CLAUDE.md` - Parser
- `crates/raya-engine/src/compiler/CLAUDE.md` - Compiler
- `crates/raya-engine/src/vm/CLAUDE.md` - Virtual machine
- `crates/raya-engine/src/parser/ast/CLAUDE.md` - AST
- `crates/raya-engine/src/parser/checker/CLAUDE.md` - Type checker
- `crates/raya-engine/src/parser/types/CLAUDE.md` - Type system
- `crates/raya-engine/src/compiler/bytecode/CLAUDE.md` - Bytecode
- `crates/raya-engine/src/compiler/codegen/CLAUDE.md` - Code generation
- `crates/raya-engine/src/compiler/ir/CLAUDE.md` - Intermediate representation
- `crates/raya-engine/src/compiler/lower/CLAUDE.md` - AST lowering
- `crates/raya-engine/src/compiler/module/CLAUDE.md` - Module system
- `crates/raya-engine/src/compiler/monomorphize/CLAUDE.md` - Monomorphization
- `crates/raya-engine/src/compiler/optimize/CLAUDE.md` - Optimizations
- `crates/raya-engine/src/vm/ffi/CLAUDE.md` - FFI
- `crates/raya-engine/src/vm/gc/CLAUDE.md` - Garbage collector
- `crates/raya-engine/src/vm/interpreter/CLAUDE.md` - Interpreter
- `crates/raya-engine/src/vm/module/CLAUDE.md` - Module loading
- `crates/raya-engine/src/vm/reflect/CLAUDE.md` - Reflection API
- `crates/raya-engine/src/vm/scheduler/CLAUDE.md` - Task scheduler

## External Resources

- **Website:** [rizqme.github.io/raya/](https://rizqme.github.io/raya/)
- **Repository:** [github.com/rayalang/rayavm](https://github.com/rayalang/rayavm)
- **VitePress Docs:** `docs/` directory (rendered at website)

## Quick Links

### For Users
1. [Language Specification](../language/syntax.md)
2. [Standard Library Overview](../stdlib/overview.md)
3. [CLI Commands](../cli/commands.md)
4. [Quick Reference](quick-reference.md)

### For Contributors
1. [Architecture Overview](../architecture/overview.md)
2. [Development Workflow](../development/workflow.md)
3. [Testing Infrastructure](../development/testing.md)
4. [Project Status](../development/status.md)

### For Language Learners
1. [Type System](../language/type-system.md)
2. [Concurrency Model](../language/concurrency.md)
3. [Examples](../language/examples.md)
4. [Common Tasks](common-tasks.md)
