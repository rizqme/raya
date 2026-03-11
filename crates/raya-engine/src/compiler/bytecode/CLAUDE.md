# Compiler Bytecode

This folder defines the actual artifact the VM executes. If the compiler output is a `.ryb` file or an in-memory `Module`, its shape is defined here.

## What This Folder Owns

- Opcode definitions.
- Constant-pool layout.
- Module-level serialization format.
- Import/export records and metadata carried into runtime linking.
- Verification rules that reject malformed or inconsistent bytecode before execution.

## File Guide

- `opcode.rs`: the instruction set. Adding an opcode starts here.
- `module.rs`: bytecode `Module`, import/export records, metadata, reflection/debug/native sections, versioning.
- `constants.rs`: constants stored by compiled modules.
- `encoder.rs`: reading and writing encoded modules.
- `verify.rs`: structural checks that guard the loader and VM.

## Start Here When

- You need a new opcode or operand encoding.
- A compiled module cannot be loaded, linked, or validated.
- Debug info, reflection metadata, native function tables, or checksum handling must change.

## Read Next

- Emission side: [`../codegen/CLAUDE.md`](../codegen/CLAUDE.md)
- Runtime consumer: [`../../vm/CLAUDE.md`](../../vm/CLAUDE.md)
- Runtime linker: [`../../vm/module/CLAUDE.md`](../../vm/module/CLAUDE.md)

## Things To Watch

- Bytecode version bumps are ecosystem-wide changes.
- Import/export signature changes affect compile-time and runtime linking together.
- New opcodes are incomplete until lowering, codegen, interpreter dispatch, and tests all agree.
