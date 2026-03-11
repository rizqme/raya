# Compiler Lowering

This folder is where typed syntax becomes executable compiler IR. The parser and checker decide what the program means; the lowerer decides how to express that meaning in IR blocks, registers, and runtime calls.

## What This Folder Owns

- Converting AST nodes into IR instructions and control-flow blocks.
- Preserving checker-derived type information so later stages can emit correct bytecode.
- Choosing between static and dynamic/runtime fallback paths.
- Lowering higher-level language features such as classes, decorators, JSX, closures, destructuring, and casts.

## File Guide

- `mod.rs`: central `Lowerer` state and shared lowering infrastructure.
- `expr.rs`: expression lowering. Most feature bugs surface here first.
- `stmt.rs`: statement lowering, scope-level execution flow, declarations, loops, try/catch, exports.
- `control_flow.rs`: shared helpers for branching and loop block layout.
- `class_methods.rs`: method/environment bridging for class bodies and captured outer scope behavior.

## Start Here When

- Code parses and type-checks, but runtime behavior is wrong because the emitted program shape is wrong.
- You need a new IR instruction or metadata to support a language feature.
- Decorators, JSX, structural projection, casts, or late-bound member access change.
- A feature works in the checker but loses type or shape information before codegen.

## Read Next

- IR data model: [`../ir/CLAUDE.md`](../ir/CLAUDE.md)
- Optimization passes after lowering: [`../optimize/CLAUDE.md`](../optimize/CLAUDE.md)
- Type information source: [`../../parser/checker/CLAUDE.md`](../../parser/checker/CLAUDE.md)
- Type model details: [`../../parser/types/CLAUDE.md`](../../parser/types/CLAUDE.md)

## Things To Watch

- Pointer-based and span-based type lookup both matter because some expressions are cloned during lowering.
- New lowering behavior often requires matching VM support, not just IR changes.
- If you add a new emitted instruction shape, confirm codegen knows how to serialize it.
