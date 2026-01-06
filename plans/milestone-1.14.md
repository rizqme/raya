# Milestone 1.14: Module System (VM-Side)

**Status:** 📋 Planned
**Priority:** High
**Estimated Effort:** 4-6 weeks
**Dependencies:** Milestones 1.2 (Bytecode), 1.3 (Value System)

---

## Overview

This milestone implements the **VM-side module system** for Raya, focusing on loading, linking, and caching compiled `.rbin` modules. Inspired by Bun and Go, the system uses a global cache with content-addressable storage to eliminate the node_modules bloat.

**Scope:** VM runtime only (module loading, linking, caching). Compilation (parsing `.raya`, code generation) is handled separately in compiler milestones.

### Key Features

- **Global cache:** Single `~/.raya/cache/` for all projects (no node_modules!)
- **Bytecode-first:** Store compiled `.rbin` files, not source
- **Content-addressable:** Packages identified by SHA-256 hash
- **Zero duplication:** Same package version shared across projects
- **Semver resolution:** Caret (`^`), tilde (`~`), exact, and range constraints
- **Local path dependencies:** Support for monorepos and private packages
- **Conflict resolution:** Allow multiple major versions, resolve minor/patch automatically

### Architecture

```
~/.raya/cache/          # Global package cache
    ├── <hash>/         # Content-addressable storage
    │   ├── module.rbin # Compiled bytecode
    │   ├── module.rdef # Type definitions (optional)
    │   └── metadata.json

my-project/
    ├── raya.toml       # Package descriptor
    ├── raya.lock       # Lockfile (exact versions)
    └── src/
```

### Import Syntax

```typescript
// Named package (from registry)
import { Logger } from "logging@1.2.3";

// URL import (decentralized)
import { utils } from "https://github.com/user/repo/v1.0.0";

// Local import
import { helper } from "./utils.raya";
```

---

## Phase 1: Module Loading & Bytecode Format

**Goal:** Load and execute .rbin modules in the VM.

**Crate:** `raya-bytecode`, `raya-core`

### Tasks

#### Module Bytecode Format (`raya-bytecode/src/module.rs`)

- [ ] Define module header structure
  ```rust
  pub struct ModuleHeader {
      magic: [u8; 4],           // "RAYA"
      version: u32,              // Bytecode format version
      checksum: [u8; 32],        // SHA-256 of module content
      name: String,              // Module name
      exports: Vec<Export>,      // Public symbols
      imports: Vec<Import>,      // Dependencies
  }

  pub struct Export {
      name: String,
      symbol_type: SymbolType,  // Function, Class, Constant
      offset: usize,            // Bytecode offset
  }

  pub struct Import {
      module: String,           // Module specifier
      symbol: String,           // Imported symbol name
      alias: Option<String>,    // Optional alias
  }
  ```

- [ ] Module export table (functions, classes, constants)
- [ ] Module import table (dependencies with version constraints)
- [ ] Module constant pool (for module-level data)
- [ ] Module serialization format (binary)
- [ ] Module deserialization with validation

#### Module Loader (`raya-core/src/module/loader.rs`)

- [ ] Load .rbin file from disk
  ```rust
  pub fn load_from_file(path: &Path) -> Result<Module, ModuleError>
  ```

- [ ] Verify module checksum (SHA-256)
  ```rust
  fn verify_checksum(data: &[u8], expected: &[u8; 32]) -> Result<(), ModuleError>
  ```

- [ ] Parse module header
- [ ] Load constant pool
- [ ] Validate bytecode integrity
- [ ] Create `Module` struct in memory
  ```rust
  pub struct Module {
      name: String,
      checksum: [u8; 32],
      exports: HashMap<String, Value>,
      imports: Vec<ModuleRef>,
      bytecode: Vec<Opcode>,
      constants: Vec<Value>,
  }
  ```

#### Module Registry (`raya-core/src/module/registry.rs`)

