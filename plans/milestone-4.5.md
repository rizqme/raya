# Milestone 4.5: std:runtime Module

**Status:** In Progress (Phases 1-8 Complete)
**Depends on:** Milestone 4.2 (stdlib pattern), Milestone 3.8 (Reflection API — bytecode builder, runtime builder, bootstrap)
**Goal:** Expose the Raya compilation pipeline and VM operations as `std:runtime` — with separate classes for parsing, type checking, compiling, bytecode I/O, and VM instances

---

## Overview

The `std:runtime` module provides programmatic access to the full Raya toolchain via **five separate classes**:

| Class | Purpose |
|-------|---------|
| `Compiler` | Compile source to bytecode, eval, execute |
| `Bytecode` | Binary I/O (encode/decode), library loading, dependency management |
| `Vm` | Spawn isolated VM instances with resource limits and permissions |
| `Parser` | Parse source to AST representation |
| `TypeChecker` | Type-check parsed AST |

### Architecture

Like `std:reflect`, runtime handlers **stay in raya-engine** because they need direct access to the parser, compiler, VM interpreter, heap, scheduler, and process management.

```
Runtime.raya (raya-stdlib)  →  __NATIVE_CALL(ID, args)
                                      ↓
                           NativeCall opcode (VM)
                                      ↓
                           is_runtime_method() check (builtin.rs)
                                      ↓
                           call_runtime_method() (handlers/runtime.rs)
                                      ↓
                           Parser / Compiler / Vm / Scheduler (engine internals)
```

### Usage

```typescript
import { Compiler, Bytecode, Vm, Parser, TypeChecker } from "std:runtime";

// ── Compile & Execute ──
let mod: number = Compiler.compile("function add(a: number, b: number): number { return a + b; }");
let sum: number = Compiler.executeFunction(mod, "add", 10, 20);
let result: number = Compiler.eval("return 1 + 2;");

// ── Bytecode binary I/O ──
let bytes: Buffer = Bytecode.encode(mod);
let loaded: number = Bytecode.decode(bytes);
let lib: number = Bytecode.loadLibrary("./mylib.ryb");

// ── Dependency loading ──
Bytecode.resolveDependency("utils");  // auto-search ./deps/, ./lib/, etc.
Bytecode.loadDependency("./vendor/utils.ryb", "utils");

// ── Parse & Type Check (advanced) ──
let ast: number = Parser.parse("let x: number = 42;");
let checked: number = TypeChecker.check(ast);

// ── Isolated VM instance ──
let child: VmInstance = Vm.spawn({
    maxHeap: 64 * 1024 * 1024,
    maxConcurrency: 4,
    maxThreads: 2,
    maxResource: 0.25,
    priority: 5,
    timeout: 5000,
    permissions: {
        allowStdlib: ["std:math", "std:logger"],
        allowReflect: false,
        allowVmSpawn: false,
    }
});

child.loadBytecode(bytes);
let task = child.runEntry("main");
let childResult: number = await task;
child.terminate();

// ── Current VM introspection ──
let current: VmInstance = Vm.current();
let heap: number = current.heapUsed();
let ver: string = current.version();
```

---

## Class APIs

### Compiler

Static methods for compiling and executing Raya source code.

```typescript
class Compiler {
    static compile(source: string): number;
    static compileExpression(expr: string): number;
    static compileAst(astId: number): number;
    static eval(source: string): number;
    static execute(moduleId: number): number;
    static executeFunction(moduleId: number, funcName: string, ...args: number[]): number;
}
```

| Method | Native ID | Description |
|--------|-----------|-------------|
| `compile` | 0x3000 | Parse + type-check + compile to bytecode module, returns module ID |
| `compileExpression` | 0x3001 | Wrap expression in `return <expr>;`, compile |
| `compileAst` | 0x3002 | Compile a pre-parsed AST to bytecode |
| `eval` | 0x3003 | Compile and immediately execute source |
| `execute` | 0x3004 | Execute a compiled module's main function |
| `executeFunction` | 0x3005 | Execute a named function with arguments |

### Bytecode

Static methods for bytecode binary I/O, library loading, and dependency management.

