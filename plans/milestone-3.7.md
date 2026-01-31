# Milestone 3.7: JSON Intrinsics & Module System

**Status:** In Progress (Phases 2-3 Complete)
**Goal:** Complete JSON intrinsics with type-safe encode/decode and implement the module system

---

## Current State Assessment

### Already Implemented (verified in codebase)

**Type Checker (~11k lines):**
- [x] Symbol table and name resolution (`checker/symbols.rs`, `binder.rs`)
- [x] Type inference for expressions (`checker/checker.rs` - 2508 lines)
- [x] Control flow-based type narrowing (`checker/narrowing.rs` - 688 lines)
- [x] Type guards (`checker/type_guards.rs` - 526 lines)
- [x] Exhaustiveness checking for discriminated unions (`checker/exhaustiveness.rs`)
- [x] Closure capture analysis (`checker/captures.rs`)
- [x] Discriminant inference with priority order (`types/discriminant.rs` - 773 lines)
- [x] Bare union transformation (`types/bare_union.rs` - 319 lines)
- [x] Assignability and subtyping (`types/assignability.rs`, `subtyping.rs`)
- [x] Builtin type signatures (`checker/builtins.rs`)
- [x] Error reporting with diagnostics (`checker/diagnostic.rs`)

**Code Generator (~18k lines):**
- [x] IR structure with SSA/three-address code (`ir/*.rs`)
- [x] AST lowering to IR (`lower/*.rs` - 4400+ lines)
- [x] Monomorphization/generic specialization (`monomorphize/*.rs`)
- [x] Optimization passes:
  - [x] Constant folding (`optimize/constant_fold.rs`)
  - [x] Dead code elimination (`optimize/dce.rs`)
  - [x] Function inlining (`optimize/inline.rs`)
  - [x] PHI elimination (`optimize/phi_elim.rs`)
- [x] Bytecode generation from IR (`codegen/*.rs`)
- [x] Module builder with verification

**Module System (partial):**
- [x] Module linker for symbol resolution (`module/linker.rs`)
- [x] Import specifier parsing (`module/import.rs`)
- [ ] Package resolution (returns TODO error)
- [ ] URL imports (returns TODO error)
- [ ] Standard library module loading

---

## Remaining Work

### Phase 1: JSON Built-in Object