- [ ] Track loaded modules by name/checksum
  ```rust
  pub struct ModuleRegistry {
      by_name: HashMap<String, Arc<Module>>,
      by_checksum: HashMap<[u8; 32], Arc<Module>>,
  }
  ```

- [ ] Prevent duplicate module loading
  ```rust
  pub fn get_or_load(&mut self, spec: &str) -> Result<Arc<Module>, ModuleError>
  ```

- [ ] Module lookup by import path
- [ ] Module unloading support (for hot reload)

---

## Phase 2: Import Resolution

**Goal:** Resolve import paths to actual modules at runtime.

**Crate:** `raya-core`

### Tasks

#### Import Resolver (`raya-core/src/module/import.rs`)

- [ ] Parse import specifiers
  - [ ] Local: `"./utils.raya"` → filesystem path
  - [ ] Package: `"logging@1.2.3"` → cache lookup
  - [ ] URL: `"https://..."` → network fetch

- [ ] Resolve local file imports
  ```rust
  fn resolve_local(path: &str, current: &Path) -> Result<PathBuf, ImportError>
  ```

- [ ] Resolve package imports (check cache first)
  ```rust
  fn resolve_package(spec: &str, cache: &Cache) -> Result<PathBuf, ImportError>
  ```

- [ ] Resolve URL imports (fetch if not cached)
  ```rust
  fn resolve_url(url: &str, cache: &Cache) -> Result<PathBuf, ImportError>
  ```

- [ ] Import path normalization (cross-platform)

#### Dependency Graph (`raya-core/src/module/deps.rs`)

- [ ] Build dependency graph during module loading
  ```rust
  pub struct DependencyGraph {
      nodes: HashMap<String, Module>,
      edges: HashMap<String, Vec<String>>,
  }
  ```

- [ ] Topological sort for load order
  ```rust
  pub fn topological_sort(&self) -> Result<Vec<String>, GraphError>
  ```

- [ ] Circular dependency detection
  ```rust
  pub fn detect_cycle(&self) -> Option<Vec<String>>
  ```

- [ ] Error reporting for circular deps (show cycle path)

---

## Phase 3: Global Cache Management

**Goal:** Manage global package cache at `~/.raya/cache/`.

**Crate:** `raya-pm`

### Tasks

#### Cache Directory Structure (`raya-pm/src/cache/mod.rs`)

- [ ] Initialize cache directory on first run
  ```
  ~/.raya/
  ├── cache/
  │   ├── <sha256-hash>/
  │   │   ├── module.rbin
  │   │   ├── module.rdef (optional)
  │   │   └── metadata.json
  │   └── <sha256-hash>/
  ├── registry/
  │   └── index.json
  └── config.toml
  ```

- [ ] Cache directory permissions (platform-specific, secure)
- [ ] Cache version migration support
- [ ] Configuration file parsing (`config.toml`)

#### Content-Addressable Storage (`raya-pm/src/cache/storage.rs`)

- [ ] Store modules by SHA-256 hash
  ```rust
  pub fn store(module: &[u8]) -> Result<[u8; 32], CacheError> {
      let hash = sha256(module);
      let path = cache_dir().join(hex::encode(hash));
      fs::create_dir_all(&path)?;
      fs::write(path.join("module.rbin"), module)?;
      Ok(hash)
  }
  ```

- [ ] Retrieve module by hash
  ```rust
  pub fn retrieve(hash: &[u8; 32]) -> Result<Vec<u8>, CacheError>
  ```

- [ ] Atomic write operations (temp file + rename)
- [ ] Handle hash collisions (should never happen, but safety check)

#### Cache Lookup (`raya-pm/src/cache/lookup.rs`)

- [ ] Check if module exists in cache
  ```rust
  pub fn exists(hash: &[u8; 32]) -> bool
  ```

- [ ] Verify cached module integrity (re-hash)
- [ ] Return path to cached .rbin file
- [ ] Cache hit/miss metrics (for debugging/optimization)

#### Cache Eviction (`raya-pm/src/cache/eviction.rs`)

