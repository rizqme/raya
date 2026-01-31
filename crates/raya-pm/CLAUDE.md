# raya-pm

Package manager library for Raya.

## Overview

This crate provides package management functionality:
- Module caching (content-addressable storage)
- Package manifest parsing (`raya.toml`)
- Lockfile management (`raya.lock`)
- Semver version parsing and constraint matching
- Dependency resolution
- Local path dependency resolution

## Module Structure

```
src/
├── lib.rs       # Crate entry point, re-exports
├── cache/       # Content-addressable module cache
├── manifest.rs  # raya.toml parsing
├── lockfile.rs  # raya.lock parsing
├── semver.rs    # Version parsing and constraints
├── resolver.rs  # Dependency resolution algorithm
└── path.rs      # Path resolution utilities
```

## Key Types

```rust
// Package manifest (raya.toml)
PackageManifest {
    package: PackageInfo,
    dependencies: HashMap<String, Dependency>,
    dev_dependencies: HashMap<String, Dependency>,
}

// Lockfile (raya.lock)
Lockfile {
    packages: Vec<LockedPackage>,
}

// Version constraint
Constraint::parse("^1.2.0") -> Constraint

// Dependency resolver
DependencyResolver::resolve(&manifest) -> ResolvedDependencies
```

## Cache Structure

```
~/.raya/cache/
├── packages/
│   ├── <sha256>/           # Content-addressed storage
│   │   ├── module.ryb      # Compiled bytecode
│   │   └── module.rdef     # Type definitions
│   └── ...
└── metadata.db             # SQLite cache index
```

## Manifest Format (raya.toml)

```toml
[package]
name = "my-app"
version = "1.0.0"
description = "My Raya application"

[dependencies]
logging = "^1.2.0"
utils = { path = "../utils" }

[dev-dependencies]
testing = "^2.0.0"
```

## Lockfile Format (raya.lock)

```toml
[[package]]
name = "logging"
version = "1.2.3"
source = "registry"
checksum = "sha256:abc123..."

[[package]]
name = "utils"
version = "0.0.0"
source = "path:../utils"
```

## Implementation Status

| Feature | Status |
|---------|--------|
| Manifest parsing | Complete |
| Lockfile parsing | Complete |
| Semver parsing | Complete |
| Local path resolution | Complete |
| Module cache | Partial |
| Registry client | Not started |
| Dependency resolution | Partial |

## For AI Assistants

- Version constraint matching uses standard semver rules
- Cache uses SHA-256 for content addressing
- Local path dependencies bypass the cache
- Registry API is not yet implemented
- Resolver handles diamond dependencies with conflict detection
