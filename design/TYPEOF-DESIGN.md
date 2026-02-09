# typeof Operator for Bare Unions - Design Change

**Date:** 2026-01-05
**Status:** Proposed Design Change
**Affects:** Language specification, runtime type checking

---

## Summary

Replace the `match()` pattern for bare primitive unions with the `typeof` operator for type narrowing.

---

## Motivation

The previous design used automatic boxing with `$type` and `$value` fields plus a `match()` utility function for bare unions. This added complexity and memory overhead.

Using `typeof` (like JavaScript) provides:
- ✅ **Familiar syntax** - developers already know `typeof`
- ✅ **Simpler implementation** - no automatic boxing required
- ✅ **Better performance** - direct type checking without indirection
- ✅ **Natural type narrowing** - works with TypeScript's control flow analysis

---

## Design

### Bare Union Definition

Bare unions are unions of **primitive types only**:

```typescript
type ID = string | number;
type Optional = string | null;
type Value = string | number | boolean;
```

**Allowed primitive types:**
- `string`
- `number`
- `boolean`
- `null`

**Not allowed in bare unions:**
- Objects, classes, interfaces
- Arrays
- Functions
- Complex types → use discriminated unions instead

---

## typeof Operator

### Syntax

```typescript
typeof expression
```

**Returns:** A string literal type representing the runtime type.

### Return Values

| Value Type | typeof Result |
|------------|---------------|
| `null` | `"null"` |
| `true` / `false` | `"boolean"` |
| `42` / `3.14` | `"number"` |
| `"hello"` | `"string"` |
| `{ x: 1 }` | `"object"` |
| `[1, 2, 3]` | `"object"` |
| `() => {}` | `"function"` |

**Note:** Arrays and objects both return `"object"` (matches JavaScript behavior).

---

## Type Narrowing with typeof

The compiler uses `typeof` checks for **control flow-based type narrowing**:

```typescript
function process(id: string | number): string {
  if (typeof id === "number") {
    // id is narrowed to number here
    return `ID: ${id.toFixed(0)}`;
  } else {
    // id is narrowed to string here
    return `ID: ${id.toUpperCase()}`;
  }
}
```

### Exhaustiveness Checking

The compiler ensures all union variants are handled:

```typescript
function handle(value: string | number | boolean): string {
  if (typeof value === "string") {
    return value;
  } else if (typeof value === "number") {
    return value.toString();
  } else if (typeof value === "boolean") {
    return value ? "yes" : "no";
  }
  // ✅ Compiler verifies all cases covered
}

// ❌ Compiler error: Not exhaustive
function bad(value: string | number): string {
  if (typeof value === "string") {
    return value;
  }
  // Error: Missing case for 'number'
}
```

### Nullable Types

`typeof` works naturally with nullable unions:

```typescript
function greet(name: string | null): string {
  if (typeof name === "string") {
    return `Hello, ${name}!`;
  } else {
    // name is narrowed to null
    return "Hello, stranger!";
  }
}

// Alternative: direct null check
function greet2(name: string | null): string {
  if (name === null) {
    return "Hello, stranger!";
  }
  // name is narrowed to string
  return `Hello, ${name}!`;
}
```

---

## Examples

### Basic Usage

```typescript
type ID = string | number;

let id: ID = 42;

if (typeof id === "number") {
  logger.info(id + 1);  // 43
} else {
  logger.info(id.length);  // string methods
}
```

### Switch Statement

```typescript
type Value = string | number | boolean;

function describe(v: Value): string {
  switch (typeof v) {
    case "string":
      return `String: "${v}"`;
    case "number":
      return `Number: ${v}`;
    case "boolean":
      return `Boolean: ${v}`;
  }
}
```

### Early Return Pattern

```typescript
function parse(input: string | number): number {
  if (typeof input === "number") {
    return input;
  }

  // input is narrowed to string here
  const parsed = parseInt(input, 10);
  if (isNaN(parsed)) {
    throw new Error("Invalid number");
  }
  return parsed;
}
```

---

## Discriminated Unions (Unchanged)

For **complex types** (objects, classes, arrays), use **discriminated unions** with explicit discriminant fields:

```typescript
type Result =
  | { status: "ok"; value: number }
  | { status: "error"; message: string };

function handle(r: Result): void {
  if (r.status === "ok") {
    logger.info(r.value);
  } else {
    logger.info(r.message);
  }
}
```

`typeof` returns `"object"` for all complex types, so discriminants are required for narrowing.

---

## Implementation

### Runtime Representation

Bare union values are stored **directly** without boxing:

```typescript
type ID = string | number;
let id: ID = 42;
// Runtime: Value::i32(42) - no wrapping
```

### typeof Bytecode

New opcode: `TYPEOF`

```
TYPEOF
  Pop: value
  Push: string (type name)
```

Example bytecode:
```
LOAD_LOCAL 0      // load id
TYPEOF            // get type
CONST_STR "number"
SEQ               // string equality
JMP_IF_FALSE else_branch
  // number case
  JMP end
else_branch:
  // string case
end:
```

### Compiler Type Narrowing

The type checker tracks typeof guards:

1. **Parse** `typeof x === "number"`
2. **Narrow** `x` to `number` in the true branch
3. **Narrow** `x` to remaining types in the false branch
4. **Check exhaustiveness** at function boundaries

---

## Migration from match()

### Before (match pattern)

```typescript
import { match } from "raya:std";

const result = match(id, {
  string: (s) => `String: ${s}`,
  number: (n) => `Number: ${n}`
});
```

### After (typeof pattern)

```typescript
let result: string;
if (typeof id === "string") {
  result = `String: ${id}`;
} else {
  result = `Number: ${id}`;
}

// Or with ternary
const result = typeof id === "string"
  ? `String: ${id}`
  : `Number: ${id}`;
```

---

## Benefits

| Aspect | typeof | match() |
|--------|--------|---------|
| **Familiarity** | ✅ Standard JS | ❌ Custom utility |
| **Performance** | ✅ No boxing | ❌ Requires boxing |
| **Memory** | ✅ Direct values | ❌ Wrapped values |
| **Simplicity** | ✅ Built-in operator | ❌ Import required |
| **Type narrowing** | ✅ Natural flow | ⚠️ Expression-based |

---

## Limitations

1. **Only for primitives** - Complex types need discriminated unions
2. **Objects are indistinguishable** - `typeof` can't tell apart different object types
3. **Array detection** - Use `Array.isArray()` instead of `typeof`

---

## Comparison with TypeScript

Raya's `typeof` matches TypeScript/JavaScript behavior:

```typescript
// TypeScript
let x: string | number = 42;
if (typeof x === "number") {
  x.toFixed(2);  // ✅ Narrowed to number
}

// Raya (same!)
let x: string | number = 42;
if (typeof x === "number") {
  x.toFixed(2);  // ✅ Narrowed to number
}
```

**Key difference:** Raya enforces exhaustiveness checking.

---

## Summary

- **Use `typeof`** for bare unions of primitives
- **Use discriminated unions** for complex types
- **No `match()` utility** needed for primitives
- **Simpler, faster, more familiar** than automatic boxing
