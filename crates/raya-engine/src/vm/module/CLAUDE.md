# module module

Module loading, linking, and import resolution for the VM.

## Overview

Handles loading bytecode modules and resolving imports between modules. Supports local files, packages, and URL imports.

## Module Structure

```
module/
├── mod.rs      # Re-exports
├── import.rs   # Import resolution
├── linker.rs   # Module linking
└── graph.rs    # Dependency graph (future)
```

## Import Types

### Local Imports
```typescript
import { foo } from "./utils";      // Relative to current file
import { bar } from "../lib/helper"; // Parent directory
```

### Package Imports
```typescript
import { Logger } from "logging";           // Latest version
import { Logger } from "logging@1.2.3";     // Exact version
import { Logger } from "@org/package@^2.0"; // Scoped + semver
```

### URL Imports
```typescript
import { utils } from "https://pkg.raya.dev/lib@1.0.0";
```

## Key Types

### ImportSpec
```rust
pub enum ImportSpec {
    Local(PathBuf),
    Package { name: String, version: Option<String> },
    Url(String),
}
```

### ImportResolver
```rust
pub struct ImportResolver {
    project_root: PathBuf,
}

resolver.parse_specifier(spec) -> Result<ImportSpec, ImportError>
resolver.resolve_local(path, current_file) -> Result<PathBuf, ImportError>
resolver.resolve_package(name, version) -> Result<PathBuf, ImportError>
resolver.resolve_url(url) -> Result<PathBuf, ImportError>
```

### ModuleLinker
```rust
pub struct ModuleLinker {
    loaded_modules: HashMap<PathBuf, Module>,
}

linker.link(main_module, imports) -> Result<LinkedModule, LinkError>
```

## Resolution Algorithm

### Local Import
```
import { foo } from "./utils"
    │
    ▼
1. Resolve relative to current file directory
2. Try: ./utils.raya
3. Try: ./utils/index.raya
4. Parse and compile
5. Return module
```

### Package Import
```
import { Logger } from "logging@1.2.3"
    │
    ▼
1. Check raya.lock for exact version
2. Look up in ~/.raya/cache/<hash>/
   ~/.raya/cache/<hash>/
   ├── module.ryb       # Required
   ├── module.d.raya    # Required
   ├── raya.toml        # Required
   └── README.md        # Optional
3. Load module.ryb (bytecode)
4. Load module.d.raya (types)
5. Return module
```

### URL Import
```
import { utils } from "https://..."
    │
    ▼
1. Check raya.lock for cached hash
2. If not cached:
   a. Fetch from URL
   b. Compute SHA-256
   c. Store in ~/.raya/cache/<hash>/
   d. Update raya.lock
3. Load from cache
```

## Import Errors

```rust
pub enum ImportError {
    InvalidSpecifier(String),
    FileNotFound(PathBuf),
    PackageNotFound(String),
    InvalidUrl(String),
    PathResolution(String),
    CircularImport(Vec<PathBuf>),
}
```

## Module Graph (Planned)

```rust
pub struct DependencyGraph {
    nodes: HashMap<PathBuf, ModuleNode>,
    edges: Vec<(PathBuf, PathBuf)>,  // (importer, imported)
}

graph.add_module(path, module)
graph.add_dependency(from, to)
graph.detect_cycles() -> Option<Vec<PathBuf>>
graph.topological_sort() -> Vec<PathBuf>
```

## Implementation Status

| Feature | Status |
|---------|--------|
| Local import parsing | ✅ Complete |
| Package import parsing | ✅ Complete |
| URL import parsing | ✅ Complete |
| Local file resolution | ✅ Complete |
| Package cache lookup | ✅ Complete |
| URL caching (via lockfile) | ✅ Complete |
| Path dependency mapping | ✅ Complete |
| Module linking | ⚠️ Partial |
| Cycle detection | ✅ Complete (in compiler/module) |

## For AI Assistants

- Import specifiers are parsed at compile time
- Module resolution happens at link/load time
- Local imports use `.raya` extension (auto-added if missing)
- Package imports require `raya.lock` for reproducibility
- URL imports are cached by content hash
- Circular imports should be detected and reported
- Linked modules share a unified constant pool
