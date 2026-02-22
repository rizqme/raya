# Why Raya?

## The Problem

Modern applications need:
- **Concurrency** - handle thousands of simultaneous operations
- **Type Safety** - catch bugs before they reach production
- **Performance** - fast execution, low overhead

Existing solutions compromise:

| Language | Syntax | Type Safety | Concurrency | Performance |
|----------|--------|-------------|-------------|-------------|
| **TypeScript** | ✅ Familiar | ❌ Runtime only | ❌ Async hell | ❌ JIT overhead |
| **Go** | ❌ Verbose | ❌ Weak types | ✅ Goroutines | ⚠️ GC pauses |
| **Rust** | ❌ Steep curve | ✅ Static | ⚠️ Complex async | ✅ Zero cost |

Raya combines the best parts.

## The Raya Approach

### TypeScript Syntax

```typescript
// This is valid Raya AND valid TypeScript
function greet(name: string): string {
  return `Hello, ${name}!`;
}
```

No new syntax to learn. TypeScript developers feel at home immediately.

### Static Type System

**TypeScript:**
```typescript
function add(a: number, b: number): number {
  return a + b;
}
add("1", "2");  // Runtime: "12" (silent coercion)
```

**Raya:**
```typescript
function add(a: number, b: number): number {
  return a + b;
}
add("1", "2");  // Compile error: expected number, got string
```

All types verified at compile time. Zero runtime overhead. No silent coercion.

### Goroutine-Style Concurrency

**JavaScript/TypeScript:**
```typescript
// Callback hell or async/await everywhere
async function process() {
  const a = await fetchA();
  const b = await fetchB();  // Sequential!
  return [a, b];
}
```

**Go:**
```go
// Goroutines, but weak type system
func process() []interface{} {
  ch := make(chan interface{}, 2)
  go func() { ch <- fetchA() }()  // No type safety
  go func() { ch <- fetchB() }()
  return []interface{}{<-ch, <-ch}
}
```

**Raya:**
```typescript
// Best of both: type-safe + concurrent
function process(): [string, number] {
  const taskA = async fetchA();  // Returns Task<string>
  const taskB = async fetchB();  // Returns Task<number>
  return [await taskA, await taskB];  // Type-safe!
}
```

- Tasks start immediately (like goroutines)
- Type safety preserved (like TypeScript)
- Work-stealing scheduler (efficient)

### Discriminated Unions

**TypeScript:**
```typescript
type Result = 
  | { ok: true; value: number }
  | { ok: false; error: string };

function handle(r: Result) {
  console.log(r.value);  // Runtime error if ok=false!
}
```

**Raya:**
```typescript
type Result = 
  | { ok: true; value: number }
  | { ok: false; error: string };

function handle(r: Result): void {
  logger.info(r.value);  // Compile error: must check 'ok' first
  
  // Correct:
  if (r.ok) {
    logger.info(r.value);  // Compiler knows 'value' exists
  }
}
```

Exhaustiveness checking at compile time. Pattern matching on discriminant.

### Zero Runtime Overhead

**TypeScript:**
- Types erased → no optimization
- Dynamic dispatch everywhere
- Boxing primitives
- Runtime type checks

**Raya:**
- Static types → typed opcodes (IADD, FADD)
- Monomorphization (specialized code per type)
- Unboxed operations
- Zero runtime type checks

```typescript
// Compiles to IADD (integer add) opcode
function addInt(a: int, b: int): int {
  return a + b;
}

// Compiles to FADD (float add) opcode
function addFloat(a: number, b: number): number {
  return a + b;
}
```

## Trade-offs

Raya isn't perfect. Here's what you give up:

### No Dynamic Types
- ✅ TypeScript: `any`, type assertions, runtime reflection
- ❌ Raya: All types known at compile time

If you need dynamic behavior, use discriminated unions or reflection API.

### No Prototype Chain
- ✅ JavaScript: Prototype inheritance, monkey patching
- ❌ Raya: Class-based only, no prototype

Cleaner semantics, but less dynamic.

### No Existing Ecosystem
- ✅ TypeScript: npm, millions of packages
- ❌ Raya: Early project, small stdlib

You'll need to write more yourself or wait for ecosystem to grow.

### Learning Curve
- ✅ TypeScript: Gradual typing, escape hatches
- ❌ Raya: Sound type system, no escape

The type checker will reject code that TypeScript allows. This is by design.

## When to Choose Raya

**Good fit:**
- You want TypeScript syntax without JavaScript baggage
- You need goroutine-style concurrency
- You value compile-time guarantees over runtime flexibility
- You're building from scratch (no legacy code)
- Performance matters

**Not a good fit:**
- You need npm ecosystem
- You want gradual typing
- You need production stability (Raya is early stage)
- Dynamic behavior is essential
- You need JavaScript interop

## Philosophy

### Explicit Over Implicit
```typescript
// Explicit discriminant required
type State = 
  | { kind: "loading" }
  | { kind: "ready"; data: string };
```

### Safety Over Convenience
```typescript
// No 'any' escape hatch
function parse(json: string): User {
  return JSON.parse(json);  // Error: return type unknown
}
```

### Performance Through Types
```typescript
// Static types enable optimization
function sum(nums: int[]): int {
  let total: int = 0;
  for (const n of nums) {
    total += n;  // IADD opcode, unboxed
  }
  return total;
}
```

### Familiar Syntax
```typescript
// TypeScript developers feel at home
import logger from "std:logger";

class User {
  constructor(public name: string) {}
  
  greet(): void {
    logger.info(`Hello, ${this.name}!`);
  }
}
```

## Current Status

**Ready:**
- ✅ Core language (parser, type checker, compiler)
- ✅ Bytecode VM with GC
- ✅ Goroutine-style Tasks
- ✅ Standard library (14 modules)
- ✅ CLI tooling

**Experimental:**
- 🚧 JIT compilation (feature-gated)
- 🚧 AOT compilation (feature-gated)
- 🚧 LSP server

**Not Ready:**
- ❌ Production stability
- ❌ Large ecosystem
- ❌ Windows support (POSIX only for now)

## Try It

```bash
curl -fsSL https://raya.land/install.sh | sh
raya eval "1 + 2 * 3"
```

Read the [Getting Started](/getting-started) guide to learn more.

---

*Raya is an experiment in combining TypeScript syntax with compile-time guarantees and goroutine-style concurrency. It's not trying to replace anything - just exploring what's possible.*
