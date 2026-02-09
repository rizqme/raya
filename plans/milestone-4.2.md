# Milestone 4.2: std:logger Module

**Status:** Complete
**Depends on:** Milestone 3.7 (Module System)
**Goal:** Replace the global `console.log` API with a proper `std:logger` module, and migrate all references across documentation and code

**Infrastructure Complete:**
- ✅ `export default` support (parser, binder, typedef, visitor)
- ✅ `std:` module registry with source embedding
- ✅ `NativeHandler` trait for stdlib decoupling
- ✅ `raya-runtime` crate binds engine + stdlib
- ✅ Logger.raya source with native call IDs
- ✅ StdNativeHandler routes logger calls to raya_stdlib::logger
- ✅ E2E tests moved to raya-runtime for proper stdlib integration
- ✅ Tests: 827 engine lib, 594 runtime e2e, 4 stdlib, 2 runtime unit (all passing)

---

## Overview

Raya currently specifies a JavaScript-style global `console` object (`console.log`, `console.error`, `console.warn`, `console.info`) in `design/STDLIB.md` Section 6. This milestone removes that global and replaces it with a standard library module `std:logger` that must be imported explicitly.

### Motivation

- **Explicit imports** over implicit globals — consistent with Raya's module philosophy
- **Structured logging** — log levels, not just print functions
- **No magic globals** — `console` is not a class, not importable, just a special object; `std:logger` is a normal module

### Usage

```typescript
import logger from "std:logger";

logger.info("Server started on port 8080");
logger.warn("Deprecated API called");
logger.error("Connection failed");
logger.debug("Request payload:", data.toString());
```

---

## API Design

### Module: `std:logger`

```typescript
module "std:logger" {
  interface Logger {
    debug(...args: string[]): void;
    info(...args: string[]): void;
    warn(...args: string[]): void;
    error(...args: string[]): void;
  }

  // Default export: singleton logger instance
  const logger: Logger;
  export default logger;
}
```

### logger.info()

**Type Signature:**
```typescript
info(...args: string[]): void;
```

**Description:** Prints arguments to standard output.

**Behavior:**
- Arguments are converted to strings via `toString()`
- Separated by spaces
- Ends with newline

---

### logger.error()

**Type Signature:**
```typescript
error(...args: string[]): void;
```

**Description:** Prints arguments to standard error.

---

### logger.warn()

**Type Signature:**
```typescript
warn(...args: string[]): void;
```

**Description:** Prints warning to standard error.

---

### logger.debug()

**Type Signature:**
```typescript
debug(...args: string[]): void;
```

**Description:** Prints debug output to standard output. Can be disabled via runtime configuration.

---

## Phases

### Phase 1: Define std:logger Module ✅

**Status:** Complete

**Completed:**
- [x] Define native IDs for logger methods in `builtin.rs`
  - [x] `LOGGER_DEBUG` (0x1000)
  - [x] `LOGGER_INFO` (0x1001)
  - [x] `LOGGER_WARN` (0x1002)
  - [x] `LOGGER_ERROR` (0x1003)
  - [x] `is_logger_method()` helper
- [x] Add constants in `native_id.rs`
- [x] `export default` support
  - [x] AST: `ExportDecl::Default` variant
  - [x] Parser: `export default <expr>` parsing
  - [x] Binder: Register "default" symbol with type
  - [x] Typedef: Handle default exports in .d.raya
  - [x] Visitor: Handle default export traversal
- [x] `std:` module registry
  - [x] `StdModuleRegistry` with source embedding
  - [x] Module resolver checks `std:` prefix
  - [x] Test harness includes std sources
- [x] `NativeHandler` trait for stdlib decoupling
  - [x] Trait defined in `vm/native_handler.rs`
  - [x] `NoopNativeHandler` for tests
  - [x] `SharedVmState` holds `Arc<dyn NativeHandler>`
  - [x] `TaskInterpreter` receives handler reference
- [x] `raya-runtime` crate
  - [x] Depends on engine + stdlib
  - [x] `StdNativeHandler` implements `NativeHandler`
  - [x] Routes logger calls to `raya_stdlib::logger`
