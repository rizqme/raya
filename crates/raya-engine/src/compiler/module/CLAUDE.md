# Compiler Module System

This folder handles compilation once a program stops being "just one file". It resolves imports, builds dependency graphs, loads declaration surfaces, embeds std modules, and produces consistent import/export metadata for runtime linking.

## What This Folder Owns

- Module specifier resolution for local files, packages, URLs, and `std:` or `node:` modules.
- Dependency graph construction and cycle reporting.
- Cross-module symbol export/import metadata.
- Declaration-file loading and builtin declaration seeding.
- Late-link requirements used when compile-time information must survive into runtime linkage.

## File Guide

- `compiler.rs`: orchestration entrypoint for multi-file compile flows.
- `resolver.rs`: import resolution rules.
- `graph.rs`: dependency graph structure and cycle logic.
- `cache.rs`: compiled module cache.
- `exports.rs`: export registry used for cross-module checks and signatures.
- `declaration.rs`: builtin/global declarations and declaration-backed imports.
- `typedef.rs`: `.d.raya` parsing and signature extraction.
- `std_modules.rs`: embedded standard-library source registry.

## Start Here When

- `import` resolution is wrong.
- A package, URL, or std module compiles differently than a local file.
- Declaration files are ignored or interpreted incorrectly.
- Cross-module type compatibility or import/export identity is broken.

## Read Next

- High-level runtime program loader: [`../../../../raya-runtime/CLAUDE.md`](../../../../raya-runtime/CLAUDE.md)
- Runtime linker: [`../../vm/module/CLAUDE.md`](../../vm/module/CLAUDE.md)
- Type signatures used for compatibility: [`../../parser/types/CLAUDE.md`](../../parser/types/CLAUDE.md)

## Things To Watch

- Structural signatures and symbol ids are part of the linking contract.
- Compile-time module resolution and runtime module loading are different layers; keep them conceptually separate but behaviorally aligned.
