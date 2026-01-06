# Module System and Package Management

**Status:** Draft
**Version:** 0.1
**Last Updated:** 2026-01-06

---

## Overview

Raya's module system is designed for **simplicity, speed, and efficiency**, taking inspiration from:
- **Bun:** Global cache, fast installation, content-addressable storage
- **Go:** Import by URL, minimal duplication, no central registry lock-in
- **NOT Node.js:** Avoid massive `node_modules` directories and per-project duplication

### Key Principles

1. **Global Cache:** Single source of truth for all packages (~/.raya/cache)
2. **Bytecode-First:** Store compiled `.rbin` files, not source (optional source for debugging)
3. **Content-Addressable:** Packages identified by cryptographic hash
4. **Lockfile-Based:** Reproducible builds with `raya.lock`
5. **Zero Duplication:** Same package version used across all projects
6. **Fast:** Parallel downloads, incremental compilation
7. **Offline-First:** Work without network once packages cached

---

## Module Resolution

### Import Syntax

Raya supports three types of imports:

#### 1. Named Package Imports (Registry)

```typescript
import { Logger } from "logging";              // Latest stable
import { hash } from "crypto@1.2.0";          // Specific version
import { fetch } from "http@^2.0.0";          // Semver range
```

**Resolution:**
1. Check `raya.lock` for exact version
2. If not locked, resolve from registry (raya.dev)
3. Download `.rbin` to global cache
4. Link to project

#### 2. URL Imports (Decentralized)

```typescript
import { utils } from "https://github.com/user/repo/v1.0.0";
import { lib } from "https://pkg.raya.dev/lib@1.0.0";
```

**Resolution:**
1. Fetch from URL
2. Verify checksum (from lockfile or query)
3. Cache in `~/.raya/cache/<hash>/`

#### 3. Local Imports (Relative Paths)

```typescript
import { helper } from "./utils.raya";
import { config } from "../config.raya";
```

**Resolution:**
1. Relative to current file
2. Compile on-the-fly if not cached
3. No package management involved

---

## Global Cache Structure

### Directory Layout

```
~/.raya/
├── cache/                    # Global package cache
│   ├── <sha256-hash>/       # Content-addressable packages
│   │   ├── module.rbin      # Compiled bytecode (required)
│   │   ├── module.rdef      # Type definitions (optional, like .d.ts)
│   │   └── metadata.json    # Package metadata
│   └── <sha256-hash>/
│       └── module.rbin
├── registry/                 # Registry index cache
│   └── index.json           # Package name → versions mapping
├── tmp/                      # Temporary downloads
└── config.toml              # Global config (registry URL, etc.)
```

### Example: Package Storage

```
~/.raya/cache/a3f2b1.../
├── module.rbin              # Compiled logging@1.2.3 (required)
├── module.rdef              # Type definitions (optional)
└── metadata.json            # { name, version, dependencies, checksum }
```

**Why content-addressable?**
- Same package (same hash) stored only once
- Works across all versions and registries
- Automatic deduplication
- Integrity verification built-in

---

## Package Format

### Package Descriptor (raya.toml)

```toml
[package]
name = "logging"
version = "1.2.3"
description = "Fast structured logging library"
authors = ["Jane Doe <jane@example.com>"]
license = "MIT"
repository = "https://github.com/rayalang/logging"

# Entry point (main module)
main = "src/index.raya"

# Dependencies (package name → version constraint)
[dependencies]
"time" = "^2.0.0"
"fmt" = "1.3.0"

# Dev dependencies (only for tests)
[dev-dependencies]
"testing" = "0.5.0"

# Optional: Native modules
[native]
crypto = "native:crypto"  # Requires native:crypto to be installed

# Build configuration
[build]
target = "bytecode"       # Output format
optimize = true           # Enable optimizations
```

### Lockfile (raya.lock)

```toml
# This file is auto-generated. Do not edit manually.
version = 1

[[package]]
name = "logging"
version = "1.2.3"
checksum = "sha256:a3f2b1c4d5e6..."
source = "registry+https://raya.dev"

[[package]]
name = "time"
version = "2.1.0"
checksum = "sha256:9f8e7d6c5b4a..."
source = "registry+https://raya.dev"

[[package]]
name = "fmt"
version = "1.3.0"
checksum = "sha256:3b2a1f9e8d7c..."
source = "git+https://github.com/user/fmt#v1.3.0"
```