```typescript
class Bytecode {
    static encode(moduleId: number): Buffer;
    static decode(data: Buffer): number;
    static validate(moduleId: number): boolean;
    static disassemble(moduleId: number): string;
    static getModuleName(moduleId: number): string;
    static getModuleFunctions(moduleId: number): string[];
    static getModuleClasses(moduleId: number): string[];
    static loadLibrary(path: string): number;
    static loadDependency(path: string, name: string): number;
    static resolveDependency(name: string): number;
}
```

| Method | Native ID | Description |
|--------|-----------|-------------|
| `encode` | 0x3010 | Serialize module to `.ryb` binary |
| `decode` | 0x3011 | Deserialize `.ryb` binary to module |
| `validate` | 0x3012 | Verify module integrity (checksums) |
| `disassemble` | 0x3013 | Disassemble to human-readable listing |
| `getModuleName` | 0x3014 | Get module name |
| `getModuleFunctions` | 0x3015 | List function names |
| `getModuleClasses` | 0x3016 | List class names |
| `loadLibrary` | 0x3017 | Load `.ryb` file from path, returns module ID |
| `loadDependency` | 0x3018 | Load `.ryb` file and register as importable module under `name` |
| `resolveDependency` | 0x3019 | Auto-resolve `.ryb` from search paths, register as importable module |

### Vm

Static methods for VM instance management and introspection.

```typescript
class Vm {
    static current(): VmInstance;
    static spawn(config: VmConfig): VmInstance;
    static heapUsed(): number;
    static heapLimit(): number;
    static taskCount(): number;
    static concurrency(): number;
    static threadCount(): number;
    static gcCollect(): void;
    static gcStats(): number;
    static version(): string;
    static uptime(): number;
    static loadedModules(): string[];
    static hasModule(name: string): boolean;
    static hasPermission(name: string): boolean;
    static getPermissions(): VmPermissions;
    static getAllowedStdlib(): string[];
    static isStdlibAllowed(module: string): boolean;
}
```

| Method | Native ID | Description |
|--------|-----------|-------------|
| `current` | 0x3020 | Get the current VM instance (permission-gated) |
| `spawn` | 0x3021 | Spawn child VM with isolated context |
| `heapUsed` | 0x3040 | Current heap allocation in bytes |
| `heapLimit` | 0x3041 | Max heap size (0 = unlimited) |
| `taskCount` | 0x3042 | Total tasks (queued + running + suspended) |
| `concurrency` | 0x3043 | Tasks actively running right now |
| `threadCount` | 0x3044 | Max threads from shared pool |
| `gcCollect` | 0x3045 | Trigger manual GC |
| `gcStats` | 0x3046 | Total bytes freed by GC |
| `version` | 0x3047 | Raya VM version string |
| `uptime` | 0x3048 | VM uptime in milliseconds |
| `loadedModules` | 0x3049 | List loaded module names |
| `hasModule` | 0x304A | Check if module is loaded |
| `hasPermission` | 0x3030 | Check if current VM has a permission |
| `getPermissions` | 0x3031 | Get current VM's permission policy |
| `getAllowedStdlib` | 0x3034 | List stdlib modules allowed in current VM |
| `isStdlibAllowed` | 0x3035 | Check if a specific stdlib module is allowed |

### VmInstance

Returned by `Vm.spawn()` and `Vm.current()`. Represents an isolated VM execution context.

```typescript
interface VmInstance {
    // Identity
    id(): number;
    isRoot(): boolean;
    isAlive(): boolean;

    // Load & run bytecode (from INNER_VM.md)
    loadBytecode(bytes: Buffer): void;
    loadModule(name: string, sourceOrBytes: string | Buffer): void;
    runEntry(name: string, args?: number[]): Task<number>;
    spawn(funcName: string, args?: number[]): Task<number>;

    // Compile & execute within this instance
    compile(source: string): number;
    execute(moduleId: number): number;
    eval(source: string): number;
    executeFunction(moduleId: number, funcName: string, ...args: number[]): number;

    // Binary I/O within this instance
    encode(moduleId: number): Buffer;
    decode(data: Buffer): number;
    loadLibrary(path: string): number;
    loadDependency(path: string, name: string): number;
    resolveDependency(name: string): number;

    // Introspection
    heapUsed(): number;
    heapLimit(): number;
    taskCount(): number;
    concurrency(): number;
    threadCount(): number;
    uptime(): number;
    version(): string;

    // Resource control
    gcCollect(): void;
    gcStats(): number;

    // Module info
    loadedModules(): string[];
    hasModule(name: string): boolean;

    // Lifecycle
    terminate(): void;
    isDestroyed(): boolean;

    // Permissions
    getPermissions(): VmPermissions;
    hasPermission(name: string): boolean;
}
```

