# VM

This folder executes bytecode and holds the runtime object model. It is where compiled Raya programs become running tasks, heap objects, native calls, and suspension/resume events.

## Runtime Shape

The VM is not just "the interpreter". Execution depends on:

- bytecode definitions from the compiler
- shared runtime state and object/value layout
- task scheduling and suspension
- garbage collection
- module linking
- reflection and native interop

## Main Areas

- [`interpreter/CLAUDE.md`](interpreter/CLAUDE.md): execution loop, VM facade, shared runtime state.
- [`scheduler/CLAUDE.md`](scheduler/CLAUDE.md): task scheduling and IO/worker coordination.
- [`gc/CLAUDE.md`](gc/CLAUDE.md): allocation, collection, and root discovery.
- [`module/CLAUDE.md`](module/CLAUDE.md): runtime import/export resolution and linking.
- [`reflect/CLAUDE.md`](reflect/CLAUDE.md): reflection metadata, proxies, runtime builders.
- [`ffi/CLAUDE.md`](ffi/CLAUDE.md): C ABI and native module loading.
- [`snapshot/CLAUDE.md`](snapshot/CLAUDE.md): serialized VM pause/resume state.
- [`sync/CLAUDE.md`](sync/CLAUDE.md): task-aware mutexes and semaphores.

## Top-Level Files

- `builtin.rs` and `builtins/`: builtin ids, handlers, and Rust-owned runtime surfaces.
- `native_handler.rs` and `native_registry.rs`: native dispatch traits and registries.
- `value.rs` and `object.rs`: runtime value/object model.
- `types/`, `stack.rs`, `abi.rs`: runtime type info, call stack, ABI helpers.
- `json/` and `defaults.rs`: JSON/type-schema helpers and VM defaults.

## How To Choose A Subfolder

- Wrong opcode behavior or VM control flow: go to [`interpreter`](interpreter/CLAUDE.md).
- Task starvation, preemption, or blocking issues: go to [`scheduler`](scheduler/CLAUDE.md).
- Memory leaks or invalid collection: go to [`gc`](gc/CLAUDE.md).
- Import/export resolution at runtime: go to [`module`](module/CLAUDE.md).
- Reflection, decorators, dynamic builders, proxies: go to [`reflect`](reflect/CLAUDE.md).
- C/native module boundary issues: go to [`ffi`](ffi/CLAUDE.md).
- Paused VM transfer or snapshot compatibility: go to [`snapshot`](snapshot/CLAUDE.md).
- Mutex/semaphore behavior: go to [`sync`](sync/CLAUDE.md).
