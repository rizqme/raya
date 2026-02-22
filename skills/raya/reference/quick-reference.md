# Quick Reference

Fast lookup for common Raya questions.

## Language FAQs

| Question | Answer |
|----------|--------|
| Use `any` type? | ❌ No - banned for type safety |
| Use `typeof`? | ✅ Yes - for primitive unions only |
| Use `instanceof`? | ✅ Yes - for class type checking |
| Runtime type checks? | ❌ Never - compile-time only |
| Generic erasure? | ❌ No - uses monomorphization |
| Concurrency model? | Goroutine-style Tasks |
| async/await? | ✅ Yes - Tasks start immediately |
| `undefined`? | ❌ No - use `null` |
| Prototype chains? | ❌ No - class-based only |

## Type System

### Primitive Types

| Type | Size | Range | Example |
|------|------|-------|---------|
| `int` | 32-bit | -2^31 to 2^31-1 | `42` |
| `number` | 64-bit float | IEEE 754 | `3.14` |
| `string` | UTF-8 | Variable | `"hello"` |
| `boolean` | 1-bit | true/false | `true` |
| `null` | - | null | `null` |

### Type Checking

```typescript
// Primitives
typeof x == "string"
typeof x == "int"
typeof x == "number"

// Classes
x instanceof MyClass

// Discriminated unions
if (result.status == "ok") { ... }
```

## Concurrency

```typescript
// Start Task
const task = async compute();

// Await Task
const result = await task;

// Parallel Tasks
const t1 = async work1();
const t2 = async work2();
const r1 = await t1;
const r2 = await t2;
```

## Common Patterns

### Result Type

```typescript
type Result<T> =
  | { ok: true; value: T }
  | { ok: false; error: string };
```

### Option Type

```typescript
type Option<T> = T | null;

function unwrap(opt: Option<T>): T {
  if (opt == null) throw new Error("unwrap null");
  return opt;
}
```

### Error Handling

```typescript
try {
  const result = operation();
} catch (e) {
  logger.error(e.message);
}
```

## Standard Library Cheat Sheet

### Logging

```typescript
import logger from "std:logger";
logger.debug("Debug");
logger.info("Info");
logger.warn("Warning");
logger.error("Error");
```

### Math

```typescript
import math from "std:math";
math.sqrt(16)      // 4
math.pow(2, 3)     // 8
math.floor(3.7)    // 3
math.random()      // 0.0-1.0
```

### Crypto

```typescript
import crypto from "std:crypto";
crypto.hash("sha256", "data")
crypto.randomBytes(32)
crypto.randomUUID()
```

### Time

```typescript
import time from "std:time";
const start = time.monotonic();
time.sleep(100);  // 100ms
const elapsed = time.elapsed(start);
```

### File System

```typescript
import fs from "std:fs";
const content = fs.readTextFile("file.txt");
fs.writeTextFile("out.txt", "data");
const entries = fs.readDir(".");
```

### HTTP

```typescript
import { HttpServer } from "std:http";
const server = new HttpServer("127.0.0.1", 8080);
server.serve((req) => ({
  status: 200,
  body: "Hello!"
}));
```

## CLI Commands

| Command | Purpose |
|---------|---------|
| `raya run app.raya` | Execute program |
| `raya build app.raya` | Compile to bytecode |
| `raya check app.raya` | Type-check only |
| `raya eval "1+2"` | Evaluate expression |
| `raya repl` | Interactive shell |
| `raya init` | Initialize project |
| `raya add pkg` | Add dependency |
| `raya install` | Install deps |
| `raya bundle app.raya` | Native compile (AOT) |

## Build & Test

```bash
# Build
cargo build --workspace
cargo build --features jit
cargo build --features aot

# Test
cargo test
cargo test -p raya-engine
cargo test -p raya-runtime

# Run
raya run app.raya
raya run --no-jit app.raya
```

## Performance Hints

| Operation | Cost | Notes |
|-----------|------|-------|
| Task creation | ~100ns | Lazy stacks |
| Task switch | ~20-50ns | No syscall |
| Function call | ~5-10ns | Direct bytecode |
| Native call | ~50-100ns | FFI overhead |
| GC nursery alloc | ~10ns | Bump pointer |

## Import Patterns

```typescript
// Default export
import logger from "std:logger";

// Named exports
import { TcpListener, TcpStream } from "std:net";

// Namespace import
import * as Reflect from "std:reflect";

// Local modules
import { MyClass } from "./lib.raya";
```

## Design Principles

| Principle | Implementation |
|-----------|----------------|
| Explicit over implicit | Discriminated unions, type annotations |
| Safety over convenience | No escape hatches, sound type system |
| Performance through types | Typed opcodes, monomorphization |
| Familiar syntax | TypeScript-compatible |
| Predictable semantics | Well-defined execution model |

## Related

- [Common Tasks](common-tasks.md) - Recipe-style guides
- [Documentation](documentation.md) - Links to all docs
