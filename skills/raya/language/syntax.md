# Language Syntax

Raya uses **TypeScript-compatible syntax** where possible, making it familiar to JavaScript/TypeScript developers.

## Module System

### Imports

```typescript
// Standard library imports (std: prefix)
import logger from "std:logger";
import math from "std:math";
import { TcpListener, TcpStream } from "std:net";
import * as Reflect from "std:reflect";

// Local module imports
import { MyClass } from "./lib.raya";
import utils from "../utils/helpers.raya";

// Default exports
import path from "std:path";  // path is the default export
```

### Exports

```typescript
// Named exports
export class MyClass { }
export function myFunction(): void { }
export const MY_CONSTANT = 42;

// Default export
export default class MainClass { }

// Re-exports
export { Something } from "./other.raya";
```

## Variables

### Declarations

```typescript
// Immutable (preferred)
const x = 42;
const name = "Alice";

// Mutable
let count = 0;
count = count + 1;
```

**Rules:**
- `const` for immutable bindings (cannot reassign)
- `let` for mutable bindings
- No `var` keyword
- Block-scoped, not function-scoped

### Type Annotations

```typescript
const x: int = 42;
const y: number = 3.14;
const name: string = "Bob";
const flag: boolean = true;
const nullable: string | null = null;
```

## Primitive Types

| Type | Description | Example |
|------|-------------|---------|
| `int` | 32-bit signed integer | `42`, `-10` |
| `number` | 64-bit float (double) | `3.14`, `2.5` |
| `string` | UTF-8 string | `"hello"` |
| `boolean` | True/false | `true`, `false` |
| `null` | Null value | `null` |

## Functions

### Function Declarations

```typescript
function add(a: int, b: int): int {
  return a + b;
}

function greet(name: string): void {
  logger.info("Hello,", name);
}
```

### Arrow Functions

```typescript
const double = (x: int): int => x * 2;

const greet = (name: string): void => {
  logger.info("Hello,", name);
};
```

### Generic Functions

```typescript
function identity<T>(x: T): T {
  return x;
}

const num = identity<int>(42);
const str = identity<string>("hi");
```

### Rest Parameters

```typescript
function sum(...nums: int[]): int {
  let total = 0;
  for (let i = 0; i < nums.length; i = i + 1) {
    total = total + nums[i];
  }
  return total;
}

sum(1, 2, 3, 4);  // 10
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

## Classes

### Basic Class

```typescript
class Point {
  x: number;
  y: number;
  
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  
  distance(): number {
    return math.sqrt(this.x * this.x + this.y * this.y);
  }
}

const p = new Point(3, 4);
logger.info(p.distance());  // 5
```

### Inheritance

```typescript
class Animal {
  name: string;
  
  constructor(name: string) {
    this.name = name;
  }
  
  speak(): void {
    logger.info(this.name, "makes a sound");
  }
}

class Dog extends Animal {
  breed: string;
  
  constructor(name: string, breed: string) {
    super(name);
    this.breed = breed;
  }
  
  speak(): void {
    logger.info(this.name, "barks");
  }
}
```

### Interfaces

```typescript
interface Drawable {
  draw(): void;
  getPosition(): { x: number; y: number };
}

class Circle implements Drawable {
  x: number;
  y: number;
  radius: number;
  
  draw(): void {
    logger.info("Drawing circle at", this.x, this.y);
  }
  
  getPosition(): { x: number; y: number } {
    return { x: this.x, y: this.y };
  }
}
```

### Generic Classes

```typescript
class Box<T> {
  value: T;
  
  constructor(value: T) {
    this.value = value;
  }
  
  get(): T {
    return this.value;
  }
  
  set(value: T): void {
    this.value = value;
  }
}

const intBox = new Box<int>(42);
const strBox = new Box<string>("hello");
```

## Control Flow

### If Statements

```typescript
if (x > 0) {
  logger.info("Positive");
} else if (x < 0) {
  logger.info("Negative");
} else {
  logger.info("Zero");
}
```

### For Loops

```typescript
// C-style for loop
for (let i = 0; i < 10; i = i + 1) {
  logger.info(i);
}

