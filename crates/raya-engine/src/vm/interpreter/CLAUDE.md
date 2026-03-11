# VM Interpreter

This folder contains the execution engine proper. It owns the public `Vm` facade, the interpreter loop, shared runtime state, runtime registries, and the mechanics of pause/resume, marshaling, and capability checks.

## What This Folder Owns

- Dispatching bytecode opcodes.
- Tracking execution frames and return/suspend behavior.
- Holding runtime-global state shared across tasks and contexts.
- Exposing the main `Vm` API used by higher layers.
- Managing runtime class/module/native-module registries.
- Handling safepoints and debugger-facing state.

## File Guide

- `core.rs`: interpreter loop and dispatch entrypoints.
- `execution.rs`: execution result types and control-flow framing.
- `context.rs`: VM contexts, options, and resource limits/counters.
- `shared_state.rs`: shared registries, telemetry, microtasks, runtime layout state.
- `module_registry.rs`: loaded modules and runtime layout metadata.
- `class_registry.rs`: runtime classes and layout lookups.
- `native_module_registry.rs`: dynamically loaded native modules.
- `marshal.rs`: value conversion across VM boundaries.
- `capabilities.rs`: capability and permission-style runtime gating.
- `debug_state.rs`: debugger-visible state.
- `vm_facade.rs`: user-facing `Vm` wrapper.
- `safepoint.rs`: stop-the-world coordination.

## Start Here When

- A bytecode instruction behaves incorrectly.
- Suspend/resume behavior is wrong.
- VM context isolation or shared state ownership is wrong.
- Runtime registries for modules/classes/native modules are inconsistent.
- A high-level runtime API ultimately fails inside the VM core.

## Read Next

- Scheduler integration: [`../scheduler/CLAUDE.md`](../scheduler/CLAUDE.md)
- GC invariants: [`../gc/CLAUDE.md`](../gc/CLAUDE.md)
- Bytecode source of opcodes: [`../../compiler/bytecode/CLAUDE.md`](../../compiler/bytecode/CLAUDE.md)
- Reflection integration: [`../reflect/CLAUDE.md`](../reflect/CLAUDE.md)