`JSON` is a global built-in object (like JavaScript's `JSON`), with compile-time code generation for type-safe encode/decode.

**API:**
```typescript
// Global built-in - no import needed
JSON.stringify(value)          // any -> string (runtime, like JS)
JSON.parse(jsonString)         // string -> any (runtime, like JS)
JSON.encode<T>(value: T)       // T -> Result<string, Error> (compile-time codegen)
JSON.decode<T>(json: string)   // string -> Result<T, Error> (compile-time codegen)
```

**Go-style Field Mapping with Annotations:**

| Feature | Go | Raya |
|---------|-----|------|
| Rename field | `` `json:"name"` `` | `//@@json name` |
| Omit empty | `` `json:"name,omitempty"` `` | `//@@json name omitEmpty` |
| Skip field | `` `json:"-"` `` | `//@@json -` |

**Example:**
```typescript
class User {
  //@@json user_name
  name: string;

  //@@json age omitEmpty
  age: number;

  //@@json -
  internalId: number;

  email: string | null;  // uses field name "email" by default
}

const user = new User();
user.name = "Alice";
user.age = 0;
user.email = null;

// Type-safe encode (compiler generates specialized function)
const result = JSON.encode(user);
if (result.status === "ok") {
  console.log(result.value);
  // {"user_name":"Alice","email":null}
  // age omitted (omitEmpty + zero)
  // internalId omitted ("-")
}

// Type-safe decode
const decoded = JSON.decode<User>('{"user_name":"Bob","email":"bob@test.com"}');
if (decoded.status === "ok") {
  console.log(decoded.value.name);  // "Bob"
}
```

**Tasks:**
- [ ] Add `JSON` as built-in global object in compiler
- [ ] Parse `//@@json` annotations on fields
- [ ] Store JSON field metadata in class type info
- [ ] Detect `JSON.encode<T>()` calls and generate specialized encoder
- [ ] Detect `JSON.decode<T>()` calls and generate specialized decoder
- [ ] Handle `omitEmpty` for primitives, nullables, arrays
- [ ] Handle `-` to skip fields
- [ ] Implement runtime `JSON.stringify`/`JSON.parse` (delegate to VM)
- [ ] Add tests for JSON codegen with field mapping

**Files to Create/Modify:**
```
crates/raya-engine/src/compiler/intrinsic/
├── mod.rs           # Intrinsic detection
├── json.rs          # JSON encode/decode codegen
```

**Reference:** `design/LANG.md` Section 17.7

---

### Phase 2: Core Module Resolution ✅

Implement the foundation for resolving imports.

**Reference:** [design/MODULES.md](../design/MODULES.md)

**Tasks:**
- [x] Implement local import resolution (`./path`, `../path`)
  - Resolve relative paths from current file
  - Auto-add `.raya` extension if missing
  - Compile imported modules on-the-fly
- [x] Implement module graph construction
  - Track dependencies between modules
  - Detect circular imports
  - Topological sort for compilation order
- [x] Add module cache (in-memory)
  - Cache compiled modules by path
  - Invalidate on source change (dev mode)

**Files Created:**
- `crates/raya-engine/src/compiler/module/mod.rs` - Module entry point
- `crates/raya-engine/src/compiler/module/resolver.rs` - Path resolution
- `crates/raya-engine/src/compiler/module/graph.rs` - Dependency graph
- `crates/raya-engine/src/compiler/module/cache.rs` - Module cache
- `crates/raya-engine/src/compiler/module/compiler.rs` - ModuleCompiler

**Tests:** 26 tests passing

**Import Resolution Algorithm:**
```
import { foo } from "./utils"
    ↓
1. Resolve path relative to current file
2. Try: ./utils.raya, ./utils/index.raya
3. Parse and type-check
4. Compile to bytecode
5. Link symbols
```

**Files to Modify:**
```
crates/raya-engine/src/
├── compiler/module/
│   ├── resolver.rs      # Path resolution logic
│   ├── graph.rs         # Dependency graph
│   └── cache.rs         # Module cache
├── vm/module/
│   └── import.rs        # Update import handling
```

---

### Phase 3: Multi-Module Compilation ✅

Enable compiling multiple files together.

**Tasks:**
- [x] Implement multi-file compilation pipeline
  - Accept multiple entry points
  - Build complete dependency graph
  - Compile in dependency order
- [x] Cross-module symbol resolution
  - Export/import matching
  - Type checking across modules (variables)
  - Error on missing exports
- [ ] Generate linked bytecode module (Phase 4+)
  - Merge constant pools
  - Resolve cross-module references
  - Single `.ryb` output (or multiple)

**Files Created/Modified:**
- `crates/raya-engine/src/compiler/module/exports.rs` - Export tracking
- `crates/raya-engine/src/parser/checker/binder.rs` - Export handling
- `crates/raya-engine/src/parser/checker/symbols.rs` - Export/import support

**Limitations (Known):**
- Function/class imports compile but calling them fails due to TypeContext isolation
- Types are not migrated between modules (TypeIDs are module-local)
- Bytecode linking not yet implemented

**Tests:** 31 tests passing

**Example:**
```typescript
// main.raya
import { Logger } from "./logger";
let log = new Logger("app");
log.info("Started");

// logger.raya
export class Logger {
    constructor(private name: string) {}
    info(msg: string): void { ... }
}
```

**Compilation:**
```bash
raya build main.raya
# Automatically finds and compiles logger.raya
# Produces: dist/main.ryb
```

**Files to Modify:**
```
crates/raya-engine/src/compiler/
├── pipeline.rs          # Multi-module pipeline
├── module/linker.rs     # Cross-module linking
```

---

### Phase 4: Package Imports

Implement named package imports from registry/cache.

**Tasks:**
- [ ] Parse package specifiers
  - `"logging"` - latest version
  - `"logging@1.2.0"` - exact version
  - `"logging@^1.0.0"` - semver range
- [ ] Implement `raya.toml` parser
  - Read `[dependencies]` section
  - Support version constraints
- [ ] Implement `raya.lock` reader
  - Read locked versions
  - Verify checksums
- [ ] Implement global cache lookup
  - Check `~/.raya/cache/<hash>/`
  - Load pre-compiled `.ryb` files
- [ ] Load `.rdef` type definitions
  - Parse type definition files
  - Use for type checking without source

**Package Resolution:**
```
import { Logger } from "logging"
    ↓
1. Check raya.lock for exact version
2. Look up in ~/.raya/cache/<hash>/
3. Load module.ryb (bytecode)
4. Load module.rdef (types, if needed)
5. Link into current module
```

**Files to Create:**
```
crates/raya-pm/src/
├── toml.rs              # raya.toml parser
├── lock.rs              # raya.lock parser
├── cache/
│   ├── mod.rs           # Cache operations
│   └── lookup.rs        # Package lookup
```

---

### Phase 5: Package Installation

Implement package download and installation.

**Tasks:**
- [ ] Implement registry client
  - HTTP client for raya.dev API
  - Query package metadata
  - Download `.ryb` archives
- [ ] Implement `raya install` command
  - Parse raya.toml dependencies
  - Resolve dependency tree
  - Download missing packages
  - Update raya.lock
- [ ] Implement `raya add <package>` command
  - Add to raya.toml
  - Install immediately
- [ ] Implement version resolution
  - Semver constraint solving
  - Conflict detection
  - Generate lockfile

**Registry API:**
```
GET /packages/{name}           → Package metadata
GET /packages/{name}/{version} → Version info + download URL
GET /packages/{name}/{version}/download → .ryb archive
```

**Files to Create:**
```
crates/raya-pm/src/
├── registry/
│   ├── client.rs        # HTTP client
│   └── api.rs           # Registry API types
├── resolver/
│   ├── mod.rs           # Version resolution
│   └── semver.rs        # Semver parsing
├── commands/
│   ├── install.rs       # raya install
│   └── add.rs           # raya add
```

---

### Phase 6: URL Imports

Implement direct URL imports.

**Tasks:**
- [ ] Parse URL import specifiers
  - `https://github.com/user/repo/v1.0.0`
  - `https://pkg.raya.dev/lib@1.0.0`
- [ ] Fetch and cache URL imports
  - Download on first use
  - Cache by content hash
  - Verify checksums
- [ ] Update lockfile for URL imports
  - Store URL → hash mapping
  - Reproducible builds

**URL Import Flow:**
```
import { utils } from "https://github.com/user/repo/v1.0.0"
    ↓
1. Check raya.lock for cached hash
2. If not cached:
   a. Fetch from URL
   b. Compute SHA-256
   c. Store in ~/.raya/cache/<hash>/
   d. Update raya.lock
3. Load from cache
```

**Files to Modify:**
```
crates/raya-engine/src/compiler/module/
├── resolver.rs          # Add URL resolution
crates/raya-pm/src/
├── url/
│   ├── fetch.rs         # URL fetching
│   └── cache.rs         # URL → cache mapping
```

---

## Test Coverage Required

### JSON Codegen Tests
```rust
#[test]
fn test_json_encode_basic() {
    // class User { name: string; age: number; }
    // JSON.encode(user) -> {"name":"Alice","age":30}
}

#[test]
fn test_json_field_rename() {
    // //@@json user_name
    // name: string;
    // JSON.encode(user) -> {"user_name":"Alice"}
}

#[test]
fn test_json_omit_empty() {
    // //@@json age omitEmpty
    // age: number;
    // age = 0 -> field omitted from output
}

#[test]
fn test_json_skip_field() {
    // //@@json -
    // internal: string;
    // field not included in JSON output
}

#[test]
fn test_json_decode_with_mapping() {
    // {"user_name":"Bob"} -> user.name = "Bob"
}
```

### Module System Tests
```rust
#[test]
fn test_local_import_resolution() {
    // import { foo } from "./utils"
    // Resolves ./utils.raya relative to current file
}

#[test]
fn test_circular_import_detection() {
    // a.raya imports b.raya, b.raya imports a.raya
    // Should detect and report error
}

#[test]
fn test_multi_module_compilation() {
    // main.raya imports logger.raya
    // Both compiled and linked correctly
}

#[test]
fn test_package_import_from_cache() {
    // import { Logger } from "logging"
    // Loads from ~/.raya/cache/
}

#[test]
fn test_url_import() {
    // import { utils } from "https://..."
    // Fetches, caches, and loads
}

#[test]
fn test_export_import_matching() {
    // Verify exported symbols match imports
    // Error on missing exports
}
```

---

## Success Criteria

1. **JSON Intrinsics:** `JSON.encode/decode` generate specialized code per type
2. **Local Imports:** `./path` imports work with auto-compilation
3. **Multi-Module:** Multiple files compile and link correctly
4. **Package Imports:** Named packages load from global cache
5. **URL Imports:** Direct URL imports work with caching
6. **All existing tests pass:** 573+ unit tests, 516 e2e tests

---

## Dependencies

- Milestone 3.6 (Cooperative Task Scheduler) - Complete
- Milestone 3.5 (Built-in Types) - Complete

---

## Estimated Effort

| Phase | Description | Effort |
|-------|-------------|--------|
| 1 | JSON Codegen | Medium |
| 2 | Core Module Resolution | Medium |
| 3 | Multi-Module Compilation | Medium |
| 4 | Package Imports | Medium |
| 5 | Package Installation | High |
| 6 | URL Imports | Low |

---

**Last Updated:** 2026-01-31
