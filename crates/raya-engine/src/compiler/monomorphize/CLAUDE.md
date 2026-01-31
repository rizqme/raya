# monomorphize module

Generic specialization for the Raya compiler.

## Overview

Monomorphization generates specialized versions of generic functions and classes for each concrete type instantiation. This eliminates runtime type dispatch overhead.

## Module Structure

```
monomorphize/
├── mod.rs         # Entry point, monomorphize()
├── collect.rs     # Collect generic instantiations
├── specialize.rs  # Generate specialized code
├── substitute.rs  # Type parameter substitution
└── rewrite.rs     # IR rewriting
```

## Process

### 1. Collection (`collect.rs`)

Scan the IR to find all generic instantiations:

```typescript
// Source
function identity<T>(x: T): T { return x; }

let a = identity(42);        // identity<number>
let b = identity("hello");   // identity<string>
```

Produces:
```
GenericInstantiation {
    base: FunctionId(identity),
    type_args: [TypeId::NUMBER]
}
GenericInstantiation {
    base: FunctionId(identity),
    type_args: [TypeId::STRING]
}
```

### 2. Specialization (`specialize.rs`)

For each instantiation, create a specialized version:

```
// identity<number>
function identity$number(x: number): number {
    return x;
}

// identity<string>
function identity$string(x: string): string {
    return x;
}
```

### 3. Rewriting (`rewrite.rs`)

Replace generic calls with specialized calls:

```
// Before
r0 = Call identity<number>, arg0

// After
r0 = Call identity$number, arg0
```

## Key Types

```rust
pub struct MonomorphizeResult {
    pub functions_specialized: usize,
    pub classes_specialized: usize,
}

pub fn monomorphize(
    module: &mut IrModule,
    type_ctx: &TypeContext,
    interner: &Interner,
) -> MonomorphizeResult
```

## Naming Convention

Specialized functions/classes use a mangled name:

```
// Functions
identity<number> → identity$number
map<string, number> → map$string$number

// Classes
Array<number> → Array$number
Map<string, User> → Map$string$User
```

## Type Substitution (`substitute.rs`)

Replaces type parameters with concrete types:

```rust
substitute_type(
    TypeId::TypeParam("T"),
    &[("T", TypeId::NUMBER)]
) → TypeId::NUMBER
```

Handles:
- Direct type parameters: `T` → `number`
- Nested generics: `Array<T>` → `Array<number>`
- Function types: `(T) => T` → `(number) => number`

## Recursive Specialization

Generic functions that call other generics trigger cascading specialization:

```typescript
function wrap<T>(x: T): Array<T> {
    return [x];  // Creates Array<T>
}

let a = wrap(42);  // Specializes wrap<number> AND Array<number>
```

## For AI Assistants

- Monomorphization runs BEFORE optimizations
- Every distinct type argument combination creates a new function
- Name mangling uses `$` separator for type arguments
- Recursive generics are handled by fixpoint iteration
- Specialization can significantly increase code size
- Type erasure is NOT used - all generics are fully specialized