| Method | Native ID | Description |
|--------|-----------|-------------|
| `id` | 0x3022 | Get instance ID |
| `isRoot` | 0x3023 | Check if instance is the root VM |
| `isAlive` | 0x3024 | Check if instance is still alive |
| `loadBytecode` | 0x3025 | Load bytecode buffer into this VM |
| `loadModule` | 0x3026 | Load module from source or bytecode |
| `runEntry` | 0x3027 | Run a named entry point function |
| `spawn` (instance) | 0x3028 | Spawn a Task within this VM for a function |
| `terminate` | 0x3029 | Kill all Tasks and reclaim resources |
| `isDestroyed` | 0x302A | Check if instance has been terminated |

### Parser

Static methods for parsing Raya source code.

```typescript
class Parser {
    static parse(source: string): number;
    static parseExpression(expr: string): number;
}
```

| Method | Native ID | Description |
|--------|-----------|-------------|
| `parse` | 0x3050 | Parse source to AST, returns AST ID |
| `parseExpression` | 0x3051 | Parse a single expression to AST |

### TypeChecker

Static methods for type-checking parsed ASTs.

```typescript
class TypeChecker {
    static check(astId: number): number;
    static checkExpression(astId: number): number;
}
```

| Method | Native ID | Description |
|--------|-----------|-------------|
| `check` | 0x3060 | Type-check AST, returns typed AST ID |
| `checkExpression` | 0x3061 | Type-check an expression AST |

---

## Core Types

### VmConfig

Configuration for spawning isolated VM instances. **Every field is bounded by the parent VM's config.**

```typescript
interface VmConfig {
    maxHeap: number;
    maxConcurrency: number;
    maxThreads: number;
    maxResource: number;       // 0.0-1.0 CPU fraction
    priority: number;          // 1-10, default 5
    maxStack: number;
    timeout: number;           // ms, 0 = unlimited
    maxModules: number;
    permissions: VmPermissions;
}
```

### VmPermissions

```typescript
interface VmPermissions {
    allowStdlib: string[];     // e.g. ["std:math", "std:logger"], ["*"] = all
    allowReflect: boolean;
    allowVmAccess: boolean;
    allowVmSpawn: boolean;
    allowLibLoad: boolean;
    allowNativeCalls: boolean;
    allowEval: boolean;
    allowBinaryIO: boolean;
}
```

---

## Dependency Resolution

By default, all dependencies are **bundled into the `.ryb`** file at compile time. When bundling isn't possible, external `.ryb` files can be loaded at runtime.

**`Bytecode.resolveDependency(name)`** searches these folders in order:

1. `./deps/` — project-local dependency folder
2. `./lib/` — project-local library folder
3. `<entry_dir>/deps/` — relative to the entry point script's directory
4. `~/.raya/libs/` — user-global shared library folder

**`Bytecode.loadDependency(path, name)`** loads from an explicit path.

Both register the module so subsequent `import x from "name"` resolves to it. Both are permission-gated by `allowLibLoad`.

---

## Phases

### Phase 1: Compiler + Bytecode I/O

**Status:** Complete

Implements `Compiler` class (compile, eval, execute) and `Bytecode` class (encode, decode, loadLibrary, loadDependency, resolveDependency).

**Tasks:**
- [x] Add runtime native IDs (0x3000-0x3005, 0x3010-0x3019) to `builtin.rs`
- [x] Create `handlers/runtime.rs` with Compiler and Bytecode handlers
- [x] Register handler in `handlers/mod.rs`
- [x] Wire dispatch in `task_interpreter.rs`
- [x] Add `call_runtime_method` method on `TaskInterpreter` (bridging to handler)
- [x] Create `crates/raya-stdlib/raya/Runtime.raya` with Compiler + Bytecode classes
- [x] Register `Runtime.raya` in `std_modules.rs`
- [x] Create `crates/raya-stdlib/raya/runtime.d.raya` declaration file
- [x] E2E tests: import, compile, execute, eval, encode/decode roundtrip (15 tests)

