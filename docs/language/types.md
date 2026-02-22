# Type System

Raya has a fully static type system with compile-time verification and zero runtime overhead.

## Primitive Types

### Numbers
- `int` - 64-bit signed integer
- `number` - 64-bit floating point (IEEE 754)
- `boolean` - true or false
- `string` - UTF-8 strings

```typescript
const count: int = 42;
const pi: number = 3.14159;
const active: boolean = true;
const name: string = "Alice";
```

### Special Types
- `null` - absence of value
- `void` - no return value

```typescript
function greet(name: string | null): void {
  if (name == null) {
    io.writeln("Hello, stranger!");
  } else {
    io.writeln(`Hello, ${name}!`);
  }
}
```

## Type Operators

### typeof
Use `typeof` for primitive union type checking:

```typescript
function process(value: string | int | number | boolean | null): void {
  if (typeof value == "string") {
    io.writeln("String: " + value);
  } else if (typeof value == "int") {
    io.writeln("Integer: " + value.toString());
  }
}
```

### instanceof
Use `instanceof` for class type checking:

```typescript
class User {
  constructor(public name: string) {}
}

function check(obj: User): void {
  if (obj instanceof User) {
    io.writeln(`User: ${obj.name}`);
  }
}
```

## Discriminated Unions

Required discriminant field for complex types:

```typescript
type Result<T> =
  | { status: "ok"; value: T }
  | { status: "error"; error: string };

function divide(a: number, b: number): Result<number> {
  if (b == 0) {
    return { status: "error", error: "Division by zero" };
  }
  return { status: "ok", value: a / b };
}
```

## No `any` Type

All values have known types. No escape hatches.