// For-of loop (arrays)
const arr = [1, 2, 3, 4, 5];
for (const item of arr) {
  logger.info(item);
}
```

### While Loops

```typescript
let i = 0;
while (i < 10) {
  logger.info(i);
  i = i + 1;
}
```

### Break and Continue

```typescript
for (let i = 0; i < 100; i = i + 1) {
  if (i == 50) break;           // Exit loop
  if (i % 2 == 0) continue;     // Skip even numbers
  logger.info(i);
}
```

## Type Definitions

### Type Aliases

```typescript
type UserId = int;
type Callback = (x: int) => void;
type Point = { x: number; y: number };
```

### Discriminated Unions

```typescript
type Result<T> =
  | { status: "ok"; value: T }
  | { status: "error"; error: string };

function process(r: Result<int>): void {
  if (r.status == "ok") {
    logger.info("Success:", r.value);
  } else {
    logger.error("Error:", r.error);
  }
}
```

### Interfaces

```typescript
interface User {
  id: int;
  name: string;
  email: string;
}

interface AdminUser extends User {
  permissions: string[];
}
```

## Arrays

```typescript
// Array literals
const nums: int[] = [1, 2, 3, 4, 5];
const strs: string[] = ["a", "b", "c"];

// Array methods
nums.push(6);              // Add to end
const last = nums.pop();   // Remove from end
const len = nums.length;   // Get length

// Indexing
const first = nums[0];
nums[1] = 10;
```

## Objects

```typescript
// Object literals
const user = {
  id: 1,
  name: "Alice",
  email: "alice@example.com"
};

// Property access
logger.info(user.name);
logger.info(user["email"]);

// Property assignment
user.name = "Bob";
```

## Operators

### Arithmetic

```typescript
const a = 10 + 5;   // Addition
const b = 10 - 5;   // Subtraction
const c = 10 * 5;   // Multiplication
const d = 10 / 5;   // Division
const e = 10 % 3;   // Modulo
```

### Comparison

```typescript
x == y    // Equality
x != y    // Inequality
x < y     // Less than
x <= y    // Less than or equal
x > y     // Greater than
x >= y    // Greater than or equal
```

### Logical

```typescript
a && b    // Logical AND
a || b    // Logical OR
!a        // Logical NOT
```

### Nullish Coalescing

```typescript
const result = value ?? defaultValue;  // Use defaultValue if value is null
```

## Error Handling

### Try-Catch

```typescript
try {
  const result = riskyOperation();
  logger.info("Success:", result);
} catch (e) {
  logger.error("Error:", e.message);
}
```

### Throw

```typescript
function divide(a: int, b: int): int {
  if (b == 0) {
    throw new Error("Division by zero");
  }
  return a / b;
}
```

## Comments

```typescript
// Single-line comment

/*
 * Multi-line comment
 */

/**
 * Documentation comment
 * @param x The input value
 * @returns The doubled value
 */
function double(x: int): int {
  return x * 2;
}
```

## Special Features

### Decorators

```typescript
@Route("/users")
class UserController {
  @Get()
  @Validate("id", "int")
  getUser(@Param("id") id: int): User {
    return findUser(id);
  }
}
```

See [Decorators Documentation](https://rizqme.github.io/raya/metaprogramming/decorators) for details.

### Reflection

```typescript
import * as Reflect from "std:reflect";

const fields = Reflect.getClassFields(MyClass);
for (const field of fields) {
  logger.info("Field:", field.name, field.type);
}
```

See [Reflection API](../stdlib/native-ids.md) for details.

## Banned Syntax

- ❌ `var` declarations
- ❌ `with` statements
- ❌ `eval()` (except in `std:runtime`)
- ❌ `arguments` object
- ❌ `this` outside of classes
- ❌ Prototype manipulation

## Syntax Differences from TypeScript

| Feature | TypeScript | Raya |
|---------|------------|------|
| Type erasure | Yes | No (monomorphization) |
| `any` type | Allowed | Banned |
| Runtime type checks | Possible | None (compile-time only) |
| Prototype chains | Yes | No |
| `undefined` | Yes | No (use `null`) |

## Best Practices

1. **Prefer `const`** over `let` when possible
2. **Use type annotations** on function signatures
3. **Leverage type inference** for local variables
4. **Use discriminated unions** for sum types
5. **Keep functions small** and focused
6. **Use async/await** for concurrency

## Related

- [Type System](type-system.md) - Type checking rules
- [Concurrency](concurrency.md) - async/await syntax
- [Examples](examples.md) - Complete programs