- [ ] Track cache size and last access time
- [ ] LRU eviction when cache exceeds limit
  ```rust
  pub fn evict_lru(target_size: u64) -> Result<usize, CacheError>
  ```

- [ ] Configurable cache size limit (default: 10 GB)
- [ ] Never evict currently loaded modules
- [ ] Manual cache clean command support

---

## Phase 4: Module Linking

**Goal:** Link imported modules to resolve symbols at runtime.

**Crate:** `raya-core`

### Tasks

#### Symbol Resolution (`raya-core/src/module/linker.rs`)

- [ ] Resolve imported symbols from exports
  ```rust
  pub fn link_import(import: &Import, module: &Module) -> Result<Value, LinkError> {
      module.exports.get(&import.symbol)
          .ok_or(LinkError::SymbolNotFound(import.symbol.clone()))
          .cloned()
  }
  ```

- [ ] Handle re-exports (`export { foo } from "./bar"`)
- [ ] Lazy symbol resolution (defer until first use)
- [ ] Symbol versioning (for multiple major versions)

#### Module Initialization (`raya-core/src/module/init.rs`)

- [ ] Execute module-level code (top-level statements)
  ```rust
  pub fn initialize_module(module: &Module, vm: &mut Vm) -> Result<(), InitError>
  ```

- [ ] Initialize module exports (assign values)
- [ ] Run module constructors (if any)
- [ ] Maintain module initialization order (dependencies first)
- [ ] Handle initialization errors gracefully

#### Module Namespacing (`raya-core/src/module/namespace.rs`)

- [ ] Create isolated namespace per module
- [ ] Prevent global scope pollution
- [ ] Support for multiple major versions of same package
  ```rust
  pub struct Namespace {
      // name -> major_version -> module
      modules: HashMap<String, HashMap<u32, Arc<Module>>>,
  }

  pub fn get_module(&self, name: &str, version: u32) -> Option<&Arc<Module>>
  ```

---

## Phase 5: Package Metadata & Lockfile

**Goal:** Parse and manage package metadata without compilation.

**Crate:** `raya-pm`

### Tasks

#### raya.toml Parser (`raya-pm/src/manifest.rs`)

- [ ] Parse package descriptor (use `toml` crate)
  ```rust
  use serde::Deserialize;

  #[derive(Debug, Deserialize)]
  pub struct PackageManifest {
      pub package: PackageInfo,
      #[serde(default)]
      pub dependencies: HashMap<String, Dependency>,
      #[serde(default)]
      pub dev_dependencies: HashMap<String, Dependency>,
  }

  #[derive(Debug, Deserialize)]
  pub struct PackageInfo {
      pub name: String,
      pub version: String,
      pub description: Option<String>,
      pub authors: Vec<String>,
      pub license: Option<String>,
  }

  #[derive(Debug, Deserialize)]
  #[serde(untagged)]
  pub enum Dependency {
      Simple(String),  // "^1.2.0"
      Detailed {
          version: Option<String>,
          path: Option<PathBuf>,
          url: Option<String>,
      },
  }
  ```

- [ ] Validate manifest fields
- [ ] Support for local path dependencies
- [ ] Semver version constraints parsing

#### Lockfile Management (`raya-pm/src/lockfile.rs`)

- [ ] Parse raya.lock (TOML format)
  ```rust
  #[derive(Debug, Deserialize, Serialize)]
  pub struct Lockfile {
      pub version: u32,
      pub packages: Vec<LockedPackage>,
  }

  #[derive(Debug, Deserialize, Serialize)]
  pub struct LockedPackage {
      pub name: String,
      pub version: String,
      pub checksum: String,  // hex-encoded SHA-256
      pub source: Source,
  }

  #[derive(Debug, Deserialize, Serialize)]
  #[serde(tag = "type")]
  pub enum Source {
      Registry { url: String },
      Git { url: String, rev: String },
      Path { path: PathBuf },
  }
  ```

