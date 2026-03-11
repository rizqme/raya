# Compiler IR

This folder defines the compiler's intermediate representation: the form that sits between checked syntax and concrete bytecode. It should be easier to optimize than AST and easier to emit than raw syntax trees.

## What This Folder Owns

- Register-based instruction model.
- Basic blocks and terminators.
- Function and module-level IR containers.
- IR-level class and type-alias metadata.
- Pretty-printers used for compiler debugging.

## File Guide

- `module.rs`: top-level `IrModule` plus class/type-alias IR records.
- `function.rs`: `IrFunction` representation.
- `block.rs`: `BasicBlock` and terminators.
- `instr.rs`: instruction enum and operator ids.
- `value.rs`: registers, constants, and value origin tracking.
- `pretty.rs`: debugging/inspection formatting.

## Start Here When

- Lowering needs a new intermediate concept that bytecode should not model directly.
- Optimizations need richer block, instruction, or register metadata.
- You need to understand the compiler's internal program shape before codegen.

## Read Next

- Producer: [`../lower/CLAUDE.md`](../lower/CLAUDE.md)
- Consumers: [`../optimize/CLAUDE.md`](../optimize/CLAUDE.md) and [`../codegen/CLAUDE.md`](../codegen/CLAUDE.md)

## Things To Watch

- IR additions are cross-cutting changes. Lowering must emit them, optimizers must tolerate them, and codegen must serialize them.
- Keep the IR expressive enough for optimization but not so high-level that it duplicates AST semantics.