### Package File Formats

Published packages contain **compiled bytecode only** (not source code). This ensures:
- **Fast loading:** No compilation needed at install time
- **Smaller downloads:** Bytecode is more compact than source
- **IP protection:** Source code not exposed (optional)

#### `.rbin` - Compiled Bytecode (Required)

**Format:** Raya bytecode format (see [OPCODE.md](OPCODE.md))

**Contents:**
- Compiled functions and classes
- Constant pool (strings, numbers)
- Module metadata (exports, dependencies)
- Optimized for VM execution

**Generation:**
```bash
# During package build
raya build --release
# Produces: dist/module.rbin
```

**Properties:**
- Platform-independent (runs on any architecture with Raya VM)
- Includes all necessary runtime information
- No separate compilation step needed by consumers

#### `.rdef` - Type Definitions (Optional)

**Format:** TypeScript-like declaration file (similar to `.d.ts`)

**Purpose:**
- Provide type information for IDE autocomplete
- Enable static type checking for package users
- Document public API

**Example:**

```typescript
// module.rdef (for "logging" package)

/**
 * Structured logger with multiple output levels
 */
export class Logger {
    constructor(name: string);

    /**
     * Log informational message
     */
    info(message: string): void;

    /**
     * Log error message
     */
    error(message: string): void;
}

/**
 * Create default logger instance
 */
export function createLogger(name: string): Logger;
```

**Generation:**
```bash
# Auto-generate from source
raya build --emit-rdef

# Or write manually
# dist/module.rdef
```

**When to include `.rdef`:**
- ✅ Public libraries (improve developer experience)
- ✅ Complex APIs (documentation + type safety)
- ❌ Private internal packages (not necessary)
- ❌ Simple utilities (types inferable from usage)

#### `.raya` Source Files (NOT in Published Packages)

**Important:** Published packages do **NOT** include `.raya` source files by default.

**Source files are only present during:**
- Local development
- Building the package
- Debugging with `--include-source` flag

**Why exclude source?**
- Reduces package size
- Protects intellectual property
- Prevents source/bytecode inconsistencies
- Faster installation (no compilation needed)

**To include source (optional):**

```toml
# raya.toml
[package]
include-source = true  # Include .raya files in published package
```

**Use cases for including source:**
- Open-source projects (transparency)
- Educational packages (learning resource)
- Debugging support (source maps)

#### File Size Comparison

Example: `logging` package

| Format | Size | Included |
|--------|------|----------|
| `.raya` source | 15 KB | ❌ No (by default) |
| `.rbin` bytecode | 8 KB | ✅ Always |
| `.rdef` types | 2 KB | ✅ Recommended |
| **Total download** | **10 KB** | |

Compare to Node.js (source + dependencies): typically 50-500 KB per package.

#### Registry Download Format

Packages are downloaded as **single compressed archive:**

```bash
# Registry provides:
GET /packages/logging/1.2.3/download
→ Returns: logging-1.2.3.tar.zst (Zstandard-compressed tar)

# Archive contents:
logging-1.2.3.tar.zst
├── module.rbin      # Required
├── module.rdef      # Optional
└── metadata.json    # Required
```

**Compression:** Zstandard (`.zst`) for fast decompression and high compression ratio.

---

## Package Manager Commands

### Installation

```bash
# Install dependencies from raya.toml
raya install

# Add a package
raya add logging@1.2.3

# Add dev dependency
raya add --dev testing

# Install specific version
raya install logging@1.0.0

# Install from URL
raya add https://github.com/user/repo/v1.0.0
```

**Process:**
1. Parse `raya.toml`
2. Resolve dependency tree
3. Check `~/.raya/cache/` for existing packages
4. Download missing packages (parallel)
5. Compile `.raya` → `.rbin` (if not cached)
6. Update `raya.lock`

### Project Structure

```
my-project/
├── raya.toml           # Package descriptor
├── raya.lock           # Dependency lockfile (auto-generated)
├── src/
│   ├── main.raya      # Entry point
│   └── utils.raya
└── .raya/              # Project-local cache (symlinks to global)
    └── cache@ -> ~/.raya/cache/  # Symlink to global cache
```