- [ ] Generate lockfile from resolved dependencies
- [ ] Update lockfile on dependency changes
- [ ] Lockfile integrity verification

#### Metadata Format (`raya-pm/src/metadata.rs`)

- [ ] Module metadata structure
  ```rust
  #[derive(Debug, Serialize, Deserialize)]
  pub struct ModuleMetadata {
      pub name: String,
      pub version: String,
      pub checksum: String,
      pub dependencies: HashMap<String, String>,
  }
  ```

- [ ] Store/load metadata.json in cache
- [ ] Validate metadata against module header

---

## Phase 6: Semver & Dependency Resolution

**Goal:** Resolve package versions according to semver constraints.

**Crate:** `raya-pm`

### Tasks

#### Semver Parser (`raya-pm/src/semver.rs`)

- [ ] Parse version strings (`1.2.3`, `^1.2.0`, `~1.5.0`)
  ```rust
  pub enum Constraint {
      Exact(Version),
      Caret(Version),  // ^1.2.0 -> >=1.2.0 <2.0.0
      Tilde(Version),  // ~1.2.0 -> >=1.2.0 <1.3.0
      Range(VersionReq),
  }
  ```

- [ ] Implement version constraint matching
  ```rust
  pub fn matches(constraint: &Constraint, version: &Version) -> bool
  ```

- [ ] Version ordering and comparison (implement Ord)

#### Dependency Resolver (`raya-pm/src/resolver.rs`)

- [ ] Build dependency tree from manifest
  ```rust
  pub struct DependencyResolver {
      manifest: PackageManifest,
      lockfile: Option<Lockfile>,
  }

  pub fn resolve(&self) -> Result<ResolvedDependencies, ResolverError>
  ```

- [ ] Resolve version constraints (pick latest compatible)
- [ ] Detect version conflicts
- [ ] Default: allow multiple major versions
- [ ] Optional: strict mode (fail on any conflict)
- [ ] Generate resolved dependency list for lockfile

#### Conflict Resolution (`raya-pm/src/conflict.rs`)

- [ ] Handle major version conflicts (bundle both versions)
  ```rust
  pub enum ConflictStrategy {
      Relaxed,  // Allow multiple majors
      Strict,   // Fail on any conflict
  }
  ```

- [ ] Handle minor/patch conflicts (pick latest)
- [ ] User override support (`[resolution.override]`)
- [ ] Conflict resolution strategies implementation

---

## Phase 7: Local Path Dependencies

**Goal:** Support local filesystem packages (for monorepos).

**Crate:** `raya-pm`, `raya-core`

### Tasks

#### Path Resolution (`raya-pm/src/path.rs`)

- [ ] Resolve relative paths from manifest
  ```rust
  pub fn resolve_path_dep(
      path: &str,
      project_root: &Path
  ) -> Result<PathBuf, PathError>
  ```

- [ ] Validate path dependency has raya.toml
- [ ] Watch for changes in path dependencies (hot reload)
- [ ] Path normalization (cross-platform)

#### Local Module Loading (`raya-core/src/module/local.rs`)

- [ ] Load modules directly from filesystem
- [ ] Skip cache for local dependencies
- [ ] Recompile on source changes (watch mode)
- [ ] Dependency graph for local packages

---

## Phase 8: Testing & Error Handling

**Goal:** Comprehensive testing and error reporting.

### Tasks

#### Unit Tests

- [ ] Module loader tests (`tests/module_loader.rs`)
  - [ ] Load valid .rbin module
  - [ ] Reject invalid magic bytes
  - [ ] Verify checksum validation
  - [ ] Parse module header correctly

- [ ] Import resolver tests (`tests/import_resolver.rs`)
  - [ ] Resolve local paths correctly
  - [ ] Resolve package specs
  - [ ] Handle URL imports
  - [ ] Error on invalid paths

- [ ] Cache operations tests (`tests/cache.rs`)
  - [ ] Store and retrieve modules
  - [ ] Verify cache hits/misses
  - [ ] Test LRU eviction
  - [ ] Atomic write operations

