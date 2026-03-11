# raya-engine

This crate is the language implementation itself. If Raya parses differently, type-checks differently, emits different bytecode, or executes differently, the change almost always lands here.

## Internal Map

- [`src/parser/CLAUDE.md`](src/parser/CLAUDE.md): tokenization, AST construction, type representation, binding, checking, diagnostics.
- [`src/compiler/CLAUDE.md`](src/compiler/CLAUDE.md): lowering typed AST to IR, optimizing IR, generating bytecode, multi-module compilation.
- [`src/vm/CLAUDE.md`](src/vm/CLAUDE.md): bytecode execution, runtime data structures, GC, scheduling, reflection, FFI.
- `src/jit`: optional JIT backend and hot-path compilation logic.
- `src/aot`: optional ahead-of-time compilation path.
- `src/profiler`: profiling support.
- `src/linter`: lint analysis.
- `builtins`: embedded builtin/runtime source files that seed language surfaces.

## When You Should Be In This Crate

- Adding or changing syntax.
- Fixing type-system behavior.
- Changing import/export linkage rules at the bytecode or engine level.
- Adding an opcode, builtin id, reflection capability, or runtime feature.
- Debugging a runtime result that is wrong even when the high-level runtime API is not involved.

## How To Navigate Inside

- Parsed wrong or failed to parse: go to `src/parser`.
- Parsed fine but type error or missing inference: go to `src/parser/checker` and `src/parser/types`.
- Type-checks fine but generated program is wrong: go to `src/compiler/lower`, `src/compiler/ir`, and `src/compiler/codegen`.
- Bytecode looks right but execution is wrong: go to `src/vm/interpreter`, `src/vm/scheduler`, and `src/vm/object/value`.
- Native call ids or runtime builders mismatch: go to `src/vm/builtin.rs`, `src/vm/reflect`, and stdlib crates.

## Read Next

- For language frontend work: [`src/parser/CLAUDE.md`](src/parser/CLAUDE.md)
- For compilation pipeline work: [`src/compiler/CLAUDE.md`](src/compiler/CLAUDE.md)
- For runtime execution work: [`src/vm/CLAUDE.md`](src/vm/CLAUDE.md)