**No node_modules!** All packages live in `~/.raya/cache/`.

---

## Dependency Resolution

### Algorithm

1. **Parse dependencies:**
   - Read `raya.toml` → extract dependencies
   - Check `raya.lock` for locked versions

2. **Resolve versions:**
   - For each dependency, resolve semver constraint
   - Build dependency graph
   - Detect conflicts (different major versions of same package)

3. **Fetch packages:**
   - For each resolved package:
     - Check global cache (`~/.raya/cache/<hash>/`)
     - If missing, download from registry/URL
     - Verify checksum

4. **Compile:**
   - If `.rbin` not in cache:
     - Compile `.raya` source → `.rbin`
     - Store in cache with hash

5. **Generate lockfile:**
   - Write exact versions and checksums to `raya.lock`

### Semver Resolution Rules

Raya follows **semantic versioning** (MAJOR.MINOR.PATCH) with the following constraint operators:

#### Version Constraint Operators

```toml
[dependencies]
"http" = "1.2.3"      # Exact version (1.2.3 only)
"fmt" = "^1.2.3"      # Compatible (>=1.2.3, <2.0.0) - allows minor/patch updates
"time" = "~1.2.3"     # Patch-level (>=1.2.3, <1.3.0) - allows patch updates only
"logging" = ">=1.0.0" # Greater than or equal to
"utils" = "*"         # Latest stable (not recommended for production)
```

#### Resolution Algorithm

**For each dependency constraint:**

1. **Exact version (`1.2.3`):**
   - Resolve to exactly `1.2.3`
   - No flexibility

2. **Caret constraint (`^1.2.3`):**
   - Allow minor and patch updates within same major version
   - Resolve to latest: `>=1.2.3` AND `<2.0.0`
   - Example: `^1.2.3` can resolve to `1.9.5` but not `2.0.0`

3. **Tilde constraint (`~1.2.3`):**
   - Allow only patch updates within same minor version
   - Resolve to latest: `>=1.2.3` AND `<1.3.0`
   - Example: `~1.2.3` can resolve to `1.2.9` but not `1.3.0`

4. **Range constraint (`>=1.0.0`):**
   - Resolve to latest version matching constraint

#### Lockfile Behavior

Once resolved, versions are **pinned in `raya.lock`:**

```toml
[[package]]
name = "http"
version = "1.2.7"        # Exact resolved version
checksum = "sha256:..."
source = "registry+https://raya.dev"
```

**On subsequent `raya install`:** Use locked version (ignore semver constraints).

**To update dependencies:** Run `raya update` to re-resolve constraints.

### Local Module Resolution

For **private/internal dependencies** (not published to registry), use local path references:

```toml
[dependencies]
"shared-utils" = { path = "../shared/utils" }
"internal-lib" = { path = "../../libs/internal-lib" }
```

**Resolution:**
1. Path is relative to project root (where `raya.toml` lives)
2. Target directory must contain valid `raya.toml`
3. Local packages are compiled on-the-fly
4. No version constraints (always use latest code)
5. Changes reflected immediately (no cache staleness)

**Use cases:**
- Monorepo/workspace setups
- Internal company libraries
- Development before publishing
- Private projects not suitable for public registry

**Example monorepo structure:**

```
my-monorepo/
├── packages/
│   ├── shared-utils/
│   │   ├── raya.toml
│   │   └── src/index.raya
│   ├── core-api/
│   │   ├── raya.toml    # depends on shared-utils via path
│   │   └── src/index.raya
│   └── web-app/
│       ├── raya.toml    # depends on both via path
│       └── src/index.raya
```

```toml
# packages/core-api/raya.toml
[dependencies]
"shared-utils" = { path = "../shared-utils" }
"logging" = "^1.2.0"  # Registry package
```

### Conflict Resolution Policy

**Goal:** Minimize bloat while ensuring compatibility.

#### Default Behavior (Relaxed)

**MAJOR version conflicts:**
- ✅ **Allow multiple major versions** (different major = breaking changes)
- Each dependent gets its requested major version
- Example: Package A uses `http@1.x`, Package B uses `http@2.x` → both bundled

