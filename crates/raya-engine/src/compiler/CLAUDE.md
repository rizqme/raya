# Compiler

This folder takes a checked Raya program and turns it into executable bytecode modules. It is the bridge between the language frontend and the VM.

## Pipeline

The main path is:

typed AST -> [`lower`](lower/CLAUDE.md) -> [`ir`](ir/CLAUDE.md) -> [`monomorphize`](monomorphize/CLAUDE.md) -> [`optimize`](optimize/CLAUDE.md) -> [`codegen`](codegen/CLAUDE.md) -> [`bytecode`](bytecode/CLAUDE.md)

Multi-file and stdlib-aware compilation flows through [`module`](module/CLAUDE.md).

## What Lives Here

- `mod.rs`: top-level compiler API and configuration flags.
- `error.rs`: compiler error types.
- `module_builder.rs`: helpers for constructing bytecode modules programmatically.
- `native_id.rs`: compiler-visible native id definitions used during lowering and codegen.
- `type_registry.rs`: compiler-side type metadata not owned by parser/type checker.
- `intrinsic/`: special lowering/codegen hooks for builtins and optimized helper paths.
- `codegen_ast.rs`: older direct AST-to-bytecode path. Useful for compatibility and tests, but not the main evolution path.

## How To Choose A Subfolder

- Expression or statement compiles to the wrong shape: go to [`lower`](lower/CLAUDE.md).
- You need a new intermediate instruction or register-level concept: go to [`ir`](ir/CLAUDE.md).
- Generic specialization is wrong: go to [`monomorphize`](monomorphize/CLAUDE.md).
- IR is correct but overly verbose or illegal for codegen: go to [`optimize`](optimize/CLAUDE.md).
- IR is correct but emitted bytecode is wrong: go to [`codegen`](codegen/CLAUDE.md).
- Module serialization, imports, exports, verification, or opcodes are wrong: go to [`bytecode`](bytecode/CLAUDE.md).
- Files/imports/packages/std modules resolve incorrectly during compilation: go to [`module`](module/CLAUDE.md).

## Working Rules

- Prefer extending the IR pipeline instead of pushing new behavior into `codegen_ast.rs`.
- Compiler changes usually have contracts with parser types, VM opcodes, and runtime linking. Follow the full path before calling a fix complete.