- [ ] Semver resolution tests (`tests/semver.rs`)
  - [ ] Parse version constraints
  - [ ] Match constraints correctly
  - [ ] Version ordering

- [ ] Circular dependency detection tests (`tests/circular_deps.rs`)
  - [ ] Detect simple cycles (A → B → A)
  - [ ] Detect complex cycles (A → B → C → A)
  - [ ] Report cycle path correctly

#### Integration Tests

- [ ] Load multi-module projects
  - [ ] Test project with 5+ modules
  - [ ] Verify all modules loaded
  - [ ] Verify execution order

- [ ] Test monorepo with path dependencies
  - [ ] Local path resolution
  - [ ] Mixed local + registry deps

- [ ] Test cache invalidation
  - [ ] Modify module, verify reload
  - [ ] Test checksum change detection

- [ ] Test version conflict resolution
  - [ ] Multiple major versions
  - [ ] Minor/patch auto-resolution
  - [ ] User overrides

#### Error Messages

- [ ] Module not found errors (with suggestions)
  ```rust
  "Module 'loging' not found. Did you mean 'logging'?"
  ```

- [ ] Circular dependency errors (show cycle)
  ```rust
  "Circular dependency detected: A → B → C → A"
  ```

- [ ] Version conflict errors (show constraints)
  ```rust
  "Version conflict for 'http':
   Package A requires: ^1.0.0
   Package B requires: ^2.0.0"
  ```

- [ ] Checksum mismatch errors
  ```rust
  "Checksum mismatch for module 'logging@1.2.3'
   Expected: a3f2b1c4...
   Got:      9f8e7d6c..."
  ```

- [ ] Corrupted cache errors
  ```rust
  "Corrupted cache entry at ~/.raya/cache/a3f2b1.../
   Run 'raya cache clean' to repair"
  ```

---

## Implementation Order

1. **Phase 1:** Module bytecode format & loader (2 weeks)
2. **Phase 2:** Import resolution (1 week)
3. **Phase 3:** Global cache (1 week)
4. **Phase 4:** Module linking (1 week)
5. **Phase 5:** Package metadata (1 week)
6. **Phase 6:** Semver & resolution (1 week)
7. **Phase 7:** Local paths (3 days)
8. **Phase 8:** Testing (1 week)

**Total:** 4-6 weeks

---

## Success Criteria

- [ ] Load and execute .rbin modules from cache
- [ ] Resolve all three import types (local, package, URL)
- [ ] Global cache working with content-addressable storage
- [ ] Semver resolution working (^, ~, exact, range)
- [ ] Local path dependencies working (monorepo support)
- [ ] Circular dependency detection with clear errors
- [ ] >85% test coverage for module system
- [ ] Zero memory leaks in module loading/unloading
- [ ] Clear error messages for common issues

---

## Out of Scope

The following are **NOT** part of this milestone (separate milestones):

- ❌ Compilation (parsing .raya, AST, code generation)
- ❌ Package manager CLI (`raya install`, `raya add`, etc.)
- ❌ Registry client (HTTP downloads, publish)
- ❌ Type checking and type definitions (.rdef generation)
- ❌ Hot module replacement (HMR)
- ❌ Workspaces (multi-package management)

---

## Reference Documents

- **Module System Design:** `design/MODULES.md`
- **Bytecode Format:** `design/OPCODE.md`
- **Package Format:** See `design/MODULES.md` sections:
  - Global Cache Structure
  - Package File Formats
  - Semver Resolution Rules
  - Import Resolution Algorithm

---

## Dependencies

**Rust Crates:**
```toml
[dependencies]
sha2 = "0.10"          # SHA-256 hashing
hex = "0.4"            # Hex encoding
toml = "0.8"           # TOML parsing
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"     # JSON metadata
semver = "1.0"         # Semver parsing
walkdir = "2.4"        # Directory traversal
```

---

**Status:** Ready for implementation after Milestone 1.3 (Value System) is complete.
