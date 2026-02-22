# Cross-Platform Standard Library

14 modules that work on all platforms (Windows, macOS, Linux, BSD).

**Package:** `raya-stdlib`

## logger

**Import:** `import logger from "std:logger"`

Structured logging with levels and formatting.

```typescript
logger.debug("Debug message");
logger.info("Info message");
logger.warn("Warning message");
logger.error("Error message");
```

**Features:**
- Level filtering (debug, info, warn, error)
- Timestamps
- Custom prefixes
- JSON formatting
- Structured data support

## math

**Import:** `import math from "std:math"`

Mathematical functions and constants.

```typescript
const result = math.sqrt(16);      // 4
const angle = math.atan2(1, 1);    // π/4
const random = math.random();      // 0.0-1.0
```

**Functions (22):**
`abs`, `sign`, `floor`, `ceil`, `round`, `trunc`, `min`, `max`, `pow`, `sqrt`, `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `exp`, `log`, `log10`, `random`

**Constants:** `PI`, `E`

## crypto

**Import:** `import crypto from "std:crypto"`

Cryptographic operations.

```typescript
// Hashing
const hash = crypto.hash("sha256", "data");
logger.info(crypto.toHex(hash));

// HMAC
const hmac = crypto.hmac("sha256", "key", "data");

// Random
const bytes = crypto.randomBytes(32);
const uuid = crypto.randomUUID();
```

**Functions:**
- `hash(algorithm, data)` - SHA-256/384/512, SHA-1, MD5
- `hmac(algorithm, key, data)` - HMAC-SHA-256/384/512
- `randomBytes(n)` - Cryptographically secure random
- `randomInt(min, max)` - Random integer
- `randomUUID()` - RFC 4122 UUID v4
- `toHex(bytes)`, `fromHex(hex)` - Hex encoding
- `toBase64(bytes)`, `fromBase64(b64)` - Base64 encoding
- `timingSafeEqual(a, b)` - Constant-time comparison

## time

**Import:** `import time from "std:time"`

Clocks, sleep, and duration utilities.

```typescript
const start = time.monotonic();
time.sleep(100);  // 100ms
const elapsed = time.elapsed(start);  // nanoseconds
```

**Functions:**
- `now()` - Wall clock time (Unix timestamp in ns)
- `monotonic()` - Monotonic clock for measuring
- `hrtime()` - High-resolution time
- `elapsed(start)` - Time since start (ns)
- `sleep(ms)` - Sleep milliseconds
- `sleepMicros(us)` - Sleep microseconds
- Duration conversions: `seconds`, `minutes`, `hours`, `toSeconds`, `toMinutes`, `toHours`

## path

**Import:** `import path from "std:path"`

Path manipulation (OS-independent).

```typescript
const fullPath = path.join("/home/user", "docs", "file.txt");
const dir = path.dirname(fullPath);  // /home/user/docs
const name = path.basename(fullPath);  // file.txt
const ext = path.extname(fullPath);  // .txt
```

**Functions:**
- `join(...parts)` - Join path components
- `normalize(path)` - Normalize path
- `dirname(path)` - Directory name
- `basename(path)` - File name
- `extname(path)` - File extension
- `isAbsolute(path)` - Check if absolute
- `resolve(...parts)` - Resolve to absolute path
- `relative(from, to)` - Relative path
- `cwd()` - Current working directory
- `sep` - Path separator (`/` or `\`)
- `delimiter` - PATH delimiter (`:` or `;`)
- `stripExt(path)` - Remove extension
- `withExt(path, ext)` - Replace extension
- `isRelative(path)` - Check if relative

## stream (In Progress)

**Import:** `import stream from "std:stream"`

Reactive streams for data processing.

## url

**Import:** `import url from "std:url"`

WHATWG URL parsing and manipulation.

```typescript
const u = url.parse("https://example.com/path?key=value");
logger.info(u.hostname);  // example.com
logger.info(u.pathname);  // /path
logger.info(u.search);    // ?key=value
```

## compress

**Import:** `import compress from "std:compress"`

Compression and decompression.

```typescript
const compressed = compress.gzip("data");
const data = compress.gunzip(compressed);
```

**Functions:**
- `gzip(data)` / `gunzip(data)`
- `deflate(data)` / `inflate(data)`
- `zlib(data)` / `unzlib(data)`

## encoding

**Import:** `import encoding from "std:encoding"`

Additional encoding formats.

```typescript
const hex = encoding.toHex(bytes);
const b32 = encoding.toBase32(bytes);
const b64url = encoding.toBase64Url(bytes);
```

**Functions:**
- Hex: `toHex`, `fromHex`
- Base32: `toBase32`, `fromBase32`
- Base64url: `toBase64Url`, `fromBase64Url`

## semver

**Import:** `import semver from "std:semver"`

Semantic versioning.

```typescript
const v = semver.parse("1.2.3");
logger.info(v.major, v.minor, v.patch);

if (semver.satisfies("1.5.0", "^1.0.0")) {
  logger.info("Compatible");
}
```

## template

**Import:** `import template from "std:template"`

String template engine.

```typescript
const tmpl = '{{#if user}}Hello, {{user.name}}!{{/if}}';
const output = template.render(tmpl, { user: { name: "Alice" } });
```

## args

**Import:** `import args from "std:args"`

Command-line argument parser (pure Raya).

## runtime

**Import:** `import { Compiler, Vm } from "std:runtime"`

Runtime introspection and compilation.

```typescript
const code = 'function add(a, b) { return a + b; }';
const module = Compiler.compile(code);
const result = Compiler.eval('add(2, 3)');
```

## reflect

**Import:** `import * as Reflect from "std:reflect"`

Reflection API (149+ handlers).

```typescript
const fields = Reflect.getClassFields(MyClass);
for (const field of fields) {
  logger.info(field.name, field.type);
}
```

See [Native IDs](native-ids.md) for full API.