**MINOR/PATCH version conflicts:**
- ✅ **Resolve to single version** (pick latest compatible)
- Assumes semver compatibility within same major version
- Example: Package A uses `^1.2.0`, Package B uses `^1.5.0` → resolve to latest `1.x.x`

#### Strict Mode (Enforced)

Enable strict conflict checking in `raya.toml`:

```toml
[package]
name = "my-app"
version = "1.0.0"

[resolution]
conflict-strategy = "strict"  # Fail on any conflict
```

**Strict mode behavior:**
- ❌ **Fail on ANY version conflict** (even minor/patch)
- User must explicitly resolve conflicts with overrides

**Override resolution:**

```toml
[resolution.override]
"http" = "1.5.3"  # Force all dependents to use 1.5.3
```

#### Example Conflict Scenarios

**Scenario 1: Major version conflict (allowed by default)**

```toml
# Package A depends on http@1.5.0
# Package B depends on http@2.1.0
```

**Resolution:**
- Bundle both `http@1.5.0` and `http@2.1.0`
- Binary size increases, but no compatibility issues
- Each package uses its required version

**Scenario 2: Minor version conflict (auto-resolved by default)**

```toml
# Package A depends on fmt@^1.2.0
# Package B depends on fmt@^1.5.0
```

**Resolution:**
- Resolve to latest `1.x.x` (e.g., `1.7.2`)
- Both packages use same version (semver guarantees compatibility)
- If semver is violated (breaking change in minor), build may fail at runtime

**Scenario 3: User-enforced resolution**

```toml
[resolution.override]
"fmt" = "1.5.0"  # Pin specific version for all
```

**Resolution:**
- All dependents use `fmt@1.5.0` regardless of constraints
- User takes responsibility for compatibility

#### Trade-offs

| Strategy | Binary Size | Compatibility | Build Speed |
|----------|-------------|---------------|-------------|
| **Relaxed (default)** | Larger (multiple majors) | High (semver-based) | Fast |
| **Strict** | Smaller (single version) | Requires manual verification | Slower (conflict resolution) |

---

## Compilation Pipeline

### Source to Bytecode

```
user-code.raya
    ↓
[Parser] → AST
    ↓
[Type Checker] → Typed AST
    ↓
[Compiler] → Bytecode
    ↓
module.rbin (stored in cache)
```

### Incremental Compilation

- **Cache key:** SHA-256 of source + compiler version
- **Cache hit:** Reuse existing `.rbin`
- **Cache miss:** Recompile and store

### Ahead-of-Time (AOT) Compilation

```bash
# Compile all dependencies
raya build

# Produces:
# - Single .rbin bundle (all dependencies)
# - Or separate .rbin per module (lazy loading)
```

---

## Registry Protocol

### Package Registry (raya.dev)

**API Endpoints:**

```
GET /packages/{name}
→ Returns: Package metadata (all versions)

GET /packages/{name}/{version}
→ Returns: Specific version metadata + download URL

GET /packages/{name}/{version}/download
→ Returns: .rbin file (compiled bytecode)

POST /publish
→ Publishes a new package version
```

### Example: Fetch Package

```bash
# 1. Query registry
curl https://raya.dev/packages/logging/1.2.3

# Response:
{
  "name": "logging",
  "version": "1.2.3",
  "checksum": "sha256:a3f2b1...",
  "download_url": "https://cdn.raya.dev/logging-1.2.3.rbin",
  "dependencies": {
    "time": "^2.0.0"
  }
}

# 2. Download .rbin
curl https://cdn.raya.dev/logging-1.2.3.rbin -o ~/.raya/cache/a3f2b1.../module.rbin

# 3. Verify checksum
sha256sum ~/.raya/cache/a3f2b1.../module.rbin
```

---

## Comparison with Other Systems

