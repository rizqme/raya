# Parser

This folder is the language frontend. It turns source text into tokens, AST, symbols, types, and diagnostics that the compiler can trust.

## Frontend Flow

The main path is:

source text -> lexer/token stream -> parser/AST -> binder/checker -> typed AST + inferred type maps

## Main Areas

- [`ast/CLAUDE.md`](ast/CLAUDE.md): syntax tree node definitions and visitors.
- [`types/CLAUDE.md`](types/CLAUDE.md): the type model and low-level type relations.
- [`checker/CLAUDE.md`](checker/CLAUDE.md): scopes, symbols, type checking, narrowing, warnings, and diagnostics.

## Top-Level Files

- `lexer.rs`: tokenization.
- `token.rs`: token and span types.
- `parser.rs` and `parser/`: syntactic parsing routines.
- `interner.rs`: symbol interning so identifiers are shared cheaply.
- `ast.rs`: AST re-exports and root node definitions.

## How To Choose A Subfolder

- Syntax tree shape is missing data or a new language form: go to [`ast`](ast/CLAUDE.md).
- Type identities, assignability, signatures, or generic relations are wrong: go to [`types`](types/CLAUDE.md).
- Name binding, inference, narrowing, or diagnostics are wrong: go to [`checker`](checker/CLAUDE.md).

## Things To Watch

- Preserve spans carefully; the rest of the toolchain depends on them for diagnostics and source maps.
- A frontend feature is not complete just because parsing works. The checker and compiler need aligned semantics.
