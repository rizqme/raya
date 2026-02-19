# CLAUDE.md - Raya Project

**Raya** is a statically-typed language with TypeScript syntax, implemented in Rust. Custom bytecode VM with goroutine-style concurrency. Fully static type system with zero runtime type checks.

---

## ⚠️ Documentation Maintenance (MANDATORY)

**After EVERY turn, update relevant documentation:**

1. **Milestone files** (`plans/milestone-*.md`) - Mark tasks `[x]`, update status
2. **PLAN.md** (`plans/PLAN.md`) - Update overall progress and current focus
3. **Hierarchical CLAUDE.md** (`crates/**/CLAUDE.md`) - Keep concise, key info only
4. **Design docs** (`docs/`) - If behavior/API changes
5. **This file** - Update status section if milestone progress changes

---

## Current Status

**Complete:** Milestones 3.4-3.6, 3.9, Milestone 3.7 Phases 2-6 (module system)
**Milestone 3.8 (Reflection API):** Core implementation complete
- Phases 1-17 handlers done (metadata, introspection, proxies, dynamic classes, bytecode generation, permissions, VM bootstrap)
- Phases 12, 18 blocked (framework tests, performance validation) - waiting for compiler `std:` import support
- 149+ reflect unit tests passing

**Milestone 3.9 (Decorators):** Complete (41/41 e2e tests)
- Type aliases registered (`ClassDecorator<T>`, `MethodDecorator<F>`, `ParameterDecorator<T>`, etc.)
- `Class<T>` interface available
- Phase 3 code generation complete with decorator lowering
- Phase 4 runtime infrastructure complete (`DecoratorRegistry`, wrappers)
- Phase 5 integration tests: 41/41 passing
  - Class, method, field, parameter decorators work
  - Decorator factories work (including chained `@Factory("a")("b")`)
  - Framework patterns (HTTP routing, DI, validation, ORM) work
  - Inheritance with decorators works

**Milestone 4.1 (Core Types):** Complete
- Date: All 20 handlers implemented (getters, setters, formatting, parse)
- Object: hashCode, equals, toString handlers implemented
- Task: isDone, isCancelled handlers implemented (via `self.tasks` RwLock)
- Error: ERROR_STACK handler implemented (placeholder — needs stack capture at throw time)
- Number: toFixed, toPrecision, toString(radix) — native IDs, compiler lowering, VM handlers all complete
- String-RegExp bridge verified, RegExp.replaceWith implemented with callback support
- 34 new e2e tests added

**Milestone 4.2 (std:logger):** Complete
- ✅ Phase 1: Infrastructure complete
  - `export default` support (parser, binder, typedef, visitor)
  - `std:` module registry with source embedding
  - `NativeHandler` trait for stdlib decoupling
  - `raya-runtime` crate binds engine + stdlib
  - logger.raya with debug/info/warn/error methods
- ✅ Phase 2: Console global removed
  - design/STDLIB.md updated (console → logger reference)
  - Reflect bootstrap: CONSOLE_LOG → LOGGER_INFO
  - CLAUDE.md files updated (stdlib, VM)
