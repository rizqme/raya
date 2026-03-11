# Parser Types

This folder defines Raya's type model and the low-level relations that the checker and compiler build on. If the checker asks "are these compatible?" or the compiler needs a stable structural signature, the answer starts here.

## What This Folder Owns

- `Type` and `TypeId`.
- Type interning and context storage.
- Assignability and subtyping rules.
- Normalization and generic substitution.
- Canonical signatures used for cross-module compatibility and metadata.
- Discriminated-union helpers and union-specific analysis.

## File Guide

- `ty.rs`: core type representations.
- `context.rs`: storage and interning for types.
- `assignability.rs`: assignment compatibility rules.
- `subtyping.rs`: subtype relationships.
- `signature.rs`: canonical string/signature hashing logic.
- `generics.rs`: generic context and substitution support.
- `normalize.rs`: canonicalization and simplification.
- `discriminant.rs`: discriminant inference for unions.
- `bare_union.rs`: special handling for bare unions.
- `error.rs`: type-layer errors.

## Start Here When

- Type equality, assignability, or subtyping is wrong.
- Generic substitution is unstable.
- Structural signatures mismatch across modules.
- Checker logic fails because the underlying type primitives are not expressing the right rules.

## Read Next

- Consumer: [`../checker/CLAUDE.md`](../checker/CLAUDE.md)
- Compiler use of signatures and ids: [`../../compiler/module/CLAUDE.md`](../../compiler/module/CLAUDE.md)

## Things To Watch

- Signature logic is part of the import/export compatibility contract, not just an internal convenience.
- `TypeId` and canonical signature changes can affect reflection, metadata, and runtime linking.