**Implementation Notes:**
- Compilation uses the same pipeline as the CLI: `Parser::new(source) → parse() → Binder → TypeChecker → Compiler::compile_via_ir()`
- Compiled modules stored in a `CompiledModuleRegistry` (static, keyed by integer ID)
- `eval` wraps source in an anonymous function for top-level `return` handling
- `encode`/`decode` use existing `Module::encode()`/`Module::decode()` (.ryb format with CRC32 + SHA-256)

**Files:**
- `crates/raya-engine/src/vm/builtin.rs` — `pub mod runtime` + `is_runtime_method()`
- `crates/raya-engine/src/vm/vm/handlers/runtime.rs` — handler implementations
- `crates/raya-engine/src/vm/vm/handlers/mod.rs` — register module
- `crates/raya-engine/src/vm/vm/task_interpreter.rs` — dispatch + bridge method
- `crates/raya-stdlib/raya/Runtime.raya` — Raya source
- `crates/raya-stdlib/raya/runtime.d.raya` — type declarations
- `crates/raya-engine/src/compiler/module/std_modules.rs` — register "runtime" module

---

### Phase 2: Bytecode Inspection + Parser + TypeChecker

**Status:** Complete

Adds `Bytecode.validate()`, `Bytecode.disassemble()`, module info queries, plus `Parser` and `TypeChecker` classes.

**Tasks:**
- [x] Implement handlers 0x3012-0x3016 (bytecode inspection)
- [x] Implement handlers 0x3050-0x3051 (Parser)
- [x] Implement handlers 0x3060-0x3061 (TypeChecker)
- [x] Implement handler 0x3002 (Compiler.compileAst)
- [x] Add AST registry for storing parsed ASTs
- [x] Update `Runtime.raya` and `runtime.d.raya`
- [x] E2E tests: bytecode inspection, Parser, TypeChecker, compileAst pipeline (11 tests)

**Implementation Notes:**
- `AstRegistry` stores parsed ASTs (`AstEntry`: AST + Interner) and typed ASTs (`TypedAstEntry`: AST + Interner + TypeContext + SymbolTable + expr_types)
- `Parser.parse()` creates an AST, stores it in `AstRegistry`, returns ID
- `TypeChecker.check()` takes AST ID, clones from parsed registry, runs Binder + TypeChecker, stores `TypedAstEntry`, returns typed AST ID
- `Compiler.compileAst()` takes typed AST ID, removes `TypedAstEntry` from registry (consuming it), compiles to bytecode
- `compile_source()` refactored to reuse `parse_source()` and `typecheck_ast()` helpers

---

### Phase 3: VM Instances & Isolation

**Status:** Complete

Implements `Vm` class with `spawn()`, `current()` and `VmInstance` class for isolated child VMs.

**Tasks:**
- [x] Define `VmInstanceRegistry` with `VmInstanceEntry` (id, Option<Vm>, per-instance modules, parent/children, is_alive)
- [x] Implement `Vm.current()` (0x3020) — get-or-create root instance handle
- [x] Implement `Vm.spawn()` (0x3021) — create child Vm with 1 worker thread
- [x] Implement VmInstance identity methods: `id` (0x3022), `isRoot` (0x3023), `isAlive` (0x3024), `isDestroyed` (0x302A)
- [x] Implement VmInstance compile/execute/eval (0x3028, 0x302B, 0x302C) — root delegates to global registry, child uses take/put-back pattern
- [x] Implement `loadBytecode` (0x3025) and `runEntry` (0x3027)
- [x] Implement `terminate` (0x3029) with cascading child termination
- [x] Update `Runtime.raya` with VmInstance and VmClass classes
- [x] Update `runtime.d.raya` with type declarations
- [x] Fix compiler `variable_class_map` for type-annotated variables and `method_return_class_map` for chained method calls
- [x] E2E tests: 13 tests covering current/spawn/compile/execute/eval/terminate/isolation

