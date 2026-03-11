# VM Reflect

This folder is the runtime metaprogramming layer. It powers reflection APIs, decorator metadata, proxies, dynamic class/function/module construction, and object inspection helpers.

## What This Folder Owns

- Metadata storage attached to classes, methods, fields, and objects.
- Runtime type and class introspection.
- Proxy trap plumbing.
- Permission checks around reflective access and code generation.
- Dynamic builders for bytecode, classes, functions, and modules.
- Generic-type metadata parsing used by reflection features.

## File Guide

- `metadata.rs` and `class_metadata.rs`: stored metadata and class descriptors.
- `introspection.rs`: lookups for classes, fields, methods, type info, and hierarchy.
- `permissions.rs`: reflective access policy checks.
- `proxy.rs`: proxy wrapping and trap resolution.
- `bootstrap.rs`: bootstrap context and well-known ids for dynamic execution.
- `bytecode_builder.rs`, `function_builder.rs`, `runtime_builder.rs`, `type_builder.rs`, `dynamic_module.rs`: runtime construction APIs.
- `generic_metadata.rs`: generic and monomorphized metadata helpers.
- `snapshot.rs`: reflection-facing object snapshot and diff helpers.

## Start Here When

- `Reflect.*` behavior is wrong.
- Decorator metadata is missing or misapplied at runtime.
- Proxy semantics are incorrect.
- Dynamic module/class/function creation needs a new operation.
- Reflection permissions need tightening or expansion.

## Read Next

- Stdlib-facing wrappers: [`../../../../raya-stdlib/CLAUDE.md`](../../../../raya-stdlib/CLAUDE.md)
- Compiler metadata producers: [`../../compiler/bytecode/CLAUDE.md`](../../compiler/bytecode/CLAUDE.md) and [`../../compiler/monomorphize/CLAUDE.md`](../../compiler/monomorphize/CLAUDE.md)
- VM runtime host: [`../interpreter/CLAUDE.md`](../interpreter/CLAUDE.md)
