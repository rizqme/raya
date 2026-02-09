# compiler/module - Multi-Module Compilation

Module compilation system for multi-file Raya projects.

## Overview

Provides infrastructure for compiling multiple Raya source files with import resolution:
- Local import resolution (`./path`, `../path`)
- Package import resolution (`"logging"`, `"logging@1.2.0"`)
- Module dependency graph construction
- Circular dependency detection
- Topological ordering for compilation
- In-memory module caching
- Global cache lookup (`~/.raya/cache/`)

## Module Structure

```
module/
├── mod.rs          # Entry point, re-exports
├── resolver.rs     # Path resolution logic
├── graph.rs        # Dependency graph
├── cache.rs        # Module cache
├── exports.rs      # Export tracking for cross-module resolution
├── compiler.rs     # ModuleCompiler orchestrator
├── typedef.rs      # Type definition (.d.raya) parsing
└── std_modules.rs  # std: module registry (embedded sources)
```

## Key Types

### ModuleResolver
```rust
pub struct ModuleResolver {
    project_root: PathBuf,
    package_resolver: Option<PackageResolverConfig>,
}

// Resolve import specifiers to absolute paths
resolver.resolve("./utils", &from_file) -> Result<ResolvedModule, ResolveError>
resolver.resolve("logging", &from_file) -> Result<ResolvedModule, ResolveError>
```

**Local resolution** for `import { x } from "./utils"`:
1. Try `./utils.raya`
2. Try `./utils/index.raya`

**Package resolution** for `import { x } from "logging"`:
1. Check `raya.toml` for dependency
2. If path dependency, compile from source
3. Otherwise, check `raya.lock` for version
4. Look up in `~/.raya/cache/<checksum>/`

**Cached package structure:**
```
~/.raya/cache/<checksum>/
├── module.ryb       # Required - compiled bytecode
├── module.d.raya    # Required - type definitions with doc comments
├── raya.toml        # Required - package manifest
└── README.md        # Optional - package documentation
```

### PackageSpecifier
```rust
// Parse package specifiers
PackageSpecifier::parse("logging")           // name only
PackageSpecifier::parse("logging@1.2.0")     // exact version
PackageSpecifier::parse("logging@^1.0.0")    // semver constraint
PackageSpecifier::parse("@org/pkg@1.0.0")    // scoped package
```

### ModuleGraph
```rust
pub struct ModuleGraph {
    nodes: HashMap<PathBuf, ModuleNode>,
    entry_points: HashSet<PathBuf>,
}

// Dependency tracking
graph.add_module(path)
graph.add_dependency(from, to)

// Cycle detection
graph.detect_cycles() -> Result<(), GraphError>

// Compilation order (dependencies first)
graph.topological_order() -> Result<Vec<PathBuf>, GraphError>
```

### ModuleCache
```rust
pub struct ModuleCache {
    bytecode_cache: HashMap<PathBuf, CachedModule>,
}

// Cache compiled modules
cache.insert(path, bytecode)
cache.get(&path) -> Option<&CachedModule>  // Checks mtime for invalidation
cache.invalidate(&path)
```

### ModuleCompiler
```rust
pub struct ModuleCompiler {
    resolver: ModuleResolver,
    graph: ModuleGraph,
    cache: ModuleCache,
}

// Compile entry point and all dependencies
compiler.compile(&entry_point) -> Result<Vec<CompiledModule>, ModuleCompileError>
```

## Usage Example

```rust
use raya_engine::compiler::{ModuleCompiler, ModuleCompileError};
use std::path::PathBuf;

let mut compiler = ModuleCompiler::new(PathBuf::from("/project"));
let compiled = compiler.compile(&PathBuf::from("/project/src/main.raya"))?;

// Modules returned in dependency order (dependencies first)
for module in compiled {
    println!("Compiled: {} ({} bytes)",
        module.path.display(),
        module.bytecode.functions.len()
    );
}
```

## Implementation Status

| Feature | Status |
|---------|--------|
| Local import resolution | ✅ Complete |
| Index file support (`./lib` → `./lib/index.raya`) | ✅ Complete |
| Dependency graph | ✅ Complete |
| Cycle detection | ✅ Complete |
| Topological ordering | ✅ Complete |
| Module caching | ✅ Complete |
| Stale cache detection (mtime) | ✅ Complete |
| Cross-module variable imports | ✅ Complete |
| Export tracking | ✅ Complete |
| Aliased imports (`as`) | ✅ Complete |
| Cross-module function/class calls | ⚠️ Partial |
| Package specifier parsing | ✅ Complete |
| Path dependency resolution | ✅ Complete |
| raya.toml/raya.lock lookup | ✅ Complete |
| Global cache lookup | ✅ Complete |
| .d.raya type definitions | ✅ Complete |
| URL imports (via lockfile) | ✅ Complete |
| Path dependency mapping | ✅ Complete |
| `std:` module registry | ✅ Complete (M4.2) |
| `export default` support | ✅ Complete (M4.2) |
| Bytecode linking | ❌ Future |

## Phase 3: Cross-Module Symbol Resolution

**What works:**
```typescript
// utils.raya
export let value: number = 42;

// main.raya
import { value } from "./utils";
let x: number = value + 1;  // ✅ Works!
```

**What doesn't work (known limitation):**
```typescript
// utils.raya
export function add(a: number, b: number): number { return a + b; }

// main.raya
import { add } from "./utils";
let result = add(1, 2);  // ❌ Type error: function type mismatch
                         // (TypeIDs are module-local, not shared)
```

This limitation exists because TypeIDs are local to each module's TypeContext. A full solution requires either:
1. TypeContext merging/sharing between modules
2. Type reconstruction when importing

## For AI Assistants

- Phase 3 adds cross-module symbol resolution for variables
- Phase 4 adds package import resolution via `raya.toml`/`raya.lock`
- Exported symbols are tracked in `ExportRegistry`
- Imports are injected into the binder before binding
- TypeIDs don't transfer between TypeContexts (function/class call limitation)
- Package imports require `raya.toml` in project root
- Path dependencies (`{ path = "./lib" }`) compile from source
- Registry dependencies require `raya.lock` and cached `.ryb` files
- URL imports (`"https://..."`) require entry in `raya.lock` with cached checksum
- Cache uses mtime for invalidation (detects source file changes)
- Graph uses topological sort (Kahn's algorithm) for compilation order
- `std:` modules (e.g., `std:logger`) are resolved via `std_modules.rs` with embedded sources
- `export default` is supported for default imports (`import logger from "std:logger"`)
