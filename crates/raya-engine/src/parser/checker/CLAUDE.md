# Parser Checker

This folder gives parsed Raya code its static meaning. It binds names to scopes, assigns types to expressions, computes warnings/errors, and produces the inferred type data used by the compiler.

## What This Folder Owns

- Scope creation and symbol tables.
- Binding declarations and references.
- Expression and statement type checking.
- Flow-sensitive narrowing and exhaustiveness analysis.
- Closure capture analysis.
- Checker behavior differences between Raya, TS, and JS-like modes.

## File Guide

- `binder.rs`: scope construction and declaration binding.
- `checker.rs`: main type-checking engine.
- `builtins.rs`: builtin classes/functions/properties visible to the checker.
- `captures.rs`: closure capture analysis.
- `narrowing.rs`: flow-sensitive refinement after guards and control flow.
- `exhaustiveness.rs`: union coverage checks.
- `type_guards.rs`: recognition of guard patterns that drive narrowing.
- `symbols.rs`: symbol and scope model.
- `diagnostic.rs` and `error.rs`: emitted diagnostics, warnings, and formatting support.

## Start Here When

- A name resolves to the wrong declaration.
- A program should type-check but does not, or vice versa.
- Narrowing after `if`, pattern checks, or discriminant tests is wrong.
- Closure capture behavior is wrong.
- Builtin surface or mode-specific checker behavior diverges unexpectedly.

## Read Next

- Type primitives and relations: [`../types/CLAUDE.md`](../types/CLAUDE.md)
- Compiler consumer of checker output: [`../../compiler/lower/CLAUDE.md`](../../compiler/lower/CLAUDE.md)

## Things To Watch

- The checker emits maps keyed by AST identity and spans; lowering depends on that data surviving intact.
- Many "compiler bugs" are actually checker bugs when the wrong type or symbol information is handed downstream.
