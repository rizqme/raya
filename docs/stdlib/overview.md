# Standard Library

Raya's standard library is split into two crates:

## Core Modules (`raya-stdlib`)
Cross-platform, pure Rust implementations:
- **logger** - Structured logging
- **math** - Mathematical functions  
- **crypto** - Cryptography & hashing
- **time** - Time & duration utilities
- **path** - Path manipulation
- **stream** - Reactive streams
- **url** - URL parsing
- **compress** - Compression (gzip, deflate, brotli)
- **encoding** - Base64, hex, etc
- **semver** - Semantic versioning
- **template** - String templates
- **json** - JSON parsing/serialization
- **toml** - TOML parsing/serialization
- **runtime** - Compiler & VM APIs
- **reflect** - Reflection & metaprogramming

## System Modules (`raya-stdlib-posix`)
POSIX-specific, OS integration:
- **fs** - File system operations
- **net** - TCP/UDP networking
- **http** - HTTP server
- **fetch** - HTTP client
- **env** - Environment variables
- **process** - Process management
- **os** - OS information
- **io** - Standard I/O
- **dns** - DNS resolution
- **terminal** - Terminal control
- **ws** - WebSockets
- **readline** - Line editing
- **glob** - File globbing
- **archive** - Tar/zip archives
- **watch** - File watching

## Import Syntax

All modules use the `std:` prefix:

```typescript
import logger from "std:logger";
import math from "std:math";
import { TcpListener } from "std:net";
import * as Reflect from "std:reflect";
```

## Philosophy

### Synchronous by Default
All I/O operations are synchronous. Concurrency achieved at call site:

```typescript
import fs from "std:fs";

// Synchronous
const data = fs.readTextFile("file.txt");

// Concurrent
const t1 = async fs.readTextFile("a.txt");
const t2 = async fs.readTextFile("b.txt");
```

### Batteries Included
Everything you need to build real applications without external dependencies.

### Zero-Cost Abstractions
Static types enable optimization. No runtime overhead.