**Implementation Notes:**
- Root VM is virtual (vm: None), delegates to global `COMPILED_MODULE_REGISTRY`
- Child VMs own a real `Vm` via `Option<Vm>` — taken during execution, put back after
- Per-instance `CompiledModuleRegistry` provides module isolation
- Child VMs can run pure Raya code but not stdlib modules (no NativeHandler passed through)
- Compiler enhanced: `variable_class_map` now populated from type annotations; `method_return_class_map` enables chained method call resolution (e.g., `Vm.current().isAlive()`)

---

### Phase 4: Permission Management

**Status:** Complete

**Tasks:**
- [x] Implement `Vm.hasPermission()` (0x3030)
- [x] Implement `Vm.getPermissions()` (0x3031)
- [x] Implement `Vm.getAllowedStdlib()` (0x3034)
- [x] Implement `Vm.isStdlibAllowed()` (0x3035)
- [x] Integrate permission checks into dispatch paths
- [x] Update `Runtime.raya` and `runtime.d.raya`
- [x] E2E tests: 11 tests covering hasPermission, getPermissions, getAllowedStdlib, isStdlibAllowed

**Implementation Notes:**
- `VmPermissions` struct with 8 permission fields (eval, binaryIO, vmSpawn, vmAccess, libLoad, reflect, nativeCalls, allowStdlib)
- Root VM: all permissions enabled, `allowStdlib: ["*"]`
- Child VMs: restrictive defaults (no vmSpawn, no libLoad, no reflect)
- `check_permission()` helper gates existing handlers: `eval` → Compiler.eval, `binaryIO` → encode/decode, `libLoad` → loadLibrary/loadDependency/resolveDependency, `vmSpawn` → Vm.spawn
- Permission query methods return strings (comma-separated) since runtime can't create objects yet

---

### Phase 5: VM Introspection & Resource Control

**Status:** Complete

**Tasks:**
- [x] Add native IDs 0x3040-0x304A to `builtin.rs`
- [x] Implement 11 introspection handlers in `handlers/runtime.rs`
- [x] Update `Runtime.raya` with VmClass introspection methods
- [x] Update `runtime.d.raya` with method declarations
- [x] E2E tests: 11 tests covering heapUsed, heapLimit, threadCount, taskCount, concurrency, gcCollect, gcStats, version, uptime, loadedModules, hasModule

**Implementation Notes:**
- `heapUsed()` queries GC's `heap_stats().allocated_bytes`
- `heapLimit()` returns 0 (unlimited — no configurable limit yet)
- `taskCount()` and `concurrency()` return 0 (placeholder — scheduler not accessible from handler context)
- `threadCount()` uses `std::thread::available_parallelism()`
- `gcCollect()` triggers `gc.collect()`, `gcStats()` returns `gc.stats().bytes_freed`
- `version()` returns "0.1.0", `uptime()` tracks from a static `LazyLock<Instant>`
- `loadedModules()` and `hasModule()` query the `COMPILED_MODULE_REGISTRY` names index

---

### Phase 6: Bytecode Builder

**Status:** Complete

Wraps existing reflect handlers (0x0DF0-0x0DFD) in a `BytecodeBuilder` class.

**Implementation:**
- [x] Add `BytecodeBuilder` class to `Runtime.raya` wrapping reflect IDs 0x0DF0-0x0DFD
- [x] Update `runtime.d.raya` with BytecodeBuilder declarations
- [x] Add bytecode builder dispatch to `task_interpreter.rs` (14 match arms for reflect IDs)
- [x] Made `BYTECODE_BUILDER_REGISTRY` pub(crate) in handlers/reflect.rs
- [x] 5 E2E tests: create, emit+build, validate, declareLocal, defineLabel

**Note:** Required Rust changes in `task_interpreter.rs` because its inline `call_reflect_method` didn't dispatch bytecode builder reflect IDs (0x0DF0-0x0DFD).

---

### Phase 7: Dynamic Modules & Runtime Types

**Status:** Complete

Wraps existing reflect handlers for dynamic class creation (0x0DE0-0x0DE6) and dynamic module creation (0x0E10-0x0E15).

