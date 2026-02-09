# Raya Language Specification (v0.5)

*A strict, clean, concurrent language inspired by TypeScript — designed to be a safe, typed subset with a modern multi-threaded runtime.*

---

## Table of Contents

1. [Vision & Goals](#1-vision--goals)
2. [Language Philosophy](#2-language-philosophy)
3. [Lexical Structure](#3-lexical-structure)
4. [Type System](#4-type-system)
5. [Variables & Constants](#5-variables--constants)
6. [Expressions](#6-expressions)
7. [Statements](#7-statements)
8. [Functions](#8-functions)
9. [Classes](#9-classes)
10. [Interfaces](#10-interfaces)
11. [Type Aliases](#11-type-aliases)
12. [Arrays & Tuples](#12-arrays--tuples)
13. [Generics](#13-generics)
14. [Concurrency Model](#14-concurrency-model)
15. [Synchronization](#15-synchronization)
16. [Module System](#16-module-system)
17. [JSX/TSX Support](#17-jsxtsx-support)
18. [Standard Library](#18-standard-library)
19. [Optional Reflection System](#19-optional-reflection-system)
20. [Banned Features](#20-banned-features)
21. [Error Handling](#21-error-handling)
22. [Memory Model](#22-memory-model)
23. [Examples](#23-examples)

---

## 1. Vision & Goals

### 1.1 Core Vision

**Raya** is a statically typed language with TypeScript syntax and semantics **where possible**, but with a:

* **Predictable runtime** — No prototype chains, clear object model
* **Safe concurrency model** — Goroutine-style tasks with mutex synchronization
* **Strong type guarantees** — Sound type system with no escape hatches
* **Minimal legacy baggage** — Remove JavaScript's problematic features

Raya is designed for **clarity, reliability, and performance**.

### 1.2 Primary Goals

* **Subset of TypeScript**
  * Every valid Raya program must also be valid TypeScript
  * Some TypeScript programs will be rejected by Raya
  * Enables gradual migration and tooling compatibility

* **Fully Static Type System**
  * **All types known at compile time** — Zero runtime type checks
  * **No type tags or RTTI** — Types erased after compilation
  * **Sound type checking** — No `any`, no unsafe casts, no escape hatches
  * **Discriminated unions** — Explicit value-based variants instead of runtime type queries

* **Goroutine-Style Async Model**
  * `async` functions always run in their own Task (green thread)
  * `await` blocks the current Task until completion
  * Multi-threaded scheduler maximizes CPU utilization

* **Deterministic, Class-Based Runtime**
  * Objects have fixed layouts determined at compile time
  * Methods resolved via vtables (virtual method tables)
  * No prototype chain manipulation
  * No runtime type introspection

* **Performance via Typing**
  * Typed bytecode enables optimization
  * Unboxed primitive values in locals and stack
  * Specialized object layouts based on class definitions
  * **Monomorphization** — Generic code specialized per concrete type

* **Simple, Understandable Memory Model**
  * Single variable reads/writes are atomic
  * Mutex provides multi-operation atomicity
  * Clear happens-before relationships

---

## 2. Language Philosophy

Raya removes ambiguity wherever possible and prioritizes safety and clarity over flexibility.

| Concept     | Rule                              | Rationale |
| ----------- | --------------------------------- | --------- |
| Syntax      | Matches TypeScript where possible | Familiarity, tooling |
| Runtime     | Well-defined object + Task model  | Predictability |
| Types       | Checked strictly, inferred safely | Correctness |
| Concurrency | Explicit, structured, safe        | Prevent data races |
| Legacy JS   | Removed                           | Simplicity |

**Design Principles:**

1. **Explicit over implicit** — Types, concurrency, and control flow are visible in code
2. **Safety over convenience** — No escape hatches that bypass type system
3. **Performance through types** — Static types enable optimization, not just checking
4. **Familiar syntax** — Leverage TypeScript syntax where semantics align

---

## 3. Lexical Structure

### 3.1 Source Code

* Source files are UTF-8 encoded
* File extension: `.raya` or `.ts` (for TypeScript compatibility)
* Line terminators: `\n`, `\r\n`, or `\r`

### 3.2 Comments

```ts
// Single-line comment

/*
 * Multi-line comment
 */
```

### 3.3 Identifiers

Identifiers follow JavaScript/TypeScript rules:

* Start with letter, `_`, or `$`
* Subsequent characters can include digits
* Case-sensitive
* Cannot be reserved keywords

**Reserved Keywords:**

```
async      await      break      case       catch      class      const
continue   debugger   default    do         else       export     extends
false      finally    for        function   if         implements import
in         instanceof interface  let        new        null       return
static     super      switch     this       throw      true       try
type       typeof     void       while      yield
```

**Future Reserved:**

```
enum       namespace  private    protected  public
```

### 3.4 Literals

#### Boolean Literals

```ts
true
false
```

#### Null Literal

```ts
null
```

#### Number Literals

```ts
42              // decimal integer
3.14            // decimal float
0x1A            // hexadecimal
0o755           // octal
0b1010          // binary
1e6             // scientific notation
1_000_000       // numeric separator (for readability)
```

All numbers are 64-bit floating point (IEEE 754 double).

#### String Literals

```ts
"double quotes"
'single quotes'
`template string with ${expression}`
```

**Escape sequences:**

```ts
"\n"   // newline
"\t"   // tab
"\r"   // carriage return
"\\"   // backslash
"\'"   // single quote
"\""   // double quote
"\u{1F600}"  // Unicode code point
```

**Template strings:**

```ts
const name = "World";
const greeting = `Hello, ${name}!`;  // "Hello, World!"
```

Template expressions must be of type `string`, `number`, or `boolean`.

### 3.5 Operators

#### Arithmetic

```
+  -  *  /  %  **  (exponentiation)
```

#### Comparison

```
==  !=  ===  !==  <  >  <=  >=
```

#### Logical

```
&&  ||  !
```

#### Bitwise

```
&  |  ^  ~  <<  >>  >>>
```

#### Assignment

```
=  +=  -=  *=  /=  %=  &=  |=  ^=  <<=  >>=  >>>=
```

#### Other

```
?:  (ternary)
??  (nullish coalescing)
```

### 3.6 Operator Precedence

From highest to lowest:

1. Member access: `.`, `[]`
2. Function call, `new`
3. Postfix: `++`, `--`
4. Prefix: `!`, `~`, `+`, `-`, `typeof`, `++`, `--`
5. Exponentiation: `**`
6. Multiplicative: `*`, `/`, `%`
7. Additive: `+`, `-`
8. Bitwise shift: `<<`, `>>`, `>>>`
9. Relational: `<`, `>`, `<=`, `>=`, `instanceof`
10. Equality: `==`, `!=`, `===`, `!==`
11. Bitwise AND: `&`
12. Bitwise XOR: `^`
13. Bitwise OR: `|`
14. Logical AND: `&&`
15. Logical OR: `||`
16. Nullish coalescing: `??`
17. Conditional: `?:`
18. Assignment: `=`, `+=`, etc.

---

## 4. Type System

### 4.1 Primitive Types

Raya has six primitive types:

#### `int`

* 32-bit signed integer
* Range: -2,147,483,648 to 2,147,483,647
* Stored unboxed in VM (efficient)
* Used for counters, indices, and whole numbers

```ts
let count: int = 42;
let index: int = 0;
let offset: int = -100;
```

#### `float`

* 64-bit IEEE 754 floating point
* Full double precision
* Special values: `Infinity`, `-Infinity`, `NaN`
* Used for decimal numbers and large integers

```ts
let pi: float = 3.14159;
let huge: float = 1e100;
let temperature: float = 98.6;
```

#### `number`

* Type alias for `int | float`
* Accepts both integers and floats
* Provides TypeScript/JavaScript compatibility
* The compiler infers the specific type when possible

```ts
let value: number = 42;        // Inferred as int
let ratio: number = 3.14;      // Inferred as float
let result: number = getValue(); // Could be either

// Type narrowing with typeof
function double(x: number): number {
  if (typeof x === "int") {
    return x * 2;  // x is int, result is int
  } else {
    return x * 2;  // x is float, result is float
  }
}
```

**Numeric Literal Inference:**

```ts
42          // Inferred as int
3.14        // Inferred as float
1e6         // Inferred as float
0x1A        // Inferred as int
2_147_483_647   // Inferred as int (max i32)
2_147_483_648   // Inferred as float (exceeds i32)
```

**Type Conversions:**

```ts
let i: int = 42;
let f: float = i;  // Implicit conversion: int → float

let f2: float = 3.14;
let i2: int = f2;  // ERROR: No implicit float → int conversion

// Explicit conversion with built-in functions
let i3: int = Math.floor(f2);     // floor/ceil/round/trunc
let f3: float = float(i);          // Explicit cast
```

#### `boolean`

* Two values: `true` and `false`

```ts
let isActive: boolean = true;
let isComplete: boolean = false;
```

#### `string`

* UTF-16 encoded text
* Immutable
* Arbitrary length

```ts
let name: string = "Alice";
let message: string = `Hello, ${name}`;
```

#### `null`

* Single value: `null`
* Represents intentional absence of value

```ts
let value: string | null = null;
```

**Note:** There is no `undefined` type in Raya. Use `null` for absent values.

### 4.2 Composite Types

#### Arrays

```ts
let numbers: number[] = [1, 2, 3];
let names: string[] = ["Alice", "Bob"];
let matrix: number[][] = [[1, 2], [3, 4]];
```

#### Tuples

```ts
let pair: [number, string] = [42, "answer"];
let triple: [string, number, boolean] = ["test", 123, true];
```

Tuples have fixed length and per-position types.

#### Object Types (Interfaces)

```ts
interface Point {
  x: number;
  y: number;
}

let p: Point = { x: 10, y: 20 };
```

#### Class Types

```ts
class User {
  constructor(public name: string, public age: number) {}
}

let user: User = new User("Alice", 30);
```

### 4.3 Union Types

Union types represent values that can be one of several types:

```ts
type StringOrNumber = string | number;
type Result = Success | Failure;
```

Raya supports two patterns for union types:

1. **Bare Primitive Unions** — Use `typeof` for type narrowing (simple primitives only)
2. **Discriminated Unions** — Explicit discriminant fields (for complex types)

#### Bare Primitive Unions (typeof Operator)

For **primitive types only** (`int`, `float`, `number`, `string`, `boolean`, `null`), you can write bare unions and use `typeof` for type narrowing:

```ts
type ID = string | int;

let id: ID = 42;  // OK
id = "abc";       // OK

// Type narrowing with typeof
if (typeof id === "int") {
  logger.info(id + 1);  // id is narrowed to int
} else {
  logger.info(id.toUpperCase());  // id is narrowed to string
}
```

**How it works:**

1. Bare unions store values **directly** without boxing
2. Use `typeof` operator for runtime type checking
3. Compiler performs **control flow-based type narrowing**
4. Exhaustiveness checking ensures all cases are handled

**Supported bare unions:**
- `int | float` (equivalent to `number`)
- `string | int`
- `string | float`
- `string | number`
- `int | boolean`
- `float | boolean`
- Any combination with `null` (e.g., `int | null`, `string | null`)

**Limitations:**
- Only primitive types (no objects, arrays, or classes)
- For complex types, use discriminated unions

**Benefits:**
- ✅ Familiar syntax (matches JavaScript/TypeScript)
- ✅ No memory overhead (values stored directly)
- ✅ Better performance (no boxing/unboxing)
- ✅ Natural type narrowing with control flow
- ✅ Exhaustiveness checking at compile time

**typeof Operator:**

The `typeof` operator returns a string indicating the type:

```ts
typeof 42           // "int"
typeof 3.14         // "float"
typeof "hello"      // "string"
typeof true         // "boolean"
typeof null         // "null"
typeof { x: 1 }     // "object"
typeof [1, 2, 3]    // "object"
typeof (() => {})   // "function"
```

**Type Narrowing Examples:**

```ts
// Simple if/else with number union
function process(value: string | number): string {
  if (typeof value === "int") {
    return value.toString();  // value is int
  } else if (typeof value === "float") {
    return value.toFixed(2);  // value is float
  } else {
    return value.toUpperCase();  // value is string
  }
}

// Switch statement with all primitives
function describe(v: int | float | string | boolean): string {
  switch (typeof v) {
    case "int":
      return `Int: ${v}`;
    case "float":
      return `Float: ${v.toFixed(2)}`;
    case "string":
      return `String: "${v}"`;
    case "boolean":
      return `Boolean: ${v}`;
  }
  // Compiler enforces exhaustiveness
}

// Nullable types
function greet(name: string | null): string {
  if (typeof name === "string") {
    return `Hello, ${name}!`;
  }
  return "Hello, stranger!";
}

// Early return pattern with number
function processNumber(input: string | number): float {
  if (typeof input === "int") {
    return float(input);  // Convert int to float
  } else if (typeof input === "float") {
    return input;
  }
  // input is narrowed to string here
  return parseFloat(input);
}

// Working with int | float (number type)
function double(x: number): number {
  if (typeof x === "int") {
    return x * 2;  // int * int = int
  } else {
    return x * 2.0;  // float * float = float
  }
}
```

**Runtime Representation:**

Values are stored directly without boxing:

```ts
type ID = string | number;
let id: ID = 42;
// Runtime: Value::i32(42) - stored inline, no wrapper
```

**Performance Characteristics:**
- ✅ Zero memory overhead (no boxing)
- ✅ Direct value storage (no indirection)
- ✅ Fast type checks (single opcode)
- ✅ No allocations for type narrowing

#### Discriminated Unions (Explicit Pattern)

For **complex types** (objects, classes, arrays), use **discriminated unions** with explicit discriminant fields:

```ts
type Value =
  | { kind: "string"; value: string }
  | { kind: "number"; value: number };

function handle(v: Value): void {
  switch (v.kind) {
    case "string":
      logger.info(v.value.toUpperCase());
      break;
    case "number":
      logger.info(v.value.toFixed(2));
      break;
  }
}
```

**Key Requirements:**
* Every variant must have a common discriminant field (`kind`, `type`, `tag`, etc.)
* Discriminant must use literal types (`"string"`, `"number"`, etc.)
* Pattern matching via `switch` or `if` statements on discriminant
* Compiler performs exhaustiveness checking

#### Why Discriminated Unions?

**Benefits over runtime type checks:**

1. **Compile-Time Safety** — Type errors caught during compilation, not at runtime
2. **Exhaustiveness Checking** — Compiler ensures all cases are handled
3. **Zero Runtime Overhead** — No runtime type introspection needed
4. **Explicit Design** — Forces explicit modeling of variants
5. **Better Tooling** — IDEs can autocomplete and validate all cases
6. **No Type Confusion** — Clear separation between variants

**Example: Result Type Pattern**

```ts
type Result<T, E> =
  | { status: "ok"; value: T }
  | { status: "err"; error: E };

async function fetchUser(id: number): Task<Result<User, string>> {
  if (id > 0) {
    return { status: "ok", value: new User(id, "Alice") };
  } else {
    return { status: "err", error: "Invalid user ID" };
  }
}

async function main(): Task<void> {
  const result = await fetchUser(1);

  switch (result.status) {
    case "ok":
      logger.info(`User: ${result.value.name}`);
      break;
    case "err":
      logger.error(`Error: ${result.error}`);
      break;
  }
}
```

### 4.4 Type Inference

Raya infers types where possible:

```ts
let x = 42;              // x: number
let name = "Alice";      // name: string
let items = [1, 2, 3];   // items: number[]

function add(a: number, b: number) {
  return a + b;          // return type inferred as number
}
```

**Inference rules:**

1. Variable initializers determine type
2. Function return types inferred from return statements
3. Generic type parameters inferred from arguments
4. Array literals infer element type from elements

**When explicit types are required:**

1. Function parameters (always)
2. Class fields (always)
3. Variables without initializers
4. When inference would produce a wider type than intended

### 4.5 Type Compatibility

#### Structural Typing (Interfaces)

Interfaces use structural typing:

```ts
interface Point {
  x: number;
  y: number;
}

// This works - structural compatibility
let p: Point = { x: 1, y: 2, z: 3 };  // Extra properties OK
```

#### Nominal Typing (Classes)

Classes use nominal typing:

```ts
class Point {
  constructor(public x: number, public y: number) {}
}

class Vector {
  constructor(public x: number, public y: number) {}
}

let p: Point = new Vector(1, 2);  // ERROR: incompatible types
```

#### Type Assignability Rules

Raya supports **implicit type conversions** in specific, well-defined cases:

**1. Primitive Coercions:**

```ts
// ✅ OK: number → string (automatic conversion)
let x: number = 42;
let s: string = x;  // Implicitly converts to "42"

function fn(x: string): void {
  logger.info(x);
}
fn(42);  // OK: number coerces to string

// ❌ ERROR: string → number (no implicit conversion)
let str: string = "hello";
let num: number = str;  // ERROR: Cannot convert string to number
```

**2. Union Type Coercion:**

When assigning a union type to a target type, Raya checks if **all variants** of the union can be coerced to the target:

```ts
// ✅ OK: string | number → string
// (Both string and number can coerce to string)
let a: string | number = 42;
function fn(x: string): void { }
fn(a);  // OK: number in union coerces to string

let b: string | number = "hello";
fn(b);  // OK: string is already compatible

// ❌ ERROR: string | number → number
// (string cannot coerce to number)
let c: string | number = "hello";
function gn(x: number): void { }
gn(c);  // ERROR: Cannot convert string to number
```

**3. Subtype Widening:**

Subtypes can always be used where supertypes are expected:

```ts
// ✅ OK: Dog → Animal (subclass to superclass)
class Animal {
  eat(): void { }
}

class Dog extends Animal {
  bark(): void { }
}

function handle(animal: Animal): void {
  animal.eat();
}

let dog: Dog = new Dog();
handle(dog);  // OK: Dog is a subtype of Animal
```

**4. Structural Subtyping (Intersection Types):**

Objects with **more properties** can be assigned to types expecting **fewer properties**:

```ts
// ✅ OK: RaceCar → Car (more specific to less specific)
type Car = {
  honk(): void;
};

type RaceCar = Car & {
  speed(): void;
};

function drive(car: Car): void {
  car.honk();
}

let raceCar: RaceCar = {
  honk() { logger.info("Beep!"); },
  speed() { logger.info("Zoom!"); }
};

drive(raceCar);  // OK: RaceCar has all properties of Car
```

**5. Forbidden Conversions:**

```ts
// ❌ ERROR: string → number
let str: string = "42";
let num: number = str;  // ERROR

// ❌ ERROR: float → int (precision loss)
let f: float = 3.14;
let i: int = f;  // ERROR: Use Math.floor/ceil/round instead

// ❌ ERROR: Unrelated class types
class Cat { }
class Dog { }
let cat: Cat = new Cat();
let dog: Dog = cat;  // ERROR: Incompatible nominal types
```

**Summary of Coercion Rules:**

| From | To | Allowed? | Notes |
|------|-----|----------|-------|
| `number` | `string` | ✅ Yes | Automatic conversion |
| `int` | `float` | ✅ Yes | Safe widening |
| `string` | `number` | ❌ No | Use `parseFloat()` |
| `float` | `int` | ❌ No | Use `Math.floor()` |
| `T` | `T \| U` | ✅ Yes | Widening to union |
| `Dog` | `Animal` | ✅ Yes | Subtype to supertype |
| `RaceCar` | `Car` | ✅ Yes | Structural subtyping |
| `string \| number` | `string` | ✅ Yes | If all variants coerce |
| `string \| number` | `number` | ❌ No | string can't coerce |

### 4.6 Type Assertions

**Raya supports type casting with `as` but with strict safety guarantees.**

TypeScript's `as` syntax is **supported** with Raya-specific semantics that prevent unsound casts:

```ts
// ✅ ALLOWED: Safe upcast (widening)
let x: number = 42;
let y: number | string = x as number | string;  // Safe widening

// ✅ ALLOWED: Cast to narrower type with discriminant check
type Result = { status: "ok"; value: number } | { status: "error"; error: string };
let result: Result = getResult();

if (result.status === "ok") {
  let value: number = (result as { status: "ok"; value: number }).value;  // Safe after check
}

// ✅ ALLOWED: JSON type casts (runtime-checked)
import { JsonValue, JsonObject } from "raya:json";
let json: JsonValue = parseJson();
let obj: JsonObject = json as JsonObject;  // Runtime type check inserted

// ❌ BANNED: Unsafe downcast without runtime check
let value: unknown = getValue();
let str: string = value as string;  // ERROR: Unsound cast

// ❌ BANNED: Cast between unrelated types
let num: number = 42;
let str: string = num as string;  // ERROR: Incompatible types
```

**Raya `as` semantics:**
1. **Safe widening** — Always allowed (e.g., `T` → `T | U`)
2. **Safe narrowing with evidence** — Allowed after discriminant check
3. **JSON casts** — Runtime type check automatically inserted
4. **Unsound casts** — Compile error

**Safe alternatives for complex cases:**
```ts
// Use type guards with typeof
function asString(value: unknown): string | null {
  if (typeof value === "string") {
    return value;
  }
  return null;
}

// Use discriminated unions
type Value =
  | { kind: "string"; value: string }
  | { kind: "number"; value: number };

function processValue(v: Value): void {
  if (v.kind === "string") {
    // v is narrowed to { kind: "string"; value: string }
    logger.info(v.value.toUpperCase());
  }
}
```

### 4.7 Type Guards

**Note:** Raya does **not** support runtime `typeof` or `instanceof` operators. Instead, use compile-time type narrowing with discriminated unions and type predicates.

#### Discriminated Unions (Preferred)

```ts
type Shape =
  | { kind: "circle"; radius: number }
  | { kind: "rectangle"; width: number; height: number };

function area(shape: Shape): number {
  switch (shape.kind) {
    case "circle":
      return Math.PI * shape.radius * shape.radius;
    case "rectangle":
      return shape.width * shape.height;
  }
}
```

**Advantages:**
* Type-safe at compile time
* Exhaustiveness checking
* No runtime type checking overhead
* Clear, explicit discriminant field

#### Null Check (Allowed)

```ts
function process(value: string | null): void {
  if (value !== null) {
    logger.info(value.length);  // value: string
  }
}
```

**Note:** Null checks via `=== null` or `!== null` are the only runtime checks allowed.

#### Type Predicates (Custom Type Guards)

```ts
type Fish = { kind: "fish"; swim(): void };
type Bird = { kind: "bird"; fly(): void };
type Animal = Fish | Bird;

function isFish(animal: Animal): animal is Fish {
  return animal.kind === "fish";
}

function move(animal: Animal): void {
  if (isFish(animal)) {
    animal.swim();  // animal: Fish
  } else {
    animal.fly();   // animal: Bird
  }
}
```

**Important:** Type predicates must check discriminant fields, not use runtime type introspection.

#### Exhaustiveness Checking

```ts
type Action =
  | { type: "add"; value: number }
  | { type: "subtract"; value: number }
  | { type: "reset" };

function reducer(state: number, action: Action): number {
  switch (action.type) {
    case "add":
      return state + action.value;
    case "subtract":
      return state - action.value;
    case "reset":
      return 0;
    // Compiler ensures all cases handled
  }
}
```

---

## 5. Variables & Constants

### 5.1 Variable Declarations

#### `let` - Mutable Binding

```ts
let count: number = 0;
count = count + 1;  // OK

let name = "Alice";  // type inferred as string
name = "Bob";        // OK
```

#### `const` - Immutable Binding

```ts
const MAX_SIZE: number = 100;
MAX_SIZE = 200;  // ERROR: cannot reassign const

const config = { debug: true };
config.debug = false;  // OK - object is mutable, binding is not
config = {};           // ERROR: cannot reassign const
```

**Note:** `const` makes the *binding* immutable, not the value. For immutable values, use immutable data structures.

#### `var` - BANNED

Raya does not support `var` declarations. Use `let` or `const`.

### 5.2 Scope Rules

#### Block Scope

```ts
{
  let x = 1;
  const y = 2;
  // x and y visible here
}
// x and y not visible here
```

#### Function Scope

```ts
function example(): void {
  let x = 1;
  if (true) {
    let y = 2;
    // x and y visible here
  }
  // only x visible here
}
```

#### Module Scope

```ts
// module.raya
let moduleVar = 42;     // module-scoped
export let exported = 1; // exported to other modules
```

### 5.3 Shadowing

Inner scopes can shadow outer names:

```ts
let x = 1;
{
  let x = 2;  // shadows outer x
  logger.info(x);  // prints 2
}
logger.info(x);  // prints 1
```

### 5.4 Initialization

Variables must be initialized before use:

```ts
let x: number;
logger.info(x);  // ERROR: variable used before initialization
x = 42;
logger.info(x);  // OK
```

---

## 6. Expressions

### 6.1 Literal Expressions

```ts
42                    // number
"hello"               // string
true                  // boolean
null                  // null
[1, 2, 3]            // array
{ x: 1, y: 2 }       // object
```

### 6.2 Arithmetic Expressions

```ts
a + b      // addition
a - b      // subtraction
a * b      // multiplication
a / b      // division
a % b      // remainder
a ** b     // exponentiation
-a         // negation
+a         // unary plus
```

Type rules:
* Operands must be `number`
* Result is `number`

### 6.3 Comparison Expressions

```ts
a == b     // equality (with coercion)
a != b     // inequality (with coercion)
a === b    // strict equality
a !== b    // strict inequality
a < b      // less than
a > b      // greater than
a <= b     // less or equal
a >= b     // greater or equal
```

Type rules:
* `==`, `!=` allow comparison of compatible types
* `===`, `!==` require exact type match
* `<`, `>`, `<=`, `>=` require `number` or `string` operands
* Result is always `boolean`

### 6.4 Logical Expressions

```ts
a && b     // logical AND (short-circuit)
a || b     // logical OR (short-circuit)
!a         // logical NOT
```

Type rules:
* Operands can be any type
* Truthy/falsy semantics match JavaScript
* `&&` and `||` return one of the operands (not always boolean)
* `!` always returns `boolean`

### 6.5 Conditional Expression

```ts
condition ? trueValue : falseValue
```

Type rules:
* Condition can be any type (truthy/falsy)
* Result type is union of true and false branch types

```ts
let x = flag ? "yes" : "no";  // x: string
let y = flag ? 1 : "no";      // y: number | string
```

### 6.6 Nullish Coalescing

```ts
a ?? b     // returns b if a is null, otherwise a
```

Type rules:
* If `a` is `T | null`, result is `T | typeof b`

```ts
let name: string | null = null;
let display = name ?? "Anonymous";  // display: string
```

### 6.7 Function Call Expressions

```ts
func(arg1, arg2)
obj.method(arg)
```

Type rules:
* Arguments must match parameter types
* Result type is function return type

### 6.9 Member Access Expressions

```ts
obj.property
obj[expression]
```

Type rules:
* `.` requires property to exist on type
* `[]` requires index type compatible with array/object

### 6.10 Object Literal Expressions

```ts
let obj = {
  name: "Alice",
  age: 30,
  greet(): void {
    logger.info(`Hello, ${this.name}`);
  }
};
```

Type is inferred from properties. For interfaces:

```ts
interface Point {
  x: number;
  y: number;
}

let p: Point = { x: 1, y: 2 };
```

### 6.11 Array Literal Expressions

```ts
let numbers = [1, 2, 3];        // number[]
let mixed = [1, "two", true];   // (number | string | boolean)[]
let empty: number[] = [];       // explicit type needed
```

### 6.12 `new` Expressions

```ts
let user = new User("Alice", 30);
let arr = new Array<number>(10);
```

Type rules:
* Constructor must be a class
* Arguments must match constructor signature
* Result type is the class type

### 6.13 Runtime Type Operators

#### `typeof` Operator

**Limited support for specific use cases:**

```ts
// ✅ ALLOWED: Type narrowing with bare unions (primitives)
type ID = string | number | boolean | null;
function processID(id: ID): void {
  if (typeof id === "string") {
    logger.info(id.toUpperCase());
  } else if (typeof id === "number") {
    logger.info(id.toFixed(2));
  }
}

// ✅ ALLOWED: JSON type checking
import { JsonValue } from "raya:json";
function processJson(value: JsonValue): void {
  if (typeof value === "string") {
    // value is narrowed to string
  }
}

// ❌ BANNED: Complex object type checking
function process(value: Shape): void {
  if (typeof value === "object") {  // ERROR: Use discriminated unions
    // ...
  }
}
```

**Allowed contexts:**
* Bare union type narrowing (primitives: `string`, `number`, `boolean`, `null`)
* JSON type checking within `JsonValue` types
* Type predicates that check primitive types

**Use discriminated unions for complex types:**
```ts
type Shape =
  | { kind: "circle"; radius: number }
  | { kind: "rectangle"; width: number; height: number };

function area(shape: Shape): number {
  switch (shape.kind) {  // Check discriminant, not typeof
    case "circle":
      return Math.PI * shape.radius ** 2;
    case "rectangle":
      return shape.width * shape.height;
  }
}
```

#### `instanceof` Operator

**BANNED** — Use discriminated unions for type narrowing

**Rationale:**
* `instanceof` requires runtime type information
* Encourages explicit discriminant fields
* More performant (simple field check vs prototype chain walk)

#### `delete` Operator

**Limited support for JSON objects:**

```ts
import { JsonObject } from "raya:json";

// ✅ ALLOWED: Delete JSON object properties
function removeField(obj: JsonObject, key: string): void {
  delete obj[key];
}

// ❌ BANNED: Delete class instance properties
class Point {
  x: number = 0;
  y: number = 0;
}

let p = new Point();
delete p.x;  // ERROR: Cannot delete class properties
```

**Allowed contexts:**
* JSON object properties only
* Dynamic property removal in JSON data structures

**Rationale:**
* Class properties have fixed layouts (cannot be deleted)
* JSON objects are dynamic and may need property removal
* Clear distinction between structured classes and dynamic JSON

---

## 7. Statements

### 7.1 Expression Statements

```ts
expression;
```

Any expression can be a statement.

### 7.2 Block Statements

```ts
{
  statement1;
  statement2;
}
```

Creates a new scope.

### 7.3 Variable Declaration Statements

```ts
let x: number = 42;
const name = "Alice";
```

### 7.4 If Statements

```ts
if (condition) {
  // then branch
}

if (condition) {
  // then branch
} else {
  // else branch
}

if (condition1) {
  // branch 1
} else if (condition2) {
  // branch 2
} else {
  // else branch
}
```

### 7.5 While Loops

```ts
while (condition) {
  // body
}
```

### 7.6 Do-While Loops

```ts
do {
  // body
} while (condition);
```

### 7.7 For Loops

#### Traditional For Loop

```ts
for (let i = 0; i < 10; i++) {
  // body
}
```

#### For-Of Loop (Arrays)

```ts
for (const item of array) {
  // body
}
```

Type rules:
* Expression must be an array type `T[]`
* Loop variable has type `T`

#### For-In Loop (BANNED)

Raya does not support `for-in` loops. Use `for-of` instead.

### 7.8 Break and Continue

```ts
while (true) {
  if (condition1) break;
  if (condition2) continue;
}
```

### 7.9 Return Statements

```ts
return;              // void return
return expression;   // value return
```

Type rules:
* Return type must match function signature
* All code paths must return (if non-void)

### 7.10 Throw Statements

```ts
throw new Error("Something went wrong");
```

Type rules:
* Expression can be any type (typically Error)
* Terminates current Task

### 7.11 Try-Catch-Finally

**Note: Raya v0.5 has limited exception support.**

Currently, exceptions terminate the Task. Future versions may add try-catch.

---

## 8. Functions

### 8.1 Function Declarations

```ts
function add(a: number, b: number): number {
  return a + b;
}

function greet(name: string): void {
  logger.info(`Hello, ${name}`);
}
```

**Required annotations:**
* All parameters must have type annotations
* Return type can be inferred or explicit

### 8.2 Arrow Functions

```ts
const add = (a: number, b: number): number => a + b;

const greet = (name: string): void => {
  logger.info(`Hello, ${name}`);
};
```

### 8.3 Optional Parameters

```ts
function greet(name: string, title?: string): void {
  if (title !== null) {
    logger.info(`Hello, ${title} ${name}`);
  } else {
    logger.info(`Hello, ${name}`);
  }
}

greet("Alice");              // OK
greet("Alice", "Dr.");       // OK
```

Optional parameters have type `T | null`.

### 8.4 Default Parameters

```ts
function greet(name: string, greeting: string = "Hello"): void {
  logger.info(`${greeting}, ${name}`);
}

greet("Alice");           // "Hello, Alice"
greet("Alice", "Hi");     // "Hi, Alice"
```

### 8.5 Rest Parameters

```ts
function sum(...numbers: number[]): number {
  let total = 0;
  for (const n of numbers) {
    total += n;
  }
  return total;
}

sum(1, 2, 3);  // 6
```

Type rules:
* Must be last parameter
* Type must be an array `T[]`

### 8.6 Function Overloading - BANNED

Raya does not support function overloading. Use union types instead:

```ts
// NOT ALLOWED
function process(x: string): string;
function process(x: number): number;

// INSTEAD, use:
function process(x: string | number): string | number {
  if (typeof x === "string") {
    return x.toUpperCase();
  } else {
    return x * 2;
  }
}
```

### 8.7 Async Functions

See [Section 14: Concurrency Model](#14-concurrency-model)

---

## 9. Classes

### 9.1 Class Declarations

```ts
class Point {
  x: number;
  y: number;

  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }

  distance(): number {
    return Math.sqrt(this.x * this.x + this.y * this.y);
  }
}
```

### 9.2 Class Fields

All fields must be declared:

```ts
class User {
  name: string;      // must declare
  age: number;       // must declare

  constructor(name: string, age: number) {
    this.name = name;
    this.age = age;
  }
}
```

**Field initialization:**

```ts
class Counter {
  count: number = 0;  // initialized inline

  constructor() {
    // count already 0
  }
}
```

### 9.3 Constructor Parameter Properties

```ts
class User {
  // Shorthand - declares and initializes fields
  constructor(public name: string, public age: number) {}
}

// Equivalent to:
class User {
  name: string;
  age: number;

  constructor(name: string, age: number) {
    this.name = name;
    this.age = age;
  }
}
```

### 9.4 Methods

```ts
class Rectangle {
  constructor(public width: number, public height: number) {}

  area(): number {
    return this.width * this.height;
  }

  scale(factor: number): void {
    this.width *= factor;
    this.height *= factor;
  }
}
```

### 9.5 Static Members

```ts
class Math {
  static PI: number = 3.14159;

  static abs(x: number): number {
    return x < 0 ? -x : x;
  }
}

logger.info(Math.PI);      // 3.14159
logger.info(Math.abs(-5)); // 5
```

### 9.6 Inheritance

```ts
class Animal {
  constructor(public name: string) {}

  move(distance: number): void {
    logger.info(`${this.name} moved ${distance}m`);
  }
}

class Dog extends Animal {
  bark(): void {
    logger.info("Woof!");
  }
}

const dog = new Dog("Rex");
dog.move(10);  // inherited
dog.bark();    // own method
```

**Rules:**
* Single inheritance only
* Constructor must call `super()`
* Can override methods

### 9.7 Method Overriding

```ts
class Animal {
  move(): void {
    logger.info("Moving");
  }
}

class Bird extends Animal {
  move(): void {
    logger.info("Flying");
  }
}
```

**Rules:**
* Override must have compatible signature
* Use `super.method()` to call parent

### 9.8 Abstract Classes

Abstract classes provide base implementations with abstract methods that subclasses must implement:

```ts
abstract class Shape {
  // Abstract methods - no implementation
  abstract area(): number;
  abstract perimeter(): number;

  // Concrete method
  describe(): string {
    return `Area: ${this.area()}, Perimeter: ${this.perimeter()}`;
  }
}

class Circle extends Shape {
  constructor(public radius: number) {
    super();
  }

  area(): number {
    return Math.PI * this.radius ** 2;
  }

  perimeter(): number {
    return 2 * Math.PI * this.radius;
  }
}

class Rectangle extends Shape {
  constructor(public width: number, public height: number) {
    super();
  }

  area(): number {
    return this.width * this.height;
  }

  perimeter(): number {
    return 2 * (this.width + this.height);
  }
}

// ❌ ERROR: Cannot instantiate abstract class
let shape = new Shape();

// ✅ OK: Instantiate concrete subclass
let circle = new Circle(5);
logger.info(circle.describe());  // "Area: 78.54, Perimeter: 31.42"
```

**Rules:**
* Abstract classes cannot be instantiated directly
* Abstract methods must be implemented by concrete subclasses
* Concrete methods provide shared implementation
* Abstract classes can have fields and constructors

### 9.9 Decorators

Decorators provide a way to add metadata and modify class behavior at compile-time:

```ts
// Decorator function
function logged(target: any, propertyKey: string, descriptor: PropertyDescriptor): void {
  const original = descriptor.value;
  descriptor.value = function(...args: any[]): any {
    logger.info(`Calling ${propertyKey} with`, args);
    const result = original.apply(this, args);
    logger.info(`Result:`, result);
    return result;
  };
}

// Class decorator
function sealed(constructor: Function): void {
  Object.seal(constructor);
  Object.seal(constructor.prototype);
}

@sealed
class Calculator {
  @logged
  add(a: number, b: number): number {
    return a + b;
  }

  @logged
  multiply(a: number, b: number): number {
    return a * b;
  }
}

let calc = new Calculator();
calc.add(2, 3);  // Logs: "Calling add with [2, 3]" then "Result: 5"
```

**Decorator types:**
* **Class decorators** — Applied to class declarations
* **Method decorators** — Applied to class methods
* **Property decorators** — Applied to class properties
* **Parameter decorators** — Applied to method parameters

**Execution order:**
1. Property decorators (in order of declaration)
2. Parameter decorators (for each method)
3. Method decorators (in order of declaration)
4. Class decorators (outermost first)

**Common use cases:**
* Logging and debugging
* Validation
* Memoization/caching
* Dependency injection
* Metadata emission

### 9.10 Access Modifiers

Raya supports Java-like access modifiers for class members:

```ts
class BankAccount {
  private balance: number;
  protected accountNumber: string;
  public owner: string;

  constructor(owner: string, initialBalance: number) {
    this.owner = owner;
    this.balance = initialBalance;
    this.accountNumber = generateAccountNumber();
  }

  public deposit(amount: number): void {
    this.balance += amount;
  }

  public getBalance(): number {
    return this.balance;
  }

  private validateAmount(amount: number): boolean {
    return amount > 0 && amount <= this.balance;
  }

  protected logTransaction(type: string, amount: number): void {
    logger.info(`${type}: ${amount}`);
  }
}
```

**Visibility rules (Java-like):**

| Modifier | Same Class | Subclass | Other Classes |
|----------|------------|----------|---------------|
| `private` | ✅ | ❌ | ❌ |
| `protected` | ✅ | ✅ | ❌ |
| `public` | ✅ | ✅ | ✅ |
| (default) | ✅ | ✅ | ✅ |

**Notes:**
* Default visibility is `public` (if no modifier specified)
* Access modifiers apply to both fields and methods
* Cannot override with less restrictive visibility
* Private members are not inherited

```ts
class SavingsAccount extends BankAccount {
  private interestRate: number = 0.02;

  calculateInterest(): number {
    // ✅ Can access protected member
    this.logTransaction("interest", this.getBalance() * this.interestRate);

    // ❌ Cannot access private member
    // this.balance; // Error: 'balance' is private

    return this.getBalance() * this.interestRate;
  }
}

const account = new SavingsAccount("Alice", 1000);
account.deposit(100);       // ✅ Public method
account.owner;              // ✅ Public field
// account.balance;         // ❌ Private field
// account.accountNumber;   // ❌ Protected field
```

---

## 10. Interfaces - BANNED

**Raya does NOT support `interface` declarations. Use `type` instead.**

**Rationale:**
* Simplifies the type system (one way to do things)
* Type aliases are more flexible (support unions, primitives, tuples)
* Eliminates confusion between `interface` and `type`
* Prevents declaration merging complexity
* Aligns with modern TypeScript best practices

### Use Type Aliases Instead

```ts
// ❌ BANNED: interface
interface Point {
  x: number;
  y: number;
}

// ✅ USE: type alias
type Point = {
  x: number;
  y: number;
};
```

### Object Types with Type Aliases

```ts
// Simple object type
type User = {
  name: string;
  email?: string;  // optional property
};

// Method signatures
type Comparable = {
  compareTo(other: Comparable): number;
};

// Or using arrow syntax
type Comparable = {
  compareTo: (other: Comparable) => number;
};
```

### Extending Types with Intersection

```ts
// Combine multiple types
type Named = {
  name: string;
};

type Versioned = {
  version: number;
};

type Document = Named & Versioned & {
  content: string;
};

// Equivalent to:
// { name: string; version: number; content: string }
```

### Implementing Type Contracts

```ts
type Printable = {
  print(): void;
};

// Classes can implement type aliases
class Document implements Printable {
  constructor(public content: string) {}

  print(): void {
    logger.info(this.content);
  }
}
```

### Advantages of Type Aliases

```ts
// ✅ Unions (not possible with interfaces)
type ID = string | number;

// ✅ Primitives (not possible with interfaces)
type Name = string;

// ✅ Tuples (not possible with interfaces)
type Point2D = [number, number];

// ✅ Complex combinations
type Result<T, E> =
  | { status: "ok"; value: T }
  | { status: "error"; error: E };
```

---

## 11. Type Aliases

**Type aliases are the primary way to define custom types in Raya.**

### 11.1 Type Alias Declarations

```ts
type Id = number;
type Name = string;
type Point = { x: number; y: number };
```

**Important:** Type aliases create no runtime type. They are compile-time only.

### 11.2 Union Type Aliases

```ts
type StringOrNumber = string | number;
type Result<T> = T | null;

// Discriminated unions
type Shape =
  | { kind: "circle"; radius: number }
  | { kind: "rectangle"; width: number; height: number };
```

### 11.3 Intersection Types

```ts
type Named = { name: string };
type Aged = { age: number };

// Combine types
type Person = Named & Aged;
// Equivalent to: { name: string; age: number }

// Extend object types
type Employee = Person & {
  employeeId: number;
  department: string;
};
```

### 11.4 Generic Type Aliases

```ts
// Generic wrapper
type Box<T> = {
  value: T;
  isEmpty: boolean;
};

// Generic result type
type Result<T, E> =
  | { status: "ok"; value: T }
  | { status: "error"; error: E };

// Constrained generics
type Lengthwise<T extends { length: number }> = {
  value: T;
  length: number;
};
```

### 11.5 Why Type Aliases Only?

Raya uses only `type` declarations (no `interface`) for simplicity:

| Capability | Type Alias | Interface |
|---------|------------|-----------|
| Object shapes | ✅ | ❌ Banned |
| Union types | ✅ | ❌ N/A |
| Intersection | ✅ `&` | ❌ N/A |
| Primitive aliases | ✅ | ❌ N/A |
| Tuple types | ✅ | ❌ N/A |
| Generic types | ✅ | ❌ Banned |
| Class implementation | ✅ | ❌ Banned |

**Benefits:**
* One way to define types (simplicity)
* More flexible than interfaces
* Supports all type constructs
* No confusion between `type` and `interface`

**When to use:**
* **Interface** — For object contracts, especially when implementing
* **Type alias** — For unions, primitives, tuples, or complex types

---

## 12. Arrays & Tuples

### 12.1 Array Types

```ts
let numbers: number[] = [1, 2, 3];
let strings: Array<string> = ["a", "b", "c"];
```

Both syntaxes are equivalent. Prefer `T[]` for simplicity.

### 12.2 Array Operations

```ts
const arr = [1, 2, 3];

arr.length          // 3
arr[0]              // 1
arr[10]             // runtime error if out of bounds
arr.push(4)         // [1, 2, 3, 4]
arr.pop()           // 4, arr is [1, 2, 3]
```

**Standard array methods:**
* `push(item)` — add to end
* `pop()` — remove from end
* `shift()` — remove from start
* `unshift(item)` — add to start
* `slice(start, end)` — copy portion
* `concat(other)` — concatenate arrays
* `map(fn)` — transform elements
* `filter(fn)` — select elements
* `reduce(fn, init)` — fold/accumulate
* `find(fn)` — find first match
* `indexOf(item)` — find index

### 12.3 Multi-Dimensional Arrays

```ts
let matrix: number[][] = [
  [1, 2, 3],
  [4, 5, 6],
  [7, 8, 9]
];

logger.info(matrix[1][2]);  // 6
```

### 12.4 Tuple Types

```ts
type Pair = [number, string];
let p: Pair = [42, "answer"];

let [num, str] = p;  // destructuring
```

**Rules:**
* Fixed length
* Each position has its own type
* Indexing beyond length is compile error

### 12.5 Array vs Tuple

```ts
let arr: number[] = [1, 2, 3];
arr.push(4);  // OK, arrays are dynamic

let tuple: [number, number] = [1, 2];
tuple.push(3);  // ERROR: tuples are fixed length
```

---

## 13. Generics

### 13.1 Generic Functions

```ts
function identity<T>(value: T): T {
  return value;
}

let x = identity<number>(42);      // explicit
let y = identity("hello");          // inferred as string
```

### 13.2 Generic Classes

```ts
class Box<T> {
  constructor(public value: T) {}

  get(): T {
    return this.value;
  }

  set(value: T): void {
    this.value = value;
  }
}

let box = new Box<number>(42);
logger.info(box.get());  // 42
```

### 13.3 Generic Interfaces

```ts
interface Container<T> {
  value: T;
  getValue(): T;
}

class NumberContainer implements Container<number> {
  constructor(public value: number) {}

  getValue(): number {
    return this.value;
  }
}
```

### 13.4 Generic Type Aliases

```ts
type Result<T> = T | null;
type Pair<T, U> = [T, U];
```

### 13.5 Type Parameter Constraints

```ts
interface HasLength {
  length: number;
}

function logLength<T extends HasLength>(item: T): void {
  logger.info(item.length);
}

logLength("hello");        // OK
logLength([1, 2, 3]);      // OK
logLength(42);             // ERROR: number has no length
```

### 13.6 Multiple Type Parameters

```ts
function pair<T, U>(first: T, second: U): [T, U] {
  return [first, second];
}

let p = pair(1, "one");  // [number, string]
```

### 13.7 Monomorphization

**Raya uses monomorphization for generics** — each concrete instantiation generates specialized code.

**Example:**
```ts
function identity<T>(x: T): T {
  return x;
}

let a = identity(42);        // Compiles to identity_number
let b = identity("hello");   // Compiles to identity_string
```

**Compiler generates:**
```ts
// Generated specialized functions
function identity_number(x: number): number { return x; }
function identity_string(x: string): string { return x; }
```

**Benefits:**
1. **Zero runtime overhead** — No generic dispatch, direct function calls
2. **Type-specific optimizations** — Each variant optimized for its type
3. **No type parameters at runtime** — Types completely erased
4. **Better inlining** — Specialized code easier to inline

**Classes:**
```ts
class Box<T> {
  constructor(public value: T) {}
}

let numBox = new Box(42);      // Box_number instance
let strBox = new Box("hello"); // Box_string instance
```

Each generic class instantiation creates a distinct class at compile time.

---

## 13A. Compile-Time Type Safety

### 13A.1 Type Erasure

**All type information is erased after compilation:**

```ts
type Result<T> =
  | { status: "ok"; value: T }
  | { status: "err"; error: string };

let r: Result<number> = { status: "ok", value: 42 };
```

**At runtime:**
- No type tags on values
- No generic type parameters
- Only discriminant field `status` exists as a runtime value

### 13A.2 How Types Are Enforced

**Compile-time:**
1. **Type checking pass** — Verifies all operations are type-safe
2. **Type inference** — Deduces types where not explicitly stated
3. **Monomorphization** — Specializes generic code for each concrete type
4. **Bytecode emission** — Generates typed opcodes (IADD vs FADD vs NADD)

**Runtime:**
1. **No type checks** — VM assumes compiler verified everything
2. **Direct dispatch** — Methods called via vtables, no type queries
3. **Value checks only** — Null checks, discriminant comparisons (values, not types)

### 13A.3 What The Compiler Guarantees

After successful compilation, the VM guarantees:

✅ **Type safety** — Operations never applied to wrong types
✅ **Memory safety** — No buffer overruns, use-after-free, or dangling pointers
✅ **Null safety** — Null values explicitly typed and checked
✅ **Exhaustiveness** — All discriminated union cases handled

The compiler **rejects** programs that would require runtime type checks.

### 13A.4 Forbidden Patterns

**These patterns are compile errors:**

```ts
// BANNED - No way to check type at runtime
function process(value: unknown): void {
  if (typeof value === "string") { // ERROR: typeof banned
    // ...
  }
}

// BANNED - Unsafe cast
let x = value as string; // ERROR: unsound cast

// BANNED - Non-null assertion
let y = value!; // ERROR: bypasses null checking

// BANNED - Any type
let z: any = ...; // ERROR: any not allowed
```

**Required patterns:**

```ts
// REQUIRED - Discriminated union
type Value =
  | { kind: "string"; value: string }
  | { kind: "number"; value: number };

function process(v: Value): void {
  switch (v.kind) { // Checked at compile time
    case "string":
      logger.info(v.value); // v.value: string
      break;
    case "number":
      logger.info(v.value); // v.value: number
      break;
  }
}
```

### 13A.5 Compiler Verification

The Raya compiler performs:

1. **Type checking** — Every expression has a statically known type
2. **Exhaustiveness checking** — All discriminated union cases covered
3. **Null safety** — All nullable values checked before access
4. **Definite assignment** — Variables initialized before use
5. **Generic instantiation** — All generic types have concrete type arguments
6. **No escape hatches** — No unsafe operations that bypass type system

**If compilation succeeds, the program is type-safe.**

---

## 14. Concurrency Model

### 14.1 Task Type

The fundamental unit of concurrency in Raya is the **Task**:

```ts
interface Task<T> extends PromiseLike<T> {
  readonly id: number;
  cancel(): void;
}
```

A Task is a green thread (lightweight thread) scheduled by the VM.

### 14.2 Async Functions

Functions declared with `async` always return a `Task<T>`:

```ts
async function fetchData(): Task<string> {
  // runs in its own Task
  return "data";
}
```

**Key semantics:**
* Calling an async function **immediately** starts a new Task
* The Task runs concurrently with the caller
* Returns a `Task<T>` handle

```ts
const task = fetchData();  // Task started NOW, running concurrently
// ... do other work ...
const result = await task; // wait for Task to complete
```

### 14.3 Async Function Syntax

```ts
// Function declaration
async function compute(): Task<number> {
  return 42;
}

// Arrow function
const compute = async (): Task<number> => {
  return 42;
};

// Method
class Worker {
  async doWork(): Task<void> {
    // work here
  }
}
```

### 14.4 Await Expression

The `await` keyword suspends the current Task until another Task completes:

```ts
async function main(): Task<void> {
  const task = compute();  // start Task
  // Task is now running concurrently
  const result = await task;  // suspend until complete
  logger.info(result);
}
```

**Semantics:**
* `await` can only be used inside `async` functions
* Suspends current Task (does not block OS thread)
* Resumes when awaited Task completes
* May resume on a different OS thread

### 14.5 Async Call Expression

The `async` keyword can be used as a prefix operator to explicitly wrap any function call in a Task:

```ts
// Non-async function
function compute(x: number): number {
  return x * 2;
}

// Wrap in Task explicitly
const task = async compute(42);  // Creates Task<number>
const result = await task;       // result: number = 84
```

**Semantics:**
* `async foo()` immediately creates and starts a new Task that executes `foo()`
* If `foo()` is already an async function, this has no additional effect (both create Tasks)
* If `foo()` is non-async, this converts it to run in a Task
* The `async` prefix binds to the entire call expression: `async obj.method(args)`

**Use Cases:**

```ts
// 1. Convert synchronous function to async
function heavyComputation(data: number[]): number {
  // CPU-intensive work
  return data.reduce((a, b) => a + b, 0);
}

async function processData(): Task<void> {
  const data = [1, 2, 3, 4, 5];

  // Run synchronous function in separate Task
  const task = async heavyComputation(data);

  // Do other work while computation runs
  logger.info("Computing...");

  const result = await task;
  logger.info(result);
}

// 2. Explicit parallelization
async function parallelSum(): Task<number> {
  const data1 = [1, 2, 3];
  const data2 = [4, 5, 6];
  const data3 = [7, 8, 9];

  // Run three synchronous computations in parallel Tasks
  const t1 = async sum(data1);
  const t2 = async sum(data2);
  const t3 = async sum(data3);

  // Wait for all to complete
  return (await t1) + (await t2) + (await t3);
}

// 3. Method calls
class Calculator {
  compute(x: number): number {
    return x * x;
  }
}

const calc = new Calculator();
const task = async calc.compute(10);  // Task<number>
```

**Comparison with `await`:**

| Expression | Effect | Result Type |
|------------|--------|-------------|
| `foo()` | Execute synchronously | `T` |
| `async foo()` | Create Task, execute concurrently | `Task<T>` |
| `await foo()` | Execute and wait (if `foo` is async) | `T` |
| `await async foo()` | Create Task, immediately wait | `T` |

**Note:** `async foo()` is an expression, not a statement. It can be used anywhere an expression is valid:

```ts
// As function argument
processTask(async compute(42));

// In binary expression
const sum = (await async compute(10)) + (await async compute(20));

// In array
const tasks = [async fn1(), async fn2(), async fn3()];
```

### 14.6 Task Lifecycle

```ts
async function worker(): Task<number> {
  return 42;
}

async function main(): Task<void> {
  // 1. Create Task
  const task = worker();

  // Task is now RUNNING concurrently

  // 2. Wait for completion
  const result = await task;

  // Task is now COMPLETED
  logger.info(result);
}
```

**States:**
* `NEW` — Just created
* `READY` — Queued for execution
* `RUNNING` — Executing on a worker thread
* `BLOCKED` — Waiting for another Task or mutex
* `COMPLETED` — Finished successfully
* `FAILED` — Terminated with error

### 14.7 Task Composition

```ts
async function fetchUser(id: number): Task<User> {
  // fetch user
}

async function fetchPosts(userId: number): Task<Post[]> {
  // fetch posts
}

async function getUserWithPosts(id: number): Task<[User, Post[]]> {
  const user = await fetchUser(id);
  const posts = await fetchPosts(user.id);
  return [user, posts];
}
```

### 14.8 Concurrent Task Execution

```ts
async function main(): Task<void> {
  // Start all Tasks concurrently
  const task1 = fetchData(1);
  const task2 = fetchData(2);
  const task3 = fetchData(3);

  // Wait for all to complete
  const result1 = await task1;
  const result2 = await task2;
  const result3 = await task3;
}
```

### 14.9 Task Cancellation

```ts
async function longRunningTask(): Task<void> {
  // work...
}

async function main(): Task<void> {
  const task = longRunningTask();

  // Cancel if needed
  task.cancel();

  // Note: cancellation is cooperative
  // Task must check and respond to cancellation
}
```

### 14.10 Task vs Promise

Raya Tasks are similar to Promises but with key differences:

| Feature | Task | Promise (JS) |
|---------|------|--------------|
| Execution | Starts immediately | Starts immediately |
| Scheduled on | VM thread pool | Event loop |
| Parallelism | True parallel (multi-core) | Concurrent (single-threaded) |
| Cancellable | Yes | No (standard) |
| Type | `Task<T>` | `Promise<T>` |

Tasks implement `PromiseLike<T>` for compatibility with async/await syntax.

---

## 15. Synchronization

### 15.1 Data Races

Without synchronization, concurrent Tasks can race:

```ts
let counter = 0;

async function increment(): Task<void> {
  counter = counter + 1;  // RACE: multiple Tasks may read/write
}

async function main(): Task<void> {
  const tasks: Task<void>[] = [];
  for (let i = 0; i < 1000; i++) {
    tasks.push(increment());
  }
  for (const t of tasks) await t;
  logger.info(counter);  // May not be 1000!
}
```

### 15.2 Mutex

Raya provides `Mutex` for mutual exclusion:

```ts
class Mutex {
  lock(): void;
  unlock(): void;
}
```

**Creating a mutex:**

```ts
const mu = new Mutex();
```

**Using a mutex:**

```ts
mu.lock();
try {
  // critical section
  counter = counter + 1;
} finally {
  mu.unlock();
}
```

### 15.3 Mutex Semantics

**Lock acquisition:**
* If unlocked: acquire immediately, Task continues
* If locked by another Task: current Task blocks, yields to scheduler

**Lock release:**
* Must be called by the Task that acquired the lock
* If other Tasks are waiting: one is woken and acquires the lock
* If no waiters: mutex becomes unlocked

### 15.4 Correct Mutex Usage

```ts
let counter = 0;
const mu = new Mutex();

async function increment(): Task<void> {
  mu.lock();
  counter = counter + 1;
  mu.unlock();
}

async function main(): Task<void> {
  const tasks: Task<void>[] = [];
  for (let i = 0; i < 1000; i++) {
    tasks.push(increment());
  }
  for (const t of tasks) await t;
  logger.info(counter);  // Always 1000
}
```

### 15.5 Await in Critical Section - FORBIDDEN

**Critical rule:** You **cannot** `await` while holding a mutex.

```ts
// FORBIDDEN - will not compile
async function bad(): Task<void> {
  mu.lock();
  const result = await otherTask();  // ERROR: await while locked
  mu.unlock();
}
```

**Reason:** Prevents deadlocks. If a Task suspends while holding a lock, it may never wake up.

**Workaround:** Release lock before await:

```ts
async function good(): Task<void> {
  mu.lock();
  const temp = sharedData;
  mu.unlock();

  const result = await process(temp);  // OK: not locked

  mu.lock();
  sharedData = result;
  mu.unlock();
}
```

### 15.6 Atomic Operations

Single reads and writes of word-sized values are atomic:

```ts
let flag: boolean = false;  // atomic read/write

// Task 1
flag = true;  // atomic write

// Task 2
if (flag) {   // atomic read
  // ...
}
```

**Guaranteed atomic:**
* Read or write of `number`, `boolean`, `string`, `null`, object references

**Not atomic (requires mutex):**
* Read-modify-write (e.g., `x = x + 1`)
* Multiple operations
* Non-word-sized data

### 15.7 Memory Model

Raya follows a simple happens-before memory model:

1. **Sequential consistency within a Task** — operations within a Task execute in order
2. **Task creation happens-before Task execution** — starting a Task happens-before its first instruction
3. **Task completion happens-before await returns** — all Task writes visible to awaiter
4. **Lock acquisition happens-before lock release** — acquiring a lock sees all writes from previous holder
5. **Unlock happens-before subsequent lock** — releasing a lock makes all writes visible to next acquirer

---

## 16. Module System

### 16.1 Module Basics

Each `.raya` file is a module. Modules are statically analyzed and linked.

### 16.2 Named Exports

```ts
// math.raya
export function add(a: number, b: number): number {
  return a + b;
}

export const PI = 3.14159;

export class Calculator {
  // ...
}
```

### 16.3 Named Imports

```ts
// main.raya
import { add, PI } from "./math";

logger.info(add(1, 2));
logger.info(PI);
```

### 16.4 Namespace Imports

```ts
import * as Math from "./math";

logger.info(Math.add(1, 2));
logger.info(Math.PI);
```

### 16.5 Export Lists

```ts
function helper() {}
export function publicFunc() {}

export { helper as utilityHelper };
```

### 16.6 Re-Exports

```ts
export { add, subtract } from "./math";
export * from "./utils";
```

### 16.7 Default Exports - BANNED

Raya does **not** support default exports:

```ts
// NOT ALLOWED
export default function() {}
export default class {}
```

Use named exports instead.

### 16.8 Module Resolution

Raya uses a well-defined module resolution algorithm:

#### 1. Standard Library Modules (Resolved First)

```ts
import { match } from "raya:std";              // Built-in standard library
import { JSON } from "raya:json";              // Built-in JSON support
import { JsonValue } from "raya:json/internal"; // Internal JSON utilities
import * as Reflect from "raya:reflect";       // Reflection API
```

Standard library modules are always resolved first, before any user code.

#### 2. Relative Imports

```ts
import { foo } from "./sibling";      // ./sibling.raya (same directory)
import { bar } from "../parent";      // ../parent.raya (parent directory)
import { baz } from "./dir/module";   // ./dir/module.raya (subdirectory)
```

- Resolved relative to the importing file's location
- Must have `.raya` extension in filesystem
- `.raya` extension is optional in import statement (compiler adds it)

#### 3. Absolute/Package Imports

```ts
import { Component } from "ui/button";  // Package import
```

**Resolution order:**
1. `./node_modules/ui/button.raya`
2. `../node_modules/ui/button.raya`
3. (Continue up directory tree)
4. Directories in `RAYA_PATH` environment variable

#### 4. Circular Dependencies

Circular dependencies result in a **compile error**:

```ts
// a.raya
import { b } from "./b";  // ERROR: Circular dependency

// b.raya
import { a } from "./a";  // detected during compilation
```

**Reason:** Simplifies module initialization and prevents runtime issues.

**Workaround:** Refactor shared code into a third module:

```ts
// shared.raya
export const common = "shared";

// a.raya
import { common } from "./shared";

// b.raya
import { common } from "./shared";
```

### 16.9 Module Initialization

Modules are initialized in dependency order (topological sort):

1. All imports are resolved at compile time
2. Modules are initialized bottom-up (dependencies first)
3. Each module is initialized exactly once
4. Initialization is single-threaded and deterministic

---

## 17. JSX/TSX Support

Raya supports JSX syntax for declarative UI construction, similar to React and other UI frameworks. JSX expressions compile to function calls at compile time.

### 17.1 JSX Elements

Basic JSX element syntax:

```tsx
const element = <div className="container">Hello World</div>;
```

Compiles to:

```ts
const element = createElement("div", { className: "container" }, "Hello World");
```

### 17.2 JSX Fragments

Fragments allow grouping multiple elements without a wrapper:

```tsx
const fragment = (
  <>
    <h1>Title</h1>
    <p>Content</p>
  </>
);
```

Compiles to:

```ts
const fragment = createElement(Fragment, null,
  createElement("h1", null, "Title"),
  createElement("p", null, "Content")
);
```

### 17.3 JSX Expressions

JavaScript expressions can be embedded with `{}`:

```tsx
const name = "World";
const greeting = <div>Hello {name}!</div>;

const count = 5;
const message = <p>You have {count > 0 ? count : "no"} items</p>;
```

### 17.4 JSX Attributes

Attributes use camelCase naming:

```tsx
<div
  className="box"
  id="main"
  style={{ color: "red", fontSize: 14 }}
  onClick={handleClick}
  data-value={42}
/>
```

### 17.5 Spread Attributes

Object spread in attributes:

```tsx
const props = { className: "box", id: "main" };
const element = <div {...props} />;
```

Compiles to:

```ts
const props = { className: "box", id: "main" };
const element = createElement("div", props);
```

### 17.6 JSX Children

Children can be text, expressions, or nested elements:

```tsx
const complex = (
  <div>
    <h1>{title}</h1>
    {items.map(item => <Item key={item.id} data={item} />)}
    <footer>End</footer>
  </div>
);
```

### 17.7 Self-Closing Tags

Elements without children use self-closing syntax:

```tsx
<img src="photo.jpg" />
<br />
<Component prop={value} />
```

### 17.8 Component Elements

Components are referenced by capitalized identifiers:

```tsx
// Built-in elements (lowercase)
<div />
<span />

// Component elements (capitalized)
<MyComponent />
<UserProfile user={currentUser} />
```

### 17.9 Member Expression Components

Namespace-style component references:

```tsx
<UI.Button variant="primary" />
<Form.Input type="text" />
```

### 17.10 JSX Type Checking

JSX elements are type-checked like regular expressions:

```tsx
type ButtonProps = {
  label: string;
  onClick: () => void;
};

function Button(props: ButtonProps) {
  return <button onClick={props.onClick}>{props.label}</button>;
}

// Type error: missing 'onClick'
const btn = <Button label="Click" />;  // ERROR

// Correct
const btn = <Button label="Click" onClick={() => logger.info("clicked")} />;
```

### 17.11 JSX Transformation

JSX transforms to `createElement` calls:

```tsx
// Source
<Component prop1={value1} prop2={value2}>
  <Child />
  {expression}
</Component>

// Compiled
createElement(Component,
  { prop1: value1, prop2: value2 },
  createElement(Child, null),
  expression
)
```

The `createElement` function must be in scope (typically imported from a UI framework).

### 17.12 File Extension

Files containing JSX must use `.tsx` extension:

```
component.tsx   // Contains JSX
utils.raya      // Plain Raya, no JSX
```

### 17.13 JSX in Raya vs React

**Similarities:**
- Same syntax for elements, attributes, children
- Same transformation to function calls
- Type checking for props

**Differences:**
- Raya requires explicit `createElement` import
- No React-specific features (hooks, effects, etc.)
- Framework-agnostic - works with any UI library

---

## 18. Standard Library

### 17.1 Console

```ts
logger.info(...values: any[]): void;
logger.error(...values: any[]): void;
```

### 17.2 Math

```ts
Math.abs(x: number): number;
Math.floor(x: number): number;
Math.ceil(x: number): number;
Math.round(x: number): number;
Math.sqrt(x: number): number;
Math.pow(base: number, exp: number): number;
Math.random(): number;  // [0, 1)

Math.PI: number;
Math.E: number;
```

### 17.3 String Methods

```ts
str.length: number;
str.charAt(index: number): string;
str.substring(start: number, end?: number): string;
str.indexOf(search: string): number;
str.toUpperCase(): string;
str.toLowerCase(): string;
str.split(separator: string): string[];
```

### 17.4 Array Methods

See [Section 12.2](#122-array-operations)

### 17.5 Task Utilities

```ts
async function sleep(ms: number): Task<void>;
async function all<T>(tasks: Task<T>[]): Task<T[]>;
async function race<T>(tasks: Task<T>[]): Task<T>;
```

### 17.6 Pattern Matching Utility

The `match()` function provides elegant pattern matching for all union types:

```ts
import { match } from "raya:std";

// Bare primitive unions
type ID = string | number;
const id: ID = 42;

const desc = match(id, {
  string: (s) => `String ID: ${s}`,
  number: (n) => `Numeric ID: ${n}`
});

// Discriminated unions
type Result<T> =
  | { status: "ok"; value: T }
  | { status: "err"; error: string };

const message = match(result, {
  ok: (r) => `Success: ${r.value}`,
  err: (r) => `Error: ${r.error}`
});
```

**Type Signature:**

```ts
function match<T, R>(
  value: T,
  handlers: MatchHandlers<T, R>
): R;
```

**How it works:**

1. **For bare primitive unions** (`string | number`):
   - Keys are type names: `"string"`, `"number"`, `"boolean"`, `"null"`
   - Compiler unwraps internal `{ $type, $value }` representation
   - Each handler receives the unwrapped primitive value

2. **For discriminated unions** (`{ status: "ok" | "err" }`):
   - Keys are discriminant values: `"ok"`, `"err"`
   - Compiler infers discriminant field using this algorithm:
     1. Find all fields with literal types that exist in ALL variants
     2. If multiple candidates exist, use this priority order:
        - `"kind"` (highest priority)
        - `"type"`
        - `"tag"`
        - `"variant"`
        - First alphabetically among remaining fields
     3. If no common field with literal types exists, compilation error
   - Each handler receives the full variant object

**Features:**

- ✅ **Type-safe** — TypeScript/Raya infers all parameter types
- ✅ **Exhaustiveness checking** — Compiler ensures all cases handled
- ✅ **Expression form** — Returns value from matched handler
- ✅ **Works everywhere** — Not limited to specific contexts

**Examples:**

```ts
import { match } from "raya:std";

// Example 1: Bare union with null
type MaybeString = string | null;
const value: MaybeString = getValue();

const result = match(value, {
  string: (s) => s.toUpperCase(),
  null: () => "DEFAULT"
});

// Example 2: Multiple primitives
type Primitive = string | number | boolean;
const prim: Primitive = true;

match(prim, {
  string: (s) => logger.info(`String: ${s}`),
  number: (n) => logger.info(`Number: ${n}`),
  boolean: (b) => logger.info(`Boolean: ${b}`)
});

// Example 3: Discriminated union
type Action =
  | { type: "increment"; by: number }
  | { type: "decrement"; by: number }
  | { type: "reset" };

const newState = match(action, {
  increment: (a) => state + a.by,
  decrement: (a) => state - a.by,
  reset: () => 0
});

// Example 4: Nested matching
type Response =
  | { status: "ok"; data: string | number }
  | { status: "error"; code: number };

match(response, {
  ok: (r) => {
    match(r.data, {
      string: (s) => logger.info(`Text: ${s}`),
      number: (n) => logger.info(`ID: ${n}`)
    });
  },
  error: (r) => logger.error(`Error ${r.code}`)
});
```

**Limitations:**

- No default case (must be exhaustive)
- Cannot match on computed values
- For partial matching, use traditional if/else

**How `match()` works with interfaces:**

```ts
// Interfaces require explicit discriminated unions
interface Dog {
  kind: "dog";  // Discriminant field required
  name: string;
  bark(): void;
}

interface Cat {
  kind: "cat";  // Discriminant field required
  name: string;
  meow(): void;
}

type Animal = Dog | Cat;

// match() works by checking the discriminant value
const animal: Animal = { kind: "dog", name: "Buddy", bark: () => {} };

match(animal, {
  dog: (a) => a.bark(),  // a is Dog
  cat: (a) => a.meow()   // a is Cat
});
```

**Key points:**

1. **Interfaces are structural** — Any object with matching shape satisfies the interface
2. **Discriminants are explicit** — You must add discriminant fields to make variants distinguishable
3. **match() checks values** — Not interface implementation, but discriminant field values
4. **No bare unions for interfaces** — Only primitives (`string | number`) get automatic transformation

**Why interfaces need discriminants:**

```ts
// WITHOUT discriminant - ambiguous at runtime
interface Point2D {
  x: number;
  y: number;
}

interface Point3D {
  x: number;
  y: number;
  z: number;
}

// ❌ Can't use bare union - how would match() know which is which?
type Point = Point2D | Point3D;  // Both satisfy Point2D structurally!

// ✅ WITH discriminant - clear at runtime
interface Point2D {
  dims: 2;  // Literal type discriminant
  x: number;
  y: number;
}

interface Point3D {
  dims: 3;  // Literal type discriminant
  x: number;
  y: number;
  z: number;
}

type Point = Point2D | Point3D;

// Now match() works!
match(point, {
  2: (p) => logger.info(`2D: (${p.x}, ${p.y})`),
  3: (p) => logger.info(`3D: (${p.x}, ${p.y}, ${p.z})`)
});
```

**Compiler's role:**

- **For primitives (`string | number`):** Compiler auto-generates `{ $type, $value }` wrapper
- **For interfaces:** Compiler requires explicit discriminant fields (compile error otherwise)
- **For classes:** Use discriminant fields (classes are nominal, but still need runtime distinction)

**Example combining primitives and interfaces:**

```ts
// Interface with bare primitive union inside
interface Response {
  id: string | number;  // Bare union - auto-transformed
  data: Data;           // Interface - needs discriminant if union
}

type Data =
  | { type: "text"; content: string }    // Explicit discriminant
  | { type: "binary"; buffer: ArrayBuffer };

const response: Response = /* ... */;

// Match on the primitive union
match(response.id, {
  string: (id) => logger.info(`String ID: ${id}`),
  number: (id) => logger.info(`Numeric ID: ${id}`)
});

// Match on the interface union
match(response.data, {
  text: (d) => logger.info(d.content),
  binary: (d) => logger.info(d.buffer.byteLength)
});
```

**Summary:**
- `match()` is **value-based**, not type-based
- Works by checking discriminant field values (strings/numbers/booleans)
- Primitives get automatic discrimination via compiler
- Interfaces/objects require explicit discriminant fields
- This maintains zero runtime overhead while providing type safety

### 17.7 JSON Serialization

Raya provides compile-time JSON encoding/decoding via code generation:

```ts
import { JSON } from "raya:json";

interface User {
  name: string;
  age: number;
  email: string | null;
}

// Encoding - compiler generates specialized encoder
const result = JSON.encode(user);  // Result<string, Error>

// Decoding - compiler generates specialized decoder
const decoded = JSON.decode<User>(jsonString);  // Result<User, Error>

// Result type
type Result<T, E> =
  | { status: "ok"; value: T }
  | { status: "error"; error: E };
```

**How It Works:**

The compiler analyzes the type structure at compile time and generates specialized encode/decode functions:

1. **For each interface/class** used with `JSON.encode()` or `JSON.decode<T>()`, the compiler generates:
   - An encoder function that converts the type to JSON
   - A decoder function that validates and converts JSON to the type

2. **Code Generation Strategy:**
   - Interface/class fields → JSON object properties
   - Arrays → JSON arrays
   - Primitives → JSON primitives
   - Union types → Requires discriminated unions for proper decoding
   - Optional fields (T | null) → Can be missing or null in JSON

3. **No Runtime Overhead:**
   - All type information used during compilation
   - Generated code is specialized for each type
   - No reflection or runtime type inspection

**Example Generated Code:**

```ts
// User source
interface User {
  name: string;
  age: number;
  email: string | null;
}

// Compiler generates (conceptually):
function __encode_User(value: User): string {
  return `{"name":"${value.name}","age":${value.age},"email":${value.email === null ? "null" : `"${value.email}"`}}`;
}

function __decode_User(input: string): Result<User, Error> {
  const json = parseJSON(input);  // Built-in parser
  if (json.kind !== "object") {
    return { status: "error", error: { message: "Expected object" } };
  }

  const name = json.value.get("name");
  if (!name || name.kind !== "string") {
    return { status: "error", error: { message: "Invalid 'name' field" } };
  }

  const age = json.value.get("age");
  if (!age || age.kind !== "number") {
    return { status: "error", error: { message: "Invalid 'age' field" } };
  }

  const email = json.value.get("email");
  let emailVal: string | null = null;
  if (email && email.kind === "string") {
    emailVal = email.value;
  }

  return {
    status: "ok",
    value: { name: name.value, age: age.value, email: emailVal }
  };
}
```

**Benefits:**

✅ **Simple API** — Single function call for encode/decode
✅ **Type-safe** — Compiler verifies types match
✅ **Zero runtime overhead** — Specialized code generated at compile time
✅ **Clear errors** — Validation errors include field paths
✅ **No reflection needed** — Pure compile-time code generation

**Limitations:**

- Only works with types known at compile time
- Cannot serialize/deserialize arbitrary runtime values
- For dynamic JSON handling, use manual decoders with discriminated unions

### 17.8 Handling Third-Party APIs Without Discriminants

**Problem:** Third-party APIs often don't use discriminated unions. For example, an API might return:

```json
{ "id": 123 }
```

or

```json
{ "id": "abc123" }
```

The JSON itself doesn't have a discriminant field like `"type"` or `"kind"`.

#### Automatic Approach (Compiler-Generated)

**The simple way:** Use bare primitive unions and the `match()` utility:

```ts
import { JSON } from "raya:json";
import { match } from "raya:std";

interface User {
  id: string | number;  // Bare union - compiler handles it!
  name: string;
}

const result = JSON.decode<User>(jsonString);

if (result.status === "ok") {
  const user = result.value;

  // Pattern match on the bare union
  const idString = match(user.id, {
    string: (id) => `String ID: ${id}`,
    number: (id) => `Numeric ID: ${id}`
  });

  logger.info(idString);
}
```

**How it works:**

1. Compiler sees `id: string | number` in interface
2. Internally transforms to: `{ $type: "string"; $value: string } | { $type: "number"; $value: number }`
3. JSON decoder inspects structure and creates appropriate variant
4. Use `match()` from `raya:std` for type narrowing (works everywhere, not just JSON)

**Inline handling:**

```ts
if (result.status === "ok") {
  const user = result.value;

  // Direct pattern matching
  match(user.id, {
    string: (id) => logger.info(`String ID: ${id}`),
    number: (id) => logger.info(`Numeric ID: ${id}`)
  });
}
```

**Benefits:**
- ✅ Go-like simplicity - just use bare unions in types
- ✅ Compiler handles the complexity
- ✅ Full type safety maintained
- ✅ Works everywhere (not just JSON)
- ✅ Uses general `match()` utility

**Limitations:**
- Only works for primitive unions (`string`, `number`, `boolean`, `null`)
- Complex unions still require manual discriminated unions

#### Manual Approach (Full Control)

**Solution:** Write a decoder that inspects the **JSON structure** and creates the appropriate discriminated union:

```ts
import { JsonValue, parseJson } from "raya:json/internal";

type UserId =
  | { kind: "numeric"; value: number }
  | { kind: "string"; value: string };

interface User {
  id: UserId;
  name: string;
}

function decodeUserId(json: JsonValue): Result<UserId, Error> {
  // Check the JSON structure (not Raya types!)
  if (json.kind === "number") {
    // JSON is a number, create numeric variant
    return {
      status: "ok",
      value: { kind: "numeric", value: json.value }
    };
  } else if (json.kind === "string") {
    // JSON is a string, create string variant
    return {
      status: "ok",
      value: { kind: "string", value: json.value }
    };
  } else {
    return {
      status: "error",
      error: { message: "id must be number or string" }
    };
  }
}

function decodeUser(json: JsonValue): Result<User, Error> {
  if (json.kind !== "object") {
    return { status: "error", error: { message: "Expected object" } };
  }

  const obj = json.value;

  // Decode id field
  const idField = obj.get("id");
  if (!idField) {
    return { status: "error", error: { message: "Missing id field" } };
  }

  const idResult = decodeUserId(idField);
  if (idResult.status !== "ok") {
    return idResult;
  }

  // Decode name field
  const nameField = obj.get("name");
  if (!nameField || nameField.kind !== "string") {
    return { status: "error", error: { message: "Invalid name field" } };
  }

  return {
    status: "ok",
    value: {
      id: idResult.value,
      name: nameField.value
    }
  };
}
```

**Usage:**

```ts
const apiResponse = '{"id": 123, "name": "Alice"}';
const result = decodeUser(parseJson(apiResponse).value);

if (result.status === "ok") {
  const user = result.value;

  // Now statically handle the discriminated union
  switch (user.id.kind) {
    case "numeric":
      logger.info(`Numeric ID: ${user.id.value}`);
      break;
    case "string":
      logger.info(`String ID: ${user.id.value}`);
      break;
  }
}
```

**Key Insight:**

The runtime checking happens **on the JSON structure**, not on Raya types:

1. **JSON has type information** — A JSON number is structurally different from a JSON string
2. **Decoder inspects JSON** — Checks `json.kind === "number"` vs `json.kind === "string"`
3. **Decoder creates discriminated union** — Based on what it finds
4. **Raya code is fully static** — Once the discriminated union is created, all handling is compile-time checked

**The boundary is the decoder:**

```
Third-Party API (dynamic JSON)
         ↓
    JSON Parser (parses to JsonValue)
         ↓
    Custom Decoder (inspects JSON structure)
         ↓
    Discriminated Union (fully static Raya type)
         ↓
    Your Code (compile-time type safety)
```

**More Complex Example:**

For APIs with nested unions:

```ts
// API might return:
// { "data": 123 } OR { "data": "text" } OR { "data": { "nested": true } }

type ApiData =
  | { type: "number"; value: number }
  | { type: "string"; value: string }
  | { type: "object"; value: Map<string, JsonValue> };

function decodeApiData(json: JsonValue): Result<ApiData, Error> {
  switch (json.kind) {
    case "number":
      return {
        status: "ok",
        value: { type: "number", value: json.value }
      };

    case "string":
      return {
        status: "ok",
        value: { type: "string", value: json.value }
      };

    case "object":
      return {
        status: "ok",
        value: { type: "object", value: json.value }
      };

    default:
      return {
        status: "error",
        error: { message: `Unexpected type: ${json.kind}` }
      };
  }
}
```

**This maintains full static type safety** because:

- The JSON structure is checked at the boundary
- The decoder transforms dynamic JSON into static discriminated unions
- All subsequent code operates on statically-known types
- The compiler verifies exhaustiveness and type correctness

---

## 19. Optional Reflection System

Raya provides an **optional reflection capability** that can be enabled at compile time. By default, Raya compiles with **zero runtime type information**. However, when reflection is enabled, the compiler emits type metadata that allows runtime type introspection.

### 18.1 Reflection Overview

Reflection metadata is always included in compiled modules:

- Type metadata embedded in bytecode
- Reflection API always available
- Enables TypeScript compatibility shims
- Supports frameworks (DI, ORM, serialization)

### 18.2 Reflection API

The `Reflect` module is always available:

```ts
// Import reflection module
import * as Reflect from "raya:reflect";

// Type information
interface TypeInfo {
  readonly kind: "primitive" | "class" | "interface" | "union" | "array" | "tuple";
  readonly name: string;
  readonly properties?: PropertyInfo[];
  readonly methods?: MethodInfo[];
  readonly constructors?: ConstructorInfo[];
}

interface PropertyInfo {
  readonly name: string;
  readonly type: TypeInfo;
  readonly isReadonly: boolean;
}

interface MethodInfo {
  readonly name: string;
  readonly parameters: ParameterInfo[];
  readonly returnType: TypeInfo;
}

interface ParameterInfo {
  readonly name: string;
  readonly type: TypeInfo;
}

interface ConstructorInfo {
  readonly parameters: ParameterInfo[];
}
```

### 18.3 Reflection Functions

```ts
// Get type information for a value
Reflect.typeOf(value: any): TypeInfo;

// Get type information for a class
Reflect.typeInfo<T>(): TypeInfo;

// Check if value is instance of class (requires reflection)
Reflect.instanceof(value: any, type: TypeInfo): boolean;

// Get all properties of an object
Reflect.getProperties(value: object): PropertyInfo[];

// Get property value by name
Reflect.getProperty(value: object, name: string): any;

// Set property value by name
Reflect.setProperty(value: object, name: string, val: any): void;

// Check if object has property
Reflect.hasProperty(value: object, name: string): boolean;

// Create instance from type info
Reflect.construct(type: TypeInfo, args: any[]): any;
```

### 18.4 Example: TypeScript Compatibility Shim

Using reflection, you can build a TypeScript compatibility layer:

```ts
// ts-compat.raya - TypeScript compatibility shim
import * as Reflect from "raya:reflect";

export function typeof(value: any): string {
  const typeInfo = Reflect.typeOf(value);

  switch (typeInfo.kind) {
    case "primitive":
      if (typeInfo.name === "number") return "number";
      if (typeInfo.name === "string") return "string";
      if (typeInfo.name === "boolean") return "boolean";
      if (typeInfo.name === "null") return "object";  // TypeScript compat
      return "undefined";
    case "class":
    case "interface":
      return "object";
    case "array":
      return "object";
    default:
      return "object";
  }
}

export function instanceof<T>(value: any, classType: T): boolean {
  const typeInfo = Reflect.typeInfo<T>();
  return Reflect.instanceof(value, typeInfo);
}
```

**Usage:**
```ts
import { typeof, instanceof } from "./ts-compat";

// Now you can use TypeScript-style type checking
let x: any = 42;
if (typeof(x) === "number") {
  logger.info("It's a number!");
}

class User {
  constructor(public name: string) {}
}

let obj: any = new User("Alice");
if (instanceof(obj, User)) {
  logger.info("It's a User!");
}
```

### 18.5 Example: Dynamic Serialization (Reflection-Based)

**Note:** For standard JSON serialization, use `JSON.encode()`/`JSON.decode<T>()` (Section 17.7) which uses compile-time code generation. This example shows reflection-based serialization for dynamic scenarios.

```ts
import * as Reflect from "raya:reflect";

export function serialize(value: any): string {
  const typeInfo = Reflect.typeOf(value);

  if (typeInfo.kind === "primitive") {
    return JSON.stringify(value);
  }

  if (typeInfo.kind === "class" || typeInfo.kind === "interface") {
    const props = Reflect.getProperties(value);
    const obj: Record<string, any> = {};

    for (const prop of props) {
      obj[prop.name] = Reflect.getProperty(value, prop.name);
    }

    return JSON.stringify(obj);
  }

  if (typeInfo.kind === "array") {
    return JSON.stringify(value);
  }

  return "null";
}

export function deserialize<T>(json: string): T {
  const typeInfo = Reflect.typeInfo<T>();
  const data = JSON.parse(json);

  if (typeInfo.kind === "class") {
    // Use reflection to construct instance
    const instance = Reflect.construct(typeInfo, []);

    // Set properties from parsed data
    for (const key in data) {
      if (Reflect.hasProperty(instance, key)) {
        Reflect.setProperty(instance, key, data[key]);
      }
    }

    return instance as T;
  }

  return data as T;
}
```

### 18.6 Example: Debugging/Inspection

```ts
import * as Reflect from "raya:reflect";

export function inspect(value: any): string {
  const typeInfo = Reflect.typeOf(value);

  let result = `Type: ${typeInfo.kind} (${typeInfo.name})\n`;

  if (typeInfo.properties) {
    result += "Properties:\n";
    for (const prop of typeInfo.properties) {
      const val = Reflect.getProperty(value, prop.name);
      result += `  ${prop.name}: ${prop.type.name} = ${val}\n`;
    }
  }

  if (typeInfo.methods) {
    result += "Methods:\n";
    for (const method of typeInfo.methods) {
      const params = method.parameters.map(p => `${p.name}: ${p.type.name}`).join(", ");
      result += `  ${method.name}(${params}): ${method.returnType.name}\n`;
    }
  }

  return result;
}
```

**Usage:**
```ts
class User {
  constructor(public name: string, public age: number) {}

  greet(): string {
    return `Hello, ${this.name}`;
  }
}

const user = new User("Alice", 30);
logger.info(inspect(user));

// Output:
// Type: class (User)
// Properties:
//   name: string = Alice
//   age: number = 30
// Methods:
//   greet(): string
```

### 18.7 Performance Implications

**With Reflection Enabled:**
- **Binary Size**: +10-30% due to embedded metadata
- **Startup Time**: +5-10% to load metadata
- **Runtime Performance**: No impact on normal code (metadata only accessed via Reflect API)
- **Memory**: +2-5% for metadata structures

**Recommendation:**
- **Production builds**: Disable reflection for maximum performance
- **Development builds**: Enable reflection for debugging and introspection
- **Interop libraries**: Enable reflection only for modules that need TypeScript compatibility

### 18.8 Limitations

Even with reflection enabled, Raya maintains its type safety guarantees:

1. **Type safety preserved** — Reflection cannot bypass type checking at compile time
2. **No dynamic code execution** — Cannot use reflection to call arbitrary methods without type checking
3. **Monomorphization still applies** — Generic types are still specialized, metadata describes monomorphized types
4. **No `any` type** — Reflection API uses `any` only at module boundary (like FFI)

### 18.9 Compile-Time vs Runtime

**Important distinction:**

| Feature | Without Reflection | With Reflection |
|---------|-------------------|-----------------|
| Type checking | Compile-time only | Compile-time only |
| Type narrowing | Discriminated unions | Discriminated unions + Reflect API |
| Binary size | Minimal | +10-30% |
| `typeof`/`instanceof` | **Banned** | Available via shim |
| Performance | Zero overhead | Reflection API has overhead |
| TypeScript compat | Via code rewrite | Via runtime shim |

---

## 20. Banned Features

### 19.1 JavaScript Runtime Features

**Not supported in Raya:**

* `eval()` — arbitrary code execution
* `with` — ambiguous scoping
* `delete` — **Exception:** Allowed for JSON object properties only
* `prototype` — prototype manipulation
* `__proto__` — prototype access
* Global `this` — implicit global object
* `arguments` — use rest parameters instead
* `var` — use `let` or `const`
* `for-in` — use `for-of` or explicit iteration
* `typeof` — **Exception:** Allowed for JSON types and bare union type narrowing
* `instanceof` — use discriminated unions instead
* Automatic semicolon insertion edge cases — always use semicolons

### 19.2 Type System Features

**Not supported in Raya:**

* `any` type — unsafe type escape
* Implicit `any` — all types must be explicit or inferred soundly
* Non-null assertion (`!`) — unsafe null bypass
* `as` casting — **Exception:** Supported with Raya-specific semantics (safe casts only)
* `satisfies` — not needed with sound inference
* Index signatures (`[key: string]: T`) — use `Map<K, V>` instead
* Function overloading — use union types
* `enum` — use union of literals instead
* `namespace` — use modules
* **`interface` declarations** — use `type` aliases instead (LANG.md §10)

### 18.3 Module Features

**Not supported in Raya:**

* `export default` — use named exports
* CommonJS (`require`, `module.exports`) — use ES modules
* Dynamic imports (`import()`) — not in v0.5
* `export =` — TypeScript legacy syntax

### 19.4 Advanced TypeScript Features

**Supported in Raya:**

* **Decorators** — supported with Raya semantics
* **Abstract classes** — supported for class inheritance patterns
* **Generics** — full support with monomorphization

**Not in v0.5:**

* Conditional types — advanced type-level programming
* Mapped types — advanced type transformations
* Template literal types — string template types
* Mixins — complex composition patterns

---

## 21. Error Handling

### 20.1 Runtime Errors

Runtime errors terminate the current Task:

```ts
async function worker(): Task<void> {
  throw new Error("something went wrong");  // Task terminates
}

async function main(): Task<void> {
  const task = worker();
  const result = await task;  // propagates error to awaiter
}
```

### 19.2 Error Propagation

Errors propagate through `await`:

```ts
async function a(): Task<void> {
  throw new Error("error in a");
}

async function b(): Task<void> {
  await a();  // error propagates here
}

async function c(): Task<void> {
  await b();  // error propagates here
}
```

### 19.3 Try-Catch-Finally

Raya supports exception handling with try-catch-finally blocks:

```ts
// Basic try-catch
async function safe(): Task<void> {
  try {
    await riskyOperation();
  } catch (e) {
    logger.error("Caught:", e);
  }
}

// Try-finally (cleanup)
async function withCleanup(): Task<void> {
  try {
    await performWork();
  } finally {
    await cleanup();  // Always executes
  }
}

// Try-catch-finally
async function complete(): Task<void> {
  try {
    await riskyOperation();
  } catch (e) {
    logger.error("Error:", e);
  } finally {
    await cleanup();  // Always executes
  }
}
```

**Semantics:**

* **catch block** — Receives the exception as a value
* **finally block** — Always executes (even if no exception)
* **Rethrow** — Use `throw e;` in catch block to re-raise
* **Stack unwinding** — Call frames unwound to handler
* **Mutex safety** — Mutexes auto-unlock during unwinding

**Exception propagation:**

```ts
async function inner(): Task<void> {
  throw new Error("inner error");
}

async function middle(): Task<void> {
  // No try-catch, exception propagates
  await inner();
}

async function outer(): Task<void> {
  try {
    await middle();
  } catch (e) {
    // Catches exception from inner()
    logger.info("Caught:", e);
  }
}
```

**Finally guarantees:**

```ts
async function example(): Task<void> {
  try {
    if (Math.random() > 0.5) {
      return;  // Finally still runs
    }
    throw new Error("error");  // Finally still runs
  } finally {
    logger.info("Cleanup");  // Always prints
  }
}
```

**Rethrow pattern:**

```ts
async function logAndRethrow(): Task<void> {
  try {
    await riskyOperation();
  } catch (e) {
    logger.error("Operation failed:", e);
    throw e;  // Re-raise to caller
  }
}
```

**Mutex auto-unlock:**

```ts
async function safeAccess(): Task<void> {
  const mtx = new Mutex();
  mtx.lock();
  try {
    await operation();  // May throw
  } finally {
    mtx.unlock();  // Always unlocks
  }
}

// Even without finally, mutexes auto-unlock on exception
async function autoUnlock(): Task<void> {
  const mtx = new Mutex();
  mtx.lock();
  await operation();  // If throws, mutex automatically unlocks
}
```

### 19.4 Common Runtime Errors

* **Null access** — accessing property on `null`
* **Out of bounds** — array access beyond length
* **Type error** — invalid cast or type guard failure
* **Invalid mutex** — unlock without lock, wrong Task unlocking
* **Division by zero** — NaN result (not an error)

---

## 22. Memory Model

### 21.1 Garbage Collection

Raya uses automatic garbage collection:

* Objects are allocated on the heap
* GC reclaims unreachable objects
* GC uses type metadata for precise scanning

### 20.2 Object Lifetime

Objects live as long as references exist:

```ts
let obj = { x: 1 };  // object created
let ref = obj;       // second reference
obj = null;          // first reference gone
// object still alive (ref exists)
ref = null;          // last reference gone
// object eligible for GC
```

### 20.3 Value Semantics

Primitives are copied by value:

```ts
let a = 42;
let b = a;  // copy
b = 100;
logger.info(a);  // still 42
```

Objects are copied by reference:

```ts
let obj1 = { x: 1 };
let obj2 = obj1;  // reference copy
obj2.x = 100;
logger.info(obj1.x);  // 100 - same object
```

### 20.4 Memory Safety

Raya guarantees:

* No use-after-free — GC prevents
* No dangling pointers — GC prevents
* No buffer overruns — bounds checking on arrays
* No null pointer dereferences (at runtime) — null checks or errors

---

## 23. Examples

### 22.1 Discriminated Unions Example

```ts
type Result<T> =
  | { status: "success"; value: T }
  | { status: "error"; error: string };

async function fetchData(id: number): Task<Result<string>> {
  if (id > 0) {
    return { status: "success", value: "data" };
  } else {
    return { status: "error", error: "Invalid ID" };
  }
}

async function main(): Task<void> {
  const result = await fetchData(1);

  switch (result.status) {
    case "success":
      logger.info(`Got: ${result.value}`);
      break;
    case "error":
      logger.error(`Error: ${result.error}`);
      break;
  }
}
```

### 22.### 21.2 Concurrent Counter

```ts
class Counter {
  constructor(public value: number = 0) {}
}

const mu = new Mutex();
const counter = new Counter();

async function worker(id: number): Task<void> {
  for (let i = 0; i < 100; i++) {
    mu.lock();
    counter.value = counter.value + 1;
    mu.unlock();
  }
}

async function main(): Task<void> {
  const tasks: Task<void>[] = [];
  for (let i = 0; i < 10; i++) {
    tasks.push(worker(i));
  }
  for (const t of tasks) {
    await t;
  }
  logger.info(counter.value);  // 1000
}
```

### 22.### 21.3 Parallel Map

```ts
async function processItem(item: number): Task<number> {
  // Simulate work
  return item * 2;
}

async function parallelMap(items: number[]): Task<number[]> {
  const tasks: Task<number>[] = [];
  for (const item of items) {
    tasks.push(processItem(item));
  }

  const results: number[] = [];
  for (const task of tasks) {
    results.push(await task);
  }
  return results;
}

async function main(): Task<void> {
  const input = [1, 2, 3, 4, 5];
  const output = await parallelMap(input);
  logger.info(output);  // [2, 4, 6, 8, 10]
}
```

### 22.### 21.4 Producer-Consumer

```ts
class Queue<T> {
  private items: T[] = [];
  private mu = new Mutex();

  push(item: T): void {
    this.mu.lock();
    this.items.push(item);
    this.mu.unlock();
  }

  pop(): T | null {
    this.mu.lock();
    const item = this.items.shift() ?? null;
    this.mu.unlock();
    return item;
  }
}

const queue = new Queue<number>();

async function producer(id: number): Task<void> {
  for (let i = 0; i < 10; i++) {
    queue.push(id * 100 + i);
  }
}

async function consumer(id: number): Task<void> {
  for (let i = 0; i < 10; i++) {
    const item = queue.pop();
    if (item !== null) {
      logger.info(`Consumer ${id} got ${item}`);
    }
  }
}

async function main(): Task<void> {
  const producers = [producer(1), producer(2)];
  const consumers = [consumer(1), consumer(2)];

  for (const p of producers) await p;
  for (const c of consumers) await c;
}
```

### 22.### 21.5 Generic Data Structures

```ts
class Stack<T> {
  private items: T[] = [];

  push(item: T): void {
    this.items.push(item);
  }

  pop(): T | null {
    return this.items.pop() ?? null;
  }

  peek(): T | null {
    return this.items[this.items.length - 1] ?? null;
  }

  getSize(): number {
    return this.items.length;
  }
}

const stack = new Stack<number>();
stack.push(1);
stack.push(2);
logger.info(stack.pop());  // 2
logger.info(stack.peek()); // 1
```

### 22.### 21.6 Interface Implementation

```ts
interface Drawable {
  draw(): void;
}

interface Movable {
  move(x: number, y: number): void;
}

class Sprite implements Drawable, Movable {
  constructor(public x: number, public y: number) {}

  draw(): void {
    logger.info(`Drawing at (${this.x}, ${this.y})`);
  }

  move(x: number, y: number): void {
    this.x = x;
    this.y = y;
  }
}

const sprite = new Sprite(0, 0);
sprite.draw();
sprite.move(10, 20);
sprite.draw();
```

---

## Appendix A: Grammar Summary

```ebnf
Program ::= ModuleItem*

ModuleItem ::=
  | ExportDeclaration
  | ImportDeclaration
  | Statement

ExportDeclaration ::=
  | 'export' Declaration
  | 'export' '{' ExportList '}'
  | 'export' '{' ExportList '}' 'from' StringLiteral

ImportDeclaration ::=
  | 'import' '{' ImportList '}' 'from' StringLiteral
  | 'import' '*' 'as' Identifier 'from' StringLiteral

Statement ::=
  | VariableDeclaration
  | FunctionDeclaration
  | ClassDeclaration
  | InterfaceDeclaration
  | TypeAliasDeclaration
  | ExpressionStatement
  | IfStatement
  | WhileStatement
  | ForStatement
  | ReturnStatement
  | ThrowStatement
  | BlockStatement

Expression ::=
  | Literal
  | Identifier
  | BinaryExpression
  | UnaryExpression
  | CallExpression
  | MemberExpression
  | ArrayExpression
  | ObjectExpression
  | ConditionalExpression
  | ArrowFunction
```

---

## Appendix B: Type System Rules

### Subtyping

* `T` <: `T` (reflexive)
* `null` <: `T | null` for any T
* `T` <: `T | U` for any U
* Class `C extends D` means `C` <: `D`

### Type Compatibility

* Structural compatibility for interfaces
* Nominal compatibility for classes
* Union types require type guards for safe access

---

## Appendix C: Compilation Model

1. **Parse** — Source to AST
2. **Type Check** — Validate all types
3. **Lower** — AST to typed IR
4. **Optimize** — Type-based optimizations
5. **Emit** — IR to typed bytecode
6. **Verify** — Bytecode verification
7. **Execute** — VM interprets bytecode

---

**End of Raya Language Specification v0.5**

Raya combines the **familiarity of TypeScript** with a **clean runtime & concurrency model** — while staying intentionally smaller, safer, and more predictable.
