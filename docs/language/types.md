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

## Mode-Specific Dynamic Types

Raya has three parsing/type modes:

- `raya`:
  - `any` is forbidden.
  - bare `let x;` is forbidden.
  - fallback inference uses `unknown`.
  - `unknown` is not actionable until narrowed/casted.
- `ts`:
  - behavior is driven by `tsconfig.json` `compilerOptions`.
  - explicit `any` is allowed, while implicit-any checks depend on flags.
- `js`:
  - dynamic JavaScript-compatible behavior.
  - bare `let x;` is allowed.
  - dynamic fallback may infer `JSObject`.

### Method Extraction Binding

Extracted methods are unbound (JS-like):

```typescript
class Counter {
  value: number;
  constructor(v: number) { this.value = v; }
  get(): number { return this.value; }
}

let c = new Counter(1);
let f = c.get;
// f(); // compile-time error (unbound method call)
let bound = f.bind(c);
bound(); // ok
```

Binding checks are compile-time validated for `.bind/.call/.apply` when the target is an extracted method.
