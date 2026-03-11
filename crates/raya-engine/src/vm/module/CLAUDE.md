# VM Module Linking

This folder links compiled bytecode modules together at runtime. The compiler writes import/export contracts; this code enforces and resolves them when modules are loaded into a VM.

## What This Folder Owns

- Runtime dependency graph handling.
- Import specification helpers.
- Symbol resolution across loaded modules.
- Linking compiled modules into live runtime state.

## File Guide

- `deps.rs`: runtime dependency graph support.
- `import.rs`: import specs and import-level helper types.
- `linker.rs`: main runtime linker and symbol resolution logic.

## Start Here When

- Modules compile successfully but fail when loaded together.
- Import/export resolution is wrong at runtime but compile-time metadata looks reasonable.
- VM-side linking contracts need to evolve.

## Read Next

- Compile-time counterpart: [`../../compiler/module/CLAUDE.md`](../../compiler/module/CLAUDE.md)
- Runtime owner of loaded modules: [`../interpreter/CLAUDE.md`](../interpreter/CLAUDE.md)
