# vm/reflect module

Runtime reflection implementation for the Raya VM.

## Overview

This module implements the `std:reflect` API, providing runtime introspection and dynamic code capabilities. Reflection metadata is always emitted (no compiler flag needed).

## Module Structure

```
reflect/
├── mod.rs              # Entry point, re-exports
├── metadata.rs         # MetadataStore (WeakMap-style user metadata)
├── class_metadata.rs   # ClassMetadataRegistry (field/method info per class)
├── introspection.rs    # Type queries, instanceof, cast, hierarchy
├── snapshot.rs         # ObjectSnapshot, ObjectDiff for state tracking
├── proxy.rs            # Proxy objects and trap helpers
├── type_builder.rs     # Dynamic subclass creation (Phase 10)
├── generic_metadata.rs # Generic type tracking (Phase 13)
├── runtime_builder.rs  # ClassBuilder, DynamicFunction (Phase 14)
├── bytecode_builder.rs # BytecodeBuilder for dynamic code (Phase 15)
├── function_builder.rs # FunctionWrapper, DecoratorRegistry (M3.9 Phase 4)
├── permissions.rs      # ReflectionPermission, PermissionStore (Phase 16)
├── dynamic_module.rs   # DynamicModuleRegistry (Phase 17)
└── bootstrap.rs        # BootstrapContext for VM bootstrap (Phase 17)
```

## Key Types

### MetadataStore
```rust
// User-defined metadata (WeakMap-style)
metadata_store.define(key, value, target, property_key?);
metadata_store.get(key, target, property_key?);
metadata_store.has(key, target, property_key?);
metadata_store.delete(key, target, property_key?);
```

### ClassMetadataRegistry
```rust
// Class structure info
registry.register(class_id, ClassMetadata { name, fields, methods, ... });
registry.get(class_id) -> Option<&ClassMetadata>
registry.get_by_name(name) -> Option<usize>
registry.all_classes() -> Vec<usize>
```

### BytecodeBuilder
```rust
// Dynamic bytecode generation
let mut builder = BytecodeBuilder::new("funcName", param_count);
builder.emit_push(42);
builder.emit_iadd();
builder.emit_return();
let compiled = builder.build()?;
```

### PermissionStore
```rust
// Security permissions
store.set_object_permissions(obj_id, ReflectionPermission::READ_PUBLIC);
store.get_effective_permissions(obj_id, class_id, module);
store.seal_permissions(obj_id);
```

### FunctionWrapper (Decorator Support)
```rust
// High-level wrapper creation for method decorators
let wrapper = FunctionWrapper::new(original_func_id, param_count)
    .with_before(before_hook_id)
    .with_after(after_hook_id)
    .build()?;
WRAPPER_FUNCTION_REGISTRY.lock().register(wrapper);
```

### DecoratorRegistry
```rust
// Track decorator applications for getClassesWithDecorator
registry.register_class_decorator(class_id, decorator_app);
registry.get_classes_with_decorator("Injectable") -> Vec<usize>
registry.get_class_decorators(class_id) -> Vec<&DecoratorApplication>
```

## Native Call IDs

| Range | Category | Phase |
|-------|----------|-------|
| 0x0D00-0x0D0F | Metadata operations | 1 |
| 0x0D10-0x0D1F | Class introspection | 2 |
| 0x0D20-0x0D2F | Field access | 3 |
| 0x0D30-0x0D3F | Method invocation | 4 |
| 0x0D40-0x0D4F | Object creation | 5 |
| 0x0D50-0x0D5F | Type utilities | 6 |
| 0x0D60-0x0D6F | Interface/hierarchy | 7 |
| 0x0D70-0x0D8F | Object inspection | 8 |
| 0x0D90-0x0D9F | Stack introspection | 8 |
| 0x0DA0-0x0DAF | Serialization | 8 |
| 0x0DB0-0x0DBF | Proxy objects | 9 |
| 0x0DC0-0x0DCF | Dynamic subclass | 10 |
| 0x0DD0-0x0DDF | Generic metadata | 13 |
| 0x0DE0-0x0DEF | Runtime type creation | 14 |
| 0x0DF0-0x0DFF | Bytecode builder | 15 |
| 0x0E00-0x0E0F | Permissions | 16 |
| 0x0E10-0x0E2F | Dynamic VM bootstrap | 17 |

## Implementation Status

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | Core metadata | ✅ Complete |
| 2 | Class introspection | ✅ Complete |
| 3 | Field access | ✅ Complete |
| 4 | Method invocation | ✅ Complete |
| 5 | Object creation | ✅ Complete |
| 6 | Type utilities | ✅ Complete |
| 7 | Interface query | ✅ Complete |
| 8 | Object inspection | ✅ Complete |
| 9 | Proxy objects | ✅ Complete |
| 10 | Dynamic subclass | ✅ Complete |
| 13 | Generic metadata | ✅ Complete |
| 14 | Runtime types | ✅ Complete |
| 15 | Bytecode builder | ✅ Complete |
| 16 | Permissions | ✅ Complete |
| 17 | VM bootstrap | ✅ Complete |

**Blocked**: Phase 12 (integration tests), Phase 18 (benchmarks) need compiler `std:` import support.

## Testing

- 149+ unit tests in this module
- Tests in each submodule's `#[cfg(test)]` block
- Integration tests in `tests/reflect_phase8_tests.rs`

## For AI Assistants

- All handlers are in `vm/vm/handlers/reflect.rs`
- Native IDs defined in `vm/builtin.rs`
- Type definitions in `raya-stdlib/reflect.d.raya`
- SharedVmState holds MetadataStore, ClassMetadataRegistry
- Reflection is always enabled (no compiler flag)
- Most invoke/execute operations are stubs (need VM context threading)