| Feature | Raya | Bun | Go | Node.js |
|---------|------|-----|----|----|
| **Global Cache** | ✅ ~/.raya/cache | ✅ ~/.bun/cache | ✅ GOPATH/pkg | ❌ Per-project |
| **Storage Format** | .rbin (bytecode) | Source + cache | Source | Source |
| **Deduplication** | ✅ Content-hash | ✅ Content-hash | ✅ Version-based | ❌ Duplicates |
| **Lockfile** | ✅ raya.lock | ✅ bun.lockb | ✅ go.sum | ✅ package-lock.json |
| **URL Imports** | ✅ Yes | ✅ Yes | ✅ Yes | ❌ No |
| **Registry** | ✅ Optional | ✅ NPM compatible | ✅ Optional | ✅ Required (NPM) |
| **Offline Mode** | ✅ Yes | ✅ Yes | ✅ Yes | ⚠️ Limited |
| **Speed** | 🚀 Fast (AOT) | 🚀 Fast | 🐢 Moderate | 🐢 Slow |

**Key Advantages:**
- **No node_modules bloat:** Global cache means zero per-project duplication
- **Bytecode storage:** Pre-compiled packages load instantly
- **Content-addressable:** Automatic deduplication across versions
- **Offline-first:** Once cached, works without network
- **Decentralized:** Can import from any URL, not locked to one registry

---

## Import Resolution Algorithm

### Pseudocode

```rust
fn resolve_import(import_path: &str, current_file: &Path, project_root: &Path) -> Result<Module, Error> {
    match import_path {
        // Local import: "./foo.raya" or "../bar.raya"
        path if path.starts_with("./") || path.starts_with("../") => {
            let resolved = current_file.parent().join(path);
            compile_and_load(resolved)
        }

        // URL import: "https://..."
        url if url.starts_with("http://") || url.starts_with("https://") => {
            let hash = fetch_and_cache(url)?;
            load_from_cache(hash)
        }

        // Package import: "logging" or "logging@1.2.3"
        package_spec => {
            let (name, version) = parse_package_spec(package_spec);

            // 1. Check if it's a local path dependency in raya.toml
            if let Some(local_dep) = project_dependencies.get(name) {
                if let DependencySource::Path(path) = local_dep.source {
                    // Resolve local package
                    let local_path = project_root.join(path);
                    let local_toml = local_path.join("raya.toml");

                    if !local_toml.exists() {
                        return Err(Error::InvalidLocalPackage(name));
                    }

                    // Compile local package on-the-fly
                    return compile_local_package(local_path);
                }
            }

            // 2. Check lockfile
            if let Some(locked) = lockfile.get(name) {
                return load_from_cache(locked.checksum);
            }

            // 3. Resolve version from registry
            let resolved = registry.resolve(name, version)?;

            // 4. Check cache
            if let Some(cached) = cache.get(resolved.checksum) {
                return Ok(cached);
            }

            // 5. Download and store in cache
            let bytecode = download(resolved.download_url)?;
            verify_checksum(&bytecode, resolved.checksum)?;
            cache.store(resolved.checksum, bytecode)?;

            Ok(bytecode)
        }
    }
}

/// Compile local package from filesystem path
fn compile_local_package(package_path: &Path) -> Result<Module, Error> {
    // 1. Read raya.toml
    let toml = read_package_toml(package_path.join("raya.toml"))?;

    // 2. Find entry point
    let entry_point = package_path.join(toml.main);

    // 3. Compile (respecting cache)
    let cache_key = compute_cache_key(&entry_point)?;

    if let Some(cached) = local_cache.get(cache_key) {
        return Ok(cached);
    }

    // 4. Fresh compilation
    let ast = parse(entry_point)?;
    let typed_ast = type_check(ast)?;
    let bytecode = compile(typed_ast)?;

    // 5. Cache for next time
    local_cache.store(cache_key, bytecode.clone())?;

    Ok(bytecode)
}
```

---

## Publishing Workflow

### Prepare Package

```bash
# 1. Create raya.toml
cat > raya.toml <<EOF
[package]
name = "my-package"
version = "1.0.0"
main = "src/index.raya"
EOF

# 2. Write code
mkdir src
cat > src/index.raya <<EOF
export function hello(name: string): string {
    return "Hello, " + name + "!";
}
EOF

# 3. Test locally
raya test

# 4. Build release
raya build --release
```

### Publish to Registry

```bash
# Authenticate (first time only)
raya login

# Publish package
raya publish

# Process:
# 1. Compile .raya → .rbin
# 2. Generate checksum (SHA-256)
# 3. Upload .rbin to CDN
# 4. Register package in index
```

