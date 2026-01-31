# builtins module

Precompiled builtin type signatures and native method bindings.

## Overview

This module provides:
- Type signatures for built-in classes (Array, Map, Set, etc.)
- Method signatures for primitive types (string, number methods)
- Precompiled bytecode for builtin implementations

## Module Structure

```
builtins/
├── mod.rs          # Entry point, signature loading
├── signatures.rs   # Type signature definitions
└── *.raya          # Builtin type definitions (source)
```

## Builtin Categories

### Primitives (Hardcoded in Compiler)
- `number`: Arithmetic, comparison, formatting
- `string`: charAt, substring, split, etc.
- `boolean`: No methods
- `Array<T>`: push, pop, map, filter, etc.

### Classes (Defined in .raya files)
- `Object`: Base class, toString
- `Error`: Exception type
- `Map<K, V>`: Key-value storage
- `Set<T>`: Unique value collection
- `Buffer`: Binary data
- `Date`: Date/time operations
- `Mutex`: Synchronization
- `Channel<T>`: Communication
- `Task<T>`: Async task reference

## Signature Types

```rust
pub struct BuiltinSignatures {
    pub classes: Vec<ClassSig>,
    pub functions: Vec<FunctionSig>,
}

pub struct ClassSig {
    pub name: String,
    pub type_params: Vec<String>,
    pub properties: Vec<PropertySig>,
    pub methods: Vec<MethodSig>,
}

pub struct MethodSig {
    pub name: String,
    pub params: Vec<ParamSig>,
    pub return_type: TypeSig,
    pub is_static: bool,
}
```

## Loading Builtins

```rust
// Get all builtin signatures
let signatures = get_all_signatures();

// Get specific builtin
let array_sig = get_signatures("Array");

// Get precompiled bytecode
let module = get_builtin_bytecode("Object");
```

## Builtin .raya Files

Example `Array.raya`:
```typescript
// Builtin array type - compiled to native calls

class Array<T> {
    // Native call: ARRAY_PUSH (0x0100)
    push(item: T): void {
        __NATIVE_CALL(0x0100, this, item);
    }

    // Native call: ARRAY_POP (0x0101)
    pop(): T | null {
        return __NATIVE_CALL(0x0101, this);
    }

    // Native call: ARRAY_LEN (0x0102)
    get length(): number {
        return __NATIVE_CALL(0x0102, this);
    }

    // Higher-order methods use IR intrinsics
    map<U>(fn: (item: T) => U): Array<U> {
        // Implemented via compiler intrinsic
    }
}
```

## Intrinsics

Special compiler directives in builtin files:

```typescript
// Direct opcode emission
__OPCODE_IADD()

// Native call with ID
__NATIVE_CALL(native_id, args...)

// Compiler intrinsic (generates specialized code)
__INTRINSIC_ARRAY_MAP(array, fn)
```

## Native ID Mapping

Builtin methods map to native IDs in `compiler/native_id.rs`:

```rust
// Object: 0x00xx
pub const OBJECT_TO_STRING: u16 = 0x0001;

// Array: 0x01xx
pub const ARRAY_PUSH: u16 = 0x0100;
pub const ARRAY_POP: u16 = 0x0101;
pub const ARRAY_LEN: u16 = 0x0102;
pub const ARRAY_GET: u16 = 0x0103;
pub const ARRAY_SET: u16 = 0x0104;

// String: 0x02xx
pub const STRING_CHAR_AT: u16 = 0x0200;
pub const STRING_SUBSTRING: u16 = 0x0201;
// ... etc
```

## For AI Assistants

- Builtins have NO runtime type checks (statically verified)
- Method dispatch uses native ID, not vtable for primitives
- Generic builtins (Array<T>) are monomorphized
- `.raya` files are parsed but specially handled by compiler
- Intrinsics emit direct IR, not regular function calls
- Add new builtins: signature + native ID + VM dispatch
