# compiler/module - Multi-Module Compilation

Module compilation system for multi-file Raya projects.

## Overview

Provides infrastructure for compiling multiple Raya source files with import resolution:
- Local import resolution (`./path`, `../path`)
- Module dependency graph construction
- Circular dependency detection
- Topological ordering for compilation
- In-memory module caching

## Module Structure

```
module/
├── mod.rs          # Entry point, re-exports
├── resolver.rs     # Path resolution logic
├── graph.rs        # Dependency graph
├── cache.rs        # Module cache
├── exports.rs      # Export tracking for cross-module resolution
└── compiler.rs     # ModuleCompiler orchestrator
```

## Key Types

### ModuleResolver
```rust
pub struct ModuleResolver {
    project_root: PathBuf,
}

// Resolve import specifiers to absolute paths
resolver.resolve("./utils", &from_file) -> Result<ResolvedModule, ResolveError>
```

Resolution order for `import { x } from "./utils"`:
1. Try `./utils.raya`
2. Try `./utils/index.raya`

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
| Package imports | ❌ Phase 4 |
| URL imports | ❌ Phase 6 |
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
- Exported symbols are tracked in `ExportRegistry`
- Imports are injected into the binder before binding
- TypeIDs don't transfer between TypeContexts (function/class call limitation)
- Package imports (`"logging"`) return `PackageNotSupported` error
- URL imports (`"https://..."`) return `UrlNotSupported` error
- Cache uses mtime for invalidation (detects source file changes)
- Graph uses topological sort (Kahn's algorithm) for compilation order
