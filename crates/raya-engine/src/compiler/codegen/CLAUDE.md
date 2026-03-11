# Compiler Codegen

This folder turns compiler IR into concrete bytecode instructions. Lowering decides what the program should do; codegen decides which opcodes, constants, function records, and module sections represent that behavior.

## What This Folder Owns

- Translating IR instructions and control-flow into bytecode sequences.
- Emitting functions, classes, debug info, and module sections.
- Handling operand sizing and instruction layout details.

## File Guide

- `context.rs`: `IrCodeGenerator` implementation and emission state.
- `emit.rs`: helper logic for opcode sizing and reusable encoding patterns.
- `control.rs`: branch/jump/control-flow emission helpers.
- `mod.rs`: top-level entrypoint from `IrModule` to bytecode `Module`.

## Start Here When

- Lowered IR looks correct, but the final bytecode does not.
- A new IR instruction or metadata field needs serialization into bytecode.
- Source-map, debug-info, or class/function emission changes are needed.

## Read Next

- IR definitions: [`../ir/CLAUDE.md`](../ir/CLAUDE.md)
- Bytecode format: [`../bytecode/CLAUDE.md`](../bytecode/CLAUDE.md)
- VM execution of emitted instructions: [`../../vm/interpreter/CLAUDE.md`](../../vm/interpreter/CLAUDE.md)

## Things To Watch

- Codegen assumes optimization has normalized IR into emit-ready form.
- Operand width changes can silently corrupt modules if `emit.rs` sizing and bytecode readers drift apart.
