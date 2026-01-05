# Numeric Types in Raya

**Last Updated:** 2026-01-05

This document provides a comprehensive overview of Raya's numeric type system, including `int`, `float`, and `number`.

---

## Overview

Raya provides three numeric types with distinct semantics and runtime representations:

| Type | Runtime | Range | Use Case |
|------|---------|-------|----------|
| `int` | i32 (32-bit signed integer) | -2,147,483,648 to 2,147,483,647 | Counters, indices, whole numbers |
| `float` | f64 (64-bit IEEE 754 double) | ±5.0 × 10⁻³²⁴ to ±1.7 × 10³⁰⁸ | Decimals, large integers, scientific |
| `number` | `int \| float` (type alias) | Combined range | Generic numeric values |

---

## Type Definitions

### `int` - 32-bit Signed Integer

```ts
let count: int = 42;
let offset: int = -100;
let max: int = 2_147_483_647;  // Max i32
```

**Properties:**
- Stored unboxed (NaN-boxed in 64-bit Value)
- Exact integer arithmetic within range
- Overflow wraps (like Rust's wrapping arithmetic)
- No decimal point

**Operations:**
```ts
let a: int = 10;
let b: int = 3;

a + b   // 13 (int)
a - b   // 7 (int)
a * b   // 30 (int)
a / b   // 3 (int) - integer division, truncates
a % b   // 1 (int) - modulo
```

### `float` - 64-bit Floating Point

```ts
let pi: float = 3.14159;
let huge: float = 1e100;
let temp: float = 98.6;
```

**Properties:**
- Stored as direct IEEE 754 double (not NaN-boxed)
- Full double precision
- Special values: `Infinity`, `-Infinity`, `NaN`
- Can represent integers up to 2⁵³ exactly

**Operations:**
```ts
let a: float = 10.5;
let b: float = 3.2;

a + b   // 13.7 (float)
a - b   // 7.3 (float)
a * b   // 33.6 (float)
a / b   // 3.28125 (float) - true division
a % b   // 0.9 (float) - floating-point modulo
```

### `number` - Union Type

`number` is a type alias defined as:

```ts
type number = int | float;
```

**Use Cases:**
- Generic numeric functions
- TypeScript/JavaScript compatibility
- Accepting either integer or float values

**Example:**
```ts
function double(x: number): number {
  // x can be int or float
  return x * 2;
}

double(21);    // returns 42 (int)
double(3.14);  // returns 6.28 (float)
```

---

## Type Inference

Raya infers numeric literal types based on their syntax:

### Integer Literals → `int`

```ts
42              // int
0               // int
-17             // int
0x1A            // int (hexadecimal)
0o755           // int (octal)
0b1010          // int (binary)
1_000_000       // int (with separators)
2_147_483_647   // int (max i32)
```

### Float Literals → `float`

```ts
3.14            // float (has decimal point)
1.0             // float (explicit decimal)
1e6             // float (scientific notation)
1.5e-10         // float
2_147_483_648   // float (exceeds i32 max)
Infinity        // float
NaN             // float
```

### Context-Driven Inference

```ts
let a = 42;         // Inferred as int
let b = 3.14;       // Inferred as float

let c: number = 42; // Explicit number type, value is int
let d: int = 42;    // Explicit int type
let e: float = 42;  // ERROR: Cannot implicitly convert int to float
let f: float = 42.0; // OK: float literal
```

---

## Type Conversions

### Implicit Conversions

**✅ Allowed:**
- `int` → `float` (always safe, no loss of precision for most values)

```ts
let i: int = 42;
let f: float = i;  // OK: Implicit widening conversion
```

**❌ Not Allowed:**
- `float` → `int` (requires explicit conversion, may lose precision)

```ts
let f: float = 3.14;
let i: int = f;  // ERROR: No implicit narrowing conversion
```

### Explicit Conversions

Use built-in functions for explicit conversions:

```ts
let f: float = 3.14;

// float → int (various rounding strategies)
let i1: int = Math.floor(f);    // 3 (round down)
let i2: int = Math.ceil(f);     // 4 (round up)
let i3: int = Math.round(f);    // 3 (round to nearest)
let i4: int = Math.trunc(f);    // 3 (truncate towards zero)

// int → float (explicit cast)
let i: int = 42;
let f2: float = float(i);       // 42.0 (explicit conversion)
```

---

## Type Narrowing with `typeof`

The `typeof` operator distinguishes between `int` and `float` at runtime:

```ts
function processNumber(x: number): string {
  if (typeof x === "int") {
    return `Integer: ${x}`;
  } else {
    return `Float: ${x.toFixed(2)}`;
  }
}

processNumber(42);    // "Integer: 42"
processNumber(3.14);  // "Float: 3.14"
```

**typeof values:**
```ts
typeof 42           // "int"
typeof 3.14         // "float"
typeof (42 + 1)     // "int"
typeof (3.14 * 2)   // "float"
```

---

## Arithmetic Operations

### Homogeneous Operations

Operations between the same types preserve the type:

```ts
// int op int → int
10 + 5          // 15 (int)
10 / 3          // 3 (int, truncated)

// float op float → float
10.0 + 5.0      // 15.0 (float)
10.0 / 3.0      // 3.333... (float)
```

### Mixed Operations

Operations between `int` and `float` promote to `float`:

```ts
let i: int = 10;
let f: float = 3.0;

i + f           // 13.0 (float)
i * f           // 30.0 (float)
i / f           // 3.333... (float)
```

**Promotion Rules:**
- `int` op `float` → `float`
- `float` op `int` → `float`
- The `int` operand is implicitly converted to `float` before the operation

---

## Division Behavior

Division behavior depends on operand types:

### Integer Division

```ts
let a: int = 10;
let b: int = 3;

a / b           // 3 (int) - truncates towards zero
10 / 3          // 3 (int)
10 / -3         // -3 (int)
-10 / 3         // -3 (int)
```

### Floating-Point Division

```ts
let a: float = 10.0;
let b: float = 3.0;

a / b           // 3.333... (float) - true division
10.0 / 3.0      // 3.333... (float)
```

### Mixed Division

```ts
let i: int = 10;
let f: float = 3.0;

i / f           // 3.333... (float) - promotes to float
f / i           // 0.333... (float) - promotes to float
```

---

## Special Float Values

```ts
let inf: float = Infinity;
let ninf: float = -Infinity;
let nan: float = NaN;

// Infinity operations
Infinity + 1            // Infinity
Infinity * 2            // Infinity
Infinity / Infinity     // NaN
1 / 0.0                 // Infinity

// NaN operations
NaN + 1                 // NaN
NaN === NaN             // false (IEEE 754 behavior)
Number.isNaN(NaN)       // true
```

---

## Overflow Behavior

### Integer Overflow

```ts
let max: int = 2_147_483_647;
max + 1         // -2_147_483_648 (wraps around)

let min: int = -2_147_483_648;
min - 1         // 2_147_483_647 (wraps around)
```

### Float Overflow

```ts
let huge: float = 1e308;
huge * 10       // Infinity (overflow to infinity)

let tiny: float = 1e-323;
tiny / 10       // 0.0 (underflow to zero)
```

---

## Best Practices

### When to Use `int`

✅ Use `int` when:
- Working with counters, indices, or small whole numbers
- Performance is critical (unboxed representation)
- You need exact integer arithmetic within ±2 billion range

```ts
for (let i: int = 0; i < 100; i++) {
  array[i] = i * 2;
}
```

### When to Use `float`

✅ Use `float` when:
- Working with decimal values
- Need large numbers (>2³¹)
- Scientific or mathematical calculations
- Dealing with measurements, ratios, or percentages

```ts
let price: float = 19.99;
let discount: float = 0.15;
let final: float = price * (1.0 - discount);
```

### When to Use `number`

✅ Use `number` when:
- Writing generic numeric functions
- Maintaining TypeScript compatibility
- Accepting either integer or float input

```ts
function abs(x: number): number {
  if (x < 0) {
    return -x;
  }
  return x;
}
```

---

## Runtime Representation

### Value Encoding (NaN-Boxing)

Raya uses NaN-boxing to efficiently store all values in 64 bits:

```
f64 (float):  Any value where upper 13 bits != 0x1FFF (regular IEEE 754 double)
int:          0xFFF8001000000000 | (i32 as u64)  [NaN-boxed, tag=001]
bool:         0xFFF8002000000000 | (b as u64)    [NaN-boxed, tag=010]
null:         0xFFF8006000000000                 [NaN-boxed, tag=110]
pointer:      0xFFF8000000000000 | (ptr & 0xFFFFFFFFFFFF)  [NaN-boxed, tag=000]
```

**Benefits:**
- `float` stored directly as IEEE 754 (no boxing overhead)
- `int` stored in NaN-boxed representation (still unboxed)
- Type checking is a single bit-pattern test
- All values fit in 64 bits

---

## Examples

### Generic Math Functions

```ts
function max(a: number, b: number): number {
  return a > b ? a : b;
}

max(10, 20)         // 20 (int)
max(3.14, 2.71)     // 3.14 (float)
max(10, 3.14)       // 10.0 (float, promoted)
```

### Type-Specific Operations

```ts
function integerDivision(a: int, b: int): int {
  return a / b;  // Integer division
}

function floatDivision(a: float, b: float): float {
  return a / b;  // True division
}

integerDivision(10, 3)    // 3
floatDivision(10.0, 3.0)  // 3.333...
```

### Conditional Processing

```ts
function format(x: number): string {
  if (typeof x === "int") {
    return x.toString();
  } else {
    return x.toFixed(2);
  }
}

format(42)      // "42"
format(3.14159) // "3.14"
```

---

## Comparison with Other Languages

| Language | Integer | Float | Generic |
|----------|---------|-------|---------|
| **Raya** | `int` (i32) | `float` (f64) | `number` (int \| float) |
| **TypeScript** | - | `number` (f64) | - |
| **Rust** | `i32` | `f64` | Generic `T: Num` |
| **Go** | `int`, `int32` | `float64` | `interface{}` |
| **Python** | `int` | `float` | Dynamically typed |

**Key Difference from TypeScript:**
- TypeScript only has `number` (always f64)
- Raya distinguishes `int` (i32) from `float` (f64)
- Raya allows type narrowing between `int` and `float`

---

## Summary

- **`int`**: 32-bit signed integer, unboxed, for exact whole number arithmetic
- **`float`**: 64-bit IEEE 754 double, for decimals and large numbers
- **`number`**: Type alias for `int | float`, for generic numeric code
- **Inference**: Integer literals → `int`, decimal literals → `float`
- **Conversions**: `int` → `float` is implicit, `float` → `int` requires explicit rounding
- **typeof**: Distinguishes `"int"` from `"float"` at runtime
- **Division**: `int / int` truncates, `float / float` gives true division
- **Mixed ops**: `int` op `float` promotes to `float`

This design provides:
- ✅ Performance (unboxed integers)
- ✅ Precision (distinct int and float types)
- ✅ Compatibility (number type for TypeScript code)
- ✅ Safety (explicit conversions prevent precision loss)
