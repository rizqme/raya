# types module

Type system representation and operations for Raya.

## Module Structure

```
types/
├── mod.rs            # Entry point, Type enum, TypeId, TypeContext
├── discriminant.rs   # Discriminant inference for unions
├── bare_union.rs     # Bare union (primitive union) handling
├── assignability.rs  # Type assignability rules
└── subtyping.rs      # Subtyping relationships
```

## Key Types

### TypeId
Lightweight type reference (index into TypeContext).
```rust
#[derive(Copy, Clone, Eq, Hash)]
pub struct TypeId(u32);

// Well-known TypeIds:
// 0 = number
// 1 = string
// 2 = boolean
// 3 = null
// 4 = void
// 5 = never
// 6 = unknown
```

### Type
Full type representation.
```rust
pub enum Type {
    Primitive(PrimitiveType),
    Class(ClassType),
    Function(FunctionType),
    Array(ArrayType),
    Tuple(TupleType),
    Union(UnionType),
    Object(ObjectType),
    Generic(GenericType),
    TypeParameter(TypeParameter),
    // ...
}
```

### TypeContext
Type registry and operations.
```rust
pub struct TypeContext {
    types: Vec<Type>,
    // caches, interned types, etc.
}

ctx.add_type(ty) -> TypeId
ctx.get_type(id) -> &Type
ctx.is_assignable(from, to) -> bool
ctx.common_type(a, b) -> Option<TypeId>
```

## Discriminant Inference (`discriminant.rs`)

For discriminated unions, automatically infers the discriminant field:

```typescript
type Result =
    | { status: "ok"; value: number }
    | { status: "error"; error: string };
// Infers "status" as discriminant
```

Priority order: `kind` > `type` > `tag` > `variant` > alphabetical

## Bare Unions (`bare_union.rs`)

Handles primitive-only unions that use `typeof` for narrowing:

```typescript
type ID = string | number;
// This is a "bare union" - no discriminant, uses typeof
```

## Assignability (`assignability.rs`)

Determines if one type can be assigned to another:
- Structural subtyping for objects
- Covariance for return types
- Contravariance for parameter types
- Union types are assignable if all variants are

## Subtyping (`subtyping.rs`)

Establishes subtype relationships:
- `never` is subtype of everything
- Everything is subtype of `unknown`
- Class inheritance creates subtyping
- Object types use structural subtyping

## For AI Assistants

- `TypeId` is cheap to copy, use it instead of cloning `Type`
- TypeContext is the central registry for all types
- Discriminated unions require a common discriminant field
- Bare unions are for primitives only (`string | number`)
- Assignability is NOT symmetric (`A assignable to B` ≠ `B assignable to A`)
- No implicit type coercion - all conversions must be explicit
