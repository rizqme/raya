# Compiler Monomorphize

This folder specializes generic IR into concrete instantiations. It exists so generic functions and classes can be turned into concrete runtime shapes instead of relying on generic dispatch everywhere.

## What This Folder Owns

- Discovering which generic entities need specialization.
- Creating deterministic specialization keys.
- Rewriting IR to point at specialized functions/classes.
- Emitting metadata that later stages can use for debugging and reflection.

## File Guide

- `collect.rs`: finds generic definitions and concrete call/construct sites.
- `specialize.rs`: clones or materializes specialized IR items.
- `substitute.rs`: replaces generic type parameters with concrete types.
- `rewrite.rs`: patches call sites to the specialized targets.
- `mod.rs`: shared context, work queue, and stable key/hash logic.

## Start Here When

- Generic code compiles but calls the wrong specialization.
- Specialization deduplication or naming is unstable.
- Reflection or debug metadata for generic specializations is wrong.

## Read Next

- IR model: [`../ir/CLAUDE.md`](../ir/CLAUDE.md)
- Lowering source for generic info: [`../lower/CLAUDE.md`](../lower/CLAUDE.md)
- Reflection consumers of generic metadata: [`../../vm/reflect/CLAUDE.md`](../../vm/reflect/CLAUDE.md)
