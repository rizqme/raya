# Milestone 1.15: Native Module System (Rust-Only)

**Status:** 📋 Planned
**Priority:** High
**Dependencies:** Milestone 1.14 (Module System - VM-side)
**Estimated Duration:** 3-4 weeks
**Target Completion:** TBD

**Design Decision:** Rust-only native modules for simplicity and safety
- C/C++ code must be wrapped in Rust first (use standard Rust FFI)
- Zero-cost abstraction through proc-macros
- Thread safety enforced by Rust's type system
- Simpler ABI (no C header files needed)

**Scope:**
- Rust ergonomic API with proc-macros (#[function], #[module])
- Thread safety infrastructure (Send/Sync enforcement)
- Migrate JSON parse/stringify from opcodes to std:json native module
- 8 Node.js parity modules (std:json, std:fs, std:crypto, std:buffer, std:http, std:net, std:events, std:stream)
- Cross-platform dynamic library loading (.so/.dylib/.dll)
- Configuration-based native bindings in raya.toml
- Type definition integration (.d.raya files) for native modules
- Pre-configured standard library native modules

---

## Overview

Enable Raya programs to call native functions written in Rust. C/C++ libraries can be wrapped using Rust's standard FFI. This milestone implements a complete native module infrastructure with ergonomic Rust bindings, automatic marshalling, and thread safety.

**Architecture:**
```
Raya Program (.raya)
    ↓ imports std:json or custom:mylib (transparent - no "native:" prefix)
Module Resolver
    ↓ detects implementation type (bytecode .ryb vs native .so/.dylib/.dll)
Dynamic Library Loader (for native modules)
    ↓ loads native implementation
Native Module (Rust)
    ↓ implements raya-ffi API
VM Function Registry
```

**Key Principle: Transparency**
- From Raya code perspective, all modules look the same
- `import { parse } from "std:json"` - user doesn't know it's native
- Module resolver automatically detects `.ryb` (bytecode) vs `.so/.dylib/.dll` (native)
- No special syntax or imports for native modules

---

## Goals & Checkboxes

### [ ] Phase 1: C FFI API (Week 1)

**Core Infrastructure:**
- [ ] Define opaque handle types (RayaContext, RayaValue, RayaModule)
- [ ] Implement value type checking functions (is_null, is_bool, is_i32, etc.)
- [ ] Implement value conversion functions (to_bool, to_i32, to_string, etc.)
- [ ] Implement value constructor functions (from_null, from_bool, from_i32, etc.)
- [ ] Create module builder API for registration
- [ ] Add error handling functions (set_error, get_last_error, clear_error)

**Array/Object Accessors:**
- [ ] Array operations (length, get, set, push)
- [ ] Object operations (get, set, has)

**Thread Safety Functions:**
- [ ] Atomic reference counting (value_ref, value_unref)
- [ ] Thread-local storage (tls_get, tls_set, tls_remove)
- [ ] Synchronization helpers (mutex_new, mutex_lock, mutex_unlock)

**Deliverables:**
- [ ] C header file `raya/module.h` with complete API
- [ ] FFI implementation in `raya-ffi/src/lib.rs`
- [ ] Unit tests for all FFI functions
- [ ] Example C module demonstrating API usage

---

### [ ] Phase 2: Dynamic Library Loader (Week 1-2)

**Cross-Platform Loading:**
- [ ] Implement platform detection (Linux, macOS, Windows)
- [ ] Linux dynamic loading (dlopen, dlsym, dlclose)
- [ ] macOS dynamic loading (same as Linux with dylib extension)
- [ ] Windows dynamic loading (LoadLibrary, GetProcAddress, FreeLibrary)
- [ ] Handle platform-specific library naming conventions
  - Linux: `libnative_<name>.so`
  - macOS: `libnative_<name>.dylib`
  - Windows: `native_<name>.dll`

**Symbol Resolution:**
- [ ] Resolve module init function (`raya_module_init_<name>`)
- [ ] Validate function signature and ABI version
- [ ] Handle missing or malformed symbols gracefully

**Module Search Path:**
- [ ] Search current directory `./native_modules/`
- [ ] Search `$RAYA_MODULE_PATH` environment variable (colon-separated)
- [ ] Search user modules `~/.raya/modules/`
- [ ] Search system modules `/usr/local/lib/raya/modules/`
- [ ] Cache resolved paths for performance

**Module Registry:**
- [ ] Track loaded modules by name
- [ ] Prevent duplicate loading (singleton pattern)
- [ ] Keep library handle alive (prevent unload while in use)
- [ ] Reference count for safe unloading

**ABI Version Checking:**
- [ ] Read ABI version from loaded module
- [ ] Reject incompatible MAJOR versions
- [ ] Accept compatible MINOR/PATCH versions
- [ ] Provide clear error messages for version mismatches

**Deliverables:**
- [ ] `raya-core/src/native/loader.rs` with NativeModuleLoader
- [ ] `raya-core/src/native/module.rs` with LoadedModule struct
- [ ] Cross-platform tests (Linux, macOS, Windows)
- [ ] Documentation on module search paths and naming

---

### [ ] Phase 3: Native Function Invocation (Week 2)

**Call Mechanism:**
- [ ] Add CALL_NATIVE opcode or extend CALL opcode
- [ ] Pop arguments from stack
- [ ] Convert Raya values to C representations
- [ ] Pin values to prevent GC during call
- [ ] Call native function pointer
- [ ] Check for errors (NULL return indicates error)
- [ ] Convert result back to Raya value
- [ ] Unpin arguments
- [ ] Push result or throw exception

**Value Marshalling:**
- [ ] Marshal primitives (i32, f64, bool, null) by value (~1-5ns)
- [ ] Pass strings as opaque handles with accessors (zero-copy)
  - [ ] `raya_string_data()` - Get const char* (pinned, O(1))
  - [ ] `raya_string_len()` - Get length (O(1))
- [ ] Pass objects/arrays as opaque handles (zero-copy)
- [ ] Implement type-aware optimized casts (when static type known)
  - [ ] Fast path for typed value → primitive conversions
  - [ ] Skip runtime type checks when compiler provides type info

**GC Safety:**
- [ ] Disable GC during native call (safepoint disabled)
- [ ] Pin values passed to native code
- [ ] Automatic unpinning on return
- [ ] Ensure no GC runs while native code holds references

**Error Handling:**
- [ ] Catch native panics and convert to Raya exceptions
- [ ] Stack overflow checks before native call
- [ ] Resource cleanup on error (RAII guards)
- [ ] Thread-local error storage

**Deliverables:**
- [ ] Native call infrastructure in interpreter
- [ ] Marshalling implementation and tests
- [ ] Error handling tests
- [ ] Performance benchmarks (call overhead ~50-100ns)

---

### [ ] Phase 4: Rust Ergonomic API (Week 2-3)

**Proc Macros:**
- [ ] `#[function]` attribute macro for automatic FFI wrapping
- [ ] `#[module]` attribute macro for module definition
- [ ] Automatic argument marshalling
- [ ] Automatic return value conversion
- [ ] Panic catching and error conversion

**Type Conversions:**
- [ ] Automatic conversions for primitives (bool, i32, i64, f32, f64)
- [ ] Automatic conversions for strings (String, &str)
- [ ] Automatic conversions for collections (Vec<T>, HashMap<K, V>)
- [ ] Automatic conversions for Option<T> (null handling)
- [ ] Automatic conversions for Result<T, E> (exception handling)

**Traits:**
- [ ] FromRaya trait for custom type extraction
- [ ] ToRaya trait for custom type conversion
- [ ] Context trait for native function context
- [ ] Error trait for native error types

**Send/Sync Enforcement:**
- [ ] Enforce Send bound for values passed to spawned tasks
- [ ] Enforce Sync bound for shared mutable state
- [ ] Type system prevents data races

**Deliverables:**
- [ ] `raya-native` crate with proc-macros
- [ ] Example Rust modules demonstrating all features
- [ ] Comprehensive documentation
- [ ] Migration guide from C API

---

### [ ] Phase 5A: Core Native Modules (Week 3)

**[ ] native:json Module**
- [ ] parse(input: string): json - Parse JSON string
- [ ] stringify(value: json, pretty?: bool): string - Convert to JSON string
- [ ] validate(input: string): bool - Validate JSON syntax
- [ ] Migration from JsonParse/JsonStringify opcodes
- [ ] Deprecation plan for opcodes (1-2 release cycles)

**[ ] native:fs Module**
- [ ] readFile(path: string): string - Read file to string
- [ ] writeFile(path: string, content: string) - Write string to file
- [ ] exists(path: string): bool - Check file existence
- [ ] mkdir(path: string) - Create directory
- [ ] readdir(path: string): string[] - List directory contents
- [ ] stat(path: string): FileStat - Get file metadata
- [ ] Async implementations using tokio::fs

**[ ] native:crypto Module**
- [ ] hash(algorithm: string, data: string): string - Hash data (SHA256, SHA512, SHA1, MD5)
- [ ] randomBytes(size: number): Buffer - Cryptographically secure random
- [ ] randomInt(min: number, max: number): number - Random integer in range
- [ ] randomUUID(): string - Generate UUIDv4

**[ ] native:buffer Module**
- [ ] alloc(size: number, fill?: number): Buffer - Allocate buffer
- [ ] from(data: string | number[]): Buffer - Create from data
- [ ] concat(buffers: Buffer[]): Buffer - Concatenate buffers
- [ ] compare(buf1: Buffer, buf2: Buffer): number - Compare buffers
- [ ] toString(buffer: Buffer, encoding?: string): string - Convert to string
- [ ] write(buffer: Buffer, string: string, offset?: number): number - Write string
- [ ] slice(buffer: Buffer, start: number, end?: number): Buffer - Extract slice
- [ ] copy(source: Buffer, target: Buffer, targetStart?: number): number - Copy data
- [ ] Thread-safe implementation using Arc<Vec<u8>>

**Deliverables:**
- [ ] 4 modules implemented and tested
- [ ] Integration tests for each module
- [ ] Security audit for crypto operations
- [ ] Documentation with usage examples

---

### [ ] Phase 5B: Networking Native Modules (Week 3-4)

**[ ] native:http Module**
- [ ] get(url: string, options?: HttpOptions): HttpResponse - HTTP GET
- [ ] post(url: string, body: string | Buffer, options?: HttpOptions): HttpResponse - HTTP POST
- [ ] request(options: HttpRequestOptions): HttpResponse - Generic request
- [ ] createServer(handler: Function): HttpServer - Create HTTP server
- [ ] listen(server: HttpServer, port: number, host?: string) - Start listening
- [ ] close(server: HttpServer) - Stop server
- [ ] Implementation using reqwest (client) and hyper (server)

**[ ] native:net Module**
- [ ] createServer(handler: Function): TcpServer - Create TCP server
- [ ] connect(port: number, host: string): TcpSocket - Connect to server
- [ ] listen(server: TcpServer, port: number, host?: string) - Start listening
- [ ] close(server: TcpServer) - Stop server
- [ ] createSocket(type: "udp4" | "udp6"): UdpSocket - Create UDP socket
- [ ] bind(socket: UdpSocket, port: number, address?: string) - Bind to address
- [ ] send(socket: UdpSocket, data: Buffer, port: number, address: string): number - Send datagram
- [ ] recv(socket: UdpSocket): { data: Buffer, address: string, port: number } - Receive datagram
- [ ] Implementation using tokio::net

**[ ] native:events Module**
- [ ] createEmitter(): EventEmitter - Create event emitter
- [ ] on(emitter: EventEmitter, event: string, listener: Function) - Add listener
- [ ] once(emitter: EventEmitter, event: string, listener: Function) - Add one-time listener
- [ ] off(emitter: EventEmitter, event: string, listener: Function) - Remove listener
- [ ] emit(emitter: EventEmitter, event: string, ...args: any[]): bool - Emit event
- [ ] removeAllListeners(emitter: EventEmitter, event?: string) - Remove all listeners
- [ ] listeners(emitter: EventEmitter, event: string): Function[] - Get listeners
- [ ] Thread-safe implementation using Arc<RwLock<HashMap>>

**[ ] native:stream Module**
- [ ] createReadStream(path: string, options?: ReadStreamOptions): Readable - Create read stream
- [ ] createWriteStream(path: string, options?: WriteStreamOptions): Writable - Create write stream
- [ ] pipe(source: Readable, dest: Writable): Writable - Pipe data
- [ ] pipeline(...streams: Stream[]) - Chain multiple streams
- [ ] Readable interface (read, on, pause, resume, isPaused)
- [ ] Writable interface (write, end, on)
- [ ] Duplex interface (both readable and writable)
- [ ] Transform interface (modify data as it passes through)
- [ ] Implementation using tokio::io

**Deliverables:**
- [ ] 4 modules implemented and tested
- [ ] Integration tests for each module
- [ ] Performance benchmarks (throughput, latency)
- [ ] Documentation with usage examples

---

### [ ] Phase 6: Thread Safety & Documentation (Week 4)

**Thread Safety Infrastructure:**
- [ ] Implement atomic reference counting for shared values
- [ ] Add thread-local storage support
- [ ] Enforce Send/Sync bounds in Rust API
- [ ] Implement value pinning mechanism
- [ ] Add synchronization primitives (Mutex, RwLock wrappers)

**Thread Safety Rules:**
- [ ] Document ownership model (owned, borrowed, shared)
- [ ] Document pure functions (always thread-safe)
- [ ] Document immutable shared state (Arc<T>)
- [ ] Document mutable shared state (Arc<Mutex<T>>)
- [ ] Document thread-local state (thread_local! macro)
- [ ] Document atomic operations (AtomicU64, etc.)

**Safety Guarantees:**
- [ ] No data races (Rust type system prevents unsynchronized mutable access)
- [ ] No deadlocks (async functions never hold locks across await points)
- [ ] No use-after-free (reference counting ensures values live long enough)
- [ ] No double-free (Drop trait and RAII ensure cleanup happens exactly once)

**Documentation:**
- [ ] C API reference (all functions, types, macros)
- [ ] Rust API guide (proc-macros, traits, type conversions)
- [ ] Tutorial for C module authors
- [ ] Tutorial for Rust module authors
- [ ] Thread safety guide
- [ ] Migration guide for JSON opcodes
- [ ] Performance optimization guide

**Testing:**
- [ ] 30+ integration tests (all modules)
- [ ] Thread safety stress tests (concurrent scenarios)
- [ ] Cross-platform tests (Linux, macOS, Windows)
- [ ] Performance benchmarks (call overhead, throughput)
- [ ] Memory safety tests (Valgrind, AddressSanitizer)

**Deliverables:**
- [ ] Complete thread-safe marshalling infrastructure
- [ ] Comprehensive documentation (API reference, tutorials, guides)
- [ ] 30+ passing integration tests
- [ ] Performance baseline established
- [ ] Example projects demonstrating all features

---

## Design Decisions

### ABI Stability

**Versioning:**
- Semantic versioning (MAJOR.MINOR.PATCH)
- MAJOR = breaking changes (incompatible)
- MINOR = new functions (backward compatible)
- PATCH = bug fixes (no API/ABI changes)
- Current ABI version: 1.0.0

**Compatibility Checking:**
- VM checks module ABI version at load time
- Reject incompatible MAJOR version
- Accept same or older MINOR version
- Clear error messages for version mismatches

---

### Value Marshalling

**Zero-Copy Marshalling Strategy:**
- **Primitives (i32, f64, bool, null):** Passed by value (copy, ~1-5ns)
- **Strings:** Passed as opaque handles with safe accessors (zero-copy)
  - `raya_string_data(str)` - Get const char* pointer (pinned)
  - `raya_string_len(str)` - Get length
  - No deep copy, GC-pinned during native call
- **Objects/Arrays:** Passed as opaque handles (zero-copy)
  - Accessor functions for property/element access
  - No marshalling overhead
- **All handles pinned during native call** (GC safety)

**Type-Aware Optimization:**
- Compiler knows static types, can use optimized casts
- `RayaValue* → i32` fast path when type is known
- `RayaValue* → string` fast path when type is known
- Avoids runtime type checks for typed functions

**GC Safety:**
- Pin all handles before passing to native code
- Automatic unpinning on return
- No GC while native code holds references
- Long-running functions should yield periodically

**Performance Target:**
- Primitive marshalling: ~1-5ns (direct copy)
- String/Object handles: ~1ns (pointer copy)
- Total FFI overhead: ~25-50ns (within budget)

---

### Thread Safety

**Design Principles:**
- Native modules must be thread-safe by default
- Values are thread-local or explicitly protected
- Shared mutable state requires synchronization
- Send/Sync bounds enforced at compile time

**Ownership Model:**
- **Owned:** Passed by value, safe to mutate
- **Borrowed:** Read-only reference, safe to share
- **Shared:** Arc-wrapped, immutable or internally synchronized

**Safety Mechanisms:**
- Atomic reference counting (value_ref, value_unref)
- Thread-local storage for task-local caching
- Synchronization primitives (Mutex, RwLock)
- Send/Sync enforcement in Rust API
- Atomic operations for lockless counters

---

### Error Handling

**Error Propagation:**
- Native functions return NULL on error
- Last error stored in thread-local storage
- VM converts NULL to Raya exception
- Error objects contain message and stack trace

**Panic Handling:**
- Native panics caught at FFI boundary
- Converted to Raya exceptions
- Stack unwinding prevented
- Resources cleaned up via RAII

---

### Dynamic Library Loading

**Platform-Specific Details:**

**Linux:**
- Use `dlopen()` with RTLD_NOW | RTLD_LOCAL flags
- Use `dlsym()` for symbol resolution
- Use `dlclose()` for cleanup
- Library naming: `libnative_<name>.so`

**macOS:**
- Same API as Linux (POSIX dlopen)
- Library naming: `libnative_<name>.dylib`
- Code signing requirements for .dylib files
- Notarization may be required for distribution

**Windows:**
- Use `LoadLibrary()` or `LoadLibraryEx()`
- Use `GetProcAddress()` for symbol resolution
- Use `FreeLibrary()` for cleanup
- Library naming: `native_<name>.dll`
- Handle DLL search path security

**Security Considerations:**
- Validate library paths (prevent path traversal)
- Check library signatures on supported platforms
- Limit search paths to trusted directories
- Prevent DLL hijacking on Windows
- Log all library load attempts

**Error Handling:**
- Platform-specific error messages
- Linux: `dlerror()` for error strings
- Windows: `GetLastError()` + `FormatMessage()`
- Clear, actionable error messages
- Include search paths in error output

---

### Module Search Path

**Priority Order:**
1. Current directory `./native_modules/`
2. `$RAYA_MODULE_PATH` (colon-separated on Unix, semicolon on Windows)
3. User modules `~/.raya/modules/`
4. System modules `/usr/local/lib/raya/modules/` (or Windows equivalent)

**Path Resolution:**
- Resolve relative paths relative to current working directory
- Expand `~` to user home directory
- Normalize paths (remove `.` and `..`)
- Check file existence and permissions
- Cache resolved paths for performance

---

### JSON Opcode Migration

**Migration Strategy:**
- JsonParse (0xE0) and JsonStringify (0xE1) deprecated
- Existing bytecode redirected to native:json module
- Compiler generates native module calls for new code
- Opcodes retained for 1-2 release cycles
- Then removed to free opcode space

**Rationale:**
- **Consistency:** All I/O and parsing in native modules
- **Flexibility:** Easy to extend (pretty-print, custom reviver)
- **Opcode Budget:** Frees 2 opcodes for core VM operations
- **Performance:** Native implementation can be optimized independently
- **Maintenance:** JSON library updates don't require VM recompilation

---

## Success Criteria

### Must Have

- [ ] Complete C FFI API (raya-ffi crate)
- [ ] Cross-platform dynamic library loading (Linux, macOS, Windows)
- [ ] Value marshalling (all primitive types)
- [ ] Thread-safe value marshalling (atomic refcounting)
- [ ] Error handling (exceptions from native code)
- [ ] GC safety (pinning mechanism)
- [ ] Rust ergonomic API (proc-macros)
- [ ] native:json module (parse, stringify - moved from opcodes)
- [ ] native:fs module (basic file operations)
- [ ] native:crypto module (hash, random)
- [ ] native:buffer module (binary data manipulation)
- [ ] Documentation (C API reference, Rust guide)
- [ ] Integration tests (>90% coverage)
- [ ] Thread safety tests (concurrent scenarios)

### Should Have

- [ ] native:http module (HTTP client/server)
- [ ] native:net module (TCP/UDP sockets)
- [ ] native:events module (event emitter)
- [ ] native:stream module (stream abstraction)
- [ ] Async native functions (via Rust async)
- [ ] Custom type conversions (FromRaya/ToRaya traits)
- [ ] Thread-local storage (TLS) support
- [ ] Send/Sync enforcement in Rust API
- [ ] Module hot-reloading (for development)
- [ ] ABI compatibility checking
- [ ] Performance benchmarks (including concurrent)

### Nice to Have

- [ ] Native debugger integration
- [ ] Profiling native function calls
- [ ] Automatic binding generation (from C headers)
- [ ] WebAssembly native modules (wasm32-unknown-unknown)

---

## Dependencies

### External Crates

**raya-ffi:**
- libc (POSIX types)
- parking_lot (thread-safe synchronization)

**raya-core:**
- libloading (cross-platform dynamic loading)
- tokio (async runtime for native modules)

**raya-native:**
- raya-ffi (FFI bindings)
- proc-macro2, quote, syn (proc-macros)

**stdlib/native/json:**
- serde_json (JSON parsing/stringification)

**stdlib/native/http:**
- reqwest (HTTP client)
- hyper (HTTP server)
- tokio (async runtime)

**stdlib/native/net:**
- tokio (networking)

**stdlib/native/stream:**
- tokio (I/O utilities)

**stdlib/native/crypto:**
- sha2, sha1, md-5 (hashing)
- rand (random number generation)
- uuid (UUID generation)

---

## Risks and Mitigations

### Risk 1: ABI Stability
**Impact:** High
**Probability:** Medium
**Mitigation:**
- Strict versioning enforcement
- Comprehensive ABI tests
- Document breaking changes clearly
- Provide migration tools

### Risk 2: Memory Corruption
**Impact:** Critical
**Probability:** Low
**Mitigation:**
- Extensive testing (Valgrind, AddressSanitizer)
- Safe Rust API (prevent UB)
- Runtime validation
- Fuzzing

### Risk 3: Cross-Platform Issues
**Impact:** High
**Probability:** Medium
**Mitigation:**
- Test on Linux, macOS, Windows
- Use cross-platform libraries
- Document platform-specific behavior
- CI/CD on all platforms

### Risk 4: Thread Safety Bugs
**Impact:** Critical
**Probability:** Medium
**Mitigation:**
- Rust type system prevents data races
- Comprehensive thread safety tests
- ThreadSanitizer during development
- Careful code review

### Risk 5: Security Vulnerabilities
**Impact:** Critical
**Probability:** Medium
**Mitigation:**
- Security audit before release
- Sandboxing documentation
- Validate library paths
- Regular dependency updates

---

## Summary

Milestone 1.15 implements a complete native module system with Node.js parity, enabling seamless interop between Raya and C/C++/Rust code.

**Key Features:**
- [ ] Complete C FFI with thread safety primitives
- [ ] Cross-platform dynamic library loading (Linux, macOS, Windows)
- [ ] Ergonomic Rust API with automatic marshalling
- [ ] 8 standard native modules (json, fs, crypto, buffer, http, net, events, stream)
- [ ] Moved JSON parse/stringify from opcodes to native module
- [ ] Full Node.js parity for core I/O operations
- [ ] Superior thread safety guarantees

**Design Priorities:**
1. **Safety:** Memory-safe API, thread-safe marshalling, GC integration
2. **Ergonomics:** Easy C API, delightful Rust API with proc-macros
3. **Performance:** Low-overhead calls (~50-100ns), efficient marshalling
4. **Compatibility:** Stable ABI, version checking, cross-platform
5. **Thread Safety:** Atomic refcounting, TLS support, Send/Sync enforcement

**Target:** Production-ready native module system matching/exceeding Node.js N-API quality with superior thread safety.

**Timeline:** 4-5 weeks across 6 implementation phases.

**Risk Level:** Medium-High (FFI complexity, thread safety, platform-specific code, async integration, security implications)
