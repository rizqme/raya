# Parser AST

This folder defines the abstract syntax tree for Raya source code. If the language has a syntactic construct, its parsed shape should be represented here.

## What This Folder Owns

- Expression, statement, pattern, and type-annotation node structures.
- Shared traversal utilities used by binder, checker, and compiler.
- Span-carrying syntax nodes that preserve source locations.

## File Guide

- `expression.rs`: literals, operators, calls, member access, classes, decorators, JSX, casts, and other expression forms.
- `statement.rs`: declarations, control flow, imports/exports, blocks, and module-level statements.
- `pattern.rs`: destructuring and binding patterns.
- `types.rs`: parsed type annotation syntax.
- `visitor.rs`: AST traversal helpers.

## Start Here When

- Adding or modifying syntax.
- A node needs extra fields for later stages.
- Visitors miss a new construct and downstream passes silently ignore it.

## Read Next

- Frontend semantics: [`../checker/CLAUDE.md`](../checker/CLAUDE.md)
- Compiler consumption of syntax: [`../../compiler/lower/CLAUDE.md`](../../compiler/lower/CLAUDE.md)

## Things To Watch

- AST shape changes ripple outward quickly.
- Every new node form needs parser support, visitor coverage, and often checker/lowerer updates.