### Versioning

```bash
# Bump version and publish
raya publish --patch   # 1.0.0 → 1.0.1
raya publish --minor   # 1.0.1 → 1.1.0
raya publish --major   # 1.1.0 → 2.0.0

# Pre-release versions
raya publish --tag beta  # 1.0.0-beta.1
```

---

## Security Considerations

### Checksum Verification

Every package download is verified:

```rust
fn verify_package(data: &[u8], expected_checksum: &str) -> Result<(), Error> {
    let actual = sha256(data);
    if actual != expected_checksum {
        return Err(Error::ChecksumMismatch);
    }
    Ok(())
}
```

### Code Signing (Optional)

```toml
[package]
signature = "ed25519:a1b2c3d4..."
public_key = "https://keybase.io/user/raya.pub"
```

### Sandboxing

Native modules run with restricted capabilities (see NATIVE_BINDINGS.md).

---

## Configuration

### Global Config (~/.raya/config.toml)

```toml
[registry]
default = "https://raya.dev"
mirrors = [
    "https://mirror1.raya.dev",
    "https://mirror2.raya.dev"
]

[cache]
dir = "~/.raya/cache"
max_size = "10GB"

[build]
parallel = true
jobs = 8  # Parallel compilation workers

[network]
timeout = 30  # seconds
retry = 3
```

---

## Implementation Plan (Milestone 1.14)

### Phase 1: Core Module System
- [ ] Import resolution (local, package, URL)
- [ ] Module loading from bytecode
- [ ] Dependency graph construction
- [ ] Circular dependency detection

### Phase 2: Package Format
- [ ] raya.toml parser
- [ ] Package metadata structure
- [ ] Lockfile (raya.lock) generation
- [ ] Checksum computation (SHA-256)

### Phase 3: Global Cache
- [ ] Cache directory structure
- [ ] Content-addressable storage
- [ ] Cache lookup and retrieval
- [ ] Cache cleanup (LRU eviction)

### Phase 4: Package Manager CLI
- [ ] `raya install` - Install dependencies
- [ ] `raya add <package>` - Add dependency
- [ ] `raya remove <package>` - Remove dependency
- [ ] `raya update` - Update dependencies
- [ ] `raya publish` - Publish package

### Phase 5: Registry Client
- [ ] HTTP client for registry API
- [ ] Package search and resolution
- [ ] Parallel downloads
- [ ] Retry logic and error handling

### Phase 6: Compilation Integration
- [ ] Compile .raya to .rbin
- [ ] Incremental compilation
- [ ] Cache compiled artifacts
- [ ] Bundle generation

---

## Examples

### Simple Package

```
my-logger/
├── raya.toml
└── src/
    └── index.raya
```

```typescript
// src/index.raya
export class Logger {
    constructor(private name: string) {}

    info(message: string): void {
        console.log(`[${this.name}] INFO: ${message}`);
    }

    error(message: string): void {
        console.error(`[${this.name}] ERROR: ${message}`);
    }
}
```

### Using the Package

```typescript
// user-code.raya
import { Logger } from "my-logger";

const logger = new Logger("MyApp");
logger.info("Application started");
```

### Multi-Module Package

```
web-framework/
├── raya.toml
└── src/
    ├── index.raya       # Main entry
    ├── router.raya      # Submodule
    └── middleware.raya  # Submodule
```

```typescript
// src/index.raya
export { Router } from "./router.raya";
export { Middleware } from "./middleware.raya";

// user-code.raya
import { Router, Middleware } from "web-framework";
```

---

## Future Enhancements

1. **Workspaces:** Monorepo support with shared dependencies
2. **Private Registries:** Self-hosted package registry
3. **Binary Packages:** Native .so/.dylib/.dll distribution
4. **Hot Reloading:** Module updates without restart
5. **CDN Integration:** Edge-cached package delivery

---

## References

- Bun: https://bun.sh/docs/cli/install
- Go Modules: https://go.dev/ref/mod
- Deno: https://deno.land/manual/linking_to_external_code
- Cargo (Rust): https://doc.rust-lang.org/cargo/

---

**Status:** Ready for implementation (Milestone 1.14)
