# Type System

Raya's type system is **fully static** — all types are verified at compile time with zero runtime type checking overhead.

## Core Principles

- **No `any` type** - No escape hatches
- **No runtime type tags** - Types erased after compilation (except for polymorphic dispatch)
- **Monomorphization** - Generics specialized at compile time like Rust/C++
- **Sound type system** - Guarantees honored at runtime

## Type Checking Operators

### `typeof` - Primitive Type Checking

Used for checking primitive types and their unions:

```typescript
if (typeof x == "string") {
  // x is narrowed to string
  const len = x.length;
}

if (typeof y == "int" || typeof y == "number") {
  // y is int | number
  const doubled = y * 2;
}
```

**Supported types:**
- `"string"`
- `"int"` 
- `"number"` (float/double)
- `"boolean"`
- `"null"`

### `instanceof` - Class Type Checking

Used for checking class instances:

```typescript
class MyClass {}

if (obj instanceof MyClass) {
  // obj is narrowed to MyClass
  obj.someMethod();
}
```

## Discriminated Unions

Complex sum types require a discriminant field for type narrowing:

```typescript
type Result<T> =
  | { status: "ok"; value: T }
  | { status: "error"; error: string };

function processResult(r: Result<number>): void {
  if (r.status == "ok") {
    // r.value is accessible
    logger.info(r.value);
  } else {
    // r.error is accessible
    logger.error(r.error);
  }
}
```

**Requirements:**
- Must have a discriminant field (e.g., `status`)
- Discriminant must be a literal type
- All variants must have the discriminant field
- Compiler checks exhaustiveness

## Generics

### Type Parameters

```typescript
class Box<T> {
  value: T;
  
  constructor(value: T) {
    this.value = value;
  }
  
  get(): T {
    return this.value;
  }
}

// Usage
const box = new Box<int>(42);
const val = box.get(); // val: int
```

### Function Type Parameters

```typescript
function identity<T>(x: T): T {
  return x;
}

const num = identity<int>(42);       // returns int
const str = identity<string>("hi");  // returns string
```

### Method-Level Type Parameters

```typescript
class Container {
  store<T>(value: T): T {
    // Implementation
    return value;
  }
}
```

**Note:** Partial support - some edge cases may not work yet.

### Constraints

```typescript
interface Comparable {
  compareTo(other: Comparable): int;
}

function max<T extends Comparable>(a: T, b: T): T {
  return a.compareTo(b) > 0 ? a : b;
}
```

## Monomorphization

Generics are **specialized at compile time**, not erased:

```typescript
const intBox = new Box<int>(42);    // Generates Box_int
const strBox = new Box<string>(""); // Generates Box_string
```

Each instantiation creates a separate implementation with:
- Typed opcodes (e.g., `IADD` for `int`, `FADD` for `number`)
- No boxing/unboxing overhead
- Direct memory layout

## Function Parameters

### Rest Parameters

```typescript
function sum(...nums: int[]): int {
  let total = 0;
  for (let i = 0; i < nums.length; i = i + 1) {
    total = total + nums[i];
  }
  return total;
}

sum(1, 2, 3, 4); // 10
```

### Optional Parameters

```typescript
function greet(name: string, greeting?: string): string {
  const g = greeting ?? "Hello";
  return g + ", " + name;
}

greet("Alice");           // "Hello, Alice"
greet("Bob", "Hi");       // "Hi, Bob"
```

**Rules:**
- Optional parameters must come after required parameters
- Use `??` operator for default values
- Cannot mix rest and optional in same function

## Type Narrowing

The compiler tracks type information through control flow:

```typescript
function process(x: string | int): void {
  if (typeof x == "string") {
    // x is string here
    logger.info(x.toUpperCase());
  } else {
    // x is int here
    logger.info(x * 2);
  }
}
```

### Supported Narrowing

- `typeof` checks
- `instanceof` checks
- Discriminant field checks
- Truthiness checks
- Nullish checks (`x == null`)

## Type Aliases

```typescript
type UserId = int;
type Callback = (x: int) => void;
type Point = { x: number; y: number };
```

## Interface Types

```typescript
interface Drawable {
  draw(): void;
  getX(): number;
  getY(): number;
}

class Circle implements Drawable {
  x: number;
  y: number;
  radius: number;
  
  draw(): void { /* ... */ }
  getX(): number { return this.x; }
  getY(): number { return this.y; }
}
```

## Null Safety

No `null` or `undefined` in the type system by default. Use union types:

```typescript
function find(id: int): User | null {
  // May return null
  return null;
}

const user = find(42);
if (user != null) {
  // user is User here
  logger.info(user.name);
}
```

## Type Inference

The compiler infers types where possible:

```typescript
const x = 42;              // inferred as int
const y = 3.14;            // inferred as number
const z = "hello";         // inferred as string
const arr = [1, 2, 3];     // inferred as int[]

function double(x: int) {
  return x * 2;            // return type inferred as int
}
```

## Banned Features

- ❌ `any` type
- ❌ Runtime type assertions (`as` casting)
- ❌ Type coercion
- ❌ Prototype manipulation
- ❌ `eval()` with dynamic types

## Best Practices

1. **Use discriminated unions** for sum types
2. **Prefer `typeof` and `instanceof`** over custom type checks
3. **Leverage type inference** but annotate function signatures
4. **Use generics** for reusable code
5. **Avoid complex type gymnastics** - keep it simple

## Related

- [Concurrency](concurrency.md) - Type system with Tasks
- [Syntax](syntax.md) - Language syntax
- [Examples](examples.md) - Type system in action