**Implementation:**
- [x] Add `ClassBuilder` class to `Runtime.raya` (7 methods wrapping reflect IDs 0x0DE0-0x0DE6)
- [x] Add `DynamicModule` class to `Runtime.raya` (6 methods wrapping reflect IDs 0x0E10-0x0E15)
- [x] Update `runtime.d.raya` with ClassBuilder and DynamicModule declarations
- [x] Add 13 dispatch match arms to `task_interpreter.rs` (7 ClassBuilder + 6 DynamicModule)
- [x] Made `CLASS_BUILDER_REGISTRY` and `DYNAMIC_MODULE_REGISTRY` pub(crate) in handlers/reflect.rs
- [x] Fixed function ID cast for MODULE_ADD_FUNCTION (i32 → u32 → usize to preserve bit pattern)
- [x] 10 E2E tests: 5 ClassBuilder + 5 DynamicModule

**Note:** ClassBuilder.build() (0x0DE6) needs `self.classes` and `self.class_metadata` from TaskInterpreter for DynamicClassBuilder integration. Permissions handlers (0x0E00-0x0E0F) deferred to Reflect API scope.

---

### Phase 8: E2E Tests

**Status:** Complete

**Implementation Notes:**
- 96 total runtime E2E tests (20 new in Phase 8 + 76 existing from Phases 1-7)
- Phase 1 gap tests: executeFunction, eval complex expression, encode-decode-execute roundtrip
- Phase 2 gap tests: getModuleName, checkExpression, validate after decode
- Phase 3 gap tests: loadBytecode+execute on child, runEntry on child, fault containment, multiple evals, unique spawn IDs, root VM alive
- Cross-phase integration: compile in children independently, BytecodeBuilder+DynamicModule, ClassBuilder+DynamicModule, full pipeline parse→check→compileAst→execute, encode→decode→validate+disassemble, BytecodeBuilder local variables, BytecodeBuilder labels+jumps

**Tasks:**
- [x] Update test harness to include `Runtime.raya` in `get_std_sources()`
- [x] Phase 1 tests: import, compile, execute, eval, encode/decode roundtrip
- [x] Phase 2 tests: validate, disassemble, module info, Parser.parse, TypeChecker.check
- [x] Phase 3 tests: Vm.spawn, loadBytecode, runEntry, terminate, isolation, fault containment
- [x] Phase 5 tests: heapUsed, version, taskCount, loadedModules, permissions, gc
- [x] Phase 6-7 tests: bytecode builder, class builder, dynamic modules
- [x] Cross-phase integration tests

---

### Phase 9: Documentation

**Status:** Not Started

**Tasks:**
- [ ] Update `design/STDLIB.md` with `std:runtime` module documentation
- [ ] Update `CLAUDE.md` with milestone 4.5 status
- [ ] Update `plans/PLAN.md` with 4.5 status

---

## Native ID Ranges

| Range | Category | Class | Phase |
|-------|----------|-------|-------|
| 0x3000-0x3005 | Compile & execute | `Compiler` | 1 |
| 0x3010-0x3019 | Binary I/O & dependencies | `Bytecode` | 1-2 |
| 0x3020-0x302A | VM instances | `Vm` / `VmInstance` | 3 |
| 0x3030-0x3035 | Permissions | `Vm` | 4 |
| 0x3040-0x304A | Introspection & resources | `Vm` | 5 |
| 0x3050-0x3051 | Parsing | `Parser` | 2 |
| 0x3060-0x3061 | Type checking | `TypeChecker` | 2 |
| 0x0DF0-0x0DFE | Bytecode builder (existing) | wrapper | 6 |
| 0x0DE0-0x0E17 | Dynamic modules/types (existing) | wrapper | 7 |

---

## Isolation Model

Child VMs run **in-process** using a separate `VmContext`. Each child is isolated by:

