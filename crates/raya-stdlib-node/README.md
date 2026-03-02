# raya-stdlib-node

Node.js compatibility standard library for Raya. Provides `node:*` module shims so
that code written against the Node.js API surface can run on the Raya runtime with
minimal changes.

This crate contains `.raya` source-only implementations — it does not add native Rust
bindings. All I/O is delegated to existing `raya-stdlib` / `raya-stdlib-posix` natives.

## Quick start

```typescript
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";

const data = fs.readFileSync("/tmp/input.txt");
const hash = crypto.createHash("sha256").update(data).digest("hex");
fs.writeFileSync(path.join("/tmp", "hash.txt"), hash);
```

Both default and named imports work:

```typescript
import { URL, URLSearchParams } from "node:url";
import { EventEmitter } from "node:events";
import { describe, it } from "node:test";
```

## Conformance matrix

| Module | Level | Notes |
|--------|-------|-------|
| **node:fs** | Full | All sync methods, Stats, Dirent |
| **node:fs/promises** | Full | Async versions of all fs operations |
| **node:path** | Full | join, resolve, parse, format, normalize, etc. |
| **node:process** | Full | env, cwd, pid, hrtime, memoryUsage, signals |
| **node:os** | Full | platform, arch, cpus, homedir, networkInterfaces |
| **node:events** | Full | Re-exports builtin EventEmitter |
| **node:buffer** | Full | Re-exports builtin Buffer |
| **node:timers** | Full | setTimeout/setInterval/setImmediate with cancellation |
| **node:crypto** | High | createHash, createHmac, randomBytes/UUID/Int, pbkdf2, HKDF, key generation |
| **node:child_process** | High | exec/execSync, spawn/spawnSync, kill, signals |
| **node:http** | High | createServer, get, request (delegates to std:http2) |
| **node:https** | Medium | get, request (client-only, no createServer) |
| **node:net** | High | createServer, connect, isIP/isIPv4/isIPv6, Socket |
| **node:dns** | High | lookup, resolve (all record types), promises |
| **node:url** | High | URL, URLSearchParams, parse, format, encode/decode |
| **node:stream** | High | Readable, Writable, Transform, Duplex, pipeline |
| **node:assert** | High | ok, equal, deepEqual, throws, doesNotThrow |
| **node:util** | High | format, inspect, promisify, callbackify, types |
| **node:perf_hooks** | High | performance.now/mark/measure/clearMarks |
| **node:diagnostics_channel** | High | channel, subscribe/unsubscribe/publish |
| **node:test** | Medium | describe, it, before/after/beforeEach/afterEach |
| **node:string_decoder** | Medium | StringDecoder (UTF-8 only) |
| **node:v8** | Medium | getHeapStatistics, getHeapSpaceStatistics |
| **node:vm** | Minimal | Simple expression eval only |
| **node:timers/promises** | Medium | setTimeout, setImmediate (async) |
| **node:stream/promises** | Minimal | finished (no-op), pipeline (stub) |
| **node:stream/web** | Stub | All methods throw |
| **node:http2** | Passthrough | Delegates to std:http2 |
| **node:worker_threads** | Minimal | isMainThread only, no real threading |
| **node:cluster** | Stub | isPrimary returns true |
| **node:module** | Stub | createRequire returns thrower |
| **node:async_hooks** | Stub | createHook throws |
| **node:dgram** | Stub | createSocket throws |
| **node:inspector** | Stub | open throws, close no-op |
| **node:inspector/promises** | Stub | open throws |
| **node:repl** | Stub | start throws |
| **node:test/reporters** | Stub | Returns reporter names only |

**Levels**: Full = complete API · High = most APIs work · Medium = core works ·
Minimal = basic import works · Stub = throws or no-ops · Passthrough = delegates to std:

## Architecture

```
node:fs   ──┐
node:path ──┤  include_str!()   ┌──────────────┐
node:*    ──┴─────────────────► │ compile.rs    │──► bytecode
                prepended as    │ (prelude      │
                module prelude  │  injection)   │
                                └──────┬───────┘
                                       │
                              __NATIVE_CALL() ──► raya-stdlib / raya-stdlib-posix
```

Each `node:*` import resolves through `NODE_STD_ALIASES` in `compile.rs`. The module
source is prepended to user code at compile time, making all exports available. Native
operations use `__NATIVE_CALL()` to invoke registered Rust functions from `raya-stdlib`
(cross-platform: time, crypto) and `raya-stdlib-posix` (fs, path, env, process, io).

## Key differences from Node.js

1. **Sync-first**: Filesystem operations prefer sync methods. Async variants in
   `fs/promises` use Raya's `async {}` blocks.
2. **No Streams event API**: Streams use method calls (`read()`, `write()`) rather than
   event-driven `data`/`end` patterns.
3. **No `require()`**: Use ES module `import` syntax exclusively.
4. **Static typing**: All APIs are statically typed. Union return types
   (e.g., `readdirSync` returning `string[] | Dirent[]`) require type-aware handling.
5. **No `Buffer.from()`**: Use `Buffer` constructor or native methods directly.
6. **Crypto**: Chain-style API works (`createHash().update().digest()`), but cipher
   streams and some async variants are not available.

## Test coverage

97 E2E tests in `crates/raya-runtime/tests/e2e/node_stdlib.rs` covering:

- **path**: join, parse, dirname, basename, extname, isAbsolute, normalize, resolve,
  format, sep/delimiter, toNamespacedPath
- **fs**: read/write, append, stat, exists, mkdir, rename, copy, truncate, access, lstat
- **fs/promises**: async read/write, stat, copyFile
- **crypto**: hash chains, HMAC, randomBytes/UUID/Int, getHashes, digest encodings
- **process**: cwd, pid, env, hrtime, memoryUsage, platform/arch, version, uptime, exitCode
- **url**: URL constructor, protocol/host, searchParams, origin, toJSON
- **events**: on/off/once, emit, listenerCount, eventNames, named imports
- **util**: format, inspect, isDeepStrictEqual, types
- **child_process**: execSync, spawnSync, error handling
- **timers**: setTimeout/setInterval/setImmediate with cancellation
- **string_decoder**: constructor, encoding
- **diagnostics_channel**: pub/sub, unsubscribe
- **perf_hooks**: now, mark/measure, clearMarks
- **v8**: heap statistics
- **vm**: script eval, isContext
- **test**: describe/it, hooks
- **assert**: ok, equal, deepEqual, throws, doesNotThrow, notEqual