- [x] Logger.raya source with native calls
- [x] Rust implementations in `raya-stdlib/src/logger.rs`
  - [x] debug → `println!("[DEBUG] {}")`
  - [x] info → `println!("{}")`
  - [x] warn → `eprintln!("[WARN] {}")`
  - [x] error → `eprintln!("[ERROR] {}")`
- [x] VM dispatcher delegates to `NativeHandler`

**Files:**
- `crates/raya-engine/src/vm/builtin.rs` — `pub mod logger { ... }`
- `crates/raya-engine/src/compiler/native_id.rs` — `LOGGER_*` constants
- `crates/raya-engine/src/parser/ast/statement.rs` — `ExportDecl::Default`
- `crates/raya-engine/src/parser/parser/stmt.rs` — Export default parsing
- `crates/raya-engine/src/parser/checker/binder.rs` — Default export binding
- `crates/raya-engine/src/compiler/module/typedef.rs` — Default export typedef
- `crates/raya-engine/src/parser/ast/visitor.rs` — Default export visitor
- `crates/raya-engine/src/compiler/module/std_modules.rs` — NEW: Std registry
- `crates/raya-engine/src/compiler/module/resolver.rs` — `std:` detection
- `crates/raya-engine/src/vm/native_handler.rs` — NEW: NativeHandler trait
- `crates/raya-engine/src/vm/vm/shared_state.rs` — Handler integration
- `crates/raya-engine/src/vm/vm/task_interpreter.rs` — Handler dispatch
- `crates/raya-engine/tests/e2e/harness.rs` — Std sources
- `crates/raya-stdlib/Logger.raya` — NEW: Logger source
- `crates/raya-stdlib/src/logger.rs` — NEW: Rust implementations
- `crates/raya-runtime/` — NEW: Runtime crate
- `crates/raya-runtime/src/lib.rs` — StdNativeHandler

---

### Phase 2: Remove console Global ✅

**Status:** Complete

**Completed:**
- [x] Remove Section 6 "Console API" from `design/STDLIB.md`, replace with `std:logger` reference
- [x] Remove `console.rs` placeholder from `crates/raya-stdlib/src/`
- [x] Remove `pub mod console;` from `crates/raya-stdlib/src/lib.rs`
- [x] Update `crates/raya-stdlib/CLAUDE.md` — remove console references, add logger architecture
- [x] Update reflect bootstrap `CONSOLE_LOG` native ID
  - [x] Rename `CONSOLE_LOG` → `LOGGER_INFO` (0x1001) in `crates/raya-engine/src/vm/reflect/bootstrap.rs`
  - [x] Update `console_log_native_id` field → `logger_info_native_id`
  - [x] DYNAMIC_PRINT uses `println!` directly (no change needed)
- [x] Update `crates/raya-engine/src/vm/vm/CLAUDE.md` — document NativeHandler dispatch

**Files:**
- `design/STDLIB.md` — Section 6 replaced with std:logger reference
- `crates/raya-stdlib/src/console.rs` — Deleted (already removed)
- `crates/raya-stdlib/src/lib.rs` — Only contains `pub mod logger;`
- `crates/raya-stdlib/CLAUDE.md` — Fully rewritten for post-M4.2 architecture
- `crates/raya-engine/src/vm/reflect/bootstrap.rs` — CONSOLE_LOG → LOGGER_INFO
- `crates/raya-engine/src/vm/vm/CLAUDE.md` — Updated native call documentation

---

### Phase 3: Migrate Documentation ✅

**Status:** Complete

Replaced `console.log(...)` → `logger.info(...)` (and `console.error` → `logger.error`, etc.) across all design and plan docs.