- **Heap** — independent `VmContext` with own allocations and GC
- **Globals** — separate global namespace
- **Scheduler** — shared with parent (child tasks run on the existing thread pool)
- **Permissions** — independent permission policy (subset of parent's)
- **Resource counters** — independent heap/concurrency/CPU tracking

```
┌─────────────── OS Process ─────────────────────────────┐
│                                                         │
│  Shared Scheduler (N worker threads = cpu cores)        │
│                                                         │
│  Root VM (all permissions)                              │
│  heap: unlimited    tasks: unlimited                    │
│                                                         │
│  ┌──── Child A ───────┐  ┌──── Child B ────────┐       │
│  │ heap: 64MB         │  │ heap: 32MB          │       │
│  │ concurrency: 4     │  │ concurrency: 2      │       │
│  │ threads: ≤4        │  │ threads: ≤2         │       │
│  │ resource: 25%      │  │ resource: 10%       │       │
│  │ priority: 5        │  │ priority: 3         │       │
│  │ timeout: 5000ms    │  │ timeout: 1000ms     │       │
│  │ stdlib: [math]     │  │ stdlib: [math, log] │       │
│  │                    │  │                     │       │
│  │ ┌─ Grandchild ──┐ │  │                     │       │
│  │ │ heap: 16MB     │ │  │                     │       │
│  │ │ concurrency: 2 │ │  │                     │       │
│  │ │ resource: 10%  │ │  │                     │       │
│  │ └────────────────┘ │  │                     │       │
│  └────────────────────┘  └─────────────────────┘       │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### Nesting Rules

A child VM with `allowVmSpawn: true` can create sub-VMs. All config fields are **strictly bounded** by the parent. The `spawn` handler validates **every field** against the parent's config.

### Fault Isolation

| Failure | Containment | Parent Impact |
|---------|-------------|---------------|
| Unhandled exception | Caught at instance boundary | Parent sees error return |
| Stack overflow | Child's `maxStack` exceeded | Parent unaffected |
| Heap exhaustion | Child's `maxHeap` hit | Parent heap independent |
| Timeout | Wall-clock limit, child tasks killed | Parent continues |
| Infinite loop | Preempted by throttling + timeout | Parent tasks keep running |
| Panic / fatal | Child marked dead, resources reclaimed | Parent gets RuntimeError |

### Value Passing

Cross-instance values are **copied** at the boundary:
- Primitives — copied by value
- Objects — serialized/deserialized
- Functions — **not transferable**
- Buffers — byte data copied

---

## Compilation Pipeline (Internal)

```
source: string
    ↓ Parser::new(source).parse()
ast: Module + Interner
    ↓ Binder::bind_module() + TypeChecker::check()
typed AST + TypeContext
    ↓ Compiler::compile_via_ir()
    ├── Lowerer::lower_module()        (AST → IR)
    ├── monomorphize()                 (generics specialization)
    ├── Optimizer::optimize()          (constant folding, DCE)
    └── codegen::generate()            (IR → bytecode)
bytecode: Module
    ↓ store in CompiledModuleRegistry
module ID: number
```

---

## Key Differences from Other Stdlib Modules

| Aspect | std:logger / std:math | std:reflect | std:runtime |
|--------|----------------------|-------------|-------------|
| Handler location | raya-stdlib | raya-engine | raya-engine |
| NativeHandler trait | Yes | No | No |
| Native ID ranges | 0x1000, 0x2000 | 0x0D00-0x0E2F | 0x3000-0x3061 + existing |
| VM state needed | No | Yes | Yes (full) |
| Export style | `export default` singleton | `export default` singleton | Named exports (5 classes) |
| In-process isolation | N/A | N/A | Yes (VmInstance) |

---

## Key Files Reference

| File | Purpose |
|------|---------|
| `crates/raya-stdlib/raya/Runtime.raya` | Raya source: Compiler, Bytecode, Vm, Parser, TypeChecker classes |
| `crates/raya-stdlib/raya/runtime.d.raya` | Type declarations |
| `crates/raya-engine/src/vm/vm/handlers/runtime.rs` | Native handlers |
| `crates/raya-engine/src/vm/vm/handlers/mod.rs` | Register runtime handler module |
| `crates/raya-engine/src/vm/builtin.rs` | `is_runtime_method()`, native IDs |
| `crates/raya-engine/src/compiler/module/std_modules.rs` | Register `Runtime.raya` |
| `crates/raya-runtime/tests/e2e/runtime.rs` | E2E tests |