- ✅ Phase 3: Documentation migration complete
  - 36 files migrated: all design/*.md, plans/*.md, crate files, root files
  - console.log → logger.info (87 in LANG.md, 16 in MAPPING.md, 18 in REFLECTION.md)
  - console.error → logger.error, console.warn → logger.warn
  - Zero remaining console.* references (verified via grep)
- ✅ Phase 4: E2E tests moved to raya-runtime
  - All 594 e2e tests moved from raya-engine to raya-runtime
  - Tests run with StdNativeHandler integration
- Tests: 843 engine lib, 854 runtime (e2e + unit), 17 stdlib

**Milestone 4.8 (std:path):** Complete (codec removed — not in initial release)
- ✅ Phase 1: Native IDs & engine infrastructure complete
  - `pub mod path` (0x6000-0x600A) in builtin.rs
  - Cargo.toml deps (pathdiff)
- ✅ Phase 2: std:path complete
  - 14 methods (11 native + 3 pure Raya): join, normalize, dirname, basename, extname, isAbsolute, resolve, relative, cwd, sep, delimiter, stripExt, withExt, isRelative
  - Engine-side handler with GC context for string allocation
  - 21 e2e tests (19 passing, 2 ignored — CallMethod in nested call)

**Milestone 4.3 (std:math):** Complete
- ✅ All 4 phases complete: native IDs, Math.raya + stdlib, VM dispatch, e2e tests
- 22 functions: abs, sign, floor, ceil, round, trunc, min, max, pow, sqrt, sin, cos, tan, asin, acos, atan, atan2, exp, log, log10, random, PI, E
- Native ID range: 0x2000-0x2016
- 44 e2e tests + 10 stdlib unit tests passing

**Milestone 4.6 (std:crypto):** Complete
- ✅ All 4 phases complete: native IDs, Crypto.raya, engine handler, e2e tests
- 12 methods: hash, hashBytes, hmac, hmacBytes, randomBytes, randomInt, randomUUID, toHex, fromHex, toBase64, fromBase64, timingSafeEqual
- Engine-side handler (not NativeHandler) — needs direct Buffer/GC heap access
- Native ID range: 0x4000-0x400B
- Algorithms: SHA-256, SHA-384, SHA-512, SHA-1, MD5 (hash); SHA-256/384/512 (HMAC)
- 27 e2e tests passing

**Milestone 4.7 (std:time):** Complete
- ✅ All 3 phases complete: native IDs, Time.raya + engine handler, e2e tests
- 12 methods: now, monotonic, hrtime, elapsed, sleep, sleepMicros, seconds, minutes, hours, toSeconds, toMinutes, toHours
- Only 5 native calls (system operations); 7 pure Raya methods (duration conversions)
- Engine-side handler with `LazyLock<Instant>` monotonic epoch
- Native ID range: 0x5000-0x5004
- 19 e2e tests passing

**Milestone 4.4 (std:reflect):** Not Started
- Expose existing Reflect API (141+ handlers) as `std:reflect` module
- Create Reflect.raya with `__NATIVE_CALL` wrappers, register in std_modules
- Fix `is_reflect_method()` range to cover 0x0D00-0x0E2F
- Handlers stay in raya-engine (need VM internals), no raya-stdlib module needed
- E2E tests for metadata, introspection, field access, proxies, permissions

**Milestone 4.9 (Native ABI Refactor):** In Progress (Phases 1-2 Complete)
- Unified NativeHandler interface with full VM context (GC, classes, scheduler)
- Type-safe NativeValue wrapper (no string parsing)
- Support for all value types (primitives, strings, buffers, objects, arrays)
- Phase 1: Core ABI infrastructure complete (331 lines)
- Phase 2: Interface refactor complete (NativeHandler trait updated)
- Remaining: VM dispatcher migration, stdlib handler migration, crypto migration
- Design doc: [docs/native/abi.md](docs/native/abi.md)

**Milestone 4.5 (std:runtime):** In Progress (Phases 1-8 Complete)
- `import { Compiler, Bytecode, Vm, Parser, TypeChecker, VmInstance, BytecodeBuilder, ClassBuilder, DynamicModule } from "std:runtime"` — 9 exports, named
- ✅ Phase 1: Compiler + Bytecode I/O complete
  - Compiler: compile, compileExpression, eval, execute, executeFunction
  - Bytecode: encode, decode, loadLibrary, loadDependency, resolveDependency
  - 15 e2e tests
- ✅ Phase 2: Bytecode Inspection + Parser + TypeChecker complete
  - Bytecode: validate, disassemble, getModuleName, getModuleFunctions, getModuleClasses
  - Parser: parse, parseExpression; TypeChecker: check, checkExpression; Compiler.compileAst
  - 11 new e2e tests (26 total)
- ✅ Phase 3: VM Instances & Isolation complete
  - Vm.current(), Vm.spawn() → VmInstance with id/isRoot/isAlive/isDestroyed
  - VmInstance: compile, execute, eval, loadBytecode, runEntry, terminate
  - VmInstanceRegistry with per-instance module isolation, cascading terminate
  - Compiler fix: variable_class_map from type annotations + method_return_class_map for chained calls
  - 13 new e2e tests (39 total runtime tests)
- ✅ Phase 4: Permission Management complete
  - VmPermissions struct (eval, binaryIO, vmSpawn, vmAccess, libLoad, reflect, nativeCalls, allowStdlib)
  - Vm.hasPermission(), getPermissions(), getAllowedStdlib(), isStdlibAllowed()
  - Permission checks integrated into dispatch (eval, encode/decode, loadLibrary, loadDependency, resolveDependency, spawn)
  - 11 new e2e tests (50 total runtime tests)
- ✅ Phase 5: VM Introspection & Resource Control complete
  - Vm: heapUsed, heapLimit, taskCount, concurrency, threadCount, gcCollect, gcStats, version, uptime, loadedModules, hasModule
  - heapUsed queries GC heap_stats; threadCount uses available_parallelism; version returns "0.1.0"
  - 11 new e2e tests (61 total runtime tests)
- ✅ Phase 6: Bytecode Builder complete
  - BytecodeBuilder class wrapping reflect handlers 0x0DF0-0x0DFD
  - 16 methods: emit, emitWithArg, emitWithArgs, emitPush, defineLabel, markLabel, emitJump, emitJumpIf, declareLocal, emitLoadLocal, emitStoreLocal, emitCall, emitReturn, emitReturnVoid, validate, build
  - Added 14 bytecode builder match arms to core.rs call_reflect_method
  - 5 new e2e tests (66 total runtime tests)
- ✅ Phase 7: Dynamic Modules & Runtime Types complete
  - ClassBuilder class wrapping reflect handlers 0x0DE0-0x0DE6 (addField, addMethod, setConstructor, setParent, addInterface, build)
  - DynamicModule class wrapping reflect handlers 0x0E10-0x0E15 (addFunction, addClass, addGlobal, seal, link)
  - Added 13 dispatch match arms to core.rs (7 ClassBuilder + 6 DynamicModule)
  - 10 new e2e tests (76 total runtime tests)
- ✅ Phase 8: E2E Tests complete
  - 20 new gap/integration tests covering all phases
  - Phase 1 gaps: executeFunction, eval complex, encode-decode-execute roundtrip
  - Phase 2 gaps: getModuleName, checkExpression, validate after decode
  - Phase 3 gaps: loadBytecode+execute, runEntry, fault containment, multiple evals, unique spawn IDs
  - Cross-phase: full pipeline, BytecodeBuilder+DynamicModule, ClassBuilder+DynamicModule, labels+jumps
  - 96 total runtime e2e tests
- Remaining: documentation (Phase 9)

**Milestone 4.10 (VM Unification):** Complete
- Unified 3 duplicate executors into single `Interpreter::run()`
- Extracted opcodes to 15 categorized modules (`interpreter/opcodes/`)
- Extracted native handlers to 4 modules (`interpreter/handlers/`)
- Added compiler intrinsics for replaceWith (inline loop + CallClosure)
- Deleted TaskExecutor + execute_nested_function (~3,400 lines)
- Net ~9,500 lines removed across 35 files

**Tests:** 3,128+ total (1,672 engine + 142 JIT, 1,297 runtime, 17 stdlib) — 0 ignored

**JIT Compilation (feature-gated):** Complete
- Cranelift backend with NaN-boxing ABI, SSA lifter, optimization passes
- Static analysis heuristics for hot function detection
- Pre-warming pipeline (compile at module load)
- Vm integration: `enable_jit()`, automatic pre-warm in `execute()`
- `cargo build --features jit` / `cargo test --features jit`
- 142 tests (83 unit + 59 integration with native code execution)

See [plans/milestone-3.8.md](plans/milestone-3.8.md), [plans/milestone-3.9.md](plans/milestone-3.9.md), [plans/milestone-4.1.md](plans/milestone-4.1.md), [plans/milestone-4.2.md](plans/milestone-4.2.md), [plans/milestone-4.3.md](plans/milestone-4.3.md), [plans/milestone-4.4.md](plans/milestone-4.4.md), [plans/milestone-4.5.md](plans/milestone-4.5.md), [plans/milestone-4.6.md](plans/milestone-4.6.md), [plans/milestone-4.7.md](plans/milestone-4.7.md), [plans/milestone-4.8.md](plans/milestone-4.8.md), and [plans/milestone-4.9.md](plans/milestone-4.9.md) for details.

---

## Critical Design Rules

### Type System
- `typeof` for primitive unions (`string | int | number | boolean | null`)
- `instanceof` for class type checking
- Discriminated unions for complex types (required discriminant field)
- **BANNED:** `any` type, runtime type tags/RTTI

### Concurrency
- `async` functions create Tasks (green threads), start immediately
- `await` suspends current Task (doesn't block OS thread)
- Work-stealing scheduler across CPU cores

### Compilation
- Monomorphization (generics specialized at compile time)
- Typed opcodes: `IADD` (int), `FADD` (float/number)
- No runtime type checking overhead

---

## Key Documents

| Document | Purpose |
|----------|---------|
| [docs/language/lang.md](docs/language/lang.md) | Language specification |
| [docs/runtime/architecture.md](docs/runtime/architecture.md) | VM architecture |
| [docs/compiler/opcode.md](docs/compiler/opcode.md) | Bytecode instructions |
| [docs/compiler/mapping.md](docs/compiler/mapping.md) | Compilation patterns |
| [plans/milestone-3.7.md](plans/milestone-3.7.md) | Module system |
| [plans/milestone-3.8.md](plans/milestone-3.8.md) | Reflection API |
| [plans/milestone-3.9.md](plans/milestone-3.9.md) | Decorators (uses Reflection) |
| [plans/milestone-4.1.md](plans/milestone-4.1.md) | Core Types (built-in functions) |
| [plans/milestone-4.2.md](plans/milestone-4.2.md) | std:logger (replaces logger.info) |
| [plans/milestone-4.3.md](plans/milestone-4.3.md) | std:math (20 methods + PI, E) |
| [plans/milestone-4.4.md](plans/milestone-4.4.md) | std:reflect (141+ handlers as module) |
| [plans/milestone-4.5.md](plans/milestone-4.5.md) | std:vm (compile, execute, isolation, permissions) |
| [plans/milestone-4.6.md](plans/milestone-4.6.md) | std:crypto (hashing, HMAC, random, encoding) |
| [plans/milestone-4.7.md](plans/milestone-4.7.md) | std:time (clocks, sleep, duration utilities) |
| [plans/milestone-4.8.md](plans/milestone-4.8.md) | std:path (path manipulation) |
| [plans/milestone-4.9.md](plans/milestone-4.9.md) | Native ABI Refactor (unified interface, full VM context) |
| [docs/native/abi.md](docs/native/abi.md) | Native ABI design specification |

---

## Project Structure

```
crates/
├── raya-engine/     # Parser, compiler, VM, JIT (main crate)
├── raya-runtime/    # Binds engine + stdlib via NativeHandler trait
├── raya-stdlib/     # Native stdlib implementations (logger, etc.)
├── raya-cli/        # CLI tool
├── raya-pm/         # Package manager (rpkg)
├── raya-sdk/        # Native module FFI types
└── raya-native/     # Proc-macros for native modules

docs/                # Design documentation (VitePress site)
plans/               # Implementation roadmap
```

Each crate has its own `CLAUDE.md` with module-specific details.

---

## Build & Test

```bash
cargo build                    # Build all
cargo test                     # Run all tests (2,785+)
cargo test -p raya-engine      # Engine tests only (1,672)
cargo test -p raya-engine --features jit  # Engine + JIT tests (1,814)
cargo test -p raya-runtime     # Runtime + e2e tests (1,297)
cargo test -p raya-stdlib      # Stdlib tests (17)
cargo test -p rpkg             # Package manager tests
```

---

## Quick Reference

| Question | Answer |
|----------|--------|
| Use `typeof`? | ✅ For primitive unions only |
| Use `instanceof`? | ✅ For class type checking |
| Runtime type checks? | ❌ Never (compile-time only) |
| Generic erasure? | ❌ No, use monomorphization |
| Concurrency model? | Goroutine-style Tasks |
| Implementation language? | Rust (stable) |