**Completed:**
- [x] `design/LANG.md` — 87 references migrated
- [x] `design/MAPPING.md` — 16 references migrated (including bytecode examples)
- [x] `design/STDLIB.md` — Examples in match/sleep/JSON sections
- [x] `design/REFLECTION.md` — 18 references migrated
- [x] `design/JSON-TYPE.md` — JSON usage examples
- [x] `design/README.md` — Overview examples
- [x] `design/EXCEPTION_HANDLING.md` — Error handling examples
- [x] `design/CHANNELS.md` — Channel examples
- [x] `design/FORMATS.md` — Format examples
- [x] `design/NATIVE_BINDING_SIMPLE_EXAMPLE.md`
- [x] `design/NATIVE_BINDING_COMPLEX_EXAMPLE.md`
- [x] `design/DECORATORS.md`
- [x] `design/TYPEOF-DESIGN.md`
- [x] `design/TSX.md`
- [x] `design/ISSUES.md`
- [x] `design/CLI.md`
- [x] `design/MODULES.md`
- [x] `design/INNER_VM.md`
- [x] `plans/milestone-4.1.md` — Error handling examples
- [x] `plans/milestone-3.9.md` — Decorator examples
- [x] `plans/PLAN.md` — Roadmap examples
- [x] `plans/milestone-3.8.md`, `milestone-3.7.md`, `milestone-3.2.md`
- [x] `plans/milestone-2.8.md`, `milestone-2.7.md`, `milestone-2.5.md`, `milestone-2.3.md`
- [x] `crates/raya-stdlib/json.d.raya` — Type def comments
- [x] `crates/raya-stdlib/reflect.d.raya` — Type def comments
- [x] `crates/raya-stdlib/README.md`
- [x] `crates/raya-pm/src/commands/init.rs` — Generated example code
- [x] `crates/raya-engine/tests/statement_tests.rs` — Test assertions
- [x] `crates/raya-engine/src/vm/json/mod.rs` — Doc comment
- [x] `README.md` — Root readme examples
- [x] `CLAUDE.md` — Project instructions

**Verification:** Zero remaining `console.log/error/warn/info` references in codebase (grep confirmed)

---

### Phase 4: Tests ✅

**Status:** Complete - E2E tests moved to raya-runtime

**Completed:**
- [x] Moved all e2e tests from raya-engine to raya-runtime
- [x] E2E tests now run in raya-runtime with StdNativeHandler integration
- [x] 594 e2e tests passing in raya-runtime (all compilation tests)
- [x] Verified logger imports and compilation across test suite

**Architecture Change:**
Moved all integration tests from `crates/raya-engine/tests/e2e/` to `crates/raya-runtime/tests/e2e/` for proper stdlib integration testing.

**Test Distribution:**
- **raya-engine**: 827 lib tests (parser, compiler, VM unit tests)
- **raya-runtime**: 596 tests total (594 e2e + 2 unit tests)
- **raya-stdlib**: 4 unit tests
- **Total**: 1,427 tests passing

**Files:**
- `crates/raya-runtime/tests/e2e/` — All e2e tests (moved from raya-engine)
- `crates/raya-runtime/tests/e2e/harness.rs` — Updated paths for new location
- `crates/raya-runtime/tests/e2e_tests.rs` — Test module entry point

**Future Work:**
Logger runtime execution tests (output verification) will be added when VM native call support is integrated into the e2e test harness. Currently, tests verify successful compilation and module loading.

---

## Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Global vs import | Import (`std:logger`) | Explicit > implicit, consistent with module system |
| Export style | Default export (`import logger`) | Clean, single-purpose module |
| Casing | `logger` (lowercase) | Instance, not a class |
| Method names | `debug`, `info`, `warn`, `error` | Standard log levels |
| Output targets | info/debug → stdout, warn/error → stderr | Standard convention |
| Variadic args | `...args: string[]` | Simple, no `any` type needed |

---

## Migration Guide

### Before (removed)
```typescript
// Global — no import needed (old API)
logger.info("hello");
logger.error("failed");
```

### After
```typescript
import logger from "std:logger";

logger.info("hello");
logger.error("failed");
```

---

## Key Files Reference

| File | Purpose |
|------|---------|
| `crates/raya-engine/src/vm/builtin.rs` | Logger native ID module |
| `crates/raya-engine/src/compiler/native_id.rs` | Logger native ID constants |
| `crates/raya-engine/src/parser/checker/checker.rs` | Logger type resolution |
| `crates/raya-engine/src/compiler/lower/expr.rs` | logger → NATIVE_CALL lowering |
| `crates/raya-engine/src/vm/vm/task_interpreter.rs` | Logger VM handlers |
| `design/STDLIB.md` | API specification |
