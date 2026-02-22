# Standard Library Overview

Raya's standard library provides cross-platform and POSIX-specific modules for common programming tasks.

## Design Philosophy

### 1. Synchronous APIs + Goroutines

All I/O operations are **synchronous**. Achieve concurrency with goroutines:

```typescript
import fs from "std:fs";

// Synchronous (blocks current Task)
const data = fs.readTextFile("config.json");

// Concurrent (multiple Tasks)
const t1 = async fs.readTextFile("a.txt");
const t2 = async fs.readTextFile("b.txt");
const a = await t1;  // Both reads happen in parallel
const b = await t2;
```

**Benefits:**
- Simpler API (no callbacks/promises in stdlib)
- Explicit concurrency at call site
- Predictable execution order
- Easy to reason about

### 2. Type-Safe APIs

No `any` types, no optional chaining gymnastics:

```typescript
// Clear result types
type ReadResult = { ok: true; content: string } | { ok: false; error: string };

// Or throw errors
const content = fs.readTextFile("file.txt");  // Throws on error
```

### 3. Batteries Included

Common tasks have first-class support:
- Logging, math, crypto, time, paths
- File I/O, networking, HTTP
- JSON, compression, encoding
- Process management, environment

## Module Organization

### Cross-Platform (`raya-stdlib`)

14 modules that work everywhere:

| Module | Purpose |
|--------|---------|
| `logger` | Structured logging |
| `math` | Mathematical operations |
| `crypto` | Hashing, HMAC, random |
| `time` | Clocks and timers |
| `path` | Path manipulation |
| `stream` | Reactive streams |
| `url` | URL parsing |
| `compress` | Compression (gzip, deflate) |
| `encoding` | Hex, base32, base64url |
| `semver` | Semantic versioning |
| `template` | String templates |
| `args` | CLI argument parser |
| `runtime` | VM introspection |
| `reflect` | Reflection API |

### POSIX-Specific (`raya-stdlib-posix`)

15 modules for system programming:

| Module | Purpose |
|--------|---------|
| `fs` | File system |
| `net` | TCP/UDP networking |
| `http` | HTTP server |
| `fetch` | HTTP client |
| `env` | Environment variables |
| `process` | Process management |
| `os` | Platform information |
| `io` | stdin/stdout/stderr |
| `dns` | DNS resolution |
| `terminal` | Terminal control |
| `ws` | WebSocket client |
| `readline` | Line editing |
| `glob` | File globbing |
| `archive` | tar/zip |
| `watch` | File watching |

## Import Patterns

### Default Exports

Most modules use default export:

```typescript
import logger from "std:logger";
import math from "std:math";
import fs from "std:fs";

logger.info("Value:", math.sqrt(16));
fs.writeTextFile("out.txt", "data");
```

### Named Exports

Some modules export multiple types:

```typescript
import { TcpListener, TcpStream } from "std:net";
import { Compiler, Vm } from "std:runtime";
```

### Namespace Imports

For modules with many exports:

```typescript
import * as Reflect from "std:reflect";

const fields = Reflect.getClassFields(MyClass);
```

## Module Categories

### I/O and System

**File System:**
```typescript
import fs from "std:fs";

fs.writeTextFile("data.txt", "content");
const content = fs.readTextFile("data.txt");
const entries = fs.readDir("./");
```

**Networking:**
```typescript
import { TcpListener } from "std:net";

const listener = new TcpListener("127.0.0.1", 8080);
for (const stream of listener.accept()) {
  handleClient(stream);
}
```

**HTTP:**
```typescript
import { HttpServer } from "std:http";

const server = new HttpServer("127.0.0.1", 8080);
server.serve((req) => ({
  status: 200,
  body: "Hello!"
}));
```

### Data Processing

**JSON:**
```typescript
import { JSON } from "std:json";

const user = JSON.parse<User>('{"id": 1}');
const json = JSON.stringify(user);
```

**Compression:**
```typescript
import compress from "std:compress";

const compressed = compress.gzip("data");
const data = compress.gunzip(compressed);
```

**Encoding:**
```typescript
import encoding from "std:encoding";

const hex = encoding.toHex(bytes);
const b64 = encoding.toBase64(bytes);
```

### Utilities

**Logging:**
```typescript
import logger from "std:logger";

logger.debug("Debug info");
logger.info("App started");
logger.warn("Low memory");
logger.error("Failed");
```

**Math:**
```typescript
import math from "std:math";

const result = math.sqrt(x * x + y * y);
const angle = math.atan2(y, x);
```

**Time:**
```typescript
import time from "std:time";

const start = time.monotonic();
time.sleep(100);  // 100ms
const elapsed = time.elapsed(start);
```

### Advanced

**Reflection:**
```typescript
import * as Reflect from "std:reflect";

const cls = Reflect.getClass(obj);
const methods = Reflect.getClassMethods(cls);
```

**Runtime:**
```typescript
import { Compiler, Vm } from "std:runtime";

const code = 'function add(a, b) { return a + b; }';
const module = Compiler.compile(code);
Vm.current().execute(module);
```

## Error Handling

### Throwing Errors

Most stdlib functions throw on error:

```typescript
try {
  const content = fs.readTextFile("missing.txt");
} catch (e) {
  logger.error("File not found:", e.message);
}
```

### Result Types

Some use discriminated unions:

```typescript
type Result<T> = { ok: true; value: T } | { ok: false; error: string };

function safeRead(path: string): Result<string> {
  try {
    return { ok: true, value: fs.readTextFile(path) };
  } catch (e) {
    return { ok: false, error: e.message };
  }
}
```

## Extension

### Custom Native Modules

Implement `NativeHandler` trait:

```rust
use raya_sdk::{NativeHandler, NativeContext, NativeValue, NativeCallResult};

pub struct MyHandler;

impl NativeHandler for MyHandler {
    fn call(&self, ctx: &NativeContext, id: u16, args: &[NativeValue]) 
        -> NativeCallResult {
        match id {
            0x7000 => my_function(ctx, args),
            _ => NativeCallResult::Unhandled,
        }
    }
}
```

See [Adding Modules](../development/adding-modules.md) for details.

## Performance Notes

- **I/O operations** run on IO pool (don't block VM workers)
- **Crypto operations** use efficient Rust implementations
- **String operations** benefit from string interning
- **Math operations** compile to native instructions (with JIT/AOT)

## Related

- [Cross-Platform Modules](cross-platform.md) - 14 modules
- [POSIX Modules](posix.md) - 15 system modules
- [Native IDs](native-ids.md) - ID ranges and dispatch
