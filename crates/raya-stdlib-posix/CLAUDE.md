# raya-stdlib-posix

POSIX/OS-dependent native implementations for Raya's standard library.

## Overview

This crate contains native (Rust) implementations of standard library functions that require OS-level APIs (file system, networking, processes, etc.). Uses name-based dispatch exclusively via `NativeFunctionRegistry`.

Cross-platform modules (math, crypto, url, etc.) live in `raya-stdlib`. This crate is for OS-dependent functionality.

## Architecture

```
raya-sdk (defines NativeHandler trait, NativeContext, NativeValue, IoRequest/IoCompletion)
    ↓
raya-stdlib-posix (implements register_posix(), depends on raya-sdk)
    ↓
raya-runtime (binds both raya-stdlib + raya-stdlib-posix)
```

## Module Structure

```
src/
├── lib.rs           # Crate entry, re-exports register_posix + PosixNativeHandler
├── handles.rs       # HandleRegistry<T> — generic thread-safe handle management
├── registry.rs      # register_posix() (name-based handler registration)
├── env.rs           # Env: get/set/remove/has/all/cwd/home/configDir/etc.
├── os.rs            # OS: platform/arch/hostname/cpus/memory/uptime/etc.
├── io.rs            # IO: readLine/readAll/write/writeln/writeErr/flush
├── fs.rs            # FS: readFile/writeFile/stat/mkdir/readDir/rename/copy/etc.
├── process.rs       # Process: exit/pid/argv/exec/spawn + title/heapUsed/signals/subprocess
├── net.rs           # Net: TCP/UDP/TLS sockets (listen/connect/read/write)
├── tls.rs           # TLS: rustls-based TLS stream wrapper
├── http.rs          # HTTP server: create/accept/respond + reqUrl/reqRemoteAddr
├── fetch.rs         # Fetch: HTTP client (GET/POST/PUT/DELETE) + resOk/resRedirected
├── dns.rs           # DNS: lookup/lookup4/lookup6/lookupMx/lookupTxt/etc.
├── terminal.rs      # Terminal: TTY detection, raw mode, cursor/screen control, key input
├── ws.rs            # WebSocket: client + server, send/receive text/bytes
├── readline.rs      # Readline: interactive line editor with history, prompts, select
├── glob_mod.rs      # Glob: pattern matching (find/findInDir/matches)
├── archive.rs       # Archive: tar/tar.gz/zip create/extract/list
└── watch.rs         # Watch: filesystem watcher (create/addPath/removePath/nextEvent)

raya/                # .raya source files and type definitions (paired .raya + .d.raya)
├── env.raya/d       # std:env — Environment variables and directories
├── os.raya/d        # std:os — OS info (platform, arch, memory, network, etc.)
├── io.raya/d        # std:io — Standard I/O (stdin/stdout/stderr)
├── fs.raya/d        # std:fs — File system operations
├── process.raya/d   # std:process — Process management, signals, subprocess spawning
├── net.raya/d       # std:net — TCP/UDP/TLS networking
├── http.raya/d      # std:http — HTTP server (HttpServer, HttpRequest)
├── fetch.raya/d     # std:fetch — HTTP client (Fetch, Response)
├── dns.raya/d       # std:dns — DNS resolution
├── terminal.raya/d  # std:terminal — Terminal control (TTY, cursor, screen, input)
├── ws.raya/d        # std:ws — WebSocket client + server
├── readline.raya/d  # std:readline — Interactive line editing
├── glob.raya/d      # std:glob — File pattern matching
├── archive.raya/d   # std:archive — Tar/Zip archive operations
└── watch.raya/d     # std:watch — Filesystem watching
```

## Key Patterns

### Handle Registry

Stateful resources (sockets, servers, child processes, watchers) use `HandleRegistry<T>`:

```rust
static TCP_LISTENERS: LazyLock<HandleRegistry<TcpListener>> =
    LazyLock::new(HandleRegistry::new);
```

Handles are integer IDs passed to/from Raya. `HandleRegistry` provides thread-safe insert/with/remove operations.

### IO-Bound Operations

OS operations that may block use the suspend pattern:

```rust
NativeCallResult::Suspend(IoRequest::BlockingWork {
    work: Box::new(move || {
        // Runs on IO thread pool, not VM worker
        let result = std::fs::read_to_string(&path);
        // Return IoCompletion
    }),
})
```

## Implementation Status

| Module | Status | Key Functions |
|--------|--------|---------------|
| env | Complete | get, set, remove, has, all, cwd, home, configDir, cacheDir, dataDir, stateDir, runtimeDir |
| os | Complete | platform, arch, hostname, cpus, totalMemory, freeMemory, uptime, networkInterfaces, etc. |
| io | Complete | readLine, readAll, write, writeln, writeErr, writeErrln, flush |
| fs | Complete | readFile, writeFile, stat, mkdir, readDir, remove, rename, copy, chmod, symlink, etc. |
| process | Complete | exit, pid, argv, exec, spawn, signals, title, setTitle, heapUsed, heapTotal |
| net | Complete | TCP (listen/connect/read/write), UDP (bind/sendTo/receive), TLS (connect/read/write) |
| http | Complete | serverCreate, serverCreateTls, accept, respond, respondBytes, respondWithHeaders, reqUrl, reqRemoteAddr |
| fetch | Complete | request (GET/POST/PUT/DELETE), resStatus, resText, resBytes, resOk, resRedirected |
| dns | Complete | lookup, lookup4, lookup6, lookupMx, lookupTxt, lookupSrv, lookupCname, lookupNs, reverse |
| terminal | Complete | isTerminal, columns, rows, rawMode, readKey, cursor control, screen control |
| ws | Complete | connect, serverCreate, send, sendBytes, receive, receiveBytes, close |
| readline | Complete | new, prompt, addHistory, loadHistory, saveHistory, simplePrompt, confirm, password, select |
| glob | Complete | find, findInDir, matches |
| archive | Complete | tar (create/extract/list), tgz (create/extract), zip (create/extract/list) |
| watch | Complete | create, createRecursive, nextEvent, addPath, removePath, close |

## For AI Assistants

- **Name-based dispatch only**: All posix modules use `NativeFunctionRegistry` (no numeric IDs)
- **Registration**: Add entries in `registry.rs` → `register_posix()`, organized by module
- **Handle pattern**: Use `HandleRegistry<T>` for stateful OS resources
- **Blocking ops**: Use `IoRequest::BlockingWork` for anything that may block (I/O, DNS, etc.)
- **Triple file pattern**: Each module needs `.rs` (Rust), `.raya` (source), `.d.raya` (types)
- **Module registration**: Also add `include_str!()` in `raya-engine/src/compiler/module/std_modules.rs`
- **TLS**: `tls.rs` provides `TlsStream` wrapper used by both `net.rs` and `http.rs`
